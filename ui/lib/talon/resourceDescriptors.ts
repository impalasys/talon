import type { SelectionType } from '../selection';
import { resourcePhase, resourceSpec, resourceStatus, type ResourceEnvelope } from './resourceMappers';

export type ExplorerIconKey =
  | 'activity'
  | 'box'
  | 'clock'
  | 'container'
  | 'cpu'
  | 'file'
  | 'folder'
  | 'hash'
  | 'layers'
  | 'message'
  | 'package'
  | 'plug'
  | 'radio'
  | 'shield';

export type ResourceDescriptor = {
  kind: string;
  selectionType: SelectionType;
  sortPrefix: string;
  sortWeight: number;
  icon: ExplorerIconKey;
  appearsInTree: boolean;
  hasChildren?: boolean;
  badge(resource: ResourceEnvelope): string | undefined;
};

export const RESOURCE_DESCRIPTORS: ResourceDescriptor[] = [
  {
    kind: 'McpServer',
    selectionType: 'mcp-server',
    sortPrefix: 'mcp-server',
    sortWeight: 4,
    icon: 'plug',
    appearsInTree: true,
    badge: (resource) => {
      const spec = resourceSpec(resource, 'mcpServer') as { disabled?: boolean; transport?: string };
      return spec.disabled ? 'disabled' : spec.transport || 'mcp';
    },
  },
  {
    kind: 'Template',
    selectionType: 'template',
    sortPrefix: 'template',
    sortWeight: 10,
    icon: 'file',
    appearsInTree: true,
    badge: (resource) => resourceSpec(resource, 'template').kind || 'template',
  },
  {
    kind: 'Deployment',
    selectionType: 'deployment',
    sortPrefix: 'deployment',
    sortWeight: 8,
    icon: 'layers',
    appearsInTree: true,
    badge: (resource) => {
      const spec = resourceSpec(resource, 'deployment');
      return resourcePhase(resource, 'deployment') || `${spec.templates?.length || 0} templates`;
    },
  },
  {
    kind: 'DeploymentReplica',
    selectionType: 'deployment-replica',
    sortPrefix: 'deployment-replica',
    sortWeight: 9,
    icon: 'package',
    appearsInTree: true,
    badge: (resource) => {
      const spec = resourceSpec(resource, 'deploymentReplica');
      const status = resourceStatus(resource, 'deploymentReplica');
      return status.conflicts?.length ? `${status.conflicts.length} conflicts` : spec.targetNamespace || 'replica';
    },
  },
  {
    kind: 'SandboxClass',
    selectionType: 'sandbox-class',
    sortPrefix: 'sandbox-class',
    sortWeight: 7,
    icon: 'shield',
    appearsInTree: true,
    badge: (resource) => resourceSpec(resource, 'sandboxClass').provider || 'provider',
  },
  {
    kind: 'SandboxPolicy',
    selectionType: 'sandbox-policy',
    sortPrefix: 'sandbox-policy',
    sortWeight: 6,
    icon: 'box',
    appearsInTree: true,
    badge: (resource) => resourceSpec(resource, 'sandboxPolicy').classRef?.name || 'policy',
  },
  {
    kind: 'Sandbox',
    selectionType: 'sandbox',
    sortPrefix: 'sandbox',
    sortWeight: 5,
    icon: 'container',
    appearsInTree: true,
    badge: (resource) => {
      const status = resourceStatus(resource, 'sandbox');
      return status.lease?.ownerSessionId ? 'leased' : status.phase || 'sandbox';
    },
  },
];

export const RESOURCE_DESCRIPTOR_BY_SELECTION = Object.fromEntries(
  RESOURCE_DESCRIPTORS.map((descriptor) => [descriptor.selectionType, descriptor]),
) as Partial<Record<SelectionType, ResourceDescriptor>>;
