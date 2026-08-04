import { useEffect, useState } from 'react';
import { Eye, EyeOff, Plus, Save, Trash2 } from 'lucide-react';

type SecretResource = {
  apiVersion?: string;
  kind?: string;
  metadata?: {
    name?: string;
    namespace?: string;
    labels?: Record<string, string>;
    annotations?: Record<string, string>;
  };
  spec?: {
    kind?: {
      case?: string;
      value?: {
        type?: string;
        data?: Record<string, string>;
      };
    };
    type?: string;
    data?: Record<string, string>;
  };
};

type SecretEntry = {
  id: number;
  key: string;
  value: string;
  originalValue?: string;
  originalEncoded?: string;
};

function decodeBase64(value: string) {
  try {
    const binary = window.atob(value);
    const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
    return new TextDecoder().decode(bytes);
  } catch {
    return '';
  }
}

export function secretEntriesFromResource(resource: SecretResource): SecretEntry[] {
  const data = resource.spec?.kind?.case === 'secret'
    ? resource.spec.kind.value?.data || {}
    : resource.spec?.data || {};
  return Object.entries(data)
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([key, encoded], index) => {
      const value = decodeBase64(encoded);
      return {
        id: index + 1,
        key,
        value,
        originalValue: value,
        originalEncoded: encoded,
      };
    });
}

export function secretManifestFromEntries(resource: SecretResource, type: string, entries: SecretEntry[]) {
  const data: Record<string, string> = {};
  const stringData: Record<string, string> = {};
  const keys = new Set<string>();

  for (const entry of entries) {
    const key = entry.key.trim();
    if (!key) throw new Error('Secret keys cannot be empty.');
    if (keys.has(key)) throw new Error(`Secret key '${key}' is duplicated.`);
    keys.add(key);

    if (entry.originalEncoded !== undefined && entry.value === entry.originalValue) {
      data[key] = entry.originalEncoded;
    } else {
      stringData[key] = entry.value;
    }
  }

  const metadata = resource.metadata || {};
  const name = metadata.name?.trim() || '';
  const namespace = metadata.namespace?.trim() || '';
  if (!name) throw new Error('Secret metadata.name is required.');
  if (!namespace) throw new Error('Secret metadata.namespace is required.');

  return {
    apiVersion: resource.apiVersion || 'talon.impalasys.com/v1',
    kind: 'Secret',
    metadata: {
      name,
      namespace,
      labels: metadata.labels || {},
      annotations: metadata.annotations || {},
    },
    spec: {
      type: type.trim() || 'Opaque',
      data,
      stringData,
    },
  };
}

