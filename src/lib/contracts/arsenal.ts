// The shapes the Arsenal commands answer with. Nothing the desktop sends is believed until it
// has been checked here: a reply that does not match is an internal failure, never a partial view.

export const PROVIDERS = ['claude_code', 'codex'] as const;
export type ArsenalProvider = (typeof PROVIDERS)[number];

export type ImportOutcome = 'created' | 'new_revision' | 'unchanged';
/** What a discovered capability is. Only the first three can be copied into the library. */
export type CandidateKind =
  'skill' | 'agent' | 'rule' | 'hook' | 'mcp_server' | 'plugin';
export type CandidateStatus =
  'not_imported' | 'imported' | 'source_changed' | 'read_only';
export type CandidateScope = 'user' | 'project';
export type SourceState = 'read' | 'absent' | 'unreadable';
export type Compatibility =
  'supported' | 'partial' | 'unsupported' | 'unverified';
export type PreviewState = 'resolved' | 'missing' | 'revision_missing';
export type ConflictCode =
  'name_collision' | 'duplicate_content' | 'instruction_overlap';

export interface Summary {
  id: string;
  provider: ArsenalProvider;
  source: string;
  kind: string;
  name: string;
  description: string;
  origin: string;
  revision: number;
  revisions: number;
  tree_hash: string;
  updated_at: number;
}

export interface RevisionInfo {
  number: number;
  tree_hash: string;
  origin: string;
  created_at: number;
  files: number;
  bytes: number;
  current: boolean;
}

export interface FileInfo {
  path: string;
  sha256: string;
  size: number;
}

export interface Detail {
  summary: Summary;
  revision: number;
  revisions: RevisionInfo[];
  files: FileInfo[];
}

export interface FileText {
  path: string;
  text: string;
}

export interface Imported {
  capability: string;
  revision: number;
  outcome: ImportOutcome;
}

export interface Item {
  capability: string;
  revision: number | null;
}

export interface Preset {
  id: string;
  name: string;
  description: string;
  items: Item[];
  created_at: number;
  updated_at: number;
}

export interface Selection {
  id: string;
  name: string;
  provider: ArsenalProvider;
  items: Item[];
  created_at: number;
  updated_at: number;
}

export interface SourceReport {
  source: string;
  state: SourceState;
}

export interface Excluded {
  path: string;
  reason: string;
}

export interface FileDigest {
  path: string;
  size: number;
  sha256: string;
}

/** A composite collection a link is installed from, found through the link's target. */
export interface ManagedBy {
  composite: string;
  version: string;
  artifacts: number;
}

/** One discovered capability. `id` is what import names; nothing here is a path to act on. */
export interface Candidate {
  id: string;
  tree_hash: string;
  status: CandidateStatus;
  importable: boolean;
  compatibility: Compatibility;
  capability: string | null;
  duplicates: string[];
  name_collisions: string[];
  kind: CandidateKind;
  provider: ArsenalProvider;
  source: string;
  scope: CandidateScope;
  name: string;
  description: string;
  origin: string;
  executes_code: boolean;
  blocked: string | null;
  managed_by: ManagedBy | null;
  file_count: number;
  excluded_count: number;
  review_count: number;
}

/** What a chosen capability holds, read on selection from the last scan. */
export interface CandidateDetail {
  id: string;
  files: FileDigest[];
  excluded: Excluded[];
  review: Excluded[];
}

export interface Composite {
  name: string;
  version: string;
  artifacts: number;
  /** How many of this scan's entries are installed from it. */
  members: number;
  origin: string;
}

export interface DiscoverReply {
  project: string | null;
  sources: SourceReport[];
  candidates: Candidate[];
  composites: Composite[];
  truncated: boolean;
}

export interface PreviewItem {
  capability: string;
  state: PreviewState;
  kind: string;
  name: string;
  provider: string;
  revision: number | null;
  pinned: boolean;
  tree_hash: string | null;
  compatibility: Compatibility;
  reason: string;
}

export interface Conflict {
  code: ConflictCode;
  capabilities: string[];
}

export interface Preview {
  selection: Selection;
  status: 'not_applied';
  items: PreviewItem[];
  conflicts: Conflict[];
  dependencies: 'not_analysed';
}

/** The window's failure: a stable code, and for a blocked skill, the reason it is blocked. */
export interface ArsenalFailure {
  code: string;
  reason: string | null;
}

type Json = Record<string, unknown>;

export class ArsenalShapeError extends Error {
  constructor(what: string) {
    super(`arsenal: unexpected ${what}`);
  }
}

function record(value: unknown, what: string): Json {
  if (typeof value !== 'object' || value === null || Array.isArray(value))
    throw new ArsenalShapeError(what);
  return value as Json;
}

function text(value: unknown, what: string): string {
  if (typeof value !== 'string') throw new ArsenalShapeError(what);
  return value;
}

function integer(value: unknown, what: string): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value))
    throw new ArsenalShapeError(what);
  return value;
}

function flag(value: unknown, what: string): boolean {
  if (typeof value !== 'boolean') throw new ArsenalShapeError(what);
  return value;
}

