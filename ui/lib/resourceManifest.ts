export type ResourceEnvelope = {
  apiVersion?: string;
  kind?: string;
  metadata?: Record<string, any>;
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

function parseEmbeddedJson(value: unknown): any {
  if (typeof value !== 'string' || value.trim() === '') return {};
  try {
    return JSON.parse(value);
  } catch {
    return value;
  }
}

function isEmptyObject(value: any) {
  return value && typeof value === 'object' && !Array.isArray(value) && Object.keys(value).length === 0;
}

function isDefaultValue(value: any) {
  return (
    value === undefined ||
    value === null ||
    value === '' ||
    value === 0 ||
    (typeof value === 'bigint' && value === BigInt(0)) ||
    value === '0'
  );
}

export function yamlSafeValue(value: any): any {
  if (typeof value === 'bigint') return value.toString();
  if (Array.isArray(value)) return value.map(yamlSafeValue);
  if (value && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value)
        .filter(([key, entryValue]) => key !== '$typeName' && typeof entryValue !== 'undefined')
        .map(([key, entryValue]) => [key, yamlSafeValue(entryValue)]),
    );
  }
  return value;
}

function manifestMetadata(metadata: Record<string, any> | undefined) {
  return {
    name: metadata?.name || '',
    namespace: metadata?.namespace || '',
    labels: metadata?.labels || {},
    annotations: metadata?.annotations || {},
  };
}

function resourceRef(ref: any) {
  if (!ref || typeof ref !== 'object') return undefined;
  return yamlSafeValue(ref);
}

function runtimeTemplate(template: any) {
  return yamlSafeValue(template || {});
}

function enumLabel(value: any, labels: Record<number, string>) {
  if (typeof value === 'number') return labels[value] || 'UNSPECIFIED';
  if (typeof value === 'bigint') return labels[Number(value)] || 'UNSPECIFIED';
  if (typeof value === 'string' && value.trim()) {
    return value.replace(/^FILE_(PURPOSE|INDEX_POLICY|RETENTION)_/, '');
  }
  return 'UNSPECIFIED';
}

function manifestSpec(caseName: string | undefined, value: any) {
  const spec = value || {};

  switch (caseName) {
    case 'file':
      return yamlSafeValue({
        path: spec.path || '',
        mediaType: spec.mediaType || '',
        purpose: enumLabel(spec.purpose, { 1: 'MEMORY', 2: 'ARTIFACT' }),
        indexPolicy: enumLabel(spec.indexPolicy, { 1: 'NONE', 2: 'SEARCH', 3: 'RETRIEVAL' }),
        retention: enumLabel(spec.retention, { 1: 'RETAINED' }),
      });
    case 'template':
      return yamlSafeValue({
        kind: spec.kind || '',
        metadata: manifestMetadata(spec.metadata),
        spec: parseEmbeddedJson(spec.specJson),
      });
    case 'sandboxClass':
      return yamlSafeValue({
        provider: spec.provider || '',
        providerConfig: parseEmbeddedJson(spec.providerConfigJson),
        credentials: parseEmbeddedJson(spec.credentialsJson),
      });
    case 'sandboxPolicy':
      return yamlSafeValue({
        classRef: resourceRef(spec.classRef),
        template: runtimeTemplate(spec.template),
        maxConcurrent: spec.maxConcurrent || 0,
      });
    case 'sandbox':
      return yamlSafeValue({
        policyRef: spec.policyRef || '',
        classRef: resourceRef(spec.classRef),
        runtimeTemplate: runtimeTemplate(spec.runtimeTemplate),
      });
    case 'permissionRequest':
      return yamlSafeValue({
        agent: spec.agent || '',
        sessionId: spec.sessionId || '',
        action: spec.action || '',
        prompt: spec.prompt || '',
        payload: parseEmbeddedJson(spec.payloadJson),
      });
    case 'raw':
      return parseEmbeddedJson(spec.json);
    default:
      return yamlSafeValue(spec);
  }
}

function commonStatus(status: any) {
  const cleaned: Record<string, any> = {};
  if (!isDefaultValue(status?.observedGeneration)) {
    cleaned.observedGeneration = status.observedGeneration;
  }
  if (!isDefaultValue(status?.phase)) {
    cleaned.phase = status.phase;
  }
  if (Array.isArray(status?.conditions) && status.conditions.length > 0) {
    cleaned.conditions = yamlSafeValue(status.conditions);
  }
  return cleaned;
}

function manifestStatus(caseName: string | undefined, value: any) {
  const status = value || {};

  if (caseName === 'raw') {
    return parseEmbeddedJson(status.json);
  }

  const cleaned = commonStatus(status);
  for (const [key, entryValue] of Object.entries(status)) {
    if (
      key === '$typeName' ||
      key === 'observedGeneration' ||
      key === 'phase' ||
      key === 'conditions' ||
      isDefaultValue(entryValue) ||
      (Array.isArray(entryValue) && entryValue.length === 0) ||
      isEmptyObject(entryValue)
    ) {
      continue;
    }
    cleaned[key] = yamlSafeValue(entryValue);
  }
  return cleaned;
}

export function resourceToManifestDocument(resource: ResourceEnvelope) {
  const specCase = resource.spec?.kind?.case;
  const statusCase = resource.status?.kind?.case;
  const spec = manifestSpec(specCase, resource.spec?.kind?.value);
  const status = manifestStatus(statusCase, resource.status?.kind?.value);
  const document: Record<string, any> = {
    apiVersion: resource.apiVersion || '',
    kind: resource.kind || '',
    metadata: manifestMetadata(resource.metadata),
    spec,
  };

  if (!isEmptyObject(status)) {
    document.status = status;
  }

  return yamlSafeValue(document);
}
