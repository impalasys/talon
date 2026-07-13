import { dump } from 'js-yaml';
import { resourceToManifestDocument } from './resourceManifest';

describe('resourceToManifestDocument', () => {
  it('renders File enum fields as symbolic YAML values', () => {
    const document = resourceToManifestDocument({
      apiVersion: 'talon.impalasys.com/v1',
      kind: 'File',
      metadata: {
        name: 'brand-guidelines-md-7f3a',
        namespace: 'Tenant:acme',
        labels: {},
        annotations: {},
      },
      spec: {
        kind: {
          case: 'file',
          value: {
            path: '/memory/brand-guidelines.md',
            mediaType: 'text/markdown',
            purpose: 1,
            indexPolicy: 3,
            retention: 1,
          },
        },
      },
      status: {
        kind: {
          case: 'file',
          value: {
            observedGeneration: BigInt(0),
            phase: '',
            conditions: [],
          },
        },
      },
    } as any);

    expect(document.spec).toEqual({
      path: '/memory/brand-guidelines.md',
      mediaType: 'text/markdown',
      purpose: 'MEMORY',
      indexPolicy: 'RETRIEVAL',
      retention: 'RETAINED',
    });
    expect(document).not.toHaveProperty('status');
  });

  it('renders a protobuf Resource as user-facing Template YAML', () => {
    const document = resourceToManifestDocument({
      $typeName: 'talon.resources.Resource',
      apiVersion: 'talon.impalasys.com/v1',
      kind: 'Template',
      metadata: {
        $typeName: 'talon.resources.ResourceMeta',
        name: 'support-docs-template',
        namespace: 'Sys',
        labels: {},
        annotations: {},
        ownerReferences: [],
        finalizers: [],
        generation: BigInt(2),
        resourceVersion: '019ed3a1-1a79-7c10-87e6-116626743a87',
        uid: '019ed386-093f-72a0-ba23-e3fef7a60388',
      },
      spec: {
        kind: {
          case: 'template',
          value: {
            $typeName: 'talon.resources.TemplateSpec',
            kind: 'Agent',
            metadata: {
              $typeName: 'talon.resources.ResourceMeta',
              name: 'support-docs',
              namespace: '',
              labels: {},
              annotations: {},
            },
            specJson: JSON.stringify({
              modelPolicy: {
                profiles: [
                  {
                    name: 'default',
                    model: {
                      provider: 'openai',
                      name: 'gpt-5.4-nano',
                      temperature: 0,
                    },
                  },
                ],
              },
              systemPrompt: 'You are a product assistant.',
            }),
          },
        },
      },
      status: {
        kind: {
          case: 'template',
          value: {
            $typeName: 'talon.resources.CommonResourceStatus',
            observedGeneration: BigInt(0),
            phase: '',
            conditions: [],
          },
        },
      },
    } as any);

    expect(document).toEqual({
      apiVersion: 'talon.impalasys.com/v1',
      kind: 'Template',
      metadata: {
        name: 'support-docs-template',
        namespace: 'Sys',
        labels: {},
        annotations: {},
      },
      spec: {
        kind: 'Agent',
        metadata: {
          name: 'support-docs',
          namespace: '',
          labels: {},
          annotations: {},
        },
        spec: {
          modelPolicy: {
            profiles: [
              {
                name: 'default',
                model: {
                  provider: 'openai',
                  name: 'gpt-5.4-nano',
                  temperature: 0,
                },
              },
            ],
          },
          systemPrompt: 'You are a product assistant.',
        },
      },
    });

    const yaml = dump(document, { noRefs: true, lineWidth: 100 });
    expect(yaml).toContain('spec:\n  kind: Agent');
    expect(yaml).toContain('systemPrompt: You are a product assistant.');
    expect(yaml).not.toContain('$typeName');
    expect(yaml).not.toContain('case: template');
    expect(yaml).not.toContain('specJson');
    expect(yaml).not.toContain('status:');
  });

  it('promotes SandboxClass embedded provider JSON into YAML fields', () => {
    const document = resourceToManifestDocument({
      apiVersion: 'talon.impalasys.com/v1',
      kind: 'SandboxClass',
      metadata: {
        name: 'docker-code',
        namespace: 'Sys',
        labels: {},
        annotations: {},
      },
      spec: {
        kind: {
          case: 'sandboxClass',
          value: {
            provider: 'docker',
            providerConfigJson: '{"image":"ubuntu:24.04","pullPolicy":"IfNotPresent"}',
            credentialsJson: '{}',
          },
        },
      },
      status: {
        kind: {
          case: 'sandboxClass',
          value: {
            observedGeneration: BigInt(7),
            phase: 'Ready',
            conditions: [],
          },
        },
      },
    } as any);

    expect(document.spec).toEqual({
      provider: 'docker',
      providerConfig: {
        image: 'ubuntu:24.04',
        pullPolicy: 'IfNotPresent',
      },
      credentials: {},
    });
    expect(document.status).toEqual({
      observedGeneration: '7',
      phase: 'Ready',
    });
  });
});
