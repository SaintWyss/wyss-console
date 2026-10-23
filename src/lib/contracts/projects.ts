/**
 * The project-centered workspace (M1): what the desktop says about projects, workspaces and
 * the sessions they show, and why a change was refused. Every answer is checked against this
 * shape and nothing that does not fit is believed.
 *
 * Nothing here is a path, a command line, an environment, a prompt or a credential: a folder
 * is a name and a one-use token, a change is the closed envelope below, and a Managed session
 * is shown with the profile that was *requested* beside the one the authority *attached*.
 */
import type { Node } from '../features/terminal/layout';

export const ERROR_CODES = [
  'unsupported_schema',
  'invalid_record',
  'capacity_exceeded',
  'revision_conflict',
  'context_stale',
  'leaf_busy',
  'root_missing',
  'root_identity_changed',
  'root_ineligible',
  'host_offline',
  'host_changed',
  'unsupported_capability',
  'profile_unavailable',
  'owner_refused',
  'effect_ambiguous',
  'io_failure',
] as const;
export type ErrorCode = (typeof ERROR_CODES)[number];
export const EFFECTS = ['none', 'committed', 'unknown'] as const;
export type Effect = (typeof EFFECTS)[number];
export const NEXT_ACTIONS = [
  'refetch',
  'retry',
  'choose_again',
  'reconcile',
  'open_recovery',
  'none',
] as const;
export type NextAction = (typeof NEXT_ACTIONS)[number];

export type SafetyProfile = 'inference' | 'local_tools';
export const SAFETY_PROFILES: readonly SafetyProfile[] = [
  'inference',
  'local_tools',
];
export type Provider = 'claude' | 'codex';
export type LaunchProvider = Provider | 'shell';
export type Mode = 'native' | 'managed';

export interface ProfileRef {
  id: string;
  version: string;
}
export interface Preferences {
  provider: Provider | null;
  mode: Mode;
  nativeProfile: ProfileRef | null;
  managedSafetyProfile: SafetyProfile | null;
}

export type StoreStatus =
  | { state: 'missing' }
  | {
      state: 'ready';
      revision: string;
      hostId: string;
      storeId: string;
      scratchWorkspaceId: string;
    }
  | { state: 'unusable'; code: ErrorCode; field: string }
  | { state: 'unavailable' };

export interface HostStatus {
  reachable: boolean;
  epoch: string | null;
  features: string[];
  reason: string | null;
}
export interface Overview {
  enabled: boolean;
  store: StoreStatus;
  host: HostStatus;
}

export interface RootView {
  bindingId: string;
  bindingRevision: string;
  label: string;
  state: 'valid' | 'missing' | 'identity_changed' | 'unverified' | 'retired';
  availability:
    'available' | 'missing' | 'identity_changed' | 'ineligible' | 'unknown';
  nativeEligible: boolean;
  configEligible: boolean;
}
export interface ProjectView {
  id: string;
  label: string;
  archived: boolean;
  tombstone: boolean;
  revision: string;
  defaultWorkspaceId: string | null;
  root: RootView | null;
  workspaceIds: string[];
  sessions: number;
}

export type SessionKind = 'native' | 'shell' | 'managed';
export type Availability = 'live' | 'ended' | 'unknown';
export interface SessionView {
  id: string;
  originProjectId: string | null;
  state: 'attached' | 'detached' | 'unavailable' | 'ambiguous';
  kind: SessionKind;
  availability: Availability;
  reason: string | null;
  nativeSessionId: string | null;
  terminalId: string | null;
  protocol: 1 | 2 | null;
  storeStreamId: string | null;
  managedSessionId: string | null;
  requestedSafetyProfile: SafetyProfile | null;
  effectiveSafetyProfile: SafetyProfile | null;
  profileMismatch: boolean;
  operationId: string | null;
  input: 'this' | 'other' | 'none';
}
export interface LeafView {
  id: string;
  title: string;
  session: SessionView | null;
}
export interface Unfinished {
  operationId: string;
  leafId: string;
  state: 'pending' | 'ambiguous';
  /** Only a Managed launch may ask its owner again; a Native one is resolved by looking. */
  mode: 'native' | 'managed';
}
/** A running terminal no association holds: what an unknown launch could have started. */
export interface Orphan {
  terminalId: string;
  nativeSessionId: string | null;
  folder: string | null;
}
export interface WorkspaceView {
  id: string;
  projectId: string | null;
  label: string;
  revision: string;
  pinned: boolean;
  layout: Node<string> | null;
  focus: string | null;
  leaves: LeafView[];
  preferences: Preferences;
  unfinished: Unfinished[];
}
export interface Page<T> {
  items: T[];
  cursor: string | null;
  storeRevision: string;
}
export interface PickView {
  token: string;
  label: string;
  nativeEligible: boolean;
  configEligible: boolean;
}

