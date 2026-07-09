import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import {
  TalonChannel as RawTalonChannel,
  TalonCopilot as RawTalonCopilot,
} from '@impalasys/talon-chat';

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

function makeControllableStreamResponse() {
  const encoder = new TextEncoder();
  let releaseNextRead: ((line: string | null) => void) | null = null;
  const releasedLines: Array<string | null> = [];

  return {
    response: {
      ok: true,
      body: {
        getReader() {
          return {
            async read() {
              const next = releasedLines.shift();
              if (next !== undefined) {
                return next === null
                  ? { done: true, value: undefined }
                  : { done: false, value: encoder.encode(next) };
              }
              return new Promise((resolve) => {
                releaseNextRead = (line) => {
                  resolve(
                    line === null
                      ? { done: true, value: undefined }
                      : { done: false, value: encoder.encode(line) },
                  );
                };
              });
            },
            cancel: jest.fn().mockResolvedValue(undefined),
          };
        },
      },
    } as any,
    release(line: string | null) {
      if (releaseNextRead) {
        const release = releaseNextRead;
        releaseNextRead = null;
        release(line);
      } else {
        releasedLines.push(line);
      }
    },
  };
}

async function* streamResponseToSessionEvents(response: any) {
  const reader = response.body?.getReader?.();
  if (!reader) return;
  const decoder = new TextDecoder();
  let buffer = '';
  let messageId = '';

  while (true) {
    const { value, done } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    while (true) {
      const newlineIndex = buffer.indexOf('\n');
      if (newlineIndex < 0) break;
      const line = buffer.slice(0, newlineIndex);
      buffer = buffer.slice(newlineIndex + 1);
      if (!line) continue;
      const separatorIndex = line.indexOf(':');
      if (separatorIndex < 0) continue;
      const code = line.slice(0, separatorIndex);
      const payload = JSON.parse(line.slice(separatorIndex + 1));
      if (code === 'f' && typeof payload?.messageId === 'string') {
        messageId = payload.messageId;
      } else if (code === '0') {
        yield {
          kind: 'SESSION_MESSAGE_PART_EVENT_KIND_DELTA',
          messageId,
          part: {
            partType: 'SESSION_MESSAGE_PART_TYPE_TEXT',
            content: String(payload),
          },
        };
      } else if (code === 'g') {
        yield {
          kind: 'SESSION_MESSAGE_PART_EVENT_KIND_DELTA',
          messageId,
          part: {
            partType: 'SESSION_MESSAGE_PART_TYPE_REASONING',
            content: String(payload),
          },
        };
      } else if (code === '9') {
        yield {
          kind: 'SESSION_MESSAGE_PART_EVENT_KIND_DELTA',
          messageId,
          part: {
            partType: 'SESSION_MESSAGE_PART_TYPE_TOOL_CALL',
            id: payload?.toolCallId,
            name: payload?.toolName,
            payloadJson: JSON.stringify({ input: payload?.args, tool_call_id: payload?.toolCallId }),
          },
        };
      } else if (code === 'a') {
        yield {
          kind: 'SESSION_MESSAGE_PART_EVENT_KIND_DELTA',
          messageId,
          part: {
            partType: 'SESSION_MESSAGE_PART_TYPE_TOOL_RESULT',
            id: payload?.toolCallId,
            payloadJson: JSON.stringify({ output: payload?.result, tool_call_id: payload?.toolCallId }),
          },
        };
      } else if (code === 'h') {
        yield {
          kind: 'SESSION_MESSAGE_PART_EVENT_KIND_DELTA',
          messageId,
          part: {
            partType: 'SESSION_MESSAGE_PART_TYPE_USAGE',
            payloadJson: JSON.stringify(payload),
          },
        };
      } else if (code === '3') {
        yield {
          kind: 'SESSION_MESSAGE_PART_EVENT_KIND_ERROR',
          messageId,
          part: { partType: 'SESSION_MESSAGE_PART_TYPE_ERROR', content: String(payload) },
        };
      }
    }
  }
}

function makeGatewayClient(raw: any = {}, gatewayUrl = 'http://localhost:18789', authToken?: string | null) {
  if (raw?.sessions || raw?.channels) return raw;
  const fetcher = global.fetch as jest.Mock;
  const headers = authToken ? { Authorization: `Bearer ${authToken}` } : {};
  const sessions = {
    create: raw.createSession ?? jest.fn(async (request: any) => {
      const response = await fetcher(`${gatewayUrl}/v1/ns/${request.ns}/agents/${request.agent}/sessions`, {
        method: 'POST',
        headers,
        body: JSON.stringify(request),
      });
      return response.json();
    }),
    clear: raw.clearSession ?? jest.fn(async (request: any) => {
      const response = await fetcher(`${gatewayUrl}/v1/ns/${request.ns}/agents/${request.agent}/sessions/${request.sessionId}:clear`, {
        method: 'POST',
      });
      return response.json();
    }),
    listMessages: raw.listSessionMessages ?? raw.getSession ?? jest.fn(async (request: any) => {
      const before = request.beforeMessageId ? `&before_message_id=${encodeURIComponent(request.beforeMessageId)}` : '';
      const response = await fetcher(`${gatewayUrl}/v1/ns/${request.ns}/agents/${request.agent}/sessions/${request.sessionId}/messages?page_size=${request.pageSize}${before}`, { headers });
      return response.json();
    }),
    get: raw.getSession ?? jest.fn(async (request: any) => {
      const response = await fetcher(`${gatewayUrl}/v1/ns/${request.ns}/agents/${request.agent}/sessions/${request.sessionId}`, expect.anything());
      return response.json();
    }),
    appendMessage: raw.appendMessage ?? jest.fn(async (request: any) => {
      return { sessionId: request.sessionId, message: request.message };
    }),
    updateMessage: raw.updateMessage ?? jest.fn(async (request: any) => {
      return {
        sessionId: request.sessionId,
        message: {
          id: request.messageId,
          role: 'ROLE_ASSISTANT',
          parts: request.parts,
          labels: request.labels,
          createdAt: String(Date.now() * 1000),
        },
      };
    }),
    submitTurn: raw.submitTurn ?? jest.fn(async function* (request: any, options?: any) {
      const bodyParts = (request.message?.parts ?? []).map((part: any) => {
        const partType = part?.partType ?? part?.part_type;
        if (partType === 7 || partType === 'SESSION_MESSAGE_PART_TYPE_IMAGE') {
          return {
            type: 'image',
            payloadJson: part.payloadJson ?? part.payload_json ?? '',
            object: part.object ?? part.objectRef ?? part.object_ref,
          };
        }
        return { type: 'text', text: part?.content ?? '' };
      });
      const response = await fetcher(`${gatewayUrl}/v1/ui/ns/${request.ns}/agents/${request.agent}/sessions/${request.sessionId}`, {
        method: 'POST',
        headers,
        body: JSON.stringify({
          messages: [{ role: 'user', parts: bodyParts }],
        }),
        signal: options?.signal,
      });
      yield* streamResponseToSessionEvents(response);
    }),
    streamParts: raw.streamParts ?? jest.fn(async function* (request: any, options?: any) {
      const response = await fetcher(`${gatewayUrl}/v1/ui/ns/${request.ns}/agents/${request.agent}/sessions/${request.sessionId}`, {
        signal: options?.signal,
      });
      yield* streamResponseToSessionEvents(response);
    }),
    stopGeneration: raw.stopGeneration ?? jest.fn(async () => ({ success: true })),
  };
  const channels = {
    listMessages: raw.listChannelMessages ?? jest.fn(async (request: any) => {
      const before = request.beforeMessageId ? `&before_message_id=${encodeURIComponent(request.beforeMessageId)}` : '';
      const response = await fetcher(`${gatewayUrl}/v1/ns/${request.ns}/channels/${request.channel}/messages?page_size=${request.pageSize ?? request.limit ?? 100}${before}`, { headers });
      return response.json();
    }),
    postMessage: raw.postChannelMessage ?? jest.fn(async (request: any) => {
      const body: any = {
        ns: request.ns,
        channel: request.channel,
        authorKind: request.authorKind,
        author: request.author,
        content: request.content,
      };
      if (request.subscriptionNames?.length) body.subscriptionNames = request.subscriptionNames;
      if (request.labels && Object.keys(request.labels).length > 0) body.labels = request.labels;
      const response = await fetcher(`${gatewayUrl}/v1/ns/${request.ns}/channels/${request.channel}/messages`, {
        method: 'POST',
        headers,
        body: JSON.stringify(body),
      });
      if (!response.ok) {
        throw new Error(`Post HTTP ${response.status}`);
      }
      return response.json();
    }),
  };
  return { sessions, channels };
}

