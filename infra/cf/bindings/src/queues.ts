import { DEFAULT_SCHEDULER_AUTH_TOKEN, TEXT_JSON, TOPICS } from "./constants";
import { body, json } from "./http";
import type { BoundFetcher, QueueMessageBody, TalonCfBindingsEnv } from "./types";

type QueuePayload = {
  topic: string;
  payloadBase64: string;
};

type StreamSubscriber = {
  writer: WritableStreamDefaultWriter<Uint8Array>;
};

const STREAM_HEADERS = {
  "content-type": "application/x-ndjson; charset=utf-8",
  "cache-control": "no-store, no-transform",
};
const encoder = new TextEncoder();

function queueForTopic(env: TalonCfBindingsEnv, topic: string): { queue: Queue; eventType: string } | null {
  if (topic === TOPICS.sessionDispatch) {
    return { queue: env.SESSION_DISPATCH_QUEUE, eventType: "session_dispatch" };
  }
  if (topic === TOPICS.resourceLifecycle) {
    return { queue: env.RESOURCE_LIFECYCLE_QUEUE, eventType: "resource_lifecycle" };
  }
  if (topic === TOPICS.sessionControl) {
    return { queue: env.SESSION_CONTROL_QUEUE, eventType: "session_control" };
  }
  if (topic === TOPICS.indexEvents) {
    return { queue: env.INDEX_EVENTS_QUEUE, eventType: "index" };
  }
  return null;
}

function isSessionPartsTopic(topic: string): boolean {
  return topic.startsWith(TOPICS.sessionPartsPrefix);
}

function streamShard(env: TalonCfBindingsEnv, topic: string): DurableObjectStub {
  return env.SESSION_STREAMS.get(env.SESSION_STREAMS.idFromName(topic));
}

/**
 * Handles the internal Queue publish bridge used by Rust `CfQueuesPublisher`.
 *
 * Contract:
 * - `POST /publish`
 * - JSON body: `{ topic: string, payloadBase64: string }`
 * - topic must be one of Talon's canonical topic names from `TOPICS`
 * - `talon.session.parts.<shard>` topics are live-stream topics. They are
 *   published to `SessionStreamShard` Durable Objects instead of Cloudflare
 *   Queues because Cloudflare Queues are push-only and cannot back gateway
 *   browser streams.
 * - `GET /subscribe?topic=<topic>` returns NDJSON lines:
 *   `{ "payloadBase64": string }\n`
 *
 * The Worker maps Talon topic names to concrete Cloudflare Queue bindings and
 * stores a queue body containing `{ eventType, payloadBase64 }`.
 */
export async function handleQueues(request: Request, env: TalonCfBindingsEnv): Promise<Response> {
  const url = new URL(request.url);
  if (url.pathname === "/subscribe") {
    const topic = url.searchParams.get("topic");
    if (!topic || !isSessionPartsTopic(topic)) return new Response("unknown stream topic", { status: 400 });
    return streamShard(env, topic).fetch(request);
  }
  if (url.pathname !== "/publish") return new Response("not found", { status: 404 });
  let payload: QueuePayload;
  try {
    payload = await body<QueuePayload>(request);
  } catch {
    return new Response("invalid JSON body", { status: 400 });
  }
  if (
    typeof payload !== "object" ||
    payload === null ||
    typeof payload.topic !== "string" ||
    typeof payload.payloadBase64 !== "string"
  ) {
    return new Response("invalid queue payload", { status: 400 });
  }
  if (isSessionPartsTopic(payload.topic)) {
    return streamShard(env, payload.topic).fetch(new Request("http://session-stream.internal/publish", {
      method: "POST",
      headers: TEXT_JSON,
      body: JSON.stringify(payload),
    }));
  }
  const destination = queueForTopic(env, payload.topic);
  if (!destination) return new Response(`unknown topic: ${payload.topic}`, { status: 400 });
  await destination.queue.send({
    eventType: destination.eventType,
    payloadBase64: payload.payloadBase64,
  } satisfies QueueMessageBody);
  return json({});
}

