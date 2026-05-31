import {
  applyGatewayAuthorizationHeader,
  buildGatewayHeaders,
  getGatewayClient,
  normalizeGatewayUrl,
  updateGatewayClient,
} from "./grpc";

describe("normalizeGatewayUrl", () => {
  it("trims whitespace and trailing slashes", () => {
    expect(normalizeGatewayUrl("  http://localhost:18789///  ")).toBe(
      "http://localhost:18789",
    );
  });

  it("preserves path segments", () => {
    expect(normalizeGatewayUrl("https://example.com/base/")).toBe(
      "https://example.com/base",
    );
  });
});

describe("buildGatewayHeaders", () => {
  it("returns undefined when no auth token is provided", () => {
    expect(buildGatewayHeaders()).toBeUndefined();
    expect(buildGatewayHeaders("")).toBeUndefined();
    expect(buildGatewayHeaders("   ")).toBeUndefined();
    expect(buildGatewayHeaders(null)).toBeUndefined();
  });

  it("uses bearer auth for bare tokens", () => {
    expect(buildGatewayHeaders("secret-token")).toEqual({
      Authorization: "Bearer secret-token",
    });
  });

  it("preserves explicit auth schemes", () => {
    expect(buildGatewayHeaders("Bearer jwt-token")).toEqual({
      Authorization: "Bearer jwt-token",
    });
    expect(buildGatewayHeaders("Basic OnNlY3JldA==")).toEqual({
      Authorization: "Basic OnNlY3JldA==",
    });
  });
});

describe("applyGatewayAuthorizationHeader", () => {
  it("adds an authorization header when a token exists", () => {
    const calls: Array<[string, string]> = [];

    applyGatewayAuthorizationHeader(
      {
        set(name, value) {
          calls.push([name, value]);
        },
      },
      "secret-token",
    );

    expect(calls).toEqual([["authorization", "Bearer secret-token"]]);
  });

  it("does nothing when the token is missing", () => {
    const calls: Array<[string, string]> = [];

    applyGatewayAuthorizationHeader(
      {
        set(name, value) {
          calls.push([name, value]);
        },
      },
      null,
    );

    expect(calls).toEqual([]);
  });
});

describe("gateway client lifecycle", () => {
  it("replaces the shared client when the gateway URL changes", () => {
    const initialClient = getGatewayClient();

    updateGatewayClient("http://localhost:18789/");

    expect(getGatewayClient()).not.toBe(initialClient);
  });
});
