export const SYSTEM_NAMESPACE = 'Sys';

export type SelectionType =
  | 'namespace'
  | 'agent'
  | 'session'
  | 'channel'
  | 'channel-subscription'
  | 'workflow'
  | 'schedule'
  | 'template'
  | 'deployment'
  | 'deployment-replica'
  | 'connector-class'
  | 'connector'
  | 'sandbox-class'
  | 'sandbox-policy'
  | 'sandbox'
  | 'mcp-server'
  | 'knowledge';

export type Selection = {
  type: SelectionType;
  ns: string;
  agent?: string;
  channel?: string;
  sessionId?: string;
  resourceName?: string;
  fullPath: string;
};

export const RESOURCE_KIND_BY_SELECTION: Partial<Record<SelectionType, string>> = {
  agent: 'Agent',
  channel: 'Channel',
  'channel-subscription': 'ChannelSubscription',
  workflow: 'Workflow',
  schedule: 'Schedule',
  template: 'Template',
  deployment: 'Deployment',
  'deployment-replica': 'DeploymentReplica',
  'connector-class': 'ConnectorClass',
  connector: 'Connector',
  'sandbox-class': 'SandboxClass',
  'sandbox-policy': 'SandboxPolicy',
  sandbox: 'Sandbox',
  'mcp-server': 'McpServer',
  knowledge: 'Knowledge',
};

export function areSelectionsEqual(left: Selection | null, right: Selection | null) {
  if (left === right) return true;
  if (!left || !right) return false;
  return (
    left.type === right.type &&
    left.ns === right.ns &&
    left.agent === right.agent &&
    left.channel === right.channel &&
    left.sessionId === right.sessionId &&
    left.resourceName === right.resourceName
  );
}

export function namespaceAncestors(ns: string) {
  if (!ns) return [''];
  const parts = ns.split(':');
  const ancestors = [''];
  for (let i = 0; i < parts.length; i++) {
    ancestors.push(parts.slice(0, i + 1).join(':'));
  }
  return ancestors;
}

export function namespaceResolutionAncestry(ns: string) {
  if (!ns) return [];
  const parts = ns.split(':').filter(Boolean);
  return parts.map((_, index) => parts.slice(0, parts.length - index).join(':'));
}

export function selectionExpansionIds(selection: Selection | null) {
  if (!selection?.ns) return [];
  const ids = namespaceAncestors(selection.ns);
  if (selection.agent) {
    ids.push(`${selection.ns}:${selection.agent}`);
  }
  if (selection.channel) {
    ids.push(`${selection.ns}:channel:${selection.channel}`);
  }
  return ids;
}

export function selectionFromSearchParams(searchParams: URLSearchParams): Selection | null {
  const type = searchParams.get('type');
  const ns = searchParams.get('ns');
  const agent = searchParams.get('agent');
  const channel = searchParams.get('channel');
  const sessionId = searchParams.get('session');
  const resourceName = searchParams.get('name');

  if (type === 'template' && resourceName) {
    const namespace = ns || SYSTEM_NAMESPACE;
    return {
      type: 'template',
      ns: namespace,
      resourceName,
      fullPath: `${namespace}:template:${resourceName}`,
    };
  }

  if (!ns) return null;

  if (sessionId && agent) {
    return {
      type: 'session',
      ns,
      agent,
      sessionId,
      fullPath: `${ns}/${agent}/${sessionId}`,
    };
  }

  if (agent) {
    return {
      type: 'agent',
      ns,
      agent,
      fullPath: `${ns}/${agent}`,
    };
  }

  if (type === 'channel-subscription' && channel && resourceName) {
    return {
      type: 'channel-subscription',
      ns,
      channel,
      resourceName,
      fullPath: `${ns}:channel:${channel}:subscription:${resourceName}`,
    };
  }

  if (type === 'channel' && (resourceName || channel)) {
    const channelName = resourceName || channel || '';
    return {
      type: 'channel',
      ns,
      channel: channelName,
      resourceName: channelName,
      fullPath: `${ns}:channel:${channelName}`,
    };
  }

  if (type === 'schedule' && resourceName) {
    return {
      type: 'schedule',
      ns,
      resourceName,
      fullPath: `${ns}:schedule:${resourceName}`,
    };
  }

  if (type === 'mcp-server' && resourceName) {
    return {
      type: 'mcp-server',
      ns,
      resourceName,
      fullPath: `${ns}:mcp-server:${resourceName}`,
    };
  }

  if (type === 'knowledge' && resourceName) {
    return {
      type: 'knowledge',
      ns,
      resourceName,
      fullPath: `${ns}:knowledge:${resourceName}`,
    };
  }

  if (
    (
      type === 'deployment' ||
      type === 'workflow' ||
      type === 'deployment-replica' ||
      type === 'connector-class' ||
      type === 'connector' ||
      type === 'sandbox-class' ||
      type === 'sandbox-policy' ||
      type === 'sandbox'
    ) &&
    resourceName
  ) {
    return {
      type,
      ns,
      resourceName,
      fullPath: `${ns}:${type}:${resourceName}`,
    };
  }

  return {
    type: 'namespace',
    ns,
    fullPath: ns,
  };
}