export interface Context {
  hostId: string;
  projectId: string | null;
  workspaceId: string | null;
  bindingId: string | null;
  bindingRevision: string | null;
  selectionRevision: string;
}
export interface Receipt {
  operationId: string;
  context: Context;
  storeRevision: string;
  created: { role: string; id: string }[];
  replayed: boolean;
}
export interface Failure {
  code: ErrorCode;
  effect: Effect;
  nextAction: NextAction;
  operationId: string | null;
  context: Context | null;
  field: string | null;
}

export interface Conflict {
  kind: string;
  count: number;
}
export interface MigrationSummary {
  previewId: string;
  mode: 'initial' | 'incremental';
  counts: {
    projects: number;
    workspaces: number;
    sessions: number;
    placedLeaves: number;
    emptyLeaves: number;
    tabsSkipped: number;
  };
  conflicts: Conflict[];
  sources: { kind: string; digest: string }[];
  canCommit: boolean;
  needsAliasAcceptance: boolean;
  olderChanged: boolean;
}
export interface RollbackReport {
  canDisable: boolean;
  unfinishedLaunches: number;
  projects: number;
  workspaces: number;
  sessions: number;
  mappedOlderRecords: number;
  note: string;
}

// ---- the commands the window may send ---------------------------------------------------

export interface LaunchChoice {
  workspaceId: string;
  leafId: string;
  provider: LaunchProvider;
  mode: Mode;
  nativeProfile: ProfileRef | null;
  managedSafetyProfile: SafetyProfile | null;
}
export interface SavedWorkspace {
  workspaceId: string;
  expectedWorkspaceRevision: string;
  label: string;
  layout: Node<string> | null;
  focus: string | null;
  pinned: boolean;
  leaves: { id: string; title: string; associationId: string | null }[];
  preferences: Preferences;
}
export type Resolution =
  | { kind: 'identified'; terminalId: string; sessionId: string | null }
  | { kind: 'absent' }
  | { kind: 'retryManaged' };

export type Command =
  | { kind: 'registerExisting'; selectionToken: string }
  | { kind: 'createFolder'; selectionToken: string; name: string }
  | {
      kind: 'relink';
      projectId: string;
      oldBindingId: string;
      selectionToken: string;
    }
  | { kind: 'renameProject'; projectId: string; label: string }
  | { kind: 'archiveProject'; projectId: string; archived: boolean }
  | { kind: 'removeProject'; projectId: string; retainHistory: boolean }
  | { kind: 'createWorkspace'; projectId: string; label: string }
  | { kind: 'duplicateWorkspace'; sourceWorkspaceId: string; label: string }
  | ({ kind: 'saveWorkspace' } & SavedWorkspace)
  | { kind: 'closeWorkspace'; workspaceId: string }
  | { kind: 'deleteWorkspace'; workspaceId: string }
  | {
      kind: 'attach' | 'detach';
      workspaceId: string;
      leafId: string;
      associationId: string;
    }
  | {
      kind: 'adoptScratch';
      associationId: string;
      projectId: string;
      bindingId: string;
    }
  | ({ kind: 'launch' } & LaunchChoice)
  | {
      kind: 'reconcileLaunch';
      launchOperationId: string;
      resolution: Resolution;
    };

export interface Envelope {
  schema: 1;
  operationId: string;
  expectedStoreRevision: string;
  context: Context;
  command: Command;
}

// ---- reading what the desktop answers -----------------------------------------------------

type Rec = Record<string, unknown>;

