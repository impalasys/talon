import { useQueries } from '@tanstack/react-query';
import { useMemo } from 'react';
import type { Selection } from '../lib/selection';
import { namespaceAncestors, selectionExpansionIds } from '../lib/selection';
import { listNamespaces, listResources, listSessions } from '../lib/talon/client';
import { talonQueryKeys, type TalonQueryScope } from '../lib/talon/queryKeys';
import { RESOURCE_DESCRIPTORS } from '../lib/talon/resourceDescriptors';

const STATIC_RESOURCE_KINDS = [
  'Agent',
  'Channel',
  'Schedule',
  'Knowledge',
  ...RESOURCE_DESCRIPTORS.map((descriptor) => descriptor.kind),
];

function unique(values: string[]) {
  return Array.from(new Set(values));
}

function maybeNamespaceId(id: string) {
  return id === '' || !id.includes(':channel:');
}

function namespaceQueryParents(expanded: Set<string>, selectedNode: Selection | null) {
  const parents = new Set<string>(['']);
  for (const id of expanded) {
    if (maybeNamespaceId(id)) parents.add(id);
  }
  for (const id of selectionExpansionIds(selectedNode)) {
    if (maybeNamespaceId(id)) parents.add(id);
  }
  return Array.from(parents);
}

function namespaceScopeFromQueryData(
  selectedNode: Selection | null,
  namespaceResults: Array<{ data?: Array<{ name: string }> }>,
) {
  const namespaces = new Set<string>();

  if (selectedNode?.ns) {
    for (const ns of namespaceAncestors(selectedNode.ns)) {
      if (ns) namespaces.add(ns);
    }
  }

  for (const result of namespaceResults) {
    for (const ns of result.data || []) {
      if (ns.name) namespaces.add(ns.name);
    }
  }

  return Array.from(namespaces).sort();
}

export function collectExpandedNamespaceIds(expanded: Set<string>, selectedNode: Selection | null) {
  const namespaces = new Set<string>();
  for (const id of expanded) {
    if (id && maybeNamespaceId(id)) namespaces.add(id);
  }
  if (selectedNode?.ns) {
    for (const ns of namespaceAncestors(selectedNode.ns)) {
      if (ns) namespaces.add(ns);
    }
  }
  return Array.from(namespaces).sort();
}

export type ExplorerQueries = ReturnType<typeof useExplorerQueries>;

