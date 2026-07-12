'use client';

import { Suspense, useMemo, useState, useRef, useEffect, useCallback } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { ChevronsLeft, ChevronsRight, Plug } from 'lucide-react';
import { motion } from 'framer-motion';
import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';
import { TalonChannel, TalonCopilot, type TalonChatObjectRef, type TalonImageUploadContext } from '@impalasys/talon-chat';
import { data } from '@impalasys/talon-client';
import { Explorer } from '../components/Explorer/Explorer';
import { ConnectionConfigScreen } from '../screens/ConnectionConfigScreen';
import { MainHeader } from '../components/MainPanel/MainHeader';
import { MainPanel } from '../components/MainPanel/MainPanel';
import { ResourceInspector } from '../components/MainPanel/ResourceInspector';
import { YamlEditor } from '../components/MainPanel/YamlEditor';
import {
  getDefaultGatewayUrl,
  getGatewayClient,
  isBlockedMixedContentGatewayUrl,
  isExpiredSignatureAuthError,
  normalizeGatewayUrl,
  TALON_AUTH_EXPIRED_EVENT,
  updateGatewayClient,
} from '../lib/grpc';
import {
  areSelectionsEqual,
  buildSearchParams,
  selectionFromSearchParams,
  SYSTEM_NAMESPACE,
  type Selection,
} from '../lib/selection';
import {
  channelSubscriptionDocumentFromResource,
  type ResourceEnvelope,
} from '../lib/talon/resourceMappers';
import { useResourceDocument } from '../hooks/useResourceDocument';

const CONNECT_TIMEOUT_MS = 8000;
const RUNTIME_AUTH_TOKEN_STORAGE_KEY = 'talon_auth_token';
const DEPRECATED_ADVANCED_STORAGE_KEYS = ['talon_manual_jwt', 'talon_connection_namespace'];
const CONNECTION_ROOT_QUERY_PARAM = 'root';
const talonObjectApiBaseUrl = (process.env.NEXT_PUBLIC_TALON_OBJECT_API_URL || '').trim().replace(/\/+$/, '');
const imageUploadsEnabled = Boolean(talonObjectApiBaseUrl);
const SIGHTLINE_AUTH_COOKIE_NAME = 'sightline_talon_auth';
const SIGHTLINE_AUTH_COOKIE_DOMAIN = '.impala.systems';
const LABEL_MESSAGE_SOURCE = 'talon.impalasys.com/message-source';
const LABEL_AUTHOR_KIND = 'talon.impalasys.com/author-kind';
const LABEL_AUTHOR = 'talon.impalasys.com/author';
const LABEL_CONNECTOR = 'talon.impalasys.com/connector';
const LABEL_CONNECTOR_CLASS = 'talon.impalasys.com/connector-class';
const LABEL_CONNECTOR_REGISTRATION = 'talon.impalasys.com/connector-registration';
const LABEL_EXTERNAL_CONVERSATION = 'talon.impalasys.com/external-conversation';
const LABEL_EXTERNAL_SENDER = 'talon.impalasys.com/external-sender';
const LABEL_CONVERSATION_TYPE = 'talon.impalasys.com/conversation-type';
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

function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

type SessionConnectorMetadata = {
  connector: string;
  connectorClass: string;
  registration: string;
  externalConversation: string;
  externalSender: string;
  conversationType: string;
};

function normalizeLabels(value: unknown): Record<string, string> {
  if (!value || typeof value !== 'object') return {};
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>)
      .filter((entry): entry is [string, string] => typeof entry[1] === 'string')
  );
}

function connectorMetadataFromLabels(labels: Record<string, string>): SessionConnectorMetadata | null {
  if (!labels[LABEL_CONNECTOR_REGISTRATION] || !labels[LABEL_EXTERNAL_CONVERSATION]) {
    return null;
  }
  return {
    connector: labels[LABEL_CONNECTOR] || '',
    connectorClass: labels[LABEL_CONNECTOR_CLASS] || '',
    registration: labels[LABEL_CONNECTOR_REGISTRATION] || '',
    externalConversation: labels[LABEL_EXTERNAL_CONVERSATION] || '',
    externalSender: labels[LABEL_EXTERNAL_SENDER] || '',
    conversationType: labels[LABEL_CONVERSATION_TYPE] || '',
  };
}

function positiveIntParam(searchParams: URLSearchParams, name: string) {
  const value = searchParams.get(name);
  if (!value || !/^\d+$/.test(value)) return undefined;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : undefined;
}

