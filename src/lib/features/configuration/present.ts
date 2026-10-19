import type {
  ConfigItem,
  ConfigurationFailure,
  Effect,
  Effective,
  ItemKind,
  NotWritable,
  Operation,
  Outcome,
  Persisted,
  Projection,
  Protection,
  Provider,
  Risk,
  Verdict,
  SecretClass,
  SourceFailure,
  SourceKey,
  SourceReport,
} from '../../contracts/configuration';
import type { RuntimeTone } from '../../shell/runtime';

/** Presentation only: nothing here is read back as state. */

export const PROVIDER_LABEL: Record<Provider, string> = {
  claude_code: 'Claude Code',
  codex: 'Codex',
};

export const KIND_LABEL: Record<ItemKind, string> = {
  hook: 'Hooks',
  skill: 'Skills',
  mcp_server: 'MCP servers',
  rule_file: 'Rule files',
  model_preference: 'Models',
};
export const KIND_ORDER: readonly ItemKind[] = [
  'hook',
  'skill',
  'mcp_server',
  'rule_file',
  'model_preference',
];
const KIND_SINGULAR: Record<ItemKind, string> = {
  hook: 'Hook',
  skill: 'Skill',
  mcp_server: 'MCP server',
  rule_file: 'Rule file',
  model_preference: 'Model setting',
};
export const kindName = (kind: ItemKind): string => KIND_SINGULAR[kind];

/** What the file says. Whether the provider honours it is a separate line. */
export const PERSISTED_LABEL: Record<
  Persisted,
  { tone: RuntimeTone; label: string }
> = {
  present_enabled: { tone: 'ok', label: 'On in the file' },
  present_disabled: { tone: 'warn', label: 'Off in the file' },
  absent: { tone: 'idle', label: 'Not in the file' },
  parked: { tone: 'idle', label: 'Parked' },
  unreadable: { tone: 'error', label: 'Could not be read' },
};

export const EFFECTIVE_LABEL: Record<Effective, string> = {
  verified_enabled: 'Confirmed active',
  verified_disabled: 'Confirmed inactive',
  documented: 'As the provider documents it',
  not_verifiable: 'Cannot be confirmed without running the provider',
};

export const SCOPE_LABEL = {
  system: 'System',
  global: 'Your account',
  project: 'Project',
} as const;

export const SOURCE_LABEL: Record<SourceKey, string> = {
  claude_managed_settings: 'Managed settings',
  claude_user_settings: 'User settings',
  claude_user_state: 'Account file (MCP servers only)',
  claude_user_skills: 'User skills folder',
  claude_user_rules: 'User rules',
  claude_project_settings: 'Project settings',
  claude_project_local_settings: 'Project local settings',
  claude_project_mcp: 'Project MCP file',
  claude_project_skills: 'Project skills folder',
  claude_project_rules: 'Project rules',
  codex_system_requirements: 'Administrator requirements',
  codex_system_managed: 'Managed configuration',
  codex_user_config: 'User configuration',
  codex_user_hooks: 'User hooks file',
  codex_user_rules: 'User instructions',
  codex_project_config: 'Project configuration',
  codex_project_rules: 'Project instructions',
};

/** Why nothing can be changed here. Every control that would change something is absent. */
export const NOT_WRITABLE_LABEL: Record<NotWritable, string> = {
  system: 'Set by the system; this app never changes it',
  provider_owned: 'Owned by the provider; changed only through its own command',
  generated: 'Produced by another tool',
  shared_path: 'Its folder can be written by a group: read, never changed',
  not_in_this_phase: 'Read-only for now: changing it is not available yet',
};

export const SECRET_LABEL: Record<SecretClass, string> = {
  environment_value: 'environment value',
  header: 'header',
  url_credentials: 'address credentials',
  secret_field: 'secret field',
  token_shape: 'token',
};

