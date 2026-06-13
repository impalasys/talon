import { DurableObject } from "cloudflare:workers";

import { DEFAULT_SCHEDULER_AUTH_TOKEN, TEXT_JSON } from "./constants";
import { body, decodeBase64, json, nowMicros } from "./http";
import type { TalonCfBindingsEnv } from "./types";

type AlarmScheduleRequest = {
  namespace: string;
  scheduleId: string;
  revision: number;
  fireAtMicros: number;
  payloadBase64: string;
};

type StoredAlarmWakeup = AlarmScheduleRequest & {
  handle: string;
  failedAttempts?: number;
};

function microsToMillis(micros: number): number {
  return Math.max(0, Math.floor(micros / 1000));
}

function configuredCount(raw: string | undefined): number {
  const parsed = Number.parseInt(raw ?? "", 10);
  return Number.isFinite(parsed) && parsed > 0 ? Math.min(parsed, 128) : 1;
}

function stableHash(value: string): number {
  let hash = 2166136261;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return hash >>> 0;
}

function workerInstanceName(key: string, count: number): string {
  if (count <= 1) return "default";
  return `worker-${stableHash(key) % count}`;
}

function retryDelayMicros(attempts: number): number {
  const seconds = Math.min(60, 2 ** Math.min(attempts, 5));
  return seconds * 1_000_000;
}

export async function handleAlarms(request: Request, env: TalonCfBindingsEnv): Promise<Response> {
  const shard = env.SCHEDULE_SHARD.get(env.SCHEDULE_SHARD.idFromName("default"));
  return shard.fetch(request);
}

export class ScheduleShard extends DurableObject<TalonCfBindingsEnv> {
  constructor(ctx: DurableObjectState, env: TalonCfBindingsEnv) {
    super(ctx, env);
  }

  async fetch(request: Request): Promise<Response> {
    const path = new URL(request.url).pathname;
    if (path === "/schedule") {
      const req = await body<AlarmScheduleRequest>(request);
      const handle = crypto.randomUUID();
      const wakeup: StoredAlarmWakeup = { ...req, handle };
      await this.ctx.storage.put(this.wakeupKey(handle), wakeup);
      await this.armNextAlarm();
      return json({ handle, armed: true });
    }

    if (path === "/cancel") {
      const { handle } = await body<{ handle: string }>(request);
      await this.ctx.storage.delete(this.wakeupKey(handle));
      await this.armNextAlarm();
      return json({});
    }

    if (path === "/healthz") return json({ ok: true });
    return new Response("not found", { status: 404 });
  }

  async alarm(): Promise<void> {
    const due = await this.dueWakeups(nowMicros());
    const workerCount = configuredCount(this.env.TALON_WORKER_CONTAINER_COUNT);
    for (const wakeup of due) {
      const workerName = workerInstanceName(`${wakeup.namespace}:${wakeup.scheduleId}`, workerCount);
      const worker = this.env.WORKER_CONTAINER.get(this.env.WORKER_CONTAINER.idFromName(workerName));
      try {
        const response = await worker.fetch("http://worker/schedules/fire", {
          method: "POST",
          headers: {
            ...TEXT_JSON,
            "X-Talon-Scheduler-Token": this.env.TALON_SCHEDULER_AUTH_TOKEN ?? DEFAULT_SCHEDULER_AUTH_TOKEN,
          },
          body: decodeBase64(wakeup.payloadBase64),
        });
        if (!response.ok) {
          throw new Error(`HTTP ${response.status}: ${await response.text()}`);
        }
        await this.ctx.storage.delete(this.wakeupKey(wakeup.handle));
      } catch (error) {
        const failedAttempts = (wakeup.failedAttempts ?? 0) + 1;
        console.error(`schedule wakeup ${wakeup.handle} failed`, error);
        await this.ctx.storage.put(this.wakeupKey(wakeup.handle), {
          ...wakeup,
          failedAttempts,
          fireAtMicros: nowMicros() + retryDelayMicros(failedAttempts),
        });
      }
    }
    await this.armNextAlarm();
  }

  private wakeupKey(handle: string): string {
    return `wakeup:${handle}`;
  }

  private async allWakeups(): Promise<StoredAlarmWakeup[]> {
    const rows = await this.ctx.storage.list<StoredAlarmWakeup>({
      prefix: "wakeup:",
    });
    return [...rows.values()];
  }

  private async dueWakeups(now: number): Promise<StoredAlarmWakeup[]> {
    return (await this.allWakeups())
      .filter((wakeup) => wakeup.fireAtMicros <= now)
      .sort((left, right) => left.fireAtMicros - right.fireAtMicros);
  }

  private async nextWakeup(): Promise<StoredAlarmWakeup | undefined> {
    return (await this.allWakeups())
      .sort((left, right) => left.fireAtMicros - right.fireAtMicros)[0];
  }

  private async armNextAlarm(): Promise<void> {
    const next = await this.nextWakeup();
    if (!next) {
      await this.ctx.storage.deleteAlarm();
      return;
    }
    await this.ctx.storage.setAlarm(microsToMillis(next.fireAtMicros));
  }
}
