import type {
  AgentMode,
  RuntimeFacts,
  SessionBinding,
  SessionFailureCode,
  SessionMode,
  SessionSummary,
} from '../../contracts/sessions';
import type { RuntimeTone } from '../../shell/runtime';
import type { ActivationStep, OperationKind } from './controller';

/** Presentation only: nothing here is read back as state. */

export function agentName(adapter: string): string {
  return adapter === 'codex' ? 'Codex' : adapter;
}

/** The last segment of an absolute workspace path, or null when it has none. */
export function folderName(workspace: string | null): string | null {
  return workspace?.split('/').filter(Boolean).at(-1) ?? null;
}
/** The short form of a session's identity, as it is written where space is scarce. */
export function shortId(id: string): string {
  return id.slice(0, 8);
}

/** The last segment of the bound workspace, or a short form of the session's identity. */
export function sessionLabel(session: {
  id: string;
  workspace: string | null;
}): string {
  return folderName(session.workspace) ?? `Session ${shortId(session.id)}`;
}

/** What a name is made from. Everything else about a session is not part of it. */
export interface NameSource {
  id: string;
  workspace: string | null;
  created_at?: string | null;
}

/** The last `count` segments of a path. */
function tail(workspace: string, count: number): string {
  return workspace.split('/').filter(Boolean).slice(-count).join('/');
}

/**
 * Makes every name in `names` unique. A name that is already unique is left as it is; sessions
 * that still share one take their full identity as well, which no other session has. This is the
 * last word on uniqueness: a suffix made for one group can equal a name made for another, or a
 * folder can be named like a suffix, and neither can be ruled out by the groups alone.
 */
export function ensureUnique<K extends string>(
  names: ReadonlyMap<K, string>,
): Map<K, string> {
  const count = new Map<string, number>();
  for (const name of names.values())
    count.set(name, (count.get(name) ?? 0) + 1);
  const unique = new Map(names);
  const taken = new Set(
    [...names.values()].filter((name) => count.get(name) === 1),
  );
  // In identity order, so the result does not depend on the order the sessions came in.
  for (const id of [...names.keys()].sort()) {
    const name = names.get(id) ?? '';
    if (count.get(name) === 1) continue;
    let candidate = `${name} · ${id}`;
    while (taken.has(candidate)) candidate += ' ·';
    taken.add(candidate);
    unique.set(id, candidate);
  }
  return unique;
}

/**
 * A name for each session that no other session in the same set has.
 *
 * A session is named for its folder. Two folders that end in the same name are told
 * apart by their parents (`a/app`, `b/app`). Several sessions in one folder, which the
 * product asks for when a project needs both Inference and Local Tools, are told apart
 * by when they were created, or by their short identity when that does not do it. A name
 * only gains words when it is needed, so it can change when another session appears in
 * the same set, and it never changes what the session is. No two names are equal: see
 * [`ensureUnique`].
 */
export function sessionNames(
  sessions: readonly NameSource[],
): Map<string, string> {
  const names = new Map<string, string>();
  const groups = new Map<string, NameSource[]>();
  for (const session of sessions) {
    if (names.has(session.id)) continue;
    names.set(session.id, '');
    const base = sessionLabel(session);
    groups.set(base, [...(groups.get(base) ?? []), session]);
  }
  for (const [base, group] of groups) {
    // Only a folder with a name of its own can be told apart by its parents. A session bound
    // to `/` has none, and is named by its identity like one with no folder.
    const named = (workspace: string | null): workspace is string =>
      folderName(workspace) !== null;
    const folders = [...new Set(group.map((s) => s.workspace).filter(named))];
    let depth = 1;
    if (folders.length > 1) {
      const deepest = Math.max(
        ...folders.map((f) => f.split('/').filter(Boolean).length),
      );
      while (
        depth < deepest &&
        new Set(folders.map((f) => tail(f, depth))).size < folders.length
      )
        depth++;
    }
    const sharing = new Map<string, NameSource[]>();
    for (const session of group) {
      const label =
        named(session.workspace) && folders.length > 1
          ? tail(session.workspace, depth)
          : base;
      sharing.set(label, [...(sharing.get(label) ?? []), session]);
    }
    for (const [label, members] of sharing) {
      if (members.length === 1) {
        names.set(members[0].id, label);
        continue;
      }
      const stamps = members.map((m) =>
        m.created_at ? localTime(m.created_at) : null,
      );
      const told = stamps.every((stamp) => stamp !== null)
        ? new Set(stamps).size === members.length
        : false;
      members.forEach((member, index) =>
        names.set(
          member.id,
          `${label} · ${told ? stamps[index] : shortId(member.id)}`,
        ),
      );
    }
  }
  return ensureUnique(names);
}

