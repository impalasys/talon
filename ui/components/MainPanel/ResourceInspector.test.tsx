import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { ResourceInspector } from './ResourceInspector';
import type { Selection } from '../../lib/selection';

const getObject = jest.fn();

jest.mock('../../lib/grpc', () => ({
  getGatewayClient: () => ({
    cas: {
      getObject,
    },
  }),
}));

jest.mock('./YamlEditor', () => ({
  YamlEditor: ({ value }: { value: string }) => <textarea aria-label="yaml" readOnly value={value} />,
}));

jest.mock('./MarkdownEditor', () => ({
  MarkdownEditor: ({ value }: { value: string }) => <textarea aria-label="markdown" readOnly value={value} />,
}));

function renderInspector(document: any) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });
  const selectedNode: Selection = {
    type: 'file',
    ns: 'image-tool-output-test',
    resourceName: 'images-thinking-collapsed-png',
    fullPath: 'image-tool-output-test:file:images-thinking-collapsed-png',
  };

  return render(
    <QueryClientProvider client={queryClient}>
      <ResourceInspector
        isConnected
        selectedNode={selectedNode}
        isLoading={false}
        error={null}
        document={document}
        yaml="kind: File"
      />
    </QueryClientProvider>,
  );
}

describe('ResourceInspector', () => {
  beforeEach(() => {
    getObject.mockReset();
    Object.defineProperty(URL, 'createObjectURL', {
      configurable: true,
      value: jest.fn(() => 'blob:preview-image'),
    });
    Object.defineProperty(URL, 'revokeObjectURL', {
      configurable: true,
      value: jest.fn(),
    });
  });

  it('renders image files in the inspector view', async () => {
    getObject.mockResolvedValue({
      data: new Uint8Array([137, 80, 78, 71]),
      mediaType: 'image/png',
      filename: 'thinking-collapsed.png',
    });

    renderInspector({
      kind: 'File',
      metadata: { name: 'images-thinking-collapsed-png' },
      spec: {
        path: '/images/thinking-collapsed.png',
        mediaType: 'image/png',
      },
      status: {
        objectRef: {
          key: 'cas/image-tool-output-test/files/file-id/sha',
          mediaType: 'image/png',
          filename: 'thinking-collapsed.png',
          sizeBytes: 79267,
        },
      },
    });

    fireEvent.click(screen.getByRole('button', { name: 'Inspector' }));

    await waitFor(() => {
      expect(getObject).toHaveBeenCalledWith({ key: 'cas/image-tool-output-test/files/file-id/sha' });
    });
    const image = await screen.findByRole('img', { name: 'thinking-collapsed.png' });
    expect(image).toHaveAttribute('src', 'blob:preview-image');
    expect(screen.getByText('image/png · 79,267 bytes')).toBeInTheDocument();
  });
});
