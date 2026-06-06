import { describe, it } from "node:test";
import assert from "node:assert/strict";

import { authorizationHeader, mintJwt, TalonServer } from "./index.js";

describe("@impalasys/talon-server", () => {
  it("exports the server helper", () => {
    assert.equal(typeof TalonServer.start, "function");
  });

  it("rejects ambiguous config options", async () => {
    await assert.rejects(
      () => TalonServer.start({ configPath: "talon.yaml", config: { workspace_dir: "." } }),
      /configPath cannot be combined/,
    );
  });

  it("rejects provider when a general config object is supplied", async () => {
    await assert.rejects(
      () => TalonServer.start({
        config: {
          workspace_dir: "/tmp/workspace",
          default_provider: "openai",
          control_plane: {
            database: { driver: "sqlite" },
            message_broker: { driver: "local_socket" },
          },
        },
        provider: {
          baseUrl: "http://127.0.0.1",
          model: "mock",
          apiKey: "secret",
        },
      }),
      /config cannot be combined/,
    );
  });

  it("mints scoped Talon JWTs", () => {
    const token = mintJwt("secret", {
      subject: "browser-demo",
      ttlSeconds: 60,
      namespace: "demo",
      agent: "copilot",
      channel: "chat",
    });
    const [encodedHeader, encodedPayload, signature] = token.split(".");
    assert.ok(encodedHeader);
    assert.ok(encodedPayload);
    assert.ok(signature);

    const header = JSON.parse(Buffer.from(encodedHeader, "base64url").toString("utf8"));
    const payload = JSON.parse(Buffer.from(encodedPayload, "base64url").toString("utf8"));
    assert.deepEqual(header, { alg: "HS256", typ: "JWT" });
    assert.equal(payload.sub, "browser-demo");
    assert.equal(payload.aud, "talon");
    assert.equal(payload["talon:ns"], "demo");
    assert.equal(payload["talon:agent"], "copilot");
    assert.equal(payload["talon:channel"], "chat");
    assert.equal(authorizationHeader(token), `Bearer ${token}`);
  });

  it("requires namespace for channel-scoped JWTs", () => {
    assert.throws(() => mintJwt("secret", { channel: "chat" }), /namespace/);
  });
});