/** One sentence for what became of a source. Never a path, never an operating-system text. */
export function failureText(failure: SourceFailure): string {
  switch (failure.failure) {
    case 'missing':
      return 'It is not there.';
    case 'unreadable':
      return 'It could not be read.';
    case 'unstable':
      return 'It kept changing while it was being read. It will be read again.';
    case 'credential_file':
      return 'It holds credentials, so it is never opened.';
    case 'untrusted_path':
      return failure.reason === 'outside_roots'
        ? 'Where it leads is outside the places this app may read.'
        : 'Its place is not safe to read from: someone else owns or can change it.';
    case 'unsupported':
      return failure.reason === 'link_limit'
        ? 'It is behind too many links.'
        : 'It is not a regular file.';
    case 'too_large':
      return 'It is larger than the bounds this app reads.';
    case 'malformed':
      return `It is not valid: reading stopped at line ${failure.line}, column ${failure.column}.`;
  }
}

/** The line shown under a source: what happened, in words. */
export function outcomeText(source: SourceReport): string {
  switch (source.outcome) {
    case 'read':
      if (source.conflict_copy)
        return 'Read. A conflict copy of this file sits beside it, so it cannot be changed from here until that copy is dealt with.';
      return source.unrecognised > 0
        ? `Read. ${source.unrecognised} ${source.unrecognised === 1 ? 'entry was' : 'entries were'} not recognised and left untouched.`
        : 'Read.';
    case 'absent':
      return 'Not present.';
    case 'project_not_trusted':
      return 'Not read: Codex loads a project’s own configuration only for a project you trust.';
    case 'failed':
      return failureText(source.failure);
  }
}
export function outcomeTone(source: SourceReport): RuntimeTone {
  switch (source.outcome) {
    case 'read':
      return source.unrecognised > 0 || source.conflict_copy ? 'warn' : 'ok';
    case 'failed':
      return source.failure.failure === 'missing' ? 'idle' : 'error';
    default:
      return 'idle';
  }
}

export function visibleItems(
  items: readonly ConfigItem[],
  provider: Provider | 'all',
  kind: ItemKind | 'all',
): ConfigItem[] {
  return items.filter(
    (item) =>
      (provider === 'all' || item.provider === provider) &&
      (kind === 'all' || item.kind === kind),
  );
}

/** How many items of each kind a provider filter leaves, for the filter's labels. */
export function kindCounts(
  items: readonly ConfigItem[],
  provider: Provider | 'all',
): Record<ItemKind | 'all', number> {
  const counts: Record<ItemKind | 'all', number> = {
    all: 0,
    hook: 0,
    skill: 0,
    mcp_server: 0,
    rule_file: 0,
    model_preference: 0,
  };
  for (const item of items) {
    if (provider !== 'all' && item.provider !== provider) continue;
    counts.all += 1;
    counts[item.kind] += 1;
  }
  return counts;
}

/** Whether redaction found anything in an item: it may have held a secret. */
export function mayHoldSecret(projection: Projection | null): boolean {
  if (projection === null) return false;
  switch (projection.kind) {
    case 'hidden':
      return true;
    case 'text':
      return projection.value.found.length > 0;
    case 'list':
      return projection.value.some(mayHoldSecret);
    case 'map':
      return projection.value.some(
        ([key, value]) => key.includes('[redacted]') || mayHoldSecret(value),
      );
    default:
      return false;
  }
}

export interface DetailLine {
  /** Dotted name of the field: `env.API_KEY`. */
  name: string;
  /** What is shown for it. A hidden value says what it was, not its length. */
  value: string;
  hidden: boolean;
}

/** The leaves of a projection as lines, in order. Bounded by what the desktop sent. */
export function detailLines(projection: Projection | null): DetailLine[] {
  const lines: DetailLine[] = [];
  const walk = (value: Projection, name: string) => {
    switch (value.kind) {
      case 'null':
        lines.push({ name, value: 'empty', hidden: false });
        break;
      case 'bool':
      case 'number':
        lines.push({ name, value: String(value.value), hidden: false });
        break;
      case 'text':
        lines.push({
          name,
          value: value.value.text,
          hidden: value.value.found.length > 0,
        });
        break;
      case 'hidden':
        lines.push({
          name,
          value: `hidden (${SECRET_LABEL[value.value]})`,
          hidden: true,
        });
        break;
      case 'list':
        if (value.value.length === 0)
          lines.push({ name, value: 'none', hidden: false });
        value.value.forEach((entry, index) =>
          walk(entry, name === '' ? `[${index}]` : `${name}[${index}]`),
        );
        break;
      case 'map':
        if (value.value.length === 0)
          lines.push({ name, value: 'none', hidden: false });
        for (const [key, entry] of value.value)
          walk(entry, name === '' ? key : `${name}.${key}`);
        break;
    }
  };
  if (projection !== null) walk(projection, '');
  return lines;
}

