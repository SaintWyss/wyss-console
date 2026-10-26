import { invoke } from '@tauri-apps/api/core';
import {
  isDigest,
  isRecord,
  type CoordinatorSnapshot,
  type DaemonError,
  type DaemonError as DaemonErrorName,
  type HandoffConfirmationV1,
  type HandoffEnvelopeV1,
  type HandoffPreview,
  type OrchestrationPlanV1,
  type PlanReviewV1,
  type RunState,
  type StepSnapshot,
  type StepState,
  type WorkspaceSnapshot,
} from '../contracts/orchestration';

export class OrchestrationCallError extends Error {
  constructor(readonly code: DaemonError) {
    super(`orchestration: ${code}`);
  }
}

type Request =
  | { method: 'workspace_snapshot'; params: { workspace_root: string } }
  | { method: 'review_plan'; params: { plan: OrchestrationPlanV1 } }
  | { method: 'seal_handoff'; params: { envelope: HandoffEnvelopeV1 } }
  | {
      method: 'submit';
      params: { plan: OrchestrationPlanV1; review: PlanReviewV1 };
    }
  | {
      method: 'submit_handoff';
      params: {
        envelope: HandoffEnvelopeV1;
        confirmation: HandoffConfirmationV1;
      };
    }
  | { method: 'list' }
  | { method: 'get'; params: { run_id: string } }
  | { method: 'pause'; params: { run_id: string } }
  | { method: 'resume'; params: { run_id: string } }
  | { method: 'stop'; params: { run_id: string } }
  | {
      method: 'verify_step';
      params: {
        run_id: string;
        step_id: string;
        reviewed_workspace_revision: string;
      };
    }
  | {
      method: 'reconcile';
      params: {
        run_id: string;
        step_id: string;
        decision:
          | 'unknown'
          | 'confirmed_completed'
          | 'confirmed_no_effect'
          | 'confirmed_cancelled';
        workspace_root: string | null;
        reviewed_workspace_revision: string | null;
      };
    };

const DAEMON_ERRORS: readonly DaemonErrorName[] = [
  'unavailable',
  'invalid_request',
  'invalid_plan',
  'invalid_handoff',
  'review_stale',
  'trust_boundary_blocked',
  'provider_blocked',
  'permission_denied',
  'resource_busy',
  'workspace_conflict',
  'journal_unavailable',
  'authority_unavailable',
  'unknown_run',
  'invalid_transition',
  'expired',
  'effect_unknown',
];
const RUN_STATES: readonly RunState[] = [
  'ready',
  'running',
  'pausing',
  'paused',
  'cancelling',
  'completed',
  'failed',
  'cancelled',
  'expired',
  'needs_review',
  'blocked',
];
const STEP_STATES: readonly StepState[] = [
  'proposed',
  'waiting_dependencies',
  'eligible',
  'waiting_resource',
  'awaiting_permission',
  'queued',
  'starting',
  'running',
  'verifying',
  'succeeded',
  'skipped',
  'blocked',
  'failed',
  'cancelled',
  'needs_review',
];
const FAILURE_CODES = [
  'invalid_plan',
  'plan_changed',
  'plan_expired',
  'dependency_failed',
  'trust_boundary_blocked',
  'provider_not_admitted',
  'managed_session_unavailable',
  'permission_denied',
  'resource_busy',
  'workspace_conflict',
  'authority_unavailable',
  'dispatch_unknown',
  'provider_failed',
  'cancel_unconfirmed',
  'owner_review_required',
  'coordinator_restarted',
] as const;

