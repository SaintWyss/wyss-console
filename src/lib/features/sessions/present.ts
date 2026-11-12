import type {
  AgentMode,
  RuntimeFacts,
  SessionBinding,
  SessionFailureCode,
  SessionMode,
  SessionSummary,
} from '../../contracts/sessions';
import { t } from '../../i18n/index.svelte';
import { msg, type Phrase } from '../../i18n/msg';
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

/**
 * The last segment of the bound workspace, or a short form of the session's identity. A name is
 * a string the app keeps, compares and sorts, so its fallback is drawn here, where it is made.
 */
export function sessionLabel(session: {
  id: string;
  workspace: string | null;
}): string {
  return (
    folderName(session.workspace) ??
    t('Session {id}', { id: shortId(session.id) })
  );
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
  prepared: { label: msg('Prepared'), tone: 'ok' },
  closed: { label: msg('Closed'), tone: 'idle' },
};

export const BINDING: Record<
  SessionBinding,
  { label: string; tone: RuntimeTone; detail: string }
> = {
  unbound: {
    label: msg('No workspace'),
    tone: 'idle',
    detail: msg('No workspace is bound to this session.'),
  },
  creating: {
    label: msg('Binding'),
    tone: 'pending',
    detail: msg('The authority is binding a workspace to this session.'),
  },
  indeterminate: {
    label: msg('Uncertain'),
    tone: 'warn',
    detail: msg(
      'The authority could not confirm whether the workspace binding was created.',
    ),
  },
  bound_detached: {
    label: msg('Detached'),
    tone: 'idle',
    detail: msg('Bound to a workspace and not attached to the running agent.'),
  },
  bound_attached: {
    label: msg('Ready'),
    tone: 'ok',
    detail: msg('Bound to a workspace and attached to the running agent.'),
  },
};

/**
 * The two modes, in the words the product uses for them. The labels are product names and stay
 * as they are; only the sentence that says what the mode does is translated.
 */
export const AGENT_MODE: Record<AgentMode, { label: string; detail: string }> =
  {
    inference: {
      label: 'Inference',
      detail: msg('Text only. Codex is offered no tool.'),
    },
    local_tools: {
      label: 'Local Tools',
      detail: msg(
        'Codex may propose commands and file changes in this folder. Each one waits for your approval.',
      ),
    },
  };
/**
 * The model of a managed session as the runtime reported it. Said as what it is: a report,
 * not a setting, a default or an entitlement. Nothing is reported while the session is not
 * attached, and then the line says so instead of showing a guess.
 */
export function observedModelLine(
  session: Pick<SessionSummary, 'binding' | 'observed_model'>,
): Phrase {
  const model = session.observed_model;
  if (model === null)
    return {
      text:
        session.binding === 'bound_attached'
          ? msg('Model in use: not reported by the runtime')
          : msg('Model in use: not known while detached'),
    };
  const effort = model.reasoning_effort
    ? {
        text: msg(', effort {level}'),
        values: { level: model.reasoning_effort },
      }
    : '';
  if (model.source === 'turn_rerouted')
    return {
      text: msg(
        'Model in use: {model}{effort} (the runtime said it rerouted a turn from {from}, {reason}; whether later turns stay on it is not documented)',
      ),
      values: {
        model: model.model_id,
        effort,
        from: String(model.rerouted_from),
        reason: {
          text:
            model.reroute_reason === 'high_risk_cyber_activity'
              ? msg('for a safety reason it named')
              : msg('for a reason this app does not know'),
        },
      },
    };
  return {
    text:
      model.source === 'thread_resume'
        ? msg(
            'Model in use: {model}{effort} (reported by the runtime when the session was resumed)',
          )
        : msg(
            'Model in use: {model}{effort} (reported by the runtime when the session was started)',
          ),
    values: { model: model.model_id, effort },
  };
}

export function modeLabel(mode: SessionMode): Phrase {
  return mode === 'unknown'
    ? { text: msg('Not attached') }
    : { text: AGENT_MODE[mode].label };
}
/** What the one managed runtime is doing now. Null when the runtime is not known to run a mode. */
export function runtimeLine(runtime: RuntimeFacts): Phrase | null {
  return runtime.mode === null
    ? null
    : runningIn(runtime.mode, runtime.workspace);
}
function runningIn(mode: AgentMode, workspace: string | null): Phrase {
  return mode === 'local_tools'
    ? {
        text: msg('Codex runs in Local Tools for {folder}'),
        values: {
          folder: folderName(workspace) ?? { text: msg('a workspace') },
        },
      }
    : { text: msg('Codex runs in Inference') };
}
/**
 * What a change of mode does, said before it is asked for. The runtime is one and
 * shared: nothing here softens that.
 */