function list<T>(
  value: unknown,
  what: string,
  each: (item: unknown) => T,
): T[] {
  if (!Array.isArray(value)) throw new ArsenalShapeError(what);
  return value.map(each);
}

function oneOf<T extends string>(
  value: unknown,
  allowed: readonly T[],
  what: string,
): T {
  if (
    typeof value !== 'string' ||
    !(allowed as readonly string[]).includes(value)
  )
    throw new ArsenalShapeError(what);
  return value as T;
}

function nullable<T>(value: unknown, each: (item: unknown) => T): T | null {
  return value === null || value === undefined ? null : each(value);
}

function provider(value: unknown, what: string): ArsenalProvider {
  return oneOf(value, PROVIDERS, what);
}

const CANDIDATE_KINDS = [
  'skill',
  'agent',
  'rule',
  'hook',
  'mcp_server',
  'plugin',
] as const;
const CANDIDATE_STATUSES = [
  'not_imported',
  'imported',
  'source_changed',
  'read_only',
] as const;
const CANDIDATE_SCOPES = ['user', 'project'] as const;
const SOURCE_STATES = ['read', 'absent', 'unreadable'] as const;
const COMPATIBILITIES = [
  'supported',
  'partial',
  'unsupported',
  'unverified',
] as const;
const PREVIEW_STATES = ['resolved', 'missing', 'revision_missing'] as const;
const CONFLICT_CODES = [
  'name_collision',
  'duplicate_content',
  'instruction_overlap',
] as const;
const OUTCOMES = ['created', 'new_revision', 'unchanged'] as const;

export function parseSummary(value: unknown): Summary {
  const v = record(value, 'capability');
  return {
    id: text(v.id, 'id'),
    provider: provider(v.provider, 'provider'),
    source: text(v.source, 'source'),
    kind: text(v.kind, 'kind'),
    name: text(v.name, 'name'),
    description: text(v.description, 'description'),
    origin: text(v.origin, 'origin'),
    revision: integer(v.revision, 'revision'),
    revisions: integer(v.revisions, 'revisions'),
    tree_hash: text(v.tree_hash, 'tree_hash'),
    updated_at: integer(v.updated_at, 'updated_at'),
  };
}

export function parseSummaries(value: unknown): Summary[] {
  return list(value, 'capability list', parseSummary);
}

function parseRevisionInfo(value: unknown): RevisionInfo {
  const v = record(value, 'revision');
  return {
    number: integer(v.number, 'number'),
    tree_hash: text(v.tree_hash, 'tree_hash'),
    origin: text(v.origin, 'origin'),
    created_at: integer(v.created_at, 'created_at'),
    files: integer(v.files, 'files'),
    bytes: integer(v.bytes, 'bytes'),
    current: flag(v.current, 'current'),
  };
}

function parseFileInfo(value: unknown): FileInfo {
  const v = record(value, 'file');
  return {
    path: text(v.path, 'path'),
    sha256: text(v.sha256, 'sha256'),
    size: integer(v.size, 'size'),
  };
}

export function parseDetail(value: unknown): Detail {
  const v = record(value, 'detail');
  return {
    summary: parseSummary(v.summary),
    revision: integer(v.revision, 'revision'),
    revisions: list(v.revisions, 'revisions', parseRevisionInfo),
    files: list(v.files, 'files', parseFileInfo),
  };
}

export function parseFileText(value: unknown): FileText {
  const v = record(value, 'file text');
  return { path: text(v.path, 'path'), text: text(v.text, 'text') };
}

export function parseImported(value: unknown): Imported {
  const v = record(value, 'import reply');
  return {
    capability: text(v.capability, 'capability'),
    revision: integer(v.revision, 'revision'),
    outcome: oneOf(v.outcome, OUTCOMES, 'outcome'),
  };
}

function parseItem(value: unknown): Item {
  const v = record(value, 'item');
  return {
    capability: text(v.capability, 'capability'),
    revision: nullable(v.revision, (number) => integer(number, 'revision')),
  };
}

export function parsePreset(value: unknown): Preset {
  const v = record(value, 'preset');
  return {
    id: text(v.id, 'id'),
    name: text(v.name, 'name'),
    description: text(v.description, 'description'),
    items: list(v.items, 'items', parseItem),
    created_at: integer(v.created_at, 'created_at'),
    updated_at: integer(v.updated_at, 'updated_at'),
  };
}

export function parsePresets(value: unknown): Preset[] {
  return list(value, 'preset list', parsePreset);
}

export function parseSelection(value: unknown): Selection {
  const v = record(value, 'selection');
  return {
    id: text(v.id, 'id'),
    name: text(v.name, 'name'),
    provider: provider(v.provider, 'provider'),
    items: list(v.items, 'items', parseItem),
    created_at: integer(v.created_at, 'created_at'),
    updated_at: integer(v.updated_at, 'updated_at'),
  };
}

export function parseSelections(value: unknown): Selection[] {
  return list(value, 'selection list', parseSelection);
}

