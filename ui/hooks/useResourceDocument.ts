import { useQuery, useQueryClient } from '@tanstack/react-query';
import { dump } from 'js-yaml';
import type { Selection } from '../lib/selection';
import { RESOURCE_KIND_BY_SELECTION } from '../lib/selection';
import { getNamespace, getResource } from '../lib/talon/client';
import { talonQueryKeys, type TalonQueryScope } from '../lib/talon/queryKeys';
import {
  channelDocumentFromResource,
  channelSubscriptionDocumentFromResource,
  isV2ResourceDocument,
  scheduleDocumentFromResource,
  type ResourceEnvelope,
} from '../lib/talon/resourceMappers';
import { resourceToManifestDocument, yamlSafeValue } from '../lib/resourceManifest';

function resourceIdentity(selection: Selection | null) {
  if (!selection || selection.type === 'session') return null;
  if (selection.type === 'namespace') {
    return { kind: 'Namespace', name: selection.ns };
  }
  const kind = RESOURCE_KIND_BY_SELECTION[selection.type];
  const name = selection.agent || selection.resourceName || selection.channel || '';
  if (!kind || !name) return null;
  return { kind, name };
}

function documentFromSelection(selection: Selection, resource: any) {
  if (selection.type === 'channel') return channelDocumentFromResource((resource || {}) as ResourceEnvelope);
  if (selection.type === 'channel-subscription') {
    return channelSubscriptionDocumentFromResource((resource || {}) as ResourceEnvelope);
  }
  if (selection.type === 'schedule') return scheduleDocumentFromResource((resource || {}) as ResourceEnvelope);
  if (isV2ResourceDocument(resource)) return resourceToManifestDocument(resource);
  return yamlSafeValue(resource);
}

export function useResourceDocument({
  isConnected,
  scope,
  selection,
}: {
  isConnected: boolean;
  scope: TalonQueryScope;
  selection: Selection | null;
}) {
  const queryClient = useQueryClient();
  const identity = resourceIdentity(selection);

  const query = useQuery({
    queryKey:
      selection && identity
        ? selection.type === 'namespace'
          ? talonQueryKeys.resource(scope, selection.ns, 'Namespace', selection.ns)
          : talonQueryKeys.resource(scope, selection.ns, identity.kind, identity.name)
        : talonQueryKeys.resource(scope, '', 'none', ''),
    queryFn: async ({ signal }) => {
      if (!selection || !identity) return null;
      if (selection.type === 'namespace') {
        return getNamespace(selection.ns, { signal });
      }

      const cachedList = queryClient.getQueryData<ResourceEnvelope[]>(
        talonQueryKeys.resources(scope, selection.ns, identity.kind),
      );
      const cachedResource = cachedList?.find((resource) => resource.metadata?.name === identity.name);
      if (cachedResource) return cachedResource;
      return getResource(selection.ns, identity.kind, identity.name, { signal });
    },
    enabled: isConnected && Boolean(selection && identity && selection.type !== 'session'),
  });

  const document = selection && query.data ? documentFromSelection(selection, query.data) : null;
  return {
    ...query,
    document,
    yaml: document ? dump(document, { noRefs: true, lineWidth: 100 }) : '',
  };
}
