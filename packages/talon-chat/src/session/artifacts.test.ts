import assert from "node:assert/strict";
import test from "node:test";
import {
  artifactUriFor,
  formatArtifactBytes,
  mergeSessionArtifacts,
  normalizeSessionArtifact,
} from "./artifacts.ts";

test("normalizes generated Artifact fields and constructs the owning session URI", () => {
  const artifact = normalizeSessionArtifact({
    id: "draft",
    title: "Launch draft",
    media_type: "text/markdown",
    object_ref: { size_bytes: "2048" },
    created_at: "1777755592000000",
  });
  assert.deepEqual(artifact, {
    id: "draft",
    title: "Launch draft",
    mediaType: "text/markdown",
    sizeBytes: "2048",
    createdAt: "1777755592000000",
  });
  assert.equal(
    artifactUriFor({ ns: "Tenant:acme", agent: "writer", sessionId: "sess-1" }, artifact!.id),
    "artifact://Tenant:acme/writer/sess-1/draft",
  );
});

test("deduplicates paginated artifacts and displays newest artifacts first", () => {
  const merged = mergeSessionArtifacts(
    [{ id: "old", title: "Old", mediaType: "text/plain", createdAt: 1_700_000_000_000_000 }],
    [
      { id: "old", title: "Old revision", mediaType: "text/plain", createdAt: 1_700_000_000_000_000 },
      { id: "new", title: "New", mediaType: "text/plain", createdAt: 1_800_000_000_000_000 },
    ],
  );
  assert.deepEqual(merged.map((artifact) => [artifact.id, artifact.title]), [
    ["new", "New"],
    ["old", "Old revision"],
  ]);
});

test("formats byte sizes for rail metadata", () => {
  assert.equal(formatArtifactBytes(999), "999 B");
  assert.equal(formatArtifactBytes("2048"), "2.0 KB");
  assert.equal(formatArtifactBytes(undefined), null);
});
