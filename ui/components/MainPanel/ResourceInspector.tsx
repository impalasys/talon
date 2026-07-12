import type { ReactNode } from 'react';
import { FileText } from 'lucide-react';
import type { Selection } from '../../lib/selection';
import { YamlEditor } from './YamlEditor';

type ResourceInspectorProps = {
  isConnected: boolean;
  selectedNode: Selection | null;
  isLoading: boolean;
  error: string | null;
  yaml: string;
  dedicatedInspector?: ReactNode;
};

export function ResourceInspector({
  isConnected,
  selectedNode,
  isLoading,
  error,
  yaml,
  dedicatedInspector,
}: ResourceInspectorProps) {
  return (
    <div className={`min-h-0 flex-1 overflow-hidden transition-opacity duration-300 ${!isConnected ? 'pointer-events-none opacity-20' : ''}`}>
      <div className="flex h-full min-h-0 w-full flex-col">
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
        ) : dedicatedInspector ? (
          dedicatedInspector
        ) : (
          <div className="min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
            <YamlEditor value={yaml} className="h-full min-h-0" />
          </div>
        )}
      </div>
    </div>
  );
}
