// @ts-nocheck
import {
  areSelectionsEqual,
  buildSearchParams,
  namespaceAncestors,
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
  });
});
