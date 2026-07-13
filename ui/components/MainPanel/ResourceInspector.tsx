import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { decompress as decompressZstd } from 'fzstd';
import { FileText } from 'lucide-react';
import type { Selection } from '../../lib/selection';
import { getGatewayClient } from '../../lib/grpc';
import { cn } from '../../utils/cn';
import { MarkdownEditor } from './MarkdownEditor';
import { YamlEditor } from './YamlEditor';

type ResourceInspectorProps = {
  isConnected: boolean;
  selectedNode: Selection | null;
  isLoading: boolean;
  error: string | null;
  document?: any;
  yaml: string;
  dedicatedInspector?: ReactNode;
};

type InspectorMode = 'yaml' | 'inspector';

function field(value: any, camelName: string, snakeName: string = camelName) {
  return value?.[camelName] ?? value?.[snakeName];
}

function fileDescriptor(document: any) {
  if (document?.kind !== 'File') return null;
  const spec = document.spec || {};
  const status = document.status || {};
  const objectRef = field(status, 'objectRef', 'object_ref');

  const mediaType = String(spec.mediaType || spec.mimeType || spec.contentType || '').toLowerCase();
  const objectMediaType = String(field(objectRef, 'mediaType', 'media_type') || '').toLowerCase();
  const effectiveMediaType = mediaType || objectMediaType;
  const path = String(spec.path || field(objectRef, 'filename') || document.metadata?.name || '').toLowerCase();
  const isMarkdown =
    effectiveMediaType.includes('markdown') ||
    effectiveMediaType === 'text/md' ||
    path.endsWith('.md') ||
    path.endsWith('.markdown') ||
    path.endsWith('.mdx');
  const isText =
    isMarkdown ||
    effectiveMediaType.startsWith('text/') ||
    effectiveMediaType === 'application/json' ||
    effectiveMediaType.endsWith('+json');
  if (!isText) return null;

  return {
    inlineContent: typeof spec.content === 'string' ? spec.content : undefined,
    objectKey: String(field(objectRef, 'key') || ''),
    language: isMarkdown ? 'markdown' as const : 'text' as const,
    mediaType: effectiveMediaType || (isMarkdown ? 'text/markdown' : 'text/plain'),
  };
}

async function decompressCasObjectData(data: Uint8Array, encoding: string): Promise<Uint8Array> {
  if (typeof DecompressionStream === 'undefined') {
    throw new Error(`${encoding} CAS object requires DecompressionStream support`);
  }
  const stream = new Blob([data as unknown as BlobPart]).stream().pipeThrough(new DecompressionStream(encoding as any));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

async function decompressZstdCasObjectData(data: Uint8Array): Promise<Uint8Array> {
  if (typeof DecompressionStream !== 'undefined') {
    try {
      return await decompressCasObjectData(data, 'zstd');
    } catch (err) {
      if (!(err instanceof TypeError)) throw err;
    }
  }
  return decompressZstd(data);
}

async function casObjectData(response: any): Promise<Uint8Array> {
  const signedUrl = typeof response?.signedUrl === 'string'
    ? response.signedUrl
    : typeof response?.signed_url === 'string'
      ? response.signed_url
      : '';
  if (signedUrl) {
    const fetched = await fetch(signedUrl);
    if (!fetched.ok) throw new Error(`Failed to fetch CAS object: HTTP ${fetched.status}`);
    return new Uint8Array(await fetched.arrayBuffer());
  }
  return response.data ?? new Uint8Array();
}

async function decodeCasObjectText(response: any) {
  const bytes = await casObjectData(response);
  const encoding = String(response?.contentEncoding ?? response?.content_encoding ?? response?.metadata?.content_encoding ?? '').toLowerCase();
  const decoded =
    encoding === 'zstd'
      ? await decompressZstdCasObjectData(bytes)
      : encoding === 'gzip'
        ? await decompressCasObjectData(bytes, 'gzip')
        : bytes;
  return new TextDecoder().decode(decoded);
}

function FileContentInspector({ document }: { document: any }) {
  const file = useMemo(() => fileDescriptor(document), [document]);
  const inlineContentVersion = file?.objectKey
    ? ''
    : String(field(document?.metadata, 'resourceVersion', 'resource_version') || field(document?.metadata, 'generation') || '');
  const contentQuery = useQuery({
    queryKey: ['file-content', file?.objectKey || '', inlineContentVersion],
    queryFn: async () => {
      if (typeof file?.inlineContent === 'string') return file.inlineContent;
      if (!file?.objectKey) return '';
      const response = await getGatewayClient().cas.getObject({ key: file.objectKey });
      return decodeCasObjectText(response);
    },
    enabled: Boolean(file && (typeof file.inlineContent === 'string' || file.objectKey)),
  });
  if (!file) return null;

  if (contentQuery.isLoading) {
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center bg-background text-sm text-muted-foreground">
        Loading file...
      </div>
    );
  }

  if (contentQuery.error) {
    return (
      <div className="m-4 rounded-lg border border-red-200/60 bg-red-50/60 p-4 text-sm text-red-700 dark:border-red-900/40 dark:bg-red-950/20 dark:text-red-400">
        {contentQuery.error instanceof Error ? contentQuery.error.message : 'Failed to load file content'}
      </div>
    );
  }

  return (
    <div className="min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <MarkdownEditor value={contentQuery.data || ''} language={file.language} className="h-full min-h-0" />
    </div>
  );
}

