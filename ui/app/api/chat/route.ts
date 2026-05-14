import { NextRequest } from 'next/server';
import { createUIMessageStream, createUIMessageStreamResponse } from 'ai';
import { createClient } from '@connectrpc/connect';
import { createGrpcWebTransport } from '@connectrpc/connect-web';
import { GatewayService } from '../../../proto/proto/gateway_pb';

function parsePayload(payloadJson?: string) {
  if (!payloadJson) return {};
  try {
    return JSON.parse(payloadJson);
  } catch {
    return {};
  }
}

async function fetchSessionJson(
  gatewayUrl: string,
  ns: string,
  agent: string,
  sessionId: string,
  authHeader: string | null,
) {
  const response = await fetch(
    `${gatewayUrl}/v1/ns/${encodeURIComponent(ns)}/agents/${encodeURIComponent(agent)}/sessions/${encodeURIComponent(sessionId)}`,
    {
      headers: authHeader ? { authorization: authHeader } : undefined,
      cache: 'no-store',
    },
  );

  if (!response.ok) {
    throw new Error(`Failed to fetch session ${sessionId}: ${response.status}`);
  }

  return response.json();
}

function extractToolStepPayload(step: any) {
  const payload = parsePayload(typeof step?.payloadJson === 'string' ? step.payloadJson : '');
  const toolCallId = typeof payload.tool_call_id === 'string' ? payload.tool_call_id : '';
  if (!toolCallId) return null;
  return {
    toolCallId,
    toolName: typeof step?.name === 'string' && step.name.length > 0 ? step.name : 'tool',
    args: payload.input ?? {},
    result: payload.output ?? step?.content,
  };
}

function isTokenStep(stepType: unknown) {
  return stepType === 1 || stepType === 'STEP_TYPE_TOKEN';
}

function isActionStep(stepType: unknown) {
  return stepType === 2 || stepType === 'STEP_TYPE_ACTION';
}

function isObservationStep(stepType: unknown) {
  return stepType === 3 || stepType === 'STEP_TYPE_OBSERVATION';
}

function isDoneStep(stepType: unknown) {
  return stepType === 4 || stepType === 'STEP_TYPE_DONE';
}

function isErrorStep(stepType: unknown) {
  return stepType === 5 || stepType === 'STEP_TYPE_ERROR';
}

function encodeResumePart(part: unknown, encoder: TextEncoder) {
  return encoder.encode(`${JSON.stringify(part)}\n`);
}

async function fetchLatestToolStep(
  gatewayUrl: string,
  ns: string,
  agent: string,
  sessionId: string,
  authHeader: string | null,
  stepType: 'STEP_TYPE_ACTION' | 'STEP_TYPE_OBSERVATION',
) {
  const session = await fetchSessionJson(gatewayUrl, ns, agent, sessionId, authHeader);
  const steps = Array.isArray(session?.steps) ? session.steps : [];
  for (let i = steps.length - 1; i >= 0; i -= 1) {
    const step = steps[i];
    if (step?.stepType !== stepType) continue;
    const payload = extractToolStepPayload(step);
    if (payload) return payload;
  }
  return null;
}

function createResumeStream(client: any, gatewayUrl: string, authHeader: string | null, ns: string, agent: string, sessionId: string) {
  const stream = new ReadableStream({
    async start(controller) {
      try {
        const encoder = new TextEncoder();
        for await (const step of client.streamSessionSteps({ ns, agent, sessionId })) {
          if (isTokenStep(step.stepType)) {
            if (step.content) {
              controller.enqueue(encodeResumePart({ type: 'text', value: step.content }, encoder));
            }
          } else if (isActionStep(step.stepType)) {
            let payload = extractToolStepPayload(step);
            if (!payload) {
              payload = await fetchLatestToolStep(gatewayUrl, ns, agent, sessionId, authHeader, 'STEP_TYPE_ACTION');
            }
            const toolCallId = payload?.toolCallId || `tool-${Date.now()}`;
            controller.enqueue(encodeResumePart({
              type: 'tool_call',
              value: {
                toolCallId,
                toolName: payload?.toolName || step.name || step.content || 'tool',
                args: payload?.args ?? {},
              },
            }, encoder));
          } else if (isObservationStep(step.stepType)) {
            let payload = extractToolStepPayload(step);
            if (!payload) {
              payload = await fetchLatestToolStep(gatewayUrl, ns, agent, sessionId, authHeader, 'STEP_TYPE_OBSERVATION');
            }
            const toolCallId = payload?.toolCallId || '';
            if (toolCallId) {
              controller.enqueue(encodeResumePart({
                type: 'tool_result',
                value: {
                  toolCallId,
                  result: payload?.result ?? step.content,
                },
              }, encoder));
            }
          } else if (isDoneStep(step.stepType)) {
            break;
          } else if (isErrorStep(step.stepType)) {
            controller.enqueue(encodeResumePart({ type: 'error', value: step.content || 'Stream error' }, encoder));
            break;
          }
        }
      } catch (e) {
        console.error('Stream error:', e);
      } finally {
        controller.close();
      }
    }
  });

  return new Response(stream, {
    headers: {
      'Content-Type': 'text/plain; charset=utf-8',
      'X-Vercel-AI-Data-Stream': 'v1'
    },
  });
}

export const dynamic = 'force-dynamic';