// ---- changing (F2a) ----------------------------------------------------------------------

export const OPERATION_LABEL: Record<Operation, string> = {
  park_hook: 'Turn off this hook',
  restore_hook: 'Put this hook back',
  disable_all_hooks: 'Turn off every hook',
  enable_all_hooks: 'Allow hooks again',
  skill_off: 'Turn off this skill',
  skill_on: 'Turn on this skill',
  codex_skill_off: 'Turn off this Codex skill',
  codex_skill_on: 'Turn on this Codex skill',
  codex_mcp_off: 'Turn off this Codex MCP server',
  codex_mcp_on: 'Turn on this Codex MCP server',
};

export const RISK_LABEL: Record<Risk, string> = {
  normal: '',
  weakens_security:
    'This can switch off something that stops an action. You will be asked to confirm in a separate window.',
  executes_code:
    'This puts back something that runs programs. You will be asked to confirm in a separate window.',
};

export const EFFECT_LABEL: Record<Effect, string> = {
  hooks_new_sessions_checked:
    'A new Claude Code session follows the file (checked). A session that is already running is documented to pick it up; that has not been checked.',
  codex_mcp_checked:
    'A new Codex process shows the server as the file says (checked). A Codex session that is already running is not known to follow it.',
  codex_skill_unverified:
    'The file says it. How a running Codex treats a skill’s switch has not been verified.',
  skills_new_sessions_checked:
    'A new Claude Code session refuses to run a skill that is switched off (checked). Skills that come from a plugin are not affected. What a running session does is not known.',
};

export const STORAGE_LABEL: Record<Protection, string> = {
  sealed:
    'The hook is kept on this computer, encrypted, with a key held by your system’s credential store. If that store is reset or its key is lost, it cannot be put back from here.',
  plain_no_secret:
    'The hook is kept on this computer as a private file that is not encrypted: it holds no secret, and no credential store could be reached.',
};

export const OUTCOME_LABEL: Record<
  Outcome,
  { tone: RuntimeTone; text: string }
> = {
  applied: { tone: 'ok', text: 'Done. The file was changed and read back.' },
  refused_by_user: {
    tone: 'idle',
    text: 'Cancelled in the confirmation window. Nothing was changed.',
  },
  changed_outside: {
    tone: 'warn',
    text: 'The file changed after this was planned, so nothing was written. Plan it again.',
  },
  entry_gone: { tone: 'warn', text: 'The entry is no longer there.' },
  entry_changed: {
    tone: 'warn',
    text: 'The entry is not the one that was planned. Nothing was written.',
  },
  conflict_copy_present: {
    tone: 'warn',
    text: 'A conflict copy of this file exists beside it. Resolve that first; nothing was written.',
  },
  expired: {
    tone: 'idle',
    text: 'The plan was too old and was dropped. Plan it again.',
  },
  key_store_unavailable: {
    tone: 'error',
    text: 'Your system’s credential store is not available, so an entry that may hold a secret is not kept. Nothing was written.',
  },
  record_unreadable: {
    tone: 'error',
    text: 'The kept entry cannot be read. Nothing was written.',
  },
  write_failed: {
    tone: 'error',
    text: 'The file could not be written. It is as it was.',
  },
  verify_failed: {
    tone: 'error',
    text: 'The file was written, but what it holds is not what was planned. Check it.',
  },
  not_recorded: {
    tone: 'error',
    text: 'The history could not be written, so nothing was changed.',
  },
  recovered_applied: {
    tone: 'ok',
    text: 'Interrupted, then found complete at start.',
  },
  recovered_not_applied: {
    tone: 'idle',
    text: 'Interrupted before it changed the file.',
  },
  recovered_provider_kept: {
    tone: 'warn',
    text: 'Interrupted while Claude Code was writing the same file. Its newer version was put back and nothing of ours was kept.',
  },
  recovered_unverified: {
    tone: 'warn',
    text: 'Interrupted, and the file holds neither version. Check it.',
  },
};

