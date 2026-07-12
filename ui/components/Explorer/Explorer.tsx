import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { useMutation, useQueries, useQueryClient } from '@tanstack/react-query';
import { Label } from 'react-aria-components';
import {
  Broadcast,
  CalendarBlank,
  ChatCircleText,
  Cube,
  Database,
  FileText,
  FolderOpen,
  FolderSimple,
  Hash,
  Plug,
  Robot,
  ShieldCheck,
  Stack,
  type Icon,
} from '@phosphor-icons/react';
import { Button } from '../tailgrids/core/button';
import {
  DialogBody as ModalBody,
  DialogContent as ModalContent,
  DialogFooter as ModalFooter,
  DialogHeader as ModalHeader,
  DialogOverlay,
} from '../tailgrids/core/dialog';
import {
  DropdownMenu as Dropdown,
  DropdownMenuContent as DropdownMenu,
  DropdownMenuItem as DropdownItem,
  DropdownMenuTrigger as DropdownTrigger,
} from '../tailgrids/core/dropdown';
import { Input } from '../tailgrids/core/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../tailgrids/core/select';
import { useExplorerQueries } from '../../hooks/useExplorerQueries';
import {
  RESOURCE_KIND_BY_SELECTION,
  SYSTEM_NAMESPACE,
  areSelectionsEqual,
  namespaceAncestors,
  selectionExpansionIds,
  type Selection,
  type SelectionType,
} from '../../lib/selection';
import {
  createNamespace,
  createResource,
  createSession,
  deleteNamespace,
  deleteResource,
  deleteSession,
  listResources,
} from '../../lib/talon/client';
import { talonQueryKeys, type TalonQueryScope } from '../../lib/talon/queryKeys';
import {
  parseJsonObject,
  resourceMetadata,
  resourceSpec,
  type ResourceEnvelope,
} from '../../lib/talon/resourceMappers';
import { cn } from '../../utils/cn';
import {
  buildNamespaceContents,
  buildNamespaceTree,
  type ExplorerGroup,
  type ExplorerNode,
} from './explorerModel';

const API_VERSION = 'talon.impalasys.com/v1';
const LEAF_TYPES: SelectionType[] = [
  'session',
  'schedule',
  'knowledge',
  'mcp-server',
  'channel-subscription',
  'workflow',
  'template',
  'deployment-replica',
  'sandbox-class',
  'sandbox-policy',
  'sandbox',
];
const LIST_PREVIEW_LIMIT = 10;

function namespaceParent(ns: string) {
  const parts = ns.split(':').filter(Boolean);
  parts.pop();
  return parts.join(':');
}

function templateOptionId(template: ResourceEnvelope) {
  return `${template.metadata?.namespace || SYSTEM_NAMESPACE}/${template.metadata?.name || ''}`;
}

function NodeIcon({ type, selected, expanded }: { type: SelectionType; selected: boolean; expanded?: boolean }) {
  const iconByType: Partial<Record<SelectionType, Icon>> = {
    namespace: expanded ? FolderOpen : FolderSimple,
    agent: Robot,
    session: ChatCircleText,
    channel: Hash,
    'channel-subscription': Broadcast,
    workflow: Stack,
    schedule: CalendarBlank,
    template: Database,
    deployment: Stack,
    'deployment-replica': Stack,
    'sandbox-class': ShieldCheck,
    'sandbox-policy': ShieldCheck,
    sandbox: Cube,
    'mcp-server': Plug,
    knowledge: FileText,
  };
  const colorByType: Partial<Record<SelectionType, string>> = {
    namespace: selected ? 'text-slate-900 dark:text-slate-50' : 'text-slate-500',
    agent: 'text-emerald-500',
    session: 'text-blue-500',
    channel: 'text-cyan-500',
    'channel-subscription': 'text-cyan-400',
    workflow: 'text-purple-500',
    schedule: 'text-amber-500',
    template: 'text-emerald-600',
    deployment: 'text-indigo-500',
    'deployment-replica': 'text-indigo-400',
    'sandbox-class': 'text-fuchsia-500',
    'sandbox-policy': 'text-fuchsia-400',
    sandbox: 'text-orange-500',
    'mcp-server': 'text-blue-600',
    knowledge: 'text-violet-500',
  };
  const IconComponent = iconByType[type] || Cube;
  return (
    <IconComponent
      weight="fill"
      className={cn('h-3.5 w-3.5 flex-none', colorByType[type] || 'text-slate-500')}
      aria-hidden="true"
    />
  );
}

function SectionHeader({
  title,
  detail,
  action,
}: {
  title: string;
  detail?: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-2 px-3 py-2">
      <div className="min-w-0">
        <h3 className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">{title}</h3>
        {detail ? <div className="truncate text-[12px] font-semibold text-foreground">{detail}</div> : null}
      </div>
      {action}
    </div>
  );
}

