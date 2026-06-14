import { DEFAULT_SCHEDULER_AUTH_TOKEN, TEXT_JSON, TOPICS } from "./constants";
import { body, json } from "./http";
import type { BoundFetcher, QueueMessageBody, TalonCfBindingsEnv } from "./types";

type QueuePayload = {
  topic: string;
  payloadBase64: string;
};

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
  return null;
}

export async function handleQueues(request: Request, env: TalonCfBindingsEnv): Promise<Response> {
  const url = new URL(request.url);
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
  const destination = queueForTopic(env, payload.topic);
  if (!destination) return new Response(`unknown topic: ${payload.topic}`, { status: 400 });
  await destination.queue.send({
    eventType: destination.eventType,
    payloadBase64: payload.payloadBase64,
  } satisfies QueueMessageBody);
  return json({});
}

export type QueueWorkerResolver = (message: Message, index: number) => BoundFetcher | Promise<BoundFetcher>;

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
        message.retry();
      } else {
        message.ack();
      }
    } catch (error) {
      console.error(`failed to dispatch queue message ${message.id}`, error);
      message.retry();
    }
  }));
}
