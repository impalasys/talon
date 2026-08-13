export type ResourceUriKind = "artifact" | "file";

export type ParsedResourceUri =
  | {
      kind: "artifact";
      uri: string;
      namespace: string;
      agent: string;
      sessionId: string;
      artifactId: string;
    }
  | {
      kind: "file";
      uri: string;
      namespace: string;
      fileName: string;
    };

const TRAILING_PROSE_PUNCTUATION = /[.,;)\]]+$/;

/** Bare resource URI matchers (scheme through first whitespace/markdown delimiter). */
const BARE_RESOURCE_URI_RE = /(?:artifact|file):\/\/[^\s<>"'`)\]]+/g;

/**
 * Markdown href prefix that survives Streamdown's rehype-harden.
 *
 * Harden hard-blocks `file:` and cannot parse `artifact://…` as a WHATWG URL,
 * so it replaces those links with a "[blocked]" indicator. Hash-only hrefs are
 * always allowed, so we encode the real URI in the fragment.
 */
export const RESOURCE_MARKDOWN_HREF_PREFIX = "#talon-resource/";

function stripTrailingProsePunctuation(value: string): string {
  return value.replace(TRAILING_PROSE_PUNCTUATION, "");
}

function isValidUriSegment(segment: string): boolean {
  if (!segment || segment.trim().length === 0) return false;
  if (segment.includes("/") || segment.includes("\0")) return false;
  for (const char of segment) {
    const code = char.charCodeAt(0);
    if (code < 0x20 || code === 0x7f) return false;
  }
  return true;
}

/**
 * Parse a Talon resource URI (`artifact://…` or `file://…`).
 * Returns null for OS-style paths like `file:///tmp/foo` (wrong segment count).
 */
export function parseResourceUri(value: string): ParsedResourceUri | null {
  const trimmed = value.trim();
  if (!trimmed) return null;

  if (trimmed.startsWith("artifact://")) {
    const rest = trimmed.slice("artifact://".length);
    const parts = rest.split("/");
    if (parts.length !== 4) return null;
    const [namespace, agent, sessionId, artifactId] = parts;
    if (
      !isValidUriSegment(namespace) ||
      !isValidUriSegment(agent) ||
      !isValidUriSegment(sessionId) ||
      !isValidUriSegment(artifactId)
    ) {
      return null;
    }
    return {
      kind: "artifact",
      uri: `artifact://${namespace}/${agent}/${sessionId}/${artifactId}`,
      namespace,
      agent,
      sessionId,
      artifactId,
    };
  }

  if (trimmed.startsWith("file://")) {
    const rest = trimmed.slice("file://".length);
    const parts = rest.split("/");
    // Reject file:///tmp/foo (empty first segment) and multi-segment OS paths.
    if (parts.length !== 2) return null;
    const [namespace, fileName] = parts;
    if (!isValidUriSegment(namespace) || !isValidUriSegment(fileName)) {
      return null;
    }
    return {
      kind: "file",
      uri: `file://${namespace}/${fileName}`,
      namespace,
      fileName,
    };
  }

  return null;
}

export function isResourceUri(value: string): boolean {
  return parseResourceUri(value) !== null;
}

/** Encode a resource URI as a markdown/href value that rehype-harden will not block. */
export function toResourceMarkdownHref(uri: string): string {
  return `${RESOURCE_MARKDOWN_HREF_PREFIX}${encodeURIComponent(uri)}`;
}

/**
 * Recover a resource URI from a markdown href or data attribute.
 * Accepts hash-encoded hrefs, raw artifact:// / file://, and null otherwise.
 */
export function resourceUriFromHref(href: string | null | undefined): string | null {
  if (!href) return null;
  const trimmed = href.trim();
  if (!trimmed) return null;

  if (trimmed.startsWith(RESOURCE_MARKDOWN_HREF_PREFIX)) {
    try {
      const decoded = decodeURIComponent(trimmed.slice(RESOURCE_MARKDOWN_HREF_PREFIX.length));
      return parseResourceUri(decoded)?.uri ?? null;
    } catch {
      return null;
    }
  }

  return parseResourceUri(trimmed)?.uri ?? null;
}

function isInsideCodeFence(markdown: string, index: number): boolean {
  // Count fence opens before index; odd count means inside a fenced block.
  const before = markdown.slice(0, index);
  let fenceCount = 0;
  const fenceRe = /^[ \t]{0,3}(`{3,}|~{3,})/gm;
  let match: RegExpExecArray | null;
  while ((match = fenceRe.exec(before)) !== null) {
    fenceCount += 1;
  }
  return fenceCount % 2 === 1;
}

function isInsideInlineCode(markdown: string, index: number): boolean {
  const lineStart = markdown.lastIndexOf("\n", index - 1) + 1;
  const before = markdown.slice(lineStart, index);
  const backtickRunRe = /`+/g;
  let activeRunLength: number | null = null;
  let match: RegExpExecArray | null;
  while ((match = backtickRunRe.exec(before)) !== null) {
    const runLength = match[0].length;
    if (activeRunLength === null) {
      activeRunLength = runLength;
    } else if (runLength === activeRunLength) {
      activeRunLength = null;
    }
  }
  return activeRunLength !== null;
}

function isInsideCode(markdown: string, index: number): boolean {
  return isInsideCodeFence(markdown, index) || isInsideInlineCode(markdown, index);
}

function isAlreadyLinked(markdown: string, start: number, end: number): boolean {
  // Skip only when this span is already a markdown link destination: ](uri)
  // Do not treat parenthesized prose like (file://ns/name) as linked.
  const after = markdown.slice(end);
  if (/^\s*\)/.test(after)) {
    const before = markdown.slice(0, start);
    if (/\]\(\s*$/.test(before)) {
      return true;
    }
  }
  // Already a full markdown link of the form [uri](uri) where the match is the label
  const before = markdown.slice(Math.max(0, start - 1), start);
  if (before === "[") {
    const afterLabel = markdown.slice(end);
    if (/^\]\([^)]*\)/.test(afterLabel)) {
      return true;
    }
  }
  return false;
}

