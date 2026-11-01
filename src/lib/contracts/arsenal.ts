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
  | 'supported'
  | 'unsupported'
  | 'unverified'
  | 'requires_qualification'
  | 'conflicting'
  | 'missing_dependency';
/** A compatibility word with the sentence that justifies it. */
export interface Verdict {
  status: Compatibility;
  reason: string;
}
export type DependencyKind = 'tool' | 'mcp_server' | 'file';
export type DependencyState = 'declared' | 'present' | 'missing' | 'unknown';
/** Something a capability's files declare or point at. Read, never resolved by running it. */
export interface Dependency {
  kind: DependencyKind;
  name: string;
  state: DependencyState;
}
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

/** A library entry with what its format means for its own provider. */
export interface LibraryItem extends Summary {
  compatibility: Verdict;
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
  compatibility: Verdict;
  dependencies: Dependency[];
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
  /** Keys of installed composites whose members stay exposed. */
  composites: string[];
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

export type CompositeKind = 'gsd_core' | 'claude_plugin' | 'codex_plugin';
export interface Composite {
  key: string;
  kind: CompositeKind;
  provider: ArsenalProvider | 'shared';
  name: string;
  version: string;
  description: string;
  artifacts: number;
  /** How many of this scan's entries are installed from it. */
  members: number;
  origin: string;
  enabled: boolean | null;
  methodology: boolean;
  skills: string[];
  agents: string[];
  commands: number;
  hooks: boolean;
  mcp: boolean;
  categories: [string, number][];
  limitations: string[];
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
  dependencies: Dependency[];
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
  /** `partial` when the reading budget ran out before every item's files were read. */
  dependencies: 'analysed' | 'partial';
}

export type EnforcementState =
  'available' | 'unqualified' | 'version_unknown' | 'incomplete_inventory';
export type PlanItemState = 'prepared' | 'rejected' | 'unsupported' | 'missing';
export type PlanStatus = 'ready' | 'partial' | 'blocked';
export type PlanConflictCode =
  | 'name_collision'
  | 'duplicate_content'
  | 'instruction_overlap'
  | 'methodology_conflict'
  | 'scope_shadow';

export interface PlanItem {
  capability: string;
  name: string;
  kind: string;
  provider: string;
  revision: number | null;
  pinned: boolean;
  tree_hash: string | null;
  state: PlanItemState;
  reason: string;
  compatibility: Verdict;
  dependencies: Dependency[];
}

/** The configuration one terminal would start with, computed now. Never a claim that it is active. */
export interface TerminalPlan {
  selection: string;
  selection_name: string;
  provider: ArsenalProvider;
  provider_version: string | null;
  project: string | null;
  status: PlanStatus;
  can_apply: boolean;
  enforcement: {
    state: EnforcementState;
    reason: string;
    evidence: { qualified_version: string; checked_on: string } | null;
  };
  items: PlanItem[];
  kept: string[];
  excluded: string[];
  /** Kept skills the provider's own configuration already switches off; they stay off. */
  inherited_off: string[];
  /** Codex: the SKILL.md of each excluded skill, which is what its switch names. */
  excluded_files: string[];
  untouched: string[];
  composites: {
    key: string;
    name: string;
    kind: CompositeKind;
    kept: boolean;
    methodology: boolean;
    enabled: boolean | null;
    skills: number;
  }[];
  conflicts: { code: PlanConflictCode; names: string[]; blocking: boolean }[];
  warnings: string[];
  overlay: string | null;
  config_hash: string;
}

/** What a terminal started with, read from the host's own record right after the start. */
export interface Binding {
  session_id: string;
  selection_id: string;
  selection_name: string;
  provider: ArsenalProvider;
  provider_version: string | null;
  project: string;
  config_hash: string;
  state: 'applied' | 'not_applied';
  reason: string;
  plan: TerminalPlan | null;
  created_at: number;
  /** No provider listing is read back yet, so nothing is ever `verified`. */
  verification: 'not_observed';
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
  'unsupported',
  'unverified',
  'requires_qualification',
  'conflicting',
  'missing_dependency',
] as const;
const DEPENDENCY_KINDS = ['tool', 'mcp_server', 'file'] as const;
const DEPENDENCY_STATES = [
  'declared',
  'present',
  'missing',
  'unknown',
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

function parseVerdict(value: unknown): Verdict {
  const v = record(value, 'compatibility');
  return {
    status: oneOf(v.status, COMPATIBILITIES, 'status'),
    reason: text(v.reason, 'reason'),
  };
}

function parseDependency(value: unknown): Dependency {
  const v = record(value, 'dependency');
  return {
    kind: oneOf(v.kind, DEPENDENCY_KINDS, 'kind'),
    name: text(v.name, 'name'),
    state: oneOf(v.state, DEPENDENCY_STATES, 'state'),
  };
}

export function parseLibraryItem(value: unknown): LibraryItem {
  const v = record(value, 'library item');
  return { ...parseSummary(v), compatibility: parseVerdict(v.compatibility) };
}

export function parseSummaries(value: unknown): LibraryItem[] {
  return list(value, 'capability list', parseLibraryItem);
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
    compatibility: parseVerdict(v.compatibility),
    dependencies: list(v.dependencies, 'dependencies', parseDependency),
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
    composites: list(v.composites, 'composites', (key) =>
      text(key, 'composite'),
    ),
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

const COMPOSITE_KINDS = ['gsd_core', 'claude_plugin', 'codex_plugin'] as const;
const strings = (value: unknown, what: string): string[] =>
  list(value, what, (item) => text(item, what));

function parseComposite(value: unknown): Composite {
  const v = record(value, 'composite');
  return {
    key: text(v.key, 'key'),
    kind: oneOf(v.kind, COMPOSITE_KINDS, 'kind'),
    provider: oneOf(
      v.provider,
      ['claude_code', 'codex', 'shared'] as const,
      'provider',
    ),
    name: text(v.name, 'name'),
    version: text(v.version, 'version'),
    description: text(v.description, 'description'),
    artifacts: integer(v.artifacts, 'artifacts'),
    members: integer(v.members, 'members'),
    origin: text(v.origin, 'origin'),
    enabled: nullable(v.enabled, (on) => flag(on, 'enabled')),
    methodology: flag(v.methodology, 'methodology'),
    skills: strings(v.skills, 'skills'),
    agents: strings(v.agents, 'agents'),
    commands: integer(v.commands, 'commands'),
    hooks: flag(v.hooks, 'hooks'),
    mcp: flag(v.mcp, 'mcp'),
    categories: list(v.categories, 'categories', (pair) => {
      if (!Array.isArray(pair) || pair.length !== 2)
        throw new ArsenalShapeError('category');
      return [text(pair[0], 'category'), integer(pair[1], 'count')] as [
        string,
        number,
      ];
    }),
    limitations: strings(v.limitations, 'limitations'),
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
        dependencies: list(i.dependencies, 'dependencies', parseDependency),
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
      ['analysed', 'partial'] as const,
      'dependencies',
    ),
  };
}

const ENFORCEMENT_STATES = [
  'available',
  'unqualified',
  'version_unknown',
  'incomplete_inventory',
] as const;
const PLAN_ITEM_STATES = [
  'prepared',
  'rejected',
  'unsupported',
  'missing',
] as const;
const PLAN_CONFLICTS = [
  'name_collision',
  'duplicate_content',
  'instruction_overlap',
  'methodology_conflict',
  'scope_shadow',
] as const;

export function parseTerminalPlan(value: unknown): TerminalPlan {
  const v = record(value, 'terminal plan');
  const e = record(v.enforcement, 'enforcement');
  return {
    selection: text(v.selection, 'selection'),
    selection_name: text(v.selection_name, 'selection_name'),
    provider: provider(v.provider, 'provider'),
    provider_version: nullable(v.provider_version, (t) => text(t, 'version')),
    project: nullable(v.project, (t) => text(t, 'project')),
    status: oneOf(v.status, ['ready', 'partial', 'blocked'] as const, 'status'),
    can_apply: flag(v.can_apply, 'can_apply'),
    enforcement: {
      state: oneOf(e.state, ENFORCEMENT_STATES, 'enforcement state'),
      reason: text(e.reason, 'enforcement reason'),
      evidence: nullable(e.evidence, (evidence) => {
        const x = record(evidence, 'evidence');
        return {
          qualified_version: text(x.qualified_version, 'qualified_version'),
          checked_on: text(x.checked_on, 'checked_on'),
        };
      }),
    },
    items: list(v.items, 'plan items', (item) => {
      const i = record(item, 'plan item');
      return {
        capability: text(i.capability, 'capability'),
        name: text(i.name, 'name'),
        kind: text(i.kind, 'kind'),
        provider: text(i.provider, 'provider'),
        revision: nullable(i.revision, (n) => integer(n, 'revision')),
        pinned: flag(i.pinned, 'pinned'),
        tree_hash: nullable(i.tree_hash, (t) => text(t, 'tree_hash')),
        state: oneOf(i.state, PLAN_ITEM_STATES, 'state'),
        reason: text(i.reason, 'reason'),
        compatibility: parseVerdict(i.compatibility),
        dependencies: list(i.dependencies, 'dependencies', parseDependency),
      };
    }),
    kept: strings(v.kept, 'kept'),
    excluded: strings(v.excluded, 'excluded'),
    inherited_off: strings(v.inherited_off, 'inherited_off'),
    excluded_files: strings(v.excluded_files, 'excluded_files'),
    untouched: strings(v.untouched, 'untouched'),
    composites: list(v.composites, 'plan composites', (item) => {
      const c = record(item, 'plan composite');
      return {
        key: text(c.key, 'key'),
        name: text(c.name, 'name'),
        kind: oneOf(c.kind, COMPOSITE_KINDS, 'kind'),
        kept: flag(c.kept, 'kept'),
        methodology: flag(c.methodology, 'methodology'),
        enabled: nullable(c.enabled, (on) => flag(on, 'enabled')),
        skills: integer(c.skills, 'skills'),
      };
    }),
    conflicts: list(v.conflicts, 'plan conflicts', (item) => {
      const c = record(item, 'plan conflict');
      return {
        code: oneOf(c.code, PLAN_CONFLICTS, 'code'),
        names: strings(c.names, 'names'),
        blocking: flag(c.blocking, 'blocking'),
      };
    }),
    warnings: strings(v.warnings, 'warnings'),
    overlay: nullable(v.overlay, (t) => text(t, 'overlay')),
    config_hash: text(v.config_hash, 'config_hash'),
  };
}

export function parseBinding(value: unknown): Binding {
  const v = record(value, 'binding');
  return {
    session_id: text(v.session_id, 'session_id'),
    selection_id: text(v.selection_id, 'selection_id'),
    selection_name: text(v.selection_name, 'selection_name'),
    provider: provider(v.provider, 'provider'),
    provider_version: nullable(v.provider_version, (t) => text(t, 'version')),
    project: text(v.project, 'project'),
    config_hash: text(v.config_hash, 'config_hash'),
    state: oneOf(v.state, ['applied', 'not_applied'] as const, 'state'),
    reason: text(v.reason, 'reason'),
    // A record whose plan no longer parses still says what it was; its details are dropped.
    plan: (() => {
      try {
        return parseTerminalPlan(v.plan);
      } catch {
        return null;
      }
    })(),
    created_at: integer(v.created_at, 'created_at'),
    verification: oneOf(
      v.verification,
      ['not_observed'] as const,
      'verification',
    ),
  };
}

export function parseBindings(value: unknown): Binding[] {
  return list(value, 'bindings', parseBinding);
}

/** One thing synchronization reports: a conflict it settled, something refused or still waiting. */
export interface SyncNote {
  at: number;
  kind: string;
  name: string;
  detail: string;
}

/** Another computer, as far as this one can tell from the shared folder. */
export interface SyncPeer {
  device: string;
  host: string;
  /** When it last wrote a record or an acknowledgment; never proof that it is online. */
  last_heard: number | null;
  /** How many of this computer's records it acknowledged having applied, of how many there are. */
  applied_mine: number;
  published_mine: number;
}

/** Arsenal synchronization with the owner's other computers. */
export interface SyncStatus {
  enabled: boolean;
  folder: string;
  device: string;
  host: string;
  /** Bumped each time synchronization changed this library: the window reads it again. */
  generation: number;
  last_run: number | null;
  last_success: number | null;
  error: string | null;
  published: number;
  pending: SyncNote[];
  conflicts: SyncNote[];
  rejected: SyncNote[];
  /** Presets and selections deleted here because another computer deleted them; a copy is kept. */
  removed: SyncNote[];
  peers: SyncPeer[];
}

function parseSyncNote(value: unknown): SyncNote {
  const v = record(value, 'sync note');
  return {
    at: integer(v.at, 'at'),
    kind: text(v.kind, 'kind'),
    name: text(v.name, 'name'),
    detail: text(v.detail, 'detail'),
  };
}

function parseSyncPeer(value: unknown): SyncPeer {
  const v = record(value, 'sync peer');
  return {
    device: text(v.device, 'device'),
    host: text(v.host, 'host'),
    last_heard: nullable(v.last_heard, (n) => integer(n, 'last_heard')),
    applied_mine: integer(v.applied_mine, 'applied_mine'),
    published_mine: integer(v.published_mine, 'published_mine'),
  };
}

export function parseSyncStatus(value: unknown): SyncStatus {
  const v = record(value, 'sync status');
  return {
    enabled: flag(v.enabled, 'enabled'),
    folder: text(v.folder, 'folder'),
    device: text(v.device, 'device'),
    host: text(v.host, 'host'),
    generation: integer(v.generation, 'generation'),
    last_run: nullable(v.last_run, (n) => integer(n, 'last_run')),
    last_success: nullable(v.last_success, (n) => integer(n, 'last_success')),
    error: nullable(v.error, (n) => text(n, 'error')),
    published: integer(v.published, 'published'),
    pending: list(v.pending, 'pending', parseSyncNote),
    conflicts: list(v.conflicts, 'conflicts', parseSyncNote),
    rejected: list(v.rejected, 'rejected', parseSyncNote),
    removed: list(v.removed, 'removed', parseSyncNote),
    peers: list(v.peers, 'peers', parseSyncPeer),
  };
}