export const STATE: Record<
  SessionSummary['state'],
  { label: string; tone: RuntimeTone }
> = {
  prepared: { label: 'Prepared', tone: 'ok' },
  closed: { label: 'Closed', tone: 'idle' },
};

export const BINDING: Record<
  SessionBinding,
  { label: string; tone: RuntimeTone; detail: string }
> = {
  unbound: {
    label: 'No workspace',
    tone: 'idle',
    detail: 'No workspace is bound to this session.',
  },
  creating: {
    label: 'Binding',
    tone: 'pending',
    detail: 'The authority is binding a workspace to this session.',
  },
  indeterminate: {
    label: 'Uncertain',
    tone: 'warn',
    detail:
      'The authority could not confirm whether the workspace binding was created.',
  },
  bound_detached: {
    label: 'Detached',
    tone: 'idle',
    detail: 'Bound to a workspace and not attached to the running agent.',
  },
  bound_attached: {
    label: 'Ready',
    tone: 'ok',
    detail: 'Bound to a workspace and attached to the running agent.',
  },
};

/** The two modes, in the words the product uses for them. */
export const AGENT_MODE: Record<AgentMode, { label: string; detail: string }> =
  {
    inference: {
      label: 'Inference',
      detail: 'Text only. Codex is offered no tool.',
    },
    local_tools: {
      label: 'Local Tools',
      detail:
        'Codex may propose commands and file changes in this folder. Each one waits for your approval.',
    },
  };
/**
 * The model of a managed session as the runtime reported it. Said as what it is: a report,
 * not a setting, a default or an entitlement. Nothing is reported while the session is not
 * attached, and then the line says so instead of showing a guess.
 */
export function observedModelLine(
  session: Pick<SessionSummary, 'binding' | 'observed_model'>,
): string {
  const model = session.observed_model;
  if (model === null)
    return session.binding === 'bound_attached'
      ? 'Model in use: not reported by the runtime'
      : 'Model in use: not known while detached';
  const effort = model.reasoning_effort
    ? `, effort ${model.reasoning_effort}`
    : '';
  return `Model in use: ${model.model_id}${effort} (reported by the runtime when the session was ${model.source === 'thread_resume' ? 'resumed' : 'started'})`;
}

export function modeLabel(mode: SessionMode): string {
  return mode === 'unknown' ? 'Not attached' : AGENT_MODE[mode].label;
}
/** What the one managed runtime is doing now, for the Sessions heading. */
export function runtimeLine(runtime: RuntimeFacts): string | null {
  if (runtime.mode === null) return null;
  return runtime.mode === 'local_tools'
    ? `Codex runs in Local Tools for ${runtime.workspace?.split('/').filter(Boolean).at(-1) ?? 'a workspace'}`
    : 'Codex runs in Inference';
}
/**
 * What a change of mode does, said before it is asked for. The runtime is one and
 * shared: nothing here softens that.
 */
export function modeImpact(mode: AgentMode): string {
  return mode === 'local_tools'
    ? 'This restarts the managed Codex runtime in Local Tools for this workspace. Sessions attached now are detached. A session created in Inference cannot be resumed in Local Tools. Every command and file change still waits for your approval.'
    : 'This restarts the managed Codex runtime in Inference. Sessions attached now are detached. A session created in Local Tools cannot be resumed in Inference.';
}
/**
 * What activating a session in `mode` does to the runtime that is running now, when it
 * is known to serve something else. Null when nothing is restarted, or when that cannot
 * be told yet. Folders are compared as text, so the same folder spelled another way is
 * announced as a restart that the authority then has no need for.
 */
