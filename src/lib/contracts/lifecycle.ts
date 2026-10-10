/**
 * LifecycleView v1: where making the authority available stands, as the desktop says
 * it. Closed words only. It names no service, command, path or process, and it is not
 * the connection: whether an authority is connected is `AuthorityView`'s to say.
 */
export const LIFECYCLE_PHASES = [
  'checking',
  'installing',
  'starting',
  'connecting',
  'ready',
  'incompatible',
  'failed',
] as const;
export type LifecyclePhase = (typeof LIFECYCLE_PHASES)[number];

export const LIFECYCLE_FAILURES = [
  'not_packaged',
  'package_invalid',
  'installation_unsafe',
  'installation_mismatch',
  'install_failed',
  'manager_unavailable',
  'start_refused',
  'startup_failed',
  'exited_repeatedly',
  'stopped',
  'socket_unavailable',
  'socket_unsafe',
] as const;
export type LifecycleFailure = (typeof LIFECYCLE_FAILURES)[number];

export const AUTHORITY_ORIGINS = ['managed', 'external'] as const;
export type AuthorityOrigin = (typeof AUTHORITY_ORIGINS)[number];

export const BUILD_KINDS = ['packaged', 'development', 'invalid'] as const;
export const INSTALLATION_STATES = [
  'unknown',
  'absent',
  'current',
  'outdated',
  'mismatch',
  'unsafe',
] as const;
export const SERVICE_STATES = [
  'unknown',
  'unavailable',
  'not_installed',
  'inactive',
  'starting',
  'running',
  'failed',
] as const;

export interface LifecycleDiagnostics {
  build: (typeof BUILD_KINDS)[number];
  installation: (typeof INSTALLATION_STATES)[number];
  service: (typeof SERVICE_STATES)[number];
}
export interface LifecycleView {
  schema_version: 1;
  revision: number;
  phase: LifecyclePhase;
  /** Present exactly while `phase` is `failed`. */
  failure: LifecycleFailure | null;
  /** Whether Retry would do anything now. */
  retry: boolean;
  /** Who started the authority that answers, while one does and it is known. */
  origin: AuthorityOrigin | null;
  /** A newer packaged authority waits for the next time none runs. */
  update_pending: boolean;
  diagnostics: LifecycleDiagnostics;
}

function invalid(field: string): Error {
  return new Error(`lifecycle: invalid_${field}`);
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

/**
 * Accepts exactly the view, and nothing that contradicts itself: a failure outside
 * `failed`, a retry that is offered when nothing failed, an origin of an authority that
 * does not answer, an update pending for an authority this installation does not own.
 */
export function parseLifecycleView(value: unknown): LifecycleView {
  const v = record(
    value,
    [
      'schema_version',
      'revision',
      'phase',
      'failure',
      'retry',
      'origin',
      'update_pending',
      'diagnostics',
    ],
    'view',
  );
  if (v.schema_version !== 1) throw invalid('schema_version');
  if (
    typeof v.revision !== 'number' ||
    !Number.isSafeInteger(v.revision) ||
    v.revision < 0
  )
    throw invalid('revision');
  const phase = member(v.phase, LIFECYCLE_PHASES, 'phase');
  const failure =
    v.failure === null
      ? null
      : member(v.failure, LIFECYCLE_FAILURES, 'failure');
  if ((failure !== null) !== (phase === 'failed')) throw invalid('failure');
  if (typeof v.retry !== 'boolean' || v.retry !== (phase === 'failed'))
    throw invalid('retry');
  const origin =
    v.origin === null ? null : member(v.origin, AUTHORITY_ORIGINS, 'origin');
  if (origin !== null && phase !== 'ready' && phase !== 'incompatible')
    throw invalid('origin');
  if (
    typeof v.update_pending !== 'boolean' ||
    (v.update_pending && (phase !== 'ready' || origin !== 'managed'))
  )
    throw invalid('update_pending');
  const d = record(
    v.diagnostics,
    ['build', 'installation', 'service'],
    'diagnostics',
  );
  return {
    schema_version: 1,
    revision: v.revision,
    phase,
    failure,
    retry: v.retry,
    origin,
    update_pending: v.update_pending,
    diagnostics: {
      build: member(d.build, BUILD_KINDS, 'build'),
      installation: member(d.installation, INSTALLATION_STATES, 'installation'),
      service: member(d.service, SERVICE_STATES, 'service'),
    },
  };
}
