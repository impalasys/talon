export function toolResultPartText(response: unknown): string {
  const content = (response as { content?: { case?: unknown; value?: unknown } } | undefined)
    ?.content;
  return content?.case === "text" && typeof content.value === "string" ? content.value : "";
}