export function composeImpact(
  mode: AgentMode,
  workspace: string | null,
  runtime: RuntimeFacts,
): string | null {
  if (runtime.mode === null) return null;
  const elsewhere =
    mode === 'local_tools' &&
    workspace !== null &&
    runtime.workspace !== workspace;
  if (runtime.mode === mode && !elsewhere) return null;
  const to = `${AGENT_MODE[mode].label}${mode === 'local_tools' ? ' for this folder' : ''}`;
  const kept =
    runtime.mode === mode
      ? ''
      : ` A session created in ${AGENT_MODE[runtime.mode].label} cannot be resumed in ${AGENT_MODE[mode].label}.`;
  return `${runtimeLine(runtime)} now, and the authority runs one Codex runtime. Activating this session restarts it in ${to}, and the sessions attached to it are detached.${kept}`;
}
/** Which change of mode to offer for a session that is bound and not attached. */
export function modeOffers(
  session: SessionSummary,
  runtime: RuntimeFacts,
): AgentMode[] {
  if (session.state !== 'prepared' || session.binding !== 'bound_detached')
    return [];
  const offers: AgentMode[] = [];
  // Not offered when plain Resume already means it: the runtime serves that mode.
  if (runtime.mode !== 'local_tools' || runtime.workspace !== session.workspace)
    offers.push('local_tools');
  if (runtime.mode === 'local_tools') offers.push('inference');
  return offers;
}

const PENDING: Record<Exclude<OperationKind, 'activate' | 'mode'>, string> = {
  resume: 'Resuming session…',
  close: 'Closing session…',
  abandon: 'Discarding the uncertain attempt…',
};
const ACTIVATING: Record<ActivationStep, string> = {
  preparing: 'Preparing session…',
  stopping: 'Stopping the managed runtime…',
  starting: 'Starting Codex…',
  binding: 'Creating workspace binding…',
  attaching: 'Attaching…',
};
/** What a pending action is doing, in the words of its current step. */
export function pendingMessage(
  kind: OperationKind,
  step: ActivationStep | undefined,
  mode?: AgentMode,
): string {
  if (kind !== 'activate' && kind !== 'mode') return PENDING[kind];
  const doing = ACTIVATING[step ?? 'starting'];
  // Local Tools is the mode that is asked for by name: when it is the one coming up,
  // that is part of what is happening.
  return step === 'starting' && mode === 'local_tools'
    ? `Starting Codex in ${AGENT_MODE[mode].label}…`
    : doing;
}

const ACTION: Record<OperationKind, string> = {
  activate: 'The session is not ready',
  mode: 'The session is not ready in that mode',
  resume: 'The session was not resumed',
  close: 'The session was not closed',
  abandon: 'The attempt was not discarded',
};

const FAILURE: Record<SessionFailureCode, string> = {
  unavailable:
    'The authority did not answer, so the result is unknown. Retrying repeats the same request safely.',
  incompatible: 'This console and the authority speak different versions.',
  invalid: 'The console sent a request it should not have. Nothing changed.',
  not_found: 'The authority no longer has this session.',
  changed: 'The session changed after it was shown. It has been read again.',
  not_allowed: 'The session is closed, so the authority refused.',
  not_bound: 'This session has no workspace binding to resume.',
  runtime_unavailable:
    'The authority denied it because the agent runtime is not ready.',
  start_refused: 'The authority refused to start Codex.',
  turn_active:
    'A turn is running, and the runtime it runs on is not restarted under it. Wait for it or cancel it.',
  mode_fixed:
    'This session is attached in the other mode: its conversation exists only there.',
  workspace_invalid:
    'The authority did not accept that folder as a workspace. Nothing was bound.',
  already_bound:
    'The session already has a workspace binding. It has been read again.',
  permission_denied:
    'The authority denied it: no execution lease allows this for the session.',
  busy: 'The authority is busy with another operation. Try again in a moment.',
  rejected: 'The agent refused to resume this session.',
  uncertain:
    'The authority could not confirm what the agent did. Check the session before trying again.',
  conflict: 'That request was already used for something else. Try again.',
  internal:
    'Something unexpected happened. Nothing is assumed to have changed.',
};
/** Where an action gives a code another meaning than the common one. */
const SPECIFIC: Partial<
  Record<OperationKind, Partial<Record<SessionFailureCode, string>>>
