/**
 * ConfigurationInventory v1: what the desktop read of the agents' own configuration,
 * and how it was read. Closed words, hashes, and text that has already been through
 * redaction in the desktop: nothing here is a path, a file's content, or a secret.
 *
 * The window checks every answer against this shape and believes nothing that does not
 * fit; the desktop is the judge of what the files said.
 */
export const PROVIDERS = ['claude_code', 'codex'] as const;
export type Provider = (typeof PROVIDERS)[number];

export const SCOPES = ['system', 'global', 'project'] as const;
export type Scope = (typeof SCOPES)[number];

export const SOURCE_CLASSES = [
  'user',
  'project',
  'system',
  'provider_owned',
  'generated',
] as const;
export type SourceClass = (typeof SOURCE_CLASSES)[number];

export const SOURCE_KEYS = [
  'claude_managed_settings',
  'claude_user_settings',
  'claude_user_state',
  'claude_user_skills',
  'claude_user_rules',
  'claude_project_settings',
  'claude_project_local_settings',
  'claude_project_mcp',
  'claude_project_skills',
  'claude_project_rules',
  'codex_system_requirements',
  'codex_system_managed',
  'codex_user_config',
  'codex_user_hooks',
  'codex_user_rules',
  'codex_project_config',
  'codex_project_rules',
] as const;
export type SourceKey = (typeof SOURCE_KEYS)[number];

export const ITEM_KINDS = [
  'hook',
  'rule_file',
  'skill',
  'mcp_server',
  'model_preference',
] as const;
export type ItemKind = (typeof ITEM_KINDS)[number];

export const PERSISTED = [
  'present_enabled',
  'present_disabled',
  'absent',
  'parked',
  'unreadable',
] as const;
export type Persisted = (typeof PERSISTED)[number];

export const EFFECTIVE = [
  'verified_enabled',
  'verified_disabled',
  'documented',
  'not_verifiable',
] as const;
export type Effective = (typeof EFFECTIVE)[number];

export const SESSION_USE = [
  'using',
  'documented_using',
  'not_using',
  'stale',
  'unknown',
] as const;
export type SessionUse = (typeof SESSION_USE)[number];

/** Why nothing may be written to an item's place, in this phase or ever. */
export const NOT_WRITABLE = [
  'system',
  'provider_owned',
  'generated',
  'shared_path',
  'not_in_this_phase',
] as const;
export type NotWritable = (typeof NOT_WRITABLE)[number];

export const SECRET_CLASSES = [
  'environment_value',
  'header',
  'url_credentials',
  'secret_field',
  'token_shape',
] as const;
export type SecretClass = (typeof SECRET_CLASSES)[number];

export type PathTrust =
  | { trust: 'trusted' }
  | { trust: 'read_only'; reason: 'group_writable' | 'multiple_names' };

export type SourceFailure =
  | { failure: 'missing' | 'unreadable' | 'unstable' | 'credential_file' }
  | { failure: 'untrusted_path'; reason: UntrustedReason }
  | { failure: 'unsupported'; reason: 'not_regular_file' | 'link_limit' }
  | { failure: 'too_large'; bound: TooLargeBound }
  | { failure: 'malformed'; line: number; column: number };

const UNTRUSTED_REASONS = [
  'not_plain',
  'ancestor_link',
  'ancestor_not_directory',
  'foreign_owner',
  'writable_by_others',
  'outside_roots',
] as const;
export type UntrustedReason = (typeof UNTRUSTED_REASONS)[number];

const TOO_LARGE_BOUNDS = [
  'file',
  'listing',
  'items',
  'depth',
  'elements',
] as const;
export type TooLargeBound = (typeof TOO_LARGE_BOUNDS)[number];

export type SourceOutcome =
  | {
      outcome: 'read';
      hash: string;
      trust: PathTrust;
      followed_links: number;
      /** Entries the adapter could not place; they stay in the file untouched. */
      unrecognised: number;
    }
  | { outcome: 'absent' }
  | { outcome: 'project_not_trusted' }
  | { outcome: 'failed'; failure: SourceFailure };

export type SourceReport = {
  key: SourceKey;
  provider: Provider;
  scope: Scope;
  class: SourceClass;
  not_writable: NotWritable;
} & SourceOutcome;

/** A value as the page may see it: text that went through redaction, or what hides. */
export type Projection =
  | { kind: 'null' }
  | { kind: 'bool'; value: boolean }
  | { kind: 'number'; value: number }
  | { kind: 'text'; value: { text: string; found: SecretClass[] } }
  | { kind: 'hidden'; value: SecretClass }
  | { kind: 'list'; value: Projection[] }
  | { kind: 'map'; value: [string, Projection][] };

