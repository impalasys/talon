import { secretEntriesFromResource, secretManifestFromEntries } from './SecretInspector';

describe('SecretInspector helpers', () => {
  it('decodes existing data for editing and preserves unchanged encoding', () => {
    const secret = {
      apiVersion: 'talon.impalasys.com/v1',
      kind: 'Secret',
      metadata: { name: 'credentials', namespace: 'demo' },
      spec: { kind: { case: 'secret', value: { type: 'Opaque', data: { token: 'dG9rZW4=' } } } },
    };
    const entries = secretEntriesFromResource(secret);

    expect(entries[0]).toMatchObject({ key: 'token', value: 'token', originalEncoded: 'dG9rZW4=' });
    expect(secretManifestFromEntries(secret, 'Opaque', entries).spec).toEqual({
      type: 'Opaque',
      data: { token: 'dG9rZW4=' },
      stringData: {},
    });
  });

  it('uses stringData for changed and new values', () => {
    const secret = {
      apiVersion: 'talon.impalasys.com/v1',
      kind: 'Secret',
      metadata: { name: 'credentials', namespace: 'demo' },
      spec: { type: 'Opaque', data: { token: 'dG9rZW4=' } },
    };
    const entries = secretEntriesFromResource(secret).map((entry) => ({ ...entry, value: 'changed' }));
    entries.push({ id: 2, key: 'region', value: 'us-west-2' });

    expect(secretManifestFromEntries(secret, 'Opaque', entries).spec).toEqual({
      type: 'Opaque',
      data: {},
      stringData: { token: 'changed', region: 'us-west-2' },
    });
  });
});
