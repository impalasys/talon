// @ts-nocheck
import {
  channelDocumentFromResource,
  channelFromResource,
  channelSubscriptionDocumentFromResource,
  channelSubscriptionFromResource,
  fileFromResource,
  isV2ResourceDocument,
  namespaceLabel,
  parseJsonObject,
  resourceMetadata,
  resourcePhase,
  resourceSpec,
  resourceStatus,
  scheduleDocumentFromResource,
  scheduleFromResource,
  templateSummary,
} from './resourceMappers';

const resource = (caseName: string, spec = {}, status = {}) => ({
  apiVersion: 'talon/v1',
  kind: 'Resource',
  metadata: { name: 'name', namespace: 'demo', labels: { workspace_name: 'Workspace' } },
  spec: { kind: { case: caseName, value: spec } },
  status: { kind: { case: caseName, value: status } },
});

describe('resource mappers', () => {
  it('reads matching oneof spec and status values only', () => {
    const item = resource('channel', { title: 'Incidents' }, { phase: 'Ready', updatedAt: 10n });
    expect(resourceSpec(item, 'channel')).toEqual({ title: 'Incidents' });
    expect(resourceSpec(item, 'agent')).toEqual({});
    expect(resourceStatus(item, 'channel')).toEqual({ phase: 'Ready', updatedAt: 10n });
    expect(resourceStatus(item, 'agent')).toEqual({});
    expect(resourcePhase(item, 'channel')).toBe('Ready');
  });

  it('maps channel and subscription resources into explorer rows and documents', () => {
    const channel = resource('channel', { title: 'Alerts', metadata: { icon: 'bell' } }, { phase: 'Active' });
    expect(channelFromResource(channel)).toEqual({
      name: 'name',
      ns: 'demo',
      title: 'Alerts',
      status: 'Active',
      labels: { workspace_name: 'Workspace' },
    });
    expect(channelDocumentFromResource(channel)).toEqual({
      name: 'name',
      ns: 'demo',
      title: 'Alerts',
      status: 'Active',
      metadata: { icon: 'bell' },
      labels: { workspace_name: 'Workspace' },
    });

    const subscription = resource('channelSubscription', {
      channel: 'alerts',
      agent: 'responder',
      enabled: true,
      trigger: 'mention',
      replyMode: 'thread',
      contextPolicy: { window: 20 },
    });
    expect(channelSubscriptionFromResource(subscription)).toEqual({
      name: 'name',
      ns: 'demo',
      channel: 'alerts',
      agent: 'responder',
      enabled: true,
      trigger: 'mention',
      replyMode: 'thread',
    });
    expect(channelSubscriptionDocumentFromResource(subscription).contextPolicy).toEqual({ window: 20 });
  });

  it('maps schedules, files, and metadata helpers', () => {
    const schedule = resource('schedule', { kind: 'cron', enabled: true }, { nextRunAt: 20n });
    expect(scheduleFromResource(schedule)).toEqual({
      name: 'name',
      ns: 'demo',
      labels: { workspace_name: 'Workspace' },
      spec: { kind: 'cron', enabled: true },
      status: { nextRunAt: 20n },
    });
    expect(scheduleDocumentFromResource(schedule)).toMatchObject({
      spec: { kind: 'cron', enabled: true },
      status: { nextRunAt: '20' },
    });

    const bytes = new Uint8Array([12, 34]);
    expect(scheduleDocumentFromResource(resource('schedule', { payload: bytes })).spec.payload).toBe(bytes);

    const protobufScheduleSpec = Object.assign(Object.create({}), {
      $typeName: 'talon.resources.ScheduleSpec',
      nextRunAt: 30n,
      payload: bytes,
    });
    expect(scheduleDocumentFromResource(resource('schedule', protobufScheduleSpec)).spec).toEqual({
      nextRunAt: '30',
      payload: bytes,
    });

    expect(fileFromResource(resource('file', { path: '/docs', mediaType: 'text/markdown' })).spec).toEqual({
      path: '/docs',
      mediaType: 'text/markdown',
    });
    expect(resourceMetadata('demo', 'root').generation).toBe(0n);
    expect(namespaceLabel({ workspace: 'workspace', display_name: 'display', name: 'name' })).toBe('workspace');
    expect(namespaceLabel({ display_name: 'display', name: 'name' })).toBe('display');
    expect(namespaceLabel({ name: 'name' })).toBe('name');
  });

  it('parses template summaries and rejects non-object json', () => {
    expect(parseJsonObject('{"systemPrompt":"  answer carefully  "}')).toEqual({ systemPrompt: '  answer carefully  ' });
    expect(parseJsonObject('[1]')).toEqual({});
    expect(parseJsonObject('nope')).toEqual({});
    expect(parseJsonObject(undefined)).toEqual({});

    expect(
      templateSummary(resource('template', {
        kind: 'Agent',
        metadata: { name: 'helper' },
        specJson: '{"systemPrompt":"  answer carefully  "}',
      })),
    ).toBe('Agent/helper: answer carefully');
    expect(templateSummary(resource('template', { metadata: {} }))).toBe('Resource/unnamed');
  });

  it('detects v2 resource envelopes', () => {
    expect(isV2ResourceDocument(resource('channel'))).toBe(true);
    expect(isV2ResourceDocument({ apiVersion: 'talon/v1', kind: 'Resource', metadata: {} })).toBe(false);
    expect(isV2ResourceDocument(null)).toBe(false);
  });
});
