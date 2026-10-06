import type { JsonValue } from './event-envelope';
import type {
  AgentRuntimeInstanceId,
  ManagedSessionId,
  TerminalId,
} from './identities';

export type AgentRuntimeKind =
  'codex' | 'claude_code' | 'opencode' | 'generic_cli';
export type AgentTransportKind =
  'structured_stdio' | 'local_http' | 'embedded_sdk' | 'terminal_fallback';
export type CapabilitySupport =
  'supported' | 'unsupported' | 'unknown' | 'conditional';
// Request is an integration operation, never policy mediation or authorization.
export type CapabilityAccess = 'observe' | 'request';
export const CAPABILITY_IDS = [
  'session_create',
  'session_resume',
  'session_interrupt',
  'session_cancel',
  'session_status',
  'prompt_submit',
  'structured_messages',
  'streaming_output',
  'model_list',
  'model_selection',
  'tool_calls',
  'tool_results',
  'approvals',
  'subagents',
  'skills',
  'hooks',
  'mcp_servers',
  'token_usage',
  'cost_usage',
  'latency',
  'working_directory',
  'terminal_io',
] as const;
export type CapabilityId = (typeof CAPABILITY_IDS)[number];
export type EvidenceKind =
  | 'static_knowledge'
  | 'runtime_handshake'
  | 'runtime_probe'
  | 'user_configuration'
  | 'fallback_assumption';
export type EvidenceOutcome = 'declared' | 'confirmed' | 'inconclusive';
export type CompatibilityState =
  'compatible' | 'incompatible' | 'degraded' | 'unknown';
export type CapabilityReason =
  | 'runtime_version_unknown'
  | 'transport_lacks_structure'
  | 'requires_configuration'
  | 'runtime_does_not_expose'
  | 'not_implemented_by_adapter'
  | 'not_probed'
  | 'partial_coverage'
  | 'probe_failed';
export type ManagedAgentSessionState =
  | 'unknown'
  | 'creating'
  | 'idle'
  | 'running'
  | 'waiting_for_input'
  | 'waiting_for_approval'
  | 'interrupted'
  | 'completed'
  | 'failed'
  | 'closed';
export type ModelAvailability = 'available' | 'unavailable' | 'unknown';
export type AdapterErrorCode =
  | 'unavailable'
  | 'unsupported'
  | 'incompatible_runtime'
  | 'protocol_error'
  | 'transport_error'
  | 'authentication_required'
  | 'permission_required'
  | 'invalid_request'
  | 'runtime_failed'
  | 'interrupted'
  | 'timeout';

declare const machineIdBrand: unique symbol;
export type AgentAdapterId = string & {
  readonly [machineIdBrand]: 'AgentAdapterId';
};
export type ModelProviderId = string & {
  readonly [machineIdBrand]: 'ModelProviderId';
};
export interface CapabilityEvidence {
  readonly kind: EvidenceKind;
  readonly outcome: EvidenceOutcome;
  readonly reference: string | null;
  readonly observed_at: string | null;
}
export interface VersionObservation {
  readonly value: string | null;
  readonly evidence: CapabilityEvidence;
}
export interface CapabilityDeclaration {
  readonly support: CapabilitySupport;
  readonly evidence: readonly CapabilityEvidence[];
  readonly conditions: readonly string[];
  readonly reason: CapabilityReason | null;
}
export interface AdapterCompatibility {
  readonly state: CompatibilityState;
  readonly evidence: readonly CapabilityEvidence[];
  readonly reason: CapabilityReason | null;
}
export interface AgentAdapterDescriptorV1 {
  readonly schema_version: 1;
  readonly adapter_id: AgentAdapterId;
  readonly runtime_kind: AgentRuntimeKind;
  readonly implementation_version: string;
  readonly runtime_version: VersionObservation;
  readonly protocol_version: VersionObservation;
  readonly transport: AgentTransportKind;
  readonly capabilities: Readonly<
    Partial<
      Record<
        CapabilityId,
        Readonly<Partial<Record<CapabilityAccess, CapabilityDeclaration>>>
      >
    >
  >;
  readonly compatibility: AdapterCompatibility;
  readonly limitations: readonly string[];
  readonly extensions: Readonly<
    Record<string, Readonly<Record<string, JsonValue>>>
  >;
}
export interface AgentModelDescriptorV1 {
  readonly schema_version: 1;
  readonly provider_id: ModelProviderId | null;
  readonly model_id: string;
  readonly display_name: string | null;
  readonly availability: ModelAvailability;
}
// Observation/projection only; this DTO does not establish canonical ownership.
export interface ManagedAgentSessionObservationV1 {
  readonly schema_version: 1;
  readonly managed_session_id: ManagedSessionId;
  readonly adapter_id: AgentAdapterId;
  readonly runtime_instance_id: AgentRuntimeInstanceId;
  readonly runtime_session_id: string | null;
  readonly state: ManagedAgentSessionState;
  readonly runtime_state: string | null;
  readonly observed_model: AgentModelDescriptorV1 | null;
  readonly terminal_id: TerminalId | null;
}
export interface SafeDiagnostic {
  readonly code: string;
  readonly summary: string;
}
export interface AdapterErrorV1 {
  readonly schema_version: 1;
  readonly code: AdapterErrorCode;
  readonly operation: CapabilityId | null;
  readonly diagnostic: SafeDiagnostic | null;
}
export function capabilitySupport(
  descriptor: AgentAdapterDescriptorV1,
  id: CapabilityId,
  access: CapabilityAccess,
): CapabilitySupport {
  return descriptor.capabilities[id]?.[access]?.support ?? 'unknown';
}
export {
  AdapterContractError,
  parseAgentAdapterDescriptor,
  parseManagedAgentSessionObservation,
  parseAgentModelDescriptor,
  parseAdapterError,
  parseAgentAdapterId,
  parseModelProviderId,
} from './agent-adapter-validation';
