import { json } from "./http";
import type { TalonCfBindingsEnv } from "./types";

export async function handleR2(request: Request, env: TalonCfBindingsEnv): Promise<Response> {
  const url = new URL(request.url);
  const key = decodeURIComponent(url.pathname.replace(/^\/objects\//, ""));
  const metaKey = `${key}.metadata.json`;
  if (!key || key === url.pathname) return new Response("missing object key", { status: 400 });

  if (request.method === "PUT") {
    const metadata = request.headers.get("x-talon-object-metadata") ?? "";
    await env.TALON_R2.put(key, request.body, {
      httpMetadata: { contentType: request.headers.get("content-type") ?? undefined },
    });
    await env.TALON_R2.put(metaKey, metadata);
    return json({});
  }

  if (request.method === "GET") {
    const object = await env.TALON_R2.get(key);
    if (!object) return new Response("not found", { status: 404 });
    const metadata = await env.TALON_R2.get(metaKey);
    const headers = new Headers();
    if (object.httpMetadata?.contentType) headers.set("content-type", object.httpMetadata.contentType);
    if (metadata) headers.set("x-talon-object-metadata", await metadata.text());
    return new Response(object.body, { headers });
  }

  if (request.method === "DELETE") {
    await env.TALON_R2.delete(key);
    await env.TALON_R2.delete(metaKey);
    return json({});
  }

  return new Response("method not allowed", { status: 405 });
}
