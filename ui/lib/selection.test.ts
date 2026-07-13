// @ts-nocheck
import {
  areSelectionsEqual,
  buildSearchParams,
  getSelectionSubtitle,
  getSelectionTitle,
  namespaceAncestors,
  namespaceResolutionAncestry,
  RESOURCE_KIND_BY_SELECTION,
  selectionExpansionIds,
  selectionFromSearchParams,
} from './selection';

describe('selection helpers', () => {
  it('parses and serializes deep session URLs without changing the public shape', () => {
    const params = new URLSearchParams({
      connected: 'true',
      ns: 'conic:wks:13',
      type: 'session',
      agent: 'cmo',
      session: 'session-123',
    });

    const selection = selectionFromSearchParams(params);

    expect(selection).toEqual({
      type: 'session',
      ns: 'conic:wks:13',
      agent: 'cmo',
      sessionId: 'session-123',
      fullPath: 'conic:wks:13/cmo/session-123',
    });
    expect(buildSearchParams(true, selection, params).toString()).toBe(
      'connected=true&ns=conic%3Awks%3A13&type=session&agent=cmo&session=session-123',
    );
  });

  it('returns namespace and selected child expansion ids for deep links', () => {
    expect(namespaceAncestors('conic:wks:13')).toEqual(['', 'conic', 'conic:wks', 'conic:wks:13']);
    expect(
      selectionExpansionIds({
        type: 'channel-subscription',
        ns: 'conic:wks',
        channel: 'incidents',
        resourceName: 'triage',
        fullPath: 'conic:wks:channel:incidents:subscription:triage',
      }),
    ).toEqual(['', 'conic', 'conic:wks', 'conic:wks:channel:incidents']);
  });

  it('compares selection identity fields only', () => {
    expect(
      areSelectionsEqual(
        { type: 'agent', ns: 'demo', agent: 'writer', fullPath: 'demo/writer' },
        { type: 'agent', ns: 'demo', agent: 'writer', fullPath: 'different' },
      ),
    ).toBe(true);
    expect(areSelectionsEqual(null, null)).toBe(true);
    expect(areSelectionsEqual(null, { type: 'namespace', ns: 'demo', fullPath: 'demo' })).toBe(false);
    expect(
      areSelectionsEqual(
        { type: 'agent', ns: 'demo', agent: 'writer', fullPath: 'demo/writer' },
        { type: 'agent', ns: 'demo', agent: 'reader', fullPath: 'demo/reader' },
      ),
    ).toBe(false);
  });

  it('parses each supported URL resource type', () => {
    const cases = [
      ['channel', { channel: 'alerts' }, 'demo:channel:alerts'],
      ['channel-subscription', { channel: 'alerts', name: 'responder' }, 'demo:channel:alerts:subscription:responder'],
      ['schedule', { name: 'daily' }, 'demo:schedule:daily'],
      ['mcp-server', { name: 'github' }, 'demo:mcp-server:github'],
      ['knowledge', { name: 'docs' }, 'demo:knowledge:docs'],
      ['workflow', { name: 'wf' }, 'demo:workflow:wf'],
      ['deployment', { name: 'prod' }, 'demo:deployment:prod'],
      ['deployment-replica', { name: 'prod-1' }, 'demo:deployment-replica:prod-1'],
      ['connector-class', { name: 'slack' }, 'demo:connector-class:slack'],
      ['connector', { name: 'alerts' }, 'demo:connector:alerts'],
      ['sandbox-class', { name: 'node' }, 'demo:sandbox-class:node'],
      ['sandbox-policy', { name: 'default' }, 'demo:sandbox-policy:default'],
      ['sandbox', { name: 'box' }, 'demo:sandbox:box'],
    ];

    for (const [type, extra, fullPath] of cases) {
      const params = new URLSearchParams({ connected: 'true', ns: 'demo', type, ...extra });
      expect(selectionFromSearchParams(params)?.fullPath).toBe(fullPath);
    }

    expect(selectionFromSearchParams(new URLSearchParams({ type: 'template', name: 'starter' }))).toEqual({
      type: 'template',
      ns: 'Sys',
      resourceName: 'starter',
      fullPath: 'Sys:template:starter',
    });
    expect(selectionFromSearchParams(new URLSearchParams())).toBeNull();
    expect(selectionFromSearchParams(new URLSearchParams({ ns: 'demo' }))).toEqual({
      type: 'namespace',
      ns: 'demo',
      fullPath: 'demo',
    });
  });

  it('builds search params defensively', () => {
    expect(buildSearchParams(false, null, new URLSearchParams({ historyPageSize: '-1' })).toString()).toBe('');
    expect(
      buildSearchParams(
        true,
        {
          type: 'channel',
          ns: 'demo',
          channel: 'alerts',
          resourceName: 'alerts',
          fullPath: 'demo:channel:alerts',
        },
        new URLSearchParams({ historyPageSize: '25' }),
      ).toString(),
    ).toBe('connected=true&historyPageSize=25&ns=demo&type=channel&channel=alerts&name=alerts');
  });

  it('keeps connection root separate from explorer selection namespace', () => {
    const params = new URLSearchParams({ root: 'Tenant:conic', ns: 'Tenant:other', type: 'namespace' });

    expect(selectionFromSearchParams(params)).toEqual({
      type: 'namespace',
      ns: 'Tenant:other',
      fullPath: 'Tenant:other',
    });
    expect(buildSearchParams(true, selectionFromSearchParams(params), params).toString()).toBe(
      'connected=true&root=Tenant%3Aconic&ns=Tenant%3Aother&type=namespace',
    );
  });

  it('describes selections for headings', () => {
    expect(namespaceResolutionAncestry('a:b:c')).toEqual(['a:b:c', 'a:b', 'a']);
    expect(namespaceResolutionAncestry('')).toEqual([]);
    expect(selectionExpansionIds(null)).toEqual([]);
    expect(RESOURCE_KIND_BY_SELECTION.agent).toBe('Agent');

    const selections = [
      [{ type: 'namespace', ns: 'demo', fullPath: 'demo' }, 'demo', 'Namespace'],
      [{ type: 'agent', ns: 'demo', agent: 'writer', fullPath: 'demo/writer' }, 'writer', 'demo / Agent'],
      [{ type: 'session', ns: 'demo', agent: 'writer', sessionId: 's1', fullPath: 'demo/writer/s1' }, 's1', 'demo / writer'],
      [{ type: 'channel', ns: 'demo', channel: 'alerts', fullPath: 'demo:channel:alerts' }, 'alerts', 'demo / Channel'],
      [
        { type: 'channel-subscription', ns: 'demo', channel: 'alerts', resourceName: 'sub', fullPath: 'demo:channel:alerts:subscription:sub' },
        'sub',
        'demo / alerts / ChannelSubscription',
      ],
      [{ type: 'workflow', ns: 'demo', resourceName: 'wf', fullPath: 'demo:workflow:wf' }, 'wf', 'demo / Workflow'],
      [{ type: 'schedule', ns: 'demo', resourceName: 'daily', fullPath: 'demo:schedule:daily' }, 'daily', 'demo / Schedule'],
      [{ type: 'mcp-server', ns: 'demo', resourceName: 'github', fullPath: 'demo:mcp-server:github' }, 'github', 'demo / MCPServer'],
      [{ type: 'knowledge', ns: 'demo', resourceName: 'docs', fullPath: 'demo:knowledge:docs' }, 'docs', 'demo / Knowledge'],
      [{ type: 'template', ns: 'Sys', resourceName: 'starter', fullPath: 'Sys:template:starter' }, 'starter', 'Sys / Template'],
      [{ type: 'deployment', ns: 'demo', resourceName: 'prod', fullPath: 'demo:deployment:prod' }, 'prod', 'demo / Deployment'],
      [
        { type: 'deployment-replica', ns: 'demo', resourceName: 'prod-1', fullPath: 'demo:deployment-replica:prod-1' },
        'prod-1',
        'demo / DeploymentReplica',
      ],
      [{ type: 'connector-class', ns: 'demo', resourceName: 'slack', fullPath: 'demo:connector-class:slack' }, 'slack', 'demo / ConnectorClass'],
      [{ type: 'connector', ns: 'demo', resourceName: 'alerts', fullPath: 'demo:connector:alerts' }, 'alerts', 'demo / Connector'],
      [{ type: 'sandbox-class', ns: 'demo', resourceName: 'node', fullPath: 'demo:sandbox-class:node' }, 'node', 'demo / SandboxClass'],
      [{ type: 'sandbox-policy', ns: 'demo', resourceName: 'default', fullPath: 'demo:sandbox-policy:default' }, 'default', 'demo / SandboxPolicy'],
      [{ type: 'sandbox', ns: 'demo', resourceName: 'box', fullPath: 'demo:sandbox:box' }, 'box', 'demo / Sandbox'],
    ];

    for (const [selection, title, subtitle] of selections) {
      expect(getSelectionTitle(selection)).toBe(title);
      expect(getSelectionSubtitle(selection)).toBe(subtitle);
    }
    expect(getSelectionTitle(null)).toBe('No Resource Selected');
    expect(getSelectionSubtitle(null)).toMatch(/Select a namespace/);
  });
});
