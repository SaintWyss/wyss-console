import { parseManagedSessionId, type ManagedSessionId } from './identities';
import { parseUuid, type Uuid } from './uuid';

declare const sequenceBrand: unique symbol;
declare const timestampBrand: unique symbol;
export type Sequence = string & { readonly [sequenceBrand]: true };
export type UtcTimestamp = string & { readonly [timestampBrand]: true };
export type Persistence = 'durable' | 'ephemeral';
export type Sensitivity = 'public' | 'internal' | 'sensitive' | 'secret';
export type Redaction = 'none' | 'applied' | 'content_omitted';
export type JsonValue =
  null | boolean | number | string | JsonValue[] | JsonObject;
export type JsonObject = { [key: string]: JsonValue };
export const DEFAULT_PERSISTENCE: Persistence = 'ephemeral';

export interface EventEnvelopeV1 {
  readonly schema_version: 1;
  readonly event_id: Uuid;
  readonly event_type: string;
  readonly payload_schema_version: number;
  readonly timestamp: UtcTimestamp;
  readonly stream_id: Uuid;
  readonly sequence: Sequence;
  readonly session_id: ManagedSessionId | null;
  readonly source: { readonly component: string; readonly instance_id: Uuid };
  readonly correlation_id: Uuid | null;
  readonly causation_id: Uuid | null;
  readonly payload: JsonObject;
  readonly persistence: Persistence;
  readonly sensitivity: Sensitivity;
  readonly redaction: Redaction;
}

export type ErrorCode =
  | 'expected_object'
  | 'missing_field'
  | 'unexpected_field'
  | 'invalid_schema_version'
  | 'invalid_uuid'
  | 'invalid_timestamp'
  | 'invalid_sequence'
  | 'invalid_event_type'
  | 'invalid_positive_integer'
  | 'invalid_component'
  | 'invalid_enum'
  | 'invalid_json_value'
  | 'durable_secret';

/** Sanitized structural diagnostics; this is not a content safety scanner. */
export class ContractValidationError extends Error {
  constructor(
    readonly field: string,
    readonly code: ErrorCode,
  ) {
    super(`${field}: ${code}`);
    this.name = 'ContractValidationError';
  }
}

function reject(field: string, code: ErrorCode): never {
  throw new ContractValidationError(field, code);
}

const fields = [
  'schema_version',
  'event_id',
  'event_type',
  'payload_schema_version',
  'timestamp',
  'stream_id',
  'sequence',
  'session_id',
  'source',
  'correlation_id',
  'causation_id',
  'payload',
  'persistence',
  'sensitivity',
  'redaction',
] as const;

function object(value: unknown, field: string): Record<string, unknown> {
  if (
    value === null ||
    typeof value !== 'object' ||
    Array.isArray(value) ||
    ![Object.prototype, null].includes(Object.getPrototypeOf(value))
  ) {
    return reject(field, 'expected_object');
  }
  return value as Record<string, unknown>;
}

function required(
  value: Record<string, unknown>,
  names: readonly string[],
  path: string,
) {
  for (const name of names) {
    if (!Object.hasOwn(value, name))
      reject(path === '$' ? name : `${path}.${name}`, 'missing_field');
  }
  if (Object.keys(value).some((name) => !names.includes(name)))
    reject(path, 'unexpected_field');
}

function uuid(value: unknown, field: string): Uuid {
  try {
    return parseUuid(value);
  } catch {
    return reject(field, 'invalid_uuid');
  }
}

function optionalUuid(value: unknown, field: string): Uuid | null {
  return value === null ? null : uuid(value, field);
}

export function parseSequence(value: unknown): Sequence {
  const maximum = '18446744073709551615';
  if (
    typeof value !== 'string' ||
    !/^[1-9][0-9]*$/.test(value) ||
    value.endsWith('\n') ||
    value.length > maximum.length ||
    (value.length === maximum.length && value > maximum)
  )
    reject('sequence', 'invalid_sequence');
  return value as Sequence;
}

