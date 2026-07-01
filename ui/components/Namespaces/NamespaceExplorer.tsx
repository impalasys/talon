import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { useMutation, useQueries, useQueryClient } from '@tanstack/react-query';
import { Box, ChevronRight, Cpu, Hash, PlusCircle, Radio, Trash2 } from 'lucide-react';
import { Label } from 'react-aria-components';
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
import { useExplorerTree, type TreeNode } from '../../hooks/useExplorerTree';
import {
  RESOURCE_KIND_BY_SELECTION,
  SYSTEM_NAMESPACE,
  selectionExpansionIds,
  type Selection,
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
import { ExplorerTree } from './ExplorerTree';

const API_VERSION = 'talon.impalasys.com/v1';

function SectionShell({
  icon,
  title,
  children,
  collapsed,
  onToggle,
  grow = false,
}: {
  icon: ReactNode;
  title: string;
  children: ReactNode;
  collapsed: boolean;
  onToggle: () => void;
  grow?: boolean;
}) {
  return (
    <section
      className={cn('flex min-h-0 flex-col overflow-hidden', grow && 'flex-1', !grow && 'flex-none')}
    >
      <button
        type="button"
        onClick={onToggle}
        className="flex items-center gap-2 px-4 py-3 text-left transition-colors hover:bg-white/[0.04]"
      >
        <ChevronRight className={cn('h-3.5 w-3.5 text-muted-foreground transition-transform', !collapsed && 'rotate-90')} />
        {icon}
        <h3 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">{title}</h3>
      </button>
      {!collapsed ? (
        <div className={cn('min-h-0 flex-1 overflow-y-auto px-4 pb-4 custom-scrollbar', !grow && 'flex-shrink-0')}>
          {children}
        </div>
      ) : null}
    </section>
  );
}

function namespaceParent(ns: string) {
  const parts = ns.split(':').filter(Boolean);
  parts.pop();
  return parts.join(':');
}

function templateOptionId(template: ResourceEnvelope) {
  return `${template.metadata?.namespace || SYSTEM_NAMESPACE}/${template.metadata?.name || ''}`;
}

export function NamespaceExplorer({
  isConnected,
  selectedNode,
  onSelect,
  queryScope,
}: {
  isConnected: boolean;
  selectedNode: Selection | null;
  onSelect: (selection: Selection) => void;
  queryScope: TalonQueryScope;
}) {
  const queryClient = useQueryClient();
  const [expanded, setExpanded] = useState<Set<string>>(new Set(['']));
  const [namespaceModalOpen, setNamespaceModalOpen] = useState(false);
  const [newNamespace, setNewNamespace] = useState('');
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; node: TreeNode } | null>(null);
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
  const [deleteConfirm, setDeleteConfirm] = useState<{ isOpen: boolean; node: TreeNode | null }>({ isOpen: false, node: null });
  const [explorerCollapsed, setExplorerCollapsed] = useState(false);

  useEffect(() => {
    if (!selectedNode?.ns) return;
    setExpanded((prev) => {
      const next = new Set(prev);
      let changed = false;
      for (const id of selectionExpansionIds(selectedNode)) {
        if (!next.has(id)) {
          next.add(id);
          changed = true;
        }
      }
      return changed ? next : prev;
    });
  }, [selectedNode]);

  useEffect(() => {
    if (selectedNode && (selectedNode.type === 'namespace' || selectedNode.type === 'agent') && !newNamespace) {
      setNewNamespace(`${selectedNode.ns}:`);
    }
  }, [newNamespace, selectedNode]);

  const queries = useExplorerQueries({
    isConnected,
    scope: queryScope,
    expanded,
    selectedNode,
  });
  const treeStructure = useExplorerTree({ queries, expanded, selectedNode });

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
      setExpanded((prev) => new Set(prev).add(name));
      setNewNamespace('');
      setNamespaceModalOpen(false);
    },
  });

  const createAgentMutation = useMutation({
    mutationFn: async () => {
      const selectedTemplate = agentCreateTemplates.find((template) => templateOptionId(template) === agentForm.template);
      const templateSpec = selectedTemplate ? resourceSpec(selectedTemplate, 'template') : null;
      if (templateSpec && templateSpec.kind !== 'Agent') {
        throw new Error(`Template '${agentForm.template}' renders ${templateSpec.kind}, not Agent`);
      }
      const agentSpec = templateSpec ? parseJsonObject(templateSpec.specJson) : { systemPrompt: '' };
      return createResource(agentModalOpen.ns, {
        apiVersion: API_VERSION,
        kind: 'Agent',
        metadata: resourceMetadata(agentForm.name.trim(), agentModalOpen.ns),
        spec: { kind: { case: 'agent', value: agentSpec } },
      });
    },
    onSuccess: async () => {
      await invalidateNamespace(agentModalOpen.ns, ['Agent']);
      setExpanded((prev) => new Set(prev).add(agentModalOpen.ns));
      setAgentModalOpen({ isOpen: false, ns: '' });
      setAgentForm({ name: '', template: '' });
    },
  });

  const createChannelMutation = useMutation({
    mutationFn: () =>
      createResource(channelModalOpen.ns, {
        apiVersion: API_VERSION,
        kind: 'Channel',
        metadata: resourceMetadata(channelForm.name.trim(), channelModalOpen.ns),
        spec: { kind: { case: 'channel', value: { title: channelForm.title.trim(), metadata: {} } } },
      }),
    onSuccess: async () => {
      await invalidateNamespace(channelModalOpen.ns, ['Channel']);
      setExpanded((prev) => new Set(prev).add(channelModalOpen.ns));
      setChannelModalOpen({ isOpen: false, ns: '' });
      setChannelForm({ name: '', title: '' });
    },
  });

  const createSubscriptionMutation = useMutation({
    mutationFn: () =>
      createResource(subscriptionModalOpen.ns, {
        apiVersion: API_VERSION,
        kind: 'ChannelSubscription',
        metadata: resourceMetadata(subscriptionForm.name.trim(), subscriptionModalOpen.ns),
        spec: {
          kind: {
            case: 'channelSubscription',
            value: {
              channel: subscriptionModalOpen.channel,
              agent: subscriptionForm.agent.trim(),
              enabled: subscriptionForm.enabled,
              trigger: subscriptionForm.trigger,
              replyMode: subscriptionForm.replyMode,
            },
          },
        },
      }),
    onSuccess: async () => {
      await invalidateNamespace(subscriptionModalOpen.ns, ['ChannelSubscription']);
      setExpanded((prev) => new Set(prev).add(`${subscriptionModalOpen.ns}:channel:${subscriptionModalOpen.channel}`));
      setSubscriptionModalOpen({ isOpen: false, ns: '', channel: '' });
      setSubscriptionForm({ name: '', agent: '', trigger: 'mention', replyMode: 'tool', enabled: true });
    },
  });

  const createSessionMutation = useMutation({
    mutationFn: ({ ns, agent }: { ns: string; agent: string }) => createSession(ns, agent),
    onSuccess: async (_, { ns, agent }) => {
      await queryClient.invalidateQueries({ queryKey: talonQueryKeys.sessions(queryScope, ns, agent) });
      setExpanded((prev) => new Set(prev).add(`${ns}:${agent}`));
    },
  });

  const deleteMutation = useMutation({
    mutationFn: async (node: TreeNode) => {
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

  const handleContextMenu = (event: React.MouseEvent, node: TreeNode) => {
    event.preventDefault();
    setContextMenu({ x: event.clientX, y: event.clientY, node });
  };

  const toggleExpanded = (id: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const menuNode = contextMenu?.node;

  const submitNamespace = (event: React.FormEvent) => {
    event.preventDefault();
    const name = newNamespace.trim();
    if (!name) return;
    createNamespaceMutation.mutate(name);
  };

  const submitAgent = (event: React.FormEvent) => {
    event.preventDefault();
    if (!agentForm.name.trim()) return;
    createAgentMutation.mutate();
  };

  const submitChannel = (event: React.FormEvent) => {
    event.preventDefault();
    if (!channelForm.name.trim()) return;
    createChannelMutation.mutate();
  };

  const submitSubscription = (event: React.FormEvent) => {
    event.preventDefault();
    if (!subscriptionForm.name.trim() || !subscriptionForm.agent.trim()) return;
    createSubscriptionMutation.mutate();
  };

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

  return (
    <div className="relative flex h-full flex-col divide-y divide-border/70 overflow-hidden bg-background/68 shadow-[inset_0_1px_0_rgba(255,255,255,0.03)] backdrop-blur-xl">
      <SectionShell
        icon={<Box className="h-3.5 w-3.5 text-muted-foreground stroke-[1.5]" />}
        title="Explorer"
        collapsed={explorerCollapsed}
        onToggle={() => setExplorerCollapsed((collapsed) => !collapsed)}
        grow
      >
        <div onContextMenu={(event) => handleContextMenu(event, treeStructure)}>
          <ExplorerTree
            tree={treeStructure}
            selectedNode={selectedNode}
            onSelect={onSelect}
            onContextMenu={handleContextMenu}
            expanded={expanded}
            toggleExpanded={toggleExpanded}
          />
          {queries.isInitialLoading ? (
            <div className="px-2 py-2 text-[11px] text-muted-foreground">Loading namespaces...</div>
          ) : null}
        </div>
      </SectionShell>

      <Dropdown isOpen={!!contextMenu} onOpenChange={(open) => !open && setContextMenu(null)}>
        <DropdownTrigger
          className="fixed"
          style={{ top: contextMenu?.y || 0, left: contextMenu?.x || 0, width: 1, height: 1, padding: 0 }}
          aria-label="Explorer context menu"
        />
        <DropdownMenu aria-label="Context Actions" variant="outline">
          {menuNode?.selection.type === 'namespace' ? (
            <DropdownItem
              id="create_namespace"
              onAction={() => {
                setContextMenu(null);
                setNewNamespace(menuNode.selection.ns ? `${menuNode.selection.ns}:` : '');
                setNamespaceModalOpen(true);
              }}
            >
              <div className="flex items-center gap-2">
                <PlusCircle className="h-4 w-4 text-muted-foreground" />
                New Namespace
              </div>
            </DropdownItem>
          ) : null}

          {menuNode?.selection.type === 'namespace' ? (
            <DropdownItem
              id="create_agent"
              onAction={() => {
                setContextMenu(null);
                setAgentModalOpen({ isOpen: true, ns: menuNode.selection.ns });
              }}
            >
              <div className="flex items-center gap-2">
                <Cpu className="h-4 w-4 text-muted-foreground" />
                Create Agent
              </div>
            </DropdownItem>
          ) : null}

          {menuNode?.selection.type === 'namespace' ? (
            <DropdownItem
              id="create_channel"
              onAction={() => {
                setContextMenu(null);
                setChannelModalOpen({ isOpen: true, ns: menuNode.selection.ns });
              }}
            >
              <div className="flex items-center gap-2">
                <Hash className="h-4 w-4 text-muted-foreground" />
                Create Channel
              </div>
            </DropdownItem>
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
                <PlusCircle className="h-4 w-4 text-muted-foreground" />
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
                <Radio className="h-4 w-4 text-muted-foreground" />
                Create Subscription
              </div>
            </DropdownItem>
          ) : null}

          <DropdownItem
            id="delete_item"
            className="text-danger"
            color="danger"
            onAction={() => {
              setContextMenu(null);
              setDeleteConfirm({ isOpen: true, node: menuNode || null });
            }}
          >
            <div className="flex items-center gap-2">
              <Trash2 className="h-4 w-4" />
              Delete {menuNode?.selection.type}
            </div>
          </DropdownItem>
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

export type { Selection };