export async function POST(req: NextRequest) {
  const { messages, gatewayUrl, ns, agent, sessionId } = await req.json();
  const lastMessage = messages[messages.length - 1].content;
  const authHeader = req.headers.get('authorization');

  // If running inside Docker, map known local domains to the envoy/gateway services
  let internalGatewayUrl = gatewayUrl || "http://localhost:18789";
  if (gatewayUrl && (gatewayUrl.includes('localhost:18789') || gatewayUrl.includes('talon.orb.local'))) {
      internalGatewayUrl = "http://envoy:8081";
  }

  const interceptors = [];
  if (authHeader) {
     interceptors.push((next: any) => async (request: any) => {
         request.header.set("authorization", authHeader);
         return await next(request);
     });
  }

  const client = createClient(
    GatewayService,
    createGrpcWebTransport({ baseUrl: internalGatewayUrl, interceptors })
  );

  // Send the message via gRPC
  await client.sendMessage({ ns, agent, sessionId, message: lastMessage });

  const stream = createUIMessageStream({
    execute: async ({ writer }) => {
      const textPartId = crypto.randomUUID();
      let textStarted = false;
      writer.write({ type: 'start' });

      for await (const step of client.streamSessionSteps({ ns, agent, sessionId })) {
        if (isTokenStep(step.stepType)) {
          if (!step.content) continue;
          if (!textStarted) {
            writer.write({ type: 'text-start', id: textPartId });
            textStarted = true;
          }
          writer.write({ type: 'text-delta', id: textPartId, delta: step.content });
        } else if (isActionStep(step.stepType)) {
          let payload = extractToolStepPayload(step);
          if (!payload) {
            payload = await fetchLatestToolStep(internalGatewayUrl, ns, agent, sessionId, authHeader, 'STEP_TYPE_ACTION');
          }
          writer.write({
            type: 'tool-input-available',
            toolCallId: payload?.toolCallId || `tool-${Date.now()}`,
            toolName: payload?.toolName || step.name || step.content || 'tool',
            input: payload?.args ?? {},
            dynamic: true,
          });
        } else if (isObservationStep(step.stepType)) {
          let payload = extractToolStepPayload(step);
          if (!payload) {
            payload = await fetchLatestToolStep(internalGatewayUrl, ns, agent, sessionId, authHeader, 'STEP_TYPE_OBSERVATION');
          }
          if (payload?.toolCallId) {
            writer.write({
              type: 'tool-output-available',
              toolCallId: payload.toolCallId,
              output: payload.result ?? step.content,
              dynamic: true,
            });
          }
        } else if (isDoneStep(step.stepType)) {
          break;
        } else if (isErrorStep(step.stepType)) {
          writer.write({ type: 'error', errorText: step.content || 'Stream error' });
          break;
        }
      }

      if (textStarted) {
        writer.write({ type: 'text-end', id: textPartId });
      }
      writer.write({ type: 'finish' });
    },
    onError(error) {
      console.error('Stream error:', error);
      return error instanceof Error ? error.message : 'Stream error';
    },
  });

  return createUIMessageStreamResponse({ stream });
}

export async function GET(req: NextRequest) {
  const { searchParams } = new URL(req.url);
  const ns = searchParams.get('ns');
  const agent = searchParams.get('agent');
  const sessionId = searchParams.get('sessionId');
  const gatewayUrl = searchParams.get('gatewayUrl');
  const authHeader = req.headers.get('authorization');

  if (!ns || !agent || !sessionId) {
    return new Response('Missing parameters', { status: 400 });
  }

  // If running inside Docker, map known local domains to the envoy/gateway services
  let internalGatewayUrl = gatewayUrl || "http://localhost:18789";
  if (gatewayUrl && (gatewayUrl.includes('localhost:18789') || gatewayUrl.includes('talon.orb.local'))) {
      internalGatewayUrl = "http://envoy:8081";
  }

  const interceptors = [];
  if (authHeader) {
     interceptors.push((next: any) => async (request: any) => {
         request.header.set("authorization", authHeader);
         return await next(request);
     });
  }

  const client = createClient(
    GatewayService,
    createGrpcWebTransport({ baseUrl: internalGatewayUrl, interceptors })
  );

  return createResumeStream(client, internalGatewayUrl, authHeader, ns, agent, sessionId);
}

export async function DELETE(req: NextRequest) {
  const { sessionId, gatewayUrl, ns, agent } = await req.json();
  const authHeader = req.headers.get('authorization');

  if (!ns || !agent || !sessionId) {
    return new Response('Missing parameters', { status: 400 });
  }

  let internalGatewayUrl = gatewayUrl || "http://localhost:18789";
  if (gatewayUrl && (gatewayUrl.includes('localhost:18789') || gatewayUrl.includes('talon.orb.local'))) {
      internalGatewayUrl = "http://envoy:8081";
  }

  const response = await fetch(
    `${internalGatewayUrl}/v1/ns/${encodeURIComponent(ns)}/agents/${encodeURIComponent(agent)}/sessions/${encodeURIComponent(sessionId)}:stop`,
    {
      method: 'POST',
      headers: authHeader ? { authorization: authHeader, 'content-type': 'application/json' } : { 'content-type': 'application/json' },
      body: JSON.stringify({ ns, agent, sessionId }),
      cache: 'no-store',
    },
  );

  if (!response.ok) {
    return new Response(`Failed to stop session: ${response.status}`, { status: response.status });
  }

  return new Response(null, { status: 204 });
}
