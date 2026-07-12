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
  data: {},
};