export function SecretInspector({
  secret,
  onSave,
}: {
  secret: SecretResource;
  onSave: (manifest: ReturnType<typeof secretManifestFromEntries>) => Promise<void>;
}) {
  const [type, setType] = useState('Opaque');
  const [entries, setEntries] = useState<SecretEntry[]>([]);
  const [visible, setVisible] = useState<Set<number>>(new Set());
  const [nextId, setNextId] = useState(1);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    const spec = secret.spec?.kind?.case === 'secret' ? secret.spec.kind.value || {} : secret.spec || {};
    const nextEntries = secretEntriesFromResource(secret);
    setType(spec.type || 'Opaque');
    setEntries(nextEntries);
    setNextId(nextEntries.length + 1);
    setVisible(new Set());
    setError(null);
    setSaved(false);
  }, [secret]);

  const updateEntry = (id: number, update: Partial<SecretEntry>) => {
    setEntries((current) => current.map((entry) => (entry.id === id ? { ...entry, ...update } : entry)));
    setSaved(false);
  };

  const addEntry = () => {
    const id = nextId;
    setNextId((current) => current + 1);
    setEntries((current) => [...current, { id, key: '', value: '' }]);
    setSaved(false);
  };

  const removeEntry = (id: number) => {
    setEntries((current) => current.filter((entry) => entry.id !== id));
    setVisible((current) => {
      const next = new Set(current);
      next.delete(id);
      return next;
    });
    setSaved(false);
  };

  const save = async () => {
    setIsSaving(true);
    setError(null);
    setSaved(false);
    try {
      await onSave(secretManifestFromEntries(secret, type, entries));
      setSaved(true);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save Secret');
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-2xl border border-border bg-muted/20">
      <div className="flex flex-wrap items-center gap-3 border-b border-border px-4 py-3">
        <div>
          <div className="text-sm font-semibold text-foreground">Secret values</div>
          <div className="mt-0.5 text-xs text-muted-foreground">{secret.metadata?.name || 'Secret'} · values are hidden by default</div>
        </div>
        <label className="ml-auto flex items-center gap-2 text-xs text-muted-foreground">
          Type
          <input
            className="h-8 w-32 rounded-md border border-border bg-background px-2 text-xs text-foreground outline-none focus:ring-2 focus:ring-ring"
            value={type}
            onChange={(event) => {
              setType(event.target.value);
              setSaved(false);
            }}
            aria-label="Secret type"
          />
        </label>
      </div>

      <div className="min-h-0 flex-1 overflow-auto p-4">
        <div className="mb-3 rounded-lg border border-amber-300/50 bg-amber-50/70 p-3 text-xs text-amber-900 dark:border-amber-900/50 dark:bg-amber-950/20 dark:text-amber-200">
          Sightline sends new or changed values as cleartext <code>stringData</code>; unchanged values keep their existing base64 encoding.
        </div>

        <div className="space-y-2">
          {entries.length === 0 ? (
            <div className="rounded-xl border border-dashed border-border p-6 text-center text-sm text-muted-foreground">
              This Secret has no data keys.
            </div>
          ) : entries.map((entry) => {
            const isVisible = visible.has(entry.id);
            return (
              <div key={entry.id} className="grid gap-2 rounded-xl border border-border bg-background/70 p-3 md:grid-cols-[minmax(9rem,0.7fr)_minmax(0,1.5fr)_auto] md:items-center">
                <input
                  className="h-9 rounded-md border border-border bg-background px-2 text-sm text-foreground outline-none focus:ring-2 focus:ring-ring"
                  value={entry.key}
                  onChange={(event) => updateEntry(entry.id, { key: event.target.value })}
                  placeholder="Key"
                  aria-label="Secret key"
                />
                <div className="flex gap-2">
                  {isVisible ? (
                    <textarea
                      className="min-h-20 min-w-0 flex-1 resize-y rounded-md border border-border bg-background px-2 py-2 text-sm text-foreground outline-none focus:ring-2 focus:ring-ring"
                      value={entry.value}
                      onChange={(event) => updateEntry(entry.id, { value: event.target.value })}
                      placeholder="Value"
                      aria-label={`Secret value for ${entry.key || 'new key'}`}
                    />
                  ) : (
                    <div
                      className="flex min-h-9 min-w-0 flex-1 items-center rounded-md border border-border bg-background px-2 text-sm tracking-widest text-muted-foreground"
                      aria-label={`Secret value for ${entry.key || 'new key'}`}
                    >
                      {'•'.repeat(Math.max(8, Math.min(24, entry.value.length || 8)))}
                    </div>
                  )}
                  <button
                    type="button"
                    className="rounded-md border border-border px-2 text-muted-foreground hover:bg-muted hover:text-foreground"
                    onClick={() => setVisible((current) => {
                      const next = new Set(current);
                      if (next.has(entry.id)) next.delete(entry.id); else next.add(entry.id);
                      return next;
                    })}
                    aria-label={isVisible ? `Hide ${entry.key || 'secret'} value` : `Show ${entry.key || 'secret'} value`}
                  >
                    {isVisible ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                  </button>
                </div>
                <button
                  type="button"
                  className="flex h-9 items-center justify-center rounded-md border border-red-200/70 px-2 text-red-600 hover:bg-red-50 dark:border-red-900/50 dark:text-red-400 dark:hover:bg-red-950/30"
                  onClick={() => removeEntry(entry.id)}
                  aria-label={`Remove ${entry.key || 'secret'} key`}
                >
                  <Trash2 className="h-4 w-4" />
                </button>
              </div>
            );
          })}
        </div>

        <button
          type="button"
          className="mt-3 inline-flex items-center gap-1.5 rounded-md border border-border bg-background px-3 py-2 text-xs font-semibold text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={addEntry}
        >
          <Plus className="h-3.5 w-3.5" /> Add key
        </button>
      </div>

      <div className="flex flex-wrap items-center gap-3 border-t border-border px-4 py-3">
        {error ? <div className="text-xs text-red-600 dark:text-red-400">{error}</div> : null}
        {saved ? <div className="text-xs text-emerald-600 dark:text-emerald-400">Secret saved.</div> : null}
        <button
          type="button"
          className="ml-auto inline-flex items-center gap-1.5 rounded-md bg-foreground px-3 py-2 text-xs font-semibold text-background disabled:cursor-not-allowed disabled:opacity-50"
          onClick={save}
          disabled={isSaving}
        >
          <Save className="h-3.5 w-3.5" /> {isSaving ? 'Saving…' : 'Save changes'}
        </button>
      </div>
    </div>
  );
}
