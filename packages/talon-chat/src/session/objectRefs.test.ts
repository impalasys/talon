import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  normalizeObjectRef,
  objectRefContentEncoding,
  objectRefFromPart,
  objectRefFromValue,
  objectRefSizeBytes,
} from "./objectRefs.ts";

describe("session object references", () => {
  it("normalizes snake_case and bigint metadata without losing the key", () => {
    const ref = normalizeObjectRef({
      key: "cas/demo/result.txt",
      media_type: "text/plain",
      size_bytes: 12n,
      content_encoding: "gzip",
    });

    assert.deepEqual(ref, {
      key: "cas/demo/result.txt",
      mediaType: "text/plain",
      sizeBytes: 12,
      sha256: "",
      filename: "",
      metadata: {},
    });
    assert.equal(objectRefSizeBytes({ key: ref.key, sizeBytes: "bad" }), 0);
    assert.equal(objectRefContentEncoding({ key: ref.key, content_encoding: "gzip" }), "gzip");
  });

  it("finds direct, nested, and tool-output content-part references", () => {
    const ref = { key: "cas/demo/nested.json", mediaType: "application/json" };
    assert.deepEqual(objectRefFromValue({ object_ref: ref }), ref);
    assert.deepEqual(objectRefFromValue({
      tool_output: {
        content_parts: [{ type: "text", text: "preview" }, { object: ref }],
      },
    }), ref);
    assert.deepEqual(objectRefFromPart({
      payloadJson: JSON.stringify({ tool_output: { contentParts: [{ objectRef: ref }] } }),
    }), ref);
  });
});