function TalonCopilot(props: any) {
  return (
    <RawTalonCopilot
      {...props}
      gatewayClient={makeGatewayClient(props.gatewayClient, props.gatewayUrl, props.authToken)}
    />
  );
}

function TalonChannel(props: any) {
  return (
    <RawTalonChannel
      {...props}
      gatewayClient={makeGatewayClient(props.gatewayClient, props.gatewayUrl, props.authToken)}
    />
  );
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
              parts: [
                {
                  partType: 1,
                  content: 'Hello from history',
                },
              ],
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

  it('shows pending connector replies and requests delivery by updating message labels', async () => {
    const updateMessage = jest.fn(async (request: any) => ({
      sessionId: request.sessionId,
      message: {
        id: request.messageId,
        role: 'ROLE_ASSISTANT',
        parts: request.parts,
        labels: {
          ...request.labels,
          'talon.impalasys.com/connector-delivery-status': 'delivered',
        },
        createdAt: String(Date.now() * 1000),
      },
    }));
    const gatewayClient = {
      createSession: jest.fn(),
      updateMessage,
      listSessionMessages: jest.fn().mockResolvedValue({
        sessionId: 'sess-review',
        state: 'IDLE',
        items: [
          {
            message: {
              id: 'assistant-review',
              role: 'ROLE_ASSISTANT',
              labels: {
                'talon.impalasys.com/connector-delivery-status': 'pending_review',
                'talon.impalasys.com/connector': 'slack-main',
              },
              parts: [
                {
                  partType: 1,
                  content: 'Draft reply',
                },
              ],
              createdAt: String(Date.now() * 1000),
            },
            steps: [],
          },
        ],
        hasMore: false,
      }),
    };

    render(
      <TalonCopilot
        namespace="ops"
        agent="copilot"
        gatewayUrl="http://localhost:18789"
        gatewayClient={gatewayClient}
        sessionId="sess-review"
        enableDebugMessageEditing
      />,
    );

    expect(await screen.findByText('Pending send')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Send'));
    await waitFor(() => expect(updateMessage).toHaveBeenCalled());
    expect(updateMessage).toHaveBeenCalledWith(expect.objectContaining({
      ns: 'ops',
      agent: 'copilot',
      sessionId: 'sess-review',
      messageId: 'assistant-review',
      labels: expect.objectContaining({
        'talon.impalasys.com/connector-delivery-status': 'delivery_requested',
        'talon.impalasys.com/connector': 'slack-main',
      }),
    }));
  });

  it('renders finalized work as typed timeline events instead of muting assistant text', async () => {
    const gatewayClient = {
      createSession: jest.fn(),
      listSessionMessages: jest.fn().mockResolvedValue({
        sessionId: 'sess-work',
        state: 'IDLE',
        items: [
          {
            message: {
              id: 'assistant-work',
              role: 'ROLE_ASSISTANT',
              parts: [
                {
                  partType: 'SESSION_MESSAGE_PART_TYPE_REASONING',
                  content: 'I should inspect the available tools first. ',
                },
                {
                  partType: 'SESSION_MESSAGE_PART_TYPE_TEXT',
                  content: "I'll work on",
                },
                {
                  partType: 'SESSION_MESSAGE_PART_TYPE_USAGE',
                  payloadJson: JSON.stringify({
                    reasoning_tokens: 141,
                    output_tokens: 333,
                    input_tokens: 726,
                    total_tokens: 1059,
                  }),
                },
                {
                  partType: 'SESSION_MESSAGE_PART_TYPE_TEXT',
                  content: ' retrieving that email.',
                },
                {
                  partType: 'SESSION_MESSAGE_PART_TYPE_TOOL_CALL',
                  toolCallId: 'call-1',
                  toolName: 'knowledge_search',
                  payloadJson: JSON.stringify({
                    tool_call_id: 'call-1',
                    input: { query: 'inspection report' },
                  }),
                },
                {
                  partType: 'SESSION_MESSAGE_PART_TYPE_TOOL_RESULT',
                  toolCallId: 'call-1',
                  toolName: 'knowledge_search',
                  payloadJson: JSON.stringify({
                    tool_call_id: 'call-1',
                    output: { matches: 0 },
                  }),
                },
                {
                  partType: 'SESSION_MESSAGE_PART_TYPE_TEXT',
                  content: 'Final answer.',
                },
              ],
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
        sessionId="sess-work"
      />,
    );

    expect(await screen.findByText('Final answer.')).toBeInTheDocument();
    expect(screen.queryByText("I'll work on")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /Worked/ }));

    expect(await screen.findByText("I'll work on retrieving that email.")).toBeInTheDocument();
    expect(screen.getByText('I should inspect the available tools first.')).toBeInTheDocument();
    expect(screen.getByText(/Called/)).toHaveTextContent('knowledge_search');
    expect(screen.getAllByText('141 reasoning • 333 output • 726 input • 1059 total')).toHaveLength(1);
  });

  it('hides message edit controls by default', async () => {
    const gatewayClient = {
      createSession: jest.fn(),
      listSessionMessages: jest.fn().mockResolvedValue({
        sessionId: 'sess-edit-default',
        state: 'IDLE',
        items: [
          {
            message: {
              id: 'user-edit-default',
              role: 'ROLE_USER',
              content: 'Original user message',
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
        sessionId="sess-edit-default"
      />,
    );

    expect(await screen.findByText('Original user message')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /edit user message/i })).not.toBeInTheDocument();
  });

  it('allows editing user and assistant session messages when enabled', async () => {
    const onMessageEdit = jest.fn();
    const writeText = jest.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    });
    const gatewayClient = {
      createSession: jest.fn(),
      listSessionMessages: jest.fn().mockResolvedValue({
        sessionId: 'sess-edit',
        state: 'IDLE',
        items: [
          {
            message: {
              id: 'user-edit',
              role: 'ROLE_USER',
              parts: [
                {
                  partType: 'SESSION_MESSAGE_PART_TYPE_TEXT',
                  content: 'Original user message',
                },
              ],
              createdAt: String(Date.now() * 1000),
            },
            steps: [],
          },
          {
            message: {
              id: 'assistant-edit',
              role: 'ROLE_ASSISTANT',
              content: 'Original assistant message',
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
        sessionId="sess-edit"
        allowMessageEditing
        onMessageEdit={onMessageEdit}
      />,
    );

    expect(await screen.findByText('Original user message')).toBeInTheDocument();
    expect(await screen.findByText('Original assistant message')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /edit user message/i })).toHaveClass('talon-session-edit-trigger');
    expect(screen.getByRole('button', { name: /edit assistant message/i })).toHaveClass('talon-session-edit-trigger');
    expect(screen.getByRole('button', { name: /edit user message/i })).toHaveClass('talon-session-message-action-button');
    expect(screen.getByRole('button', { name: /copy user message/i })).toHaveClass('talon-session-message-action-button');
    expect(screen.getByRole('button', { name: /edit user message/i }).closest('.talon-session-message-actions')).not.toBeNull();
    expect(screen.getByRole('button', { name: /copy user message/i }).closest('.talon-session-message-actions')).not.toBeNull();
    expect(document.querySelector('.talon-session-message-action-time')).not.toBeNull();

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: /copy assistant message/i }));
    });
    expect(writeText).toHaveBeenCalledWith('Original assistant message');

    fireEvent.click(screen.getByRole('button', { name: /edit user message/i }));
    fireEvent.change(screen.getByLabelText('Edit message'), {
      target: { value: 'Edited user message' },
    });
    expect(screen.getByLabelText('Edit message')).toHaveClass('talon-session-edit-textarea');
    expect(screen.getByRole('button', { name: /save message edit/i })).toHaveClass('talon-session-edit-action');
    expect(screen.getByRole('button', { name: /cancel message edit/i })).toHaveClass('talon-session-edit-action');
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: /save message edit/i }));
    });

    expect(await screen.findByText('Edited user message')).toBeInTheDocument();
    expect(screen.queryByText('Original user message')).not.toBeInTheDocument();
    expect(onMessageEdit).toHaveBeenCalledWith(expect.objectContaining({
      message: expect.objectContaining({ id: 'user-edit', role: 'user' }),
      nextContent: 'Edited user message',
      namespace: 'ops',
      agent: 'copilot',
      sessionId: 'sess-edit',
    }));

    fireEvent.click(screen.getByRole('button', { name: /edit assistant message/i }));
    fireEvent.change(screen.getByLabelText('Edit message'), {
      target: { value: 'Edited assistant **message**' },
    });
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: /save message edit/i }));
    });

    expect(await screen.findByText('Edited assistant **message**')).toBeInTheDocument();
    expect(screen.queryByText('Original assistant message')).not.toBeInTheDocument();
    expect(onMessageEdit).toHaveBeenLastCalledWith(expect.objectContaining({
      message: expect.objectContaining({ id: 'assistant-edit', role: 'assistant' }),
      nextContent: 'Edited assistant **message**',
      namespace: 'ops',
      agent: 'copilot',
      sessionId: 'sess-edit',
    }));
  });

  it('renders reloaded image object refs from session history', async () => {
    const gatewayClient = {
      createSession: jest.fn(),
      listSessionMessages: jest.fn().mockResolvedValue({
        sessionId: 'sess-image-history',
        state: 'IDLE',
        items: [
          {
            message: {
              id: 'user-image-history',
              role: 'ROLE_USER',
              parts: [
                {
                  partType: 'SESSION_MESSAGE_PART_TYPE_IMAGE',
                  payloadJson: JSON.stringify({ filename: 'chart.png' }),
                  objectRef: {
                    key: 'sessions/sess-image-history/uploads/chart.png',
                    mediaType: 'image/png',
                    sizeBytes: 42,
                    sha256: 'sha',
                    filename: 'chart.png',
                    metadata: {},
                  },
                },
              ],
              createdAt: String(Date.now() * 1000),
            },
            steps: [],
          },
        ],
        hasMore: false,
      }),
      getSession: jest.fn(),
    };
    const objectUrlForRef = jest.fn((object) => `/api/talon/objects?key=${encodeURIComponent(object.key)}`);

    render(
      <TalonCopilot
        namespace="ops"
        agent="copilot"
        gatewayUrl="http://localhost:18789"
        gatewayClient={gatewayClient}
        sessionId="sess-image-history"
        objectUrlForRef={objectUrlForRef}
      />,
    );

    const image = await screen.findByAltText('chart.png') as HTMLImageElement;
    expect(image.getAttribute('src')).toBe('/api/talon/objects?key=sessions%2Fsess-image-history%2Fuploads%2Fchart.png');
    expect(objectUrlForRef).toHaveBeenCalledWith(expect.objectContaining({
      key: 'sessions/sess-image-history/uploads/chart.png',
    }));
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
          Authorization: 'Bearer secret-token',
        }),
      }),
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      'http://localhost:18789/v1/ui/ns/ops/agents/copilot/sessions/sess-2',
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          Authorization: 'Bearer secret-token',
        }),
      }),
    );
    expect(JSON.parse(fetchMock.mock.calls[1][1].body)).toEqual({
      messages: [
        {
          role: 'user',
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

  it('uploads selected images and sends object refs as session message parts', async () => {
    Object.defineProperty(URL, 'createObjectURL', {
      configurable: true,
      value: jest.fn(),
    });
    Object.defineProperty(URL, 'revokeObjectURL', {
      configurable: true,
      value: jest.fn(),
    });
    const createObjectURL = jest.spyOn(URL, 'createObjectURL').mockReturnValue('blob:preview-photo');
    jest.spyOn(URL, 'revokeObjectURL').mockImplementation(() => undefined);
    const onImageUpload = jest.fn().mockResolvedValue({
      key: 'sessions/sess-img/uploads/photo.png',
      mediaType: 'image/png',
      sizeBytes: 12,
      sha256: 'sha',
      filename: 'photo.png',
      metadata: { width: '1', height: '1' },
    });
    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();

    fetchMock
      .mockResolvedValueOnce(makeJsonResponse({ sessionId: 'sess-img' }))
      .mockResolvedValueOnce(makeStreamResponse([
        'f:{"messageId":"assistant-img"}\n',
        '0:"That is a tiny image."\n',
      ]))
      .mockResolvedValueOnce(makeJsonResponse({
        sessionId: 'sess-img',
        state: 'IDLE',
        messages: [],
        steps: [],
      }));

    const { container } = render(
      <TalonCopilot
        namespace="ops"
        agent="copilot"
        gatewayUrl="http://localhost:18789"
        onImageUpload={onImageUpload}
      />,
    );

    const file = new File([new Uint8Array([1, 2, 3])], 'photo.png', { type: 'image/png' });
    const fileInput = container.querySelector('input[type="file"]') as HTMLInputElement;
    fireEvent.change(fileInput, { target: { files: [file] } });
    expect(createObjectURL).toHaveBeenCalledWith(file);
    expect(screen.getByAltText('photo.png')).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText('Ask Talon to perform a task...'), {
      target: { value: 'what is this?' },
    });
    fireEvent.click(screen.getByRole('button', { name: /send message/i }));

    await waitFor(() => expect(onImageUpload).toHaveBeenCalledWith(expect.objectContaining({
      file,
      namespace: 'ops',
      agent: 'copilot',
      sessionId: 'sess-img',
      signal: expect.any(AbortSignal),
    })));

    await screen.findByText('That is a tiny image.');
    const body = JSON.parse(fetchMock.mock.calls[1][1].body);
    expect(body.messages[0].parts).toEqual([
      { type: 'text', text: 'what is this?' },
      expect.objectContaining({
        type: 'image',
        payloadJson: JSON.stringify({ filename: 'photo.png' }),
        object: {
          key: 'sessions/sess-img/uploads/photo.png',
          mediaType: 'image/png',
          sizeBytes: 12,
          sha256: 'sha',
          filename: 'photo.png',
          metadata: { width: '1', height: '1' },
        },
      }),
    ]);
    expect(body.messages[0].parts[1].previewUrl).toBeUndefined();
  });

  it('marks selected images as errored when upload fails', async () => {
    Object.defineProperty(URL, 'createObjectURL', {
      configurable: true,
      value: jest.fn(),
    });
    Object.defineProperty(URL, 'revokeObjectURL', {
      configurable: true,
      value: jest.fn(),
    });
    jest.spyOn(URL, 'createObjectURL').mockReturnValue('blob:failed-photo');
    jest.spyOn(URL, 'revokeObjectURL').mockImplementation(() => undefined);

    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();
    const uploadError = new Error('upload rejected');
    const onImageUpload = jest.fn().mockRejectedValue(uploadError);
    const gatewayClient = {
      createSession: jest.fn().mockResolvedValue({ sessionId: 'sess-upload-fail' }),
      listSessionMessages: jest.fn().mockRejectedValue(new Error('no canonical recovery')),
      getSession: jest.fn(),
    };

    const { container } = render(
      <TalonCopilot
        namespace="ops"
        agent="copilot"
        gatewayUrl="http://localhost:18789"
        gatewayClient={gatewayClient}
        onImageUpload={onImageUpload}
      />,
    );

    const file = new File([new Uint8Array([1, 2, 3])], 'broken.png', { type: 'image/png' });
    const fileInput = container.querySelector('input[type="file"]') as HTMLInputElement;
    fireEvent.change(fileInput, { target: { files: [file] } });
    expect(screen.getByAltText('broken.png')).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText('Ask Talon to perform a task...'), {
      target: { value: 'what is this?' },
    });
    fireEvent.click(screen.getByRole('button', { name: /send message/i }));

    await waitFor(() => expect(onImageUpload).toHaveBeenCalledWith(expect.objectContaining({
      file,
      namespace: 'ops',
      agent: 'copilot',
      sessionId: 'sess-upload-fail',
      signal: expect.any(AbortSignal),
    })));
    expect(await screen.findByText('upload rejected')).toBeInTheDocument();
    expect(screen.getByTitle('upload rejected')).toBeInTheDocument();
    expect(container.querySelector('[aria-label="Uploading image"]')).toBeNull();
    expect(gatewayClient.createSession).toHaveBeenCalledTimes(1);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('runs the built-in clear command without sending it as a session message', async () => {
    const gatewayClient = {
      createSession: jest.fn(),
      clearSession: jest.fn().mockResolvedValue({ success: true }),
      listSessionMessages: jest.fn().mockResolvedValue({
        sessionId: 'sess-clear',
        state: 'IDLE',
        messages: [
          {
            id: 'assistant-clear',
            role: 'ROLE_ASSISTANT',
            content: 'Clear me from the transcript',
            createdAt: String(Date.now() * 1000),
          },
        ],
        steps: [],
      }),
      getSession: jest.fn(),
    };

    render(
      <TalonCopilot
        namespace="ops"
        agent="copilot"
        gatewayUrl="http://localhost:18789"
        gatewayClient={gatewayClient}
        sessionId="sess-clear"
        enabledBuiltInCommands={['clear']}
      />,
    );

    expect(await screen.findByText('Clear me from the transcript')).toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText('Ask Talon to perform a task...'), {
      target: { value: '/clear' },
    });
    fireEvent.click(screen.getByRole('button', { name: /send message/i }));

    await waitFor(() => expect(gatewayClient.clearSession).toHaveBeenCalledWith({
      ns: 'ops',
      agent: 'copilot',
      sessionId: 'sess-clear',
    }));
    expect(screen.queryByText('Clear me from the transcript')).not.toBeInTheDocument();
    expect(gatewayClient.createSession).not.toHaveBeenCalled();
  });

  it('shows enabled session commands in the command menu', async () => {
    const gatewayClient = {
      createSession: jest.fn(),
      clearSession: jest.fn(),
      listSessionMessages: jest.fn().mockResolvedValue({
        sessionId: 'sess-menu',
        state: 'IDLE',
        messages: [],
        steps: [],
      }),
      getSession: jest.fn(),
    };

    render(
      <TalonCopilot
        namespace="ops"
        agent="copilot"
        gatewayUrl="http://localhost:18789"
        gatewayClient={gatewayClient}
        sessionId="sess-menu"
        enabledBuiltInCommands={['clear']}
      />,
    );

    const input = await screen.findByPlaceholderText('Ask Talon to perform a task...');
    fireEvent.change(input, {
      target: { value: '/' },
    });

    expect(screen.getByRole('listbox', { name: 'Command menu' })).toBeInTheDocument();
    const clearOption = screen.getByRole('option', { name: /\/clear/i });
    expect(clearOption).toBeInTheDocument();
    fireEvent.click(clearOption);
    expect(input).toHaveValue('/clear');
  });

  it('runs custom session commands with parsed arguments and context', async () => {
    const commandRun = jest.fn();

    render(
      <TalonCopilot
        namespace="ops"
        agent="copilot"
        gatewayUrl="http://localhost:18789"
        commands={[
          {
            name: 'tag',
            aliases: ['t'],
            run: commandRun,
          },
        ]}
      />,
    );

    fireEvent.change(screen.getByPlaceholderText('Ask Talon to perform a task...'), {
      target: { value: '/t alpha beta' },
    });
    fireEvent.click(screen.getByRole('button', { name: /send message/i }));

    await waitFor(() => expect(commandRun).toHaveBeenCalled());
    expect(commandRun).toHaveBeenCalledWith(expect.objectContaining({
      name: 't',
      input: '/t alpha beta',
      args: 'alpha beta',
      argv: ['alpha', 'beta'],
      target: {
        type: 'session',
        namespace: 'ops',
        agent: 'copilot',
        sessionId: null,
      },
      messages: [],
    }));
  });

  it('renders composer adornments and allows custom submit handling', async () => {
    const appendMessage = jest.fn().mockResolvedValue({ sessionId: 'sess-custom' });
    const submitTurn = jest.fn(async function* () {});
    const gatewayClient = {
      sessions: {
        create: jest.fn(),
        clear: jest.fn(),
        appendMessage,
        listMessages: jest.fn().mockResolvedValue({
          sessionId: 'sess-custom',
          state: 'IDLE',
          items: [],
        }),
        submitTurn,
        streamParts: jest.fn(async function* () {}),
        stopGeneration: jest.fn(),
      },
    };

    render(
      <TalonCopilot
        namespace="ops"
        agent="copilot"
        gatewayClient={gatewayClient}
        sessionId="sess-custom"
        composerStartAdornment={<button type="button">Assistant</button>}
        onSubmitMessage={async ({ text, ensureSession, clearInput }) => {
          const session = await ensureSession();
          await appendMessage({
            ns: session.ns,
            agent: session.agent,
            sessionId: session.sessionId,
            message: {
              role: 2,
              parts: [{ partType: 1, content: text }],
            },
          });
          clearInput();
          return true;
        }}
      />,
    );

    expect(await screen.findByRole('button', { name: 'Assistant' })).toBeInTheDocument();
    const input = screen.getByPlaceholderText('Ask Talon to perform a task...');
    fireEvent.change(input, { target: { value: 'human reply' } });
    fireEvent.click(screen.getByRole('button', { name: /send message/i }));

    await waitFor(() => expect(appendMessage).toHaveBeenCalled());
    expect(appendMessage).toHaveBeenCalledWith(expect.objectContaining({
      ns: 'ops',
      agent: 'copilot',
      sessionId: 'sess-custom',
      message: expect.objectContaining({
        role: 2,
        parts: [expect.objectContaining({ content: 'human reply' })],
      }),
    }));
    expect(submitTurn).not.toHaveBeenCalled();
    expect(input).toHaveValue('');
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

  it('does not scroll the transcript down for new tokens after the user scrolls up', async () => {
    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();

    const scrollTo = jest.fn();
    const originalScrollTo = HTMLElement.prototype.scrollTo;
    Object.defineProperty(HTMLElement.prototype, 'scrollTo', {
      configurable: true,
      value: scrollTo,
    });

    const stream = makeControllableStreamResponse();
    fetchMock
      .mockResolvedValueOnce(makeJsonResponse({ sessionId: 'sess-scroll-lock' }))
      .mockResolvedValueOnce(stream.response)
      .mockResolvedValueOnce(makeJsonResponse({
        sessionId: 'sess-scroll-lock',
        state: 'IDLE',
        messages: [
          {
            id: 'assistant-scroll-lock',
            role: 'ROLE_ASSISTANT',
            content: 'New token.',
            createdAt: String(Date.now() * 1000),
          },
        ],
        steps: [],
      }));

    const { container } = render(
      <TalonCopilot
        namespace="ops"
        agent="copilot"
        gatewayUrl="http://localhost:18789"
      />,
    );

    fireEvent.change(screen.getByPlaceholderText('Ask Talon to perform a task...'), {
      target: { value: 'please stream' },
    });
    fireEvent.click(screen.getByRole('button', { name: /send message/i }));

    stream.release('f:{"messageId":"assistant-scroll-lock"}\n');
    await waitFor(() => expect(scrollTo).toHaveBeenCalled());
    scrollTo.mockClear();

    const scrollContainer = container.querySelector('div[style*="overflow-y: auto"]') as HTMLDivElement;
    Object.defineProperty(scrollContainer, 'scrollTop', { configurable: true, value: 100, writable: true });
    Object.defineProperty(scrollContainer, 'scrollHeight', { configurable: true, value: 1000 });
    Object.defineProperty(scrollContainer, 'clientHeight', { configurable: true, value: 200 });
    fireEvent.scroll(scrollContainer);

    stream.release('0:"New token."\n');
    expect(await screen.findByText('New token.')).toBeInTheDocument();
    await act(async () => {
      await Promise.resolve();
    });
    expect(scrollTo).not.toHaveBeenCalled();

    stream.release(null);

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
    expect(screen.queryByText('recover this')).not.toBeInTheDocument();
    expect(screen.queryByText(/system incident/i)).not.toBeInTheDocument();
  });

  it('aborts an existing session resume stream before sending a new message', async () => {
    const resumeStream = makeControllableStreamResponse();
    const gatewayClient = {
      createSession: jest.fn(),
      listSessionMessages: jest
        .fn()
        .mockResolvedValueOnce({
          sessionId: 'sess-existing-processing',
          state: 'PROCESSING',
          messages: [
            {
              id: 'user-existing',
              role: 'ROLE_USER',
              content: 'Previous request',
              createdAt: String(Date.now() * 1000),
            },
          ],
          steps: [],
        })
        .mockResolvedValueOnce({
          sessionId: 'sess-existing-processing',
          state: 'IDLE',
          messages: [
            {
              id: 'user-existing',
              role: 'ROLE_USER',
              content: 'Previous request',
              createdAt: String(Date.now() * 1000),
            },
            {
              id: 'assistant-existing',
              role: 'ROLE_ASSISTANT',
              content: 'Sure',
              createdAt: String(Date.now() * 1000),
            },
          ],
          steps: [],
        }),
      getSession: jest.fn(),
    };
    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();
    fetchMock.mockImplementation((_url, init) => {
      if (init?.method === 'POST') {
        return Promise.resolve(makeStreamResponse([
          'f:{"messageId":"assistant-existing"}\n',
          '0:"Sure"\n',
        ]));
      }
      return Promise.resolve(resumeStream.response);
    });

    render(
      <TalonCopilot
        namespace="ops"
        agent="copilot"
        gatewayUrl="http://localhost:18789"
        gatewayClient={gatewayClient}
        sessionId="sess-existing-processing"
      />,
    );

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        'http://localhost:18789/v1/ui/ns/ops/agents/copilot/sessions/sess-existing-processing',
        expect.objectContaining({ signal: expect.any(AbortSignal) }),
      );
    });
    const resumeSignal = fetchMock.mock.calls[0][1].signal as AbortSignal;

    fireEvent.change(screen.getByPlaceholderText('Ask Talon to perform a task...'), {
      target: { value: 'new request' },
    });
    fireEvent.click(screen.getByRole('button', { name: /send message/i }));

    await waitFor(() => expect(resumeSignal.aborted).toBe(true));
    resumeStream.release('f:{"messageId":"assistant-existing"}\n');
    resumeStream.release('0:"Sure"\n');
    resumeStream.release(null);

    expect(await screen.findByText('Sure')).toBeInTheDocument();
    await waitFor(() => expect(gatewayClient.listSessionMessages).toHaveBeenCalledTimes(2));
    expect(screen.queryByText('SureSure')).not.toBeInTheDocument();
    expect(gatewayClient.createSession).not.toHaveBeenCalled();
  });
});

