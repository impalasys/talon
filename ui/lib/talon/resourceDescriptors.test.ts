// @ts-nocheck
import { RESOURCE_DESCRIPTOR_BY_SELECTION, RESOURCE_DESCRIPTORS } from './resourceDescriptors';

describe('resource descriptors', () => {
  it('keeps supported resource kinds in a registry keyed by selection type', () => {
    expect(RESOURCE_DESCRIPTORS.map((descriptor) => descriptor.kind)).toEqual(
      expect.arrayContaining(['McpServer', 'Template', 'Deployment', 'Sandbox']),
    );
    expect(RESOURCE_DESCRIPTOR_BY_SELECTION['mcp-server']?.kind).toBe('McpServer');
    expect(RESOURCE_DESCRIPTOR_BY_SELECTION.template?.kind).toBe('Template');
    expect(RESOURCE_DESCRIPTOR_BY_SELECTION.sandbox?.sortPrefix).toBe('sandbox');
  });

  it('maps resource badges without rendering the explorer', () => {
    const descriptor = RESOURCE_DESCRIPTOR_BY_SELECTION.deployment!;
    expect(
      descriptor.badge({
        kind: 'Deployment',
        metadata: { name: 'prod', namespace: 'demo' },
        spec: { kind: { case: 'deployment', value: { templates: [{ name: 'agent' }] } } },
        status: { kind: { case: 'deployment', value: { phase: '' } } },
      }),
    ).toBe('1 templates');
  });
});
