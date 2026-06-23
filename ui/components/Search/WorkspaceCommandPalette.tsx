'use client';

import { useCallback, useEffect, useState } from 'react';
import { Search } from 'lucide-react';
import { v1Search } from '@impalasys/talon-client';
import { getGatewayClient } from '../../lib/grpc';
import type { Selection } from '../Namespaces/NamespaceExplorer';
import {
  Dialog,
  DialogContent,
  DialogOverlay,
  DialogTrigger,
} from '../tailgrids/core/dialog';
import { Input } from '../tailgrids/core/input';
import {
  Select,
  SelectContent,
  SelectIndicator,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../tailgrids/core/select';
type SearchDocument = v1Search.Document;
type SearchResult = v1Search.SearchResult;

const RESOURCE_KIND_OPTIONS = [
  { label: 'All', value: '' },
  { label: 'Sessions', value: 'SessionMessage' },
  { label: 'Knowledge', value: 'Knowledge' },
  { label: 'Agents', value: 'Agent' },
  { label: 'Workflows', value: 'Workflow' },
  { label: 'Schedules', value: 'Schedule' },
  { label: 'Channels', value: 'Channel' },
  { label: 'Deployments', value: 'Deployment' },
  { label: 'Sandboxes', value: 'Sandbox' },
];

const SELECTION_TYPE_BY_RESOURCE_KIND: Partial<Record<string, Selection['type']>> = {
  Agent: 'agent',
  Channel: 'channel',
  ChannelSubscription: 'channel-subscription',
  Schedule: 'schedule',
  Template: 'template',
  Deployment: 'deployment',
  DeploymentReplica: 'deployment-replica',
  SandboxClass: 'sandbox-class',
  SandboxPolicy: 'sandbox-policy',
  Sandbox: 'sandbox',
  McpServer: 'mcp-server',
  McpServerBinding: 'mcp-binding',
  Knowledge: 'knowledge',
  Workflow: 'workflow',
};

type WorkspaceCommandPaletteProps = {
  isConnected: boolean;
  selectedNamespace: Selection | null;
  onSelect: (selection: Selection) => void;
};

function parseMetadataJson(value?: string) {
  if (!value) return {};
  try {
    const parsed = JSON.parse(value);
    return parsed && typeof parsed === 'object' ? (parsed as Record<string, any>) : {};
  } catch {
    return {};
  }
}

function nameFromDocument(document: SearchDocument) {
  const metadata = parseMetadataJson(document.metadataJson);
  return (
    metadata.name ||
    document.title?.split('/').at(-1) ||
    document.source?.key?.split('/').at(-1) ||
    ''
  );
}

function sourceNamespace(document: SearchDocument, selectedNamespace: Selection | null) {
  return document.source?.namespace || selectedNamespace?.ns || 'default';
}

function sourceKind(document: SearchDocument) {
  return document.source?.kind || '';
}

function attr(document: SearchDocument, key: string) {
  return document.attributes?.[key] || '';
}

function selectionForDocument(
  document: SearchDocument,
  selectedNamespace: Selection | null,
): Selection | null {
  const namespace = sourceNamespace(document, selectedNamespace);
  const kind = sourceKind(document);
  const agent = attr(document, 'agent');
  const sessionId = attr(document, 'session_id');

  if (kind === 'SessionMessage' && agent && sessionId) {
    return {
      type: 'session',
      ns: namespace,
      agent,
      sessionId,
      fullPath: `${namespace}/${agent}/${sessionId}`,
    };
  }

  const resourceType = SELECTION_TYPE_BY_RESOURCE_KIND[kind];
  if (!resourceType) return null;

  const resourceName = nameFromDocument(document);
  if (!resourceName) return null;

  if (resourceType === 'agent') {
    return {
      type: 'agent',
      ns: namespace,
      agent: resourceName,
      fullPath: `${namespace}/${resourceName}`,
    };
  }

  if (resourceType === 'channel') {
    return {
      type: 'channel',
      ns: namespace,
      channel: resourceName,
      resourceName,
      fullPath: `${namespace}:channel:${resourceName}`,
    };
  }

  if (resourceType === 'channel-subscription') {
    const channel = attr(document, 'channel') || parseMetadataJson(document.metadataJson).channel || '';
    return {
      type: 'channel-subscription',
      ns: namespace,
      channel,
      resourceName,
      fullPath: `${namespace}:channel:${channel}:subscription:${resourceName}`,
    };
  }

  return {
    type: resourceType,
    ns: namespace,
    resourceName,
    fullPath: `${namespace}:${resourceType}:${resourceName}`,
  };
}

export function WorkspaceCommandPalette({
  isConnected,
  selectedNamespace,
  onSelect,
}: WorkspaceCommandPaletteProps) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [resourceKind, setResourceKind] = useState('');
  const [results, setResults] = useState<SearchResult[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const openDocument = useCallback(
    (document: SearchDocument) => {
      const selection = selectionForDocument(document, selectedNamespace);
      if (selection) {
        onSelect(selection);
        setOpen(false);
      }
    },
    [onSelect, selectedNamespace],
  );

  useEffect(() => {
    if (!open) return;

    const ns = selectedNamespace?.ns || 'default';
    const trimmedQuery = query.trim();
    if (!isConnected || !trimmedQuery) {
      setResults([]);
      setLoading(false);
      return;
    }

    let active = true;
    const handle = window.setTimeout(async () => {
      setLoading(true);
      setError(null);
      try {
        const response = await getGatewayClient().searches.search({
          query: trimmedQuery,
          source: {
            namespace: ns,
            kinds: resourceKind ? [resourceKind] : [],
          },
          limit: 12,
          mode: v1Search.SearchMode.KEYWORD,
          sort: v1Search.SearchSort.RELEVANCE,
        });
        if (active) {
          setResults(response.results);
        }
      } catch (err: any) {
        if (active) {
          setError(err?.message || 'Search failed');
          setResults([]);
        }
      } finally {
        if (active) {
          setLoading(false);
        }
      }
    }, 180);
    return () => {
      active = false;
      window.clearTimeout(handle);
    };
  }, [open, isConnected, query, resourceKind, selectedNamespace?.ns]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        if (isConnected) {
          setOpen(true);
        }
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isConnected]);

  return (
    <>
      <Dialog isOpen={open} onOpenChange={setOpen}>
        <DialogTrigger
          isDisabled={!isConnected}
          className="flex h-8 min-w-40 items-center justify-between gap-3 rounded-md border border-border/70 bg-white/[0.045] px-2.5 text-xs font-medium text-muted-foreground transition-colors hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40"
          aria-label="Search workspace"
        >
          <span className="flex min-w-0 items-center gap-2">
            <Search className="h-4 w-4" />
            <span className="truncate">Search workspace</span>
          </span>
          <kbd className="hidden rounded border border-border/70 px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground lg:inline">
            Cmd K
          </kbd>
        </DialogTrigger>

        <DialogOverlay
          className="z-[80] flex items-start justify-center px-4 pt-[12vh]"
          isDismissable
        >
          <DialogContent
            aria-label="Search workspace"
            showCloseButton={false}
            className="top-[12vh] w-full max-w-3xl translate-y-0 overflow-hidden rounded-lg border border-border bg-background p-0 shadow-2xl"
          >
            <div className="flex items-center gap-3 border-b border-border/70 px-4 py-3">
              <Search className="h-4 w-4 shrink-0 text-muted-foreground" />
              <Input
                autoFocus
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Escape') setOpen(false);
                }}
                placeholder="Search workspace"
                className="h-10 flex-1 border-0 bg-transparent px-0 py-0 text-base text-foreground shadow-none ring-0 placeholder:text-muted-foreground focus:border-0 focus:ring-0"
              />
              <Select
                aria-label="Filter search results"
                value={resourceKind || 'all'}
                onChange={(value) => setResourceKind(String(value) === 'all' ? '' : String(value))}
                className="w-44 shrink-0"
              >
                <SelectTrigger className="h-10 rounded-md border-border bg-background px-3 py-0 text-sm">
                  <SelectValue />
                  <SelectIndicator />
                </SelectTrigger>
                <SelectContent>
                  {RESOURCE_KIND_OPTIONS.map((option) => (
                    <SelectItem key={option.value || 'all'} id={option.value || 'all'}>
                      {option.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="max-h-[52vh] min-h-64 overflow-y-auto">
              {loading && (
                <div className="px-4 py-8 text-center text-sm text-muted-foreground">Searching...</div>
              )}
              {error && <div className="px-4 py-3 text-sm text-red-300">{error}</div>}
              {!loading && !error && results.length === 0 && query.trim() && (
                <div className="px-4 py-8 text-center text-sm text-muted-foreground">No results</div>
              )}
              {!query.trim() && (
                <div className="px-4 py-16 text-center text-sm text-muted-foreground">
                  Type to search indexed workspace documents.
                </div>
              )}
              {results.map((result) => {
                const document = result.document;
                if (!document) return null;
                const kind = sourceKind(document);
                const namespace = sourceNamespace(document, selectedNamespace);
                const agent = attr(document, 'agent');
                const sessionId = attr(document, 'session_id');
                const channel = attr(document, 'channel');
                return (
                  <button
                    key={document.id || `${document.source?.key}-${document.subdocumentId}`}
                    type="button"
                    onClick={() => openDocument(document)}
                    className="block w-full border-b border-border/50 px-4 py-3 text-left transition-colors hover:bg-white/[0.045]"
                  >
                    <div className="flex items-center justify-between gap-3">
                      <div className="min-w-0 truncate text-sm font-medium">
                        {document.title || kind || 'Result'}
                      </div>
                      <div className="shrink-0 text-[11px] text-muted-foreground">
                        {kind}
                        {document.documentKind ? ` / ${document.documentKind}` : ''}
                      </div>
                    </div>
                    <div className="mt-1 line-clamp-2 text-xs leading-5 text-muted-foreground">
                      {document.snippet}
                    </div>
                    <div className="mt-2 truncate text-[11px] text-muted-foreground">
                      {namespace}
                      {agent ? ` / ${agent}` : ''}
                      {sessionId ? ` / ${sessionId}` : ''}
                      {channel ? ` / ${channel}` : ''}
                    </div>
                  </button>
                );
              })}
            </div>
          </DialogContent>
        </DialogOverlay>
      </Dialog>
    </>
  );
}
