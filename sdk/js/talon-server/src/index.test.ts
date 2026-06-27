import { describe, it } from "node:test";
import assert from "node:assert/strict";

import { authorizationHeader, TalonServer } from "./index.js";

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

  it("formats bearer authorization headers", () => {
    const token = "test-token";
    assert.equal(authorizationHeader(token), `Bearer ${token}`);
  });

  it("requires non-empty bearer tokens", () => {
    assert.throws(() => authorizationHeader(" "), /token is required/);
  });
});