export const CHANGE_FAILURE_TEXT: Record<ConfigurationFailure, string> = {
  unavailable: 'Changing the configuration is not available here.',
  invalid: 'That request is not one the app accepts.',
  internal: 'Something went wrong. Nothing was changed.',
  not_supported: 'That cannot be changed from here.',
  not_writable:
    'Its file is not one this app may write: it is shared, owned by someone else, or provider-owned.',
  source_missing: 'Its file is not there.',
  source_unreadable: 'Its file could not be read.',
  source_malformed: 'Its file is not valid JSON, so it is left alone.',
  entry_gone: 'The entry is no longer there.',
  entry_changed:
    'The entry is not the one that was planned, or has a value this app does not know how to change.',
  already_done: 'It already is that way.',
  restore_conflict:
    'A different entry is where this one would go. Nothing was written.',
  key_store_unavailable:
    'Your system’s credential store is not available, and this entry may hold a secret, so it is not turned off individually. You can turn off every hook, or edit the file yourself.',
  record_unreadable: 'The kept entry is damaged and was not used.',
  key_lost:
    'The key that protected this entry is gone, so it cannot be put back from here.',
  plan_unknown: 'That plan is gone. Plan it again.',
  not_undoable: 'That cannot be undone from here.',
  import_invalid: 'That file is not a kept entry, or the passphrase is wrong.',
  no_passphrase: 'A passphrase is needed.',
};

export const VERDICT_LABEL: Record<Verdict, string> = {
  available: 'can start',
  external_hooks: 'cannot start: a hook is enabled in Codex’s files',
  mcp_unprovable:
    'cannot start: an MCP server’s name or count cannot be proved',
};

export const NOTICE_LABEL = {
  exported: 'Exported. Keep the file and the passphrase safe.',
  imported: 'Imported. It is listed under Turned off.',
  not_done: 'Cancelled. Nothing was changed.',
  expired: 'The plan was dropped because it was too old.',
} as const;

const SETTINGS_SOURCES: readonly SourceKey[] = [
  'claude_user_settings',
  'claude_project_settings',
  'claude_project_local_settings',
];

/** What the button on an item does, if the item can be changed from here. */
export function changeFor(
  item: ConfigItem,
): { operation: Operation; label: string } | null {
  if (item.not_writable !== 'not_in_this_phase') return null;
  if (item.provider === 'codex') {
    // Codex: a skill or an MCP server of the user's own configuration. Never a hook.
    if (item.source !== 'codex_user_config') return null;
    const on = item.persisted === 'present_enabled';
    const off = item.persisted === 'present_disabled';
    if (item.kind === 'skill' && (on || off))
      return on
        ? { operation: 'codex_skill_off', label: 'Turn off' }
        : { operation: 'codex_skill_on', label: 'Turn on' };
    if (item.kind === 'mcp_server' && (on || off))
      return on
        ? { operation: 'codex_mcp_off', label: 'Turn off' }
        : { operation: 'codex_mcp_on', label: 'Turn on' };
    return null;
  }
  if (
    item.kind === 'hook' &&
    SETTINGS_SOURCES.includes(item.source) &&
    item.persisted === 'present_enabled'
  )
    return { operation: 'park_hook', label: 'Turn off' };
  if (item.kind === 'skill') {
    if (item.persisted === 'present_enabled')
      return { operation: 'skill_off', label: 'Turn off' };
    if (item.persisted === 'present_disabled')
      return { operation: 'skill_on', label: 'Turn on' };
  }
  return null;
}

/** The one switch for every hook, and which way it goes now. */
export function allHooksSwitch(
  items: readonly ConfigItem[],
): { operation: Operation; label: string } | null {
  const hooks = items.filter(
    (item) =>
      item.provider === 'claude_code' &&
      item.kind === 'hook' &&
      item.source === 'claude_user_settings',
  );
  if (hooks.length === 0) return null;
  return hooks.every((hook) => hook.persisted === 'present_disabled')
    ? { operation: 'enable_all_hooks', label: 'Allow hooks again' }
    : { operation: 'disable_all_hooks', label: 'Turn off every hook' };
}
