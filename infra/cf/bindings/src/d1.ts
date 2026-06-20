import { body, decodeBase64, encodeBase64, json } from "./http";
import type { TalonCfBindingsEnv } from "./types";

type D1ExecuteMode = "run" | "all" | "first";

type D1Param =
  | { type: "null" }
  | { type: "text"; value: string }
  | { type: "number"; value: number }
  | { type: "bool"; value: boolean }
  | { type: "bytes"; valueBase64: string };

type D1Cell =
  | { type: "null" }
  | { type: "text"; value: string }
  | { type: "number"; value: number }
  | { type: "bool"; value: boolean }
  | { type: "bytes"; valueBase64: string };

type D1ExecuteRequest = {
  mode: D1ExecuteMode;
  sql: string;
  params?: D1Param[];
};

function decodeParam(param: D1Param): null | string | number | boolean | Uint8Array {
  switch (param.type) {
    case "null":
      return null;
    case "text":
    case "number":
    case "bool":
      return param.value;
    case "bytes":
      return decodeBase64(param.valueBase64);
  }
}

function encodeCell(value: unknown): D1Cell {
  if (value === null || value === undefined) return { type: "null" };
  if (typeof value === "string") return { type: "text", value };
  if (typeof value === "number") return { type: "number", value };
  if (typeof value === "boolean") return { type: "bool", value };
  if (value instanceof ArrayBuffer) {
    return { type: "bytes", valueBase64: encodeBase64(value) ?? "" };
  }
  if (value instanceof Uint8Array) {
    return { type: "bytes", valueBase64: encodeBase64(value) ?? "" };
  }
  if (Array.isArray(value) && value.every((item) => typeof item === "number")) {
    return { type: "bytes", valueBase64: encodeBase64(Uint8Array.from(value)) ?? "" };
  }
  return { type: "text", value: JSON.stringify(value) };
}

function encodeRow(row: Record<string, unknown>): Record<string, D1Cell> {
  return Object.fromEntries(Object.entries(row).map(([key, value]) => [key, encodeCell(value)]));
}

/**
 * Handles the internal D1 SQL bridge used by Rust `D1KvStore`.
 *
 * Contract:
 * - `POST /execute`
 * - JSON body: `{ mode: "run" | "all" | "first", sql: string, params?: D1Param[] }`
 * - params are tagged JSON values; bytes use `{ type: "bytes", valueBase64 }`
 * - responses are tagged JSON rows/cells so Rust can recover D1 value types
 *
 * This handler deliberately does not know Talon KV semantics. Rust owns the
 * schema, SQL text, parameter ordering, CAS logic, and pagination behavior.
 */
export async function handleD1(request: Request, env: TalonCfBindingsEnv): Promise<Response> {
  const path = new URL(request.url).pathname;
  if (path !== "/execute") return new Response("not found", { status: 404 });
  const payload = await body<D1ExecuteRequest>(request);
  try {
    const statement = env.TALON_D1.prepare(payload.sql).bind(...(payload.params ?? []).map(decodeParam));

    if (payload.mode === "run") {
      const result = await statement.run();
      return json({ meta: result.meta });
    }

    if (payload.mode === "first") {
      const row = await statement.first<Record<string, unknown>>();
      return json({ row: row ? encodeRow(row) : null });
    }

    if (payload.mode === "all") {
      const result = await statement.all<Record<string, unknown>>();
      return json({
        results: result.results.map(encodeRow),
        meta: result.meta,
      });
    }

    return new Response("unsupported execute mode", { status: 400 });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.error("D1 bridge request failed", {
      mode: payload.mode,
      sql: payload.sql.slice(0, 200),
      paramCount: payload.params?.length ?? 0,
      error: message,
    });
    return json({ error: message }, { status: 500 });
  }
}
