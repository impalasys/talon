import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { isTextReadableMediaType } from "./mediaTypes.ts";

describe("isTextReadableMediaType", () => {
  it("accepts the structured text media types supported by resource reads", () => {
    for (const mediaType of [
      "text/plain; charset=utf-8",
      "application/json; charset=utf-8",
      "application/problem+json",
      "application/yaml",
      "application/xml",
    ]) {
      assert.equal(isTextReadableMediaType(mediaType), true, mediaType);
    }
  });

  it("rejects binary attachments", () => {
    assert.equal(isTextReadableMediaType("application/pdf"), false);
  });
});