export interface ConfigItem {
  id: string;
  provider: Provider;
  source: SourceKey;
  scope: Scope;
  kind: ItemKind;
  name: string;
  persisted: Persisted;
  effective: Effective;
  session_use: SessionUse;
  not_writable: NotWritable;
  detail: Projection | null;
  /** For a rule file: the digest of the file, never its text. */
  hash: string | null;
}

export interface Inventory {
  sources: SourceReport[];
  items: ConfigItem[];
  /** More items than the bound allows were found; the rest were not kept. */
  truncated: boolean;
}

export interface InventoryReply {
  revision: number;
  inventory: Inventory;
}

export type Changes =
  | { result: 'same'; revision: number }
  | { result: 'changed'; revision: number; sources: SourceKey[] }
  | { result: 'resync'; revision: number };

export const CONFIGURATION_FAILURES = ['unavailable', 'internal'] as const;
export type ConfigurationFailure = (typeof CONFIGURATION_FAILURES)[number];

/** The deepest a projection may nest; the desktop's bound is 32. */
const MAX_DEPTH = 40;
const MAX_ITEMS = 1 << 20;

function invalid(field: string): Error {
  return new Error(`configuration: invalid_${field}`);
}
function record(
  value: unknown,
  keys: readonly string[],
  field: string,
): Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value))
    throw invalid(field);
  const names = Object.keys(value);
  if (names.length !== keys.length || !keys.every((k) => names.includes(k)))
    throw invalid(field);
  return value as Record<string, unknown>;
}
function member<T extends string>(
  value: unknown,
  allowed: readonly T[],
  field: string,
): T {
  if (typeof value !== 'string' || !allowed.includes(value as T))
    throw invalid(field);
  return value as T;
}
function text(value: unknown, field: string): string {
  if (typeof value !== 'string') throw invalid(field);
  return value;
}
function count(value: unknown, field: string): number {
  if (!Number.isSafeInteger(value) || (value as number) < 0)
    throw invalid(field);
  return value as number;
}
function flag(value: unknown, field: string): boolean {
  if (typeof value !== 'boolean') throw invalid(field);
  return value;
}
function list(value: unknown, field: string): unknown[] {
  if (!Array.isArray(value) || value.length > MAX_ITEMS) throw invalid(field);
  return value;
}
function digest(value: unknown, field: string): string {
  if (typeof value !== 'string' || !/^[0-9a-f]{64}$/.test(value))
    throw invalid(field);
  return value;
}

function parseTrust(value: unknown): PathTrust {
  const raw = value as Record<string, unknown> | null;
  if (raw?.trust === 'trusted') {
    record(value, ['trust'], 'trust');
    return { trust: 'trusted' };
  }
  const read = record(value, ['trust', 'reason'], 'trust');
  if (read.trust !== 'read_only') throw invalid('trust');
  return {
    trust: 'read_only',
    reason: member(read.reason, ['group_writable', 'multiple_names'], 'trust'),
  };
}

function parseFailure(value: unknown): SourceFailure {
  const raw = value as Record<string, unknown> | null;
  const word = member(
    raw?.failure,
    [
      'missing',
      'unreadable',
      'unstable',
      'credential_file',
      'untrusted_path',
      'unsupported',
      'too_large',
      'malformed',
    ] as const,
    'failure',
  );
  switch (word) {
    case 'untrusted_path': {
      const f = record(value, ['failure', 'reason'], 'failure');
      return {
        failure: word,
        reason: member(f.reason, UNTRUSTED_REASONS, 'failure'),
      };
    }
    case 'unsupported': {
      const f = record(value, ['failure', 'reason'], 'failure');
      return {
        failure: word,
        reason: member(f.reason, ['not_regular_file', 'link_limit'], 'failure'),
      };
    }
    case 'too_large': {
      const f = record(value, ['failure', 'bound'], 'failure');
      return {
        failure: word,
        bound: member(f.bound, TOO_LARGE_BOUNDS, 'failure'),
      };
    }
    case 'malformed': {
      const f = record(value, ['failure', 'line', 'column'], 'failure');
      return {
        failure: word,
        line: count(f.line, 'failure'),
        column: count(f.column, 'failure'),
      };
    }
    default:
      record(value, ['failure'], 'failure');
      return { failure: word };
  }
}

const SOURCE_COMMON = ['key', 'provider', 'scope', 'class', 'not_writable'];

