import React, { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import {
  TalonChatComposer,
  type TalonChatComposerImageAttachment,
  type TalonChatComposerVariant,
} from "./TalonChatComposer";

const existingImageUrl = new URL("../../../../docs/pr/hello-world-sightline.png", import.meta.url).href;
const previewImageUrl =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";

const meta = {
  title: "Talon Chat/TalonChatComposer",
  component: TalonChatComposer,
  tags: ["autodocs"],
  args: {
    placeholder: "Ask Talon to perform a task...",
    variant: "panel",
    autoFocus: false,
    imageUploadEnabled: true,
    canSubmit: false,
  },
  argTypes: {
    variant: {
      control: "radio",
      options: ["panel", "compact", "expanded", "inline"],
    },
  },
  render: (args) => {
    const [value, setValue] = useState(args.value ?? "");
    return (
      <div
        style={{
          minHeight: "100vh",
          boxSizing: "border-box",
          padding: "48px 40px",
          background: "#fff",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <div style={{ width: "min(100%, 700px)", margin: "0 auto" }}>
          <TalonChatComposer
            {...args}
            value={value}
            onValueChange={setValue}
            onSubmit={() => {}}
          />
        </div>
      </div>
    );
  },
} satisfies Meta<typeof TalonChatComposer>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Panel: Story = {
  args: {
    variant: "panel",
  },
};

export const Compact: Story = {
  args: {
    variant: "compact",
    placeholder: "Ask Talon...",
  },
};

export const Expanded: Story = {
  args: {
    variant: "expanded",
    canSubmit: false,
    placeholder: "Ask for follow-up changes",
    value: "",
  },
};

export const ExpandedAttachmentMenuOpen: Story = {
  args: {
    variant: "expanded",
    canSubmit: false,
    placeholder: "Ask for follow-up changes",
    value: "",
  },
  play: async ({ canvasElement }) => {
    const button = canvasElement.querySelector<HTMLButtonElement>('button[aria-label="Open attachment menu"]');
    button?.click();
  },
};

export const Inline: Story = {
  args: {
    variant: "inline",
    canSubmit: true,
    value: "Summarize this channel",
  },
};

export const AllLayouts: Story = {
  render: (args) => {
    const variants: TalonChatComposerVariant[] = ["panel", "compact", "expanded", "inline"];
    const [values, setValues] = useState<Record<TalonChatComposerVariant, string>>({
      panel: "",
      compact: "",
      expanded: "Launch the agent team",
      inline: "Summarize the thread",
    });

    return (
      <div style={{ minHeight: 560, padding: "48px 40px", background: "#f4f4f5", color: "#18181b" }}>
        <div style={{ width: "min(100%, 980px)", margin: "0 auto", display: "grid", gap: 28 }}>
          {variants.map((variant) => (
            <section
              key={variant}
              style={{
                display: "grid",
                gridTemplateColumns: "120px minmax(0, 1fr)",
                alignItems: "center",
                gap: 24,
              }}
            >
              <div style={{ fontFamily: "Inter, ui-sans-serif, system-ui", fontSize: 13, fontWeight: 700 }}>
                {variant}
              </div>
              <TalonChatComposer
                {...args}
                variant={variant}
                value={values[variant]}
                canSubmit={Boolean(values[variant].trim())}
                onValueChange={(value) => setValues((current) => ({ ...current, [variant]: value }))}
                onSubmit={() => {}}
              />
            </section>
          ))}
        </div>
      </div>
    );
  },
};

export const ImageInputEnabled: Story = {};

export const AttachmentMenuOpen: Story = {
  args: {
    previewTopPadding: 320,
  },
  play: async ({ canvasElement }) => {
    const button = canvasElement.querySelector<HTMLButtonElement>('button[aria-label="Open attachment menu"]');
    button?.click();
  },
};

export const ExistingImageAttachment: Story = {
  args: {
    canSubmit: true,
    imageAttachments: [
      {
        id: "existing-storybook-image",
        filename: "hello-world-sightline.png",
        previewUrl: existingImageUrl,
        status: "ready",
      },
    ] satisfies TalonChatComposerImageAttachment[],
  },
};

export const WithImageAttachment: Story = {
  args: {
    value: "What is this?",
    canSubmit: true,
    imageAttachments: [
      {
        id: "storybook-image",
        filename: "favicon.PNG",
        previewUrl: previewImageUrl,
        status: "ready",
      },
    ] satisfies TalonChatComposerImageAttachment[],
  },
};
