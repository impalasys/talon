export type YamlFrontmatter = {
  body: string;
  raw: string | null;
};

/** Separates a leading YAML frontmatter block from a Markdown document. */
export function splitYamlFrontmatter(source: string): YamlFrontmatter {
  const normalized = source.startsWith("\uFEFF") ? source.slice(1) : source;
  const opening = /^(?:---)[ \t]*(?:\r?\n)/.exec(normalized);
  if (!opening) return { body: source, raw: null };

  const closing = /^(?:---|\.\.\.)[ \t]*\r?$/m;
  closing.lastIndex = opening[0].length;
  const match = closing.exec(normalized.slice(opening[0].length));
  if (!match) return { body: source, raw: null };

  const contentEnd = opening[0].length + match.index;
  const bodyStart = contentEnd + match[0].length;
  return {
    raw: normalized.slice(opening[0].length, contentEnd).trim(),
    body: normalized.slice(bodyStart).replace(/^\r?\n/, ""),
  };
}
