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
  },
  render: (args) => {
    const [value, setValue] = useState(args.value ?? "");
    return (
      <div style={{ padding: 40, background: "#fff" }}>
        <div style={{ width: "min(100%, 960px)", margin: "0 auto" }}>
          <ChatInputBox
            {...args}
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
