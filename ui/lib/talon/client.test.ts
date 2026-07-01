// @ts-nocheck
import {
  createNamespace,
  createResource,
  createSession,
  deleteNamespace,
  deleteResource,
  deleteSession,
  getNamespace,
  getResource,
  listNamespaces,
  listResources,
  listSessions,
} from './client';
import { getGatewayClient } from '../grpc';

jest.mock('../grpc', () => ({
  getGatewayClient: jest.fn(),
}));

describe('talon client helpers', () => {
  const signal = new AbortController().signal;
  const gateway = {
    namespaces: {
      list: jest.fn(),
      get: jest.fn(),
      create: jest.fn(),
      delete: jest.fn(),
    },
    resources: {
      list: jest.fn(),
      get: jest.fn(),
      create: jest.fn(),
      delete: jest.fn(),
    },
    sessions: {
      list: jest.fn(),
      create: jest.fn(),
      delete: jest.fn(),
    },
  };

  beforeEach(() => {
    jest.clearAllMocks();
    getGatewayClient.mockReturnValue(gateway);
  });

  it('wraps namespace RPCs and normalizes list responses', async () => {
    gateway.namespaces.list.mockResolvedValueOnce({ namespaces: ['demo'] });
    await expect(listNamespaces('', { signal })).resolves.toEqual(['demo']);
    expect(gateway.namespaces.list).toHaveBeenCalledWith({ parent: undefined }, { signal });

    gateway.namespaces.get.mockResolvedValueOnce({ name: 'demo' });
    await expect(getNamespace('demo', { signal })).resolves.toEqual({ name: 'demo' });
    expect(gateway.namespaces.get).toHaveBeenCalledWith({ name: 'demo' }, { signal });

    await createNamespace('demo');
    await deleteNamespace('demo');
    expect(gateway.namespaces.create).toHaveBeenCalledWith({ name: 'demo', recursive: true });
    expect(gateway.namespaces.delete).toHaveBeenCalledWith({ name: 'demo' });
  });

  it('wraps resource RPCs and unwraps resource payloads', async () => {
    gateway.resources.list.mockResolvedValueOnce({ resources: [{ metadata: { name: 'agent' } }] });
    await expect(listResources('demo', 'Agent', { signal })).resolves.toEqual([{ metadata: { name: 'agent' } }]);
    expect(gateway.resources.list).toHaveBeenCalledWith({ ns: 'demo', kind: 'Agent' }, { signal });

    gateway.resources.get.mockResolvedValueOnce({ resource: { metadata: { name: 'agent' } } });
    await expect(getResource('demo', 'Agent', 'agent', { signal })).resolves.toEqual({ metadata: { name: 'agent' } });
    expect(gateway.resources.get).toHaveBeenCalledWith({ ns: 'demo', kind: 'Agent', name: 'agent' }, { signal });

    await createResource('demo', { kind: 'Agent' });
    await deleteResource('demo', 'Agent', 'agent');
    expect(gateway.resources.create).toHaveBeenCalledWith({ ns: 'demo', manifest: { kind: 'Agent' } });
    expect(gateway.resources.delete).toHaveBeenCalledWith({ ns: 'demo', kind: 'Agent', name: 'agent' });
  });

  it('wraps session RPCs', async () => {
    gateway.sessions.list.mockResolvedValueOnce({ sessionIds: ['s1'] });
    await expect(listSessions('demo', 'agent', { signal })).resolves.toEqual(['s1']);
    expect(gateway.sessions.list).toHaveBeenCalledWith({ ns: 'demo', agent: 'agent' }, { signal });

    await createSession('demo', 'agent');
    await deleteSession('demo', 'agent', 's1');
    expect(gateway.sessions.create).toHaveBeenCalledWith({ ns: 'demo', agent: 'agent' });
    expect(gateway.sessions.delete).toHaveBeenCalledWith({ ns: 'demo', agent: 'agent', sessionId: 's1' });
  });
});