function Row({
  node,
  level,
  selected,
  active,
  expanded,
  onSelect,
  onContextMenu,
  onToggle,
  children,
}: {
  node: ExplorerNode;
  level: number;
  selected: boolean;
  active?: boolean;
  expanded?: boolean;
  onSelect: (node: ExplorerNode) => void;
  onContextMenu: (event: React.MouseEvent, node: ExplorerNode) => void;
  onToggle?: (node: ExplorerNode) => void;
  children?: ReactNode;
}) {
  const hasChildren = node.children.length > 0 || !LEAF_TYPES.includes(node.selection.type);
  return (
    <div>
      <div
        className={cn(
          'group flex h-6 select-none items-center gap-1.5 rounded-md pr-1.5 text-[12px] font-medium leading-none transition-colors hover:bg-muted/55',
          active && 'bg-slate-200/70 text-foreground shadow-sm dark:bg-slate-800/80',
          !active && selected && 'bg-muted/65 text-foreground',
          !active && !selected && 'text-muted-foreground',
          hasChildren && onToggle && 'cursor-pointer',
        )}
        style={{ paddingLeft: `${level * 12}px` }}
        onClick={(event) => {
          event.stopPropagation();
          onSelect(node);
          if (hasChildren && onToggle) onToggle(node);
        }}
        onContextMenu={(event) => onContextMenu(event, node)}
      >
        <NodeIcon type={node.selection.type} selected={active || selected} expanded={expanded} />
        <span className="min-w-0 flex-1 truncate">{node.name}</span>
        {node.badge ? (
          <span className="max-w-[6rem] truncate rounded bg-muted px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wide text-muted-foreground">
            {node.badge}
          </span>
        ) : null}
      </div>
      {children}
    </div>
  );
}

function ShowMoreRow({
  hiddenCount,
  level,
  onShowMore,
}: {
  hiddenCount: number;
  level: number;
  onShowMore: () => void;
}) {
  return (
    <button
      type="button"
      className="flex h-6 w-full items-center rounded-md pr-1.5 text-left text-[11px] font-semibold leading-none text-muted-foreground transition-colors hover:bg-muted/55 hover:text-foreground"
      style={{ paddingLeft: `${level * 12}px` }}
      onClick={(event) => {
        event.stopPropagation();
        onShowMore();
      }}
    >
      Show More ({hiddenCount}+)
    </button>
  );
}

function NamespaceRows({
  nodes,
  activeNamespace,
  selectedNode,
  expanded,
  expandedLists,
  onSelect,
  onContextMenu,
  onToggle,
  onShowMore,
  level = 0,
  listId = 'namespaces:root',
}: {
  nodes: ExplorerNode[];
  activeNamespace: string;
  selectedNode: Selection | null;
  expanded: Set<string>;
  expandedLists: Set<string>;
  onSelect: (node: ExplorerNode) => void;
  onContextMenu: (event: React.MouseEvent, node: ExplorerNode) => void;
  onToggle: (node: ExplorerNode) => void;
  onShowMore: (listId: string) => void;
  level?: number;
  listId?: string;
}) {
  const shouldShowAll = expandedLists.has(listId);
  const visibleNodes = shouldShowAll ? nodes : nodes.slice(0, LIST_PREVIEW_LIMIT);
  const hiddenCount = Math.max(0, nodes.length - visibleNodes.length);
  return (
    <>
      {visibleNodes.map((node) => {
        const isExpanded = expanded.has(node.id);
        return (
          <Row
            key={node.id}
            node={node}
            level={level}
            selected={areSelectionsEqual(selectedNode, node.selection)}
            active={activeNamespace === node.selection.ns}
            expanded={isExpanded}
            onSelect={onSelect}
            onContextMenu={onContextMenu}
            onToggle={onToggle}
          >
            {isExpanded ? (
              <div className="space-y-px">
                <NamespaceRows
                  nodes={node.children}
                  activeNamespace={activeNamespace}
                  selectedNode={selectedNode}
                  expanded={expanded}
                  expandedLists={expandedLists}
                  onSelect={onSelect}
                  onContextMenu={onContextMenu}
                  onToggle={onToggle}
                  onShowMore={onShowMore}
                  level={level + 1}
                  listId={`namespaces:${node.id || 'root'}`}
                />
              </div>
            ) : null}
          </Row>
        );
      })}
      {hiddenCount > 0 ? <ShowMoreRow hiddenCount={hiddenCount} level={level} onShowMore={() => onShowMore(listId)} /> : null}
    </>
  );
}

