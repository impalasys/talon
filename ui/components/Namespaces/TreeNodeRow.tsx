import { Activity, Box, ChevronDown, ChevronRight, Clock3, Container, Cpu, FileText, Folder, Hash, Layers3, MessageSquare, Package, Plug, Radio, ShieldCheck } from 'lucide-react';
import { AnimatePresence, motion } from 'framer-motion';
import type { SelectionType, Selection } from '../../lib/selection';
import type { TreeNode } from '../../hooks/useExplorerTree';
import { compareTreeNodes } from '../../hooks/useExplorerTree';
import { cn } from '../../utils/cn';

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

function NodeIcon({ type, selected }: { type: SelectionType; selected: boolean }) {
  const className = (fallback: string) => cn('h-3.5 w-3.5', selected ? 'text-foreground' : fallback);
  if (type === 'namespace') return <Folder className={className('text-muted-foreground')} />;
  if (type === 'agent') return <Cpu className={className('text-emerald-500')} />;
  if (type === 'session') return <MessageSquare className={className('text-blue-500')} />;
  if (type === 'channel') return <Hash className={className('text-cyan-400')} />;
  if (type === 'channel-subscription') return <Radio className={className('text-cyan-300')} />;
  if (type === 'workflow') return <Activity className={className('text-purple-400')} />;
  if (type === 'schedule') return <Clock3 className={className('text-amber-500')} />;
  if (type === 'template') return <FileText className={className('text-emerald-400')} />;
  if (type === 'deployment') return <Layers3 className={className('text-indigo-400')} />;
  if (type === 'deployment-replica') return <Package className={className('text-indigo-300')} />;
  if (type === 'sandbox-class') return <ShieldCheck className={className('text-fuchsia-400')} />;
  if (type === 'sandbox-policy') return <Box className={className('text-fuchsia-300')} />;
  if (type === 'sandbox') return <Container className={className('text-orange-400')} />;
  if (type === 'mcp-server') return <Plug className={className('text-blue-500')} />;
  if (type === 'knowledge') return <FileText className={className('text-violet-400')} />;
  return <Box className={className('text-muted-foreground')} />;
}

export function TreeNodeRow({
  node,
  level,
  selectedNodeId,
  onSelect,
  onContextMenu,
  expanded,
  toggleExpanded,
}: {
  node: TreeNode;
  level: number;
  selectedNodeId: string | null;
  onSelect: (selection: Selection) => void;
  onContextMenu: (e: React.MouseEvent, node: TreeNode) => void;
  expanded: Set<string>;
  toggleExpanded: (id: string) => void;
}) {
  const childNodes = Object.values(node.children).sort(compareTreeNodes);
  const hasChildrenOrCanHaveChildren = !LEAF_TYPES.includes(node.selection.type);
  const isExpanded = expanded.has(node.id);
  const isSelected = selectedNodeId === node.id;

  const handleToggle = (event: React.MouseEvent) => {
    event.stopPropagation();
    if (hasChildrenOrCanHaveChildren) toggleExpanded(node.id);
  };

  const handleClick = (event: React.MouseEvent) => {
    event.stopPropagation();
    onSelect(node.selection);
  };

  const handleContextMenu = (event: React.MouseEvent) => {
    event.preventDefault();
    event.stopPropagation();
    onContextMenu(event, node);
  };

  return (
    <div>
      <div
        className={cn(
          'group flex select-none items-center gap-1.5 rounded-xl px-2.5 py-2 text-[13px] font-medium transition-colors hover:bg-white/[0.04]',
          isSelected
            ? 'border border-border/70 bg-white/[0.055] text-foreground shadow-[inset_0_1px_0_rgba(255,255,255,0.03)]'
            : 'text-muted-foreground',
        )}
        style={{ paddingLeft: `${level * 16 + 8}px` }}
        onClick={handleClick}
        onContextMenu={handleContextMenu}
      >
        <div
          className={cn(
            'flex items-center justify-center rounded-sm p-0.5 transition-colors',
            hasChildrenOrCanHaveChildren && 'hover:bg-white/[0.05]',
            !hasChildrenOrCanHaveChildren && 'cursor-default opacity-0',
          )}
          onClick={hasChildrenOrCanHaveChildren ? handleToggle : undefined}
        >
          {isExpanded ? <ChevronDown className="h-3.5 w-3.5" /> : <ChevronRight className="h-3.5 w-3.5" />}
        </div>

        <NodeIcon type={node.selection.type} selected={isSelected} />

        <span className="min-w-0 flex-1 truncate">{node.name}</span>
        {node.badge ? (
          <span className="max-w-[8rem] truncate rounded-full border border-border/70 bg-white/[0.03] px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
            {node.badge}
          </span>
        ) : null}
        {isSelected ? <Activity className="h-3.5 w-3.5 flex-shrink-0 opacity-70" /> : null}
      </div>

      <AnimatePresence initial={false}>
        {isExpanded && hasChildrenOrCanHaveChildren ? (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.15, ease: 'easeInOut' }}
            className="overflow-hidden"
          >
            <div className="mt-0.5 space-y-0.5">
              {childNodes.length > 0 ? (
                childNodes.map((child) => (
                  <TreeNodeRow
                    key={child.id}
                    node={child}
                    level={level + 1}
                    selectedNodeId={selectedNodeId}
                    onSelect={onSelect}
                    onContextMenu={onContextMenu}
                    expanded={expanded}
                    toggleExpanded={toggleExpanded}
                  />
                ))
              ) : (
                <div
                  className="flex items-center gap-1.5 px-2 py-1.5 text-[12px] italic text-muted-foreground/60"
                  style={{ paddingLeft: `${(level + 1) * 16 + 28}px` }}
                >
                  Empty
                </div>
              )}
            </div>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  );
}
