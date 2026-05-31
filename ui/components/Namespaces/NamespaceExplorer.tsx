import { useState, useEffect, useCallback, type ReactNode } from 'react';
import { Box, Activity, ChevronRight, ChevronDown, Folder, Cpu, MessageSquare, Trash2, PlusCircle, Plug, Clock3, FileText, Hash, Radio } from 'lucide-react';
import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';
import { getGatewayClient, buildGatewayHeaders, normalizeGatewayUrl } from '../../lib/grpc';
import { AnimatePresence, motion } from 'framer-motion';
import { Button } from "../tailgrids/core/button";
import { DropdownMenu as Dropdown, DropdownMenuTrigger as DropdownTrigger, DropdownMenuContent as DropdownMenu, DropdownMenuItem as DropdownItem } from "../tailgrids/core/dropdown";
import { Dialog as Modal, DialogOverlay, DialogContent as ModalContent, DialogHeader as ModalHeader, DialogBody as ModalBody, DialogFooter as ModalFooter, DialogTitle } from "../tailgrids/core/dialog";
import { Input } from "../tailgrids/core/input";
import { Select, SelectItem, SelectTrigger, SelectValue, SelectContent } from "../tailgrids/core/select";
import { Label } from "react-aria-components";

function parseSessionDate(id: string) {
  // Try UUIDv7
  if (id && id.length === 36 && id.charAt(8) === '-') {
    const hex = id.substring(0, 13).replace('-', '');
    const time = parseInt(hex, 16);
    return new Date(time).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  }
  
  // Try ULID
  if (id && id.length === 26) {
    const ENCODING = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let time = 0;
    for (let i = 0; i < 10; i++) {
      const val = ENCODING.indexOf(id.charAt(i).toUpperCase());
      if (val === -1) return id.substring(0, 8);
      time = (time * 32) + val;
    }
    return new Date(time).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  }
  
  return id ? id.substring(0, 8) : 'Unknown';
}

function parseSessionTimestamp(id: string): number | null {
  // UUIDv7
  if (id && id.length === 36 && id.charAt(8) === '-') {
    const hex = id.substring(0, 13).replace('-', '');
    const time = parseInt(hex, 16);
    return Number.isNaN(time) ? null : time;
  }

  // ULID
  if (id && id.length === 26) {
    const ENCODING = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let time = 0;
    for (let i = 0; i < 10; i++) {
      const val = ENCODING.indexOf(id.charAt(i).toUpperCase());
      if (val === -1) return null;
      time = (time * 32) + val;
    }
    return time;
  }

  return null;
}

function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export type SelectionType = 'namespace' | 'agent' | 'session' | 'channel' | 'channel-subscription' | 'schedule' | 'template' | 'mcp-server' | 'mcp-binding' | 'knowledge';

export type Selection = {
  type: SelectionType;
  ns: string;
  agent?: string;
  channel?: string;
  sessionId?: string;
  resourceName?: string;
  fullPath: string;
};

export type TreeNode = {
  id: string;
  name: string;
  selection: Selection;
  badge?: string;
  children: { [key: string]: TreeNode };
};

type ExplorerMcpServer = {
  metadata?: {
    name?: string;
    namespace?: string;
    labels?: Record<string, string>;
  };
  spec?: {
    transport?: string;
    target?: string;
    disabled?: boolean;
  };
};

type ExplorerMcpBinding = {
  metadata?: {
    name?: string;
    namespace?: string;
    labels?: Record<string, string>;
  };
  spec?: {
    serverRef?: string;
    disabled?: boolean;
    authBroker?: {
      kind?: string;
    };
  };
};

type ExplorerSchedule = {
  name?: string;
  ns?: string;
  labels?: Record<string, string>;
  spec?: {
    kind?: string;
    enabled?: boolean;
  };
  status?: {
    nextRunAt?: bigint | number | string;
    lastError?: string;
  };
};

type ExplorerKnowledge = {
  metadata?: {
    name?: string;
    namespace?: string;
    labels?: Record<string, string>;
  };
  spec?: {
    path?: string;
    content?: string;
  };
};

type ExplorerChannel = {
  name?: string;
  ns?: string;
  title?: string;
  status?: string;
  updatedAt?: bigint | number | string;
  updated_at?: bigint | number | string;
  labels?: Record<string, string>;
};

type ExplorerChannelSubscription = {
  name?: string;
  ns?: string;
  channel?: string;
  agent?: string;
  enabled?: boolean;
  trigger?: string;
};

function namespaceLabel(labels?: Record<string, string>) {
  return labels?.workspace_name || labels?.workspace || labels?.display_name || labels?.name;
}

function nodeSortWeight(node: TreeNode) {
  switch (node.selection.type) {
    case 'namespace':
      return 0;
    case 'agent':
      return 1;
    case 'channel':
      return 2;
    case 'channel-subscription':
      return 3;
    case 'mcp-binding':
      return 4;
    case 'schedule':
      return 5;
    case 'session':
      return 6;
    default:
      return 7;
  }
}

