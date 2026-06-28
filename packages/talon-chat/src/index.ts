export {
  TalonSession,
  TalonCopilot,
  type GatewayClientLike,
  type TalonSessionCommand,
  type TalonSessionCommandTarget,
  type TalonSessionProps,
  type TalonCopilotProps,
  type TalonChatObjectRef,
  type TalonImageUploadContext,
  type TalonImageUploadResult,
} from "./TalonSession";
export {
  TalonChannel,
  useTalonChannelMessages,
  type ChannelGatewayClientLike,
  type ChannelMessage,
  type TalonChannelCommand,
  type TalonChannelCommandTarget,
  type TalonChannelProps,
  type UseTalonChannelMessagesOptions,
  type UseTalonChannelMessagesResult,
} from "./TalonChannel";
export {
  type TalonBuiltInCommandName,
  type TalonChatCommand,
  type TalonChatCommandContext,
} from "./lib/commands";
export {
  type AssistantTimelineItem,
  type CopilotMessage,
  type UsageSummary,
} from "./lib/chatTimeline";
export {
  TalonChatComposer,
  type TalonChatComposerCommandMenuItem,
  type TalonChatComposerImageAttachment,
  type TalonChatComposerProps,
  type TalonChatComposerVariant,
} from "./lib/TalonChatComposer";
export { buildGatewayHeaders, normalizeGatewayUrl, applyGatewayAuthorizationHeader } from "./lib/grpc";