function parseExcluded(value: unknown): Excluded {
  const e = record(value, 'excluded entry');
  return { path: text(e.path, 'path'), reason: text(e.reason, 'reason') };
}

function parseManagedBy(value: unknown): ManagedBy | null {
  return nullable(value, (managed) => {
    const m = record(managed, 'managed by');
    return {
      composite: text(m.composite, 'composite'),
      version: text(m.version, 'version'),
      artifacts: integer(m.artifacts, 'artifacts'),
    };
  });
}

function parseCandidate(value: unknown): Candidate {
  const v = record(value, 'candidate');
  return {
    id: text(v.id, 'id'),
    tree_hash: text(v.tree_hash, 'tree_hash'),
    status: oneOf(v.status, CANDIDATE_STATUSES, 'status'),
    importable: flag(v.importable, 'importable'),
    compatibility: oneOf(v.compatibility, COMPATIBILITIES, 'compatibility'),
    capability: nullable(v.capability, (id) => text(id, 'capability')),
    duplicates: list(v.duplicates, 'duplicates', (id) => text(id, 'duplicate')),
    name_collisions: list(v.name_collisions, 'name_collisions', (id) =>
      text(id, 'name collision'),
    ),
    kind: oneOf(v.kind, CANDIDATE_KINDS, 'kind'),
    provider: provider(v.provider, 'provider'),
    source: text(v.source, 'source'),
    scope: oneOf(v.scope, CANDIDATE_SCOPES, 'scope'),
    name: text(v.name, 'name'),
    description: text(v.description, 'description'),
    origin: text(v.origin, 'origin'),
    executes_code: flag(v.executes_code, 'executes_code'),
    blocked: nullable(v.blocked, (reason) => text(reason, 'blocked')),
    managed_by: parseManagedBy(v.managed_by),
    file_count: integer(v.file_count, 'file_count'),
    excluded_count: integer(v.excluded_count, 'excluded_count'),
    review_count: integer(v.review_count, 'review_count'),
  };
}

function parseFileDigest(value: unknown): FileDigest {
  const f = record(value, 'file digest');
  return {
    path: text(f.path, 'path'),
    size: integer(f.size, 'size'),
    sha256: text(f.sha256, 'sha256'),
  };
}

export function parseCandidateDetail(value: unknown): CandidateDetail {
  const v = record(value, 'candidate detail');
  return {
    id: text(v.id, 'id'),
    files: list(v.files, 'files', parseFileDigest),
    excluded: list(v.excluded, 'excluded', parseExcluded),
    review: list(v.review, 'review', parseExcluded),
  };
}

function parseComposite(value: unknown): Composite {
  const v = record(value, 'composite');
  return {
    name: text(v.name, 'name'),
    version: text(v.version, 'version'),
    artifacts: integer(v.artifacts, 'artifacts'),
    members: integer(v.members, 'members'),
    origin: text(v.origin, 'origin'),
  };
}

export function parseDiscover(value: unknown): DiscoverReply {
  const v = record(value, 'discovery');
  return {
    project: nullable(v.project, (name) => text(name, 'project')),
    sources: list(v.sources, 'sources', (source) => {
      const s = record(source, 'source');
      return {
        source: text(s.source, 'source'),
        state: oneOf(s.state, SOURCE_STATES, 'state'),
      };
    }),
    candidates: list(v.candidates, 'candidates', parseCandidate),
    composites: list(v.composites, 'composites', parseComposite),
    truncated: flag(v.truncated, 'truncated'),
  };
}

export function parseImportFailure(value: unknown): ArsenalFailure {
  const v = record(value, 'failure');
  return {
    code: text(v.code, 'code'),
    reason: nullable(v.reason, (reason) => text(reason, 'reason')),
  };
}

export function parsePreview(value: unknown): Preview {
  const v = record(value, 'preview');
  return {
    selection: parseSelection(v.selection),
    status: oneOf(v.status, ['not_applied'] as const, 'status'),
    items: list(v.items, 'items', (item) => {
      const i = record(item, 'preview item');
      return {
        capability: text(i.capability, 'capability'),
        state: oneOf(i.state, PREVIEW_STATES, 'state'),
        kind: text(i.kind, 'kind'),
        name: text(i.name, 'name'),
        provider: text(i.provider, 'provider'),
        revision: nullable(i.revision, (number) => integer(number, 'revision')),
        pinned: flag(i.pinned, 'pinned'),
        tree_hash: nullable(i.tree_hash, (hash) => text(hash, 'tree_hash')),
        compatibility: oneOf(i.compatibility, COMPATIBILITIES, 'compatibility'),
        reason: text(i.reason, 'reason'),
      };
    }),
    conflicts: list(v.conflicts, 'conflicts', (conflict) => {
      const c = record(conflict, 'conflict');
      return {
        code: oneOf(c.code, CONFLICT_CODES, 'code'),
        capabilities: list(c.capabilities, 'capabilities', (id) =>
          text(id, 'capability'),
        ),
      };
    }),
    dependencies: oneOf(
      v.dependencies,
      ['not_analysed'] as const,
      'dependencies',
    ),
  };
}
