import { useMemo } from 'react';
import type { Selection, SelectionType } from '../lib/selection';
import { namespaceAncestors } from '../lib/selection';
import { namespaceLabel } from '../lib/talon/resourceMappers';
import {
  channelFromResource,
  channelSubscriptionFromResource,
  knowledgeFromResource,
  scheduleFromResource,
  type ResourceEnvelope,
} from '../lib/talon/resourceMappers';
import { RESOURCE_DESCRIPTORS } from '../lib/talon/resourceDescriptors';
import type { ExplorerQueries } from './useExplorerQueries';

export type TreeNode = {
  id: string;
  name: string;
  selection: Selection;
  badge?: string;
  children: { [key: string]: TreeNode };
};

function parseSessionTimestamp(id: string): number | null {
  if (id && id.length === 36 && id.charAt(8) === '-') {
    const hex = id.substring(0, 13).replace('-', '');
    const time = parseInt(hex, 16);
    return Number.isNaN(time) ? null : time;
  }

  if (id && id.length === 26) {
    const encoding = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';
    let time = 0;
    for (let i = 0; i < 10; i++) {
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

export function nodeSortWeight(node: TreeNode) {
  switch (node.selection.type) {
    case 'namespace':
      return 0;
    case 'agent':
      return 1;
    case 'channel':
      return 2;
    case 'channel-subscription':
      return 3;
    case 'mcp-server':
      return 4;
    case 'sandbox':
      return 5;
    case 'sandbox-policy':
      return 6;
    case 'sandbox-class':
      return 7;
    case 'deployment':
      return 8;
    case 'deployment-replica':
      return 9;
    case 'template':
      return 10;
    case 'schedule':
      return 11;
    case 'session':
      return 12;
    default:
      return 20;
  }
}

export function compareTreeNodes(a: TreeNode, b: TreeNode) {
  const weightDiff = nodeSortWeight(a) - nodeSortWeight(b);
  if (weightDiff !== 0) return weightDiff;

  if (a.selection.type === 'session' && b.selection.type === 'session') {
    const aTime = parseSessionTimestamp(a.selection.sessionId || '');
    const bTime = parseSessionTimestamp(b.selection.sessionId || '');
    if (aTime !== null && bTime !== null && aTime !== bTime) {
      return bTime - aTime;
    }
  }

  return a.name.localeCompare(b.name);
}

function ensureNamespaceNode(root: TreeNode, ns: string, labels?: Record<string, string>) {
  const parts = ns.split(':').filter(Boolean);
  let current = root;
  for (let i = 0; i < parts.length; i++) {
    const part = parts[i];
    const currentNsId = parts.slice(0, i + 1).join(':');
    if (!current.children[part]) {
      current.children[part] = {
        id: currentNsId,
        name: part,
        badge: i === parts.length - 1 ? namespaceLabel(labels) : undefined,
        selection: { type: 'namespace', ns: currentNsId, fullPath: currentNsId },
        children: {},
      };
    } else if (i === parts.length - 1 && labels) {
      current.children[part].badge = namespaceLabel(labels);
    }
    current = current.children[part];
  }
  return current;
}

function addResourceNode(
  parent: TreeNode,
  ns: string,
  key: string,
  name: string,
  selectionType: SelectionType,
  sortPrefix: string,
  badge?: string,
) {
  const id = `${ns}:${sortPrefix}:${name}`;
  parent.children[key] = {
    id,
    name,
    badge,
    selection: {
      type: selectionType,
      ns,
      resourceName: name,
      fullPath: id,
    },
    children: {},
  };
}

export function buildExplorerTree({
  queries,
  expanded,
}: {
  queries: Pick<
    ExplorerQueries,
    'namespaceParents' | 'namespaceQueries' | 'resourcesByNamespaceKind' | 'sessionsByAgentKey' | 'channelSubscriptionsByKey'
  >;
  expanded: Set<string>;
}) {
  const root: TreeNode = {
    id: '',
    name: 'root',
    selection: { type: 'namespace', ns: '', fullPath: '' },
    children: {},
  };

  queries.namespaceParents.forEach((_, index) => {
    for (const namespace of queries.namespaceQueries[index]?.data || []) {
      ensureNamespaceNode(root, namespace.name, namespace.labels);
    }
  });

  for (const ns of Object.keys(queries.resourcesByNamespaceKind)) {
    if (!ns) continue;
    const currentLevel = ensureNamespaceNode(root, ns);
    const resourcesByKind = queries.resourcesByNamespaceKind[ns] || {};

    for (const agentResource of resourcesByKind.Agent || []) {
      const agent = agentResource.metadata?.name || '';
      if (!agent) continue;
      const agentId = `${ns}:${agent}`;
      currentLevel.children[agent] = {
        id: agentId,
        name: agent,
        selection: { type: 'agent', ns, agent, fullPath: agentId },
        children: {},
      };

      if (expanded.has(agentId)) {
        for (const sessionId of queries.sessionsByAgentKey[`${ns}/${agent}`] || []) {
          const sessionFullId = `${ns}:${agent}:${sessionId}`;
          currentLevel.children[agent].children[sessionId] = {
            id: sessionFullId,
            name: parseSessionDate(sessionId),
            selection: { type: 'session', ns, agent, sessionId, fullPath: sessionFullId },
            children: {},
          };
        }
      }
    }

    for (const resource of resourcesByKind.Channel || []) {
      const channel = channelFromResource(resource);
      const channelName = channel.name || 'unknown-channel';
      const channelId = `${ns}:channel:${channelName}`;
      const status = channel.status || 'open';
      currentLevel.children[`channel:${channelName}`] = {
        id: channelId,
        name: channelName,
        badge: status === 'closed' ? 'closed' : channel.title || 'channel',
        selection: {
          type: 'channel',
          ns,
          channel: channelName,
          resourceName: channelName,
          fullPath: channelId,
        },
        children: {},
      };

      if (expanded.has(channelId)) {
        for (const subscriptionResource of queries.channelSubscriptionsByKey[`${ns}/${channelName}`] || []) {
          const subscription = channelSubscriptionFromResource(subscriptionResource);
          const subscriptionName = subscription.name || 'unknown-subscription';
          const subscriptionId = `${channelId}:subscription:${subscriptionName}`;
          currentLevel.children[`channel:${channelName}`].children[`subscription:${subscriptionName}`] = {
            id: subscriptionId,
            name: subscriptionName,
            badge:
              subscription.enabled === false
                ? 'disabled'
                : `${subscription.trigger || 'mention'}${(subscription.replyMode || subscription.reply_mode) === 'none' ? ' / no reply' : ''}`,
            selection: {
              type: 'channel-subscription',
              ns,
              channel: channelName,
              resourceName: subscriptionName,
              fullPath: subscriptionId,
            },
            children: {},
          };
        }
      }
    }

    for (const resource of resourcesByKind.Schedule || []) {
      const schedule = scheduleFromResource(resource);
      const scheduleName = schedule.name || 'unknown-schedule';
      const scheduleId = `${ns}:schedule:${scheduleName}`;
      currentLevel.children[`schedule:${scheduleName}`] = {
        id: scheduleId,
        name: scheduleName,
        badge: schedule.spec?.enabled !== false ? schedule.spec?.kind || 'schedule' : 'disabled',
        selection: { type: 'schedule', ns, resourceName: scheduleName, fullPath: scheduleId },
        children: {},
      };
    }

    for (const resource of resourcesByKind.Knowledge || []) {
      const knowledge = knowledgeFromResource(resource);
      const knowledgeName = knowledge.metadata?.name || knowledge.spec?.path || 'unknown-knowledge';
      const knowledgeId = `${ns}:knowledge:${knowledgeName}`;
      currentLevel.children[`knowledge:${knowledgeName}`] = {
        id: knowledgeId,
        name: knowledge.spec?.path || knowledgeName,
        badge: 'knowledge',
        selection: { type: 'knowledge', ns, resourceName: knowledgeName, fullPath: knowledgeId },
        children: {},
      };
    }

    for (const descriptor of RESOURCE_DESCRIPTORS) {
      for (const resource of (resourcesByKind[descriptor.kind] || []) as ResourceEnvelope[]) {
        const name = resource.metadata?.name || `unknown-${descriptor.kind.toLowerCase()}`;
        addResourceNode(
          currentLevel,
          ns,
          `${descriptor.sortPrefix}:${name}`,
          name,
          descriptor.selectionType,
          descriptor.sortPrefix,
          descriptor.badge(resource),
        );
      }
    }
  }

  return root;
}

export function useExplorerTree({
  queries,
  expanded,
  selectedNode,
}: {
  queries: ExplorerQueries;
  expanded: Set<string>;
  selectedNode: Selection | null;
}) {
  return useMemo(() => {
    const tree = buildExplorerTree({ queries, expanded });
    if (selectedNode?.ns) {
      for (const ns of namespaceAncestors(selectedNode.ns)) {
        if (ns) ensureNamespaceNode(tree, ns);
      }
    }
    return tree;
  }, [expanded, queries, selectedNode]);
}