describe('TalonChannel', () => {
  afterEach(() => {
    jest.useRealTimers();
    jest.restoreAllMocks();
  });

  it('renders channel messages without inspector tabs', async () => {
    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();
    fetchMock.mockResolvedValueOnce(makeJsonResponse({
      messages: [
        {
          id: '019e7fa7-2cfe-7670-a661-42e0a70a751d',
          authorKind: 'user',
          author: 'sightline',
          content: '@triage-agent How are you doing?',
          createdAt: String(Date.now() * 1000),
        },
      ],
    }));

    render(
      <TalonChannel
        namespace="channel-collaboration"
        channel={{ name: 'incident-room', status: 'open' }}
        gatewayUrl="http://localhost:18789"
        refreshIntervalMs={false}
      />,
    );

    expect(await screen.findByText('@triage-agent How are you doing?')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'subscriptions' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Open session' })).not.toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledWith(
      'http://localhost:18789/v1/ns/channel-collaboration/channels/incident-room/messages?page_size=100',
      expect.anything(),
    );
  });

  it('renders channel message markdown', async () => {
    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();
    fetchMock.mockResolvedValueOnce(makeJsonResponse({
      messages: [
        {
          id: 'channel-md',
          authorKind: 'agent',
          author: 'scribe-agent',
          content: '### Match Update\n\n- Blue guessed `apple`\n- Red is thinking',
          createdAt: String(Date.now() * 1000),
        },
      ],
    }));

    const { container } = render(
      <TalonChannel
        namespace="game"
        channel="match-room"
        gatewayUrl="http://localhost:18789"
        refreshIntervalMs={false}
      />,
    );

    await screen.findByText('Match Update');
    expect(container.querySelector('h3')).not.toBeNull();
    expect(container.querySelector('ul')).not.toBeNull();
    expect(screen.getByText(/Blue guessed/)).toBeInTheDocument();
    expect(screen.getByText('Red is thinking')).toBeInTheDocument();
  });

  it('ignores stale channel refresh responses after switching channels', async () => {
    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();
    let resolveIncidentResponse: (value: any) => void = () => {};
    const incidentResponse = new Promise((resolve) => {
      resolveIncidentResponse = resolve;
    });

    fetchMock.mockImplementation((url: string) => {
      if (url.includes('/channels/incident-room/messages')) {
        return incidentResponse;
      }
      return Promise.resolve(makeJsonResponse({
        messages: [
          {
            id: 'next-message',
            authorKind: 'user',
            author: 'sightline',
            content: 'Message from next room',
          },
        ],
      }));
    });

    const { rerender } = render(
      <TalonChannel
        namespace="channel-collaboration"
        channel="incident-room"
        gatewayUrl="http://localhost:18789"
        refreshIntervalMs={false}
      />,
    );

    rerender(
      <TalonChannel
        namespace="channel-collaboration"
        channel="next-room"
        gatewayUrl="http://localhost:18789"
        refreshIntervalMs={false}
      />,
    );

    expect(await screen.findByText('Message from next room')).toBeInTheDocument();

    await act(async () => {
      resolveIncidentResponse(makeJsonResponse({
        messages: [
          {
            id: 'stale-message',
            authorKind: 'user',
            author: 'sightline',
            content: 'Stale incident message',
          },
        ],
      }));
      await incidentResponse;
    });

    expect(screen.queryByText('Stale incident message')).not.toBeInTheDocument();
    expect(screen.getByText('Message from next room')).toBeInTheDocument();
  });

  it('reloads channel messages when auth changes without clearing the current view first', async () => {
    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();
    fetchMock.mockResolvedValueOnce(makeJsonResponse({
      messages: [
        {
          id: 'initial-message',
          authorKind: 'user',
          author: 'sightline',
          content: 'Initial channel message',
        },
      ],
    }));
    let resolveReload: (value: any) => void = () => {};
    const reloadResponse = new Promise((resolve) => {
      resolveReload = resolve;
    });
    fetchMock.mockImplementationOnce(() => reloadResponse as any);

    const { rerender } = render(
      <TalonChannel
        namespace="channel-collaboration"
        channel="incident-room"
        gatewayUrl="http://localhost:18789"
        authToken="old-token"
        refreshIntervalMs={false}
      />,
    );

    expect(await screen.findByText('Initial channel message')).toBeInTheDocument();

    rerender(
      <TalonChannel
        namespace="channel-collaboration"
        channel="incident-room"
        gatewayUrl="http://localhost:18789"
        authToken="new-token"
        refreshIntervalMs={false}
      />,
    );

    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2));
    expect(screen.getByText('Initial channel message')).toBeInTheDocument();
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      'http://localhost:18789/v1/ns/channel-collaboration/channels/incident-room/messages?page_size=100',
      expect.objectContaining({
        headers: expect.objectContaining({
          Authorization: 'Bearer new-token',
        }),
      }),
    );

    await act(async () => {
      resolveReload(makeJsonResponse({
        messages: [
          {
            id: 'updated-message',
            authorKind: 'user',
            author: 'sightline',
            content: 'Updated channel message',
          },
        ],
      }));
      await reloadResponse;
    });

    expect(await screen.findByText('Updated channel message')).toBeInTheDocument();
    expect(screen.queryByText('Initial channel message')).not.toBeInTheDocument();
  });

  it('does not block channel refreshes when auth changes during an active refresh', async () => {
    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();
    let resolveInitialRefresh: (value: any) => void = () => {};
    const initialRefresh = new Promise((resolve) => {
      resolveInitialRefresh = resolve;
    });

    fetchMock
      .mockImplementationOnce(() => initialRefresh as any)
      .mockResolvedValueOnce(makeJsonResponse({
        messages: [
          {
            id: 'authorized-message',
            authorKind: 'user',
            author: 'sightline',
            content: 'Authorized channel message',
          },
        ],
      }));

    const { rerender } = render(
      <TalonChannel
        namespace="channel-collaboration"
        channel="incident-room"
        gatewayUrl="http://localhost:18789"
        authToken="old-token"
        refreshIntervalMs={false}
      />,
    );

    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));

    rerender(
      <TalonChannel
        namespace="channel-collaboration"
        channel="incident-room"
        gatewayUrl="http://localhost:18789"
        authToken="new-token"
        refreshIntervalMs={false}
      />,
    );

    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2));
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      'http://localhost:18789/v1/ns/channel-collaboration/channels/incident-room/messages?page_size=100',
      expect.objectContaining({
        headers: expect.objectContaining({
          Authorization: 'Bearer new-token',
        }),
      }),
    );

    await act(async () => {
      resolveInitialRefresh(makeJsonResponse({
        messages: [
          {
            id: 'stale-auth-message',
            authorKind: 'user',
            author: 'sightline',
            content: 'Stale auth message',
          },
        ],
      }));
      await initialRefresh;
    });

    expect(await screen.findByText('Authorized channel message')).toBeInTheDocument();
    expect(screen.queryByText('Stale auth message')).not.toBeInTheDocument();
  });

  it('renders injected channel message actions', async () => {
    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();
    fetchMock.mockResolvedValueOnce(makeJsonResponse({
      messages: [
        {
          id: 'agent-message-1',
          authorKind: 'agent',
          author: 'triage-agent',
          content: 'I am checking the incident.',
          sourceAgent: 'triage-agent',
          sourceSessionId: 'session-1',
        },
      ],
    }));
    const openSession = jest.fn();

    render(
      <TalonChannel
        namespace="channel-collaboration"
        channel="incident-room"
        gatewayUrl="http://localhost:18789"
        refreshIntervalMs={false}
        renderMessageActions={(message) => {
          const agent = message.sourceAgent || message.source_agent;
          const sessionId = message.sourceSessionId || message.source_session_id;
          if (!agent || !sessionId) return null;
          return <button type="button" onClick={() => openSession(agent, sessionId)}>Open session</button>;
        }}
      />,
    );

    fireEvent.click(await screen.findByRole('button', { name: 'Open session' }));
    expect(openSession).toHaveBeenCalledWith('triage-agent', 'session-1');
  });

  it('scrolls channel messages to the bottom after loading the newest page', async () => {
    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();

    const scrollTo = jest.fn();
    const originalScrollTo = HTMLElement.prototype.scrollTo;
    Object.defineProperty(HTMLElement.prototype, 'scrollTo', {
      configurable: true,
      value: scrollTo,
    });

    fetchMock.mockResolvedValueOnce(makeJsonResponse({
      messages: [
        {
          id: 'channel-message-1',
          authorKind: 'agent',
          author: 'triage-agent',
          content: 'Newest channel message',
        },
      ],
    }));

    render(
      <TalonChannel
        namespace="channel-collaboration"
        channel="incident-room"
        gatewayUrl="http://localhost:18789"
        refreshIntervalMs={false}
      />,
    );

    expect(await screen.findByText('Newest channel message')).toBeInTheDocument();
    await waitFor(() => {
      expect(scrollTo).toHaveBeenCalled();
    });

    Object.defineProperty(HTMLElement.prototype, 'scrollTo', {
      configurable: true,
      value: originalScrollTo,
    });
  });

  it('does not auto-scroll channel messages while reading older history', async () => {
    jest.useFakeTimers();
    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();

    const scrollTo = jest.fn();
    const originalScrollTo = HTMLElement.prototype.scrollTo;
    Object.defineProperty(HTMLElement.prototype, 'scrollTo', {
      configurable: true,
      value: scrollTo,
    });

    fetchMock
      .mockResolvedValueOnce(makeJsonResponse({
        messages: [
          {
            id: 'channel-message-1',
            authorKind: 'agent',
            author: 'triage-agent',
            content: 'First channel message',
          },
        ],
      }))
      .mockResolvedValueOnce(makeJsonResponse({
        messages: [
          {
            id: 'channel-message-2',
            authorKind: 'agent',
            author: 'triage-agent',
            content: 'Background channel message',
          },
        ],
      }));

    const { container } = render(
      <TalonChannel
        namespace="channel-collaboration"
        channel="incident-room"
        gatewayUrl="http://localhost:18789"
        refreshIntervalMs={750}
      />,
    );

    expect(await screen.findByText('First channel message')).toBeInTheDocument();
    await waitFor(() => expect(scrollTo).toHaveBeenCalled());
    await act(async () => {
      jest.advanceTimersByTime(32);
      await Promise.resolve();
    });
    scrollTo.mockClear();

    const scrollContainer = container.querySelector('div[aria-label="Channel messages"]') as HTMLDivElement;
    Object.defineProperty(scrollContainer, 'scrollTop', { configurable: true, value: 0, writable: true });
    Object.defineProperty(scrollContainer, 'scrollHeight', { configurable: true, value: 1000 });
    Object.defineProperty(scrollContainer, 'clientHeight', { configurable: true, value: 200 });
    fireEvent.scroll(scrollContainer);

    await act(async () => {
      jest.advanceTimersByTime(750);
      await Promise.resolve();
    });

    expect(await screen.findByText('Background channel message')).toBeInTheDocument();
    expect(scrollTo).not.toHaveBeenCalled();

    Object.defineProperty(HTMLElement.prototype, 'scrollTo', {
      configurable: true,
      value: originalScrollTo,
    });
    jest.useRealTimers();
  });

  it('posts a channel message through the gateway', async () => {
    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();
    fetchMock
      .mockResolvedValueOnce(makeJsonResponse({ messages: [] }))
      .mockResolvedValueOnce(makeJsonResponse({ message: { id: 'channel-message-1' } }))
      .mockResolvedValueOnce(makeJsonResponse({
        messages: [
          {
            id: 'channel-message-1',
            authorKind: 'user',
            author: 'sightline',
            content: 'hello channel',
          },
        ],
      }));

    render(
      <TalonChannel
        namespace="channel-collaboration"
        channel="incident-room"
        gatewayUrl="http://localhost:18789"
        refreshIntervalMs={false}
      />,
    );

    fireEvent.change(await screen.findByPlaceholderText('Message #incident-room'), {
      target: { value: 'hello channel' },
    });
    fireEvent.click(screen.getByRole('button', { name: /send channel message/i }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenNthCalledWith(
        2,
        'http://localhost:18789/v1/ns/channel-collaboration/channels/incident-room/messages',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({
            ns: 'channel-collaboration',
            channel: 'incident-room',
            authorKind: 'user',
            author: 'sightline',
            content: 'hello channel',
          }),
        }),
      );
    });
    expect(await screen.findByText('hello channel')).toBeInTheDocument();
  });

  it('posts /clear as a normal channel message because channels have no built-in clear command', async () => {
    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();
    fetchMock
      .mockResolvedValueOnce(makeJsonResponse({ messages: [] }))
      .mockResolvedValueOnce(makeJsonResponse({ message: { id: 'channel-clear-text' } }))
      .mockResolvedValueOnce(makeJsonResponse({
        messages: [
          {
            id: 'channel-clear-text',
            authorKind: 'user',
            author: 'sightline',
            content: '/clear',
          },
        ],
      }));

    render(
      <TalonChannel
        namespace="channel-collaboration"
        channel="incident-room"
        gatewayUrl="http://localhost:18789"
        refreshIntervalMs={false}
      />,
    );

    const input = await screen.findByPlaceholderText('Message #incident-room');
    fireEvent.change(input, { target: { value: '/' } });
    expect(screen.queryByRole('listbox', { name: 'Command menu' })).not.toBeInTheDocument();

    fireEvent.change(input, {
      target: { value: '/clear' },
    });
    fireEvent.click(screen.getByRole('button', { name: /send channel message/i }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenNthCalledWith(
        2,
        'http://localhost:18789/v1/ns/channel-collaboration/channels/incident-room/messages',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({
            ns: 'channel-collaboration',
            channel: 'incident-room',
            authorKind: 'user',
            author: 'sightline',
            content: '/clear',
          }),
        }),
      );
    });
    expect(await screen.findByText('/clear')).toBeInTheDocument();
  });

  it('does not post duplicate channel messages while a submit is in flight', async () => {
    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();
    let resolvePost: (value: any) => void = () => {};
    const postResponse = new Promise((resolve) => {
      resolvePost = resolve;
    });
    fetchMock
      .mockResolvedValueOnce(makeJsonResponse({ messages: [] }))
      .mockImplementationOnce(() => postResponse as any)
      .mockResolvedValueOnce(makeJsonResponse({ messages: [] }));

    render(
      <TalonChannel
        namespace="channel-collaboration"
        channel="incident-room"
        gatewayUrl="http://localhost:18789"
        refreshIntervalMs={false}
      />,
    );

    const input = await screen.findByPlaceholderText('Message #incident-room');
    fireEvent.change(input, {
      target: { value: 'hello once' },
    });
    const sendButton = screen.getByRole('button', { name: /send channel message/i });
    fireEvent.click(sendButton);
    fireEvent.click(sendButton);

    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2));
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      'http://localhost:18789/v1/ns/channel-collaboration/channels/incident-room/messages',
      expect.objectContaining({ method: 'POST' }),
    );

    await act(async () => {
      resolvePost(makeJsonResponse({ message: { id: 'channel-message-1' } }));
      await postResponse;
    });
  });

  it('keeps the channel draft when posting fails', async () => {
    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();
    fetchMock
      .mockResolvedValueOnce(makeJsonResponse({ messages: [] }))
      .mockResolvedValueOnce({ ok: false, status: 500, json: async () => ({ error: 'nope' }) } as any);

    render(
      <TalonChannel
        namespace="channel-collaboration"
        channel="incident-room"
        gatewayUrl="http://localhost:18789"
        refreshIntervalMs={false}
      />,
    );

    const input = await screen.findByPlaceholderText('Message #incident-room');
    fireEvent.change(input, {
      target: { value: 'retry me' },
    });
    fireEvent.click(screen.getByRole('button', { name: /send channel message/i }));

    expect(await screen.findByText('Post HTTP 500')).toBeInTheDocument();
    expect(input).toHaveValue('retry me');
  });

  it('clears delayed channel refresh when unmounted after posting', async () => {
    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();
    fetchMock
      .mockResolvedValueOnce(makeJsonResponse({ messages: [] }))
      .mockResolvedValueOnce(makeJsonResponse({ message: { id: 'channel-message-1' } }))
      .mockResolvedValueOnce(makeJsonResponse({
        messages: [
          {
            id: 'channel-message-1',
            authorKind: 'user',
            author: 'sightline',
            content: 'hello channel',
          },
        ],
      }));
    const clearTimeoutSpy = jest.spyOn(window, 'clearTimeout');

    const { unmount } = render(
      <TalonChannel
        namespace="channel-collaboration"
        channel="incident-room"
        gatewayUrl="http://localhost:18789"
        refreshIntervalMs={false}
      />,
    );

    fireEvent.change(await screen.findByPlaceholderText('Message #incident-room'), {
      target: { value: 'hello channel' },
    });
    fireEvent.click(screen.getByRole('button', { name: /send channel message/i }));

    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(3));
    unmount();

    expect(clearTimeoutSpy).toHaveBeenCalled();
  });

  it('can render a channel in observer mode without user input', async () => {
    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();
    fetchMock.mockResolvedValueOnce(makeJsonResponse({
      messages: [
        {
          id: 'agent-message-1',
          authorKind: 'agent',
          author: 'red-agent',
          content: 'Opening move recorded.',
        },
      ],
    }));

    render(
      <TalonChannel
        namespace="game"
        channel="match-room"
        gatewayUrl="http://localhost:18789"
        refreshIntervalMs={false}
        disableUserInput
      />,
    );

    expect(await screen.findByText('Opening move recorded.')).toBeInTheDocument();
    expect(screen.queryByPlaceholderText('Message #match-room')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /send channel message/i })).not.toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('loads older channel message pages when scrolled near the top', async () => {
    const fetchMock = global.fetch as jest.Mock;
    fetchMock.mockReset();
    fetchMock
      .mockResolvedValueOnce(makeJsonResponse({
        messages: [
          {
            id: '019f0000-0000-7000-8000-000000000002',
            authorKind: 'agent',
            author: 'triage-agent',
            content: 'Newest channel page',
            createdAt: '2000',
          },
        ],
        hasMore: true,
        nextBeforeMessageId: '019f0000-0000-7000-8000-000000000002',
      }))
      .mockResolvedValueOnce(makeJsonResponse({
        messages: [
          {
            id: '019f0000-0000-7000-8000-000000000001',
            authorKind: 'user',
            author: 'sightline',
            content: 'Older channel page',
            createdAt: '1000',
          },
        ],
        hasMore: false,
      }));

    const { container } = render(
      <TalonChannel
        namespace="channel-collaboration"
        channel="incident-room"
        gatewayUrl="http://localhost:18789"
        messageLimit={1}
        refreshIntervalMs={false}
      />,
    );

    await screen.findByText('Newest channel page');

    const scrollContainer = container.querySelector('div[aria-label="Channel messages"]') as HTMLDivElement;
    Object.defineProperty(scrollContainer, 'scrollTop', { configurable: true, value: 0, writable: true });
    Object.defineProperty(scrollContainer, 'scrollHeight', { configurable: true, value: 1000 });
    fireEvent.scroll(scrollContainer);

    expect(await screen.findByText('Older channel page')).toBeInTheDocument();
    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      'http://localhost:18789/v1/ns/channel-collaboration/channels/incident-room/messages?page_size=1',
      expect.anything(),
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      'http://localhost:18789/v1/ns/channel-collaboration/channels/incident-room/messages?page_size=1&before_message_id=019f0000-0000-7000-8000-000000000002',
      expect.anything(),
    );
  });
});
