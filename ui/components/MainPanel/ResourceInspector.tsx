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

type FileDescriptor =
  | {
      kind: 'text';
      inlineContent?: string;
      objectKey: string;
      language: 'markdown' | 'text';
      mediaType: string;
      filename: string;
      sizeBytes: number;
    }
  | {
      kind: 'image';
      objectKey: string;
      mediaType: string;
      filename: string;
      sizeBytes: number;
    };

function mediaTypeBase(mediaType: string) {
  return mediaType.split(';')[0]?.trim().toLowerCase() || '';
}

function objectRefSizeBytes(objectRef: any) {
  const value = field(objectRef, 'sizeBytes', 'size_bytes');
  if (typeof value === 'bigint') return Number(value);
  if (typeof value === 'number') return value;
  if (typeof value === 'string') return Number(value) || 0;
  return 0;
}

function fileDescriptor(document: any): FileDescriptor | null {
  if (document?.kind !== 'File') return null;
  const spec = document.spec || {};
  const status = document.status || {};
  const objectRef = field(status, 'objectRef', 'object_ref');

  const mediaType = mediaTypeBase(String(spec.mediaType || spec.mimeType || spec.contentType || ''));
  const objectMediaType = mediaTypeBase(String(field(objectRef, 'mediaType', 'media_type') || ''));
  const effectiveMediaType = mediaType || objectMediaType;
  const rawPath = String(spec.path || field(objectRef, 'filename') || document.metadata?.name || '');
  const path = rawPath.toLowerCase();
  const filename = String(field(objectRef, 'filename') || rawPath.split('/').filter(Boolean).pop() || document.metadata?.name || 'file');
  const objectKey = String(field(objectRef, 'key') || '');
  const sizeBytes = objectRefSizeBytes(objectRef);
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
  const isImage = effectiveMediaType.startsWith('image/');
  if (!isText && !isImage) return null;

  if (isImage) {
    return {
      kind: 'image',
      objectKey,
      mediaType: effectiveMediaType || 'image/*',
      filename,
      sizeBytes,
    };
  }

  return {
    kind: 'text',
    inlineContent: typeof spec.content === 'string' ? spec.content : undefined,
    objectKey,
    language: isMarkdown ? 'markdown' as const : 'text' as const,
    mediaType: effectiveMediaType || (isMarkdown ? 'text/markdown' : 'text/plain'),
    filename,
    sizeBytes,
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
  const decoded = await decodeCasObjectBytes(response);
  return new TextDecoder().decode(decoded);
}

async function decodeCasObjectBytes(response: any) {
  const bytes = await casObjectData(response);
  const encoding = String(response?.contentEncoding ?? response?.content_encoding ?? response?.metadata?.content_encoding ?? '').toLowerCase();
  return (
    encoding === 'zstd'
      ? await decompressZstdCasObjectData(bytes)
      : encoding === 'gzip'
        ? await decompressCasObjectData(bytes, 'gzip')
        : bytes
  );
}

function FileContentInspector({ document }: { document: any }) {
  const file = useMemo(() => fileDescriptor(document), [document]);
  const [imageBlobUrl, setImageBlobUrl] = useState<string | null>(null);
  const inlineContentVersion = file?.objectKey
    ? ''
    : String(field(document?.metadata, 'resourceVersion', 'resource_version') || field(document?.metadata, 'generation') || '');
  const contentQuery = useQuery({
    queryKey: ['file-content', file?.objectKey || '', inlineContentVersion],
    queryFn: async () => {
      if (file?.kind === 'image') {
        if (!file.objectKey) return { kind: 'image' as const, src: '', bytes: null };
        const response = await getGatewayClient().cas.getObject({ key: file.objectKey });
        const casResponse: any = response;
        const signedUrl = typeof casResponse?.signedUrl === 'string'
          ? casResponse.signedUrl
          : typeof casResponse?.signed_url === 'string'
            ? casResponse.signed_url
            : '';
        if (signedUrl) return { kind: 'image' as const, src: signedUrl, bytes: null };
        return { kind: 'image' as const, src: '', bytes: await decodeCasObjectBytes(response) };
      }
      if (typeof file?.inlineContent === 'string') return file.inlineContent;
      if (!file?.objectKey) return '';
      const response = await getGatewayClient().cas.getObject({ key: file.objectKey });
      return decodeCasObjectText(response);
    },
    enabled: Boolean(file && ((file.kind === 'text' && typeof file.inlineContent === 'string') || file.objectKey)),
  });

  useEffect(() => {
    if (file?.kind !== 'image') {
      setImageBlobUrl(null);
      return;
    }
    const data = contentQuery.data;
    if (!data || typeof data === 'string' || data.kind !== 'image' || !data.bytes?.byteLength) {
      setImageBlobUrl(null);
      return;
    }
    const copy = new Uint8Array(data.bytes.byteLength);
    copy.set(data.bytes);
    const url = URL.createObjectURL(new Blob([copy.buffer], { type: file.mediaType || 'application/octet-stream' }));
    setImageBlobUrl(url);
    return () => {
      URL.revokeObjectURL(url);
    };
  }, [contentQuery.data, file]);

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

  if (file.kind === 'image') {
    const data = contentQuery.data;
    const imageSrc = data && typeof data !== 'string' && data.kind === 'image'
      ? data.src || imageBlobUrl
      : imageBlobUrl;
    return (
      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-background">
        <div className="flex items-center justify-between gap-4 border-b border-border/70 px-5 py-3">
          <div className="min-w-0">
            <div className="truncate text-sm font-semibold text-foreground">{file.filename}</div>
            <div className="mt-0.5 text-xs text-muted-foreground">
              {file.mediaType}{file.sizeBytes ? ` · ${file.sizeBytes.toLocaleString()} bytes` : ''}
            </div>
          </div>
          {imageSrc ? (
            <a
              href={imageSrc}
              target="_blank"
              rel="noreferrer"
              download={file.filename}
              className="shrink-0 rounded-md border border-border px-3 py-1.5 text-xs font-semibold text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            >
              Open
            </a>
          ) : null}
        </div>
        <div className="flex min-h-0 flex-1 items-center justify-center overflow-auto bg-muted/20 p-6">
          {imageSrc ? (
            <img
              src={imageSrc}
              alt={file.filename}
              className="max-h-full max-w-full rounded-lg border border-border bg-background object-contain shadow-sm"
            />
          ) : (
            <div className="text-sm text-muted-foreground">Image bytes are unavailable.</div>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <MarkdownEditor
        value={typeof contentQuery.data === 'string' ? contentQuery.data : ''}
        language={file.language}
        className="h-full min-h-0"
      />
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

  const fileInspectorDescriptor = selectedNode?.type === 'file' && document ? fileDescriptor(document) : null;
  const inspector =
    dedicatedInspector ||
    (fileInspectorDescriptor ? <FileContentInspector document={document} /> : null);
  const canToggle = Boolean(selectedNode && !isLoading && !error && yaml && inspector);

  useEffect(() => {
    if (fileInspectorDescriptor?.kind === 'image') {
      setMode('inspector');
    }
  }, [fileInspectorDescriptor?.kind, selectedNode?.fullPath]);

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
