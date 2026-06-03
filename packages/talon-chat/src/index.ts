export { TalonCopilot, type GatewayClientLike, type TalonCopilotProps } from "./TalonCopilot";
export {
  TalonChannel,
  useTalonChannelMessages,
  type ChannelMessage,
  type TalonChannelProps,
  type UseTalonChannelMessagesOptions,
  type UseTalonChannelMessagesResult,
} from "./TalonChannel";
export {
  type AssistantTimelineItem,
  type CopilotMessage,
  type UsageSummary,
} from "./lib/chatTimeline";
export { buildGatewayHeaders, normalizeGatewayUrl, applyGatewayAuthorizationHeader } from "./lib/grpc";
