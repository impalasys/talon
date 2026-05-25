import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { TalonCopilot } from '@talonai/copilot';

function makeJsonResponse(payload: any, ok = true) {
  return {
    ok,
    json: async () => payload,
  } as any;
}

function makeStreamResponse(lines: string[]) {
  const encoder = new TextEncoder();
  return {
    ok: true,
    body: {
      getReader() {
        let index = 0;
        return {
          async read() {
            if (index >= lines.length) {
              return { done: true, value: undefined };
            }
            const value = encoder.encode(lines[index]);
            index += 1;
            return { done: false, value };
          },
        };
      },
    },
  } as any;
}

describe('TalonCopilot', () => {
  afterEach(() => {
    jest.restoreAllMocks();
  });

  it('renders an injected session history via gatewayClient', async () => {
    const gatewayClient = {
      createSession: jest.fn(),
      listSessionMessages: jest.fn().mockResolvedValue({
        sessionId: 'sess-1',
        state: 'IDLE',
        items: [
          {
            message: {
              id: 'assistant-1',
              role: 'ROLE_ASSISTANT',
              content: 'Hello from history',
              createdAt: String(Date.now() * 1000),
            },
            steps: [],
          },
        ],
        hasMore: false,
      }),
      getSession: jest.fn(),
    };

    render(
      <TalonCopilot
        namespace="ops"
        agent="copilot"
        gatewayUrl="http://localhost:18789"
        gatewayClient={gatewayClient}
        sessionId="sess-1"
      />,
    );

    await waitFor(() => {
      expect(gatewayClient.listSessionMessages).toHaveBeenCalledWith({
        ns: 'ops',
        agent: 'copilot',
        sessionId: 'sess-1',
        pageSize: 50,
        beforeMessageId: undefined,
      });
    });
    expect(await screen.findByText('Hello from history')).toBeInTheDocument();
  });

  it('renders assistant markdown instead of plain text blobs', async () => {
    const gatewayClient = {
      createSession: jest.fn(),
      getSession: jest.fn().mockResolvedValue({
        sessionId: 'sess-md',
        state: 'IDLE',
        messages: [
          {
            id: 'assistant-md',
            role: 'ROLE_ASSISTANT',
            content: '### Tools\n\n- Search\n- Fetch',
            createdAt: String(Date.now() * 1000),
          },
        ],
        steps: [],
      }),
    };

    const { container } = render(
      <TalonCopilot
        namespace="ops"
        agent="copilot"
        gatewayUrl="http://localhost:18789"
        gatewayClient={gatewayClient}
        sessionId="sess-md"
      />,
    );

    await screen.findByText('Tools');
    expect(container.querySelector('h3')).not.toBeNull();
    expect(screen.getByText('Search')).toBeInTheDocument();
    expect(screen.getByText('Fetch')).toBeInTheDocument();
  });

  it('renders a timestamp from a UUID-like message id when createdAt is absent', async () => {
    const gatewayClient = {
      createSession: jest.fn(),
      getSession: jest.fn().mockResolvedValue({
        sessionId: 'sess-ts',
        state: 'IDLE',
        messages: [
          {
            id: '019e33a9-a91f-71f2-96d7-679799caeafc',
            role: 'ROLE_ASSISTANT',
            content: 'Timestamped message',
          },
        ],
        steps: [],
      }),
    };

    render(
      <TalonCopilot
        namespace="ops"
        agent="copilot"
        gatewayUrl="http://localhost:18789"
        gatewayClient={gatewayClient}
        sessionId="sess-ts"
      />,
    );

    await screen.findByText('Timestamped message');
    expect(screen.queryByText('—')).not.toBeInTheDocument();
  });

  it('renders a timestamp from an explicit createdAt epoch-seconds value', async () => {
    const gatewayClient = {
      createSession: jest.fn(),
      getSession: jest.fn().mockResolvedValue({
        sessionId: 'sess-seconds',
        state: 'IDLE',
        messages: [
          {
            id: 'assistant-seconds',
            role: 'ROLE_ASSISTANT',
            content: 'Seconds timestamped message',
            createdAt: '1777755592',
          },
        ],
        steps: [],
      }),
    };

    render(
      <TalonCopilot
        namespace="ops"
        agent="copilot"
        gatewayUrl="http://localhost:18789"
        gatewayClient={gatewayClient}
        sessionId="sess-seconds"
      />,
    );

    await screen.findByText('Seconds timestamped message');
    expect(screen.queryByText('—')).not.toBeInTheDocument();
  });

  it('renders a timestamp from an explicit createdAt bigint value from the Connect client', async () => {
    const gatewayClient = {
      createSession: jest.fn(),
      getSession: jest.fn().mockResolvedValue({
        sessionId: 'sess-bigint',
        state: 'IDLE',
        messages: [
          {
            id: 'assistant-bigint',
            role: 'ROLE_ASSISTANT',
            content: 'Bigint timestamped message',
            createdAt: 1777755592000000n,
          },
        ],
        steps: [],
      }),
    };

    render(
      <TalonCopilot
        namespace="ops"
        agent="copilot"
        gatewayUrl="http://localhost:18789"
        gatewayClient={gatewayClient}
        sessionId="sess-bigint"
      />,
    );

    await screen.findByText('Bigint timestamped message');
    expect(screen.queryByText('—')).not.toBeInTheDocument();
  });

  it('renders a timestamp from a ULID message id when createdAt is absent', async () => {
    const gatewayClient = {
      createSession: jest.fn(),
      getSession: jest.fn().mockResolvedValue({
        sessionId: 'sess-ulid',
        state: 'IDLE',
        messages: [
          {
            id: '01ARZ3NDEKTSV4RRFFQ69G5FAV',
            role: 'ROLE_ASSISTANT',
            content: 'ULID timestamped message',
          },
        ],
        steps: [],
      }),
    };

    render(
      <TalonCopilot
        namespace="ops"
        agent="copilot"
        gatewayUrl="http://localhost:18789"
        gatewayClient={gatewayClient}
        sessionId="sess-ulid"
      />,
    );

    await screen.findByText('ULID timestamped message');
    expect(screen.queryByText('—')).not.toBeInTheDocument();
  });

  it('creates a session with auth headers and streams a reply in internal transport mode', async () => {
    const onSessionChange = jest.fn();
    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();

    fetchMock
      .mockResolvedValueOnce(makeJsonResponse({ sessionId: 'sess-2' }))
      .mockResolvedValueOnce(makeStreamResponse([
        'f:{"messageId":"assistant-2"}\n',
        '0:"The square root of 144 is 12."\n',
      ]))
      .mockResolvedValueOnce(makeJsonResponse({
        sessionId: 'sess-2',
        state: 'IDLE',
        messages: [
          {
            id: 'user-1',
            role: 'ROLE_USER',
            content: 'square root of 144',
            createdAt: String(Date.now() * 1000),
          },
          {
            id: 'assistant-2',
            role: 'ROLE_ASSISTANT',
            content: 'The square root of 144 is 12.',
            createdAt: String(Date.now() * 1000),
          },
        ],
        steps: [],
      }));

    render(
      <TalonCopilot
        namespace="ops"
        agent="copilot"
        gatewayUrl="http://localhost:18789"
        authToken="secret-token"
        onSessionChange={onSessionChange}
      />,
    );

    fireEvent.change(screen.getByPlaceholderText('Ask Talon to perform a task...'), {
      target: { value: 'square root of 144' },
    });
    fireEvent.click(screen.getByRole('button', { name: /send message/i }));

    await waitFor(() => {
      expect(onSessionChange).toHaveBeenCalledWith('sess-2');
    });

    expect(await screen.findByText('The square root of 144 is 12.')).toBeInTheDocument();

    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      'http://localhost:18789/v1/ns/ops/agents/copilot/sessions',
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          Authorization: 'Basic OnNlY3JldC10b2tlbg==',
        }),
      }),
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      'http://localhost:18789/v1/ui/ns/ops/agents/copilot/sessions/sess-2',
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          Authorization: 'Basic OnNlY3JldC10b2tlbg==',
        }),
      }),
    );
    expect(JSON.parse(fetchMock.mock.calls[1][1].body)).toEqual({
      messages: [
        {
          role: 'user',
          content: 'square root of 144',
          parts: [{ type: 'text', text: 'square root of 144' }],
        },
      ],
    });
    expect(fetchMock).toHaveBeenNthCalledWith(
      3,
      'http://localhost:18789/v1/ns/ops/agents/copilot/sessions/sess-2/messages?page_size=50',
      expect.anything(),
    );
  });

  it('scrolls the transcript container as streamed output arrives', async () => {
    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();

    const scrollTo = jest.fn();
    const originalScrollTo = HTMLElement.prototype.scrollTo;
    Object.defineProperty(HTMLElement.prototype, 'scrollTo', {
      configurable: true,
      value: scrollTo,
    });

    fetchMock
      .mockResolvedValueOnce(makeJsonResponse({ sessionId: 'sess-scroll' }))
      .mockResolvedValueOnce(makeStreamResponse([
        'f:{"messageId":"assistant-scroll"}\n',
        '0:"Scrolling text."\n',
      ]))
      .mockResolvedValueOnce(makeJsonResponse({
        sessionId: 'sess-scroll',
        state: 'IDLE',
        messages: [
          {
            id: 'assistant-scroll',
            role: 'ROLE_ASSISTANT',
            content: 'Scrolling text.',
            createdAt: String(Date.now() * 1000),
          },
        ],
        steps: [],
      }));

    render(
      <TalonCopilot
        namespace="ops"
        agent="copilot"
        gatewayUrl="http://localhost:18789"
      />,
    );

    fireEvent.change(screen.getByPlaceholderText('Ask Talon to perform a task...'), {
      target: { value: 'scroll please' },
    });
    fireEvent.click(screen.getByRole('button', { name: /send message/i }));

    expect(await screen.findByText('Scrolling text.')).toBeInTheDocument();
    await waitFor(() => {
      expect(scrollTo).toHaveBeenCalled();
    });

    Object.defineProperty(HTMLElement.prototype, 'scrollTo', {
      configurable: true,
      value: originalScrollTo,
    });
  });

  it('loads older history pages when scrolled near the top', async () => {
    const gatewayClient = {
      createSession: jest.fn(),
      listSessionMessages: jest
        .fn()
        .mockResolvedValueOnce({
          sessionId: 'sess-pages',
          state: 'IDLE',
          items: [
            {
              message: {
                id: '019f0000-0000-7000-8000-000000000002',
                role: 'ROLE_ASSISTANT',
                content: 'Newest page',
                createdAt: String(Date.now() * 1000),
              },
              steps: [],
            },
          ],
          hasMore: true,
          nextBeforeMessageId: '019f0000-0000-7000-8000-000000000002',
        })
        .mockResolvedValueOnce({
          sessionId: 'sess-pages',
          state: 'IDLE',
          items: [
            {
              message: {
                id: '019f0000-0000-7000-8000-000000000001',
                role: 'ROLE_ASSISTANT',
                content: 'Older page',
                createdAt: String(Date.now() * 1000),
              },
              steps: [],
            },
          ],
          hasMore: false,
        }),
      getSession: jest.fn(),
    };

    const { container } = render(
      <TalonCopilot
        namespace="ops"
        agent="copilot"
        gatewayUrl="http://localhost:18789"
        gatewayClient={gatewayClient}
        sessionId="sess-pages"
      />,
    );

    await screen.findByText('Newest page');

    const scrollContainer = container.querySelector('div[style*="overflow-y: auto"]') as HTMLDivElement;
    Object.defineProperty(scrollContainer, 'scrollTop', { configurable: true, value: 0, writable: true });
    Object.defineProperty(scrollContainer, 'scrollHeight', { configurable: true, value: 1000 });
    fireEvent.scroll(scrollContainer);

    expect(await screen.findByText('Older page')).toBeInTheDocument();
    await waitFor(() => {
      expect(gatewayClient.listSessionMessages).toHaveBeenNthCalledWith(2, {
        ns: 'ops',
        agent: 'copilot',
        sessionId: 'sess-pages',
        pageSize: 50,
        beforeMessageId: '019f0000-0000-7000-8000-000000000002',
      });
    });
  });

  it('recovers from an empty live stream by loading the canonical session state', async () => {
    const gatewayClient = {
      createSession: jest.fn().mockResolvedValue({ sessionId: 'sess-recover' }),
      getSession: jest
        .fn()
        .mockResolvedValueOnce({
          sessionId: 'sess-recover',
          state: 'IDLE',
          messages: [],
          steps: [],
        })
        .mockResolvedValueOnce({
          sessionId: 'sess-recover',
          state: 'IDLE',
          messages: [
            {
              id: 'assistant-recover',
              role: 'ROLE_ASSISTANT',
              content: 'Recovered after stream timeout.',
              createdAt: String(Date.now() * 1000),
            },
          ],
          steps: [],
        })
        .mockResolvedValueOnce({
          sessionId: 'sess-recover',
          state: 'IDLE',
          messages: [
            {
              id: 'assistant-recover',
              role: 'ROLE_ASSISTANT',
              content: 'Recovered after stream timeout.',
              createdAt: String(Date.now() * 1000),
            },
          ],
          steps: [],
        }),
    };

    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();
    fetchMock.mockResolvedValueOnce(makeStreamResponse([]));

    render(
      <TalonCopilot
        namespace="ops"
        agent="copilot"
        gatewayUrl="http://localhost:18789"
        gatewayClient={gatewayClient}
      />,
    );

    fireEvent.change(screen.getByPlaceholderText('Ask Talon to perform a task...'), {
      target: { value: 'recover this' },
    });
    fireEvent.click(screen.getByRole('button', { name: /send message/i }));

    expect(await screen.findByText('Recovered after stream timeout.')).toBeInTheDocument();
    expect(screen.getByText('recover this')).toBeInTheDocument();
    expect(screen.queryByText(/system incident/i)).not.toBeInTheDocument();
  });
});
