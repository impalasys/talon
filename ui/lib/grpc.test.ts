import {
  applyGatewayAuthorizationHeader,
  buildGatewayHeaders,
  getDefaultGatewayUrl,
  getGatewayClient,
  getSightlineRefreshUrl,
  isExpiredSignatureAuthError,
  normalizeGatewayUrl,
  updateGatewayClient,
} from "./grpc";

describe("normalizeGatewayUrl", () => {
  it("trims whitespace and trailing slashes", () => {
    expect(normalizeGatewayUrl("  http://localhost:50051///  ")).toBe(
      "http://localhost:50051",
    );
  });

  it("preserves path segments", () => {
    expect(normalizeGatewayUrl("https://example.com/base/")).toBe(
      "https://example.com/base",
    );
  });
});

describe("getDefaultGatewayUrl", () => {
  const originalGatewayUrl = process.env.NEXT_PUBLIC_GATEWAY_URL;

  afterEach(() => {
    if (originalGatewayUrl === undefined) {
      delete process.env.NEXT_PUBLIC_GATEWAY_URL;
    } else {
      process.env.NEXT_PUBLIC_GATEWAY_URL = originalGatewayUrl;
    }
  });

  it("uses the configured gateway URL when present", () => {
    process.env.NEXT_PUBLIC_GATEWAY_URL = "  https://gateway.example.com///  ";

    expect(getDefaultGatewayUrl()).toBe("https://gateway.example.com");
  });
});

describe("getSightlineRefreshUrl", () => {
  afterEach(() => {
    document.cookie = "sightline_refresh_url=; Max-Age=0; path=/";
  });

  it("uses the Osprey refresh URL cookie when present", () => {
    document.cookie = "sightline_refresh_url=https%3A%2F%2Fosprey.test%2Finternal%2Fv1%2Fsightline%2Frefresh; path=/";

    expect(getSightlineRefreshUrl()).toBe("https://osprey.test/internal/v1/sightline/refresh");
  });

  it("returns null when the Osprey refresh URL cookie is absent", () => {
    document.cookie = "other_cookie=value; path=/";

    expect(getSightlineRefreshUrl()).toBeNull();
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

describe("isExpiredSignatureAuthError", () => {
  it("detects expired signature messages", () => {
    expect(isExpiredSignatureAuthError({ rawMessage: "JWT expired signature" })).toBe(true);
    expect(isExpiredSignatureAuthError({ message: "signature has expired" })).toBe(true);
  });

  it("detects unauthenticated expired errors", () => {
    expect(isExpiredSignatureAuthError({ codeName: "Unauthenticated", message: "token expired" })).toBe(true);
  });

  it("ignores unrelated auth errors", () => {
    expect(isExpiredSignatureAuthError({ codeName: "Unauthenticated", message: "invalid audience" })).toBe(false);
  });
});

describe("gateway client lifecycle", () => {
  it("replaces the shared client when the gateway URL changes", () => {
    const initialClient = getGatewayClient();

    updateGatewayClient("http://localhost:50051/");

    expect(getGatewayClient()).not.toBe(initialClient);
    expect(getGatewayClient().sessions).toBeDefined();
  });
});