export function useExplorerQueries({
  isConnected,
  scope,
  expanded,
  namespaceExpanded,
  resourceExpanded,
  activeNamespace,
  selectedNode,
}: {
  isConnected: boolean;
  scope: TalonQueryScope;
  expanded: Set<string>;
  namespaceExpanded?: Set<string>;
  resourceExpanded?: Set<string>;
  activeNamespace?: string;
  selectedNode: Selection | null;
}) {
  const namespaceExpansion = namespaceExpanded || expanded;
  const resourceExpansion = resourceExpanded || expanded;
  const namespaceParents = useMemo(
    () => namespaceQueryParents(namespaceExpansion, selectedNode),
    [namespaceExpansion, selectedNode],
  );

  const namespaceQueries = useQueries({
    queries: namespaceParents.map((parent) => ({
      queryKey: talonQueryKeys.namespaces(scope, parent),
      queryFn: ({ signal }) => listNamespaces(parent, { signal }),
      enabled: isConnected,
    })),
  });

  const namespaceIds = useMemo(
    () => namespaceScopeFromQueryData(selectedNode, namespaceQueries),
    [namespaceQueries, selectedNode],
  );

  const resourceNamespaceIds = useMemo(() => {
    const discovered = new Set(namespaceIds);
    const namespaces = new Set<string>();
    if (activeNamespace) namespaces.add(activeNamespace);
    if (selectedNode?.ns) namespaces.add(selectedNode.ns);
    for (const id of namespaceExpansion) {
      if (id && discovered.has(id)) {
        namespaces.add(id);
      }
    }
    return Array.from(namespaces).sort();
  }, [activeNamespace, namespaceExpansion, namespaceIds, selectedNode]);

  const resourceTargets = useMemo(
    () =>
      resourceNamespaceIds.flatMap((ns) =>
        STATIC_RESOURCE_KINDS.map((kind) => ({
          ns,
          kind,
        })),
      ),
    [resourceNamespaceIds],
  );

  const resourceQueries = useQueries({
    queries: resourceTargets.map(({ ns, kind }) => ({
      queryKey: talonQueryKeys.resources(scope, ns, kind),
      queryFn: ({ signal }) => listResources(ns, kind, { signal }),
      enabled: isConnected && Boolean(ns),
    })),
  });

  const resourcesByNamespaceKind = useMemo(() => {
    const map: Record<string, Record<string, any[]>> = {};
    resourceTargets.forEach((target, index) => {
      map[target.ns] ||= {};
      map[target.ns][target.kind] = resourceQueries[index]?.data || [];
    });
    return map;
  }, [resourceQueries, resourceTargets]);

  const agentSessionTargets = useMemo(() => {
    const targets: Array<{ ns: string; agent: string }> = [];
    for (const [ns, byKind] of Object.entries(resourcesByNamespaceKind)) {
      for (const resource of byKind.Agent || []) {
        const agent = resource.metadata?.name || '';
        if (!agent) continue;
        if (resourceExpansion.has(`${ns}:${agent}`) || selectedNode?.agent === agent && selectedNode.ns === ns) {
          targets.push({ ns, agent });
        }
      }
    }
    return targets;
  }, [resourceExpansion, resourcesByNamespaceKind, selectedNode]);

  const sessionQueries = useQueries({
    queries: agentSessionTargets.map(({ ns, agent }) => ({
      queryKey: talonQueryKeys.sessions(scope, ns, agent),
      queryFn: ({ signal }) => listSessions(ns, agent, { signal }),
      enabled: isConnected,
    })),
  });

  const sessionsByAgentKey = useMemo(() => {
    const map: Record<string, string[]> = {};
    agentSessionTargets.forEach((target, index) => {
      map[`${target.ns}/${target.agent}`] = sessionQueries[index]?.data || [];
    });
    return map;
  }, [agentSessionTargets, sessionQueries]);

  const channelSubscriptionTargets = useMemo(() => {
    const targets: Array<{ ns: string; channel: string }> = [];
    for (const [ns, byKind] of Object.entries(resourcesByNamespaceKind)) {
      for (const resource of byKind.Channel || []) {
        const channel = resource.metadata?.name || '';
        if (!channel) continue;
        if (resourceExpansion.has(`${ns}:channel:${channel}`) || selectedNode?.channel === channel && selectedNode.ns === ns) {
          targets.push({ ns, channel });
        }
      }
    }
    return targets;
  }, [resourceExpansion, resourcesByNamespaceKind, selectedNode]);

  const channelSubscriptionNamespaces = useMemo(
    () => unique(channelSubscriptionTargets.map((target) => target.ns)),
    [channelSubscriptionTargets],
  );

  const subscriptionQueries = useQueries({
    queries: channelSubscriptionNamespaces.map((ns) => ({
      queryKey: talonQueryKeys.resources(scope, ns, 'ChannelSubscription'),
      queryFn: ({ signal }) => listResources(ns, 'ChannelSubscription', { signal }),
      enabled: isConnected,
    })),
  });

  const channelSubscriptionsByKey = useMemo(() => {
    const map: Record<string, any[]> = {};
    const subscriptionsByNamespace = Object.fromEntries(
      channelSubscriptionNamespaces.map((ns, index) => [ns, subscriptionQueries[index]?.data || []]),
    );
    channelSubscriptionTargets.forEach((target) => {
      map[`${target.ns}/${target.channel}`] = (subscriptionsByNamespace[target.ns] || []).filter((resource: any) => {
        const spec = resource.spec?.kind?.case === 'channelSubscription' ? resource.spec.kind.value || {} : {};
        return spec.channel === target.channel;
      });
    });
    return map;
  }, [channelSubscriptionNamespaces, channelSubscriptionTargets, subscriptionQueries]);

  return {
    namespaceParents,
    namespaceQueries,
    namespaceIds,
    resourceNamespaceIds,
    resourceTargets,
    resourceQueries,
    resourcesByNamespaceKind,
    agentSessionTargets,
    sessionQueries,
    sessionsByAgentKey,
    channelSubscriptionTargets,
    subscriptionQueries,
    channelSubscriptionsByKey,
    isInitialLoading: namespaceQueries.some((query) => query.isLoading),
  };
}