function ResourceRows({
  nodes,
  selectedNode,
  expanded,
  expandedLists,
  onSelect,
  onContextMenu,
  onToggle,
  onShowMore,
  level = 0,
  listId = 'resources:root',
}: {
  nodes: ExplorerNode[];
  selectedNode: Selection | null;
  expanded: Set<string>;
  expandedLists: Set<string>;
  onSelect: (node: ExplorerNode) => void;
  onContextMenu: (event: React.MouseEvent, node: ExplorerNode) => void;
  onToggle: (node: ExplorerNode) => void;
  onShowMore: (listId: string) => void;
  level?: number;
  listId?: string;
}) {
  const shouldShowAll = expandedLists.has(listId);
  const visibleNodes = shouldShowAll ? nodes : nodes.slice(0, LIST_PREVIEW_LIMIT);
  const hiddenCount = Math.max(0, nodes.length - visibleNodes.length);
  return (
    <>
      {visibleNodes.map((node) => {
        const isExpanded = expanded.has(node.id);
        const canExpand = node.selection.type === 'agent' || node.selection.type === 'channel';
        return (
          <Row
            key={node.id}
            node={node}
            level={level}
            selected={areSelectionsEqual(selectedNode, node.selection)}
            expanded={isExpanded}
            onSelect={onSelect}
            onContextMenu={onContextMenu}
            onToggle={canExpand ? onToggle : undefined}
          >
            {isExpanded && canExpand ? (
              <div className="space-y-px">
                {node.children.length > 0 ? (
                  <ResourceRows
                    nodes={node.children}
                    selectedNode={selectedNode}
                    expanded={expanded}
                    expandedLists={expandedLists}
                    onSelect={onSelect}
                    onContextMenu={onContextMenu}
                    onToggle={onToggle}
                    onShowMore={onShowMore}
                    level={level + 1}
                    listId={`resources:${node.id}`}
                  />
                ) : (
                  <div
                    className="px-2 py-1 text-[11px] italic text-muted-foreground/60"
                    style={{ paddingLeft: `${(level + 1) * 12 + 16}px` }}
                  >
                    Empty
                  </div>
                )}
              </div>
            ) : null}
          </Row>
        );
      })}
      {hiddenCount > 0 ? <ShowMoreRow hiddenCount={hiddenCount} level={level} onShowMore={() => onShowMore(listId)} /> : null}
    </>
  );
}

function ResourceGroup({
  group,
  selectedNode,
  expanded,
  expandedLists,
  onSelect,
  onContextMenu,
  onToggle,
  onShowMore,
}: {
  group: ExplorerGroup;
  selectedNode: Selection | null;
  expanded: Set<string>;
  expandedLists: Set<string>;
  onSelect: (node: ExplorerNode) => void;
  onContextMenu: (event: React.MouseEvent, node: ExplorerNode) => void;
  onToggle: (node: ExplorerNode) => void;
  onShowMore: (listId: string) => void;
}) {
  return (
    <section className="space-y-px">
      <div className="pt-2 pb-0.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">{group.title}</div>
      <ResourceRows
        nodes={group.nodes}
        selectedNode={selectedNode}
        expanded={expanded}
        expandedLists={expandedLists}
        onSelect={onSelect}
        onContextMenu={onContextMenu}
        onToggle={onToggle}
        onShowMore={onShowMore}
        listId={`resources:${group.id}`}
      />
    </section>
  );
}

