function hasAuthorizationScheme(value) {
  return /^(Basic|Bearer)\s+/i.test(value);
}

function buildAuthorizationHeader(authToken) {
  if (!authToken) return undefined;
  const normalizedToken = authToken.trim();
  if (!normalizedToken) return undefined;
  return hasAuthorizationScheme(normalizedToken)
    ? normalizedToken
    : `Bearer ${normalizedToken}`;
}

function createService() {
  return new Proxy({}, {
    get(target, prop) {
      if (!target[prop]) {
        target[prop] = jest.fn();
      }
      return target[prop];
    },
  });
}

function createTalonClient() {
  return {
    auth: createService(),
    cas: createService(),
    channels: createService(),
    connectors: createService(),
    knowledge: createService(),
    namespaces: createService(),
    resources: createService(),
    search: createService(),
    sessions: createService(),
    workflows: createService(),
  };
}

module.exports = {
  buildAuthorizationHeader,
  createTalonClient,
  data: {
    MessageRole: {
      ROLE_ASSISTANT: "ROLE_ASSISTANT",
      ROLE_USER: "ROLE_USER",
      ROLE_SYSTEM: "ROLE_SYSTEM",
    },
    SessionMessagePartType: {
      TEXT: "SESSION_MESSAGE_PART_TYPE_TEXT",
      REASONING: "SESSION_MESSAGE_PART_TYPE_REASONING",
      TOOL_CALL: "SESSION_MESSAGE_PART_TYPE_TOOL_CALL",
      TOOL_RESULT: "SESSION_MESSAGE_PART_TYPE_TOOL_RESULT",
      USAGE: "SESSION_MESSAGE_PART_TYPE_USAGE",
      ERROR: "SESSION_MESSAGE_PART_TYPE_ERROR",
      IMAGE: "SESSION_MESSAGE_PART_TYPE_IMAGE",
    },
  },
};