function rec(value: unknown, what: string): Rec {
  if (typeof value !== 'object' || value === null || Array.isArray(value))
    throw new Error(`projects: ${what}`);
  return value as Rec;
}
function str(value: unknown, what: string, most = 4096): string {
  if (typeof value !== 'string' || value.length > most)
    throw new Error(`projects: ${what}`);
  return value;
}
function strOrNull(value: unknown, what: string): string | null {
  return value === null ? null : str(value, what);
}
function bool(value: unknown, what: string): boolean {
  if (typeof value !== 'boolean') throw new Error(`projects: ${what}`);
  return value;
}
function counter(value: unknown, what: string): string {
  const text = str(value, what, 20);
  if (!/^[1-9][0-9]*$/.test(text)) throw new Error(`projects: ${what}`);
  return text;
}
function oneOf<T extends string>(
  value: unknown,
  allowed: readonly T[],
  what: string,
): T {
  if (typeof value !== 'string' || !allowed.includes(value as T))
    throw new Error(`projects: ${what}`);
  return value as T;
}
function list<T>(value: unknown, what: string, item: (v: unknown) => T): T[] {
  if (!Array.isArray(value) || value.length > 4096)
    throw new Error(`projects: ${what}`);
  return value.map(item);
}
const SAFETY = ['inference', 'local_tools'] as const;
const safetyOrNull = (v: unknown, what: string) =>
  v === null ? null : oneOf(v, SAFETY, what);

function parseNode(value: unknown, level: number): Node<string> {
  const v = rec(value, 'node');
  if (level > 6) throw new Error('projects: node depth');
  if (v.kind === 'leaf') return { kind: 'leaf', id: str(v.id, 'leaf id', 36) };
  if (v.kind === 'split') {
    const ratio = v.ratio;
    if (typeof ratio !== 'number' || !Number.isFinite(ratio))
      throw new Error('projects: ratio');
    return {
      kind: 'split',
      dir: oneOf(v.dir, ['row', 'col'] as const, 'dir'),
      ratio,
      a: parseNode(v.a, level + 1),
      b: parseNode(v.b, level + 1),
    };
  }
  throw new Error('projects: node kind');
}

function parseProfileRef(value: unknown): ProfileRef | null {
  if (value === null) return null;
  const v = rec(value, 'profile');
  return { id: str(v.id, 'profile id', 32), version: counter(v.version, 'v') };
}
function parsePreferences(value: unknown): Preferences {
  const v = rec(value, 'preferences');
  return {
    provider:
      v.provider === null
        ? null
        : oneOf(v.provider, ['claude', 'codex'] as const, 'provider'),
    mode: oneOf(v.mode, ['native', 'managed'] as const, 'mode'),
    nativeProfile: parseProfileRef(v.nativeProfile),
    managedSafetyProfile: safetyOrNull(v.managedSafetyProfile, 'safety'),
  };
}

export function parseStoreStatus(value: unknown): StoreStatus {
  const v = rec(value, 'store');
  switch (v.state) {
    case 'missing':
      return { state: 'missing' };
    case 'unavailable':
      return { state: 'unavailable' };
    case 'ready':
      return {
        state: 'ready',
        revision: counter(v.revision, 'revision'),
        hostId: str(v.hostId, 'host', 36),
        storeId: str(v.storeId, 'store', 36),
        scratchWorkspaceId: str(v.scratchWorkspaceId, 'scratch', 36),
      };
    case 'unusable':
      return {
        state: 'unusable',
        code: oneOf(v.code, ERROR_CODES, 'code'),
        field: str(v.field, 'field', 64),
      };
    default:
      throw new Error('projects: store state');
  }
}

export function parseOverview(value: unknown): Overview {
  const v = rec(value, 'overview');
  const host = rec(v.host, 'host');
  return {
    enabled: bool(v.enabled, 'enabled'),
    store: parseStoreStatus(v.store),
    host: {
      reachable: bool(host.reachable, 'reachable'),
      epoch: strOrNull(host.epoch, 'epoch'),
      features: list(host.features, 'features', (f) => str(f, 'feature', 64)),
      reason: strOrNull(host.reason, 'reason'),
    },
  };
}

function parseRoot(value: unknown): RootView | null {
  if (value === null) return null;
  const v = rec(value, 'root');
  return {
    bindingId: str(v.bindingId, 'binding', 36),
    bindingRevision: counter(v.bindingRevision, 'binding revision'),
    label: str(v.label, 'label', 256),
    state: oneOf(
      v.state,
      [
        'valid',
        'missing',
        'identity_changed',
        'unverified',
        'retired',
      ] as const,
      'binding state',
    ),
    availability: oneOf(
      v.availability,
      [
        'available',
        'missing',
        'identity_changed',
        'ineligible',
        'unknown',
      ] as const,
      'availability',
    ),
    nativeEligible: bool(v.nativeEligible, 'native'),
    configEligible: bool(v.configEligible, 'config'),
  };
}

