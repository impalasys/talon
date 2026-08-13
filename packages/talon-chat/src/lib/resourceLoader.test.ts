import assert from "node:assert/strict";
import test from "node:test";
import { loadSignedInlineContent } from "./resourceSignedContent.ts";

test("downloads text artifact content from its signed object URL", async () => {
  const originalFetch = globalThis.fetch;
  const requests: Array<{ url: string; options?: RequestInit }> = [];
  globalThis.fetch = async (url, options) => {
    requests.push({ url: String(url), options });
    return new Response("# Stored artifact", { status: 200 });
  };

  try {
    const content = await loadSignedInlineContent({
      content: new Uint8Array(),
      mediaType: "text/markdown",
      signedUrl: "https://objects.example/artifacts/draft?signature=redacted",
      sizeBytes: 18,
      signal: new AbortController().signal,
    });

    assert.equal(new TextDecoder().decode(content), "# Stored artifact");
    assert.equal(requests.length, 1);
    assert.equal(requests[0]?.url, "https://objects.example/artifacts/draft?signature=redacted");
    assert.equal(requests[0]?.options?.credentials, "omit");
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("does not download a large signed text artifact for inline preview", async () => {
  const originalFetch = globalThis.fetch;
  let fetchCalls = 0;
  globalThis.fetch = async () => {
    fetchCalls += 1;
    return new Response("unexpected");
  };

  try {
    const content = await loadSignedInlineContent({
      content: new Uint8Array(),
      mediaType: "text/markdown",
      signedUrl: "https://objects.example/artifacts/large-draft",
      sizeBytes: 3 * 1024 * 1024 + 1,
      signal: new AbortController().signal,
    });

    assert.equal(fetchCalls, 0);
    assert.equal(content?.byteLength, 0);
  } finally {
    globalThis.fetch = originalFetch;
  }
});
