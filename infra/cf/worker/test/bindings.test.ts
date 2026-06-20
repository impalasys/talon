import assert from "node:assert/strict";
import { test } from "node:test";

import { handleD1 } from "../../bindings/src/d1";
import { handleQueues, dispatchQueueBatch, SessionStreamShard } from "../../bindings/src/queues";
import { handleR2 } from "../../bindings/src/r2";
import { TOPICS } from "../../bindings/src/constants";

class MockD1Statement {
  constructor(private readonly database: MockD1Database, private readonly sql: string) {}

  bind(...params: unknown[]) {
    this.database.lastSql = this.sql;
    this.database.lastParams = params;
    return this;
  }

  async run() {
    return { meta: { changes: 1 } };
  }

  async all() {
    return { results: this.database.rows, meta: { served_by: "mock-d1" } };
  }

  async first() {
    return this.database.firstRow;
  }
}

class MockD1Database {
  lastSql = "";
  lastParams: unknown[] = [];
  lastSessionConstraint: unknown = undefined;
  rows: Record<string, unknown>[] = [];
  firstRow: Record<string, unknown> | null = null;

  withSession(constraint: unknown) {
    this.lastSessionConstraint = constraint;
    return this;
  }

  prepare(sql: string) {
    return new MockD1Statement(this, sql);
  }
}

type StoredR2Object = {
  body: Uint8Array;
  httpMetadata?: { contentType?: string };
  customMetadata?: Record<string, string>;
};

class MockR2Bucket {
  readonly objects = new Map<string, StoredR2Object>();

  async put(key: string, body: ReadableStream | null, options: {
    httpMetadata?: { contentType?: string };
    customMetadata?: Record<string, string>;
  }) {
    const bytes = new Uint8Array(await new Response(body).arrayBuffer());
    this.objects.set(key, {
      body: bytes,
      httpMetadata: options.httpMetadata,
      customMetadata: options.customMetadata,
    });
  }

  async get(key: string) {
    return this.objects.get(key) ?? null;
  }

  async delete(key: string) {
    this.objects.delete(key);
  }
}

class MockQueue {
  readonly messages: unknown[] = [];

  async send(message: unknown) {
    this.messages.push(message);
  }
}

class MockDurableObjectNamespace {
  readonly objects = new Map<string, unknown>();

  idFromName(name: string) {
    return { name } as DurableObjectId & { name: string };
  }

  get(id: DurableObjectId & { name: string }) {
    let object = this.objects.get(id.name) as { fetch(request: Request): Promise<Response> } | undefined;
    if (!object) {
      object = new SessionStreamShard({} as DurableObjectState, baseEnv() as any);
      this.objects.set(id.name, object);
    }
    return object;
  }
}

function baseEnv(overrides: Record<string, unknown> = {}) {
  const noopQueue = new MockQueue();
  return {
    TALON_D1: new MockD1Database(),
    TALON_R2: new MockR2Bucket(),
    SESSION_DISPATCH_QUEUE: noopQueue,
    RESOURCE_LIFECYCLE_QUEUE: noopQueue,
    SESSION_CONTROL_QUEUE: noopQueue,
    SESSION_STREAMS: new MockDurableObjectNamespace(),
    TALON_SCHEDULER_AUTH_TOKEN: "test-scheduler-token",
    ...overrides,
  } as any;
}