export function parseProject(value: unknown): ProjectView {
  const v = rec(value, 'project');
  return {
    id: str(v.id, 'id', 36),
    label: str(v.label, 'label', 256),
    archived: bool(v.archived, 'archived'),
    tombstone: bool(v.tombstone, 'tombstone'),
    revision: counter(v.revision, 'revision'),
    defaultWorkspaceId: strOrNull(v.defaultWorkspaceId, 'default'),
    root: parseRoot(v.root),
    workspaceIds: list(v.workspaceIds, 'workspaces', (w) => str(w, 'w', 36)),
    sessions: typeof v.sessions === 'number' ? v.sessions : 0,
  };
}

export function parseSession(value: unknown): SessionView {
  const v = rec(value, 'session');
  const protocol = v.protocol;
  if (protocol !== null && protocol !== 1 && protocol !== 2)
    throw new Error('projects: protocol');
  return {
    id: str(v.id, 'id', 36),
    originProjectId: strOrNull(v.originProjectId, 'origin'),
    state: oneOf(
      v.state,
      ['attached', 'detached', 'unavailable', 'ambiguous'] as const,
      'state',
    ),
    kind: oneOf(v.kind, ['native', 'shell', 'managed'] as const, 'kind'),
    availability: oneOf(
      v.availability,
      ['live', 'ended', 'unknown'] as const,
      'availability',
    ),
    reason: strOrNull(v.reason, 'reason'),
    nativeSessionId: strOrNull(v.nativeSessionId, 'native'),
    terminalId: strOrNull(v.terminalId, 'terminal'),
    protocol,
    storeStreamId: strOrNull(v.storeStreamId, 'stream'),
    managedSessionId: strOrNull(v.managedSessionId, 'managed'),
    requestedSafetyProfile: safetyOrNull(v.requestedSafetyProfile, 'requested'),
    effectiveSafetyProfile: safetyOrNull(v.effectiveSafetyProfile, 'effective'),
    profileMismatch: bool(v.profileMismatch, 'mismatch'),
    operationId: strOrNull(v.operationId, 'operation'),
    input: oneOf(v.input, ['this', 'other', 'none'] as const, 'input'),
  };
}

export function parseWorkspace(value: unknown): WorkspaceView {
  const v = rec(value, 'workspace');
  return {
    id: str(v.id, 'id', 36),
    projectId: strOrNull(v.projectId, 'project'),
    label: str(v.label, 'label', 256),
    revision: counter(v.revision, 'revision'),
    pinned: bool(v.pinned, 'pinned'),
    layout: v.layout === null ? null : parseNode(v.layout, 0),
    focus: strOrNull(v.focus, 'focus'),
    leaves: list(v.leaves, 'leaves', (l) => {
      const leaf = rec(l, 'leaf');
      return {
        id: str(leaf.id, 'id', 36),
        title: str(leaf.title, 'title', 256),
        session: leaf.session === null ? null : parseSession(leaf.session),
      };
    }),
    preferences: parsePreferences(v.preferences),
    unfinished: list(v.unfinished, 'unfinished', (u) => {
      const o = rec(u, 'unfinished');
      return {
        operationId: str(o.operationId, 'op', 36),
        leafId: str(o.leafId, 'leaf', 36),
        state: oneOf(o.state, ['pending', 'ambiguous'] as const, 'state'),
        mode: oneOf(o.mode, ['native', 'managed'] as const, 'mode'),
      };
    }),
  };
}

export function parseOrphans(value: unknown): Orphan[] {
  return list(value, 'orphans', (item) => {
    const o = rec(item, 'orphan');
    return {
      terminalId: str(o.terminalId, 'terminal', 36),
      nativeSessionId: strOrNull(o.nativeSessionId, 'native'),
      folder: strOrNull(o.folder, 'folder'),
    };
  });
}

export function parsePage<T>(value: unknown, item: (v: unknown) => T): Page<T> {
  const v = rec(value, 'page');
  return {
    items: list(v.items, 'items', item),
    cursor: strOrNull(v.cursor, 'cursor'),
    storeRevision: counter(v.storeRevision, 'revision'),
  };
}

export function parsePick(value: unknown): PickView {
  const v = rec(value, 'pick');
  return {
    token: str(v.token, 'token', 128),
    label: str(v.label, 'label', 256),
    nativeEligible: bool(v.nativeEligible, 'native'),
    configEligible: bool(v.configEligible, 'config'),
  };
}

