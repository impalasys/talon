import { getGatewayClient } from '../grpc';
import type { ResourceEnvelope } from './resourceMappers';

export async function listNamespaces(parent: string, options?: { signal?: AbortSignal }) {
  const response = await getGatewayClient().namespaces.list({ parent: parent || undefined }, options);
  return response.namespaces || [];
}

export async function getNamespace(name: string, options?: { signal?: AbortSignal }) {
  return getGatewayClient().namespaces.get({ name }, options);
}

export async function createNamespace(name: string) {
  return getGatewayClient().namespaces.create({ name, recursive: true });
}

export async function deleteNamespace(name: string) {
  return getGatewayClient().namespaces.delete({ name });
}

export async function listResources(ns: string, kind: string, options?: { signal?: AbortSignal }) {
  const response = await getGatewayClient().resources.list({ ns, kind }, options);
  return (response.resources || []) as ResourceEnvelope[];
}

export async function getResource(ns: string, kind: string, name: string, options?: { signal?: AbortSignal }) {
  const response = await getGatewayClient().resources.get({ ns, kind, name }, options);
  return response.resource as ResourceEnvelope | undefined;
}

export async function createResource(ns: string, manifest: any) {
  return getGatewayClient().resources.create({ ns, manifest });
}

export async function deleteResource(ns: string, kind: string, name: string) {
  return getGatewayClient().resources.delete({ ns, kind, name });
}

export async function listSessions(ns: string, agent: string, options?: { signal?: AbortSignal }) {
  const response = await getGatewayClient().sessions.list({ ns, agent }, options);
  return response.sessionIds || [];
}

export async function createSession(ns: string, agent: string) {
  return getGatewayClient().sessions.create({ ns, agent });
}

export async function deleteSession(ns: string, agent: string, sessionId: string) {
  return getGatewayClient().sessions.delete({ ns, agent, sessionId });
}
