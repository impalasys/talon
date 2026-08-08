import type { CopilotMessage } from "../lib/chatTimeline";
import { editableMessageContent } from "./messageEditing";

export async function copyMessageContent(message: CopilotMessage) {
  const content = editableMessageContent(message);
  if (!content.trim()) return;
  try {
    if (!navigator.clipboard?.writeText) throw new Error("Clipboard API is unavailable.");
    await navigator.clipboard.writeText(content);
  } catch {
    const selection = window.getSelection();
    const textArea = document.createElement("textarea");
    textArea.value = content;
    textArea.setAttribute("readonly", "");
    textArea.style.position = "fixed";
    textArea.style.left = "-9999px";
    document.body.appendChild(textArea);
    textArea.select();
    document.execCommand("copy");
    document.body.removeChild(textArea);
    selection?.removeAllRanges();
  }
}