function compareTreeNodes(a: TreeNode, b: TreeNode) {
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

function namespaceAncestors(ns: string) {
  if (!ns) return [''];
  const parts = ns.split(':');
  const ancestors = [''];
  for (let i = 0; i < parts.length; i++) {
    ancestors.push(parts.slice(0, i + 1).join(':'));
  }
  return ancestors;
}

function selectionExpansionIds(selection: Selection | null) {
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

function collectExpandedNamespaceIds(expanded: Set<string>, selectedNode: Selection | null) {
  const namespaces = new Set<string>();

  for (const id of expanded) {
    if (id) {
      namespaces.add(id);
    }
  }

  if (selectedNode?.ns) {
    for (const ns of selectionExpansionIds(selectedNode)) {
      if (ns) {
        namespaces.add(ns);
      }
    }
  }

  return namespaces;
}

function SectionShell({
  icon,
  title,
  children,
  collapsed,
  onToggle,
  grow = false,
  height,
}: {
  icon: ReactNode;
  title: string;
  children: ReactNode;
  collapsed: boolean;
  onToggle: () => void;
  grow?: boolean;
  height?: number;
}) {
  return (
    <section
      className={cn("flex min-h-0 flex-col overflow-hidden", grow && "flex-1", !grow && "flex-none")}
      style={!grow && !collapsed && height ? { height: `${height}px` } : undefined}
    >
      <button
        type="button"
        onClick={onToggle}
        className="flex items-center gap-2 px-4 py-3 text-left transition-colors hover:bg-white/[0.04]"
      >
        <ChevronRight className={cn("h-3.5 w-3.5 text-muted-foreground transition-transform", !collapsed && "rotate-90")} />
        {icon}
        <h3 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">{title}</h3>
      </button>
      {!collapsed && (
        <div className={cn("min-h-0 flex-1 overflow-y-auto px-4 pb-4 custom-scrollbar", !grow && "flex-shrink-0")}>
          {children}
        </div>
      )}
    </section>
  );
}

function SplitHandle({ onMouseDown }: { onMouseDown: (event: React.MouseEvent<HTMLButtonElement>) => void }) {
  return (
    <button
      type="button"
      aria-label="Resize panels"
      onMouseDown={onMouseDown}
      className="flex h-3 flex-none cursor-row-resize items-center justify-center border-y border-border/70 hover:bg-white/[0.035]"
    >
      <span className="h-px w-10 rounded-full bg-white/10" />
    </button>
  );
}

function ResourceList({
  items,
  emptyState,
  selectedId,
  onSelect,
}: {
  items: Array<{
    id: string;
    name: string;
    subtitle?: string;
    tag?: string;
    tone?: 'default' | 'success' | 'warning';
    selection: Selection;
  }>;
  emptyState: string;
  selectedId: string | null;
  onSelect: (selection: Selection) => void;
}) {
  if (items.length === 0) {
    return <div className="py-3 text-center text-[11px] text-muted-foreground">{emptyState}</div>;
  }

  return (
    <div className="space-y-2">
      {items.map((item) => (
        <button
          type="button"
          key={item.id}
          onClick={() => onSelect(item.selection)}
          className={cn(
            "w-full rounded-2xl border px-3 py-2.5 text-left transition-colors",
            selectedId === item.id
              ? "border-border/80 bg-white/[0.05] shadow-[inset_0_1px_0_rgba(255,255,255,0.03)]"
              : "border-border/70 bg-white/[0.028] hover:bg-white/[0.045]"
          )}
        >
          <div className="flex items-start justify-between gap-2">
            <div className="min-w-0">
              <div className="truncate text-[13px] font-medium text-foreground">{item.name}</div>
              {item.subtitle && (
                <div className="mt-1 line-clamp-2 text-[11px] text-muted-foreground">{item.subtitle}</div>
              )}
            </div>
            {item.tag && (
              <span
                className={cn(
                  "shrink-0 rounded-full border px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide",
                  item.tone === 'success' && "border-emerald-500/14 bg-emerald-500/8 text-emerald-300",
                  item.tone === 'warning' && "border-amber-500/14 bg-amber-500/8 text-amber-300",
                  item.tone !== 'success' && item.tone !== 'warning' && "border-border/70 bg-white/[0.03] text-muted-foreground",
                )}
              >
                {item.tag}
              </span>
            )}
          </div>
        </button>
      ))}
    </div>
  );
}

function NamespaceNode({ 
  node, 
  level, 
  selectedNodeId, 
  onSelect, 
  onContextMenu,
  expanded, 
  toggleExpanded 
}: { 
  node: TreeNode, 
  level: number,
  selectedNodeId: string | null,
  onSelect: (selection: Selection) => void,
  onContextMenu: (e: React.MouseEvent, node: TreeNode) => void,
  expanded: Set<string>,
  toggleExpanded: (id: string) => void
}) {
  const childNodes = Object.values(node.children).sort(compareTreeNodes);
  const hasChildrenOrCanHaveChildren = !['session', 'schedule', 'knowledge', 'mcp-binding', 'channel-subscription'].includes(node.selection.type);
  const isExpanded = expanded.has(node.id);
  const isSelected = selectedNodeId === node.id;

  if (level === -1) {
    return (
      <div className="space-y-0.5 relative">
        {childNodes.map(child => (
          <NamespaceNode 
            key={child.id} 
            node={child} 
            level={0} 
            selectedNodeId={selectedNodeId} 
            onSelect={onSelect} 
            onContextMenu={onContextMenu}
            expanded={expanded} 
            toggleExpanded={toggleExpanded} 
          />
        ))}
      </div>
    );
  }

  const handleToggle = (e: React.MouseEvent) => {
    e.stopPropagation(); 
    if (hasChildrenOrCanHaveChildren) toggleExpanded(node.id);
  };

  const handleClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    onSelect(node.selection);
  };

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    onContextMenu(e, node);
  };

  return (
    <div>
      <div 
        className={cn(
          "flex items-center gap-1.5 rounded-xl px-2.5 py-2 text-[13px] font-medium select-none group transition-colors hover:bg-white/[0.04]",
          isSelected ? "border border-border/70 bg-white/[0.055] text-foreground shadow-[inset_0_1px_0_rgba(255,255,255,0.03)]" : "text-muted-foreground"
        )}
        style={{ paddingLeft: `${level * 16 + 8}px` }}
        onClick={handleClick}
        onContextMenu={handleContextMenu}
      >
         <div 
           className={cn("p-0.5 rounded-sm flex items-center justify-center transition-colors", 
            hasChildrenOrCanHaveChildren && "hover:bg-white/[0.05]",
            !hasChildrenOrCanHaveChildren && "opacity-0 cursor-default"
           )}
           onClick={hasChildrenOrCanHaveChildren ? handleToggle : undefined}
         >
           {isExpanded ? <ChevronDown className="w-3.5 h-3.5" /> : <ChevronRight className="w-3.5 h-3.5" />}
         </div>
         
         {node.selection.type === 'namespace' && <Folder className={cn("w-3.5 h-3.5", isSelected ? "text-foreground" : "text-muted-foreground")} />}
         {node.selection.type === 'agent' && <Cpu className={cn("w-3.5 h-3.5", isSelected ? "text-foreground" : "text-emerald-500")} />}
         {node.selection.type === 'session' && <MessageSquare className={cn("w-3.5 h-3.5", isSelected ? "text-foreground" : "text-blue-500")} />}
         {node.selection.type === 'channel' && <Hash className={cn("w-3.5 h-3.5", isSelected ? "text-foreground" : "text-cyan-400")} />}
         {node.selection.type === 'channel-subscription' && <Radio className={cn("w-3.5 h-3.5", isSelected ? "text-foreground" : "text-cyan-300")} />}
         {node.selection.type === 'schedule' && <Clock3 className={cn("w-3.5 h-3.5", isSelected ? "text-foreground" : "text-amber-500")} />}
         {node.selection.type === 'mcp-binding' && <Plug className={cn("w-3.5 h-3.5", isSelected ? "text-foreground" : "text-blue-500")} />}
         {node.selection.type === 'knowledge' && <FileText className={cn("w-3.5 h-3.5", isSelected ? "text-foreground" : "text-violet-400")} />}

         <span className="truncate flex-1">{node.name}</span>
         {node.badge && (
           <span className="max-w-[8rem] truncate rounded-full border border-border/70 bg-white/[0.03] px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
             {node.badge}
           </span>
         )}
         {isSelected && <Activity className="w-3.5 h-3.5 opacity-70 flex-shrink-0" />}
      </div>
      
      <AnimatePresence initial={false}>
        {isExpanded && hasChildrenOrCanHaveChildren && (
          <motion.div 
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.15, ease: "easeInOut" }}
            className="overflow-hidden"
          >
             <div className="mt-0.5 space-y-0.5">
               {childNodes.length > 0 ? childNodes.map(child => (
                 <NamespaceNode 
                   key={child.id} 
                   node={child} 
                   level={level + 1} 
                   selectedNodeId={selectedNodeId} 
                   onSelect={onSelect} 
                   onContextMenu={onContextMenu}
                   expanded={expanded} 
                   toggleExpanded={toggleExpanded} 
                 />
               )) : (
                 <div 
                   className="flex items-center gap-1.5 px-2 py-1.5 text-[12px] italic text-muted-foreground/60"
                   style={{ paddingLeft: `${(level+1) * 16 + 28}px` }}
                 >
                   Empty
                 </div>
               )}
             </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

export function NamespaceExplorer({ 
  isConnected, 
  gatewayUrl,
  selectedNode, 
  onSelect 
}: { 
  isConnected: boolean, 
  gatewayUrl: string,
  selectedNode: Selection | null, 
  onSelect: (selection: Selection) => void 
}) {
  const [treeStructure, setTreeStructure] = useState<TreeNode>({
    id: '', name: 'root', selection: { type: 'namespace', ns: '', fullPath: '' }, children: {}
  });
  
  const [expanded, setExpanded] = useState<Set<string>>(new Set(['']));
  const [namespaceModalOpen, setNamespaceModalOpen] = useState(false);
  const [isCreatingNamespace, setIsCreatingNamespace] = useState(false);
  const [newNamespace, setNewNamespace] = useState('');

  // Context Menu State
  const [contextMenu, setContextMenu] = useState<{ x: number, y: number, node: TreeNode } | null>(null);

  // Modals state
  const [agentModalOpen, setAgentModalOpen] = useState<{ isOpen: boolean, ns: string }>({ isOpen: false, ns: '' });
  const [agentForm, setAgentForm] = useState({ name: '', template: '' });
  const [templates, setTemplates] = useState<any[]>([]);
  const [mcpServers, setMcpServers] = useState<ExplorerMcpServer[]>([]);
  const [mcpBindingsByNamespace, setMcpBindingsByNamespace] = useState<Record<string, ExplorerMcpBinding[]>>({});
  const [schedulesByNamespace, setSchedulesByNamespace] = useState<Record<string, ExplorerSchedule[]>>({});
  const [knowledgeByNamespace, setKnowledgeByNamespace] = useState<Record<string, ExplorerKnowledge[]>>({});
  const [channelsByNamespace, setChannelsByNamespace] = useState<Record<string, ExplorerChannel[]>>({});
  const [channelSubscriptionsByKey, setChannelSubscriptionsByKey] = useState<Record<string, ExplorerChannelSubscription[]>>({});
  const [isSubmittingAgent, setIsSubmittingAgent] = useState(false);
  const [channelModalOpen, setChannelModalOpen] = useState<{ isOpen: boolean, ns: string }>({ isOpen: false, ns: '' });
  const [channelForm, setChannelForm] = useState({ name: '', title: '' });
  const [isSubmittingChannel, setIsSubmittingChannel] = useState(false);
  const [subscriptionModalOpen, setSubscriptionModalOpen] = useState<{ isOpen: boolean, ns: string, channel: string }>({ isOpen: false, ns: '', channel: '' });
  const [subscriptionForm, setSubscriptionForm] = useState({ name: '', agent: '', trigger: 'mention', enabled: true });
  const [isSubmittingSubscription, setIsSubmittingSubscription] = useState(false);

  const [deleteConfirm, setDeleteConfirm] = useState<{ isOpen: boolean, node: TreeNode | null }>({ isOpen: false, node: null });
  const [isDeleting, setIsDeleting] = useState(false);
  const [collapsedSections, setCollapsedSections] = useState({
    explorer: false,
    templates: false,
    mcpServers: false,
  });
  const [sectionHeights, setSectionHeights] = useState({
    explorer: 360,
    templates: 168,
  });

  const refreshData = useCallback(async () => {
    if (!isConnected) return;
    try {
      const newTree: TreeNode = {
        id: '', name: 'root', selection: { type: 'namespace', ns: '', fullPath: '' }, children: {}
      };

      // We discover namespaces starting from the root
      const discoverQueue: string[] = Array.from(
        new Set([
          '',
          ...Array.from(expanded),
          ...selectionExpansionIds(selectedNode),
        ]),
      );
      const processedNamespaces = new Set<string>();

      while (discoverQueue.length > 0) {
        const parentNs = discoverQueue.shift()!;
        if (processedNamespaces.has(parentNs)) continue;
        processedNamespaces.add(parentNs);

        try {
          const res = await getGatewayClient().listNamespaces({ parent: parentNs || undefined });
          const namespaces = (res.namespaces || []).slice().sort((left, right) => left.name.localeCompare(right.name));

          for (const namespace of namespaces) {
            const ns = namespace.name;
            const parts = ns.split(':');
            let currentLevel = newTree;
            
            for (let i = 0; i < parts.length; i++) {
               const part = parts[i];
               const currentNsId = parts.slice(0, i + 1).join(':');
               if (!currentLevel.children[part]) {
                   currentLevel.children[part] = {
                       id: currentNsId,
                       name: part,
                       badge: i === parts.length - 1 ? namespaceLabel(namespace.labels) : undefined,
                       selection: { type: 'namespace', ns: currentNsId, fullPath: currentNsId },
                       children: {}
                   };
               } else if (i === parts.length - 1) {
                   currentLevel.children[part].badge = namespaceLabel(namespace.labels);
               }
               currentLevel = currentLevel.children[part];
            }
            
            // If this namespace is expanded, we need to discover its children too
            if (expanded.has(ns)) {
              discoverQueue.push(ns);
            }

            // Fetch agents for this namespace
            try {
              const agentRes = await getGatewayClient().listAgents({ ns });
              for (const agent of (agentRes.agents || [])) {
                const agentId = `${ns}:${agent}`;
                currentLevel.children[agent] = {
                  id: agentId,
                  name: agent,
                  selection: { type: 'agent', ns, agent, fullPath: agentId },
                  children: {}
                };
                
                if (expanded.has(agentId)) {
                    try {
                      const sessionRes = await getGatewayClient().listSessions({ ns, agent });
                      for (const sessionId of (sessionRes.sessionIds || [])) {
                        const sessionFullId = `${ns}:${agent}:${sessionId}`;
                        currentLevel.children[agent].children[sessionId] = {
                          id: sessionFullId,
                          name: parseSessionDate(sessionId),
                          selection: { type: 'session', ns, agent, sessionId, fullPath: sessionFullId },
                          children: {}
                        };
                      }
                    } catch (e) {
                      console.warn(`Could not fetch sessions for agent ${agentId}`, e);
                    }
                }
              }
            } catch (e) {
              console.warn(`Could not fetch agents for ns ${ns}`, e);
            }

            const namespaceChannels = channelsByNamespace[ns] || [];
            for (const channel of namespaceChannels) {
              const channelName = channel.name || 'unknown-channel';
              const channelId = `${ns}:channel:${channelName}`;
              const status = channel.status || 'open';
              currentLevel.children[`channel:${channelName}`] = {
                id: channelId,
                name: channelName,
                badge: status === 'closed' ? 'closed' : (channel.title || 'channel'),
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
                const subscriptions = channelSubscriptionsByKey[`${ns}/${channelName}`] || [];
                for (const subscription of subscriptions) {
                  const subscriptionName = subscription.name || 'unknown-subscription';
                  const subscriptionId = `${channelId}:subscription:${subscriptionName}`;
                  currentLevel.children[`channel:${channelName}`].children[`subscription:${subscriptionName}`] = {
                    id: subscriptionId,
                    name: subscriptionName,
                    badge: subscription.enabled === false ? 'disabled' : (subscription.trigger || 'mention'),
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

            const namespaceBindings = mcpBindingsByNamespace[ns] || [];
            for (const binding of namespaceBindings) {
              const bindingName = binding.metadata?.name || 'unknown-binding';
              const bindingId = `${ns}:mcp-binding:${bindingName}`;
              currentLevel.children[`mcp-binding:${bindingName}`] = {
                id: bindingId,
                name: bindingName,
                badge: binding.spec?.serverRef || undefined,
                selection: {
                  type: 'mcp-binding',
                  ns,
                  resourceName: bindingName,
                  fullPath: bindingId,
                },
                children: {},
              };
            }

            const namespaceSchedules = schedulesByNamespace[ns] || [];
            for (const schedule of namespaceSchedules) {
              const scheduleName = schedule.name || 'unknown-schedule';
              const scheduleId = `${ns}:schedule:${scheduleName}`;
              const scheduleEnabled = schedule.spec?.enabled !== false;
              currentLevel.children[`schedule:${scheduleName}`] = {
                id: scheduleId,
                name: scheduleName,
                badge: scheduleEnabled ? (schedule.spec?.kind || 'schedule') : 'disabled',
                selection: {
                  type: 'schedule',
                  ns,
                  resourceName: scheduleName,
                  fullPath: scheduleId,
                },
                children: {},
              };
            }

            const namespaceKnowledge = knowledgeByNamespace[ns] || [];
            for (const knowledge of namespaceKnowledge) {
              const knowledgeName = knowledge.metadata?.name || knowledge.spec?.path || 'unknown-knowledge';
              const knowledgePath = knowledge.spec?.path || '';
              const knowledgeId = `${ns}:knowledge:${knowledgeName}`;
              currentLevel.children[`knowledge:${knowledgeName}`] = {
                id: knowledgeId,
                name: knowledgePath || knowledgeName,
                badge: 'knowledge',
                selection: {
                  type: 'knowledge',
                  ns,
                  resourceName: knowledgeName,
                  fullPath: knowledgeId,
                },
                children: {},
              };
            }
          }
        } catch (e) {
          console.warn(`Could not list namespaces for parent ${parentNs}`, e);
        }
      }
      setTreeStructure(newTree);
    } catch (e) {
      console.error(e);
    }
  }, [channelSubscriptionsByKey, channelsByNamespace, isConnected, expanded, knowledgeByNamespace, mcpBindingsByNamespace, schedulesByNamespace, selectedNode]);

  useEffect(() => {
    if (!selectedNode?.ns) return;
    setExpanded((prev) => {
      const next = new Set(prev);
      let changed = false;
      for (const ns of selectionExpansionIds(selectedNode)) {
        if (!next.has(ns)) {
          next.add(ns);
          changed = true;
        }
      }
      return changed ? next : prev;
    });
  }, [selectedNode]);

  const refreshTemplates = useCallback(async () => {
    if (!isConnected) {
      setTemplates([]);
      return;
    }

    try {
      const res = await getGatewayClient().listAgentTemplates({});
      setTemplates(res.templates || []);
    } catch (e) {
      console.warn('Could not fetch agent templates', e);
      setTemplates([]);
    }
  }, [isConnected]);

  const refreshMcpServers = useCallback(async () => {
    if (!isConnected) {
      setMcpServers([]);
      return;
    }

    try {
      const res = await getGatewayClient().listMcpServers({});
      setMcpServers(res.servers || []);
    } catch (e) {
      console.warn('Could not fetch MCP servers', e);
      setMcpServers([]);
    }
  }, [isConnected]);

  const refreshMcpBindings = useCallback(async () => {
    if (!isConnected) {
      setMcpBindingsByNamespace({});
      return;
    }

    try {
      const namespaces = collectExpandedNamespaceIds(expanded, selectedNode);
      const baseUrl = normalizeGatewayUrl(gatewayUrl);
      const headers =
        typeof window === 'undefined'
          ? undefined
          : buildGatewayHeaders(window.localStorage.getItem('talon_auth_token'));
      const bindingEntries = await Promise.all(
        Array.from(namespaces).map(async (ns) => {
          try {
            const response = await fetch(
              `${baseUrl}/v1/namespaces/${encodeURIComponent(ns)}/mcp-bindings`,
              { headers },
            );
            if (!response.ok) {
              throw new Error(`HTTP ${response.status}`);
            }
            const payload = await response.json();
            return [ns, payload.bindings || []] as const;
          } catch (e) {
            console.warn(`Could not fetch MCP bindings for ns ${ns}`, e);
            return [ns, []] as const;
          }
        }),
      );
      setMcpBindingsByNamespace(Object.fromEntries(bindingEntries));
    } catch (e) {
      console.warn('Could not list namespaces for MCP bindings', e);
      setMcpBindingsByNamespace({});
    }
  }, [expanded, gatewayUrl, isConnected, selectedNode]);

  const refreshSchedules = useCallback(async () => {
    if (!isConnected) {
      setSchedulesByNamespace({});
      return;
    }

    try {
      const namespaces = collectExpandedNamespaceIds(expanded, selectedNode);
      const scheduleEntries = await Promise.all(
        Array.from(namespaces).map(async (ns) => {
          try {
            const response = await getGatewayClient().listSchedules({ ns });
            return [ns, response.schedules || []] as const;
          } catch (e) {
            console.warn(`Could not fetch schedules for ns ${ns}`, e);
            return [ns, []] as const;
          }
        }),
      );
      setSchedulesByNamespace(Object.fromEntries(scheduleEntries));
    } catch (e) {
      console.warn('Could not list namespaces for schedules', e);
      setSchedulesByNamespace({});
    }
  }, [expanded, isConnected, selectedNode]);

  const refreshKnowledge = useCallback(async () => {
    if (!isConnected) {
      setKnowledgeByNamespace({});
      return;
    }

    try {
      const namespaces = collectExpandedNamespaceIds(expanded, selectedNode);
      const knowledgeEntries = await Promise.all(
        Array.from(namespaces).map(async (ns) => {
          try {
            const response = await getGatewayClient().listNamespaceKnowledge({ ns });
            return [ns, response.knowledge || []] as const;
          } catch (e) {
            console.warn(`Could not fetch knowledge for ns ${ns}`, e);
            return [ns, []] as const;
          }
        }),
      );
      setKnowledgeByNamespace(Object.fromEntries(knowledgeEntries));
    } catch (e) {
      console.warn('Could not list namespaces for knowledge', e);
      setKnowledgeByNamespace({});
    }
  }, [expanded, isConnected, selectedNode]);

  const refreshChannels = useCallback(async () => {
    if (!isConnected) {
      setChannelsByNamespace({});
      setChannelSubscriptionsByKey({});
      return;
    }

    try {
      const namespaces = collectExpandedNamespaceIds(expanded, selectedNode);
      const baseUrl = normalizeGatewayUrl(gatewayUrl);
      const headers =
        typeof window === 'undefined'
          ? undefined
          : buildGatewayHeaders(window.localStorage.getItem('talon_auth_token'));
      const channelEntries = await Promise.all(
        Array.from(namespaces).map(async (ns) => {
          try {
            const response = await fetch(`${baseUrl}/v1/ns/${encodeURIComponent(ns)}/channels`, { headers });
            if (!response.ok) throw new Error(`HTTP ${response.status}`);
            const payload = await response.json();
            return [ns, payload.channels || []] as const;
          } catch (e) {
            console.warn(`Could not fetch channels for ns ${ns}`, e);
            return [ns, []] as const;
          }
        }),
      );
      const channelMap = Object.fromEntries(channelEntries);
      setChannelsByNamespace(channelMap);

      const expandedChannels = Object.entries(channelMap).flatMap(([ns, channels]) =>
        (channels as ExplorerChannel[])
          .map((channel) => channel.name || '')
          .filter((name) => name && expanded.has(`${ns}:channel:${name}`))
          .map((name) => ({ ns, name })),
      );
      const subscriptionEntries = await Promise.all(
        expandedChannels.map(async ({ ns, name }) => {
          try {
            const response = await fetch(
              `${baseUrl}/v1/ns/${encodeURIComponent(ns)}/channels/${encodeURIComponent(name)}/subscriptions`,
              { headers },
            );
            if (!response.ok) throw new Error(`HTTP ${response.status}`);
            const payload = await response.json();
            return [`${ns}/${name}`, payload.subscriptions || []] as const;
          } catch (e) {
            console.warn(`Could not fetch channel subscriptions for ${ns}/${name}`, e);
            return [`${ns}/${name}`, []] as const;
          }
        }),
      );
      setChannelSubscriptionsByKey(Object.fromEntries(subscriptionEntries));
    } catch (e) {
      console.warn('Could not list namespaces for channels', e);
      setChannelsByNamespace({});
      setChannelSubscriptionsByKey({});
    }
  }, [expanded, gatewayUrl, isConnected, selectedNode]);

  useEffect(() => {
    refreshData();
    const interval = setInterval(refreshData, 3000);
    return () => clearInterval(interval);
  }, [refreshData]);

  useEffect(() => {
    refreshTemplates();
    const interval = setInterval(refreshTemplates, 5000);
    return () => clearInterval(interval);
  }, [refreshTemplates]);

  useEffect(() => {
    refreshMcpServers();
    const interval = setInterval(refreshMcpServers, 5000);
    return () => clearInterval(interval);
  }, [refreshMcpServers]);

  useEffect(() => {
    refreshMcpBindings();
    const interval = setInterval(refreshMcpBindings, 5000);
    return () => clearInterval(interval);
  }, [refreshMcpBindings]);

  useEffect(() => {
    refreshSchedules();
    const interval = setInterval(refreshSchedules, 5000);
    return () => clearInterval(interval);
  }, [refreshSchedules]);

  useEffect(() => {
    refreshKnowledge();
    const interval = setInterval(refreshKnowledge, 5000);
    return () => clearInterval(interval);
  }, [refreshKnowledge]);

  useEffect(() => {
    refreshChannels();
    const interval = setInterval(refreshChannels, 5000);
    return () => clearInterval(interval);
  }, [refreshChannels]);

  useEffect(() => {
    if (selectedNode && selectedNode.type === 'namespace' && !newNamespace) {
      setNewNamespace(`${selectedNode.ns}:`);
    } else if (selectedNode && selectedNode.type === 'agent' && !newNamespace) {
      setNewNamespace(`${selectedNode.ns}:`);
    }
  }, [selectedNode, newNamespace]);

  const handleCreateNamespace = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newNamespace.trim()) return;
    setIsCreatingNamespace(true);
    try {
      await getGatewayClient().createNamespace({
        name: newNamespace.trim(),
        recursive: true
      });
      await refreshData();
      
      const newExpansions = new Set(expanded);
      newExpansions.add(newNamespace.trim());
      setExpanded(newExpansions);
      setNewNamespace('');
      setNamespaceModalOpen(false);
    } catch (e) {
      console.error(e);
      alert(e instanceof Error ? e.message : 'Error creating namespace');
    } finally {
      setIsCreatingNamespace(false);
    }
  };

  const handleCreateAgentSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setIsSubmittingAgent(true);
    try {
      await getGatewayClient().createAgent({
        ns: agentModalOpen.ns,
        name: agentForm.name,
        definition: {
          source: {
            case: 'templated',
            value: {
              templateName: agentForm.template,
              delta: {}
            }
          }
        }
      });
      setExpanded(prev => new Set(prev).add(agentModalOpen.ns));
      await refreshData();
      setAgentModalOpen({ isOpen: false, ns: '' });
      setAgentForm({ name: '', template: '' });
    } catch (e) {
      console.error(e);
      alert(e instanceof Error ? e.message : 'Error creating agent');
    } finally {
      setIsSubmittingAgent(false);
    }
  };

  const handleCreateChannelSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!channelForm.name.trim()) return;
    setIsSubmittingChannel(true);
    try {
      const baseUrl = normalizeGatewayUrl(gatewayUrl);
      const headers = {
        'Content-Type': 'application/json',
        ...(typeof window === 'undefined' ? {} : (buildGatewayHeaders(window.localStorage.getItem('talon_auth_token')) || {})),
      };
      const response = await fetch(`${baseUrl}/v1/ns/${encodeURIComponent(channelModalOpen.ns)}/channels`, {
        method: 'POST',
        headers,
        body: JSON.stringify({
          ns: channelModalOpen.ns,
          channel: {
            name: channelForm.name.trim(),
            title: channelForm.title.trim(),
            status: 'open',
          },
        }),
      });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      setExpanded(prev => new Set(prev).add(channelModalOpen.ns));
      await refreshChannels();
      await refreshData();
      setChannelModalOpen({ isOpen: false, ns: '' });
      setChannelForm({ name: '', title: '' });
    } catch (e) {
      console.error(e);
      alert(e instanceof Error ? e.message : 'Error creating channel');
    } finally {
      setIsSubmittingChannel(false);
    }
  };

  const handleCreateSubscriptionSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!subscriptionForm.name.trim() || !subscriptionForm.agent.trim()) return;
    setIsSubmittingSubscription(true);
    try {
      const baseUrl = normalizeGatewayUrl(gatewayUrl);
      const headers = {
        'Content-Type': 'application/json',
        ...(typeof window === 'undefined' ? {} : (buildGatewayHeaders(window.localStorage.getItem('talon_auth_token')) || {})),
      };
      const response = await fetch(
        `${baseUrl}/v1/ns/${encodeURIComponent(subscriptionModalOpen.ns)}/channels/${encodeURIComponent(subscriptionModalOpen.channel)}/subscriptions`,
        {
          method: 'POST',
          headers,
          body: JSON.stringify({
            ns: subscriptionModalOpen.ns,
            channel: subscriptionModalOpen.channel,
            subscription: {
              name: subscriptionForm.name.trim(),
              agent: subscriptionForm.agent.trim(),
              enabled: subscriptionForm.enabled,
              trigger: subscriptionForm.trigger,
            },
          }),
        },
      );
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      setExpanded(prev => new Set(prev).add(`${subscriptionModalOpen.ns}:channel:${subscriptionModalOpen.channel}`));
      await refreshChannels();
      await refreshData();
      setSubscriptionModalOpen({ isOpen: false, ns: '', channel: '' });
      setSubscriptionForm({ name: '', agent: '', trigger: 'mention', enabled: true });
    } catch (e) {
      console.error(e);
      alert(e instanceof Error ? e.message : 'Error creating channel subscription');
    } finally {
      setIsSubmittingSubscription(false);
    }
  };

  const handleDeleteConfirmed = async () => {
    if (!deleteConfirm.node) return;
    const { selection } = deleteConfirm.node;
    setIsDeleting(true);
    try {
      if (selection.type === 'namespace') {
        await getGatewayClient().deleteNamespace({ name: selection.ns });
      } else if (selection.type === 'agent') {
        alert("DeleteAgent is currently not supported by the GatewayService");
      } else if (selection.type === 'session') {
        await getGatewayClient().deleteSession({ ns: selection.ns, agent: selection.agent!, sessionId: selection.sessionId! });
      } else if (selection.type === 'channel') {
        const response = await fetch(
          `${normalizeGatewayUrl(gatewayUrl)}/v1/ns/${encodeURIComponent(selection.ns)}/channels/${encodeURIComponent(selection.resourceName || selection.channel || '')}`,
          {
            method: 'DELETE',
            headers: typeof window === 'undefined' ? undefined : buildGatewayHeaders(window.localStorage.getItem('talon_auth_token')),
          },
        );
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
      } else if (selection.type === 'channel-subscription') {
        const response = await fetch(
          `${normalizeGatewayUrl(gatewayUrl)}/v1/ns/${encodeURIComponent(selection.ns)}/channels/${encodeURIComponent(selection.channel || '')}/subscriptions/${encodeURIComponent(selection.resourceName || '')}`,
          {
            method: 'DELETE',
            headers: typeof window === 'undefined' ? undefined : buildGatewayHeaders(window.localStorage.getItem('talon_auth_token')),
          },
        );
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
      } else if (selection.type === 'schedule') {
        await getGatewayClient().deleteSchedule({ ns: selection.ns, name: selection.resourceName || '' });
      }
      await refreshChannels();
      await refreshData();
      setDeleteConfirm({ isOpen: false, node: null });
    } catch (e) {
      console.error(e);
      alert(e instanceof Error ? e.message : 'Error deleting item');
    } finally {
      setIsDeleting(false);
    }
  };

  const handleContextMenu = (e: React.MouseEvent, node: TreeNode) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY, node });
  };

  const toggleExpanded = (id: string) => {
    setExpanded(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const resizeLowerPanels = useCallback((delta: number) => {
    setSectionHeights((prev) => {
      const nextTemplates = Math.max(116, prev.templates + delta);
      return {
        ...prev,
        templates: nextTemplates,
      };
    });
  }, []);

  const resizeUpperPanels = useCallback((delta: number) => {
    setSectionHeights((prev) => {
      const nextExplorer = Math.max(180, prev.explorer + delta);
      const actualDelta = nextExplorer - prev.explorer;
      const nextTemplates = Math.max(116, prev.templates - actualDelta);
      const templatesAdjustedDelta = prev.templates - nextTemplates;
      return {
        ...prev,
        explorer: prev.explorer + templatesAdjustedDelta,
        templates: nextTemplates,
      };
    });
  }, []);

  const menuNode = contextMenu?.node;
  const templateCards = templates
    .map((template) => ({
      id: `template:${template.metadata?.name || 'unknown-template'}`,
      name: template.metadata?.name || 'Unnamed template',
      subtitle:
        template.definition?.source.case === 'customSpec'
          ? template.definition.source.value.systemPrompt.slice(0, 120) || 'Custom template'
          : template.definition?.source.case === 'templated'
            ? `Extends ${template.definition.source.value.templateName}`
            : 'No template summary',
      selection: {
        type: 'template' as const,
        ns: 'Sys',
        resourceName: template.metadata?.name || '',
        fullPath: `template/${template.metadata?.name || 'unknown-template'}`,
      },
    }))
    .sort((left, right) => left.name.localeCompare(right.name));
  const mcpCards = mcpServers
    .map((server) => ({
      id: `mcp:${server.metadata?.name || 'unknown-server'}`,
      name: server.metadata?.name || 'Unnamed MCP server',
      subtitle: server.spec?.target || 'No target configured',
      tag: server.spec?.disabled ? 'disabled' : (server.spec?.transport || 'unknown'),
      tone: server.spec?.disabled ? 'warning' as const : 'success' as const,
      selection: {
        type: 'mcp-server' as const,
        ns: 'Sys',
        resourceName: server.metadata?.name || '',
        fullPath: `mcp/${server.metadata?.name || 'unknown-server'}`,
      },
    }))
    .sort((left, right) => left.name.localeCompare(right.name));

  return (
    <div className="flex h-full flex-col overflow-hidden relative divide-y divide-border/70 bg-background/68 backdrop-blur-xl shadow-[inset_0_1px_0_rgba(255,255,255,0.03)]">
      <SectionShell
        icon={<Box className="w-3.5 h-3.5 text-muted-foreground stroke-[1.5]" />}
        title="Explorer"
        collapsed={collapsedSections.explorer}
        onToggle={() => setCollapsedSections(prev => ({ ...prev, explorer: !prev.explorer }))}
        height={sectionHeights.explorer}
      >
        <div onContextMenu={(e) => handleContextMenu(e, treeStructure)}>
          {Object.keys(treeStructure.children).length > 0 ? (
            <NamespaceNode 
              node={treeStructure} 
              level={-1} 
              selectedNodeId={selectedNode?.fullPath || null} 
              onSelect={onSelect} 
              onContextMenu={handleContextMenu}
              expanded={expanded} 
              toggleExpanded={toggleExpanded}
            />
          ) : (
            <div className="py-4 text-center text-[11px] text-muted-foreground">No namespaces discovered.</div>
          )}
        </div>
      </SectionShell>
      {!collapsedSections.explorer && !collapsedSections.templates && (
        <SplitHandle
          onMouseDown={(event) => {
            event.preventDefault();
            let lastY = event.clientY;

            const handleMouseMove = (moveEvent: MouseEvent) => {
              const delta = moveEvent.clientY - lastY;
              lastY = moveEvent.clientY;
              resizeUpperPanels(delta);
            };

            const handleMouseUp = () => {
              window.removeEventListener('mousemove', handleMouseMove);
              window.removeEventListener('mouseup', handleMouseUp);
            };

            window.addEventListener('mousemove', handleMouseMove);
            window.addEventListener('mouseup', handleMouseUp);
          }}
        />
      )}

      <SectionShell
        icon={<Cpu className="w-3.5 h-3.5 text-emerald-500 stroke-[1.5]" />}
        title="Agent Templates"
        collapsed={collapsedSections.templates}
        onToggle={() => setCollapsedSections(prev => ({ ...prev, templates: !prev.templates }))}
        height={sectionHeights.templates}
      >
        <ResourceList
          items={templateCards}
          emptyState="No agent templates found."
          selectedId={selectedNode?.fullPath || null}
          onSelect={onSelect}
        />
      </SectionShell>
      {!collapsedSections.templates && !collapsedSections.mcpServers && (
        <SplitHandle
          onMouseDown={(event) => {
            event.preventDefault();
            let lastY = event.clientY;

            const handleMouseMove = (moveEvent: MouseEvent) => {
              const delta = moveEvent.clientY - lastY;
              lastY = moveEvent.clientY;
              resizeLowerPanels(delta);
            };

            const handleMouseUp = () => {
              window.removeEventListener('mousemove', handleMouseMove);
              window.removeEventListener('mouseup', handleMouseUp);
            };

            window.addEventListener('mousemove', handleMouseMove);
            window.addEventListener('mouseup', handleMouseUp);
          }}
        />
      )}

      <SectionShell
        icon={<Plug className="w-3.5 h-3.5 text-blue-500 stroke-[1.5]" />}
        title="MCP Servers"
        collapsed={collapsedSections.mcpServers}
        onToggle={() => setCollapsedSections(prev => ({ ...prev, mcpServers: !prev.mcpServers }))}
        grow
      >
        <ResourceList
          items={mcpCards}
          emptyState="No MCP servers found in Sys."
          selectedId={selectedNode?.fullPath || null}
          onSelect={onSelect}
        />
      </SectionShell>

      {/* Context Menu Dropdown */}
      <Dropdown 
        isOpen={!!contextMenu} 
        onOpenChange={(open) => !open && setContextMenu(null)}
      >
        <DropdownTrigger 
          className="fixed" 
          style={{ top: contextMenu?.y || 0, left: contextMenu?.x || 0, width: 1, height: 1, padding: 0 }}
        >
          <span />
        </DropdownTrigger>
        <DropdownMenu aria-label="Context Actions" variant="outline">
          {menuNode?.selection.type === 'namespace' && (
             <DropdownItem 
               id="create_namespace" 
               onAction={() => {
                setContextMenu(null);
                setNewNamespace(menuNode.selection.ns ? `${menuNode.selection.ns}:` : '');
                setNamespaceModalOpen(true);
               }}
             >
               <div className="flex items-center gap-2">
                 <PlusCircle className="w-4 h-4 text-muted-foreground"/>
                 New Namespace
               </div>
             </DropdownItem>
          )}
          
          {menuNode?.selection.type === 'namespace' && (
             <DropdownItem 
               id="create_agent" 
               onAction={async () => {
                setContextMenu(null);
                try {
                  const res = await getGatewayClient().listAgentTemplates({});
                  setTemplates(res.templates || []);
                } catch (e) {
                  console.warn(e);
                }
                setAgentModalOpen({ isOpen: true, ns: menuNode.selection.ns });
               }}
             >
               <div className="flex items-center gap-2">
                 <Cpu className="w-4 h-4 text-muted-foreground"/>
                 Create Agent
               </div>
             </DropdownItem>
          )}

          {menuNode?.selection.type === 'namespace' && (
             <DropdownItem
               id="create_channel"
               onAction={() => {
                setContextMenu(null);
                setChannelModalOpen({ isOpen: true, ns: menuNode.selection.ns });
               }}
             >
               <div className="flex items-center gap-2">
                 <Hash className="w-4 h-4 text-muted-foreground"/>
                 Create Channel
               </div>
             </DropdownItem>
          )}
          
          {menuNode?.selection.type === 'agent' && (
             <DropdownItem 
               id="create_session" 
               onAction={async () => {
                setContextMenu(null);
                try {
                  await getGatewayClient().createSession({ ns: menuNode.selection.ns, agent: menuNode.selection.agent });
                  setExpanded(prev => new Set(prev).add(menuNode.id));
                  await refreshData();
                } catch (e) {
                  alert('Error creating session');
                }
               }}
             >
               <div className="flex items-center gap-2">
                 <PlusCircle className="w-4 h-4 text-muted-foreground"/>
                 Create Session
               </div>
             </DropdownItem>
          )}

          {menuNode?.selection.type === 'channel' && (
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
                 <Radio className="w-4 h-4 text-muted-foreground"/>
                 Create Subscription
               </div>
             </DropdownItem>
          )}

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
               <Trash2 className="w-4 h-4"/>
               Delete {menuNode?.selection.type}
             </div>
           </DropdownItem>
        </DropdownMenu>
      </Dropdown>

      {/* Namespace Creation Modal */}
      <DialogOverlay isOpen={namespaceModalOpen} onOpenChange={(open) => !open && setNamespaceModalOpen(false)}>
        <ModalContent className="sm:max-w-md">
          <ModalHeader className="flex flex-col gap-1">New Namespace</ModalHeader>
          <ModalBody>
            <form id="create-namespace-form" onSubmit={handleCreateNamespace} className="space-y-4">
              <div>
                <Label className="block text-sm font-medium mb-1">Namespace Path</Label>
                <Input 
                  className="w-full"
                  autoFocus
                  placeholder="org:team:child"
                  value={newNamespace}
                  onChange={(e) => setNewNamespace(e.target.value)}
                  disabled={isCreatingNamespace}
                  required
                />
              </div>
            </form>
          </ModalBody>
          <ModalFooter className="flex justify-end gap-3 mt-4">
            <Button variant="ghost" appearance="outline" onClick={() => setNamespaceModalOpen(false)}>
              Cancel
            </Button>
            <Button variant="primary" type="submit" form="create-namespace-form" disabled={isCreatingNamespace || !newNamespace.trim()}>
              Create Namespace
            </Button>
          </ModalFooter>
        </ModalContent>
      </DialogOverlay>

      {/* Create Agent Modal */}
      <Modal isOpen={agentModalOpen.isOpen} onOpenChange={(open) => !open && setAgentModalOpen({ isOpen: false, ns: '' })}>
        <ModalContent className="sm:max-w-md">
            <>
              <ModalHeader className="flex flex-col gap-1">Create Agent</ModalHeader>
              <ModalBody>
                <form id="create-agent-form" onSubmit={handleCreateAgentSubmit} className="space-y-4">
                  <div>
                    <Label className="block text-sm font-medium mb-1">Agent Name</Label>
                    <Input 
                      className="w-full"
                      placeholder="my-agent"
                      value={agentForm.name}
                      onChange={(e) => setAgentForm(prev => ({ ...prev, name: e.target.value }))}
                      required
                    />
                  </div>
                  
                  <div>
                    <Label className="block text-sm font-medium mb-1">Agent Template</Label>
                    <Select 
                      value={agentForm.template || null}
                      onChange={(key) => {
                         setAgentForm(prev => ({ ...prev, template: key as string }));
                      }}
                      isRequired
                      placeholder="Select an agent template"
                    >
                      <SelectTrigger className="w-full">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {templates.length > 0 ? templates.map(t => (
                          <SelectItem id={t.metadata?.name} key={t.metadata?.name}>{t.metadata?.name}</SelectItem>
                        )) : (
                          <SelectItem id="default" key="default">default</SelectItem>
                        )}
                      </SelectContent>
                    </Select>
                  </div>
                </form>
              </ModalBody>
              <ModalFooter>
                <Button variant="ghost" appearance="outline" onClick={() => setAgentModalOpen({ isOpen: false, ns: '' })}>
                  Cancel
                </Button>
                <Button color="primary" type="submit" form="create-agent-form" disabled={isSubmittingAgent || !agentForm.name || !agentForm.template}>
                  Create Agent
                </Button>
              </ModalFooter>
            </>
        </ModalContent>
      </Modal>

      <Modal isOpen={channelModalOpen.isOpen} onOpenChange={(open) => !open && setChannelModalOpen({ isOpen: false, ns: '' })}>
        <ModalContent className="sm:max-w-md">
          <ModalHeader className="flex flex-col gap-1">Create Channel</ModalHeader>
          <ModalBody>
            <form id="create-channel-form" onSubmit={handleCreateChannelSubmit} className="space-y-4">
              <div>
                <Label className="block text-sm font-medium mb-1">Channel Name</Label>
                <Input
                  className="w-full"
                  placeholder="incident-room"
                  value={channelForm.name}
                  onChange={(e) => setChannelForm(prev => ({ ...prev, name: e.target.value }))}
                  required
                />
              </div>
              <div>
                <Label className="block text-sm font-medium mb-1">Title</Label>
                <Input
                  className="w-full"
                  placeholder="Incident Room"
                  value={channelForm.title}
                  onChange={(e) => setChannelForm(prev => ({ ...prev, title: e.target.value }))}
                />
              </div>
            </form>
          </ModalBody>
          <ModalFooter>
            <Button variant="ghost" appearance="outline" onClick={() => setChannelModalOpen({ isOpen: false, ns: '' })}>
              Cancel
            </Button>
            <Button color="primary" type="submit" form="create-channel-form" disabled={isSubmittingChannel || !channelForm.name.trim()}>
              Create Channel
            </Button>
          </ModalFooter>
        </ModalContent>
      </Modal>

      <Modal isOpen={subscriptionModalOpen.isOpen} onOpenChange={(open) => !open && setSubscriptionModalOpen({ isOpen: false, ns: '', channel: '' })}>
        <ModalContent className="sm:max-w-md">
          <ModalHeader className="flex flex-col gap-1">Create Channel Subscription</ModalHeader>
          <ModalBody>
            <form id="create-channel-subscription-form" onSubmit={handleCreateSubscriptionSubmit} className="space-y-4">
              <div>
                <Label className="block text-sm font-medium mb-1">Subscription Name</Label>
                <Input
                  className="w-full"
                  placeholder="triage"
                  value={subscriptionForm.name}
                  onChange={(e) => setSubscriptionForm(prev => ({ ...prev, name: e.target.value }))}
                  required
                />
              </div>
              <div>
                <Label className="block text-sm font-medium mb-1">Agent</Label>
                <Input
                  className="w-full"
                  placeholder="triage-agent"
                  value={subscriptionForm.agent}
                  onChange={(e) => setSubscriptionForm(prev => ({ ...prev, agent: e.target.value }))}
                  required
                />
              </div>
              <div>
                <Label className="block text-sm font-medium mb-1">Trigger</Label>
                <Select
                  value={subscriptionForm.trigger}
                  onChange={(key) => setSubscriptionForm(prev => ({ ...prev, trigger: key as string }))}
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
              <label className="flex items-center gap-2 text-sm text-foreground">
                <input
                  type="checkbox"
                  checked={subscriptionForm.enabled}
                  onChange={(e) => setSubscriptionForm(prev => ({ ...prev, enabled: e.target.checked }))}
                />
                Enabled
              </label>
            </form>
          </ModalBody>
          <ModalFooter>
            <Button variant="ghost" appearance="outline" onClick={() => setSubscriptionModalOpen({ isOpen: false, ns: '', channel: '' })}>
              Cancel
            </Button>
            <Button color="primary" type="submit" form="create-channel-subscription-form" disabled={isSubmittingSubscription || !subscriptionForm.name.trim() || !subscriptionForm.agent.trim()}>
              Create Subscription
            </Button>
          </ModalFooter>
        </ModalContent>
      </Modal>

      {/* TailGrids Modal Update */}
      <DialogOverlay isOpen={deleteConfirm.isOpen} onOpenChange={(open) => !open && setDeleteConfirm({ isOpen: false, node: null })}>
        <ModalContent className="sm:max-w-md">
          <ModalHeader className="flex flex-col gap-1 text-red-500">Confirm Deletion</ModalHeader>
          <ModalBody>
            <div className="text-[13px] text-muted-foreground whitespace-pre-wrap">
              Are you sure you want to delete the {deleteConfirm.node?.selection.type} <b>{deleteConfirm.node?.name}</b>?
              {deleteConfirm.node?.selection.type === 'namespace' && "\n\nThis will permanently delete all enclosed agents and sessions natively."}
              {deleteConfirm.node?.selection.type === 'agent' && "\n\nThis action will also sever and erase all associated execution history contexts and sessions."}
            </div>
          </ModalBody>
          <ModalFooter className="flex justify-end gap-3 mt-4">
            <Button variant="ghost" appearance="outline" onClick={() => setDeleteConfirm({ isOpen: false, node: null })}>
              Cancel
            </Button>
            <Button variant="danger" onClick={handleDeleteConfirmed} disabled={isDeleting}>
              {isDeleting ? "Deleting..." : "Permanently Delete"}
            </Button>
          </ModalFooter>
        </ModalContent>
      </DialogOverlay>
    </div>
  );
}