test("D1 bridge decodes Rust execute params and forwards prepared SQL", async () => {
  const d1 = new MockD1Database();
  const response = await handleD1(
    new Request("http://talon-d1.internal/execute", {
      method: "POST",
      body: JSON.stringify({
        mode: "run",
        sql: "INSERT INTO \"talon_kv_store\" VALUES (?1, ?2, ?3, ?4, ?5)",
        params: [
          { type: "text", value: "default" },
          { type: "text", value: "Agent/demo" },
          { type: "text", value: "Session" },
          { type: "text", value: "s1" },
          { type: "bytes", valueBase64: "aGVsbG8=" },
        ],
      }),
    }),
    baseEnv({ TALON_D1: d1 }),
  );

  assert.equal(response.status, 200);
  assert.equal(d1.lastSessionConstraint, "first-primary");
  assert.equal(d1.lastSql, "INSERT INTO \"talon_kv_store\" VALUES (?1, ?2, ?3, ?4, ?5)");
  assert.deepEqual(d1.lastParams.slice(0, 4), ["default", "Agent/demo", "Session", "s1"]);
  assert.deepEqual([...d1.lastParams[4] as Uint8Array], [...new TextEncoder().encode("hello")]);
  assert.deepEqual(await response.json(), { meta: { changes: 1 } });
});

test("D1 bridge returns logged errors for malformed execute bodies", async () => {
  const errors: unknown[][] = [];
  const originalError = console.error;
  console.error = (...args: unknown[]) => errors.push(args);
  try {
    const response = await handleD1(
      new Request("http://talon-d1.internal/execute", {
        method: "POST",
        body: "{",
      }),
      baseEnv(),
    );

    assert.equal(response.status, 500);
    assert.match((await response.json() as { error: string }).error, /JSON|parse|Unexpected/i);
    assert.equal(errors[0]?.[0], "D1 bridge request failed");
  } finally {
    console.error = originalError;
  }
});

test("Queue bridge fans out session part topics through live stream Durable Objects", async () => {
  const env = baseEnv();
  const controller = new AbortController();
  const subscription = await handleQueues(
    new Request("http://talon-queues.internal/subscribe?topic=talon.session.parts.7", {
      signal: controller.signal,
    }),
    env,
  );
  assert.equal(subscription.status, 200);

  const reader = subscription.body!.getReader();
  const published = await handleQueues(
    new Request("http://talon-queues.internal/publish", {
      method: "POST",
      body: JSON.stringify({
        topic: "talon.session.parts.7",
        payloadBase64: "aGVsbG8=",
      }),
    }),
    env,
  );
  assert.equal(published.status, 200);
  assert.deepEqual(await published.json(), { subscribers: 1 });

  const chunk = await reader.read();
  assert.equal(chunk.done, false);
  assert.equal(new TextDecoder().decode(chunk.value), "{\"payloadBase64\":\"aGVsbG8=\"}\n");
  controller.abort();
});

test("D1 bridge encodes result rows into Rust tagged cells", async () => {
  const d1 = new MockD1Database();
  d1.rows = [{
    namespace: "default",
    version: 7,
    active: true,
    value: new TextEncoder().encode("hello"),
    missing: null,
  }];

  const response = await handleD1(
    new Request("http://talon-d1.internal/execute", {
      method: "POST",
      body: JSON.stringify({ mode: "all", sql: "SELECT * FROM \"talon_kv_store\"", params: [] }),
    }),
    baseEnv({ TALON_D1: d1 }),
  );

  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), {
    results: [{
      namespace: { type: "text", value: "default" },
      version: { type: "number", value: 7 },
      active: { type: "bool", value: true },
      value: { type: "bytes", valueBase64: "aGVsbG8=" },
      missing: { type: "null" },
    }],
    meta: { served_by: "mock-d1" },
  });
});

test("R2 bridge stores and returns Rust object bytes and metadata", async () => {
  const r2 = new MockR2Bucket();
  const key = "namespaces/default/artifacts/file one.txt";
  const metadata = "eyJtZXRhZGF0YSI6eyJtZWRpYVR5cGUiOiJ0ZXh0L3BsYWluIn19";

  const put = await handleR2(
    new Request(`http://talon-r2.internal/objects/${encodeURIComponent(key)}`, {
      method: "PUT",
      headers: {
        "content-type": "text/plain",
        "x-talon-object-metadata": metadata,
      },
      body: "hello from rust",
    }),
    baseEnv({ TALON_R2: r2 }),
  );
  assert.equal(put.status, 200);

  const stored = r2.objects.get(key);
  assert.ok(stored);
  assert.equal(stored.httpMetadata?.contentType, "text/plain");
  assert.equal(stored.customMetadata?.talon, metadata);
  assert.equal(new TextDecoder().decode(stored.body), "hello from rust");

  const get = await handleR2(
    new Request(`http://talon-r2.internal/objects/${encodeURIComponent(key)}`),
    baseEnv({ TALON_R2: r2 }),
  );
  assert.equal(get.status, 200);
  assert.equal(get.headers.get("content-type"), "text/plain");
  assert.equal(get.headers.get("x-talon-object-metadata"), metadata);
  assert.equal(await get.text(), "hello from rust");
});

