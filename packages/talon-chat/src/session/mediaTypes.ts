export function isTextReadableMediaType(mediaType: string): boolean {
  const base = mediaType.split(";", 1)[0]?.trim().toLowerCase() ?? "";
  return base.startsWith("text/")
    || [
      "application/json",
      "application/yaml",
      "application/x-yaml",
      "application/toml",
      "application/xml",
      "application/javascript",
      "application/x-javascript",
    ].includes(base)
    || base.endsWith("+json")
    || base.endsWith("+xml");
}