function parseContext(value: unknown): Context {
  const v = rec(value, 'context');
  return {
    hostId: str(v.hostId, 'host', 36),
    projectId: strOrNull(v.projectId, 'project'),
    workspaceId: strOrNull(v.workspaceId, 'workspace'),
    bindingId: strOrNull(v.bindingId, 'binding'),
    bindingRevision:
      v.bindingRevision === null ? null : counter(v.bindingRevision, 'rev'),
    selectionRevision: counter(v.selectionRevision, 'selection'),
  };
}

export function parseReceipt(value: unknown): Receipt {
  const v = rec(value, 'receipt');
  return {
    operationId: str(v.operationId, 'op', 36),
    context: parseContext(v.context),
    storeRevision: counter(v.storeRevision, 'revision'),
    created: list(v.created, 'created', (c) => {
      const item = rec(c, 'created');
      return { role: str(item.role, 'role', 32), id: str(item.id, 'id', 36) };
    }),
    replayed: bool(v.replayed, 'replayed'),
  };
}

/** A failure, or the closest honest one when what arrived is not a failure the desktop sends. */
export function parseFailure(value: unknown): Failure {
  try {
    const v = rec(value, 'failure');
    return {
      code: oneOf(v.code, ERROR_CODES, 'code'),
      effect: oneOf(v.effect, EFFECTS, 'effect'),
      nextAction: oneOf(v.nextAction, NEXT_ACTIONS, 'next'),
      operationId: strOrNull(v.operationId ?? null, 'op'),
      context: v.context ? parseContext(v.context) : null,
      field: strOrNull(v.field ?? null, 'field'),
    };
  } catch {
    // An answer that does not fit says nothing about what happened.
    return {
      code: 'io_failure',
      effect: 'unknown',
      nextAction: 'refetch',
      operationId: null,
      context: null,
      field: null,
    };
  }
}

export function parseSummary(value: unknown): MigrationSummary {
  const v = rec(value, 'summary');
  const counts = rec(v.counts, 'counts');
  const n = (x: unknown) => {
    if (typeof x !== 'number' || !Number.isInteger(x) || x < 0)
      throw new Error('projects: count');
    return x;
  };
  return {
    previewId: str(v.previewId, 'preview', 36),
    mode: oneOf(v.mode, ['initial', 'incremental'] as const, 'mode'),
    counts: {
      projects: n(counts.projects),
      workspaces: n(counts.workspaces),
      sessions: n(counts.sessions),
      placedLeaves: n(counts.placedLeaves),
      emptyLeaves: n(counts.emptyLeaves),
      tabsSkipped: n(counts.tabsSkipped),
    },
    conflicts: list(v.conflicts, 'conflicts', (c) => {
      const item = rec(c, 'conflict');
      return { kind: str(item.kind, 'kind', 64), count: n(item.count) };
    }),
    sources: list(v.sources, 'sources', (s) => {
      const item = rec(s, 'source');
      return {
        kind: str(item.kind, 'kind', 16),
        digest: str(item.digest, 'digest', 64),
      };
    }),
    canCommit: bool(v.canCommit, 'can'),
    needsAliasAcceptance: bool(v.needsAliasAcceptance, 'alias'),
    olderChanged: bool(v.olderChanged, 'changed'),
  };
}

export function parseRollback(value: unknown): RollbackReport {
  const v = rec(value, 'rollback');
  const n = (x: unknown) => (typeof x === 'number' ? x : 0);
  return {
    canDisable: bool(v.canDisable, 'can'),
    unfinishedLaunches: n(v.unfinishedLaunches),
    projects: n(v.projects),
    workspaces: n(v.workspaces),
    sessions: n(v.sessions),
    mappedOlderRecords: n(v.mappedOlderRecords),
    note: str(v.note, 'note', 1024),
  };
}

export interface Output {
  data: Uint8Array;
  next: number;
  eof: boolean;
  truncated: boolean;
}
export function parseOutput(value: unknown): Output {
  const v = rec(value, 'output');
  if (
    !Array.isArray(v.data) ||
    v.data.length > 65536 ||
    !v.data.every((b) => Number.isInteger(b) && b >= 0 && b <= 255)
  )
    throw new Error('projects: data');
  if (typeof v.next !== 'number' || !Number.isSafeInteger(v.next))
    throw new Error('projects: next');
  return {
    data: Uint8Array.from(v.data as number[]),
    next: v.next,
    eof: bool(v.eof, 'eof'),
    truncated: bool(v.truncated, 'truncated'),
  };
}
