import type { Selection, SelectionType } from '../../lib/selection';
import { namespaceAncestors, SYSTEM_NAMESPACE } from '../../lib/selection';
import {
  channelFromResource,
  channelSubscriptionFromResource,
  fileFromResource,
  namespaceLabel,
  scheduleFromResource,
  type ResourceEnvelope,
} from '../../lib/talon/resourceMappers';
import { RESOURCE_DESCRIPTORS } from '../../lib/talon/resourceDescriptors';
import type { ExplorerQueries } from '../../hooks/useExplorerQueries';

export type ExplorerNode = {
  id: string;
  name: string;
  selection: Selection;
  badge?: string;
  children: ExplorerNode[];
};

export type ExplorerGroup = {
  id: string;
  title: string;
  nodes: ExplorerNode[];
};

function compareByName(left: ExplorerNode, right: ExplorerNode) {
  return left.name.localeCompare(right.name);
}

function parseSessionTimestamp(id: string): number | null {
  if (/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(id)) {
    const hex = id.substring(0, 13).replace('-', '');
    const time = parseInt(hex, 16);
    return Number.isNaN(time) ? null : time;
  }

  if (id && id.length === 26) {
    const encoding = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';
    let time = 0;
    for (let i = 0; i < 10; i += 1) {
      const val = encoding.indexOf(id.charAt(i).toUpperCase());
      if (val === -1) return null;
      time = time * 32 + val;
    }
    return time;
  }

  return null;
}

