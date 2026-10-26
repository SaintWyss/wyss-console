/** M6's versioned desktop projection. This DTO is not an authority contract. */
export const PROVIDER_LANES = [
  'managed_codex',
  'managed_claude',
  'native_codex',
  'native_claude',
  'hermes_planner',
] as const;
export type ProviderLane = (typeof PROVIDER_LANES)[number];
export type EffectClass =
  | 'proposal_only'
  | 'managed_inference'
  | 'managed_local_tools'
  | 'native_full_account';
export type RunState =
  | 'ready'
  | 'running'
  | 'pausing'
  | 'paused'
  | 'cancelling'
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'expired'
  | 'needs_review'
  | 'blocked';
export type StepState =
  | 'proposed'
  | 'waiting_dependencies'
  | 'eligible'
  | 'waiting_resource'
  | 'awaiting_permission'
  | 'queued'
  | 'starting'
  | 'running'
  | 'verifying'
  | 'succeeded'
  | 'skipped'
  | 'blocked'
  | 'failed'
  | 'cancelled'
  | 'needs_review';
export type FailureCode =
  | 'invalid_plan'
  | 'plan_changed'
  | 'plan_expired'
  | 'dependency_failed'
  | 'trust_boundary_blocked'
  | 'provider_not_admitted'
  | 'managed_session_unavailable'
  | 'permission_denied'
  | 'resource_busy'
  | 'workspace_conflict'
  | 'authority_unavailable'
  | 'dispatch_unknown'
  | 'provider_failed'
  | 'cancel_unconfirmed'
  | 'owner_review_required'
  | 'coordinator_restarted';
export type DaemonError =
  | 'unavailable'
  | 'invalid_request'
  | 'invalid_plan'
  | 'invalid_handoff'
  | 'handoff_objective_too_large'
  | 'handoff_prompt_too_large'
  | 'review_stale'
  | 'trust_boundary_blocked'
  | 'provider_blocked'
  | 'receiver_not_fresh'
  | 'receiver_freshness_unproven'
  | 'source_freshness_unproven'
  | 'permission_denied'
  | 'resource_busy'
  | 'workspace_conflict'
  | 'journal_unavailable'
  | 'authority_unavailable'
  | 'unknown_run'
  | 'invalid_transition'
  | 'expired'
  | 'effect_unknown';

export interface WorkspaceSnapshot {
  workspace_id: string;
  workspace_revision: string;
}
export interface DependencyV1 {
  step_id: string;
  allow_missing_output: boolean;
}
export interface StepPlanV1 {
  step_id: string;
  dependencies: DependencyV1[];
  required: boolean;
  provider: ProviderLane;
  effect: EffectClass;
  managed_session_id: string | null;
  model_id: string | null;
  prompt: string;
  max_attempts: number;
  max_duration_seconds: number;
}
export interface OrchestrationPlanV1 {
  schema_version: 1;
  plan_id: string;
  revision: number;
  host_id: string;
  project_id: string;
  workspace_id: string;
  workspace_root: string;
  workspace_revision: string;
  objective: string;
  created_at_unix: number;
  expires_at_unix: number;
  limits: {
    max_steps: number;
    max_parallelism: 1;
    max_total_seconds: number;
    stop_policy: 'stop_scheduling' | 'cancel_current_managed_turn';
  };
  steps: StepPlanV1[];
}
export interface PlanReviewV1 {
  plan_id: string;
  revision: number;
  digest: string;
}
export interface StepSnapshot {
  step_id: string;
  dependencies: DependencyV1[];
  required: boolean;
  provider: ProviderLane;
  effect: EffectClass;
  managed_session_id: string | null;
  model_id: string | null;
  state: StepState;
  attempt_count: number;
  attempt_id: string | null;
  managed_turn_id: string | null;
  authority_instance_id: string | null;
  runtime_instance_id: string | null;
  runtime_generation: number | null;
  turn_revision: number | null;
  workspace_revision_after: string | null;
  failure: FailureCode | null;
  updated_at_unix: number;
}
export interface CoordinatorSnapshot {
  run_id: string;
  revision: number;
  plan_revision: number;
  plan_digest: string;
  workspace_id: string;
  current_workspace_revision: string;
  state: RunState;
  failure: FailureCode | null;
  created_at_unix: number;
  updated_at_unix: number;
  steps: StepSnapshot[];
}
export type HandoffPlane = 'native' | 'managed';
export type HandoffProvenance =
  'human_entered' | 'provider_reported' | 'provider_output_untrusted';
export type HandoffItemKind =
  'objective' | 'excerpt' | 'file_reference' | 'open_question';
export interface HandoffItemV1 {
  item_id: string;
  source_revision: string;
  kind: HandoffItemKind;
  provenance: HandoffProvenance;
  text: string;
}
export interface HandoffEnvelopeV1 {
  schema_version: 1;
  handoff_id: string;
  revision: number;
  source: {
    host_id: string;
    session_id: string;
    session_generation: number;
    plane: HandoffPlane;
    project_id: string;
    workspace_id: string;
    workspace_revision: string;
    workspace_root: string;
  };
  target: {
    host_id: string;
    project_id: string;
    workspace_id: string;
    workspace_revision: string;
    workspace_root: string;
    provider: ProviderLane;
    effect: EffectClass;
    managed_session_id: string | null;
    native_profile_id: string | null;
    profile_revision: number | null;
  };
  objective: string;
  selected_context_items: HandoffItemV1[];
  selected_file_references: HandoffItemV1[];
  open_questions: HandoffItemV1[];
  created_at_unix: number;
  expires_at_unix: number;
  content_digest: string;
}
export interface HandoffConfirmationV1 {
  handoff_id: string;
  revision: number;
  reviewed_digest: string;
  confirmed_at_unix: number;
}
export interface HandoffPreview {
  envelope: HandoffEnvelopeV1;
  first_message: string;
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
export function isDigest(value: unknown): value is string {
  return typeof value === 'string' && /^[0-9a-f]{64}$/.test(value);
}
