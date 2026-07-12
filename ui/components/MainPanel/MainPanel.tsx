import type { ReactNode } from 'react';

type MainPanelProps = {
  isSessionSelected: boolean;
  sessionContent: ReactNode;
  resourceContent: ReactNode;
};

export function MainPanel({ isSessionSelected, sessionContent, resourceContent }: MainPanelProps) {
  return (
    <main className="flex min-w-0 flex-1 overflow-x-hidden overflow-y-hidden bg-transparent">
      <div className="relative flex min-w-0 flex-1 flex-col overflow-hidden bg-transparent">
        {isSessionSelected ? sessionContent : resourceContent}
      </div>
    </main>
  );
}