/**
 * Durable Object live fanout for Talon session part events.
 *
 * Rust workers publish `talon.session.parts.<shard>` payloads through
 * `POST /publish`. Rust gateways subscribe with
 * `GET /subscribe?topic=talon.session.parts.<shard>`. The response body stays
 * open and emits one NDJSON line per part payload:
 * `{ "payloadBase64": string }\n`.
 *
 * This object is deliberately ephemeral. Durable session state remains in D1;
 * this bridge exists only so currently connected browsers can receive live
 * token deltas on Cloudflare, where Queues do not provide pull subscriptions.
 */
export class SessionStreamShard {
  private subscribers = new Set<StreamSubscriber>();

  constructor(
    readonly ctx: DurableObjectState,
    readonly env: TalonCfBindingsEnv,
  ) {}

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    if (url.pathname === "/subscribe") return this.subscribe(request);
    if (url.pathname === "/publish") return this.publish(request);
    if (url.pathname === "/healthz") return json({ ok: true, subscribers: this.subscribers.size });
    return new Response("not found", { status: 404 });
  }

  private subscribe(request: Request): Response {
    const stream = new TransformStream<Uint8Array, Uint8Array>();
    const subscriber = { writer: stream.writable.getWriter() };
    this.subscribers.add(subscriber);

    const cleanup = () => {
      this.subscribers.delete(subscriber);
      subscriber.writer.close().catch(() => {});
    };
    request.signal.addEventListener("abort", cleanup, { once: true });

    return new Response(stream.readable, { headers: STREAM_HEADERS });
  }

  private async publish(request: Request): Promise<Response> {
    let payload: QueuePayload;
    try {
      payload = await body<QueuePayload>(request);
    } catch {
      return new Response("invalid JSON body", { status: 400 });
    }
    if (
      typeof payload !== "object" ||
      payload === null ||
      typeof payload.payloadBase64 !== "string"
    ) {
      return new Response("invalid stream payload", { status: 400 });
    }

    const line = encoder.encode(`${JSON.stringify({ payloadBase64: payload.payloadBase64 })}\n`);
    const subscribers = this.subscribers.size;
    for (const subscriber of this.subscribers) {
      subscriber.writer.write(line).catch(() => {
        this.subscribers.delete(subscriber);
      });
    }
    return json({ subscribers });
  }
}

export type QueueWorkerResolver = (message: Message, index: number) => BoundFetcher | Promise<BoundFetcher>;

/**
 * Forwards Cloudflare Queue batches into a Rust worker container.
 *
 * Contract with Rust worker:
 * - `POST http://worker/cloudflare/queues/dispatch`
 * - `authorization: Bearer <TALON_SCHEDULER_AUTH_TOKEN>`
 * - JSON body: `{ eventType, deliveryId, payloadBase64 }`
 *
 * Each Cloudflare message is handled independently. A malformed message,
 * worker lookup failure, non-2xx response, or thrown error retries only that
 * message and does not reject the whole Queue batch.
 */
export async function dispatchQueueBatch(
  batch: MessageBatch,
  env: TalonCfBindingsEnv,
  resolveWorker: QueueWorkerResolver = () => env.WORKER_CONTAINER.get(env.WORKER_CONTAINER.idFromName("default")),
) {
  await Promise.all(batch.messages.map(async (message, index) => {
    try {
      const worker = await resolveWorker(message, index);
      const payload = message.body as QueueMessageBody;
      if (
        typeof payload !== "object" ||
        payload === null ||
        typeof payload.eventType !== "string" ||
        typeof payload.payloadBase64 !== "string"
      ) {
        throw new Error("malformed queue message body");
      }
      const response = await worker.fetch("http://worker/cloudflare/queues/dispatch", {
        method: "POST",
        headers: {
          ...TEXT_JSON,
          authorization: `Bearer ${env.TALON_SCHEDULER_AUTH_TOKEN ?? DEFAULT_SCHEDULER_AUTH_TOKEN}`,
        },
        body: JSON.stringify({
          eventType: payload.eventType,
          deliveryId: message.id,
          payloadBase64: payload.payloadBase64,
        }),
      });
      if (!response.ok) {
        throw new Error(`worker returned HTTP ${response.status}`);
      }
      message.ack();
    } catch (error) {
      console.error(`failed to dispatch queue message ${message.id}`, error);
      try {
        message.retry();
      } catch (retryError) {
        console.error(`failed to retry queue message ${message.id}`, retryError);
      }
    }
  }));
}
