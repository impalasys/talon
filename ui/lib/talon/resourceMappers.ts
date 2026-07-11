export type ResourceEnvelope = {
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

export type ExplorerChannel = {
  name?: string;
  ns?: string;
  title?: string;
  status?: string;
  updatedAt?: bigint | number | string;
  updated_at?: bigint | number | string;
  labels?: Record<string, string>;
};

export type ExplorerChannelSubscription = {
  name?: string;
  ns?: string;
  channel?: string;
  agent?: string;
  enabled?: boolean;
  trigger?: string;
  replyMode?: string;
  reply_mode?: string;
};

export type ExplorerSchedule = {
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

export type ExplorerKnowledge = {
  metadata?: ResourceEnvelope['metadata'];
  spec?: {
    path?: string;
    content?: string;
  };
};

export type ChannelDocument = {
  name?: string;
  ns?: string;
  title?: string;
  status?: string;
  metadata?: Record<string, string>;
  labels?: Record<string, string>;
};

export type ChannelSubscriptionDocument = {
  name?: string;
  ns?: string;
  channel?: string;
  agent?: string;
  enabled?: boolean;
  trigger?: string;
  replyMode?: string;
  contextPolicy?: any;
};

export type ScheduleDocument = {
  name?: string;
  ns?: string;
  labels?: Record<string, string>;
  spec?: any;
  status?: any;
};

export function resourceSpec(resource: ResourceEnvelope | undefined, caseName: string) {
  return resource?.spec?.kind?.case === caseName ? resource.spec.kind.value || {} : {};
}

export function resourceStatus(resource: ResourceEnvelope | undefined, caseName: string) {
  return resource?.status?.kind?.case === caseName ? resource.status.kind.value || {} : {};
}

export function resourcePhase(resource: ResourceEnvelope, caseName: string) {
  return resourceStatus(resource, caseName).phase || '';
}

export function parseJsonObject(value: string | undefined) {
  if (!value) return {};
  try {
    const parsed = JSON.parse(value);
    return parsed && typeof parsed === 'object' && !Array.isArray(parsed) ? parsed : {};
  } catch {
    return {};
  }
}

function renderSafeValue(value: any): any {
  if (typeof value === 'bigint') return value.toString();
  if (Array.isArray(value)) return value.map(renderSafeValue);
  if (value && typeof value === 'object') {
    if (
      ArrayBuffer.isView(value) ||
      value instanceof ArrayBuffer ||
      value instanceof Date ||
      value instanceof RegExp ||
      value instanceof Map ||
      value instanceof Set ||
      value instanceof WeakMap ||
      value instanceof WeakSet
    ) {
      return value;
    }
    return Object.fromEntries(
      Object.entries(value)
        .filter(([key, entryValue]) => key !== '$typeName' && typeof entryValue !== 'undefined')
        .map(([key, entryValue]) => [key, renderSafeValue(entryValue)]),
    );
  }
  return value;
}

export function resourceMetadata(name: string, namespace: string) {
  return {
    name,
    namespace,
    labels: {},
    annotations: {},
    ownerReferences: [],
    finalizers: [],
    generation: BigInt(0),
    resourceVersion: '',
    uid: '',
  };
}

export function namespaceLabel(labels?: Record<string, string>) {
  return labels?.workspace_name || labels?.workspace || labels?.display_name || labels?.name;
}

export function channelFromResource(resource: ResourceEnvelope): ExplorerChannel {
  const spec = resourceSpec(resource, 'channel');
  const status = resourceStatus(resource, 'channel');
  return {
    name: resource.metadata?.name,
    ns: resource.metadata?.namespace,
    title: spec.title,
    status: status.phase,
    updatedAt: status.updatedAt,
    labels: resource.metadata?.labels,
  };
}

export function channelDocumentFromResource(resource: ResourceEnvelope): ChannelDocument {
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

export function channelSubscriptionFromResource(resource: ResourceEnvelope): ExplorerChannelSubscription {
  const spec = resourceSpec(resource, 'channelSubscription');
  return {
    name: resource.metadata?.name,
    ns: resource.metadata?.namespace,
    channel: spec.channel,
    agent: spec.agent,
    enabled: spec.enabled,
    trigger: spec.trigger,
    replyMode: spec.replyMode,
  };
}

export function channelSubscriptionDocumentFromResource(resource: ResourceEnvelope): ChannelSubscriptionDocument {
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

export function scheduleFromResource(resource: ResourceEnvelope): ExplorerSchedule {
  return {
    name: resource.metadata?.name,
    ns: resource.metadata?.namespace,
    labels: resource.metadata?.labels,
    spec: resourceSpec(resource, 'schedule'),
    status: resourceStatus(resource, 'schedule'),
  };
}

export function scheduleDocumentFromResource(resource: ResourceEnvelope): ScheduleDocument {
  return renderSafeValue({
    name: resource.metadata?.name,
    ns: resource.metadata?.namespace,
    labels: resource.metadata?.labels,
    spec: resourceSpec(resource, 'schedule'),
    status: resourceStatus(resource, 'schedule'),
  });
}

export function knowledgeFromResource(resource: ResourceEnvelope): ExplorerKnowledge {
  return {
    metadata: resource.metadata,
    spec: resourceSpec(resource, 'knowledge'),
  };
}

export function templateSummary(resource: ResourceEnvelope) {
  const spec = resourceSpec(resource, 'template');
  const targetKind = spec.kind || 'Resource';
  const targetName = spec.metadata?.name || 'unnamed';
  const targetSpec = parseJsonObject(spec.specJson);
  const prompt = typeof targetSpec.systemPrompt === 'string' ? targetSpec.systemPrompt.trim() : '';
  return prompt ? `${targetKind}/${targetName}: ${prompt.slice(0, 120)}` : `${targetKind}/${targetName}`;
}

export function isV2ResourceDocument(document: any): document is ResourceEnvelope {
  return Boolean(
    document &&
      typeof document === 'object' &&
      typeof document.apiVersion === 'string' &&
      typeof document.kind === 'string' &&
      document.metadata &&
      document.spec?.kind?.case,
  );
}
