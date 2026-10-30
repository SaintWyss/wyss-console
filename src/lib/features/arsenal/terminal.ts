import type {
  Binding,
  EnforcementState,
  PlanConflictCode,
  PlanItemState,
  TerminalPlan,
} from '../../contracts/arsenal';
import type { NativeSession, Preview } from '../../contracts/native';
import type { ArsenalStart } from '../../desktop/native-client';

/**
 * What the Workspace knows about the Arsenal part of a start. `pending` while the plan is read;
 * then the plan, whether it is applied, the Native preview that binds it, and whether Start may go.
 */
export type ArsenalChoice =
  | { pending: true }
  | {
      pending: false;
      start: ArsenalStart;
      plan: TerminalPlan;
      apply: boolean;
      preview: Preview | null;
      ready: boolean;
    };

export const ENFORCEMENT_LABEL: Record<EnforcementState, string> = {
  available: 'Can be applied',
  unqualified: 'Not qualified for this build',
  version_unknown: 'Build not identified',
  incomplete_inventory: 'Installed skills not fully read',
};

export const ITEM_STATE_LABEL: Record<PlanItemState, string> = {
  prepared: 'prepared',
  rejected: 'rejected',
  unsupported: 'not carried',
  missing: 'missing',
};

export const PLAN_CONFLICT_LABEL: Record<PlanConflictCode, string> = {
  name_collision: 'Same name twice',
  duplicate_content: 'Same content twice',
  instruction_overlap: 'Several rules',
  methodology_conflict: 'Two methodologies kept',
  scope_shadow: 'Installed in account and project',
};

export function planCounts(plan: TerminalPlan) {
  const count = (state: PlanItemState) =>
    plan.items.filter((item) => item.state === state).length;
  return {
    prepared: count('prepared'),
    rejected: count('rejected') + count('missing'),
    unsupported: count('unsupported'),
  };
}

/**
 * One line on what a session started with. A session the host launched with an exclusion says so;
 * one that only has a record says it was not applied. Nothing says a skill was used.
 */
export function sessionArsenalLine(
  session: Pick<NativeSession, 'id' | 'arsenal'>,
  bindings: readonly Binding[],
): string | null {
  if (session.arsenal) {
    return `Arsenal: ${session.arsenal.selection_name} · applied · ${session.arsenal.skills_off} installed skill(s) off · use not observed`;
  }
  const binding = bindings.find((b) => b.session_id === session.id);
  if (!binding) return null;
  return binding.state === 'applied'
    ? `Arsenal: ${binding.selection_name} · applied · use not observed`
    : `Arsenal: ${binding.selection_name} · NOT APPLIED`;
}
