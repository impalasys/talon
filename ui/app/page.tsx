'use client';

import { Suspense, useState, useRef, useEffect, useCallback } from 'react';
import { usePathname, useRouter, useSearchParams } from 'next/navigation';
import { dump } from 'js-yaml';
import { 
  Terminal, 
  Activity, 
  Box,
  Database, 
  Settings2, 
  Wifi, 
  WifiOff,
  User,
  Cpu,
  MessageSquare,
  Search,
  FileText,
  ChevronRight,
  ChevronsLeft,
  ChevronsRight,
  Folder,
  Plug,
  Clock3,
  Square,
  Hash,
  Radio,
  Layers3,
  Package,
  ShieldCheck,
  Container,
} from 'lucide-react';
import { motion, AnimatePresence } from 'framer-motion';
import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';
import { TalonChannel, TalonCopilot, type TalonChatObjectRef, type TalonImageUploadContext } from '@impalasys/talon-chat';
import { NamespaceExplorer, type Selection } from '../components/Namespaces/NamespaceExplorer';
import { updateGatewayClient, getGatewayClient } from '../lib/grpc';
import { resourceToManifestDocument, yamlSafeValue } from '../lib/resourceManifest';

const isStaticExport = process.env.NEXT_PUBLIC_TALON_STATIC_EXPORT === '1';
const SYSTEM_NAMESPACE = 'Sys';

declare global {
  interface Window {
    google?: {
      accounts?: {
        id?: {
          initialize(options: { client_id: string; callback: (response: { credential?: string }) => void }): void;
          prompt(): void;
        };
      };
    };
  }
}

const RESOURCE_KIND_BY_SELECTION: Partial<Record<Selection['type'], string>> = {
  agent: 'Agent',
  channel: 'Channel',
  'channel-subscription': 'ChannelSubscription',
  schedule: 'Schedule',
  template: 'Template',
  deployment: 'Deployment',
  'deployment-replica': 'DeploymentReplica',
  'sandbox-class': 'SandboxClass',
  'sandbox-policy': 'SandboxPolicy',
  sandbox: 'Sandbox',
  'mcp-server': 'McpServer',
  'mcp-binding': 'McpServerBinding',
  knowledge: 'Knowledge',
};

function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

type ResourceEnvelope = {
  apiVersion?: string;
  kind?: string;
  metadata?: {
    name?: string;
    namespace?: string;
    labels?: Record<string, string>;
  };
  spec?: {
    kind?: {
      case?: string;
      value?: any;
    };
  };
  status?: {
    kind?: {
      case?: string;
      value?: any;
    };
  };
};

function resourceSpec(resource: ResourceEnvelope, caseName: string) {
  return resource.spec?.kind?.case === caseName ? resource.spec.kind.value || {} : {};
}

function resourceStatus(resource: ResourceEnvelope, caseName: string) {
  return resource.status?.kind?.case === caseName ? resource.status.kind.value || {} : {};
}

function isV2ResourceDocument(document: any): document is ResourceEnvelope {
  return Boolean(
    document &&
      typeof document === 'object' &&
      typeof document.apiVersion === 'string' &&
      typeof document.kind === 'string' &&
      document.metadata &&
      document.spec?.kind?.case,
  );
}

function channelDocumentFromResource(resource: ResourceEnvelope): ChannelDocument {
  const spec = resourceSpec(resource, 'channel');
  const status = resourceStatus(resource, 'channel');
  return {
    name: resource.metadata?.name,
    ns: resource.metadata?.namespace,
    title: spec.title,
    status: status.phase,
    metadata: spec.metadata,
    labels: resource.metadata?.labels,
  };
}

function channelSubscriptionDocumentFromResource(resource: ResourceEnvelope): ChannelSubscriptionDocument {
  const spec = resourceSpec(resource, 'channelSubscription');
  return {
    name: resource.metadata?.name,
    ns: resource.metadata?.namespace,
    channel: spec.channel,
    agent: spec.agent,
    enabled: spec.enabled,
    trigger: spec.trigger,
    replyMode: spec.replyMode,
    contextPolicy: spec.contextPolicy,
  };
}

function scheduleDocumentFromResource(resource: ResourceEnvelope): ScheduleDocument {
  return {
    name: resource.metadata?.name,
    ns: resource.metadata?.namespace,
    labels: resource.metadata?.labels,
    spec: resourceSpec(resource, 'schedule'),
    status: resourceStatus(resource, 'schedule'),
  };
}

function areSelectionsEqual(left: Selection | null, right: Selection | null) {
  if (left === right) return true;
  if (!left || !right) return false;
  return (
    left.type === right.type &&
    left.ns === right.ns &&
    left.agent === right.agent &&
    left.channel === right.channel &&
    left.sessionId === right.sessionId &&
    left.resourceName === right.resourceName
  );
}

function selectionFromSearchParams(searchParams: URLSearchParams): Selection | null {
  const type = searchParams.get('type');
  const ns = searchParams.get('ns');
  const agent = searchParams.get('agent');
  const channel = searchParams.get('channel');
  const sessionId = searchParams.get('session');
  const resourceName = searchParams.get('name');

  if (type === 'template' && resourceName) {
    return {
      type: 'template',
      ns: ns || SYSTEM_NAMESPACE,
      resourceName,
      fullPath: `${ns || SYSTEM_NAMESPACE}:template:${resourceName}`,
    };
  }

  if (type === 'mcp-server' && resourceName) {
    return {
      type: 'mcp-server',
      ns: 'Sys',
      resourceName,
      fullPath: `mcp/${resourceName}`,
    };
  }

  if (!ns) return null;

  if (sessionId && agent) {
    return {
      type: 'session',
      ns,
      agent,
      sessionId,
      fullPath: `${ns}/${agent}/${sessionId}`,
    };
  }

  if (agent) {
    return {
      type: 'agent',
      ns,
      agent,
      fullPath: `${ns}/${agent}`,
    };
  }

  if (type === 'channel-subscription' && channel && resourceName) {
    return {
      type: 'channel-subscription',
      ns,
      channel,
      resourceName,
      fullPath: `${ns}:channel:${channel}:subscription:${resourceName}`,
    };
  }

  if (type === 'channel' && (resourceName || channel)) {
    const channelName = resourceName || channel || '';
    return {
      type: 'channel',
      ns,
      channel: channelName,
      resourceName: channelName,
      fullPath: `${ns}:channel:${channelName}`,
    };
  }

  if (type === 'schedule' && resourceName) {
    return {
      type: 'schedule',
      ns,
      resourceName,
      fullPath: `${ns}:schedule:${resourceName}`,
    };
  }

  if (type === 'mcp-binding' && resourceName) {
    return {
      type: 'mcp-binding',
      ns,
      resourceName,
      fullPath: `${ns}:mcp-binding:${resourceName}`,
    };
  }

  if (type === 'knowledge' && resourceName) {
    return {
      type: 'knowledge',
      ns,
      resourceName,
      fullPath: `${ns}:knowledge:${resourceName}`,
    };
  }

  if (
    (
      type === 'deployment' ||
      type === 'deployment-replica' ||
      type === 'sandbox-class' ||
      type === 'sandbox-policy' ||
      type === 'sandbox'
    ) &&
    ns &&
    resourceName
  ) {
    return {
      type,
      ns,
      resourceName,
      fullPath: `${ns}:${type}:${resourceName}`,
    };
  }

  return {
    type: 'namespace',
    ns,
    fullPath: ns,
  };
}