/**
 * Rewrite markdown link destinations that use raw artifact:// or file:// hrefs
 * into harden-safe hash hrefs. Leaves labels and non-resource links alone.
 */
function rewriteResourceLinkDestinations(markdown: string): string {
  // Match ](artifact://…) and ](file://…) destinations.
  const destRe = /\]\(((?:artifact|file):\/\/[^)\s]+)\)/g;
  let result = "";
  let lastIndex = 0;
  let match: RegExpExecArray | null;
  destRe.lastIndex = 0;

  while ((match = destRe.exec(markdown)) !== null) {
    const full = match[0];
    const dest = match[1];
    const start = match.index;

    if (isInsideCode(markdown, start)) {
      continue;
    }

    const parsed = parseResourceUri(dest);
    if (!parsed) {
      continue;
    }

    result += markdown.slice(lastIndex, start);
    result += `](${toResourceMarkdownHref(parsed.uri)})`;
    lastIndex = start + full.length;
  }

  result += markdown.slice(lastIndex);
  return result;
}

/**
 * Fence-aware preprocessor: convert bare artifact:// and file:// URIs into
 * markdown links, and rewrite existing resource link destinations to
 * harden-safe hash hrefs.
 */
export function linkifyResourceUris(markdown: string): string {
  if (!markdown || (!markdown.includes("artifact://") && !markdown.includes("file://"))) {
    return markdown;
  }

  // First rewrite already-authored [label](artifact://…) / [label](file://…).
  let working = rewriteResourceLinkDestinations(markdown);

  let result = "";
  let lastIndex = 0;
  BARE_RESOURCE_URI_RE.lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = BARE_RESOURCE_URI_RE.exec(working)) !== null) {
    const raw = match[0];
    const start = match.index;
    const end = start + raw.length;

    if (isInsideCode(working, start) || isAlreadyLinked(working, start, end)) {
      continue;
    }

    const rawWithoutTrailing = stripTrailingProsePunctuation(raw);
    const parsed = parseResourceUri(rawWithoutTrailing);
    if (!parsed) {
      continue;
    }

    // Preserve prose punctuation outside the generated link.
    const actualTrailing = raw.slice(rawWithoutTrailing.length);

    result += working.slice(lastIndex, start);
    result += `[${parsed.uri}](${toResourceMarkdownHref(parsed.uri)})`;
    result += actualTrailing;
    lastIndex = end;
  }

  result += working.slice(lastIndex);
  return result;
}

export type ResourceViewModel = {
  kind: ResourceUriKind;
  uri: string;
  title: string;
  mediaType: string;
  content?: Uint8Array | string;
  signedUrl?: string;
  /** Immutable CAS/object-store key, if supplied by the gateway. */
  objectKey?: string;
  path?: string;
  sessionId?: string;
  agent?: string;
};

/** Short display label for a resource URI (artifact id or file name). */
export function resourceUriShortLabel(uri: string): string {
  const parsed = parseResourceUri(uri);
  if (!parsed) return uri;
  return parsed.kind === "artifact" ? parsed.artifactId : parsed.fileName;
}