async function uploadTalonImage({ file, namespace, agent, sessionId, signal }: TalonImageUploadContext) {
  if (!talonObjectApiBaseUrl) {
    throw new Error('Image upload requires VITE_TALON_OBJECT_API_URL.');
  }
  const form = new FormData();
  form.set('file', file);
  form.set('namespace', namespace);
  form.set('agent', agent);
  form.set('sessionId', sessionId);

  const response = await fetch(`${talonObjectApiBaseUrl}/api/talon/objects`, {
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
  return object.key && talonObjectApiBaseUrl
    ? `${talonObjectApiBaseUrl}/api/talon/objects?key=${encodeURIComponent(object.key)}`
    : undefined;
}

function readCookie(name: string) {
  if (typeof document === 'undefined') return null;
  const prefix = `${name}=`;
  return document.cookie
    .split(';')
    .map(part => part.trim())
    .find(part => part.startsWith(prefix))
    ?.slice(prefix.length) || null;
}

function clearCookie(name: string) {
  if (typeof document === 'undefined') return;
  const expires = 'Expires=Thu, 01 Jan 1970 00:00:00 GMT';
  document.cookie = `${name}=; Path=/; ${expires}; SameSite=Lax`;
  document.cookie = `${name}=; Path=/; Domain=${SIGHTLINE_AUTH_COOKIE_DOMAIN}; ${expires}; SameSite=Lax; Secure`;
}

function consumeSightlineAuthCookie() {
  const rawValue = readCookie(SIGHTLINE_AUTH_COOKIE_NAME);
  if (!rawValue) return null;
  clearCookie(SIGHTLINE_AUTH_COOKIE_NAME);
  try {
    return decodeURIComponent(rawValue);
  } catch {
    return rawValue;
  }
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
  const normalized = typeof value === 'bigint' || typeof value === 'string' ? Number(value) : value;
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

function formatConnectionError(error: unknown) {
  if (error instanceof Error && error.message === 'connection-timeout') {
    return 'Could not connect to gateway: the request timed out.';
  }

  if (error instanceof DOMException && error.name === 'AbortError') {
    return 'Could not connect to gateway: the request timed out.';
  }

  const candidate = error as {
    message?: string;
    rawMessage?: string;
    code?: string | number;
    codeName?: string;
    cause?: { message?: string };
  };
  const lowerMessage = `${candidate?.rawMessage || candidate?.message || candidate?.cause?.message || ''}`.toLowerCase();
  if (lowerMessage.includes('signal is aborted') || lowerMessage.includes('aborterror')) {
    return 'Could not connect to gateway: the request timed out.';
  }

  const code = candidate?.codeName || candidate?.code;
  const message =
    candidate?.rawMessage ||
    candidate?.message ||
    candidate?.cause?.message ||
    'The gateway did not respond.';
  return `Could not connect to gateway${code ? ` (${code})` : ''}: ${message}`;
}

function timeoutSignal(timeoutMs: number) {
  const controller = new AbortController();
  const timeoutId = window.setTimeout(() => controller.abort(), timeoutMs);
  return {
    signal: controller.signal,
    abort: () => controller.abort(),
    clear: () => window.clearTimeout(timeoutId),
  };
}

function withConnectionTimeout<T>(promise: Promise<T>, timeoutMs: number, onTimeout: () => void) {
  let timeoutId: number | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timeoutId = window.setTimeout(() => {
      onTimeout();
      reject(new Error('connection-timeout'));
    }, timeoutMs);
  });
  return Promise.race([promise, timeout]).finally(() => {
    if (timeoutId !== undefined) window.clearTimeout(timeoutId);
  });
}

type TalonJwtPayload = {
  exp?: number;
  'talon:ns'?: string;
  ns?: string;
  grants?: Array<{ namespace?: string; ns?: string }>;
  'talon:grants'?: Array<{ namespace?: string; ns?: string }>;
};

function decodeJwtPayload(token: string) {
  const [, payload] = token.split('.');
  if (!payload) return null;
  try {
    const normalized = payload.replace(/-/g, '+').replace(/_/g, '/');
    const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, '=');
    return JSON.parse(window.atob(padded)) as TalonJwtPayload;
  } catch {
    return null;
  }
}

function tokenExpiryError(token: string) {
  const payload = decodeJwtPayload(token);
  if (!payload?.exp) return null;
  const expiresAt = payload.exp * 1000;
  if (!Number.isFinite(expiresAt)) return null;
  if (expiresAt > Date.now()) return null;
  return `Authorization token expired at ${new Date(expiresAt).toLocaleString()}.`;
}

function namespaceFromJwtToken(token: string) {
  const payload = decodeJwtPayload(token);
  const directNamespace = payload?.['talon:ns'] || payload?.ns;
  if (directNamespace?.trim()) return directNamespace.trim();

  const grants = payload?.grants || payload?.['talon:grants'] || [];
  const namespaces = Array.from(new Set(
    grants
      .map((grant) => grant.namespace || grant.ns || '')
      .map((namespace) => namespace.trim())
      .filter(Boolean),
  ));
  return namespaces.length === 1 ? namespaces[0] : '';
}

function namespaceSelection(namespace: string): Selection {
  return {
    type: 'namespace',
    ns: namespace,
    fullPath: namespace,
  };
}

