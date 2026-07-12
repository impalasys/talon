// @ts-nocheck
import { buildNamespaceContents, buildNamespaceTree } from './explorerModel';

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
      selectedNode: { type: 'knowledge', ns: 'Tenant:conic', resourceName: 'guide', fullPath: 'Tenant:conic:knowledge:guide' },
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
          Knowledge: [resource('Knowledge', 'Tenant:conic', 'guide', 'knowledge', { path: 'docs/guide.md' })],
          Channel: [resource('Channel', 'Tenant:conic', 'alerts', 'channel', { title: 'Alerts' })],
          Schedule: [],
          McpServer: [],
          Template: [],
          Deployment: [],
          DeploymentReplica: [],
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

    expect(groups.map((group) => group.title)).toEqual(['Agents', 'Channels', 'Knowledge']);
    expect(groups[0].nodes[0].selection).toMatchObject({ type: 'agent', ns: 'Tenant:conic', agent: 'writer' });
    expect(groups[0].nodes[0].children[0].selection.type).toBe('session');
    expect(groups.flatMap((group) => group.nodes).some((node) => node.name === 'other-agent')).toBe(false);
  });
});
