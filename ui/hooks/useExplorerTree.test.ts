// @ts-nocheck
import { buildExplorerTree, compareTreeNodes } from './useExplorerTree';

function resource(kind: string, ns: string, name: string, caseName: string, value: any = {}) {
  return {
    kind,
    metadata: { name, namespace: ns },
    spec: { kind: { case: caseName, value } },
  };
}

describe('buildExplorerTree', () => {
  it('derives visible namespace, resource, and expanded session nodes from query data', () => {
    const tree = buildExplorerTree({
      expanded: new Set(['', 'demo', 'demo:writer']),
      queries: {
        namespaceParents: [''],
        namespaceQueries: [{ data: [{ name: 'demo', labels: { workspace_name: 'Demo' } }] }],
        resourcesByNamespaceKind: {
          demo: {
            Agent: [resource('Agent', 'demo', 'writer', 'agent')],
            Channel: [resource('Channel', 'demo', 'incidents', 'channel', { title: 'Incidents' })],
            Schedule: [resource('Schedule', 'demo', 'nightly', 'schedule', { kind: 'cron', enabled: true })],
            Knowledge: [],
            McpServer: [resource('McpServer', 'demo', 'linear', 'mcpServer', { transport: 'stdio' })],
            Template: [],
            Deployment: [],
            DeploymentReplica: [],
            SandboxClass: [],
            SandboxPolicy: [],
            Sandbox: [],
          },
        },
        sessionsByAgentKey: {
          'demo/writer': ['01HZ0000000000000000000000'],
        },
        channelSubscriptionsByKey: {},
      },
    } as any);

    expect(tree.children.demo.badge).toBe('Demo');
    expect(tree.children.demo.children.writer.selection.type).toBe('agent');
    expect(Object.values(tree.children.demo.children.writer.children)[0].selection.type).toBe('session');
    expect(tree.children.demo.children['channel:incidents'].badge).toBe('Incidents');
    expect(tree.children.demo.children['mcp-server:linear'].selection).toMatchObject({
      type: 'mcp-server',
      ns: 'demo',
      resourceName: 'linear',
    });
    expect(tree.children.demo.children['mcp-server:linear'].badge).toBe('stdio');
    expect(tree.children.demo.children['schedule:nightly'].badge).toBe('cron');
  });

  it('sorts sessions newest first while keeping namespaces above resources', () => {
    const namespaceNode = {
      id: 'demo',
      name: 'demo',
      selection: { type: 'namespace' as const, ns: 'demo', fullPath: 'demo' },
      children: {},
    };
    const agentNode = {
      id: 'demo:agent',
      name: 'agent',
      selection: { type: 'agent' as const, ns: 'demo', agent: 'agent', fullPath: 'demo/agent' },
      children: {},
    };

    expect(compareTreeNodes(namespaceNode, agentNode)).toBeLessThan(0);
  });
});