export function modeImpact(mode: AgentMode): Phrase {
  return {
    text:
      mode === 'local_tools'
        ? msg(
            'This restarts the managed Codex runtime in Local Tools for this workspace. Sessions attached now are detached. A session created in Inference cannot be resumed in Local Tools. Every command and file change still waits for your approval.',
          )
        : msg(
            'This restarts the managed Codex runtime in Inference. Sessions attached now are detached. A session created in Local Tools cannot be resumed in Inference.',
          ),
  };
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
): Phrase | null {
  if (runtime.mode === null) return null;
  const elsewhere =
    mode === 'local_tools' &&
    workspace !== null &&
    runtime.workspace !== workspace;
  if (runtime.mode === mode && !elsewhere) return null;
  const restart: Phrase = {
    text: msg(
      '{current} now, and the authority runs one Codex runtime. Activating this session restarts it in {to}, and the sessions attached to it are detached.',
    ),
    values: {
      current: runningIn(runtime.mode, runtime.workspace),
      to:
        mode === 'local_tools'
          ? { text: msg('Local Tools for this folder') }
          : { text: AGENT_MODE.inference.label },
    },
  };
  if (runtime.mode === mode) return restart;
  return {
    text: msg('{restart} {kept}'),
    values: {
      restart,
      kept: {
        text: msg('A session created in {from} cannot be resumed in {to}.'),
        values: {
          from: AGENT_MODE[runtime.mode].label,
          to: AGENT_MODE[mode].label,
        },
      },
    },
  };
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
  resume: msg('Resuming session…'),
  close: msg('Closing session…'),
  abandon: msg('Discarding the uncertain attempt…'),
};
const ACTIVATING: Record<ActivationStep, string> = {
  preparing: msg('Preparing session…'),
  stopping: msg('Stopping the managed runtime…'),
  starting: msg('Starting Codex…'),
  binding: msg('Creating workspace binding…'),
  attaching: msg('Attaching…'),
};
/** What a pending action is doing, in the words of its current step. */
export function pendingMessage(
  kind: OperationKind,
  step: ActivationStep | undefined,
  mode?: AgentMode,
): Phrase {
  if (kind !== 'activate' && kind !== 'mode') return { text: PENDING[kind] };
  // Local Tools is the mode that is asked for by name: when it is the one coming up,
  // that is part of what is happening.
  return step === 'starting' && mode === 'local_tools'
    ? {
        text: msg('Starting Codex in {mode}…'),
        values: { mode: AGENT_MODE[mode].label },
      }
    : { text: ACTIVATING[step ?? 'starting'] };
}

const ACTION: Record<OperationKind, string> = {
  activate: msg('The session is not ready'),
  mode: msg('The session is not ready in that mode'),
  resume: msg('The session was not resumed'),
  close: msg('The session was not closed'),
  abandon: msg('The attempt was not discarded'),
};

const FAILURE: Record<SessionFailureCode, string> = {
  unavailable: msg(
    'The authority did not answer, so the result is unknown. Retrying repeats the same request safely.',
  ),
  incompatible: msg('This console and the authority speak different versions.'),
  invalid: msg(
    'The console sent a request it should not have. Nothing changed.',
  ),
  not_found: msg('The authority no longer has this session.'),
  changed: msg(
    'The session changed after it was shown. It has been read again.',
  ),
  not_allowed: msg('The session is closed, so the authority refused.'),
  not_bound: msg('This session has no workspace binding to resume.'),
  runtime_unavailable: msg(
    'The authority denied it because the agent runtime is not ready.',
  ),
  start_refused: msg('The authority refused to start Codex.'),
  turn_active: msg(
    'A turn is running, and the runtime it runs on is not restarted under it. Wait for it or cancel it.',
  ),
  mode_fixed: msg(
    'This session is attached in the other mode: its conversation exists only there.',
  ),
  workspace_invalid: msg(
    'The authority did not accept that folder as a workspace. Nothing was bound.',
  ),
  already_bound: msg(
    'The session already has a workspace binding. It has been read again.',
  ),
  permission_denied: msg(
    'The authority denied it: no execution lease allows this for the session.',
  ),
  busy: msg(
    'The authority is busy with another operation. Try again in a moment.',
  ),
  rejected: msg('The agent refused to resume this session.'),
  uncertain: msg(
    'The authority could not confirm what the agent did. Check the session before trying again.',
  ),
  conflict: msg('That request was already used for something else. Try again.'),
  internal: msg(
    'Something unexpected happened. Nothing is assumed to have changed.',
  ),
};
/** Where an action gives a code another meaning than the common one. */
const SPECIFIC: Partial<
  Record<OperationKind, Partial<Record<SessionFailureCode, string>>>