function ViewToggle({ mode, onModeChange }: { mode: InspectorMode; onModeChange: (mode: InspectorMode) => void }) {
  return (
    <div className="pointer-events-auto absolute bottom-5 left-1/2 z-20 -translate-x-1/2 rounded-full border border-border/80 bg-background/90 p-1 shadow-lg shadow-slate-950/10 backdrop-blur-xl">
      {(['yaml', 'inspector'] as const).map((nextMode) => (
        <button
          key={nextMode}
          type="button"
          className={cn(
            'h-8 rounded-full px-4 text-xs font-semibold capitalize transition-colors',
            mode === nextMode ? 'bg-foreground text-background' : 'text-muted-foreground hover:bg-muted hover:text-foreground',
          )}
          onClick={() => onModeChange(nextMode)}
        >
          {nextMode === 'yaml' ? 'YAML' : 'Inspector'}
        </button>
      ))}
    </div>
  );
}

export function ResourceInspector({
  isConnected,
  selectedNode,
  isLoading,
  error,
  document,
  yaml,
  dedicatedInspector,
}: ResourceInspectorProps) {
  const [mode, setMode] = useState<InspectorMode>('yaml');

  useEffect(() => {
    setMode('yaml');
  }, [selectedNode?.fullPath]);

  const inspector =
    dedicatedInspector ||
    (selectedNode?.type === 'file' && document && fileDescriptor(document) ? <FileContentInspector document={document} /> : null);
  const canToggle = Boolean(selectedNode && !isLoading && !error && yaml && inspector);

  return (
    <div className={`min-h-0 flex-1 overflow-hidden transition-opacity duration-300 ${!isConnected ? 'pointer-events-none opacity-20' : ''}`}>
      <div className="relative flex h-full min-h-0 w-full flex-col">
        {!selectedNode ? (
          <div className="m-4 flex flex-1 items-center justify-center rounded-2xl border border-dashed border-border bg-muted/20 md:m-6">
            <div className="text-center">
              <FileText className="mx-auto h-5 w-5 text-muted-foreground" />
              <div className="mt-3 text-sm font-medium text-foreground">No resource selected</div>
              <div className="mt-1 text-sm text-muted-foreground">Choose something from the explorer to inspect it.</div>
            </div>
          </div>
        ) : isLoading ? (
          <div className="m-4 flex flex-1 items-center justify-center rounded-2xl border border-border bg-muted/20 md:m-6">
            <div className="text-sm text-muted-foreground">Loading resource...</div>
          </div>
        ) : error ? (
          <div className="m-4 rounded-2xl border border-red-200/60 bg-red-50/60 p-4 text-sm text-red-700 dark:border-red-900/40 dark:bg-red-950/20 dark:text-red-400 md:m-6">
            {error}
          </div>
        ) : mode === 'inspector' && inspector ? (
          inspector
        ) : (
          <div className="min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
            <YamlEditor value={yaml} className="h-full min-h-0" />
          </div>
        )}
        {canToggle ? <ViewToggle mode={mode} onModeChange={setMode} /> : null}
      </div>
    </div>
  );
}