export function parseSessionDate(id: string) {
  const timestamp = parseSessionTimestamp(id);
  if (timestamp !== null) {
    return new Date(timestamp).toLocaleString(undefined, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  }
  return id ? id.substring(0, 8) : 'Unknown';
}

function ensureNamespaceNode(root: ExplorerNode, ns: string, labels?: Record<string, string>) {
  const parts = ns.split(':').filter(Boolean);
  let current = root;
  for (let index = 0; index < parts.length; index += 1) {
    const part = parts[index];
    const currentNs = parts.slice(0, index + 1).join(':');
    let child = current.children.find((node) => node.id === currentNs);
    if (!child) {
      child = {
        id: currentNs,
        name: part,
        badge: index === parts.length - 1 ? namespaceLabel(labels) : undefined,
        selection: { type: 'namespace', ns: currentNs, fullPath: currentNs },
        children: [],
      };
      current.children.push(child);
    } else if (index === parts.length - 1 && labels) {
      child.badge = namespaceLabel(labels);
    }
    current = child;
  }
  return current;
}

function sortNamespaceTree(node: ExplorerNode) {
  node.children.sort(compareByName);
  node.children.forEach(sortNamespaceTree);
}

export function buildNamespaceTree({
  namespaceParents,
  namespaceQueries,
  selectedNode,
  activeNamespace,
}: Pick<ExplorerQueries, 'namespaceParents' | 'namespaceQueries'> & {
  selectedNode: Selection | null;
  activeNamespace: string;
}) {
  const root: ExplorerNode = {
    id: '',
    name: 'root',
    selection: { type: 'namespace', ns: '', fullPath: '' },
    children: [],
  };

  namespaceParents.forEach((_, index) => {
    for (const namespace of namespaceQueries[index]?.data || []) {
      ensureNamespaceNode(root, namespace.name, namespace.labels);
    }
  });

  const namespacesToEnsure = new Set<string>();
  if (activeNamespace) {
    for (const ns of namespaceAncestors(activeNamespace)) {
      if (ns) namespacesToEnsure.add(ns);
    }
  }
  if (selectedNode?.ns) {
    for (const ns of namespaceAncestors(selectedNode.ns)) {
      if (ns) namespacesToEnsure.add(ns);
    }
  }
  for (const ns of namespacesToEnsure) {
    ensureNamespaceNode(root, ns);
  }

  sortNamespaceTree(root);

  return root;
}

function resourceNode(
  ns: string,
  id: string,
  name: string,
  type: SelectionType,
  badge?: string,
  extraSelection: Partial<Selection> = {},
): ExplorerNode {
  return {
    id,
    name,
    badge,
    selection: {
      type,
      ns,
      resourceName: name,
      fullPath: id,
      ...extraSelection,
    },
    children: [],
  };
}

function agentNode(ns: string, name: string, sessions: string[]): ExplorerNode {
  const id = `${ns}:${name}`;
  return {
    id,
    name,
    selection: { type: 'agent', ns, agent: name, fullPath: id },
    children: sessions.map((sessionId) => ({
      id: `${ns}:${name}:${sessionId}`,
      name: parseSessionDate(sessionId),
      selection: { type: 'session', ns, agent: name, sessionId, fullPath: `${ns}:${name}:${sessionId}` },
      children: [],
    })),
  };
}

function channelNode(ns: string, resource: ResourceEnvelope, subscriptions: ResourceEnvelope[]): ExplorerNode {
  const channel = channelFromResource(resource);
  const name = channel.name || 'unknown-channel';
  const id = `${ns}:channel:${name}`;
  return {
    id,
    name,
    badge: channel.status === 'closed' ? 'closed' : channel.title || 'channel',
    selection: { type: 'channel', ns, channel: name, resourceName: name, fullPath: id },
    children: subscriptions.map((subscriptionResource) => {
      const subscription = channelSubscriptionFromResource(subscriptionResource);
      const subscriptionName = subscription.name || 'unknown-subscription';
      return {
        id: `${id}:subscription:${subscriptionName}`,
        name: subscriptionName,
        badge:
          subscription.enabled === false
            ? 'disabled'
            : `${subscription.trigger || 'mention'}${(subscription.replyMode || subscription.reply_mode) === 'none' ? ' / no reply' : ''}`,
        selection: {
          type: 'channel-subscription',
          ns,
          channel: name,
          resourceName: subscriptionName,
          fullPath: `${id}:subscription:${subscriptionName}`,
        },
        children: [],
      };
    }),
  };
}

function descriptorGroupTitle(kind: string) {
  if (kind === 'Deployment' || kind === 'DeploymentReplica') return 'Deployments';
  if (kind === 'ConnectorClass' || kind === 'Connector') return 'Connectors';
  if (kind === 'McpServer') return 'MCP Servers';
  if (kind === 'SandboxClass') return 'Sandboxes';
  if (kind === 'SandboxPolicy') return 'Sandboxes';
  if (kind === 'Sandbox') return 'Sandboxes';
  return `${kind}s`;
}

export function buildNamespaceContents({
  activeNamespace,
  resourcesByNamespaceKind,
  sessionsByAgentKey,
  channelSubscriptionsByKey,
}: Pick<ExplorerQueries, 'resourcesByNamespaceKind' | 'sessionsByAgentKey' | 'channelSubscriptionsByKey'> & {
  activeNamespace: string;
}) {
  if (!activeNamespace) return [] as ExplorerGroup[];

  const resourcesByKind = resourcesByNamespaceKind[activeNamespace] || {};
  const groups: ExplorerGroup[] = [];

  const agents = (resourcesByKind.Agent || [])
    .map((resource) => resource.metadata?.name || '')
    .filter(Boolean)
    .map((agent) => agentNode(activeNamespace, agent, sessionsByAgentKey[`${activeNamespace}/${agent}`] || []))
    .sort(compareByName);
  if (agents.length > 0) groups.push({ id: 'agents', title: 'Agents', nodes: agents });

  const channels = (resourcesByKind.Channel || [])
    .map((resource) => {
      const name = resource.metadata?.name || '';
      return channelNode(activeNamespace, resource, name ? channelSubscriptionsByKey[`${activeNamespace}/${name}`] || [] : []);
    })
    .sort(compareByName);
  if (channels.length > 0) groups.push({ id: 'channels', title: 'Channels', nodes: channels });

  const files = (resourcesByKind.File || [])
    .map((resource) => {
      const summary = fileFromResource(resource);
      const name = summary.metadata?.name || summary.spec?.path || 'unknown-file';
      return resourceNode(activeNamespace, `${activeNamespace}:file:${name}`, summary.spec?.path || name, 'file', 'file', {
        resourceName: name,
      });
    })
    .sort(compareByName);
  if (files.length > 0) groups.push({ id: 'files', title: 'Files', nodes: files });

  const schedules = (resourcesByKind.Schedule || [])
    .map((resource) => {
      const schedule = scheduleFromResource(resource);
      const name = schedule.name || 'unknown-schedule';
      return resourceNode(
        activeNamespace,
        `${activeNamespace}:schedule:${name}`,
        name,
        'schedule',
        schedule.spec?.enabled !== false ? schedule.spec?.kind || 'schedule' : 'disabled',
      );
    })
    .sort(compareByName);
  if (schedules.length > 0) groups.push({ id: 'schedules', title: 'Schedules', nodes: schedules });

  const descriptorGroups = new Map<string, ExplorerNode[]>();
  for (const descriptor of RESOURCE_DESCRIPTORS) {
    const title = descriptorGroupTitle(descriptor.kind);
    for (const resource of (resourcesByKind[descriptor.kind] || []) as ResourceEnvelope[]) {
      const name = resource.metadata?.name || `unknown-${descriptor.kind.toLowerCase()}`;
      const node = resourceNode(
        activeNamespace,
        `${activeNamespace}:${descriptor.sortPrefix}:${name}`,
        name,
        descriptor.selectionType,
        descriptor.badge(resource),
      );
      const nodes = descriptorGroups.get(title) || [];
      nodes.push(node);
      descriptorGroups.set(title, nodes);
    }
  }

  for (const title of ['Deployments', 'Connectors', 'Sandboxes', 'Templates', 'MCP Servers']) {
    const nodes = (descriptorGroups.get(title) || []).sort(compareByName);
    if (nodes.length > 0) {
      groups.push({ id: title.toLowerCase().replace(/\s+/g, '-'), title, nodes });
    }
  }

  const templates = (resourcesByKind.Template || [])
    .map((resource) => {
      const name = resource.metadata?.name || 'unknown-template';
      return resourceNode(
        resource.metadata?.namespace || SYSTEM_NAMESPACE,
        `${resource.metadata?.namespace || SYSTEM_NAMESPACE}:template:${name}`,
        name,
        'template',
        'template',
      );
    })
    .sort(compareByName);
  if (templates.length > 0 && !groups.some((group) => group.id === 'templates')) {
    groups.push({ id: 'templates', title: 'Templates', nodes: templates });
  }

  return groups;
}
