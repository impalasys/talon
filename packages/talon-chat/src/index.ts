export {
  TalonSession,
  TalonCopilot,
  type ArtifactServiceClientLike,
  type FileServiceClientLike,
  type GatewayClientLike,
  type ResourceViewModel,
  type TalonSessionCommand,
  type TalonSessionCommandTarget,
  type TalonSessionProps,
  type TalonSessionMessageEditContext,
  type TalonSessionSubmitContext,
  type TalonCopilotProps,
  type TalonChatObjectRef,
  type TalonAttachmentUploadContext,
  type TalonAttachmentUploadResult,
  type TalonSessionPendingAttachment,
  type TalonImageUploadContext,
  type TalonImageUploadResult,
  type TalonSessionPendingImageAttachment,
} from "./TalonSession";
export {
  isResourceUri,
  linkifyResourceUris,
  parseResourceUri,
  RESOURCE_MARKDOWN_HREF_PREFIX,
  resourceUriFromHref,
  resourceUriShortLabel,
  toResourceMarkdownHref,
  type ParsedResourceUri,
  type ResourceUriKind,
} from "./lib/resourceUris";
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
  type TalonChatComposerAttachment,
  type TalonChatComposerCommandMenuItem,
  type TalonChatComposerImageAttachment,
  type TalonChatComposerProps,
  type TalonChatComposerVariant,
} from "./lib/TalonChatComposer";
export { buildGatewayHeaders, normalizeGatewayUrl, applyGatewayAuthorizationHeader } from "./lib/grpc";
