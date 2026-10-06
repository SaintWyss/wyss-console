import {
  CAPABILITY_IDS,
  type AgentAdapterDescriptorV1,
  type ManagedAgentSessionObservationV1,
  type AgentModelDescriptorV1,
  type AdapterErrorV1,
  type AgentAdapterId,
  type ModelProviderId,
} from './agent-adapter';
import { parseUtcTimestamp } from './event-envelope';
import { parseUuid } from './uuid';

export class AdapterContractError extends Error {
  constructor(readonly code: string) {
    super(code);
    this.name = 'AdapterContractError';
  }
}
function ensure(condition: unknown, code: string): asserts condition {
  if (!condition) throw new AdapterContractError(code);
}
function object(value: unknown): Record<string, unknown> {
  ensure(
    value !== null &&
      typeof value === 'object' &&
      !Array.isArray(value) &&
      [Object.prototype, null].includes(
        Object.getPrototypeOf(value) as object | null,
      ),
    'expected_object',
  );
  return value as Record<string, unknown>;
}
function fields(
  value: unknown,
  names: readonly string[],
): Record<string, unknown> {
  const map = object(value);
  ensure(
    names.every((key) => Object.hasOwn(map, key)),
    'missing_field',
  );
  ensure(
    Object.keys(map).every((key) => names.includes(key)),
    'unexpected_field',
  );
  return map;
}
const encoder = new TextEncoder();
const length = (s: string) => encoder.encode(s).byteLength;
function text(value: unknown, max: number): string {
  ensure(
    typeof value === 'string' &&
      !/^ *$/.test(value) &&
      length(value) <= max &&
      !Array.from(value).some((char) => {
        const code = char.codePointAt(0)!;
        return code < 32 || (code >= 127 && code <= 159);
      }),
    'invalid_text',
  );
  return value;
}
function optionalText(value: unknown, max: number): void {
  if (value !== null) text(value, max);
}
function enumValue(value: unknown, values: readonly string[]): string {
  ensure(typeof value === 'string' && values.includes(value), 'invalid_enum');
  return value;
}
function machineId(value: unknown): string {
  const s = text(value, 64);
  ensure(
    /^[a-z](?:[a-z0-9-]*[a-z0-9])?$/.test(s) && !s.includes('--'),
    'invalid_id',
  );
  return s;
}
export function parseAgentAdapterId(value: unknown): AgentAdapterId {
  return machineId(value) as AgentAdapterId;
}
export function parseModelProviderId(value: unknown): ModelProviderId {
  return machineId(value) as ModelProviderId;
}
function namespace(value: string): boolean {
  return (
    length(value) <= 96 &&
    value.includes('.') &&
    value.split('.').every((part) => /^[a-z][a-z0-9_-]*$/.test(part)) &&
    !['wyss', 'core', 'normalized'].includes(value.split('.')[0])
  );
}
function uuid(value: unknown): void {
  text(value, 36);
  try {
    parseUuid(value);
  } catch {
    throw new AdapterContractError('invalid_uuid');
  }
}
// Structural limits apply before semantic validation. No text-content safety claim.
function bounded(value: unknown): void {
  let nodes = 0;
  let bytes = 0;
  const ancestors = new Set<object>();
  function visit(value: unknown, depth: number): void {
    ensure(depth <= 12 && ++nodes <= 2048, 'limit_exceeded');
    if (typeof value === 'string') {
      ensure(!/[\ud800-\udfff]/u.test(value), 'invalid_json');
      ensure(length(value) <= 2048, 'limit_exceeded');
      bytes += length(value);
    } else if (typeof value === 'number') {
      ensure(
        Number.isFinite(value) &&
          (!Number.isInteger(value) || Number.isSafeInteger(value)),
        'invalid_json',
      );
    } else if (value !== null && typeof value === 'object') {
      ensure(!ancestors.has(value), 'invalid_json');
      ancestors.add(value);
      if (Array.isArray(value)) {
        ensure(value.length <= 64, 'limit_exceeded');
        for (let i = 0; i < value.length; i++) {
          ensure(Object.hasOwn(value, i), 'invalid_json');
          visit(value[i], depth + 1);
        }
      } else {
        const map = object(value);
        ensure(Object.keys(map).length <= 64, 'limit_exceeded');
        for (const [key, item] of Object.entries(map)) {
          ensure(!/[\ud800-\udfff]/u.test(key), 'invalid_json');
          ensure(length(key) <= 128, 'limit_exceeded');
          bytes += length(key);
          visit(item, depth + 1);
        }
      }
      ancestors.delete(value);
    } else ensure(value === null || typeof value === 'boolean', 'invalid_json');
    ensure(bytes <= 32768, 'limit_exceeded');
  }
  visit(value, 0);
}
function root(
  value: unknown,
  names: readonly string[],
): Record<string, unknown> {
  bounded(value);
  const map = fields(value, names);
  ensure(map.schema_version === 1, 'invalid_schema_version');
  return map;
}
function array(value: unknown): unknown[] {
  ensure(Array.isArray(value), 'expected_array');
  return value;
}
const reasons = [
  'runtime_version_unknown',
  'transport_lacks_structure',
  'requires_configuration',
  'runtime_does_not_expose',
  'not_implemented_by_adapter',
  'not_probed',
  'partial_coverage',
  'probe_failed',
];
function reason(value: unknown): void {
  if (value !== null) enumValue(value, reasons);
}
function evidence(value: unknown): string {
  const map = fields(value, ['kind', 'outcome', 'reference', 'observed_at']);
  const kind = enumValue(map.kind, [
    'static_knowledge',
    'runtime_handshake',
    'runtime_probe',
    'user_configuration',
    'fallback_assumption',
  ]);
  const outcome = enumValue(map.outcome, [
    'declared',
    'confirmed',
    'inconclusive',
  ]);
  optionalText(map.reference, 512);
  if (map.observed_at !== null) {
    text(map.observed_at, 64);
    try {
      parseUtcTimestamp(map.observed_at);
    } catch {
      throw new AdapterContractError('invalid_timestamp');
    }
  }
  if (outcome === 'confirmed')
    ensure(
      ['runtime_handshake', 'runtime_probe'].includes(kind) &&
        map.observed_at !== null,
      'contradictory_evidence',
    );
  return outcome;
}
function evidenceList(value: unknown): string[] {
  const list = array(value);
  ensure(list.length > 0 && list.length <= 8, 'invalid_evidence');
  return list.map(evidence);
}
function version(value: unknown): void {
  const map = fields(value, ['value', 'evidence']);
  optionalText(map.value, 128);
  const outcome = evidence(map.evidence);
  ensure(
    !(map.value === null && outcome === 'confirmed'),
    'contradictory_evidence',
  );
}
function declaration(value: unknown): string {
  const map = fields(value, ['support', 'evidence', 'conditions', 'reason']);
  const support = enumValue(map.support, [
    'supported',
    'unsupported',
    'unknown',
    'conditional',
  ]);
  const outcomes = evidenceList(map.evidence);
  reason(map.reason);
  const conditions = array(map.conditions);
  ensure(conditions.length <= 8, 'limit_exceeded');
  for (const condition of conditions)
    ensure(namespace(text(condition, 96)), 'invalid_namespace');
  ensure(
    (support === 'conditional') !== (conditions.length === 0),
    'contradictory_capability',
  );
  ensure(
    support === 'supported' || map.reason !== null,
    'contradictory_capability',
  );
  ensure(
    support !== 'unknown' || !outcomes.includes('confirmed'),
    'contradictory_capability',
  );
  ensure(
    !['supported', 'conditional'].includes(support) ||
      outcomes.some((o) => o !== 'inconclusive'),
    'contradictory_capability',
  );
  return support;
}
function accessAllowed(id: string, access: string): boolean {
  if (
    [
      'session_create',
      'session_resume',
      'session_interrupt',
      'session_cancel',
      'prompt_submit',
    ].includes(id)
  )
    return access === 'request';
  if (
    [
      'session_status',
      'structured_messages',
      'streaming_output',
      'model_list',
      'tool_calls',
      'tool_results',
      'hooks',
      'mcp_servers',
      'token_usage',
      'cost_usage',
      'latency',
    ].includes(id)
  )
    return access === 'observe';
  return true;
}
function descriptor(value: unknown): void {
  const map = root(value, [
    'schema_version',
    'adapter_id',
    'runtime_kind',
    'implementation_version',
    'runtime_version',
    'protocol_version',
    'transport',
    'capabilities',
    'compatibility',
    'limitations',
    'extensions',
  ]);
  machineId(map.adapter_id);
  enumValue(map.runtime_kind, [
    'codex',
    'claude_code',
    'opencode',
    'generic_cli',
  ]);
  text(map.implementation_version, 128);
  version(map.runtime_version);
  version(map.protocol_version);
  const transport = enumValue(map.transport, [
    'structured_stdio',
    'local_http',
    'embedded_sdk',
    'terminal_fallback',
  ]);
  const capabilities = object(map.capabilities);
  for (const [key, coverage] of Object.entries(capabilities).sort(([a], [b]) =>
    a < b ? -1 : a > b ? 1 : 0,
  )) {
    const id = enumValue(key, CAPABILITY_IDS);
    const entries = object(coverage);
    ensure(Object.keys(entries).length > 0, 'contradictory_capability');
    for (const [key, value] of Object.entries(entries).sort(([a], [b]) =>
      a < b ? -1 : a > b ? 1 : 0,
    )) {
      const access = enumValue(key, ['observe', 'request']);
      ensure(accessAllowed(id, access), 'contradictory_capability');
      const support = declaration(value);
      if (
        transport === 'terminal_fallback' &&
        ['supported', 'conditional'].includes(support)
      )
        ensure(
          ['terminal_io', 'session_status'].includes(id),
          'fallback_lacks_structure',
        );
    }
  }
  const compatibility = fields(map.compatibility, [
    'state',
    'evidence',
    'reason',
  ]);
  const state = enumValue(compatibility.state, [
    'compatible',
    'incompatible',
    'degraded',
    'unknown',
  ]);
  const outcomes = evidenceList(compatibility.evidence);
  reason(compatibility.reason);
  ensure(
    state !== 'compatible' || outcomes.includes('confirmed'),
    'contradictory_compatibility',
  );
  ensure(
    state !== 'unknown' || !outcomes.includes('confirmed'),
    'contradictory_compatibility',
  );
  ensure(
    state === 'compatible' || compatibility.reason !== null,
    'contradictory_compatibility',
  );
  const limitations = array(map.limitations);
  ensure(limitations.length <= 16, 'limit_exceeded');
  for (const item of limitations) text(item, 512);
  const extensions = object(map.extensions);
  ensure(Object.keys(extensions).length <= 16, 'limit_exceeded');
  for (const [key, value] of Object.entries(extensions)) {
    ensure(namespace(key), 'invalid_namespace');
    object(value);
  }
}
function model(value: unknown): void {
  const map = root(value, [
    'schema_version',
    'provider_id',
    'model_id',
    'display_name',
    'availability',
  ]);
  if (map.provider_id !== null) machineId(map.provider_id);
  text(map.model_id, 256);
  optionalText(map.display_name, 128);
  enumValue(map.availability, ['available', 'unavailable', 'unknown']);
}
function session(value: unknown): void {
  const map = root(value, [
    'schema_version',
    'managed_session_id',
    'adapter_id',
    'runtime_instance_id',
    'runtime_session_id',
    'state',
    'runtime_state',
    'observed_model',
    'terminal_id',
  ]);
  uuid(map.managed_session_id);
  machineId(map.adapter_id);
  uuid(map.runtime_instance_id);
  optionalText(map.runtime_session_id, 256);
  enumValue(map.state, [
    'unknown',
    'creating',
    'idle',
    'running',
    'waiting_for_input',
    'waiting_for_approval',
    'interrupted',
    'completed',
    'failed',
    'closed',
  ]);
  optionalText(map.runtime_state, 128);
  if (map.observed_model !== null) model(map.observed_model);
  if (map.terminal_id !== null) {
    uuid(map.terminal_id);
    ensure(
      (map.terminal_id as string).toLowerCase() !==
        (map.managed_session_id as string).toLowerCase(),
      'identity_alias',
    );
  }
}
function adapterError(value: unknown): void {
  const map = root(value, [
    'schema_version',
    'code',
    'operation',
    'diagnostic',
  ]);
  enumValue(map.code, [
    'unavailable',
    'unsupported',
    'incompatible_runtime',
    'protocol_error',
    'transport_error',
    'authentication_required',
    'permission_required',
    'invalid_request',
    'runtime_failed',
    'interrupted',
    'timeout',
  ]);
  if (map.operation !== null) enumValue(map.operation, CAPABILITY_IDS);
  if (map.diagnostic !== null) {
    const diagnostic = fields(map.diagnostic, ['code', 'summary']);
    ensure(namespace(text(diagnostic.code, 96)), 'invalid_namespace');
    text(diagnostic.summary, 512);
  }
}
// Return detached wire values, preserving nulls, IDs and opaque version text.
function parse<T>(value: unknown, validate: (value: unknown) => void): T {
  validate(value);
  return JSON.parse(JSON.stringify(value)) as T;
}
export const parseAgentAdapterDescriptor = (
  value: unknown,
): AgentAdapterDescriptorV1 => parse(value, descriptor);
export const parseAgentModelDescriptor = (
  value: unknown,
): AgentModelDescriptorV1 => parse(value, model);
export const parseManagedAgentSessionObservation = (
  value: unknown,
): ManagedAgentSessionObservationV1 => parse(value, session);
export const parseAdapterError = (value: unknown): AdapterErrorV1 =>
  parse(value, adapterError);
