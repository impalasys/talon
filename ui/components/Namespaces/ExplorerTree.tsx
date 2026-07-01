import type { Selection } from '../../lib/selection';
import type { TreeNode } from '../../hooks/useExplorerTree';
import { compareTreeNodes } from '../../hooks/useExplorerTree';
import { TreeNodeRow } from './TreeNodeRow';

export function ExplorerTree({
  tree,
  selectedNode,
  onSelect,
  onContextMenu,
  expanded,
  toggleExpanded,
}: {
  tree: TreeNode;
  selectedNode: Selection | null;
  onSelect: (selection: Selection) => void;
  onContextMenu: (event: React.MouseEvent, node: TreeNode) => void;
  expanded: Set<string>;
  toggleExpanded: (id: string) => void;
}) {
  const childNodes = Object.values(tree.children).sort(compareTreeNodes);
  if (childNodes.length === 0) {
    return <div className="py-4 text-center text-[11px] text-muted-foreground">No namespaces discovered.</div>;
  }

  return (
    <div className="relative space-y-0.5">
      {childNodes.map((child) => (
        <TreeNodeRow
          key={child.id}
          node={child}
          level={0}
          selectedNodeId={selectedNode?.fullPath || null}
          onSelect={onSelect}
          onContextMenu={onContextMenu}
          expanded={expanded}
          toggleExpanded={toggleExpanded}
        />
      ))}
    </div>
  );
}