export function Explorer({
  isConnected,
  selectedNode,
  activeNamespace,
  onActiveNamespaceChange,
  onSelect,
  queryScope,
}: {
  isConnected: boolean;
  selectedNode: Selection | null;
  activeNamespace: string;
  onActiveNamespaceChange: (namespace: string) => void;
  onSelect: (selection: Selection) => void;
  queryScope: TalonQueryScope;
}) {
  const queryClient = useQueryClient();
  const [namespaceExpanded, setNamespaceExpanded] = useState<Set<string>>(new Set(['']));
  const [resourceExpanded, setResourceExpanded] = useState<Set<string>>(new Set());
  const [expandedLists, setExpandedLists] = useState<Set<string>>(new Set());
  const [namespaceModalOpen, setNamespaceModalOpen] = useState(false);
  const [newNamespace, setNewNamespace] = useState('');
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; node: ExplorerNode } | null>(null);
  const [agentModalOpen, setAgentModalOpen] = useState<{ isOpen: boolean; ns: string }>({ isOpen: false, ns: '' });
  const [agentForm, setAgentForm] = useState({ name: '', template: '' });
  const [channelModalOpen, setChannelModalOpen] = useState<{ isOpen: boolean; ns: string }>({ isOpen: false, ns: '' });
  const [channelForm, setChannelForm] = useState({ name: '', title: '' });
  const [subscriptionModalOpen, setSubscriptionModalOpen] = useState<{ isOpen: boolean; ns: string; channel: string }>({
    isOpen: false,
    ns: '',
    channel: '',
  });
  const [subscriptionForm, setSubscriptionForm] = useState({
    name: '',
    agent: '',
    trigger: 'mention',
    replyMode: 'tool',
    enabled: true,
  });
  const [deleteConfirm, setDeleteConfirm] = useState<{ isOpen: boolean; node: ExplorerNode | null }>({ isOpen: false, node: null });

  useEffect(() => {
    const namespace = activeNamespace || selectedNode?.ns || '';
    if (!namespace) return;
    setNamespaceExpanded((prev) => {
      const next = new Set(prev);
      let changed = false;
      for (const ns of namespaceAncestors(namespace)) {
        if (!next.has(ns)) {
          next.add(ns);
          changed = true;
        }
      }
      return changed ? next : prev;
    });
  }, [activeNamespace, selectedNode]);

  useEffect(() => {
    setResourceExpanded((prev) => {
      const next = new Set(prev);
      let changed = false;
      for (const id of selectionExpansionIds(selectedNode)) {
        if (id && !namespaceAncestors(selectedNode?.ns || '').includes(id) && !next.has(id)) {
          next.add(id);
          changed = true;
        }
      }
      return changed ? next : prev;
    });
  }, [selectedNode]);

  useEffect(() => {
    if (selectedNode?.type === 'namespace' && !newNamespace) {
      setNewNamespace(`${selectedNode.ns}:`);
    }
  }, [newNamespace, selectedNode]);

  const combinedExpanded = useMemo(
    () => new Set([...namespaceExpanded, ...resourceExpanded]),
    [namespaceExpanded, resourceExpanded],
  );
  const queries = useExplorerQueries({
    isConnected,
    scope: queryScope,
    expanded: combinedExpanded,
    namespaceExpanded,
    resourceExpanded,
    activeNamespace,
    selectedNode,
  });

  const namespaceTree = useMemo(
    () => buildNamespaceTree({
      namespaceParents: queries.namespaceParents,
      namespaceQueries: queries.namespaceQueries,
      selectedNode,
      activeNamespace,
    }),
    [activeNamespace, queries.namespaceParents, queries.namespaceQueries, selectedNode],
  );

  const contentGroups = useMemo(
    () => buildNamespaceContents({
      activeNamespace,
      resourcesByNamespaceKind: queries.resourcesByNamespaceKind,
      sessionsByAgentKey: queries.sessionsByAgentKey,
      channelSubscriptionsByKey: queries.channelSubscriptionsByKey,
    }),
    [activeNamespace, queries.channelSubscriptionsByKey, queries.resourcesByNamespaceKind, queries.sessionsByAgentKey],
  );

  const agentTemplateQueries = useQueries({
    queries: Array.from(new Set([SYSTEM_NAMESPACE, agentModalOpen.ns].filter(Boolean))).map((ns) => ({
      queryKey: talonQueryKeys.resources(queryScope, ns, 'Template'),
      queryFn: ({ signal }) => listResources(ns, 'Template', { signal }),
      enabled: isConnected && agentModalOpen.isOpen,
    })),
  });
  const agentCreateTemplates = useMemo(
    () =>
      agentTemplateQueries
        .flatMap((query) => query.data || [])
        .filter((template) => resourceSpec(template, 'template').kind === 'Agent')
        .sort((left, right) => {
          const leftNs = left.metadata?.namespace || SYSTEM_NAMESPACE;
          const rightNs = right.metadata?.namespace || SYSTEM_NAMESPACE;
          return leftNs === rightNs
            ? (left.metadata?.name || '').localeCompare(right.metadata?.name || '')
            : leftNs.localeCompare(rightNs);
        }),
    [agentTemplateQueries],
  );

  const invalidateNamespace = useCallback(
    async (ns: string, kinds: string[] = []) => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: talonQueryKeys.namespaces(queryScope, namespaceParent(ns)) }),
        ...kinds.map((kind) => queryClient.invalidateQueries({ queryKey: talonQueryKeys.resources(queryScope, ns, kind) })),
      ]);
    },
    [queryClient, queryScope],
  );

  const createNamespaceMutation = useMutation({
    mutationFn: (name: string) => createNamespace(name),
    onSuccess: async (_, name) => {
      await invalidateNamespace(name);
      setNamespaceExpanded((prev) => new Set(prev).add(name));
      setNewNamespace('');
      setNamespaceModalOpen(false);
    },
  });

  const createAgentMutation = useMutation({
    mutationFn: async (variables: { ns: string; name: string; template: string }) => {
      const selectedTemplate = agentCreateTemplates.find((template) => templateOptionId(template) === variables.template);
      const templateSpec = selectedTemplate ? resourceSpec(selectedTemplate, 'template') : null;
      if (templateSpec && templateSpec.kind !== 'Agent') {
        throw new Error(`Template '${variables.template}' renders ${templateSpec.kind}, not Agent`);
      }
      const agentSpec = templateSpec ? parseJsonObject(templateSpec.specJson) : { systemPrompt: '' };
      return createResource(variables.ns, {
        apiVersion: API_VERSION,
        kind: 'Agent',
        metadata: resourceMetadata(variables.name.trim(), variables.ns),
        spec: { kind: { case: 'agent', value: agentSpec } },
      });
    },
    onSuccess: async (_, variables) => {
      await invalidateNamespace(variables.ns, ['Agent']);
      setAgentModalOpen({ isOpen: false, ns: '' });
      setAgentForm({ name: '', template: '' });
    },
  });

  const createChannelMutation = useMutation({
    mutationFn: (variables: { ns: string; name: string; title: string }) =>
      createResource(variables.ns, {
        apiVersion: API_VERSION,
        kind: 'Channel',
        metadata: resourceMetadata(variables.name.trim(), variables.ns),
        spec: { kind: { case: 'channel', value: { title: variables.title.trim(), metadata: {} } } },
      }),
    onSuccess: async (_, variables) => {
      await invalidateNamespace(variables.ns, ['Channel']);
      setChannelModalOpen({ isOpen: false, ns: '' });
      setChannelForm({ name: '', title: '' });
    },
  });

  const createSubscriptionMutation = useMutation({
    mutationFn: (variables: { ns: string; channel: string; name: string; agent: string; enabled: boolean; trigger: string; replyMode: string }) =>
      createResource(variables.ns, {
        apiVersion: API_VERSION,
        kind: 'ChannelSubscription',
        metadata: resourceMetadata(variables.name.trim(), variables.ns),
        spec: {
          kind: {
            case: 'channelSubscription',
            value: {
              channel: variables.channel,
              agent: variables.agent.trim(),
              enabled: variables.enabled,
              trigger: variables.trigger,
              replyMode: variables.replyMode,
            },
          },
        },
      }),
    onSuccess: async (_, variables) => {
      await invalidateNamespace(variables.ns, ['ChannelSubscription']);
      setResourceExpanded((prev) => new Set(prev).add(`${variables.ns}:channel:${variables.channel}`));
      setSubscriptionModalOpen({ isOpen: false, ns: '', channel: '' });
      setSubscriptionForm({ name: '', agent: '', trigger: 'mention', replyMode: 'tool', enabled: true });
    },
  });

  const createSessionMutation = useMutation({
    mutationFn: ({ ns, agent }: { ns: string; agent: string }) => createSession(ns, agent),
    onSuccess: async (_, { ns, agent }) => {
      await queryClient.invalidateQueries({ queryKey: talonQueryKeys.sessions(queryScope, ns, agent) });
      setResourceExpanded((prev) => new Set(prev).add(`${ns}:${agent}`));
    },
  });

  const deleteMutation = useMutation({
    mutationFn: async (node: ExplorerNode) => {
      const { selection } = node;
      if (selection.type === 'namespace') {
        await deleteNamespace(selection.ns);
        return { ns: selection.ns, kinds: [] as string[] };
      }
      if (selection.type === 'agent') {
        await deleteResource(selection.ns, 'Agent', selection.agent || '');
        return { ns: selection.ns, kinds: ['Agent'] };
      }
      if (selection.type === 'session') {
        await deleteSession(selection.ns, selection.agent!, selection.sessionId!);
        return { ns: selection.ns, agent: selection.agent || '', kinds: [] as string[] };
      }
      const kind = RESOURCE_KIND_BY_SELECTION[selection.type];
      if (kind && selection.resourceName) {
        await deleteResource(selection.ns, kind, selection.resourceName);
        return { ns: selection.ns, kinds: [kind] };
      }
      return { ns: selection.ns, kinds: [] as string[] };
    },
    onSuccess: async (target) => {
      if ('agent' in target && target.agent) {
        await queryClient.invalidateQueries({ queryKey: talonQueryKeys.sessions(queryScope, target.ns, target.agent) });
      } else {
        await invalidateNamespace(target.ns, target.kinds);
      }
      setDeleteConfirm({ isOpen: false, node: null });
    },
  });

  const mutationError =
    createNamespaceMutation.error ||
    createAgentMutation.error ||
    createChannelMutation.error ||
    createSubscriptionMutation.error ||
    deleteMutation.error ||
    createSessionMutation.error;

  useEffect(() => {
    if (mutationError) {
      alert(mutationError instanceof Error ? mutationError.message : 'Explorer action failed');
    }
  }, [mutationError]);

  const menuNode = contextMenu?.node;

  const handleContextMenu = (event: React.MouseEvent, node: ExplorerNode) => {
    event.preventDefault();
    event.stopPropagation();
    setContextMenu({ x: event.clientX, y: event.clientY, node });
  };

  const handleNamespaceSelect = (node: ExplorerNode) => {
    onActiveNamespaceChange(node.selection.ns);
    onSelect(node.selection);
  };

  const handleResourceSelect = (node: ExplorerNode) => {
    onSelect(node.selection);
  };

  const toggleNamespace = (node: ExplorerNode) => {
    setNamespaceExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(node.id)) next.delete(node.id);
      else next.add(node.id);
      return next;
    });
  };

  const toggleResource = (node: ExplorerNode) => {
    setResourceExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(node.id)) next.delete(node.id);
      else next.add(node.id);
      return next;
    });
  };

  const showMoreList = useCallback((listId: string) => {
    setExpandedLists((prev) => {
      if (prev.has(listId)) return prev;
      const next = new Set(prev);
      next.add(listId);
      return next;
    });
  }, []);

  const submitNamespace = (event: React.FormEvent) => {
    event.preventDefault();
    const name = newNamespace.trim();
    if (!name) return;
    createNamespaceMutation.mutate(name);
  };

  const submitAgent = (event: React.FormEvent) => {
    event.preventDefault();
    if (!agentForm.name.trim()) return;
    createAgentMutation.mutate({
      ns: agentModalOpen.ns,
      name: agentForm.name,
      template: agentForm.template,
    });
  };

  const submitChannel = (event: React.FormEvent) => {
    event.preventDefault();
    if (!channelForm.name.trim()) return;
    createChannelMutation.mutate({
      ns: channelModalOpen.ns,
      name: channelForm.name,
      title: channelForm.title,
    });
  };

  const submitSubscription = (event: React.FormEvent) => {
    event.preventDefault();
    if (!subscriptionForm.name.trim() || !subscriptionForm.agent.trim()) return;
    createSubscriptionMutation.mutate({
      ns: subscriptionModalOpen.ns,
      channel: subscriptionModalOpen.channel,
      name: subscriptionForm.name,
      agent: subscriptionForm.agent,
      enabled: subscriptionForm.enabled,
      trigger: subscriptionForm.trigger,
      replyMode: subscriptionForm.replyMode,
    });
  };

  return (
    <div className="relative flex h-full flex-col divide-y divide-slate-200/80 overflow-hidden bg-slate-50/90 shadow-[inset_0_1px_0_rgba(255,255,255,0.45)] backdrop-blur-xl dark:divide-border/70 dark:bg-background/72 dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.03)]">
      <div className="flex h-14 flex-none items-center border-b border-border/70 px-3">
        <div>
          <div className="text-[9px] font-semibold uppercase tracking-wider text-muted-foreground">Workspace</div>
          <div className="mt-0.5 flex items-center gap-1.5 text-[13px] font-semibold text-foreground">
            <FolderSimple weight="fill" className="h-3.5 w-3.5 text-slate-500" aria-hidden="true" />
            Explorer
          </div>
        </div>
      </div>

      <section className="flex min-h-0 max-h-[18.75rem] flex-none flex-col overflow-hidden">
        <SectionHeader
          title="Namespaces"
          action={
            <button
              type="button"
              className="rounded-md p-1 text-muted-foreground hover:bg-muted hover:text-foreground"
              onClick={() => {
                setNewNamespace(activeNamespace ? `${activeNamespace}:` : '');
                setNamespaceModalOpen(true);
              }}
              aria-label="New namespace"
            >
              <span className="block text-sm font-semibold leading-none">+</span>
            </button>
          }
        />
        <div className="min-h-0 flex-1 overflow-y-auto px-3 pb-4 custom-scrollbar">
          {namespaceTree.children.length > 0 ? (
            <NamespaceRows
              nodes={namespaceTree.children}
              activeNamespace={activeNamespace}
              selectedNode={selectedNode}
              expanded={namespaceExpanded}
              expandedLists={expandedLists}
              onSelect={handleNamespaceSelect}
              onContextMenu={handleContextMenu}
              onToggle={toggleNamespace}
              onShowMore={showMoreList}
            />
          ) : (
            <div className="py-2 text-[11px] text-muted-foreground">No namespaces discovered.</div>
          )}
          {queries.isLoading ? (
            <div className="py-1 text-[11px] text-muted-foreground">Loading namespaces...</div>
          ) : null}
        </div>
      </section>

      <section className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <div className="min-h-0 flex-1 overflow-y-auto px-3 pt-2 pb-4 custom-scrollbar">
          {!activeNamespace ? (
            <div className="py-2 text-[11px] text-muted-foreground">Select a namespace to browse resources.</div>
          ) : contentGroups.length > 0 ? (
            <div className="space-y-1">
              {contentGroups.map((group) => (
                <ResourceGroup
                  key={group.id}
                  group={group}
                  selectedNode={selectedNode}
                  expanded={resourceExpanded}
                  expandedLists={expandedLists}
                  onSelect={handleResourceSelect}
                  onContextMenu={handleContextMenu}
                  onToggle={toggleResource}
                  onShowMore={showMoreList}
                />
              ))}
            </div>
          ) : (
            <div className="py-2 text-[11px] text-muted-foreground">No resources in this namespace.</div>
          )}
        </div>
      </section>

      <Dropdown isOpen={!!contextMenu} onOpenChange={(open) => !open && setContextMenu(null)}>
        <DropdownTrigger
          className="fixed"
          style={{ top: contextMenu?.y || 0, left: contextMenu?.x || 0, width: 1, height: 1, padding: 0 }}
          aria-label="Explorer context menu"
        />
        <DropdownMenu aria-label="Context Actions" variant="outline">
          {menuNode?.selection.type === 'namespace' ? (
            <>
              <DropdownItem
                id="create_namespace"
                onAction={() => {
                  setContextMenu(null);
                  setNewNamespace(menuNode.selection.ns ? `${menuNode.selection.ns}:` : '');
                  setNamespaceModalOpen(true);
                }}
              >
                <div className="flex items-center gap-2">
                  <span className="h-2 w-2 rounded-sm bg-slate-400" aria-hidden="true" />
                  New Namespace
                </div>
              </DropdownItem>
              <DropdownItem
                id="create_agent"
                onAction={() => {
                  setContextMenu(null);
                  setAgentModalOpen({ isOpen: true, ns: menuNode.selection.ns });
                }}
              >
                <div className="flex items-center gap-2">
                  <span className="h-2 w-2 rounded-sm bg-emerald-500" aria-hidden="true" />
                  Create Agent
                </div>
              </DropdownItem>
              <DropdownItem
                id="create_channel"
                onAction={() => {
                  setContextMenu(null);
                  setChannelModalOpen({ isOpen: true, ns: menuNode.selection.ns });
                }}
              >
                <div className="flex items-center gap-2">
                  <span className="h-2 w-2 rounded-sm bg-cyan-500" aria-hidden="true" />
                  Create Channel
                </div>
              </DropdownItem>
            </>
          ) : null}

          {menuNode?.selection.type === 'agent' ? (
            <DropdownItem
              id="create_session"
              onAction={() => {
                setContextMenu(null);
                createSessionMutation.mutate({
                  ns: menuNode.selection.ns,
                  agent: menuNode.selection.agent || '',
                });
              }}
            >
              <div className="flex items-center gap-2">
                <span className="h-2 w-2 rounded-sm bg-blue-500" aria-hidden="true" />
                Create Session
              </div>
            </DropdownItem>
          ) : null}

          {menuNode?.selection.type === 'channel' ? (
            <DropdownItem
              id="create_channel_subscription"
              onAction={() => {
                setContextMenu(null);
                setSubscriptionModalOpen({
                  isOpen: true,
                  ns: menuNode.selection.ns,
                  channel: menuNode.selection.channel || menuNode.selection.resourceName || '',
                });
              }}
            >
              <div className="flex items-center gap-2">
                <span className="h-2 w-2 rounded-sm bg-cyan-400" aria-hidden="true" />
                Create Subscription
              </div>
            </DropdownItem>
          ) : null}

          {menuNode ? (
            <DropdownItem
              id="delete_item"
              className="text-danger"
              color="danger"
              onAction={() => {
                setContextMenu(null);
                setDeleteConfirm({ isOpen: true, node: menuNode });
              }}
            >
              <div className="flex items-center gap-2">
                <span className="h-2 w-2 rounded-sm bg-red-500" aria-hidden="true" />
                Delete {menuNode.selection.type}
              </div>
            </DropdownItem>
          ) : null}
        </DropdownMenu>
      </Dropdown>

      <DialogOverlay isOpen={namespaceModalOpen} onOpenChange={(open) => !open && setNamespaceModalOpen(false)}>
        <ModalContent className="sm:max-w-md">
          <ModalHeader className="flex flex-col gap-1">New Namespace</ModalHeader>
          <ModalBody>
            <form id="create-namespace-form" onSubmit={submitNamespace} className="space-y-4">
              <div>
                <Label className="mb-1 block text-sm font-medium">Namespace Path</Label>
                <Input
                  className="w-full"
                  autoFocus
                  placeholder="org:team:child"
                  value={newNamespace}
                  onChange={(event) => setNewNamespace(event.target.value)}
                  disabled={createNamespaceMutation.isPending}
                  required
                />
              </div>
            </form>
          </ModalBody>
          <ModalFooter className="mt-4 flex justify-end gap-3">
            <Button variant="ghost" appearance="outline" onClick={() => setNamespaceModalOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              type="submit"
              form="create-namespace-form"
              disabled={createNamespaceMutation.isPending || !newNamespace.trim()}
            >
              Create Namespace
            </Button>
          </ModalFooter>
        </ModalContent>
      </DialogOverlay>

      <DialogOverlay isOpen={agentModalOpen.isOpen} onOpenChange={(open) => !open && setAgentModalOpen({ isOpen: false, ns: '' })}>
        <ModalContent className="sm:max-w-md">
          <ModalHeader className="flex flex-col gap-1">Create Agent</ModalHeader>
          <ModalBody>
            <form id="create-agent-form" onSubmit={submitAgent} className="space-y-4">
              <div>
                <Label className="mb-1 block text-sm font-medium">Agent Name</Label>
                <Input
                  className="w-full"
                  placeholder="my-agent"
                  value={agentForm.name}
                  onChange={(event) => setAgentForm((prev) => ({ ...prev, name: event.target.value }))}
                  required
                />
              </div>
              <div>
                <Label className="mb-1 block text-sm font-medium">Template</Label>
                <Select
                  value={agentForm.template || null}
                  onChange={(key) => setAgentForm((prev) => ({ ...prev, template: key as string }))}
                  placeholder="Blank Agent"
                >
                  <SelectTrigger className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {agentCreateTemplates.map((template) => (
                      <SelectItem id={templateOptionId(template)} key={templateOptionId(template)}>
                        {template.metadata?.namespace || SYSTEM_NAMESPACE}/{template.metadata?.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </form>
          </ModalBody>
          <ModalFooter>
            <Button variant="ghost" appearance="outline" onClick={() => setAgentModalOpen({ isOpen: false, ns: '' })}>
              Cancel
            </Button>
            <Button color="primary" type="submit" form="create-agent-form" disabled={createAgentMutation.isPending || !agentForm.name.trim()}>
              Create Agent
            </Button>
          </ModalFooter>
        </ModalContent>
      </DialogOverlay>

      <DialogOverlay isOpen={channelModalOpen.isOpen} onOpenChange={(open) => !open && setChannelModalOpen({ isOpen: false, ns: '' })}>
        <ModalContent className="sm:max-w-md">
          <ModalHeader className="flex flex-col gap-1">Create Channel</ModalHeader>
          <ModalBody>
            <form id="create-channel-form" onSubmit={submitChannel} className="space-y-4">
              <div>
                <Label className="mb-1 block text-sm font-medium">Channel Name</Label>
                <Input
                  className="w-full"
                  placeholder="incident-room"
                  value={channelForm.name}
                  onChange={(event) => setChannelForm((prev) => ({ ...prev, name: event.target.value }))}
                  required
                />
              </div>
              <div>
                <Label className="mb-1 block text-sm font-medium">Title</Label>
                <Input
                  className="w-full"
                  placeholder="Incident Room"
                  value={channelForm.title}
                  onChange={(event) => setChannelForm((prev) => ({ ...prev, title: event.target.value }))}
                />
              </div>
            </form>
          </ModalBody>
          <ModalFooter>
            <Button variant="ghost" appearance="outline" onClick={() => setChannelModalOpen({ isOpen: false, ns: '' })}>
              Cancel
            </Button>
            <Button color="primary" type="submit" form="create-channel-form" disabled={createChannelMutation.isPending || !channelForm.name.trim()}>
              Create Channel
            </Button>
          </ModalFooter>
        </ModalContent>
      </DialogOverlay>

      <DialogOverlay
        isOpen={subscriptionModalOpen.isOpen}
        onOpenChange={(open) => !open && setSubscriptionModalOpen({ isOpen: false, ns: '', channel: '' })}
      >
        <ModalContent className="sm:max-w-md">
          <ModalHeader className="flex flex-col gap-1">Create Channel Subscription</ModalHeader>
          <ModalBody>
            <form id="create-channel-subscription-form" onSubmit={submitSubscription} className="space-y-4">
              <div>
                <Label className="mb-1 block text-sm font-medium">Subscription Name</Label>
                <Input
                  className="w-full"
                  placeholder="triage"
                  value={subscriptionForm.name}
                  onChange={(event) => setSubscriptionForm((prev) => ({ ...prev, name: event.target.value }))}
                  required
                />
              </div>
              <div>
                <Label className="mb-1 block text-sm font-medium">Agent</Label>
                <Input
                  className="w-full"
                  placeholder="triage-agent"
                  value={subscriptionForm.agent}
                  onChange={(event) => setSubscriptionForm((prev) => ({ ...prev, agent: event.target.value }))}
                  required
                />
              </div>
              <div>
                <Label className="mb-1 block text-sm font-medium">Trigger</Label>
                <Select
                  value={subscriptionForm.trigger}
                  onChange={(key) => setSubscriptionForm((prev) => ({ ...prev, trigger: key as string }))}
                  isRequired
                >
                  <SelectTrigger className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem id="mention">mention</SelectItem>
                    <SelectItem id="manual">manual</SelectItem>
                    <SelectItem id="all">all</SelectItem>
                    <SelectItem id="disabled">disabled</SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <div>
                <Label className="mb-1 block text-sm font-medium">Reply Mode</Label>
                <Select
                  value={subscriptionForm.replyMode}
                  onChange={(key) => setSubscriptionForm((prev) => ({ ...prev, replyMode: key as string }))}
                  isRequired
                >
                  <SelectTrigger className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem id="tool">tool</SelectItem>
                    <SelectItem id="none">none</SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <label className="flex items-center gap-2 text-sm text-foreground">
                <input
                  type="checkbox"
                  checked={subscriptionForm.enabled}
                  onChange={(event) => setSubscriptionForm((prev) => ({ ...prev, enabled: event.target.checked }))}
                />
                Enabled
              </label>
            </form>
          </ModalBody>
          <ModalFooter>
            <Button variant="ghost" appearance="outline" onClick={() => setSubscriptionModalOpen({ isOpen: false, ns: '', channel: '' })}>
              Cancel
            </Button>
            <Button
              color="primary"
              type="submit"
              form="create-channel-subscription-form"
              disabled={createSubscriptionMutation.isPending || !subscriptionForm.name.trim() || !subscriptionForm.agent.trim()}
            >
              Create Subscription
            </Button>
          </ModalFooter>
        </ModalContent>
      </DialogOverlay>

      <DialogOverlay isOpen={deleteConfirm.isOpen} onOpenChange={(open) => !open && setDeleteConfirm({ isOpen: false, node: null })}>
        <ModalContent className="sm:max-w-md">
          <ModalHeader className="flex flex-col gap-1 text-red-500">Confirm Deletion</ModalHeader>
          <ModalBody>
            <div className="whitespace-pre-wrap text-[13px] text-muted-foreground">
              Are you sure you want to delete the {deleteConfirm.node?.selection.type} <b>{deleteConfirm.node?.name}</b>?
              {deleteConfirm.node?.selection.type === 'namespace' &&
                '\n\nThis will permanently delete all enclosed agents and sessions natively.'}
              {deleteConfirm.node?.selection.type === 'agent' &&
                '\n\nThis action will also sever and erase all associated execution history contexts and sessions.'}
            </div>
          </ModalBody>
          <ModalFooter className="mt-4 flex justify-end gap-3">
            <Button variant="ghost" appearance="outline" onClick={() => setDeleteConfirm({ isOpen: false, node: null })}>
              Cancel
            </Button>
            <Button
              variant="danger"
              onClick={() => deleteConfirm.node && deleteMutation.mutate(deleteConfirm.node)}
              disabled={deleteMutation.isPending}
            >
              {deleteMutation.isPending ? 'Deleting...' : 'Permanently Delete'}
            </Button>
          </ModalFooter>
        </ModalContent>
      </DialogOverlay>
    </div>
  );
}
