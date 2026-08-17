import React from "react";

export type MessageImage = { id: string; src?: string; label: string };

export function MessageImages({ images, hasContent }: { images: MessageImage[]; hasContent: boolean }) {
  if (images.length === 0) return null;
  const border = "1px solid var(--talon-chat-image-border, rgba(212,212,216,0.86))";
  return <div style={{ display: "flex", flexWrap: "wrap", gap: 8, marginTop: hasContent ? 10 : 0 }}>
    {images.map((image) => image.src ? <img key={image.id} src={image.src} alt={image.label} style={{ width: 132, maxWidth: "100%", aspectRatio: "1 / 1", objectFit: "cover", borderRadius: 8, border }} /> : <div key={image.id} title={image.label} style={{ maxWidth: "100%", borderRadius: 8, border, padding: "0.45rem 0.6rem", fontSize: 12, lineHeight: 1.3, color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))", overflowWrap: "anywhere" }}>{image.label}</div>)}
  </div>;
}
