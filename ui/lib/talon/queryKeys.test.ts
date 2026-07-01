// @ts-nocheck
import { talonQueryKeys } from './queryKeys';

describe('talonQueryKeys', () => {
  it('scopes keys by gateway and auth token fingerprint', () => {
    expect(talonQueryKeys.resources({ gatewayUrl: 'http://a', authToken: null }, 'demo', 'Agent')).toEqual([
      'talon',
      { gatewayUrl: 'http://a', auth: 'anon' },
      'resources',
      'demo',
      'Agent',
    ]);
    const firstTokenKey = talonQueryKeys.resources({ gatewayUrl: 'http://a', authToken: 'secret' }, 'demo', 'Agent');
    const secondTokenKey = talonQueryKeys.resources({ gatewayUrl: 'http://a', authToken: 'different' }, 'demo', 'Agent');
    expect(firstTokenKey[1].auth).toMatch(/^auth:6:/);
    expect(secondTokenKey[1].auth).toMatch(/^auth:9:/);
    expect(firstTokenKey[1].auth).not.toEqual(secondTokenKey[1].auth);
    expect(JSON.stringify(firstTokenKey)).not.toContain('secret');
  });

  it('nests resource detail keys under namespace resource list keys', () => {
    const scope = { gatewayUrl: 'http://a', authToken: null };
    expect(talonQueryKeys.resource(scope, 'demo', 'Agent', 'cmo')).toEqual([
      ...talonQueryKeys.resources(scope, 'demo', 'Agent'),
      'detail',
      'cmo',
    ]);
  });
});