function parseSource(value: unknown): SourceReport {
  const raw = value as Record<string, unknown> | null;
  const outcome = member(
    raw?.outcome,
    ['read', 'absent', 'project_not_trusted', 'failed'] as const,
    'source',
  );
  const extra: Record<typeof outcome, string[]> = {
    read: ['hash', 'trust', 'followed_links', 'unrecognised'],
    absent: [],
    project_not_trusted: [],
    failed: ['failure'],
  };
  const source = record(
    value,
    [...SOURCE_COMMON, 'outcome', ...extra[outcome]],
    'source',
  );
  const common = {
    key: member(source.key, SOURCE_KEYS, 'source'),
    provider: member(source.provider, PROVIDERS, 'source'),
    scope: member(source.scope, SCOPES, 'source'),
    class: member(source.class, SOURCE_CLASSES, 'source'),
    not_writable: member(source.not_writable, NOT_WRITABLE, 'source'),
  };
  switch (outcome) {
    case 'read':
      return {
        ...common,
        outcome,
        hash: digest(source.hash, 'source'),
        trust: parseTrust(source.trust),
        followed_links: count(source.followed_links, 'source'),
        unrecognised: count(source.unrecognised, 'source'),
      };
    case 'failed':
      return { ...common, outcome, failure: parseFailure(source.failure) };
    default:
      return { ...common, outcome };
  }
}

function parseProjection(value: unknown, depth: number): Projection {
  if (depth > MAX_DEPTH) throw invalid('detail');
  const raw = value as Record<string, unknown> | null;
  const kind = member(
    raw?.kind,
    ['null', 'bool', 'number', 'text', 'hidden', 'list', 'map'] as const,
    'detail',
  );
  if (kind === 'null') {
    record(value, ['kind'], 'detail');
    return { kind };
  }
  const body = record(value, ['kind', 'value'], 'detail');
  switch (kind) {
    case 'bool':
      return { kind, value: flag(body.value, 'detail') };
    case 'number':
      if (typeof body.value !== 'number' || !Number.isFinite(body.value))
        throw invalid('detail');
      return { kind, value: body.value };
    case 'text': {
      const t = record(body.value, ['text', 'found'], 'detail');
      return {
        kind,
        value: {
          text: text(t.text, 'detail'),
          found: list(t.found, 'detail').map((c) =>
            member(c, SECRET_CLASSES, 'detail'),
          ),
        },
      };
    }
    case 'hidden':
      return { kind, value: member(body.value, SECRET_CLASSES, 'detail') };
    case 'list':
      return {
        kind,
        value: list(body.value, 'detail').map((v) =>
          parseProjection(v, depth + 1),
        ),
      };
    default:
      return {
        kind: 'map',
        value: list(body.value, 'detail').map((entry) => {
          const pair = list(entry, 'detail');
          if (pair.length !== 2) throw invalid('detail');
          return [text(pair[0], 'detail'), parseProjection(pair[1], depth + 1)];
        }),
      };
  }
}

function parseItem(value: unknown): ConfigItem {
  const item = record(
    value,
    [
      'id',
      'provider',
      'source',
      'scope',
      'kind',
      'name',
      'persisted',
      'effective',
      'session_use',
      'not_writable',
      'detail',
      'hash',
    ],
    'item',
  );
  return {
    id: text(item.id, 'item'),
    provider: member(item.provider, PROVIDERS, 'item'),
    source: member(item.source, SOURCE_KEYS, 'item'),
    scope: member(item.scope, SCOPES, 'item'),
    kind: member(item.kind, ITEM_KINDS, 'item'),
    name: text(item.name, 'item'),
    persisted: member(item.persisted, PERSISTED, 'item'),
    effective: member(item.effective, EFFECTIVE, 'item'),
    session_use: member(item.session_use, SESSION_USE, 'item'),
    not_writable: member(item.not_writable, NOT_WRITABLE, 'item'),
    detail: item.detail === null ? null : parseProjection(item.detail, 0),
    hash: item.hash === null ? null : digest(item.hash, 'item'),
  };
}

export function parseInventoryReply(value: unknown): InventoryReply {
  const reply = record(value, ['revision', 'inventory'], 'reply');
  const inventory = record(
    reply.inventory,
    ['sources', 'items', 'truncated'],
    'inventory',
  );
  return {
    revision: count(reply.revision, 'reply'),
    inventory: {
      sources: list(inventory.sources, 'inventory').map(parseSource),
      items: list(inventory.items, 'inventory').map(parseItem),
      truncated: flag(inventory.truncated, 'inventory'),
    },
  };
}

export function parseChanges(value: unknown): Changes {
  const raw = value as Record<string, unknown> | null;
  const result = member(
    raw?.result,
    ['same', 'changed', 'resync'] as const,
    'changes',
  );
  if (result === 'changed') {
    const changed = record(value, ['result', 'revision', 'sources'], 'changes');
    return {
      result,
      revision: count(changed.revision, 'changes'),
      sources: list(changed.sources, 'changes').map((s) =>
        member(s, SOURCE_KEYS, 'changes'),
      ),
    };
  }
  const other = record(value, ['result', 'revision'], 'changes');
  return { result, revision: count(other.revision, 'changes') };
}

/** What a failed call came to. Anything but the desktop's own word is `internal`. */
export function parseConfigurationFailure(
  error: unknown,
): ConfigurationFailure {
  return error === 'unavailable' ? 'unavailable' : 'internal';
}
