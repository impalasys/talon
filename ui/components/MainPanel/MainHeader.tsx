import { ListTree, MoreVertical } from 'lucide-react';
import { Plugs } from '@phosphor-icons/react';
import { RESOURCE_KIND_BY_SELECTION, type Selection } from '../../lib/selection';
import { WorkspaceCommandPalette } from '../Search/WorkspaceCommandPalette';

type MainHeaderProps = {
  isConnected: boolean;
  selectedNode: Selection | null;
  isHoveringConnection: boolean;
  onConnectionHoverChange: (isHovering: boolean) => void;
  onDisconnect: () => void;
  onSelect: (selection: Selection) => void;
  onOpenSidebar: () => void;
};

function resourceName(selection: Selection) {
  return selection.resourceName || selection.sessionId || selection.agent || selection.channel || '';
}

function breadcrumbParts(selection: Selection | null) {
  if (!selection) return ['Sightline'];
  const parts = ['Namespace', selection.ns || 'root'];
  if (selection.type === 'namespace') return parts;

  if (selection.type === 'session') {
    if (selection.agent) parts.push('Agent', selection.agent);
    parts.push('Session', selection.sessionId || 'current');
    return parts;
  }

  if (selection.type === 'channel-subscription') {
    if (selection.channel) parts.push('Channel', selection.channel);
    parts.push('ChannelSubscription', resourceName(selection));
    return parts;
  }

  const kind = RESOURCE_KIND_BY_SELECTION[selection.type] || selection.type;
  parts.push(kind, resourceName(selection) || selection.fullPath);
  return parts;
}

export function MainHeader({
  isConnected,
  selectedNode,
  isHoveringConnection,
  onConnectionHoverChange,
  onDisconnect,
  onSelect,
  onOpenSidebar,
}: MainHeaderProps) {
  const parts = breadcrumbParts(selectedNode);

  return (
    <header className="z-10 flex min-h-14 w-full flex-shrink-0 items-center justify-between border-b border-border/70 bg-background/72 px-4 pb-0 backdrop-blur-xl pt-[env(safe-area-inset-top)] lg:px-6">
      <div className="flex min-w-0 flex-1 items-center gap-3">
        <button
          type="button"
          className="flex h-9 w-9 flex-none items-center justify-center rounded-lg border border-border/70 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground md:hidden"
          onClick={onOpenSidebar}
          aria-label="Open explorer"
        >
          <ListTree className="h-4 w-4" />
        </button>
        <nav
          className="custom-scrollbar flex min-w-0 flex-1 items-center gap-1.5 overflow-x-auto whitespace-nowrap pb-0.5 text-sm"
          aria-label="Selected resource"
        >
          {parts.map((part, index) => (
            <span key={`${part}-${index}`} className="contents">
              {index > 0 ? <span className="text-muted-foreground/60">/</span> : null}
              <span
                className={
                  index === parts.length - 1
                    ? 'flex-none font-semibold text-foreground'
                    : 'flex-none text-muted-foreground'
                }
              >
                {part}
              </span>
            </span>
          ))}
        </nav>
      </div>

      <div className="ml-3 hidden flex-none items-center gap-4 sm:flex">
        <WorkspaceCommandPalette isConnected={isConnected} selectedNamespace={selectedNode} onSelect={onSelect} />
        {isConnected ? (
          <button
            type="button"
            className="flex h-9 w-9 items-center justify-center rounded-xl border border-emerald-500/16 bg-emerald-500/9 text-emerald-600 transition-all hover:border-red-500/16 hover:bg-red-500/10 hover:text-red-600 dark:text-emerald-300 dark:hover:text-red-300"
            onClick={onDisconnect}
            onMouseEnter={() => onConnectionHoverChange(true)}
            onMouseLeave={() => onConnectionHoverChange(false)}
            aria-label={isHoveringConnection ? 'Disconnect' : 'Connected'}
            title={isHoveringConnection ? 'Disconnect' : 'Connected'}
          >
            <Plugs weight="fill" className="h-4 w-4" />
          </button>
        ) : (
          <div
            className="flex h-9 w-9 items-center justify-center rounded-xl border border-border/70 bg-white/[0.045] text-muted-foreground"
            aria-label="Disconnected"
            title="Disconnected"
          >
            <Plugs weight="fill" className="h-4 w-4" />
          </div>
        )}
      </div>

      <div className="ml-2 flex flex-none items-center sm:hidden">
        <details className="group/menu relative">
          <summary
            className="flex h-9 w-9 list-none items-center justify-center rounded-xl border border-border/70 bg-background/80 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground [&::-webkit-details-marker]:hidden"
            aria-label="Open header menu"
          >
            <MoreVertical className="h-4 w-4" />
          </summary>
          <div className="absolute right-0 top-11 z-[90] hidden min-w-52 rounded-xl border border-border/70 bg-background p-1 shadow-xl group-open/menu:block">
            <div className="p-0.5">
              <WorkspaceCommandPalette
                isConnected={isConnected}
                selectedNamespace={selectedNode}
                onSelect={onSelect}
                triggerVariant="menu-item"
              />
              {isConnected ? (
                <button
                  type="button"
                  className="mt-1 flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left text-sm font-medium text-emerald-600 transition-colors hover:bg-red-500/10 hover:text-red-600 dark:text-emerald-300 dark:hover:text-red-300"
                  onClick={onDisconnect}
                >
                  <Plugs weight="fill" className="h-4 w-4" />
                  <span>Disconnect</span>
                </button>
              ) : (
                <div className="mt-1 flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-sm font-medium text-muted-foreground">
                  <Plugs weight="fill" className="h-4 w-4" />
                  <span>Disconnected</span>
                </div>
              )}
            </div>
          </div>
        </details>
      </div>
    </header>
  );
}
