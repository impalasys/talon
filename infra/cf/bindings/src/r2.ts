import { json } from "./http";
import type { TalonCfBindingsEnv } from "./types";

/**
 * Handles the internal R2 object bridge used by Rust `R2ObjectStore`.
 *
 * Contract:
 * - `PUT /objects/{percent-encoded-key}` stores the request body in R2
 * - `GET /objects/{percent-encoded-key}` returns the object body or 404
 * - `DELETE /objects/{percent-encoded-key}` deletes the object
 * - `content-type` is preserved through R2 HTTP metadata
 * - `x-talon-object-metadata` is preserved in `customMetadata.talon`
 *
 * The Rust side owns object key naming and metadata envelope encoding. The
 * Worker only maps HTTP requests onto the R2 binding.
 */
export async function handleR2(request: Request, env: TalonCfBindingsEnv): Promise<Response> {
  const url = new URL(request.url);
  const rawKey = url.pathname.replace(/^\/objects\//, "");
  let key: string;
  try {
    key = decodeURIComponent(rawKey);
  } catch {
    return new Response("invalid object key encoding", { status: 400 });
  }
  if (!key || key === url.pathname) return new Response("missing object key", { status: 400 });

  if (request.method === "PUT") {
    const metadata = request.headers.get("x-talon-object-metadata") ?? "";
    await env.TALON_R2.put(key, request.body, {
      httpMetadata: { contentType: request.headers.get("content-type") ?? undefined },
      customMetadata: { talon: metadata },
    });
    return json({});
  }

  if (request.method === "GET") {
    const object = await env.TALON_R2.get(key);
    if (!object) return new Response("not found", { status: 404 });
    const headers = new Headers();
    if (object.httpMetadata?.contentType) headers.set("content-type", object.httpMetadata.contentType);
    if (object.customMetadata?.talon) headers.set("x-talon-object-metadata", object.customMetadata.talon);
    return new Response(object.body, { headers });
  }

  if (request.method === "DELETE") {
    await env.TALON_R2.delete(key);
    return json({});
  }

  return new Response("method not allowed", { status: 405 });
}