function browserLocationSnapshot() {
  if (typeof window === 'undefined') {
    return { pathname: '/', search: '' };
  }
  return {
    pathname: window.location.pathname || '/',
    search: window.location.search || '',
  };
}

function useBrowserNavigation() {
  const [location, setLocation] = useState(browserLocationSnapshot);

  useEffect(() => {
    const handlePopState = () => setLocation(browserLocationSnapshot());
    window.addEventListener('popstate', handlePopState);
    return () => window.removeEventListener('popstate', handlePopState);
  }, []);

  const navigate = useCallback((url: string, mode: 'push' | 'replace') => {
    const nextUrl = new URL(url || '/', window.location.href);
    const nextPath = `${nextUrl.pathname}${nextUrl.search}${nextUrl.hash}`;
    if (mode === 'push') {
      window.history.pushState(null, '', nextPath);
    } else {
      window.history.replaceState(null, '', nextPath);
    }
    setLocation(browserLocationSnapshot());
  }, []);

  const router = useMemo(
    () => ({
      push: (url: string, _options?: { scroll?: boolean }) => navigate(url, 'push'),
      replace: (url: string, _options?: { scroll?: boolean }) => navigate(url, 'replace'),
    }),
    [navigate],
  );

  const searchParams = useMemo(() => new URLSearchParams(location.search), [location.search]);

  return {
    pathname: location.pathname,
    router,
    searchParams,
  };
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
        <YamlEditor value={resourceYaml} className="min-h-0 flex-1" />
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
  channel,
  resourceYaml,
  onOpenSession,
}: {
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
      const subscriptionsPayload = await getGatewayClient().resources.list({
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
        <YamlEditor value={resourceYaml} className="min-h-0 flex-1" />
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

function DebuggerPageContent() {
  const { router, pathname, searchParams } = useBrowserNavigation();
  const queryClient = useQueryClient();
  const nextHistoryModeRef = useRef<'push' | 'replace'>('replace');
  const explicitConnectRef = useRef(false);
  const [gatewayUrl, setGatewayUrl] = useState('');
  const [authToken, setAuthToken] = useState('');
  const [manualJwtToken, setManualJwtToken] = useState('');
  const [apiKey, setApiKey] = useState('');
  const [googleSsoEnabled, setGoogleSsoEnabled] = useState(false);
  const [googleWebClientId, setGoogleWebClientId] = useState<string | null>(null);
  const [googleSsoError, setGoogleSsoError] = useState<string | null>(null);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [isConnecting, setIsConnecting] = useState(false);
  const [isConnected, setIsConnected] = useState(false);
  const [authScreenOpen, setAuthScreenOpen] = useState(false);
  const [isHoveringConnection, setIsHoveringConnection] = useState(false);
  const [selectedNamespace, setSelectedNamespace] = useState<Selection | null>(null);
  const [activeNamespace, setActiveNamespace] = useState('');
  const [connectionNamespace, setConnectionNamespace] = useState('');
  const [sessionComposerRole, setSessionComposerRole] = useState<'user' | 'assistant'>('user');
  const [sessionConnectorMetadata, setSessionConnectorMetadata] = useState<SessionConnectorMetadata | null>(null);
  const [isSidebarPinned, setIsSidebarPinned] = useState(true);
  const [isSidebarHovered, setIsSidebarHovered] = useState(false);
  const [isMobileSidebarOpen, setIsMobileSidebarOpen] = useState(false);
  const [storageHydrated, setStorageHydrated] = useState(false);
  const lastSyncedQueryRef = useRef<string | null>(null);
  const queryScope = useMemo(
    () => ({
      gatewayUrl: normalizeGatewayUrl(gatewayUrl || getDefaultGatewayUrl()),
      authToken: authToken || null,
    }),
    [authToken, gatewayUrl],
  );
  const resourceQuery = useResourceDocument({
    isConnected,
    scope: queryScope,
    selection: selectedNamespace,
  });
  const resourceYaml = resourceQuery.yaml;
  const resourceDocument = resourceQuery.document;
  const resourceLoading = resourceQuery.isLoading || resourceQuery.isFetching;
  const resourceError = resourceQuery.error
    ? resourceQuery.error instanceof Error
      ? resourceQuery.error.message
      : 'Failed to load resource'
    : null;

  const handleSelectionChange = useCallback(
    (selection: Selection | null, historyMode: 'push' | 'replace' = 'push') => {
      nextHistoryModeRef.current = historyMode;
      setSelectedNamespace(selection);
    },
    []
  );
  const handleMobileExplorerSelect = useCallback(
    (selection: Selection) => {
      handleSelectionChange(selection);
      if (selection.type !== 'namespace') {
        setIsMobileSidebarOpen(false);
      }
    },
    [handleSelectionChange],
  );

  useEffect(() => {
    setSessionComposerRole('user');
    setSessionConnectorMetadata(null);
    if (!isConnected || selectedNamespace?.type !== 'session') return;

    let canceled = false;
    getGatewayClient().sessions.get({
      ns: selectedNamespace.ns,
      agent: selectedNamespace.agent || 'default',
      sessionId: selectedNamespace.sessionId,
      messageLimit: 0,
    }).then((response: any) => {
      if (canceled) return;
      const labels = normalizeLabels(response?.labels);
      setSessionConnectorMetadata(connectorMetadataFromLabels(labels));
    }).catch(() => {
      if (!canceled) {
        setSessionConnectorMetadata(null);
      }
    });

    return () => {
      canceled = true;
    };
  }, [isConnected, selectedNamespace]);

  useEffect(() => {
    const savedUrl = localStorage.getItem('talon_gateway_url');
    const defaultGatewayUrl = getDefaultGatewayUrl();
    if (savedUrl && !isBlockedMixedContentGatewayUrl(savedUrl)) {
      setGatewayUrl(savedUrl);
    } else {
      setGatewayUrl(defaultGatewayUrl);
      if (savedUrl && savedUrl !== defaultGatewayUrl) {
        localStorage.setItem('talon_gateway_url', defaultGatewayUrl);
      }
    }
    localStorage.removeItem('talon_gateway_http_url');
    const cookieToken = consumeSightlineAuthCookie();
    const savedToken = cookieToken || localStorage.getItem(RUNTIME_AUTH_TOKEN_STORAGE_KEY);
    if (savedToken) {
      setAuthToken(savedToken);
      localStorage.setItem(RUNTIME_AUTH_TOKEN_STORAGE_KEY, savedToken);
    }
    if (cookieToken) {
      setManualJwtToken('');
    }
    DEPRECATED_ADVANCED_STORAGE_KEYS.forEach((key) => localStorage.removeItem(key));
    setStorageHydrated(true);
  }, []);

  useEffect(() => {
    if (!storageHydrated) return;

    const timeoutId = window.setTimeout(() => {
      const nextGatewayUrl = gatewayUrl.trim();
      if (nextGatewayUrl) {
        localStorage.setItem('talon_gateway_url', normalizeGatewayUrl(nextGatewayUrl));
      } else {
        localStorage.removeItem('talon_gateway_url');
      }
    }, 300);

    return () => window.clearTimeout(timeoutId);
  }, [gatewayUrl, storageHydrated]);

  useEffect(() => {
    if (!storageHydrated) return;

    const currentParams = new URLSearchParams(searchParams.toString());
    const talonGatewayUrl = currentParams.get('talon_gateway_url');
    const talonGatewayHttpUrl = currentParams.get('talon_gateway_http_url');
    const hasTalonHandoff = Boolean(talonGatewayUrl || talonGatewayHttpUrl);
    if (talonGatewayUrl) {
      setGatewayUrl(talonGatewayUrl);
      localStorage.setItem('talon_gateway_url', talonGatewayUrl);
      updateGatewayClient(talonGatewayUrl);
      setIsConnected(true);
      setAuthScreenOpen(false);
      currentParams.delete('talon_gateway_url');
    }
    if (talonGatewayHttpUrl) {
      const normalizedGatewayUrl = normalizeGatewayUrl(talonGatewayHttpUrl);
      setGatewayUrl(normalizedGatewayUrl);
      localStorage.setItem('talon_gateway_url', normalizedGatewayUrl);
      localStorage.removeItem('talon_gateway_http_url');
      updateGatewayClient(normalizedGatewayUrl);
      setIsConnected(true);
      setAuthScreenOpen(false);
      currentParams.delete('talon_gateway_http_url');
    }
    if (hasTalonHandoff) {
      currentParams.set('connected', 'true');
      const sanitizedQuery = currentParams.toString();
      lastSyncedQueryRef.current = sanitizedQuery;
      router.replace(sanitizedQuery ? `${pathname}?${sanitizedQuery}` : pathname, { scroll: false });
      return;
    }

    const currentQuery = currentParams.toString();
    lastSyncedQueryRef.current = currentQuery;

    const connectionRoot = currentParams.get(CONNECTION_ROOT_QUERY_PARAM)?.trim();
    if (connectionRoot) {
      setConnectionNamespace(connectionRoot);
    }

    // `ns` belongs to explorer selection. It must not hydrate the connection
    // namespace field; use `root` when a link needs to prefill that field.
    const nextSelection = selectionFromSearchParams(currentParams);
    setSelectedNamespace(prev => areSelectionsEqual(prev, nextSelection) ? prev : nextSelection);
    if (nextSelection?.ns) {
      setActiveNamespace((prev) => prev || nextSelection.ns);
    }

    const wantsConnected = searchParams.get('connected') === 'true';
    if (wantsConnected) {
      explicitConnectRef.current = false;
    }
    if (authScreenOpen) {
      return;
    }
    if (wantsConnected && gatewayUrl.trim()) {
      if (isBlockedMixedContentGatewayUrl(gatewayUrl)) {
        setConnectionError('Sightline is running over HTTPS, so the gateway URL must also use HTTPS or the same origin.');
        setIsConnected(false);
        setAuthScreenOpen(true);
        return;
      }
      updateGatewayClient(normalizeGatewayUrl(gatewayUrl));
      setConnectionError(null);
      setIsConnected(true);
      setAuthScreenOpen(false);
      return;
    }

    if (!wantsConnected) {
      if (explicitConnectRef.current) {
        return;
      }
      setIsConnected(false);
      setAuthScreenOpen(true);
    }
  }, [authScreenOpen, storageHydrated, searchParams, gatewayUrl, pathname, router]);

  const effectiveGatewayHttpUrl = gatewayUrl.trim();

  useEffect(() => {
    if (!storageHydrated || !effectiveGatewayHttpUrl.trim()) return;
    if (isBlockedMixedContentGatewayUrl(effectiveGatewayHttpUrl)) {
      setGoogleSsoEnabled(false);
      setGoogleWebClientId(null);
      return;
    }

    let cancelled = false;
    const loadAuthConfig = async () => {
      try {
        updateGatewayClient(normalizeGatewayUrl(effectiveGatewayHttpUrl));
        const config = await getGatewayClient().auth.getSsoConfig({});
        if (!cancelled) {
          setGoogleSsoEnabled(Boolean(config.googleSsoEnabled && config.googleWebClientId));
          setGoogleWebClientId(config.googleWebClientId || null);
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

  useEffect(() => {
    const handleExpiredAuth = () => {
      explicitConnectRef.current = false;
      queryClient.removeQueries({ queryKey: ['talon'] });
      localStorage.removeItem(RUNTIME_AUTH_TOKEN_STORAGE_KEY);
      setAuthToken('');
      setApiKey('');
      setIsConnected(false);
      setAuthScreenOpen(true);
      setConnectionError('Your authorization token expired. Sign in again to continue.');
    };

    window.addEventListener(TALON_AUTH_EXPIRED_EVENT, handleExpiredAuth);
    return () => window.removeEventListener(TALON_AUTH_EXPIRED_EVENT, handleExpiredAuth);
  }, [queryClient]);

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
            const payload = await getGatewayClient().auth.exchangeOidcToken({
              idToken: response.credential,
              clientType: 'sightline',
            });
            setAuthToken(payload.accessToken);
            setManualJwtToken('');
            setApiKey('');
            localStorage.setItem(RUNTIME_AUTH_TOKEN_STORAGE_KEY, payload.accessToken);
            if (gatewayUrl.trim()) {
              if (isBlockedMixedContentGatewayUrl(gatewayUrl)) {
                setConnectionError('Sightline is running over HTTPS, so the gateway URL must also use HTTPS or the same origin.');
                return;
              }
              const normalizedGatewayUrl = normalizeGatewayUrl(gatewayUrl);
              localStorage.setItem('talon_gateway_url', normalizedGatewayUrl);
              updateGatewayClient(normalizedGatewayUrl);
              setConnectionError(null);
              explicitConnectRef.current = true;
              const connectedQuery = buildSearchParams(true, selectedNamespace, searchParams).toString();
              lastSyncedQueryRef.current = connectedQuery;
              router.replace(connectedQuery ? `${pathname}?${connectedQuery}` : pathname, { scroll: false });
              queryClient.removeQueries({ queryKey: ['talon'] });
              setIsConnected(true);
              setAuthScreenOpen(false);
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
  }, [effectiveGatewayHttpUrl, gatewayUrl, googleWebClientId, pathname, queryClient, router, searchParams, selectedNamespace]);

  useEffect(() => {
    if (!storageHydrated) return;

    const wantsConnected = searchParams.get('connected') === 'true';
    const queryHasSelection = searchParams.has('ns');
    if ((wantsConnected && !isConnected && !authScreenOpen) || (queryHasSelection && !selectedNamespace)) {
      return;
    }

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
  }, [authScreenOpen, storageHydrated, isConnected, selectedNamespace, pathname, router, searchParams]);

  const handleConnect = async ({
    gatewayUrl: submittedGatewayUrlValue,
    apiKey: submittedApiKeyValue,
    jwtToken: submittedJwtTokenValue,
    namespace: submittedNamespaceValue,
  }: {
    gatewayUrl: string;
    apiKey: string;
    jwtToken: string;
    namespace: string;
  }) => {
    const submittedGatewayUrl = (submittedGatewayUrlValue || gatewayUrl).trim();
    const submittedApiKey = submittedApiKeyValue.trim();
    const submittedJwtToken = (submittedJwtTokenValue || manualJwtToken).trim();
    const submittedNamespace = submittedNamespaceValue.trim();
    setGatewayUrl(submittedGatewayUrl);
    setApiKey(submittedApiKey);
    setManualJwtToken(submittedJwtToken);
    setConnectionNamespace(submittedNamespace);

    if (submittedGatewayUrl) {
      if (isBlockedMixedContentGatewayUrl(submittedGatewayUrl)) {
        setConnectionError('Sightline is running over HTTPS, so the gateway URL must also use HTTPS or the same origin.');
        setIsConnected(false);
        setAuthScreenOpen(true);
        return;
      }
      const normalizedGatewayUrl = normalizeGatewayUrl(submittedGatewayUrl);
      const previousGatewayUrl = localStorage.getItem('talon_gateway_url');
      const previousAuthToken = localStorage.getItem(RUNTIME_AUTH_TOKEN_STORAGE_KEY);
      const expiredTokenMessage = !submittedApiKey && submittedJwtToken ? tokenExpiryError(submittedJwtToken) : null;
      if (expiredTokenMessage) {
        setConnectionError(expiredTokenMessage);
        setIsConnected(false);
        setAuthScreenOpen(true);
        return;
      }

      setIsConnecting(true);
      setConnectionError(null);

      try {
        localStorage.setItem('talon_gateway_url', normalizedGatewayUrl);
        localStorage.removeItem('talon_gateway_http_url');
        updateGatewayClient(normalizedGatewayUrl);
        let nextAuthToken = submittedJwtToken;
        if (submittedApiKey) {
          localStorage.removeItem(RUNTIME_AUTH_TOKEN_STORAGE_KEY);
        }
        if (submittedApiKey) {
          const exchangeApiKey = async () => {
            try {
              return await getGatewayClient().auth.exchangeApiKey({
                apiKey: submittedApiKey,
              });
            } catch (error) {
              if (!submittedNamespace || !error) throw error;
              const candidate = error as { message?: string; rawMessage?: string };
              const message = `${candidate?.rawMessage || candidate?.message || ''}`.toLowerCase();
              if (!message.includes('grant is required')) throw error;
              try {
                return await getGatewayClient().auth.exchangeApiKey({
                  apiKey: submittedApiKey,
                  grant: { kind: 'readwrite', namespace: submittedNamespace },
                });
              } catch {
                return await getGatewayClient().auth.exchangeApiKey({
                  apiKey: submittedApiKey,
                  grant: { kind: 'read', namespace: submittedNamespace },
                });
              }
            }
          };
          const exchanged = await exchangeApiKey();
          nextAuthToken = exchanged.accessToken;
          setManualJwtToken('');
        }
        if (nextAuthToken) {
          localStorage.setItem(RUNTIME_AUTH_TOKEN_STORAGE_KEY, nextAuthToken);
        } else {
          localStorage.removeItem(RUNTIME_AUTH_TOKEN_STORAGE_KEY);
        }
        const scopedNamespace = submittedNamespace || (nextAuthToken ? namespaceFromJwtToken(nextAuthToken) : '');
        const probeTimeout = timeoutSignal(CONNECT_TIMEOUT_MS);
        try {
          const probe: Promise<unknown> = scopedNamespace
            ? getGatewayClient().namespaces.get({ name: scopedNamespace }, { signal: probeTimeout.signal })
            : getGatewayClient().namespaces.list({ parent: undefined }, { signal: probeTimeout.signal });
          await withConnectionTimeout(
            probe,
            CONNECT_TIMEOUT_MS,
            () => probeTimeout.abort(),
          );
        } finally {
          probeTimeout.clear();
        }

        setGatewayUrl(normalizedGatewayUrl);
        setAuthToken(nextAuthToken);
        setApiKey('');
        setConnectionError(null);
        explicitConnectRef.current = true;
        const nextSelection = scopedNamespace ? namespaceSelection(scopedNamespace) : selectedNamespace;
        if (scopedNamespace) {
          setSelectedNamespace(nextSelection);
          setActiveNamespace(scopedNamespace);
        }
        const connectedQuery = buildSearchParams(true, nextSelection, searchParams).toString();
        lastSyncedQueryRef.current = connectedQuery;
        router.replace(connectedQuery ? `${pathname}?${connectedQuery}` : pathname, { scroll: false });
        queryClient.removeQueries({ queryKey: ['talon'] });
        setIsConnected(true);
        setAuthScreenOpen(false);
      } catch (error) {
        if (previousGatewayUrl) {
          localStorage.setItem('talon_gateway_url', previousGatewayUrl);
          updateGatewayClient(previousGatewayUrl);
        } else {
          localStorage.removeItem('talon_gateway_url');
          updateGatewayClient(getDefaultGatewayUrl());
        }
        if (previousAuthToken) {
          localStorage.setItem(RUNTIME_AUTH_TOKEN_STORAGE_KEY, previousAuthToken);
        } else {
          localStorage.removeItem(RUNTIME_AUTH_TOKEN_STORAGE_KEY);
        }
        if (isExpiredSignatureAuthError(error)) {
          setAuthToken('');
          localStorage.removeItem(RUNTIME_AUTH_TOKEN_STORAGE_KEY);
          setConnectionError('Your authorization token expired. Sign in again to continue.');
        } else {
          setConnectionError(formatConnectionError(error));
        }
        setIsConnected(false);
        setAuthScreenOpen(true);
      } finally {
        setIsConnecting(false);
      }
    }
  };

  if (!storageHydrated) {
    return <div className="sightline-app-viewport bg-background" />;
  }

  if (authScreenOpen || !isConnected) {
    return (
      <ConnectionConfigScreen
        gatewayUrl={gatewayUrl}
        jwtToken={manualJwtToken}
        apiKey={apiKey}
        namespace={connectionNamespace}
        isConnecting={isConnecting}
        googleSsoEnabled={googleSsoEnabled}
        googleSsoError={googleSsoError}
        connectionError={connectionError}
        onGatewayUrlChange={(value) => {
          setGatewayUrl(value);
          setConnectionError(null);
        }}
        onJwtTokenChange={setManualJwtToken}
        onApiKeyChange={setApiKey}
        onNamespaceChange={(value) => {
          setConnectionNamespace(value);
          setConnectionError(null);
        }}
        onGoogleSignIn={handleGoogleSignIn}
        onConnect={handleConnect}
      />
    );
  }

  const selectedSession = selectedNamespace?.type === 'session' ? selectedNamespace : null;

  return (
    <div className="sightline-app-viewport flex min-w-0 flex-row overflow-x-hidden overflow-y-hidden bg-background text-foreground">
      {/* Invisible Hover Zone at Left Edge */}
      {!isSidebarPinned && !isSidebarHovered && (
        <div 
          className="fixed left-0 top-0 bottom-0 w-4 z-50 cursor-e-resize hidden md:block"
          onMouseEnter={() => setIsSidebarHovered(true)}
        />
      )}

      {isMobileSidebarOpen ? (
        <div className="fixed inset-0 z-[70] md:hidden">
          <button
            type="button"
            className="absolute inset-0 bg-slate-950/28"
            onClick={() => setIsMobileSidebarOpen(false)}
            aria-label="Close explorer"
          />
          <motion.div
            initial={{ x: -320 }}
            animate={{ x: 0 }}
            exit={{ x: -320 }}
            transition={{ type: 'spring', stiffness: 320, damping: 34 }}
            className="relative h-full min-h-0 w-[min(21rem,88vw)] overflow-hidden border-r border-slate-200/80 bg-slate-50/95 shadow-2xl dark:border-border/70 dark:bg-background"
          >
            <Explorer
              isConnected={isConnected}
              selectedNode={selectedNamespace}
              activeNamespace={activeNamespace}
              onActiveNamespaceChange={setActiveNamespace}
              onSelect={handleMobileExplorerSelect}
              queryScope={queryScope}
            />
          </motion.div>
        </div>
      ) : null}

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
          "border-r border-slate-200/80 bg-slate-50/95 backdrop-blur-xl hidden md:flex flex-col flex-shrink-0 z-50 h-full group/sidebar overflow-hidden shadow-[0_18px_48px_rgba(0,0,0,0.24)] dark:border-border/70 dark:bg-background/78",
          isSidebarPinned ? "relative shadow-none" : "absolute shadow-2xl"
        )}
        onMouseLeave={() => {
          if (!isSidebarPinned) setIsSidebarHovered(false);
        }}
      >
        <div className="absolute top-3 right-3 z-50 opacity-0 transition-opacity group-hover/sidebar:opacity-100 [@media(pointer:coarse)]:opacity-100">
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
        <div className="w-64 lg:w-72 h-full min-h-0 flex flex-col flex-shrink-0">
          <Explorer
            isConnected={isConnected} 
            selectedNode={selectedNamespace} 
            activeNamespace={activeNamespace}
            onActiveNamespaceChange={setActiveNamespace}
            onSelect={handleSelectionChange}
            queryScope={queryScope}
          />
        </div>
      </motion.div>

      <div className="flex-1 flex min-h-0 flex-col min-w-0 bg-transparent">
        <MainHeader
          isConnected={isConnected}
          selectedNode={selectedNamespace}
          isHoveringConnection={isHoveringConnection}
          onConnectionHoverChange={setIsHoveringConnection}
          onOpenSidebar={() => setIsMobileSidebarOpen(true)}
          onDisconnect={() => {
            explicitConnectRef.current = false;
            queryClient.removeQueries({ queryKey: ['talon'] });
            setIsConnected(false);
            setAuthScreenOpen(true);
            setConnectionError(null);
          }}
          onSelect={handleSelectionChange}
        />

        <MainPanel
          isSessionSelected={Boolean(selectedSession)}
          sessionContent={
            selectedSession ? (
              <div className={cn("flex h-full min-h-0 min-w-0 flex-1 flex-col overflow-hidden transition-opacity duration-300", !isConnected && "opacity-20 pointer-events-none")}>
                {sessionConnectorMetadata ? (
                  <div className="mx-auto mt-3 flex w-[calc(100%-2rem)] max-w-4xl flex-wrap items-center gap-2 rounded-lg border border-emerald-500/20 bg-emerald-500/8 px-3 py-2 text-xs text-emerald-200">
                    <Plug className="h-3.5 w-3.5" />
                    <span className="font-medium">{sessionConnectorMetadata.connectorClass || 'connector'}</span>
                    {sessionConnectorMetadata.connector ? <span className="text-emerald-100/70">{sessionConnectorMetadata.connector}</span> : null}
                    <span className="min-w-0 truncate text-emerald-100/80">{sessionConnectorMetadata.externalConversation}</span>
                    {sessionConnectorMetadata.externalSender ? <span className="text-emerald-100/60">from {sessionConnectorMetadata.externalSender}</span> : null}
                    {sessionConnectorMetadata.conversationType ? <span className="ml-auto rounded-full bg-emerald-400/10 px-2 py-0.5 text-[11px] text-emerald-100/80">{sessionConnectorMetadata.conversationType}</span> : null}
                  </div>
                ) : null}
                <div className="min-h-0 flex-1 overflow-hidden">
                  <TalonCopilot
                    className="h-full"
                    namespace={selectedSession.ns}
                    agent={selectedSession.agent || 'default'}
                    sessionId={selectedSession.sessionId}
                    gatewayClient={getGatewayClient()}
                  historyPageSize={positiveIntParam(searchParams, 'historyPageSize')}
                  enabledBuiltInCommands={['clear']}
                  onImageUpload={sessionComposerRole === 'assistant' || !imageUploadsEnabled ? undefined : uploadTalonImage}
                  objectUrlForRef={imageUploadsEnabled ? talonObjectUrl : undefined}
                  disabled={!isConnected}
                  allowMessageEditing
                  enableDebugMessageEditing={Boolean(sessionConnectorMetadata)}
                  composerVariant="expanded"
                  composerStartAdornment={
                    <div className="flex h-8 overflow-hidden rounded-full border border-border bg-background/80 p-0.5">
                      {(['user', 'assistant'] as const).map((role) => (
                        <button
                          key={role}
                          type="button"
                          className={cn(
                            "rounded-full px-2.5 text-[11px] font-medium capitalize transition-colors",
                            sessionComposerRole === role ? "bg-foreground text-background" : "text-muted-foreground hover:text-foreground"
                          )}
                          onClick={() => setSessionComposerRole(role)}
                        >
                          {role}
                        </button>
                      ))}
                    </div>
                  }
                  onSubmitMessage={async ({ text, imageAttachments, ensureSession, clearInput, refreshSession }) => {
                    if (sessionComposerRole !== 'assistant') return false;
                    if (imageAttachments.length > 0) {
                      throw new Error('Assistant-mode image delivery is not supported yet.');
                    }
                    const session = await ensureSession();
                    const sessions = getGatewayClient().sessions as any;
                    if (!sessions.appendMessage) {
                      throw new Error('Gateway client does not support sessions.appendMessage().');
                    }
                    await sessions.appendMessage({
                      ns: session.ns,
                      agent: session.agent,
                      sessionId: session.sessionId,
                      message: {
                        role: data.MessageRole.ROLE_ASSISTANT,
                        labels: {
                          [LABEL_MESSAGE_SOURCE]: 'sightline',
                          [LABEL_AUTHOR_KIND]: 'human',
                          [LABEL_AUTHOR]: 'sightline',
                        },
                        parts: [{
                          partType: data.SessionMessagePartType.TEXT,
                          content: text,
                        }],
                      },
                    });
                    clearInput();
                    await refreshSession();
                    return true;
                  }}
                  onSessionChange={(nextSessionId) => {
                    handleSelectionChange({
                      type: 'session',
                      ns: selectedSession.ns,
                      agent: selectedSession.agent || 'default',
                      sessionId: nextSessionId,
                      fullPath: `${selectedSession.ns}/${selectedSession.agent || 'default'}/${nextSessionId}`,
                    });
                  }}
                />
              </div>
            </div>
            ) : null
          }
          resourceContent={
            <ResourceInspector
              isConnected={isConnected}
              selectedNode={selectedNamespace}
              isLoading={resourceLoading}
              error={resourceError}
              yaml={resourceYaml}
              dedicatedInspector={
                selectedNamespace?.type === 'schedule' && resourceDocument ? (
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
                ) : selectedNamespace?.type === 'channel' && resourceDocument ? (
                  <ChannelInspector
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
                ) : undefined
              }
            />
          }
        />
      </div>
    </div>
  );
}

export default function DebuggerPage() {
  return (
    <Suspense fallback={<div className="sightline-app-viewport bg-background" />}>
      <DebuggerPageContent />
    </Suspense>
  );
}