> = {
  activate: {
    not_allowed: 'The authority refused: the session no longer allows it.',
    runtime_unavailable:
      'Codex is not ready, so the session could not be bound or attached.',
    rejected: 'Codex refused the request.',
  },
  mode: {
    not_allowed: 'The authority refused: the session no longer allows it.',
    runtime_unavailable:
      'Codex did not come up in that mode. The runtime is as the authority reports it.',
    rejected:
      'Codex refused to resume this session in that mode. A session resumes only in the mode it was created in. The runtime now runs in the mode that was asked for.',
    not_bound: 'This session has no workspace to run a mode for.',
  },
  resume: {
    // A detached session has no mode the authority reports: this is where it shows.
    rejected:
      'Codex refused to resume this session. A session resumes only in the mode it was created in: if Codex runs in the other one now, change the mode for this session instead.',
  },
  abandon: {
    not_allowed:
      'The authority holds no uncertain attempt for this session any more.',
  },
};

/**
 * One sentence for a failed action. An unknown outcome is never worded as a refusal:
 * neither an answer that did not come nor an activation the authority could not confirm.
 */
export function failureMessage(
  kind: OperationKind,
  code: SessionFailureCode,
  mode?: AgentMode,
): string {
  if (code === 'unavailable') return FAILURE[code];
  // Local Tools runs only inside the isolation the authority can set up and prove.
  if (code === 'start_refused' && mode === 'local_tools')
    return `${ACTION[kind]}. The authority refused to start Codex in Local Tools: it could not set up or prove the isolation that mode requires. Nothing runs without it.`;
  if (kind === 'activate' && code === 'uncertain')
    return 'Activation outcome uncertain: the authority could not confirm whether Codex created the workspace binding. Nothing is sent again by itself.';
  return `${ACTION[kind]}. ${SPECIFIC[kind]?.[code] ?? FAILURE[code]}`;
}

/**
 * Whether a failed action is one whose outcome is not known, as opposed to one that
 * was refused. It is shown as a caution, not as an error.
 */
export function outcomeUnknown(code: SessionFailureCode): boolean {
  return code === 'unavailable' || code === 'uncertain';
}

export function readFailureMessage(code: SessionFailureCode): string {
  return code === 'unavailable'
    ? 'Sessions could not be read: the authority did not answer.'
    : code === 'incompatible'
      ? `Sessions could not be read. ${FAILURE.incompatible}`
      : code === 'busy'
        ? `Sessions could not be read. ${FAILURE.busy}`
        : 'Sessions could not be read. Nothing is shown rather than something unverified.';
}

/** Why no folder came back from the dialog, when that is not the user's own doing. */
export function chooseFailureMessage(code: SessionFailureCode): string {
  return code === 'workspace_invalid'
    ? 'That folder cannot be used: its path is too long or cannot be written as text.'
    : code === 'busy'
      ? 'The folder dialog is already open.'
      : 'The folder dialog could not be opened.';
}

/** Why Resume is not offered, when it is not. Null when it is. */
export function resumeHint(
  session: SessionSummary,
  agentReady: boolean,
): string | null {
  if (session.state === 'closed') return 'A closed session cannot be resumed.';
  if (session.binding === 'bound_attached')
    return 'Ready: attached to the running agent.';
  // The binding itself is described next to it; this says what it means for Resume.
  if (!session.resumable)
    return session.binding === 'unbound'
      ? 'Resume needs a workspace bound to the session.'
      : 'Resume has to wait until the workspace binding is settled.';
  if (!agentReady)
    return 'The agent runtime is not ready, so there is nothing to attach to.';
  return null;
}

const DATE = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'medium',
  timeStyle: 'short',
});
/** Local date and time of an authority timestamp. */
export function localTime(timestamp: string): string {
  const time = Date.parse(timestamp);
  return Number.isNaN(time) ? timestamp : DATE.format(time);
}