export function parseUtcTimestamp(value: unknown): UtcTimestamp {
  if (typeof value !== 'string')
    return reject('timestamp', 'invalid_timestamp');
  const match =
    /^(\d{4})-(\d{2})-(\d{2})[Tt](\d{2}):(\d{2}):(\d{2})(?:\.\d+)?(?:[Zz]|\+00:00)$/.exec(
      value,
    );
  if (!match || match[0] !== value)
    return reject('timestamp', 'invalid_timestamp');
  const [year, month, day, hour, minute, second] = match.slice(1).map(Number);
  const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const days = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][
    month - 1
  ];
  if (
    !days ||
    day < 1 ||
    day > days ||
    hour > 23 ||
    minute > 59 ||
    second > 60 ||
    (second === 60 && (hour !== 23 || minute !== 59 || day !== days))
  ) {
    return reject('timestamp', 'invalid_timestamp');
  }
  return value as UtcTimestamp;
}

function enumValue<T extends string>(
  value: unknown,
  choices: readonly T[],
  field: string,
): T {
  if (typeof value !== 'string' || !choices.includes(value as T))
    reject(field, 'invalid_enum');
  return value as T;
}

function json(value: unknown, ancestors = new Set<object>()): void {
  if (value === null || typeof value === 'string' || typeof value === 'boolean')
    return;
  if (typeof value === 'number' && Number.isFinite(value)) return;
  if (typeof value !== 'object' || ancestors.has(value))
    reject('payload', 'invalid_json_value');
  ancestors.add(value);
  if (Array.isArray(value)) {
    for (let index = 0; index < value.length; index++)
      json(value[index], ancestors);
  } else {
    const record = object(value, 'payload');
    for (const item of Object.values(record)) json(item, ancestors);
  }
  ancestors.delete(value);
}

/** Accept external JSON-shaped data; never supplies missing persistence or identities. */
export function parseEventEnvelope(value: unknown): EventEnvelopeV1 {
  const root = object(value, '$');
  required(root, fields, '$');
  if (root.schema_version !== 1)
    reject('schema_version', 'invalid_schema_version');
  const event_id = uuid(root.event_id, 'event_id');
  if (
    typeof root.event_type !== 'string' ||
    !/^[a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)+$/.test(root.event_type) ||
    root.event_type.endsWith('\n')
  )
    reject('event_type', 'invalid_event_type');
  if (
    typeof root.payload_schema_version !== 'number' ||
    !Number.isInteger(root.payload_schema_version) ||
    root.payload_schema_version <= 0
  ) {
    reject('payload_schema_version', 'invalid_positive_integer');
  }
  const timestamp = parseUtcTimestamp(root.timestamp);
  const stream_id = uuid(root.stream_id, 'stream_id');
  const sequence = parseSequence(root.sequence);
  const session = optionalUuid(root.session_id, 'session_id');
  const session_id = session === null ? null : parseManagedSessionId(session);
  const source = object(root.source, 'source');
  required(source, ['component', 'instance_id'], 'source');
  // Unicode White_Space matches Rust str::trim; C0/C1 controls are disallowed.
  if (
    typeof source.component !== 'string' ||
    /^[\p{White_Space}]*$/u.test(source.component) ||
    [...source.component].some(
      (char) =>
        char.codePointAt(0)! <= 31 ||
        (char.codePointAt(0)! >= 127 && char.codePointAt(0)! <= 159),
    )
  ) {
    reject('source.component', 'invalid_component');
  }
  const instance_id = uuid(source.instance_id, 'source.instance_id');
  const correlation_id = optionalUuid(root.correlation_id, 'correlation_id');
  const causation_id = optionalUuid(root.causation_id, 'causation_id');
  const payload = object(root.payload, 'payload');
  json(payload);
  const persistence = enumValue(
    root.persistence,
    ['durable', 'ephemeral'],
    'persistence',
  );
  const sensitivity = enumValue(
    root.sensitivity,
    ['public', 'internal', 'sensitive', 'secret'],
    'sensitivity',
  );
  const redaction = enumValue(
    root.redaction,
    ['none', 'applied', 'content_omitted'],
    'redaction',
  );
  if (persistence === 'durable' && sensitivity === 'secret')
    reject('sensitivity', 'durable_secret');
  return {
    schema_version: 1,
    event_id,
    event_type: root.event_type,
    payload_schema_version: root.payload_schema_version,
    timestamp,
    stream_id,
    sequence,
    session_id,
    source: { component: source.component, instance_id },
    correlation_id,
    causation_id,
    payload: JSON.parse(JSON.stringify(payload)) as JsonObject,
    persistence,
    sensitivity,
    redaction,
  };
}
