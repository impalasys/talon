export { TalonCopilot, type GatewayClientLike, type TalonCopilotProps } from "./TalonCopilot";
export {
  type AssistantTimelineItem,
  type CopilotMessage,
  type UsageSummary,
} from "./lib/chatTimeline";
export { buildGatewayHeaders, normalizeGatewayUrl, applyGatewayAuthorizationHeader } from "./lib/grpc";