export function buildSearchParams(isConnected: boolean, selection: Selection | null, currentSearchParams?: URLSearchParams) {
  const params = new URLSearchParams();
  const historyPageSize = currentSearchParams?.get('historyPageSize');
  const root = currentSearchParams?.get('root');

  if (isConnected) {
    params.set('connected', 'true');
  }

  // URL ownership:
  // - root: connection-form namespace prefill for scoped auth/API-key exchange.
  // - ns/type/agent/...: explorer resource selection only.
  // Do not collapse root into ns or hydrate connection settings from ns.
  if (root?.trim()) {
    params.set('root', root.trim());
  }

  if (historyPageSize && /^\d+$/.test(historyPageSize) && Number(historyPageSize) > 0) {
    params.set('historyPageSize', historyPageSize);
  }

  if (selection?.ns) params.set('ns', selection.ns);
  if (selection?.type) params.set('type', selection.type);
  if (selection?.agent) params.set('agent', selection.agent);
  if (selection?.channel) params.set('channel', selection.channel);
  if (selection?.sessionId) params.set('session', selection.sessionId);
  if (selection?.resourceName) params.set('name', selection.resourceName);

  return params;
}

export function getSelectionTitle(selection: Selection | null) {
  if (!selection) return 'No Resource Selected';
  if (selection.type === 'namespace') return selection.ns;
  if (selection.type === 'agent') return selection.agent || 'Agent';
  if (selection.type === 'session') return selection.sessionId || 'Session';
  if (selection.type === 'channel') return selection.channel || selection.resourceName || 'Channel';
  return selection.resourceName || selection.type;
}

export function getSelectionSubtitle(selection: Selection | null) {
  if (!selection) return 'Select a namespace, agent, deployment, sandbox, template, MCP server, or session.';
  if (selection.type === 'namespace') return 'Namespace';
  if (selection.type === 'agent') return `${selection.ns} / Agent`;
  if (selection.type === 'session') return `${selection.ns} / ${selection.agent}`;
  if (selection.type === 'channel') return `${selection.ns} / Channel`;
  if (selection.type === 'channel-subscription') return `${selection.ns} / ${selection.channel} / ChannelSubscription`;
  if (selection.type === 'workflow') return `${selection.ns} / Workflow`;
  if (selection.type === 'schedule') return `${selection.ns} / Schedule`;
  if (selection.type === 'mcp-server') return `${selection.ns} / MCPServer`;
  if (selection.type === 'knowledge') return `${selection.ns} / Knowledge`;
  if (selection.type === 'template') return `${selection.ns} / Template`;
  if (selection.type === 'deployment') return `${selection.ns} / Deployment`;
  if (selection.type === 'deployment-replica') return `${selection.ns} / DeploymentReplica`;
  if (selection.type === 'connector-class') return `${selection.ns} / ConnectorClass`;
  if (selection.type === 'connector') return `${selection.ns} / Connector`;
  if (selection.type === 'sandbox-class') return `${selection.ns} / SandboxClass`;
  if (selection.type === 'sandbox-policy') return `${selection.ns} / SandboxPolicy`;
  if (selection.type === 'sandbox') return `${selection.ns} / Sandbox`;
  return 'Sys / MCPServer';
}
