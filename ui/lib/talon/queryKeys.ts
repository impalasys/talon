export type TalonQueryScope = {
  gatewayUrl: string;
  authToken?: string | null;
};

function authFingerprint(token?: string | null) {
  if (!token) return 'anon';
  let hash = 0;
  for (let index = 0; index < token.length; index += 1) {
    hash = (hash * 31 + token.charCodeAt(index)) >>> 0;
  }
  return `auth:${token.length}:${hash.toString(36)}`;
}

function scopeKey(scope: TalonQueryScope) {
  return {
    gatewayUrl: scope.gatewayUrl || '',
    auth: authFingerprint(scope.authToken),
  };
}

export const talonQueryKeys = {
  all: (scope: TalonQueryScope) => ['talon', scopeKey(scope)] as const,
  namespaces: (scope: TalonQueryScope, parent: string) => [...talonQueryKeys.all(scope), 'namespaces', parent] as const,
  resources: (scope: TalonQueryScope, ns: string, kind: string) =>
    [...talonQueryKeys.all(scope), 'resources', ns, kind] as const,
  resource: (scope: TalonQueryScope, ns: string, kind: string, name: string) =>
    [...talonQueryKeys.all(scope), 'resource', ns, kind, name] as const,
  sessions: (scope: TalonQueryScope, ns: string, agent: string) =>
    [...talonQueryKeys.all(scope), 'sessions', ns, agent] as const,
  knowledge: (scope: TalonQueryScope, ns: string, agent: string) =>
    [...talonQueryKeys.all(scope), 'knowledge', ns, agent] as const,
};