> = {
  activate: {
    not_allowed: msg('The authority refused: the session no longer allows it.'),
    runtime_unavailable: msg(
      'Codex is not ready, so the session could not be bound or attached.',
    ),
    rejected: msg('Codex refused the request.'),
  },
  mode: {
    not_allowed: msg('The authority refused: the session no longer allows it.'),
    runtime_unavailable: msg(
      'Codex did not come up in that mode. The runtime is as the authority reports it.',
    ),
    rejected: msg(
      'Codex refused to resume this session in that mode. A session resumes only in the mode it was created in. The runtime now runs in the mode that was asked for.',
    ),
    not_bound: msg('This session has no workspace to run a mode for.'),
  },
  resume: {
    // A detached session has no mode the authority reports: this is where it shows.
    rejected: msg(
      'Codex refused to resume this session. A session resumes only in the mode it was created in: if Codex runs in the other one now, change the mode for this session instead.',
    ),
  },
  abandon: {
    not_allowed: msg(
      'The authority holds no uncertain attempt for this session any more.',
    ),
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
): Phrase {
  if (code === 'unavailable') return { text: FAILURE[code] };
  // Local Tools runs only inside the isolation the authority can set up and prove.
  if (code === 'start_refused' && mode === 'local_tools')
    return {
      text: msg(
        '{action}. The authority refused to start Codex in Local Tools: it could not set up or prove the isolation that mode requires. Nothing runs without it.',
      ),
      values: { action: { text: ACTION[kind] } },
    };
  if (kind === 'activate' && code === 'uncertain')
    return {
      text: msg(
        'Activation outcome uncertain: the authority could not confirm whether Codex created the workspace binding. Nothing is sent again by itself.',
      ),
    };
  return {
    text: msg('{action}. {reason}'),
    values: {
      action: { text: ACTION[kind] },
      reason: { text: SPECIFIC[kind]?.[code] ?? FAILURE[code] },
    },
  };
}

/**
 * Whether a failed action is one whose outcome is not known, as opposed to one that
 * was refused. It is shown as a caution, not as an error.
 */
export function outcomeUnknown(code: SessionFailureCode): boolean {
  return code === 'unavailable' || code === 'uncertain';
}

export function readFailureMessage(code: SessionFailureCode): Phrase {
  if (code === 'unavailable')
    return {
      text: msg('Sessions could not be read: the authority did not answer.'),
    };
  if (code === 'incompatible' || code === 'busy')
    return {
      text: msg('Sessions could not be read. {reason}'),
      values: { reason: { text: FAILURE[code] } },
    };
  return {
    text: msg(
      'Sessions could not be read. Nothing is shown rather than something unverified.',
    ),
  };
}

/** Why no folder came back from the dialog, when that is not the user's own doing. */
export function chooseFailureMessage(code: SessionFailureCode): Phrase {
  return {
    text:
      code === 'workspace_invalid'
        ? msg(
            'That folder cannot be used: its path is too long or cannot be written as text.',
          )
        : code === 'busy'
          ? msg('The folder dialog is already open.')
          : msg('The folder dialog could not be opened.'),
  };
}

/** Why Resume is not offered, when it is not. Null when it is. */
export function resumeHint(
  session: SessionSummary,
  agentReady: boolean,
): Phrase | null {
  if (session.state === 'closed')
    return { text: msg('A closed session cannot be resumed.') };
  if (session.binding === 'bound_attached')
    return { text: msg('Ready: attached to the running agent.') };
  // The binding itself is described next to it; this says what it means for Resume.
  if (!session.resumable)
    return {
      text:
        session.binding === 'unbound'
          ? msg('Resume needs a workspace bound to the session.')
          : msg('Resume has to wait until the workspace binding is settled.'),
    };
  if (!agentReady)
    return {
      text: msg(
        'The agent runtime is not ready, so there is nothing to attach to.',
      ),
    };
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
