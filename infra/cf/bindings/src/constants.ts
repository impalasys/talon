export const DEFAULT_SCHEDULER_AUTH_TOKEN = "cloudflare-local-scheduler-token";
export const TEXT_JSON = { "content-type": "application/json" };

export const TOPICS = {
  sessionDispatch: "talon.session.dispatch",
  resourceLifecycle: "talon.resource.lifecycle",
  sessionControl: "talon.session.control",
  sessionPartsPrefix: "talon.session.parts.",
} as const;
