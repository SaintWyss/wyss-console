import type {
  Binding,
  EnforcementState,
  PlanConflictCode,
  PlanItemState,
  TerminalPlan,
} from '../../contracts/arsenal';
import type { NativeSession, Preview } from '../../contracts/native';
import type { ArsenalStart } from '../../desktop/native-client';
import { msg } from '../../i18n/msg';

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
  available: msg('Can be applied'),
  unqualified: msg('Not qualified for this build'),
  version_unknown: msg('Build not identified'),
  incomplete_inventory: msg('Installed skills not fully read'),
};

export const ITEM_STATE_LABEL: Record<PlanItemState, string> = {
  prepared: msg('prepared'),
  rejected: msg('rejected'),
  unsupported: msg('not carried'),
  missing: msg('missing'),
};

export const PLAN_CONFLICT_LABEL: Record<PlanConflictCode, string> = {
  name_collision: msg('Same name twice'),
  duplicate_content: msg('Same content twice'),
  instruction_overlap: msg('Several rules'),
  methodology_conflict: msg('Two methodologies kept'),
  scope_shadow: msg('Installed in account and project'),
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
 * The installed skills a plan leaves on and off, as the panel counts them: `kept` includes the ones
 * the owner's settings already switch off (`inherited_off`), and `excluded` are the ones it switches off.
 */
export function onOffCounts(
  plan: Pick<TerminalPlan, 'kept' | 'inherited_off' | 'excluded'>,
): { on: number; off: number } {
  return {
    on: plan.kept.length - plan.inherited_off.length,
    off: plan.inherited_off.length + plan.excluded.length,
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