function buildSearchParams(isConnected: boolean, selection: Selection | null, currentSearchParams?: URLSearchParams) {
  const params = new URLSearchParams();
  const historyPageSize = currentSearchParams?.get('historyPageSize');

  if (isConnected) {
    params.set('connected', 'true');
  }

  if (historyPageSize && /^\d+$/.test(historyPageSize) && Number(historyPageSize) > 0) {
    params.set('historyPageSize', historyPageSize);
  }

  if (selection?.ns) {
    params.set('ns', selection.ns);
  }

  if (selection?.type) {
    params.set('type', selection.type);
  }

  if (selection?.agent) {
    params.set('agent', selection.agent);
  }

  if (selection?.channel) {
    params.set('channel', selection.channel);
  }

  if (selection?.sessionId) {
    params.set('session', selection.sessionId);
  }

  if (selection?.resourceName) {
    params.set('name', selection.resourceName);
  }

  return params;
}

function getSelectionTitle(selection: Selection | null) {
  if (!selection) return 'No Resource Selected';
  if (selection.type === 'namespace') return selection.ns;
  if (selection.type === 'agent') return selection.agent || 'Agent';
  if (selection.type === 'session') return selection.sessionId || 'Session';
  if (selection.type === 'channel') return selection.channel || selection.resourceName || 'Channel';
  return selection.resourceName || selection.type;
}

function getSelectionSubtitle(selection: Selection | null) {
  if (!selection) return 'Select a namespace, agent, deployment, sandbox, template, MCP server, or session.';
  if (selection.type === 'namespace') return 'Namespace';
  if (selection.type === 'agent') return `${selection.ns} / Agent`;
  if (selection.type === 'session') return `${selection.ns} / ${selection.agent}`;
  if (selection.type === 'channel') return `${selection.ns} / Channel`;
  if (selection.type === 'channel-subscription') return `${selection.ns} / ${selection.channel} / ChannelSubscription`;
  if (selection.type === 'schedule') return `${selection.ns} / Schedule`;
  if (selection.type === 'mcp-binding') return `${selection.ns} / MCP Binding`;
  if (selection.type === 'knowledge') return `${selection.ns} / Knowledge`;
  if (selection.type === 'template') return `${selection.ns} / Template`;
  if (selection.type === 'deployment') return `${selection.ns} / Deployment`;
  if (selection.type === 'deployment-replica') return `${selection.ns} / DeploymentReplica`;
  if (selection.type === 'sandbox-class') return `${selection.ns} / SandboxClass`;
  if (selection.type === 'sandbox-policy') return `${selection.ns} / SandboxPolicy`;
  if (selection.type === 'sandbox') return `${selection.ns} / Sandbox`;
  return 'Sys / MCPServer';
}

function positiveIntParam(searchParams: URLSearchParams, name: string) {
  const value = searchParams.get(name);
  if (!value || !/^\d+$/.test(value)) return undefined;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : undefined;
}

function selectionIcon(selection: Selection | null) {
  if (!selection) return <FileText className="w-4 h-4 text-muted-foreground" />;
  if (selection.type === 'namespace') return <Folder className="w-4 h-4 text-muted-foreground" />;
  if (selection.type === 'agent') return <Cpu className="w-4 h-4 text-emerald-500" />;
  if (selection.type === 'session') return <MessageSquare className="w-4 h-4 text-blue-500" />;
  if (selection.type === 'channel') return <Hash className="w-4 h-4 text-cyan-400" />;
  if (selection.type === 'channel-subscription') return <Radio className="w-4 h-4 text-cyan-300" />;
  if (selection.type === 'schedule') return <Clock3 className="w-4 h-4 text-amber-500" />;
  if (selection.type === 'mcp-binding') return <Plug className="w-4 h-4 text-blue-500" />;
  if (selection.type === 'knowledge') return <FileText className="w-4 h-4 text-violet-400" />;
  if (selection.type === 'template') return <FileText className="w-4 h-4 text-emerald-500" />;
  if (selection.type === 'deployment') return <Layers3 className="w-4 h-4 text-indigo-400" />;
  if (selection.type === 'deployment-replica') return <Package className="w-4 h-4 text-indigo-300" />;
  if (selection.type === 'sandbox-class') return <ShieldCheck className="w-4 h-4 text-fuchsia-400" />;
  if (selection.type === 'sandbox-policy') return <Box className="w-4 h-4 text-fuchsia-300" />;
  if (selection.type === 'sandbox') return <Container className="w-4 h-4 text-orange-400" />;
  return <Plug className="w-4 h-4 text-blue-500" />;
}

async function uploadTalonImage({ file, namespace, agent, sessionId, signal }: TalonImageUploadContext) {
  const form = new FormData();
  form.set('file', file);
  form.set('namespace', namespace);
  form.set('agent', agent);
  form.set('sessionId', sessionId);

  const response = await fetch('/api/talon/objects', {
    method: 'POST',
    body: form,
    signal,
  });
  if (!response.ok) {
    const payload = await response.json().catch(() => ({}));
    const message = typeof payload?.error === 'string' ? payload.error : `Upload failed: ${response.status}`;
    throw new Error(message);
  }
  return response.json();
}

function talonObjectUrl(object: TalonChatObjectRef) {
  return object.key ? `/api/talon/objects?key=${encodeURIComponent(object.key)}` : undefined;
}

type StreamEventItem = {
  type: 'status' | 'tool_call' | 'tool_result' | 'reasoning' | 'usage' | 'error';
  content: string;
  name?: string;
  payload?: unknown;
};

type ScheduleDocument = {
  name?: string;
  ns?: string;
  labels?: Record<string, string>;
  spec?: {
    kind?: string;
    cron?: string;
    intervalSeconds?: number;
    interval_seconds?: number;
    runAt?: string;
    run_at?: string;
    timezone?: string;
    target?: {
      agent?: string;
      sessionMode?: string;
      session_mode?: string;
      sessionId?: string;
      session_id?: string;
    };
    inputMessage?: string;
    input_message?: string;
    enabled?: boolean;
  };
  status?: {
    revision?: number;
    nextRunAt?: string | number;
    next_run_at?: string | number;
    backendHandle?: string;
    backend_handle?: string;
    backendArmed?: boolean;
    backend_armed?: boolean;
    lastRunAt?: string | number;
    last_run_at?: string | number;
    lastSessionId?: string;
    last_session_id?: string;
    lastError?: string;
    last_error?: string;
    claimedRunAt?: string | number;
    claimed_run_at?: string | number;
    claimExpiresAt?: string | number;
    claim_expires_at?: string | number;
    recentEvents?: Array<Record<string, unknown>>;
    recent_events?: Array<Record<string, unknown>>;
  };
};

type ChannelDocument = {
  name?: string;
  ns?: string;
  title?: string;
  status?: string;
  metadata?: Record<string, string>;
  labels?: Record<string, string>;
};

type ChannelSubscriptionDocument = {
  name?: string;
  ns?: string;
  channel?: string;
  agent?: string;
  enabled?: boolean;
  trigger?: string;
  replyMode?: string;
  reply_mode?: string;
  contextPolicy?: {
    mode?: string;
    maxMessages?: number;
  };
  context_policy?: {
    mode?: string;
    max_messages?: number;
  };
};

function formatMicros(value: unknown) {
  const normalized = typeof value === 'string' ? Number(value) : value;
  if (typeof normalized !== 'number' || !Number.isFinite(normalized) || normalized <= 0) {
    return '—';
  }
  return new Date(normalized / 1000).toLocaleString([], {
    year: 'numeric',
    month: 'numeric',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
    second: '2-digit',
    hour12: true,
  });
}

function scheduleField<T>(primary: T | undefined, fallback: T | undefined): T | undefined {
  return primary ?? fallback;
}