function exact(
  value: unknown,
  keys: readonly string[],
): Record<string, unknown> {
  if (!isRecord(value)) throw new Error('invalid_response');
  const names = Object.keys(value);
  if (names.length !== keys.length || !keys.every((key) => names.includes(key)))
    throw new Error('invalid_response');
  return value;
}
function string(value: unknown): value is string {
  return typeof value === 'string';
}
function count(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) >= 0;
}
function member<T extends string>(
  value: unknown,
  choices: readonly T[],
): value is T {
  return typeof value === 'string' && choices.includes(value as T);
}
function nullableString(value: unknown): boolean {
  return value === null || string(value);
}
function parseStep(value: unknown): StepSnapshot {
  const step = exact(value, [
    'step_id',
    'dependencies',
    'required',
    'provider',
    'effect',
    'managed_session_id',
    'model_id',
    'state',
    'attempt_count',
    'attempt_id',
    'managed_turn_id',
    'authority_instance_id',
    'runtime_instance_id',
    'runtime_generation',
    'turn_revision',
    'workspace_revision_after',
    'failure',
    'updated_at_unix',
  ]);
  if (
    !string(step.step_id) ||
    !Array.isArray(step.dependencies) ||
    typeof step.required !== 'boolean' ||
    !member(step.provider, [
      'managed_codex',
      'managed_claude',
      'native_codex',
      'native_claude',
      'hermes_planner',
    ]) ||
    !member(step.effect, [
      'proposal_only',
      'managed_inference',
      'managed_local_tools',
      'native_full_account',
    ]) ||
    !nullableString(step.managed_session_id) ||
    !nullableString(step.model_id) ||
    !member(step.state, STEP_STATES) ||
    !count(step.attempt_count) ||
    !nullableString(step.attempt_id) ||
    !nullableString(step.managed_turn_id) ||
    !nullableString(step.authority_instance_id) ||
    !nullableString(step.runtime_instance_id) ||
    !(step.runtime_generation === null || count(step.runtime_generation)) ||
    !(step.turn_revision === null || count(step.turn_revision)) ||
    !(
      step.workspace_revision_after === null ||
      isDigest(step.workspace_revision_after)
    ) ||
    !(step.failure === null || member(step.failure, FAILURE_CODES)) ||
    !count(step.updated_at_unix)
  ) {
    throw new Error('invalid_response');
  }
  for (const dependency of step.dependencies) {
    const item = exact(dependency, ['step_id', 'allow_missing_output']);
    if (!string(item.step_id) || typeof item.allow_missing_output !== 'boolean')
      throw new Error('invalid_response');
  }
  return step as unknown as StepSnapshot;
}
function parseSnapshot(value: unknown): CoordinatorSnapshot {
  const run = exact(value, [
    'run_id',
    'revision',
    'plan_revision',
    'plan_digest',
    'workspace_id',
    'current_workspace_revision',
    'state',
    'failure',
    'created_at_unix',
    'updated_at_unix',
    'steps',
  ]);
  if (
    !string(run.run_id) ||
    !count(run.revision) ||
    !count(run.plan_revision) ||
    !isDigest(run.plan_digest) ||
    !isDigest(run.workspace_id) ||
    !isDigest(run.current_workspace_revision) ||
    !member(run.state, RUN_STATES) ||
    !(run.failure === null || member(run.failure, FAILURE_CODES)) ||
    !count(run.created_at_unix) ||
    !count(run.updated_at_unix) ||
    !Array.isArray(run.steps)
  ) {
    throw new Error('invalid_response');
  }
  return {
    ...run,
    steps: run.steps.map(parseStep),
  } as unknown as CoordinatorSnapshot;
}
function parseReview(value: unknown): PlanReviewV1 {
  const review = exact(value, ['plan_id', 'revision', 'digest']);
  if (
    !string(review.plan_id) ||
    !count(review.revision) ||
    !isDigest(review.digest)
  )
    throw new Error('invalid_response');
  return review as unknown as PlanReviewV1;
}
function parseWorkspace(value: unknown): WorkspaceSnapshot {
  const workspace = exact(value, ['workspace_id', 'workspace_revision']);
  if (
    !isDigest(workspace.workspace_id) ||
    !isDigest(workspace.workspace_revision)
  )
    throw new Error('invalid_response');
  return workspace as unknown as WorkspaceSnapshot;
}
function parseHandoffPreview(
  value: unknown,
  draft: HandoffEnvelopeV1,
): HandoffPreview {
  const preview = exact(value, ['envelope', 'first_message']);
  const envelope = exact(preview.envelope, Object.keys(draft));
  if (
    !string(preview.first_message) ||
    !isDigest(envelope.content_digest) ||
    envelope.handoff_id !== draft.handoff_id ||
    envelope.revision !== draft.revision
  ) {
    throw new Error('invalid_response');
  }
  return {
    envelope: { ...draft, content_digest: envelope.content_digest },
    first_message: preview.first_message,
  };
}

async function call(request: Request): Promise<unknown> {
  try {
    return await invoke<unknown>('orchestration_call', { request });
  } catch (error) {
    const code = typeof error === 'string' ? error.replace(/^"|"$/g, '') : '';
    throw new OrchestrationCallError(
      member(code, DAEMON_ERRORS) ? code : 'unavailable',
    );
  }
}

export async function workspaceSnapshot(
  root: string,
): Promise<WorkspaceSnapshot> {
  return parseWorkspace(
    await call({
      method: 'workspace_snapshot',
      params: { workspace_root: root },
    }),
  );
}
export async function reviewPlan(
  plan: OrchestrationPlanV1,
): Promise<PlanReviewV1> {
  return parseReview(await call({ method: 'review_plan', params: { plan } }));
}
export async function sealHandoff(
  envelope: HandoffEnvelopeV1,
): Promise<HandoffPreview> {
  return parseHandoffPreview(
    await call({ method: 'seal_handoff', params: { envelope } }),
    envelope,
  );
}
export async function submitPlan(
  plan: OrchestrationPlanV1,
  review: PlanReviewV1,
): Promise<CoordinatorSnapshot> {
  return parseSnapshot(
    await call({ method: 'submit', params: { plan, review } }),
  );
}
export async function submitHandoff(
  envelope: HandoffEnvelopeV1,
  confirmation: HandoffConfirmationV1,
): Promise<CoordinatorSnapshot> {
  return parseSnapshot(
    await call({
      method: 'submit_handoff',
      params: { envelope, confirmation },
    }),
  );
}
export async function listRuns(): Promise<CoordinatorSnapshot[]> {
  const value = await call({ method: 'list' });
  if (!Array.isArray(value)) throw new OrchestrationCallError('unavailable');
  return value.map(parseSnapshot);
}
export async function runAction(
  method: 'pause' | 'resume' | 'stop',
  run_id: string,
): Promise<CoordinatorSnapshot> {
  return parseSnapshot(await call({ method, params: { run_id } }));
}
export async function verifyStep(
  run_id: string,
  step_id: string,
  reviewed_workspace_revision: string,
): Promise<CoordinatorSnapshot> {
  return parseSnapshot(
    await call({
      method: 'verify_step',
      params: { run_id, step_id, reviewed_workspace_revision },
    }),
  );
}
export async function reconcileStep(
  run_id: string,
  step_id: string,
  decision:
    | 'unknown'
    | 'confirmed_completed'
    | 'confirmed_no_effect'
    | 'confirmed_cancelled',
  workspace_root: string | null,
  reviewed_workspace_revision: string | null,
): Promise<CoordinatorSnapshot> {
  return parseSnapshot(
    await call({
      method: 'reconcile',
      params: {
        run_id,
        step_id,
        decision,
        workspace_root,
        reviewed_workspace_revision,
      },
    }),
  );
}
