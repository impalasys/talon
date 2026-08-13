import assert from "node:assert/strict";
import test from "node:test";
import { splitYamlFrontmatter } from "./yamlFrontmatter.ts";

test("separates leading YAML frontmatter from Markdown", () => {
  assert.deepEqual(
    splitYamlFrontmatter("---\ntitle: Draft\ntags:\n  - review\n---\n# Document"),
    { raw: "title: Draft\ntags:\n  - review", body: "# Document" },
  );
});

test("leaves non-frontmatter Markdown unchanged", () => {
  assert.deepEqual(splitYamlFrontmatter("---\nNot a closed frontmatter block"), {
    raw: null,
    body: "---\nNot a closed frontmatter block",
  });
});