function ScheduleInspector({
  schedule,
  resourceYaml,
  onOpenSession,
  onOpenAgent,
}: {
  schedule: ScheduleDocument;
  resourceYaml: string;
  onOpenSession: (sessionId: string) => void;
  onOpenAgent: (agent: string) => void;
}) {
  const [tab, setTab] = useState<'overview' | 'raw'>('overview');
  const spec = schedule.spec ?? {};
  const status = schedule.status ?? {};
  const target = spec.target ?? {};
  const recentEvents = (scheduleField(status.recentEvents, status.recent_events) ?? []) as Array<Record<string, unknown>>;
  const enabled = spec.enabled !== false;
  const backendArmed = scheduleField(status.backendArmed, status.backend_armed) === true;
  const nextRun = formatMicros(scheduleField(status.nextRunAt, status.next_run_at));
  const lastRun = formatMicros(scheduleField(status.lastRunAt, status.last_run_at));
  const claimedRun = formatMicros(scheduleField(status.claimedRunAt, status.claimed_run_at));
  const claimExpires = formatMicros(scheduleField(status.claimExpiresAt, status.claim_expires_at));
  const sessionMode = scheduleField(target.sessionMode, target.session_mode) || 'new';
  const sessionId = scheduleField(target.sessionId, target.session_id) || '';
  const inputMessage = scheduleField(spec.inputMessage, spec.input_message) || '';
  const lastSessionId = scheduleField(status.lastSessionId, status.last_session_id) || '';
  const lastError = scheduleField(status.lastError, status.last_error) || '';
  const cadence =
    spec.kind === 'cron'
      ? spec.cron || '—'
      : spec.kind === 'every'
        ? `Every ${scheduleField(spec.intervalSeconds, spec.interval_seconds) || 0}s`
        : scheduleField(spec.runAt, spec.run_at) || '—';

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-2xl border border-border bg-muted/20">
      <div className="border-b border-border px-4 py-3">
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            className={cn(
              "rounded-full px-3 py-1 text-xs font-medium",
              tab === 'overview' ? 'bg-foreground text-background' : 'bg-background text-muted-foreground border border-border'
            )}
            onClick={() => setTab('overview')}
          >
            Overview
          </button>
          <button
            type="button"
            className={cn(
              "rounded-full px-3 py-1 text-xs font-medium",
              tab === 'raw' ? 'bg-foreground text-background' : 'bg-background text-muted-foreground border border-border'
            )}
            onClick={() => setTab('raw')}
          >
            Raw YAML
          </button>
          <span className={cn("ml-auto rounded-full px-2 py-1 text-[11px] font-medium", enabled ? "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300" : "bg-muted text-muted-foreground")}>
            {enabled ? 'Enabled' : 'Disabled'}
          </span>
          <span className={cn("rounded-full px-2 py-1 text-[11px] font-medium", backendArmed ? "bg-blue-500/15 text-blue-700 dark:text-blue-300" : "bg-amber-500/15 text-amber-700 dark:text-amber-300")}>
            {backendArmed ? 'Armed' : 'Unarmed'}
          </span>
        </div>
      </div>

      {tab === 'raw' ? (
        <pre className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap break-words p-4 text-[13px] leading-relaxed text-foreground [overflow-wrap:anywhere]">
          <code>{resourceYaml}</code>
        </pre>
      ) : (
        <div className="grid min-h-0 flex-1 gap-4 overflow-auto p-4 md:grid-cols-2">
          <div className="rounded-xl border border-border bg-background/70 p-4">
            <div className="text-xs uppercase tracking-wide text-muted-foreground">Overview</div>
            <dl className="mt-3 space-y-2 text-sm">
              <div className="flex justify-between gap-4"><dt className="text-muted-foreground">Kind</dt><dd>{spec.kind || '—'}</dd></div>
              <div className="flex justify-between gap-4"><dt className="text-muted-foreground">Cadence</dt><dd className="text-right">{cadence}</dd></div>
              <div className="flex justify-between gap-4"><dt className="text-muted-foreground">Timezone</dt><dd>{spec.timezone || 'UTC/default'}</dd></div>
              <div className="flex justify-between gap-4"><dt className="text-muted-foreground">Revision</dt><dd>{status.revision ?? '—'}</dd></div>
              <div className="flex justify-between gap-4"><dt className="text-muted-foreground">Next run</dt><dd className="text-right">{nextRun}</dd></div>
              <div className="flex justify-between gap-4"><dt className="text-muted-foreground">Last run</dt><dd className="text-right">{lastRun}</dd></div>
              <div className="flex justify-between gap-4"><dt className="text-muted-foreground">Claimed run</dt><dd className="text-right">{claimedRun}</dd></div>
              <div className="flex justify-between gap-4"><dt className="text-muted-foreground">Claim expires</dt><dd className="text-right">{claimExpires}</dd></div>
            </dl>
          </div>

          <div className="rounded-xl border border-border bg-background/70 p-4">
            <div className="text-xs uppercase tracking-wide text-muted-foreground">Target</div>
            <dl className="mt-3 space-y-2 text-sm">
              <div className="flex justify-between gap-4">
                <dt className="text-muted-foreground">Agent</dt>
                <dd>
                  {target.agent ? (
                    <button type="button" className="text-blue-600 hover:underline dark:text-blue-300" onClick={() => onOpenAgent(target.agent!)}>
                      {target.agent}
                    </button>
                  ) : '—'}
                </dd>
              </div>
              <div className="flex justify-between gap-4"><dt className="text-muted-foreground">Session mode</dt><dd>{sessionMode}</dd></div>
              <div className="flex justify-between gap-4"><dt className="text-muted-foreground">Session id</dt><dd className="text-right">{sessionId || '—'}</dd></div>
            </dl>
            <div className="mt-4 text-xs uppercase tracking-wide text-muted-foreground">Input message</div>
            <div className="mt-2 rounded-lg border border-border bg-muted/30 p-3 text-sm whitespace-pre-wrap">
              {inputMessage || '—'}
            </div>
          </div>

          <div className="rounded-xl border border-border bg-background/70 p-4 md:col-span-2">
            <div className="flex items-center justify-between gap-4">
              <div className="text-xs uppercase tracking-wide text-muted-foreground">Runtime status</div>
              {lastSessionId ? (
                <button type="button" className="text-xs text-blue-600 hover:underline dark:text-blue-300" onClick={() => onOpenSession(lastSessionId)}>
                  Open last session
                </button>
              ) : null}
            </div>
            <div className="mt-3 grid gap-3 md:grid-cols-2">
              <div className="rounded-lg border border-border bg-muted/30 p-3">
                <div className="text-xs text-muted-foreground">Last session</div>
                <div className="mt-1 text-sm">{lastSessionId || '—'}</div>
              </div>
              <div className="rounded-lg border border-border bg-muted/30 p-3">
                <div className="text-xs text-muted-foreground">Last error</div>
                <div className="mt-1 text-sm whitespace-pre-wrap">{lastError || '—'}</div>
              </div>
            </div>
          </div>

          <div className="rounded-xl border border-border bg-background/70 p-4 md:col-span-2">
            <div className="text-xs uppercase tracking-wide text-muted-foreground">Recent events</div>
            <div className="mt-3 space-y-3">
              {recentEvents.length === 0 ? (
                <div className="text-sm text-muted-foreground">No recent schedule lifecycle events.</div>
              ) : recentEvents.slice().reverse().map((event, index) => (
                <div key={`${event.timestamp ?? index}-${event.phase ?? 'event'}`} className="rounded-lg border border-border bg-muted/30 p-3">
                  <div className="flex flex-wrap items-center gap-2 text-xs">
                    <span className="font-medium text-foreground">{String(event.phase ?? 'event')}</span>
                    <span className="rounded-full bg-muted px-2 py-0.5 text-muted-foreground">{String(event.outcome ?? 'unknown')}</span>
                    <span className="ml-auto text-muted-foreground">{formatMicros(event.timestamp)}</span>
                  </div>
                  <div className="mt-2 text-sm whitespace-pre-wrap">{String(event.detail ?? '') || '—'}</div>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function ChannelInspector({
  gatewayUrl,
  authToken,
  channel,
  resourceYaml,
  onOpenSession,
}: {
  gatewayUrl: string;
  authToken: string;
  channel: ChannelDocument;
  resourceYaml: string;
  onOpenSession: (agent: string, sessionId: string) => void;
}) {
  const [tab, setTab] = useState<'messages' | 'subscriptions' | 'raw'>('messages');
  const [subscriptions, setSubscriptions] = useState<ChannelSubscriptionDocument[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const ns = channel.ns || '';
  const channelName = channel.name || '';
  const currentChannelRef = useRef({ ns, channelName });

  useEffect(() => {
    currentChannelRef.current = { ns, channelName };
  }, [ns, channelName]);

  const refresh = useCallback(async () => {
    if (!ns || !channelName) return;
    const requestNs = ns;
    const requestChannelName = channelName;
    setIsLoading(true);
    setError(null);
    try {
      const subscriptionsPayload = await getGatewayClient().listResources({
        ns: requestNs,
        kind: 'ChannelSubscription',
      });
      if (
        requestNs !== currentChannelRef.current.ns ||
        requestChannelName !== currentChannelRef.current.channelName
      ) {
        return;
      }
      setSubscriptions(
        ((subscriptionsPayload.resources || []) as ResourceEnvelope[])
          .map(channelSubscriptionDocumentFromResource)
          .filter((subscription) => subscription.channel === requestChannelName),
      );
    } catch (err: any) {
      if (
        requestNs === currentChannelRef.current.ns &&
        requestChannelName === currentChannelRef.current.channelName
      ) {
        setError(err?.message || 'Failed to load channel subscriptions');
      }
    } finally {
      if (
        requestNs === currentChannelRef.current.ns &&
        requestChannelName === currentChannelRef.current.channelName
      ) {
        setIsLoading(false);
      }
    }
  }, [channelName, ns]);

  useEffect(() => {
    setSubscriptions([]);
    setIsLoading(false);
    setError(null);
    refresh();
  }, [refresh]);

  const status = channel.status || 'open';

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-2xl border border-border bg-muted/20">
      <div className="flex flex-wrap items-center gap-2 border-b border-border px-4 py-3">
        {(['messages', 'subscriptions', 'raw'] as const).map((nextTab) => (
          <button
            key={nextTab}
            type="button"
            className={cn(
              "rounded-full px-3 py-1 text-xs font-medium capitalize",
              tab === nextTab ? 'bg-foreground text-background' : 'bg-background text-muted-foreground border border-border'
            )}
            onClick={() => setTab(nextTab)}
          >
            {nextTab}
          </button>
        ))}
        <span className={cn("ml-auto rounded-full px-2 py-1 text-[11px] font-medium", status === 'open' ? "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300" : "bg-muted text-muted-foreground")}>
          {status}
        </span>
      </div>

      {tab === 'raw' ? (
        <pre className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap break-words p-4 text-[13px] leading-relaxed text-foreground [overflow-wrap:anywhere]">
          <code>{resourceYaml}</code>
        </pre>
      ) : tab === 'subscriptions' ? (
        <div className="min-h-0 flex-1 overflow-auto p-4">
          {isLoading && <div className="mb-3 text-xs text-muted-foreground">Loading subscriptions…</div>}
          {error && <div className="mb-3 rounded-lg border border-red-200/60 bg-red-50/60 p-3 text-sm text-red-700 dark:border-red-900/40 dark:bg-red-950/20 dark:text-red-400">{error}</div>}
          {subscriptions.length === 0 ? (
            <div className="text-sm text-muted-foreground">No channel subscriptions.</div>
          ) : (
            <div className="grid gap-3 md:grid-cols-2">
              {subscriptions.map((subscription) => {
                const policy = (subscription.contextPolicy || subscription.context_policy) as
                  | { mode?: string; maxMessages?: number; max_messages?: number }
                  | undefined;
                const maxMessages = policy?.maxMessages ?? policy?.max_messages ?? 20;
                return (
                  <div key={subscription.name} className="rounded-xl border border-border bg-background/70 p-4">
                    <div className="flex items-start justify-between gap-3">
                      <div>
                        <div className="text-sm font-medium text-foreground">{subscription.name || 'unnamed'}</div>
                        <div className="mt-1 text-xs text-muted-foreground">{subscription.agent || 'no agent'}</div>
                      </div>
                      <span className={cn("rounded-full px-2 py-1 text-[11px] font-medium", subscription.enabled === false ? "bg-muted text-muted-foreground" : "bg-cyan-500/15 text-cyan-700 dark:text-cyan-300")}>
                        {subscription.enabled === false ? 'disabled' : (subscription.trigger || 'mention')}
                      </span>
                    </div>
                    <div className="mt-3 text-xs text-muted-foreground">
                      Context: {policy?.mode || 'recent_public'} / {maxMessages} messages
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      ) : (
        <TalonChannel
          className="min-h-0 flex-1"
          gatewayUrl={gatewayUrl}
          authToken={authToken}
          gatewayClient={getGatewayClient()}
          namespace={ns}
          channel={channel}
          renderMessageActions={(message) => {
            const sourceAgent = message.sourceAgent || message.source_agent || '';
            const sourceSessionId = message.sourceSessionId || message.source_session_id || '';
            if (!sourceAgent || !sourceSessionId) return null;
            return (
              <button
                type="button"
                className="text-xs text-blue-600 hover:underline dark:text-blue-300"
                onClick={() => onOpenSession(sourceAgent, sourceSessionId)}
              >
                Open session
              </button>
            );
          }}
        />
      )}
    </div>
  );
}

function extractStreamEvents(data: unknown): StreamEventItem[] {
  if (!Array.isArray(data)) return [];

  return data.flatMap((entry) => {
    if (!Array.isArray(entry)) return [];
    return entry.filter((item): item is StreamEventItem => {
      if (!item || typeof item !== 'object') return false;
      const candidate = item as { type?: unknown; content?: unknown };
      return (
        (
          candidate.type === 'status' ||
          candidate.type === 'tool_call' ||
          candidate.type === 'tool_result' ||
          candidate.type === 'reasoning' ||
          candidate.type === 'usage' ||
          candidate.type === 'error'
        ) &&
        typeof candidate.content === 'string'
      );
    });
  });
}

function KnowledgeExplorer({ isConnected, selection }: { isConnected: boolean, selection: Selection | null }) {
  const [activeTab, setActiveTab] = useState<'context'|'search'>('context');
  const [contextData, setContextData] = useState<string>('');
  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<string[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const targetNamespace = selection?.ns || 'default';
  const targetAgent = selection?.agent || 'default';

  useEffect(() => {
    if (isConnected && activeTab === 'context') {
      setIsLoading(true);
      getGatewayClient().getKnowledge({ ns: targetNamespace, agent: targetAgent })
        .then(data => {
          setContextData(data.modules?.map((m: any) => `[${m.path}]\n${m.content}`).join('\n\n') || '');
          setIsLoading(false);
        })
        .catch(() => setIsLoading(false));
    }
  }, [isConnected, activeTab, targetAgent, targetNamespace]);

  useEffect(() => {
    if (isConnected && activeTab === 'search' && searchQuery.trim().length > 2) {
      const timer = setTimeout(() => {
        setIsLoading(true);
        getGatewayClient().searchKnowledge({ ns: targetNamespace, agent: targetAgent, query: searchQuery })
          .then(data => {
            setSearchResults(data.results?.map((r: any) => `[${r.path}]\n${r.snippet}`) || []);
            setIsLoading(false);
          })
          .catch(() => setIsLoading(false));
      }, 500);
      return () => clearTimeout(timer);
    } else {
      setSearchResults([]);
    }
  }, [isConnected, activeTab, searchQuery, targetAgent, targetNamespace]);

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center gap-2 mb-3">
        <Database className="w-3.5 h-3.5 text-muted-foreground stroke-[1.5]" />
        <h3 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Semantic Knowledge</h3>
      </div>
      
      <div className="flex bg-muted rounded-md p-1 mb-4 h-8 flex-shrink-0">
        <button 
          onClick={() => setActiveTab('context')}
          className={cn("flex-1 text-[12px] font-medium rounded-sm flex items-center justify-center gap-1.5 transition-colors", activeTab === 'context' ? "bg-background text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground")}
        >
          <FileText className="w-3.5 h-3.5" /> Context
        </button>
        <button 
          onClick={() => setActiveTab('search')}
          className={cn("flex-1 text-[12px] font-medium rounded-sm flex items-center justify-center gap-1.5 transition-colors", activeTab === 'search' ? "bg-background text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground")}
        >
          <Search className="w-3.5 h-3.5" /> Search
        </button>
      </div>

      <div className="flex-1 overflow-y-auto min-h-0 bg-background border border-border rounded-md p-3 relative">
        {isLoading && (
          <div className="absolute top-2 right-2 flex gap-1">
             <div className="w-1.5 h-1.5 bg-muted-foreground rounded-full animate-bounce [animation-delay:-0.3s]" />
             <div className="w-1.5 h-1.5 bg-muted-foreground rounded-full animate-bounce [animation-delay:-0.15s]" />
             <div className="w-1.5 h-1.5 bg-muted-foreground rounded-full animate-bounce" />
          </div>
        )}

        {activeTab === 'context' ? (
          <div className="text-[12px] text-muted-foreground whitespace-pre-wrap font-mono leading-relaxed">
            {contextData || "No long-term knowledge context established."}
          </div>
        ) : (
          <div className="flex flex-col h-full gap-3">
            <div className="relative flex-shrink-0">
              <Search className="w-3.5 h-3.5 absolute left-2.5 top-2.5 text-muted-foreground" />
              <input 
                type="text" 
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder={`Search ${targetNamespace} knowledge...`}
                className="w-full bg-muted border border-border rounded-md pl-8 pr-3 py-1.5 text-[12px] font-medium focus:outline-none focus:ring-1 focus:ring-ring text-foreground"
              />
            </div>
            <div className="flex-1 overflow-y-auto space-y-2">
              {searchResults.length === 0 && searchQuery.trim().length > 2 && !isLoading && (
                <div className="text-center text-[11px] text-muted-foreground py-4">No semantic matches found.</div>
              )}
              {searchResults.map((result, idx) => (
                <div key={idx} className="p-2.5 rounded-md border border-border bg-muted/30 text-[12px] text-foreground leading-relaxed">
                  {result}
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function DebuggerPageContent() {
  const router = useRouter();
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const nextHistoryModeRef = useRef<'push' | 'replace'>('replace');
  const [gatewayUrl, setGatewayUrl] = useState('');
  const [gatewayHttpUrl, setGatewayHttpUrl] = useState('');
  const [authToken, setAuthToken] = useState('');
  const [googleSsoEnabled, setGoogleSsoEnabled] = useState(false);
  const [googleWebClientId, setGoogleWebClientId] = useState<string | null>(null);
  const [googleSsoError, setGoogleSsoError] = useState<string | null>(null);
  const [isConnected, setIsConnected] = useState(false);
  const [isHoveringConnection, setIsHoveringConnection] = useState(false);
  const [selectedNamespace, setSelectedNamespace] = useState<Selection | null>(null);
  const [isSidebarPinned, setIsSidebarPinned] = useState(true);
  const [isSidebarHovered, setIsSidebarHovered] = useState(false);
  const [isRightSidebarPinned, setIsRightSidebarPinned] = useState(true);
  const [isRightSidebarHovered, setIsRightSidebarHovered] = useState(false);
  const [resourceYaml, setResourceYaml] = useState<string>('');
  const [resourceDocument, setResourceDocument] = useState<any | null>(null);
  const [resourceLoading, setResourceLoading] = useState(false);
  const [resourceError, setResourceError] = useState<string | null>(null);
  const [storageHydrated, setStorageHydrated] = useState(false);
  const lastSyncedQueryRef = useRef<string | null>(null);

  const handleSelectionChange = useCallback(
    (selection: Selection | null, historyMode: 'push' | 'replace' = 'push') => {
      nextHistoryModeRef.current = historyMode;
      setSelectedNamespace(selection);
    },
    []
  );

  useEffect(() => {
    const savedUrl = localStorage.getItem('talon_gateway_url');
    const defaultGatewayUrl = process.env.NEXT_PUBLIC_GATEWAY_URL || 'https://envoy.talon.orb.local';
    if (savedUrl) {
      setGatewayUrl(savedUrl);
    } else {
      setGatewayUrl(defaultGatewayUrl);
    }
    const savedHttpUrl = localStorage.getItem('talon_gateway_http_url');
    if (savedHttpUrl) {
      setGatewayHttpUrl(savedHttpUrl);
    } else {
      setGatewayHttpUrl(process.env.NEXT_PUBLIC_GATEWAY_HTTP_URL || defaultGatewayUrl);
    }
    const savedToken = localStorage.getItem('talon_auth_token');
    if (savedToken) {
      setAuthToken(savedToken);
    }
    setStorageHydrated(true);
  }, []);

  useEffect(() => {
    if (!storageHydrated) return;

    const currentQuery = searchParams.toString();
    lastSyncedQueryRef.current = currentQuery;

    const nextSelection = selectionFromSearchParams(new URLSearchParams(currentQuery));
    setSelectedNamespace(prev => areSelectionsEqual(prev, nextSelection) ? prev : nextSelection);

    const wantsConnected = searchParams.get('connected') === 'true';
    if (wantsConnected && gatewayUrl.trim()) {
      updateGatewayClient(gatewayUrl.trim());
      setIsConnected(true);
      return;
    }

    if (!wantsConnected) {
      setIsConnected(false);
    }
  }, [storageHydrated, searchParams, gatewayUrl]);

  const effectiveGatewayHttpUrl = gatewayHttpUrl.trim() || gatewayUrl.trim();

  useEffect(() => {
    if (!storageHydrated || !effectiveGatewayHttpUrl.trim()) return;
    let cancelled = false;
    const loadAuthConfig = async () => {
      try {
        const response = await fetch(`${effectiveGatewayHttpUrl.replace(/\/+$/, '')}/v1/oidc/sso`);
        if (!response.ok) throw new Error(`status ${response.status}`);
        const config = await response.json();
        if (!cancelled) {
          setGoogleSsoEnabled(Boolean(config.google_sso_enabled && config.google_web_client_id));
          setGoogleWebClientId(config.google_web_client_id || null);
          setGoogleSsoError(null);
        }
      } catch {
        if (!cancelled) {
          setGoogleSsoEnabled(false);
          setGoogleWebClientId(null);
        }
      }
    };
    loadAuthConfig();
    return () => {
      cancelled = true;
    };
  }, [effectiveGatewayHttpUrl, storageHydrated]);

  const handleGoogleSignIn = useCallback(async () => {
    if (!googleWebClientId) return;
    setGoogleSsoError(null);

    const loadGoogleScript = () =>
      new Promise<void>((resolve, reject) => {
        if (window.google?.accounts?.id) {
          resolve();
          return;
        }
        const existing = document.querySelector<HTMLScriptElement>('script[src="https://accounts.google.com/gsi/client"]');
        if (existing) {
          if (window.google?.accounts?.id) {
            resolve();
            return;
          }
          existing.addEventListener('load', () => resolve(), { once: true });
          existing.addEventListener('error', () => reject(new Error('Google sign-in script failed to load')), { once: true });
          return;
        }
        const script = document.createElement('script');
        script.src = 'https://accounts.google.com/gsi/client';
        script.async = true;
        script.defer = true;
        script.onload = () => resolve();
        script.onerror = () => reject(new Error('Google sign-in script failed to load'));
        document.head.appendChild(script);
      });

    try {
      await loadGoogleScript();
      window.google?.accounts?.id?.initialize({
        client_id: googleWebClientId,
        callback: async (response) => {
          try {
            if (!response.credential) throw new Error('Google did not return an ID token');
            const exchange = await fetch(`${effectiveGatewayHttpUrl.replace(/\/+$/, '')}/v1/oidc/exchange`, {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ idToken: response.credential, clientType: 'sightline' }),
            });
            const payload = await exchange.json();
            if (!exchange.ok) throw new Error(payload?.error || 'Google sign-in failed');
            setAuthToken(payload.access_token);
            localStorage.setItem('talon_auth_token', payload.access_token);
            if (gatewayUrl.trim()) {
              localStorage.setItem('talon_gateway_url', gatewayUrl.trim());
              updateGatewayClient(gatewayUrl.trim());
              setIsConnected(true);
            }
          } catch (err: any) {
            setGoogleSsoError(err?.message || 'Google sign-in failed');
          }
        },
      });
      window.google?.accounts?.id?.prompt();
    } catch (err: any) {
      setGoogleSsoError(err?.message || 'Google sign-in failed');
    }
  }, [effectiveGatewayHttpUrl, gatewayUrl, googleWebClientId]);

  useEffect(() => {
    if (!storageHydrated) return;

    const nextQuery = buildSearchParams(isConnected, selectedNamespace, searchParams).toString();
    if (nextQuery === lastSyncedQueryRef.current) return;

    lastSyncedQueryRef.current = nextQuery;
    const nextUrl = nextQuery ? `${pathname}?${nextQuery}` : pathname;
    const historyMode = nextHistoryModeRef.current;
    nextHistoryModeRef.current = 'replace';
    if (historyMode === 'push') {
      router.push(nextUrl, { scroll: false });
    } else {
      router.replace(nextUrl, { scroll: false });
    }
  }, [storageHydrated, isConnected, selectedNamespace, pathname, router, searchParams]);

  useEffect(() => {
    if (!isConnected || !selectedNamespace || selectedNamespace.type === 'session') {
      setResourceYaml('');
      setResourceDocument(null);
      setResourceLoading(false);
      setResourceError(null);
      return;
    }

    const selection = selectedNamespace;
    let cancelled = false;
    const fetchResource = async () => {
      setResourceLoading(true);
      setResourceError(null);

      try {
        let document: any;
        switch (selection.type) {
          case 'namespace':
            document = await getGatewayClient().getNamespace({ name: selection.ns });
            break;
          case 'agent':
            document = (await getGatewayClient().getResource({
              ns: selection.ns,
              kind: 'Agent',
              name: selection.agent || '',
            })).resource;
            break;
          case 'channel':
            document = channelDocumentFromResource(((await getGatewayClient().getResource({
              ns: selection.ns,
              kind: 'Channel',
              name: selection.resourceName || selection.channel || '',
            })).resource || {}) as ResourceEnvelope);
            break;
          case 'channel-subscription':
            document = channelSubscriptionDocumentFromResource(((await getGatewayClient().getResource({
              ns: selection.ns,
              kind: 'ChannelSubscription',
              name: selection.resourceName || '',
            })).resource || {}) as ResourceEnvelope);
            break;
          case 'schedule':
            document = scheduleDocumentFromResource(((await getGatewayClient().getResource({
              ns: selection.ns,
              kind: 'Schedule',
              name: selection.resourceName || '',
            })).resource || {}) as ResourceEnvelope);
            break;
          case 'template':
          case 'deployment':
          case 'deployment-replica':
          case 'sandbox-class':
          case 'sandbox-policy':
          case 'sandbox':
          case 'mcp-server':
          case 'mcp-binding':
          case 'knowledge': {
            const kind = RESOURCE_KIND_BY_SELECTION[selection.type];
            if (!kind) throw new Error(`Unsupported resource selection '${selection.type}'`);
            document = (await getGatewayClient().getResource({
              ns: selection.type === 'mcp-server' ? SYSTEM_NAMESPACE : selection.ns,
              kind,
              name: selection.resourceName || '',
            })).resource;
            break;
          }
        }
        if (!document) {
          throw new Error('Resource not found');
        }

        if (!cancelled) {
          const displayDocument = isV2ResourceDocument(document)
            ? resourceToManifestDocument(document)
            : yamlSafeValue(document);
          setResourceDocument(displayDocument);
          setResourceYaml(dump(displayDocument, { noRefs: true, lineWidth: 100 }));
        }
      } catch (err: any) {
        if (!cancelled) {
          setResourceError(err?.message || 'Failed to load resource');
          setResourceYaml('');
          setResourceDocument(null);
        }
      } finally {
        if (!cancelled) {
          setResourceLoading(false);
        }
      }
    };

    fetchResource();
    return () => {
      cancelled = true;
    };
  }, [authToken, gatewayUrl, isConnected, selectedNamespace]);

  const handleConnect = (e: React.FormEvent) => {
    e.preventDefault();
    if (gatewayUrl.trim()) {
      localStorage.setItem('talon_gateway_url', gatewayUrl.trim());
      if (gatewayHttpUrl.trim()) {
        localStorage.setItem('talon_gateway_http_url', gatewayHttpUrl.trim());
      } else {
        localStorage.removeItem('talon_gateway_http_url');
      }
      if (authToken.trim()) {
        localStorage.setItem('talon_auth_token', authToken.trim());
      } else {
        localStorage.removeItem('talon_auth_token');
      }
      updateGatewayClient(gatewayUrl.trim());
      setIsConnected(true);
    }
  };

  return (
    <div className="flex h-screen min-w-0 flex-row overflow-x-hidden overflow-y-hidden bg-background text-foreground">
      {/* Invisible Hover Zone at Left Edge */}
      {!isSidebarPinned && !isSidebarHovered && (
        <div 
          className="fixed left-0 top-0 bottom-0 w-4 z-50 cursor-e-resize hidden md:block"
          onMouseEnter={() => setIsSidebarHovered(true)}
        />
      )}

      {/* Left Sidebar (Namespaces) - Full Height */}
      <motion.div 
        initial={false}
        animate={{ 
          width: (isSidebarPinned || isSidebarHovered) ? 288 : 0,
          opacity: (isSidebarPinned || isSidebarHovered) ? 1 : 0,
          x: (isSidebarPinned || isSidebarHovered) ? 0 : -20
        }}
        transition={{ type: 'spring', stiffness: 300, damping: 30 }}
        className={cn(
          "border-r border-border/70 bg-background/78 backdrop-blur-xl hidden md:flex flex-col flex-shrink-0 z-50 h-full group/sidebar overflow-hidden shadow-[0_18px_48px_rgba(0,0,0,0.24)]",
          isSidebarPinned ? "relative shadow-none" : "absolute shadow-2xl"
        )}
        onMouseLeave={() => {
          if (!isSidebarPinned) setIsSidebarHovered(false);
        }}
      >
        <div className="absolute top-3 right-3 z-50 opacity-0 group-hover/sidebar:opacity-100 transition-opacity">
          <button
            onClick={(e) => {
              e.stopPropagation();
              setIsSidebarPinned(!isSidebarPinned);
              if (isSidebarPinned) {
                setIsSidebarHovered(false);
              }
            }}
            className="rounded-md p-1 text-muted-foreground transition-colors hover:bg-muted"
          >
            {isSidebarPinned ? <ChevronsLeft className="w-4 h-4" /> : <ChevronsRight className="w-4 h-4" />}
          </button>
        </div>
        <div className="w-64 lg:w-72 h-full flex flex-col flex-shrink-0">
          <NamespaceExplorer 
            isConnected={isConnected} 
            selectedNode={selectedNamespace} 
            onSelect={setSelectedNamespace} 
          />
        </div>
      </motion.div>

      <div className="flex-1 flex flex-col min-w-0 bg-transparent">
        {/* Top Navigation */}
        <header className="h-14 w-full border-b border-border/70 flex flex-shrink-0 items-center justify-between px-4 lg:px-6 bg-background/72 backdrop-blur-xl z-10">
          <div className="flex items-center gap-3">
            <Terminal className="w-5 h-5 text-foreground stroke-[1.5]" />
            <div className="flex items-center gap-2">
              <h1 className="text-sm font-semibold tracking-tight">Talon Sightline</h1>
              {selectedNamespace?.agent && (
                <>
                  <div className="h-3 w-px bg-border/60 mx-1" />
                  <span className="text-sm font-medium text-muted-foreground flex items-center gap-1.5">
                    <Cpu className="w-3 h-3" />
                    {selectedNamespace.agent}
                  </span>
                </>
              )}
            </div>
            <div className="h-4 w-px bg-border mx-2" />
            <span className="text-xs text-muted-foreground font-mono bg-white/[0.045] px-2 py-0.5 rounded-md border border-border/60">
              v1.0.0-alpha
            </span>
          </div>

          <div className="flex items-center gap-4">
            {isConnected ? (
              <div 
                className="flex items-center gap-2 px-3 py-1.5 rounded-xl text-[13px] font-medium transition-all bg-emerald-500/9 text-emerald-300 border border-emerald-500/16 cursor-pointer hover:bg-red-500/10 hover:text-red-300 hover:border-red-500/16"
                onClick={() => setIsConnected(false)}
                onMouseEnter={() => setIsHoveringConnection(true)}
                onMouseLeave={() => setIsHoveringConnection(false)}
              >
                {isHoveringConnection ? <WifiOff className="w-3.5 h-3.5" /> : <Wifi className="w-3.5 h-3.5" />}
                {isHoveringConnection ? 'Disconnect' : 'Connected'}
              </div>
            ) : (
              <div className="flex items-center gap-2 px-3 py-1.5 rounded-xl text-[13px] font-medium bg-white/[0.045] text-muted-foreground border border-border/70">
                <WifiOff className="w-3.5 h-3.5" />
                Offline
              </div>
            )}
          </div>
        </header>

        {/* Main Content */}
        <main className="flex min-w-0 flex-1 overflow-x-hidden overflow-y-hidden bg-transparent">
          {/* Center Pane */}
          <div className="relative flex min-w-0 flex-1 flex-col overflow-hidden bg-transparent">
          {!isConnected ? (
            <div className="absolute inset-0 flex flex-col items-center justify-center bg-background/44 backdrop-blur-md z-20">
              <motion.div 
                initial={{ opacity: 0, scale: 0.95 }}
                animate={{ opacity: 1, scale: 1 }}
                className="w-full max-w-md p-8 bg-background/84 border border-border/70 shadow-[0_24px_80px_rgba(0,0,0,0.42)] rounded-[1.5rem] backdrop-blur-xl"
              >
                <div className="flex flex-col items-center text-center space-y-4 mb-8">
                  <div className="w-12 h-12 bg-white/[0.045] rounded-xl flex items-center justify-center border border-border/70">
                    <Activity className="w-6 h-6 text-foreground stroke-[1.5]" />
                  </div>
                  <div>
                    <h2 className="text-lg font-semibold text-foreground">Connect to Talon Engine</h2>
                    <p className="text-[13px] text-muted-foreground mt-1">Provide gateway endpoints for this workspace.</p>
                  </div>
                </div>
                
                <form onSubmit={handleConnect} className="space-y-4">
                  <div className="space-y-2">
                    <label className="text-[12px] font-medium text-foreground">gRPC Gateway URL</label>
                    <input 
                      type="url" 
                      required
                      value={gatewayUrl}
                      onChange={(e) => setGatewayUrl(e.target.value)}
                      className="w-full bg-white/[0.03] border border-border/70 text-foreground px-3 py-2.5 rounded-xl focus:outline-none focus:ring-1 focus:ring-ring focus:border-ring text-sm transition-shadow font-mono"
                      placeholder="http://localhost:18789"
                      autoFocus
                    />
                  </div>
                  <div className="space-y-2">
                    <label className="text-[12px] font-medium text-foreground">HTTP Gateway URL</label>
                    <input
                      type="url"
                      value={gatewayHttpUrl}
                      onChange={(e) => setGatewayHttpUrl(e.target.value)}
                      className="w-full bg-white/[0.03] border border-border/70 text-foreground px-3 py-2.5 rounded-xl focus:outline-none focus:ring-1 focus:ring-ring focus:border-ring text-sm transition-shadow font-mono"
                      placeholder="http://localhost:50052"
                    />
                  </div>
                  <div className="space-y-2">
                    <label className="text-[12px] font-medium text-foreground">Authorization Token (Optional)</label>
                    <input 
                      type="password" 
                      value={authToken}
                      onChange={(e) => setAuthToken(e.target.value)}
                      className="w-full bg-white/[0.03] border border-border/70 text-foreground px-3 py-2.5 rounded-xl focus:outline-none focus:ring-1 focus:ring-ring focus:border-ring text-sm transition-shadow font-mono"
                      placeholder="Enter bearer token"
                    />
                  </div>
                  {googleSsoEnabled ? (
                    <div className="space-y-2">
                      <button
                        type="button"
                        onClick={handleGoogleSignIn}
                        className="w-full bg-white/[0.05] text-foreground py-2.5 rounded-xl text-[13px] font-medium hover:bg-white/[0.08] border border-border/70 transition-all flex items-center justify-center gap-2"
                      >
                        <ShieldCheck className="w-4 h-4 stroke-[2]" />
                        Sign in with Google
                      </button>
                      {googleSsoError ? (
                        <p className="text-[12px] text-red-400">{googleSsoError}</p>
                      ) : null}
                    </div>
                  ) : null}
                  <button 
                    type="submit"
                    disabled={!gatewayUrl.trim()}
                    className="w-full bg-foreground text-background py-2.5 rounded-xl text-[13px] font-medium hover:opacity-90 disabled:opacity-50 transition-all flex items-center justify-center gap-2"
                  >
                    <Settings2 className="w-4 h-4 stroke-[2]" />
                    Initialize Connection
                  </button>
                </form>
              </motion.div>
            </div>
          ) : null}
          {selectedNamespace?.type === 'session' ? (
            <div className={cn("min-h-0 min-w-0 flex-1 overflow-hidden transition-opacity duration-300", !isConnected && "opacity-20 pointer-events-none")}>
              <TalonCopilot
                className="h-full"
                namespace={selectedNamespace.ns}
                agent={selectedNamespace.agent || 'default'}
                sessionId={selectedNamespace.type === 'session' ? selectedNamespace.sessionId : undefined}
                gatewayUrl={effectiveGatewayHttpUrl}
                authToken={authToken || undefined}
                gatewayClient={getGatewayClient()}
                historyPageSize={positiveIntParam(searchParams, 'historyPageSize')}
                enabledBuiltInCommands={['clear']}
                onImageUpload={isStaticExport ? undefined : uploadTalonImage}
                objectUrlForRef={isStaticExport ? undefined : talonObjectUrl}
                disabled={!isConnected}
                onSessionChange={(nextSessionId) => {
                  handleSelectionChange({
                    type: 'session',
                    ns: selectedNamespace.ns,
                    agent: selectedNamespace.agent || 'default',
                    sessionId: nextSessionId,
                    fullPath: `${selectedNamespace.ns}/${selectedNamespace.agent || 'default'}/${nextSessionId}`,
                  });
                }}
              />
            </div>
          ) : (
            <div className={cn("flex-1 overflow-y-auto overflow-x-hidden transition-opacity duration-300 elegant-scrollbar", !isConnected && "opacity-20 pointer-events-none")}>
              <div className="mx-auto flex h-full w-full max-w-5xl flex-col p-4 md:p-8">
                <div className="mb-6 flex items-center gap-3 border-b border-border pb-4">
                  <div className="flex h-10 w-10 items-center justify-center rounded-lg border border-border bg-muted/40">
                    {selectionIcon(selectedNamespace)}
                  </div>
                  <div>
                    <div className="text-lg font-semibold text-foreground">{getSelectionTitle(selectedNamespace)}</div>
                    <div className="text-sm text-muted-foreground">{getSelectionSubtitle(selectedNamespace)}</div>
                  </div>
                </div>

                {!selectedNamespace ? (
                  <div className="flex flex-1 items-center justify-center rounded-2xl border border-dashed border-border bg-muted/20">
                    <div className="text-center">
                          <div className="text-sm font-medium text-foreground">No resource selected</div>
                          <div className="mt-1 text-sm text-muted-foreground">Choose something from the explorer to inspect its YAML.</div>
                    </div>
                  </div>
                ) : resourceLoading ? (
                  <div className="flex flex-1 items-center justify-center rounded-2xl border border-border bg-muted/20">
                    <div className="text-sm text-muted-foreground">Loading resource…</div>
                  </div>
                ) : resourceError ? (
                  <div className="rounded-2xl border border-red-200/60 bg-red-50/60 p-4 text-sm text-red-700 dark:border-red-900/40 dark:bg-red-950/20 dark:text-red-400">
                    {resourceError}
                  </div>
                ) : selectedNamespace.type === 'schedule' && resourceDocument ? (
                  <ScheduleInspector
                    schedule={resourceDocument as ScheduleDocument}
                    resourceYaml={resourceYaml}
                    onOpenSession={(sessionId) => {
                      setSelectedNamespace({
                        type: 'session',
                        ns: selectedNamespace.ns,
                        agent: ((resourceDocument as ScheduleDocument).spec?.target?.agent || ''),
                        sessionId,
                        fullPath: `${selectedNamespace.ns}/${(resourceDocument as ScheduleDocument).spec?.target?.agent || ''}/${sessionId}`,
                      });
                    }}
                    onOpenAgent={(agent) => {
                      setSelectedNamespace({
                        type: 'agent',
                        ns: selectedNamespace.ns,
                        agent,
                        fullPath: `${selectedNamespace.ns}/${agent}`,
                      });
                    }}
                  />
                ) : selectedNamespace.type === 'channel' && resourceDocument ? (
                  <ChannelInspector
                    gatewayUrl={effectiveGatewayHttpUrl}
                    authToken={authToken}
                    channel={resourceDocument as ChannelDocument}
                    resourceYaml={resourceYaml}
                    onOpenSession={(agent, sessionId) => {
                      setSelectedNamespace({
                        type: 'session',
                        ns: selectedNamespace.ns,
                        agent,
                        sessionId,
                        fullPath: `${selectedNamespace.ns}/${agent}/${sessionId}`,
                      });
                    }}
                  />
                ) : (
                  <div className="min-h-0 min-w-0 flex-1 overflow-hidden rounded-2xl border border-border bg-muted/20">
                    <div className="border-b border-border px-4 py-2 text-[11px] uppercase tracking-wider text-muted-foreground">
                      YAML
                    </div>
                    <pre className="h-full overflow-auto whitespace-pre-wrap break-words p-4 text-[13px] leading-relaxed text-foreground [overflow-wrap:anywhere]">
                      <code>{resourceYaml}</code>
                    </pre>
                  </div>
                )}
              </div>
            </div>
          )}
        </div>
      </main>
    </div>

      {/* Telemetry Sidebar (Right Pane) */}
      {selectedNamespace?.type === 'session' && !isRightSidebarPinned && !isRightSidebarHovered && (
        <div 
          className="fixed right-0 top-0 bottom-0 w-4 z-50 cursor-w-resize hidden md:block"
          onMouseEnter={() => setIsRightSidebarHovered(true)}
        />
      )}

      {selectedNamespace?.type === 'session' && <motion.div 
        initial={false}
        animate={{ 
          width: (isRightSidebarPinned || isRightSidebarHovered) ? 320 : 0,
          opacity: (isRightSidebarPinned || isRightSidebarHovered) ? 1 : 0,
          x: (isRightSidebarPinned || isRightSidebarHovered) ? 0 : 20
        }}
        transition={{ type: 'spring', stiffness: 300, damping: 30 }}
        className={cn(
          "border-l border-border/70 bg-background/78 backdrop-blur-xl hidden md:flex flex-col gap-6 flex-shrink-0 z-50 h-full group/right-sidebar overflow-hidden",
          isRightSidebarPinned ? "relative shadow-none" : "absolute right-0 shadow-2xl"
        )}
        onMouseLeave={() => {
          if (!isRightSidebarPinned) setIsRightSidebarHovered(false);
        }}
      >
        <div className="absolute top-3 left-3 z-50 opacity-0 group-hover/right-sidebar:opacity-100 transition-opacity">
          <button
            onClick={(e) => {
              e.stopPropagation();
              setIsRightSidebarPinned(!isRightSidebarPinned);
              setIsRightSidebarHovered(false);
            }}
            className="p-1 rounded-md hover:bg-black/5 dark:hover:bg-white/10 text-muted-foreground transition-colors"
          >
            {isRightSidebarPinned ? <ChevronsRight className="w-4 h-4" /> : <ChevronsLeft className="w-4 h-4" />}
          </button>
        </div>
        <div className="w-80 h-full flex flex-col gap-6 p-4 flex-shrink-0">
          <div>
            <div className="flex items-center gap-2 mb-3">
              <Activity className="w-3.5 h-3.5 text-muted-foreground stroke-[1.5]" />
              <h3 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Infrastructure</h3>
            </div>
            <div className="space-y-[1px]">
              {[
                { name: 'Talon Engine', status: 'online', type: 'Rust Node' },
                { name: 'Gateway Proxy', status: 'online', type: 'WebSocket' },
                { name: 'Mobile Client', status: 'offline', type: 'iOS Sandbox' },
              ].map((node) => (
                <div key={node.name} className="flex items-center justify-between p-2.5 rounded-md hover:bg-muted/50 transition-colors">
                  <div className="flex items-center gap-3">
                    <div className={cn(
                      "w-1.5 h-1.5 rounded-full",
                      node.status === 'online' ? "bg-emerald-500" : "bg-muted-foreground/30"
                    )} />
                    <div>
                      <p className="text-[13px] font-medium text-foreground">{node.name}</p>
                    </div>
                  </div>
                  <span className="text-[11px] text-muted-foreground font-mono">{node.type}</span>
                </div>
              ))}
            </div>
          </div>

          <div className="h-px w-full bg-border flex-shrink-0" />

          <div className="flex-1 min-h-0 overflow-hidden">
            <KnowledgeExplorer isConnected={isConnected} selection={selectedNamespace} />
          </div>
        </div>
      </motion.div>}
    </div>
  );
}

export default function DebuggerPage() {
  return (
    <Suspense fallback={<div className="h-screen bg-background" />}>
      <DebuggerPageContent />
    </Suspense>
  );
}
