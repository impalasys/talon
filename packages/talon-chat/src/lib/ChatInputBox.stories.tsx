import React, { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ChatInputBox, type ChatInputImageAttachment } from "./ChatInputBox";

const previewImageUrl =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";

const meta = {
  title: "Talon Chat/ChatInputBox",
  component: ChatInputBox,
  tags: ["autodocs"],
  args: {
    placeholder: "Ask Talon to perform a task...",
    autoFocus: false,
    imageUploadEnabled: true,
    canSubmit: false,
    previewTopPadding: 40,
  },
  argTypes: {
    previewTopPadding: { table: { disable: true } },
  },
  render: (args) => {
    const [value, setValue] = useState(args.value ?? "");
    const { previewTopPadding, ...inputArgs } = args;
    return (
      <div style={{ padding: `${previewTopPadding ?? 40}px 40px 40px`, background: "#fff" }}>
        <div style={{ width: "min(100%, 960px)", margin: "0 auto" }}>
          <ChatInputBox
            {...inputArgs}
            value={value}
            onValueChange={setValue}
            onSubmit={() => {}}
          />
        </div>
      </div>
    );
  },
} satisfies Meta<typeof ChatInputBox>;

export default meta;
type Story = StoryObj<typeof meta>;

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
    ] satisfies ChatInputImageAttachment[],
  },
};
