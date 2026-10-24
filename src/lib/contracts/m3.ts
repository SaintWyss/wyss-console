/**
 * The desktop's projection of M3: the provider profiles and why each can or cannot start, the
 * model-selection and history capabilities, and — only in a build that hosts a fixture runtime —
 * that runtime's conversation, held actions, usage and model facts.
 *
 * Every payload is decoded strictly: an unknown or missing key, an unknown word or a number out of
 * range is an error and the window shows nothing from it. A counter the provider did not report
 * is `null`, and `null` is never shown as zero.
 */
export const M3_VERSION = 'm3.1';

const MAX_COUNT = Number.MAX_SAFE_INTEGER;
const MAX_ITEMS = 256;

function invalid(field: string): Error {
  return new Error(`m3: ${field} is not valid`);
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
function flag(value: unknown, field: string): boolean {
  if (typeof value !== 'boolean') throw invalid(field);
  return value;
}
function count(value: unknown, field: string): number {
  if (
    !Number.isSafeInteger(value) ||
    (value as number) < 0 ||
    (value as number) > MAX_COUNT
  )
    throw invalid(field);
  return value as number;
}
function reported(value: unknown, field: string): number | null {
  return value === null ? null : count(value, field);
}
function word(value: unknown, field: string, most = 160): string {
  if (typeof value !== 'string' || value.length > most) throw invalid(field);
  return value;
}
function token(value: unknown, field: string, most = 128): string {
  const text = word(value, field, most);
  if (text === '' || /[^\x21-\x7e]/.test(text)) throw invalid(field);
  return text;
}
function nullable<T>(value: unknown, parse: (v: unknown) => T): T | null {
  return value === null ? null : parse(value);
}
function array(value: unknown, field: string, most = MAX_ITEMS): unknown[] {
  if (!Array.isArray(value) || value.length > most) throw invalid(field);
  return value;
}
function words(value: unknown, field: string, most = 64): string[] {
  return array(value, field, most).map((v) => token(v, field));
}

// ---- profiles ---------------------------------------------------------------------------------

export const PROFILE_IDS = [
  'codex_inference',
  'codex_local_tools',
  'claude_inference',
  'claude_local_tools',
] as const;
export type ProfileId = (typeof PROFILE_IDS)[number];
export const AVAILABILITY = [
  'available',
  'unverified',
  'blocked',
  'unsupported',
] as const;
export type Availability = (typeof AVAILABILITY)[number];
export const PROVIDERS = ['codex', 'claude'] as const;
export type Provider = (typeof PROVIDERS)[number];
export const MODES = ['inference', 'local_tools'] as const;
export type Mode = (typeof MODES)[number];
export const EVIDENCE = ['passed', 'not_run', 'not_applicable'] as const;
export type EvidenceState = (typeof EVIDENCE)[number];

export interface ProfileCard {
  id: ProfileId;
  provider: Provider;
  mode: Mode;
  name: string;
  availability: Availability;
  reasons: string[];
  build: { sdk: string; cli: string } | null;
  evidence: { fixture: EvidenceState; real_provider: EvidenceState };
  can_start: boolean;
}

function parseCard(value: unknown): ProfileCard {
  const r = record(
    value,
    [
      'id',
      'provider',
      'mode',
      'name',
      'availability',
      'reasons',
      'build',
      'evidence',
      'can_start',
    ],
    'profile',
  );
  const evidence = record(r.evidence, ['fixture', 'real_provider'], 'evidence');
  const build = nullable(r.build, (b) => {
    const pair = array(b, 'build', 2);
    if (pair.length !== 2) throw invalid('build');
    return { sdk: word(pair[0], 'sdk', 40), cli: word(pair[1], 'cli', 40) };
  });
  const availability = member(r.availability, AVAILABILITY, 'availability');
  const can_start = flag(r.can_start, 'can_start');
  // A start action exists only for an available profile: anything else is a corrupt payload.
  if (can_start !== (availability === 'available')) throw invalid('can_start');
  return {
    id: member(r.id, PROFILE_IDS, 'id'),
    provider: member(r.provider, PROVIDERS, 'provider'),
    mode: member(r.mode, MODES, 'mode'),
    name: token(r.name, 'name', 40),
    availability,
    reasons: words(r.reasons, 'reasons', 16),
    build,
    evidence: {
      fixture: member(evidence.fixture, EVIDENCE, 'fixture'),
      real_provider: member(evidence.real_provider, EVIDENCE, 'real_provider'),
    },
    can_start,
  };
}

export type InstallState =
  | { state: 'not_configured' }
  | { state: 'verifying' }
  | { state: 'refused'; reason: string }
  | { state: 'verified'; sdk_version: string; cli_version: string };

function parseInstall(value: unknown): InstallState {
  if (typeof value !== 'object' || value === null) throw invalid('install');
  const state = member(
    (value as Record<string, unknown>).state,
    ['not_configured', 'verifying', 'refused', 'verified'] as const,
    'install',
  );
  switch (state) {
    case 'not_configured':
    case 'verifying':
      record(value, ['state'], 'install');
      return { state };
    case 'refused': {
      const r = record(value, ['state', 'reason'], 'install');
      return { state, reason: token(r.reason, 'reason', 80) };
    }
    case 'verified': {
      const r = record(
        value,
        ['state', 'sdk_version', 'cli_version'],
        'install',
      );
      return {
        state,
        sdk_version: token(r.sdk_version, 'sdk_version', 40),
        cli_version: token(r.cli_version, 'cli_version', 40),
      };
    }
  }
}

// ---- capabilities -----------------------------------------------------------------------------

export interface FeatureInfo {
  capability: string;
  version: number;
  available: boolean;
  reasons: string[];
}
function parseFeature(value: unknown): FeatureInfo {
  const r = record(
    value,
    ['capability', 'version', 'available', 'reasons'],
    'feature',
  );
  return {
    capability: token(r.capability, 'capability', 60),
    version: count(r.version, 'version'),
    available: flag(r.available, 'available'),
    reasons: words(r.reasons, 'reasons', 16),
  };
}

export const HISTORY_STATES = [
  'off',
  'unavailable',
  'recording',
  'partial',
] as const;
export interface HistoryInfo {
  state: (typeof HISTORY_STATES)[number];
  enabled: boolean;
  can_enable: boolean;
  reasons: string[];
  retention_days: number;
  max_records: number;
  records: number | null;
}
function parseHistory(value: unknown): HistoryInfo {
  const r = record(
    value,
    [
      'state',
      'enabled',
      'can_enable',
      'reasons',
      'retention_days',
      'max_records',
      'records',
    ],
    'history',
  );
  return {
    state: member(r.state, HISTORY_STATES, 'state'),
    enabled: flag(r.enabled, 'enabled'),
    can_enable: flag(r.can_enable, 'can_enable'),
    reasons: words(r.reasons, 'reasons', 16),
    retention_days: count(r.retention_days, 'retention_days'),
    max_records: count(r.max_records, 'max_records'),
    records: reported(r.records, 'records'),
  };
}

// ---- usage ------------------------------------------------------------------------------------

export interface Tokens {
  input: number | null;
  output: number | null;
  cache_creation: number | null;
  cache_read: number | null;
  reasoning: number | null;
}
const TOKEN_KEYS = [
  'input',
  'output',
  'cache_creation',
  'cache_read',
  'reasoning',
] as const;
function parseTokens(value: unknown): Tokens {
  const r = record(value, TOKEN_KEYS, 'tokens');
  return {
    input: reported(r.input, 'input'),
    output: reported(r.output, 'output'),
    cache_creation: reported(r.cache_creation, 'cache_creation'),
    cache_read: reported(r.cache_read, 'cache_read'),
    reasoning: reported(r.reasoning, 'reasoning'),
  };
}
export const COMPLETENESS = [
  'complete',
  'provider_error',
  'interrupted',
  'unreported',
  'running',
] as const;
export interface TurnUsage {
  turn_id: string;
  provider: Provider;
  total: Tokens;
  by_model: Record<string, Tokens>;
  echoed_models: string[];
  completeness: (typeof COMPLETENESS)[number];
  /** The SDK's own client-side estimate: never billing, never subscription spend. */
  estimate_usd: number | null;
}
export interface Aggregate {
  sum: number;
  reported_turns: number;
  unreported_turns: number;
}
export const LIMIT_WINDOWS = [
  'five_hour',
  'seven_day',
  'seven_day_opus',
  'seven_day_sonnet',
  'overage',
] as const;
export const LIMIT_STATUSES = [
  'allowed',
  'allowed_warning',
  'rejected',
] as const;
export interface LimitInfo {
  window: (typeof LIMIT_WINDOWS)[number];
  status: (typeof LIMIT_STATUSES)[number];
  resets_at: number | null;
  utilization: number | null;
  observed_at: number;
}
export interface UsageInfo {
  provider: Provider;
  turns: TurnUsage[];
  dropped_turns: number;
  running: Record<(typeof TOKEN_KEYS)[number], Aggregate>;
  limits: LimitInfo[];
}
function parseAggregate(value: unknown): Aggregate {
  const r = record(
    value,
    ['sum', 'reported_turns', 'unreported_turns'],
    'aggregate',
  );
  return {
    sum: count(r.sum, 'sum'),
    reported_turns: count(r.reported_turns, 'reported_turns'),
    unreported_turns: count(r.unreported_turns, 'unreported_turns'),
  };
}
function parseTurnUsage(value: unknown): TurnUsage {
  const r = record(
    value,
    [
      'turn_id',
      'provider',
      'total',
      'by_model',
      'echoed_models',
      'completeness',
      'estimate',
    ],
    'turn usage',
  );
  const byModel: Record<string, Tokens> = {};
  if (
    typeof r.by_model !== 'object' ||
    r.by_model === null ||
    Array.isArray(r.by_model)
  )
    throw invalid('by_model');
  for (const [k, v] of Object.entries(r.by_model).slice(0, 8)) {
    byModel[token(k, 'model')] = parseTokens(v);
  }
  const estimate = nullable(r.estimate, (e) => {
    const x = record(e, ['usd'], 'estimate');
    if (typeof x.usd !== 'number' || !Number.isFinite(x.usd) || x.usd < 0)
      throw invalid('usd');
    return x.usd;
  });
  return {
    turn_id: token(r.turn_id, 'turn_id'),
    provider: member(r.provider, PROVIDERS, 'provider'),
    total: parseTokens(r.total),
    by_model: byModel,
    echoed_models: words(r.echoed_models, 'echoed_models', 8),
    completeness: member(r.completeness, COMPLETENESS, 'completeness'),
    estimate_usd: estimate,
  };
}
function parseLimit(value: unknown): LimitInfo {
  const r = record(
    value,
    ['window', 'status', 'resets_at', 'utilization', 'observed_at'],
    'limit',
  );
  const utilization = nullable(r.utilization, (u) => {
    if (typeof u !== 'number' || !Number.isFinite(u) || u < 0 || u > 1)
      throw invalid('utilization');
    return u;
  });
  return {
    window: member(r.window, LIMIT_WINDOWS, 'window'),
    status: member(r.status, LIMIT_STATUSES, 'status'),
    resets_at: reported(r.resets_at, 'resets_at'),
    utilization,
    observed_at: count(r.observed_at, 'observed_at'),
  };
}
function parseUsage(value: unknown): UsageInfo {
  const r = record(
    value,
    ['provider', 'turns', 'dropped_turns', 'running', 'limits'],
    'usage',
  );
  const running = record(r.running, TOKEN_KEYS, 'running');
  return {
    provider: member(r.provider, PROVIDERS, 'provider'),
    turns: array(r.turns, 'turns', 80).map(parseTurnUsage),
    dropped_turns: count(r.dropped_turns, 'dropped_turns'),
    running: {
      input: parseAggregate(running.input),
      output: parseAggregate(running.output),
      cache_creation: parseAggregate(running.cache_creation),
      cache_read: parseAggregate(running.cache_read),
      reasoning: parseAggregate(running.reasoning),
    },
    limits: array(r.limits, 'limits', 16).map(parseLimit),
  };
}

// ---- model facts ------------------------------------------------------------------------------

export interface ModelEntry {
  model_id: string;
  alias: string | null;
  efforts: string[];
  is_default: boolean;
}
function parseEntry(value: unknown): ModelEntry {
  const r = record(
    value,
    ['model_id', 'alias', 'efforts', 'is_default'],
    'model entry',
  );
  return {
    model_id: token(r.model_id, 'model_id'),
    alias: nullable(r.alias, (a) => token(a, 'alias')),
    efforts: words(r.efforts, 'efforts', 16),
    is_default: flag(r.is_default, 'is_default'),
  };
}
export interface ModelRequestInfo {
  model: string;
  effort: string | null;
}
function parseRequest(value: unknown): ModelRequestInfo {
  const r = record(value, ['model', 'effort'], 'request');
  return {
    model: token(r.model, 'model'),
    effort: nullable(r.effort, (e) => token(e, 'effort', 32)),
  };
}
export type RerouteInfo =
  | { state: 'not_requested' | 'not_reported' | 'honored' }
  | { state: 'rerouted'; requested: string; reported: string[] };
export type EffortInfo =
  | { state: 'not_requested' }
  | {
      state: 'requested' | 'observed_in_provider_request' | 'provider_reported';
      value: string;
    };
export interface ModelFactsInfo {
  offered: {
    provider: Provider;
    source: string;
    revision: number;
    observed_at: number;
    freshness: 'fresh' | 'stale';
    complete: boolean;
    entitlement: 'not_checked';
    entries: ModelEntry[];
  } | null;
  configured: ModelRequestInfo | null;
  requested: {
    turn_id: string;
    request: ModelRequestInfo;
    resolved_model: string;
    generation: number;
  } | null;
  reported_in_use: string[];
  reroute: RerouteInfo;
  effort: EffortInfo;
}
function parseReroute(value: unknown): RerouteInfo {
  if (typeof value !== 'object' || value === null) throw invalid('reroute');
  const state = member(
    (value as Record<string, unknown>).state,
    ['not_requested', 'not_reported', 'honored', 'rerouted'] as const,
    'reroute',
  );
  if (state !== 'rerouted') {
    record(value, ['state'], 'reroute');
    return { state };
  }
  const r = record(value, ['state', 'requested', 'reported'], 'reroute');
  return {
    state,
    requested: token(r.requested, 'requested'),
    reported: words(r.reported, 'reported', 8),
  };
}
function parseEffort(value: unknown): EffortInfo {
  if (typeof value !== 'object' || value === null) throw invalid('effort');
  const state = member(
    (value as Record<string, unknown>).state,
    [
      'not_requested',
      'requested',
      'observed_in_provider_request',
      'provider_reported',
    ] as const,
    'effort',
  );
  if (state === 'not_requested') {
    record(value, ['state'], 'effort');
    return { state };
  }
  const r = record(value, ['state', 'value'], 'effort');
  return { state, value: token(r.value, 'effort', 32) };
}
function parseFacts(value: unknown): ModelFactsInfo {
  const r = record(
    value,
    [
      'offered',
      'configured',
      'requested',
      'reported_in_use',
      'reroute',
      'effort',
    ],
    'model facts',
  );
  return {
    offered: nullable(r.offered, (o) => {
      const x = record(
        o,
        [
          'provider',
          'source',
          'revision',
          'observed_at',
          'freshness',
          'complete',
          'entitlement',
          'entries',
        ],
        'offered',
      );
      return {
        provider: member(x.provider, PROVIDERS, 'provider'),
        source: token(x.source, 'source', 40),
        revision: count(x.revision, 'revision'),
        observed_at: count(x.observed_at, 'observed_at'),
        freshness: member(
          x.freshness,
          ['fresh', 'stale'] as const,
          'freshness',
        ),
        complete: flag(x.complete, 'complete'),
        entitlement: member(
          x.entitlement,
          ['not_checked'] as const,
          'entitlement',
        ),
        entries: array(x.entries, 'entries', 64).map(parseEntry),
      };
    }),
    configured: nullable(r.configured, parseRequest),
    requested: nullable(r.requested, (q) => {
      const x = record(
        q,
        ['turn_id', 'request', 'resolved_model', 'generation'],
        'requested',
      );
      return {
        turn_id: token(x.turn_id, 'turn_id'),
        request: parseRequest(x.request),
        resolved_model: token(x.resolved_model, 'resolved_model'),
        generation: count(x.generation, 'generation'),
      };
    }),
    reported_in_use: words(r.reported_in_use, 'reported_in_use', 16),
    reroute: parseReroute(r.reroute),
    effort: parseEffort(r.effort),
  };
}

// ---- the fixture runtime ----------------------------------------------------------------------

export const ACTION_STATES = [
  'held',
  'declined',
  'cancelled',
  'running',
  'completed',
  'failed',
  'indeterminate',
  'refused',
] as const;
export interface ActionRecord {
  item: string;
  tool: string;
  state: (typeof ACTION_STATES)[number];
  refusal: string | null;
}
export interface TapInfo {
  requests: number;
  violations: number;
  tools_seen: string[];
  models_seen: string[];
  efforts_seen: string[];
}
export const REPORT_PHASES = ['starting', 'ready', 'refused'] as const;
export interface ProviderReportInfo {
  provider: Provider;
  profile: string;
  phase: (typeof REPORT_PHASES)[number];
  refusal: string | null;
  build: { sdk_version: string | null; cli_version: string | null };
  offered: ModelEntry[];
  tap: TapInfo;
  usage: UsageInfo;
  refused_proposals: number;
  pending_approvals: number;
  indeterminate_actions: number;
  denied_attempts: number;
  failure: string | null;
  provider_error: string | null;
  model: ModelFactsInfo | null;
  actions: ActionRecord[];
}
function parseReport(value: unknown): ProviderReportInfo {
  const r = record(
    value,
    [
      'provider',
      'profile',
      'phase',
      'refusal',
      'build',
      'offered',
      'tap',
      'usage',
      'refused_proposals',
      'pending_approvals',
      'indeterminate_actions',
      'denied_attempts',
      'failure',
      'provider_error',
      'model',
      'actions',
    ],
    'report',
  );
  const build = record(r.build, ['sdk_version', 'cli_version'], 'build');
  const tap = record(
    r.tap,
    ['requests', 'violations', 'tools_seen', 'models_seen', 'efforts_seen'],
    'tap',
  );
  return {
    provider: member(r.provider, PROVIDERS, 'provider'),
    profile: token(r.profile, 'profile', 40),
    phase: member(r.phase, REPORT_PHASES, 'phase'),
    refusal: nullable(r.refusal, (x) => token(x, 'refusal', 60)),
    build: {
      sdk_version: nullable(build.sdk_version, (x) => token(x, 'sdk', 40)),
      cli_version: nullable(build.cli_version, (x) => token(x, 'cli', 40)),
    },
    offered: array(r.offered, 'offered', 64).map(parseEntry),
    tap: {
      requests: count(tap.requests, 'requests'),
      violations: count(tap.violations, 'violations'),
      tools_seen: words(tap.tools_seen, 'tools_seen', 16),
      models_seen: words(tap.models_seen, 'models_seen', 16),
      efforts_seen: words(tap.efforts_seen, 'efforts_seen', 16),
    },
    usage: parseUsage(r.usage),
    refused_proposals: count(r.refused_proposals, 'refused_proposals'),
    pending_approvals: count(r.pending_approvals, 'pending_approvals'),
    indeterminate_actions: count(r.indeterminate_actions, 'indeterminate'),
    denied_attempts: count(r.denied_attempts, 'denied_attempts'),
    failure: nullable(r.failure, (x) => word(x, 'failure', 160)),
    provider_error: nullable(r.provider_error, (x) =>
      token(x, 'provider_error', 60),
    ),
    model: nullable(r.model, parseFacts),
    actions: array(r.actions, 'actions', 32).map((a) => {
      const x = record(a, ['item', 'tool', 'state', 'refusal'], 'action');
      return {
        item: token(x.item, 'item'),
        tool: word(x.tool, 'tool', 40),
        state: member(x.state, ACTION_STATES, 'state'),
        refusal: nullable(x.refusal, (z) => token(z, 'refusal', 40)),
      };
    }),
  };
}

export const FIXTURE_PHASES = [
  'stopped',
  'starting',
  'ready',
  'refused',
  'failed',
] as const;
export const TURN_STATES = [
  'starting',
  'running',
  'completed',
  'failed',
  'interrupted',
] as const;
export const APPROVAL_VIEW_STATES = [
  'pending',
  'accepted',
  'declined',
  'cancelled',
  'confirmed',
  'failed',
  'indeterminate',
  'refused',
] as const;
export const DECISIONS = ['accept', 'decline', 'cancel'] as const;
export type Decision = (typeof DECISIONS)[number];
export interface HeldAction {
  item: string;
  kind: string;
  fingerprint: string;
  /** The action exactly as the authority holds it: a command, or file diffs. */
  action: unknown;
  decisions: Decision[];
  state: (typeof APPROVAL_VIEW_STATES)[number];
}
export interface FixtureInfo {
  label: string;
  phase: (typeof FIXTURE_PHASES)[number];
  profile: string | null;
  conversation: { role: 'user' | 'assistant'; text: string }[];
  turn: { id: string; state: (typeof TURN_STATES)[number] } | null;
  approvals: HeldAction[];
  report: ProviderReportInfo | null;
  detail: string | null;
}
function parseFixture(value: unknown): FixtureInfo {
  const r = record(
    value,
    [
      'label',
      'phase',
      'profile',
      'conversation',
      'turn',
      'approvals',
      'report',
      'detail',
    ],
    'fixture',
  );
  return {
    label: word(r.label, 'label', 120),
    phase: member(r.phase, FIXTURE_PHASES, 'phase'),
    profile: nullable(r.profile, (p) => token(p, 'profile', 40)),
    conversation: array(r.conversation, 'conversation', 200).map((m) => {
      const x = record(m, ['role', 'text'], 'message');
      return {
        role: member(x.role, ['user', 'assistant'] as const, 'role'),
        text: word(x.text, 'text', 1_100_000),
      };
    }),
    turn: nullable(r.turn, (t) => {
      const x = record(t, ['id', 'state'], 'turn');
      return {
        id: token(x.id, 'turn id'),
        state: member(x.state, TURN_STATES, 'state'),
      };
    }),
    approvals: array(r.approvals, 'approvals', 16).map((a) => {
      const x = record(
        a,
        ['item', 'kind', 'fingerprint', 'action', 'decisions', 'state'],
        'approval',
      );
      return {
        item: token(x.item, 'item'),
        kind: token(x.kind, 'kind', 40),
        fingerprint: token(x.fingerprint, 'fingerprint', 64),
        action: x.action,
        decisions: array(x.decisions, 'decisions', 3).map((d) =>
          member(d, DECISIONS, 'decision'),
        ),
        state: member(x.state, APPROVAL_VIEW_STATES, 'state'),
      };
    }),
    report: nullable(r.report, parseReport),
    detail: nullable(r.detail, (d) => word(d, 'detail', 160)),
  };
}

// ---- the whole state --------------------------------------------------------------------------

export interface M3State {
  capability: 'm3';
  version: string;
  profiles: ProfileCard[];
  claude_install: InstallState;
  model_selection: FeatureInfo;
  history: HistoryInfo;
  fixture: FixtureInfo | null;
}

export function parseM3State(value: unknown): M3State {
  const r = record(
    value,
    [
      'capability',
      'version',
      'profiles',
      'claude_install',
      'model_selection',
      'history',
      'fixture',
    ],
    'm3 state',
  );
  const profiles = array(r.profiles, 'profiles', 4).map(parseCard);
  if (
    profiles.length !== PROFILE_IDS.length ||
    !PROFILE_IDS.every((id, i) => profiles[i]?.id === id)
  )
    throw invalid('profiles');
  return {
    capability: member(r.capability, ['m3'] as const, 'capability'),
    version: member(r.version, [M3_VERSION] as const, 'version'),
    profiles,
    claude_install: parseInstall(r.claude_install),
    model_selection: parseFeature(r.model_selection),
    history: parseHistory(r.history),
    fixture: nullable(r.fixture, parseFixture),
  };
}

export interface Ack {
  ok: boolean;
  error: string | null;
}
export function parseAck(value: unknown): Ack {
  const r = record(value, ['ok', 'error'], 'ack');
  return {
    ok: flag(r.ok, 'ok'),
    error: nullable(r.error, (e) => token(e, 'error', 40)),
  };
}
