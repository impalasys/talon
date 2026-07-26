import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  isResourceUri,
  linkifyResourceUris,
  parseResourceUri,
  resourceUriFromHref,
  resourceUriShortLabel,
  toResourceMarkdownHref,
} from "./resourceUris.ts";

describe("parseResourceUri", () => {
  it("parses artifact URIs with colon namespaces", () => {
    const uri = "artifact://Tenant:acme:Ops/writer/sess-1/draft";
    const parsed = parseResourceUri(uri);
    assert.deepEqual(parsed, {
      kind: "artifact",
      uri,
      namespace: "Tenant:acme:Ops",
      agent: "writer",
      sessionId: "sess-1",
      artifactId: "draft",
    });
  });

  it("parses file URIs with colon namespaces", () => {
    const uri = "file://Tenant:acme:Ops/memory-brand-guidelines";
    const parsed = parseResourceUri(uri);
    assert.deepEqual(parsed, {
      kind: "file",
      uri,
      namespace: "Tenant:acme:Ops",
      fileName: "memory-brand-guidelines",
    });
  });

  it("strips trailing prose punctuation", () => {
    const parsed = parseResourceUri("file://ns/name.");
    assert.equal(parsed?.uri, "file://ns/name");
    assert.equal(parseResourceUri("artifact://ns/a/s/id,").uri, "artifact://ns/a/s/id");
    assert.equal(parseResourceUri("file://ns/name;").uri, "file://ns/name");
    assert.equal(parseResourceUri("file://ns/name)").uri, "file://ns/name");
    assert.equal(parseResourceUri("file://ns/name]").uri, "file://ns/name");
  });

  it("rejects OS-style file paths", () => {
    assert.equal(parseResourceUri("file:///tmp/foo"), null);
    assert.equal(parseResourceUri("file:///Users/me/file.txt"), null);
    assert.equal(parseResourceUri("file://ns/a/b"), null);
  });

  it("rejects wrong artifact segment counts", () => {
    assert.equal(parseResourceUri("artifact://ns/agent"), null);
    assert.equal(parseResourceUri("artifact://ns/a/s"), null);
    assert.equal(parseResourceUri("artifact://ns/a/s/id/extra"), null);
  });

  it("rejects empty segments", () => {
    assert.equal(parseResourceUri("file://ns/"), null);
    assert.equal(parseResourceUri("file:///name"), null);
    assert.equal(parseResourceUri("artifact://ns//s/id"), null);
  });

  it("rejects unrelated schemes", () => {
    assert.equal(parseResourceUri("https://example.com"), null);
    assert.equal(parseResourceUri(""), null);
  });
});

describe("isResourceUri", () => {
  it("returns true only for parseable resource URIs", () => {
    assert.equal(isResourceUri("file://ns/name"), true);
    assert.equal(isResourceUri("artifact://ns/a/s/id"), true);
    assert.equal(isResourceUri("file:///tmp/x"), false);
  });
});

describe("toResourceMarkdownHref / resourceUriFromHref", () => {
  it("round-trips artifact and file URIs through harden-safe hash hrefs", () => {
    const artifact = "artifact://Tenant:acme:Ops/writer/sess-1/draft";
    const file = "file://Tenant:acme:Ops/memory-brand-guidelines";
    const artifactHref = toResourceMarkdownHref(artifact);
    const fileHref = toResourceMarkdownHref(file);
    assert.ok(artifactHref.startsWith("#talon-resource/"));
    assert.ok(fileHref.startsWith("#talon-resource/"));
    assert.equal(resourceUriFromHref(artifactHref), artifact);
    assert.equal(resourceUriFromHref(fileHref), file);
  });

  it("accepts raw resource URIs as hrefs", () => {
    assert.equal(resourceUriFromHref("file://ns/name"), "file://ns/name");
    assert.equal(resourceUriFromHref("artifact://ns/a/s/id"), "artifact://ns/a/s/id");
  });

  it("returns null for non-resource hrefs", () => {
    assert.equal(resourceUriFromHref("https://example.com"), null);
    assert.equal(resourceUriFromHref("#section"), null);
    assert.equal(resourceUriFromHref(""), null);
  });
});

describe("linkifyResourceUris", () => {
  it("linkifies bare artifact and file URIs with harden-safe hrefs", () => {
    const input =
      "See artifact://Tenant:acme:Ops/writer/sess-1/draft and file://Tenant:acme:Ops/memory-brand.";
    const out = linkifyResourceUris(input);
    const artifact = "artifact://Tenant:acme:Ops/writer/sess-1/draft";
    const file = "file://Tenant:acme:Ops/memory-brand";
    assert.ok(out.includes(`[${artifact}](${toResourceMarkdownHref(artifact)})`));
    assert.ok(out.includes(`[${file}](${toResourceMarkdownHref(file)}).`));
    assert.ok(!out.includes(`](${artifact})`));
    assert.ok(!out.includes(`](${file})`));
  });

  it("does not linkify inside fenced code blocks", () => {
    const input = "Before\n```\nfile://ns/name\n```\nAfter file://ns/other";
    const out = linkifyResourceUris(input);
    assert.ok(out.includes("```\nfile://ns/name\n```"));
    assert.ok(out.includes(`[file://ns/other](${toResourceMarkdownHref("file://ns/other")})`));
  });

  it("rewrites already-linked resource destinations to harden-safe hrefs", () => {
    const input = "[label](file://ns/name) and [file://ns/other](file://ns/other)";
    const out = linkifyResourceUris(input);
    assert.ok(out.includes(`[label](${toResourceMarkdownHref("file://ns/name")})`));
    assert.ok(out.includes(`[file://ns/other](${toResourceMarkdownHref("file://ns/other")})`));
    assert.ok(!out.includes("](file://"));
  });

  it("handles multi-URI lines and list items", () => {
    const input = "- draft: artifact://ns/a/s/id\n- guidelines: file://ns/guidelines";
    const out = linkifyResourceUris(input);
    assert.ok(out.includes(`[artifact://ns/a/s/id](${toResourceMarkdownHref("artifact://ns/a/s/id")})`));
    assert.ok(out.includes(`[file://ns/guidelines](${toResourceMarkdownHref("file://ns/guidelines")})`));
  });

  it("linkifies parenthesized prose URIs", () => {
    const input = "See the draft (file://ns/name) and notes.";
    const out = linkifyResourceUris(input);
    assert.ok(out.includes(`([file://ns/name](${toResourceMarkdownHref("file://ns/name")}))`));
  });

  it("leaves non-resource text unchanged", () => {
    assert.equal(linkifyResourceUris("hello world"), "hello world");
    assert.equal(linkifyResourceUris("visit https://example.com"), "visit https://example.com");
  });
});

describe("resourceUriShortLabel", () => {
  it("returns artifact id or file name", () => {
    assert.equal(resourceUriShortLabel("artifact://ns/a/s/draft"), "draft");
    assert.equal(resourceUriShortLabel("file://ns/memory-brand"), "memory-brand");
    assert.equal(resourceUriShortLabel("not-a-uri"), "not-a-uri");
  });
});
