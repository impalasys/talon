// @ts-nocheck
import { buildNamespaceContents, buildNamespaceTree, parseSessionDate } from './explorerModel';

function resource(kind: string, ns: string, name: string, caseName: string, value: any = {}) {
  return {
    kind,
    metadata: { name, namespace: ns },
    spec: { kind: { case: caseName, value } },
  };
}

describe('explorer model', () => {
  it('builds a namespace-only tree', () => {
    const tree = buildNamespaceTree({
      activeNamespace: 'Tenant:conic',
      selectedNode: { type: 'file', ns: 'Tenant:conic', resourceName: 'guide', fullPath: 'Tenant:conic:file:guide' },
      namespaceParents: ['', 'Tenant'],
      namespaceQueries: [
        { data: [{ name: 'Tenant', labels: {} }] },
        { data: [{ name: 'Tenant:conic', labels: { workspace_name: 'Conic' } }] },
      ],
    } as any);

    expect(tree.children).toHaveLength(1);
    expect(tree.children[0].selection.type).toBe('namespace');
    expect(tree.children[0].children[0].selection).toEqual({
      type: 'namespace',
      ns: 'Tenant:conic',
      fullPath: 'Tenant:conic',
    });
    expect(tree.children[0].children[0].badge).toBe('Conic');
  });

  it('builds contents only for the active namespace', () => {
    const groups = buildNamespaceContents({
      activeNamespace: 'Tenant:conic',
      resourcesByNamespaceKind: {
        'Tenant:conic': {
          Agent: [resource('Agent', 'Tenant:conic', 'writer', 'agent')],
          File: [resource('File', 'Tenant:conic', 'guide', 'file', { path: 'docs/guide.md' })],
          Channel: [resource('Channel', 'Tenant:conic', 'alerts', 'channel', { title: 'Alerts' })],
          Schedule: [],
          McpServer: [],
          Template: [],
          Deployment: [],
          DeploymentReplica: [],
          ConnectorClass: [
            resource('ConnectorClass', 'Tenant:conic', 'slack', 'connectorClass', { platform: 'slack' }),
          ],
          Connector: [
            resource('Connector', 'Tenant:conic', 'alerts', 'connector', {
              classRef: { name: 'slack' },
              enabled: true,
            }),
          ],
          SandboxClass: [],
          SandboxPolicy: [],
          Sandbox: [],
        },
        'Tenant:other': {
          Agent: [resource('Agent', 'Tenant:other', 'other-agent', 'agent')],
        },
      },
      sessionsByAgentKey: {
        'Tenant:conic/writer': ['01HZ0000000000000000000000'],
      },
      channelSubscriptionsByKey: {},
    } as any);

    expect(groups.map((group) => group.title)).toEqual(['Agents', 'Channels', 'Files', 'Connectors']);
    expect(groups[0].nodes[0].selection).toMatchObject({ type: 'agent', ns: 'Tenant:conic', agent: 'writer' });
    expect(groups[0].nodes[0].children[0].selection.type).toBe('session');
    expect(groups[3].nodes.map((node) => node.selection.type)).toEqual(['connector', 'connector-class']);
    expect(groups.flatMap((group) => group.nodes).some((node) => node.name === 'other-agent')).toBe(false);
  });

  it('falls back to the ID prefix for non-v7 UUID session IDs', () => {
    expect(parseSessionDate('550e8400-e29b-41d4-a716-446655440000')).toBe('550e8400');
  });
});
