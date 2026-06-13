export type BoundFetcher = {
  fetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response>;
};

export type ContainerNamespace = {
  idFromName(name: string): DurableObjectId;
  get(id: DurableObjectId): BoundFetcher;
};

export type TalonCfBindingsEnv = {
  TALON_D1: D1Database;
  TALON_R2: R2Bucket;
  TALON_SCHEDULER_AUTH_TOKEN?: string;
  TALON_WORKER_CONTAINER_COUNT?: string;
  SESSION_DISPATCH_QUEUE: Queue;
  RESOURCE_LIFECYCLE_QUEUE: Queue;
  SESSION_CONTROL_QUEUE: Queue;
  WORKER_CONTAINER: ContainerNamespace;
  SCHEDULE_SHARD: DurableObjectNamespace;
};

export type QueueMessageBody = {
  eventType: string;
  payloadBase64: string;
};