test("Queue bridge maps Rust topics onto Cloudflare queue messages", async () => {
  const sessionDispatch = new MockQueue();
  const resourceLifecycle = new MockQueue();
  const sessionControl = new MockQueue();
  const env = baseEnv({
    SESSION_DISPATCH_QUEUE: sessionDispatch,
    RESOURCE_LIFECYCLE_QUEUE: resourceLifecycle,
    SESSION_CONTROL_QUEUE: sessionControl,
  });

  for (const topic of [TOPICS.sessionDispatch, TOPICS.resourceLifecycle, TOPICS.sessionControl]) {
    const response = await handleQueues(
      new Request("http://talon-queues.internal/publish", {
        method: "POST",
        body: JSON.stringify({ topic, payloadBase64: "eyJvayI6dHJ1ZX0=" }),
      }),
      env,
    );
    assert.equal(response.status, 200);
  }

  assert.deepEqual(sessionDispatch.messages, [{
    eventType: "session_dispatch",
    payloadBase64: "eyJvayI6dHJ1ZX0=",
  }]);
  assert.deepEqual(resourceLifecycle.messages, [{
    eventType: "resource_lifecycle",
    payloadBase64: "eyJvayI6dHJ1ZX0=",
  }]);
  assert.deepEqual(sessionControl.messages, [{
    eventType: "session_control",
    payloadBase64: "eyJvayI6dHJ1ZX0=",
  }]);
});

test("Queue batch dispatch posts to the Rust worker endpoint and acks on success", async () => {
  let dispatched: { input: RequestInfo | URL; init?: RequestInit } | null = null;
  let acked = false;
  let retried = false;
  const message = {
    id: "delivery-1",
    body: { eventType: "session_dispatch", payloadBase64: "aGVsbG8=" },
    ack: () => {
      acked = true;
    },
    retry: () => {
      retried = true;
    },
  };

  await dispatchQueueBatch(
    { messages: [message] } as any,
    baseEnv(),
    () => ({
      async fetch(input, init) {
        dispatched = { input, init };
        return new Response("", { status: 200 });
      },
    }),
  );

  assert.equal(dispatched?.input, "http://worker/cloudflare/queues/dispatch");
  assert.equal((dispatched?.init?.headers as Record<string, string>).authorization, "Bearer test-scheduler-token");
  assert.deepEqual(JSON.parse(dispatched?.init?.body as string), {
    eventType: "session_dispatch",
    deliveryId: "delivery-1",
    payloadBase64: "aGVsbG8=",
  });
  assert.equal(acked, true);
  assert.equal(retried, false);
});

test("Queue batch dispatch retries a message when the Rust worker rejects it", async () => {
  const originalConsoleError = console.error;
  console.error = () => {};
  let acked = false;
  let retried = false;
  const message = {
    id: "delivery-2",
    body: { eventType: "session_dispatch", payloadBase64: "aGVsbG8=" },
    ack: () => {
      acked = true;
    },
    retry: () => {
      retried = true;
    },
  };

  try {
    await dispatchQueueBatch(
      { messages: [message] } as any,
      baseEnv(),
      () => ({
        async fetch() {
          return new Response("nope", { status: 503 });
        },
      }),
    );
  } finally {
    console.error = originalConsoleError;
  }

  assert.equal(acked, false);
  assert.equal(retried, true);
});
