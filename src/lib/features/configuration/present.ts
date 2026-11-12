import type {
  ConfigItem,
  ConfigurationFailure,
  Effect,
  Effective,
  ItemKind,
  NewDocumentKind,
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
import { msg, type Phrase } from '../../i18n/msg';

/** Presentation only: nothing here is read back as state. */

// Product names (Claude Code, Codex, OpenCode) are not translated, so they carry no msg().
export const PROVIDER_LABEL: Record<Provider, string> = {
  claude_code: 'Claude Code',
  codex: 'Codex',
  opencode: 'OpenCode',
};

export const KIND_LABEL: Record<ItemKind, string> = {
  hook: msg('Hooks'),
  skill: msg('Skills'),
  mcp_server: msg('MCP servers'),
  rule_file: msg('Rule files'),
  model_preference: msg('Models'),
  plugin: msg('Plugins'),
};
export const KIND_ORDER: readonly ItemKind[] = [
  'hook',
  'skill',
  'mcp_server',
  'rule_file',
  'model_preference',
  'plugin',
];
const KIND_SINGULAR: Record<ItemKind, string> = {
  hook: msg('Hook'),
  skill: msg('Skill'),
  mcp_server: msg('MCP server'),
  rule_file: msg('Rule file'),
  model_preference: msg('Model setting'),
  plugin: msg('Plugin'),
};
export const kindName = (kind: ItemKind): string => KIND_SINGULAR[kind];

/** What the file says. Whether the provider honours it is a separate line. */
export const PERSISTED_LABEL: Record<
  Persisted,
  { tone: RuntimeTone; label: string }
> = {
  present_enabled: { tone: 'ok', label: msg('On in the file') },
  present_disabled: { tone: 'warn', label: msg('Off in the file') },
  absent: { tone: 'idle', label: msg('Not in the file') },
  parked: { tone: 'idle', label: msg('Parked') },
  unreadable: { tone: 'error', label: msg('Could not be read') },
};

export const EFFECTIVE_LABEL: Record<Effective, string> = {
  verified_enabled: msg('Confirmed active'),
  verified_disabled: msg('Confirmed inactive'),
  documented: msg('As the provider documents it'),
  not_verifiable: msg('Cannot be confirmed without running the provider'),
};

export const SCOPE_LABEL = {
  system: msg('System'),
  global: msg('Your account'),
  project: msg('Project'),
} as const;

export const SOURCE_LABEL: Record<SourceKey, string> = {
  claude_managed_settings: msg('Managed settings'),
  claude_user_settings: msg('User settings'),
  claude_user_state: msg('Account file (MCP servers only)'),
  claude_user_skills: msg('User skills folder'),
  claude_user_rules: msg('User rules'),
  claude_project_settings: msg('Project settings'),
  claude_project_local_settings: msg('Project local settings'),
  claude_project_mcp: msg('Project MCP file'),
  claude_project_skills: msg('Project skills folder'),
  claude_project_rules: msg('Project rules'),
  codex_system_requirements: msg('Administrator requirements'),
  codex_system_managed: msg('Managed configuration'),
  codex_user_config: msg('User configuration'),
  codex_user_hooks: msg('User hooks file'),
  codex_user_rules: msg('User instructions'),
  codex_user_skills: msg('User skills folder (~/.agents/skills)'),
  codex_project_config: msg('Project configuration'),
  codex_project_rules: msg('Project instructions'),
  opencode_user_config: msg('User configuration'),
  opencode_user_rules: msg('User instructions'),
  opencode_project_config: msg('Project configuration'),
  opencode_project_rules: msg('Project instructions'),
};

/** Why nothing can be changed here. Every control that would change something is absent. */
export const NOT_WRITABLE_LABEL: Record<NotWritable, string> = {
  system: msg('Set by the system; this app never changes it'),
  provider_owned: msg(
    'Owned by the provider; changed only through its own command',
  ),
  generated: msg('Produced by another tool'),
  shared_path: msg('Its folder can be written by a group: read, never changed'),
  not_in_this_phase: msg('Read-only for now: changing it is not available yet'),
};

export const SECRET_LABEL: Record<SecretClass, string> = {
  environment_value: msg('environment value'),
  header: msg('header'),
  url_credentials: msg('address credentials'),
  secret_field: msg('secret field'),
  token_shape: msg('token'),
};

/** One sentence for what became of a source. Never a path, never an operating-system text. */
export function failureText(failure: SourceFailure): Phrase {
  switch (failure.failure) {
    case 'missing':
      return { text: msg('It is not there.') };
    case 'unreadable':
      return { text: msg('It could not be read.') };
    case 'unstable':
      return {
        text: msg(
          'It kept changing while it was being read. It will be read again.',
        ),
      };
    case 'credential_file':
      return { text: msg('It holds credentials, so it is never opened.') };
    case 'untrusted_path':
      return failure.reason === 'outside_roots'
        ? {
            text: msg(
              'Where it leads is outside the places this app may read.',
            ),
          }
        : {
            text: msg(
              'Its place is not safe to read from: someone else owns or can change it.',
            ),
          };
    case 'unsupported':
      return failure.reason === 'link_limit'
        ? { text: msg('It is behind too many links.') }
        : { text: msg('It is not a regular file.') };
    case 'too_large':
      return { text: msg('It is larger than the bounds this app reads.') };
    case 'malformed':
      return {
        text: msg(
          'It is not valid: reading stopped at line {line}, column {column}.',
        ),
        values: { line: failure.line, column: failure.column },
      };
  }
}

/** The line shown under a source: what happened, in words. */
export function outcomeText(source: SourceReport): Phrase {
  switch (source.outcome) {
    case 'read':
      if (source.conflict_copy)
        return {
          text: msg(
            'Read. A conflict copy of this file sits beside it, so it cannot be changed from here until that copy is dealt with.',
          ),
        };
      return source.unrecognised > 0
        ? {
            text: msg('Read. {entries}'),
            values: {
              entries: {
                text:
                  source.unrecognised === 1
                    ? msg(
                        '{count} entry was not recognised and left untouched.',
                      )
                    : msg(
                        '{count} entries were not recognised and left untouched.',
                      ),
                values: { count: source.unrecognised },
              },
            },
          }
        : { text: msg('Read.') };
    case 'absent':
      return { text: msg('Not present.') };
    case 'project_not_trusted':
      return {
        text: msg(
          'Not read: Codex loads a project’s own configuration only for a project you trust.',
        ),
      };
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

/** Items whose name contains `query`, ignoring case and surrounding spaces; no query keeps all. */
export function matchingName(
  items: readonly ConfigItem[],
  query: string,
): ConfigItem[] {
  const needle = query.trim().toLowerCase();
  return needle === ''
    ? [...items]
    : items.filter((item) => item.name.toLowerCase().includes(needle));
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
    plugin: 0,
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
  /**
   * What is shown for it. Data from the desktop is a string and is shown as it arrived; a
   * sentence this window wrote (`empty`, `none`, a hidden value) is a Phrase, drawn with tPhrase().
   */
  value: string | Phrase;
  hidden: boolean;
}

/** The leaves of a projection as lines, in order. Bounded by what the desktop sent. */
export function detailLines(projection: Projection | null): DetailLine[] {
  const lines: DetailLine[] = [];
  const walk = (value: Projection, name: string) => {
    switch (value.kind) {
      case 'null':
        lines.push({ name, value: { text: msg('empty') }, hidden: false });
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
          value: {
            text: msg('hidden ({kind})'),
            values: { kind: { text: SECRET_LABEL[value.value] } },
          },
          hidden: true,
        });
        break;
      case 'list':
        if (value.value.length === 0)
          lines.push({ name, value: { text: msg('none') }, hidden: false });
        value.value.forEach((entry, index) =>
          walk(entry, name === '' ? `[${index}]` : `${name}[${index}]`),
        );
        break;
      case 'map':
        if (value.value.length === 0)
          lines.push({ name, value: { text: msg('none') }, hidden: false });
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
  edit_document: msg('Change this instruction file'),
  create_document: msg('Create this instruction file'),
  delete_document: msg(
    'Delete this instruction file (kept encrypted, so it can be put back)',
  ),
  park_hook: msg('Turn off this hook'),
  restore_hook: msg('Put this hook back'),
  disable_all_hooks: msg('Turn off every hook'),
  enable_all_hooks: msg('Allow hooks again'),
  skill_off: msg('Turn off this skill'),
  skill_on: msg('Turn on this skill'),
  codex_skill_off: msg('Turn off this Codex skill'),
  codex_skill_on: msg('Turn on this Codex skill'),
  codex_mcp_off: msg('Turn off this Codex MCP server'),
  codex_mcp_on: msg('Turn on this Codex MCP server'),
  opencode_mcp_off: msg('Turn off this OpenCode MCP server'),
  opencode_mcp_on: msg('Turn on this OpenCode MCP server'),
  park_plugin: msg('Turn off this OpenCode plugin'),
  add_hook: msg('Add this hook'),
  edit_hook: msg('Change this hook’s command'),
  add_mcp: msg('Add this OpenCode MCP server'),
  edit_mcp: msg('Change this OpenCode MCP server'),
  park_mcp: msg(
    'Delete this OpenCode MCP server (kept, so it can be put back)',
  ),
  revert_authored: msg('Take back what you wrote'),
  codex_add_mcp: msg('Add this Codex MCP server'),
  codex_edit_mcp: msg('Change this Codex MCP server'),
  codex_park_mcp: msg(
    'Delete this Codex MCP server (kept, so it can be put back)',
  ),
};

export const RISK_LABEL: Record<Risk, string> = {
  normal: '',
  weakens_security: msg(
    'This can switch off something that stops an action. You will be asked to confirm in a separate window.',
  ),
  executes_code: msg(
    'This writes, or puts back, something that runs programs. You will be asked to confirm in a separate window.',
  ),
};

export const EFFECT_LABEL: Record<Effect, string> = {
  document_written: msg(
    'Written and read back. Reload and use by a running provider are not observed.',
  ),
  hooks_new_sessions_checked: msg(
    'A new Claude Code session follows the file (checked). A session that is already running is documented to pick it up; that has not been checked.',
  ),
  codex_mcp_checked: msg(
    'A new Codex process shows the server as the file says (checked). A Codex session that is already running is not known to follow it.',
  ),
  opencode_listed: msg(
    'A new OpenCode process reads the file. That a running OpenCode follows it has not been checked.',
  ),
  codex_skill_listed: msg(
    'A new Codex process lists the skill as the file says (checked, through its own skills/list). A Codex session that is already running is not known to follow it.',
  ),
  skills_new_sessions_checked: msg(
    'A new Claude Code session refuses to run a skill that is switched off (checked). Skills that come from a plugin are not affected. What a running session does is not known.',
  ),
};

export const STORAGE_LABEL: Record<Protection, string> = {
  sealed: msg(
    'What this keeps is stored on this computer, encrypted, with a key held by your system’s credential store. If that store is reset or its key is lost, it cannot be put back from here.',
  ),
  plain_no_secret: msg(
    'What this keeps is stored on this computer as a private file that is not encrypted: it holds no secret, and no credential store could be reached.',
  ),
};

export const OUTCOME_LABEL: Record<
  Outcome,
  { tone: RuntimeTone; text: string }
> = {
  applied: {
    tone: 'ok',
    text: msg('Done. The file was changed and read back.'),
  },
  refused_by_user: {
    tone: 'idle',
    text: msg('Cancelled in the confirmation window. Nothing was changed.'),
  },
  changed_outside: {
    tone: 'warn',
    text: msg(
      'The file changed after this was planned, so nothing was written. Plan it again.',
    ),
  },
  entry_gone: { tone: 'warn', text: msg('The entry is no longer there.') },
  entry_changed: {
    tone: 'warn',
    text: msg(
      'The entry is not the one that was planned. Nothing was written.',
    ),
  },
  conflict_copy_present: {
    tone: 'warn',
    text: msg(
      'A conflict copy of this file exists beside it. Resolve that first; nothing was written.',
    ),
  },
  expired: {
    tone: 'idle',
    text: msg('The plan was too old and was dropped. Plan it again.'),
  },
  key_store_unavailable: {
    tone: 'error',
    text: msg(
      'Your system’s credential store is not available, so an entry that may hold a secret is not kept. Nothing was written.',
    ),
  },
  record_unreadable: {
    tone: 'error',
    text: msg('The kept entry cannot be read. Nothing was written.'),
  },
  write_failed: {
    tone: 'error',
    text: msg('The file could not be written. It is as it was.'),
  },
  verify_failed: {
    tone: 'error',
    text: msg(
      'The file was written, but what it holds is not what was planned. Check it.',
    ),
  },
  not_recorded: {
    tone: 'error',
    text: msg('The history could not be written, so nothing was changed.'),
  },
  recovered_applied: {
    tone: 'ok',
    text: msg('Interrupted, then found complete at start.'),
  },
  recovered_not_applied: {
    tone: 'idle',
    text: msg('Interrupted before it changed the file.'),
  },
  recovered_provider_kept: {
    tone: 'warn',
    text: msg(
      'Interrupted while Claude Code was writing the same file. Its newer version was put back and nothing of ours was kept.',
    ),
  },
  recovered_unverified: {
    tone: 'warn',
    text: msg('Interrupted, and the file holds neither version. Check it.'),
  },
};

export const CHANGE_FAILURE_TEXT: Record<ConfigurationFailure, string> = {
  unavailable: msg('Changing the configuration is not available here.'),
  invalid: msg('That request is not one the app accepts.'),
  internal: msg('Something went wrong. Nothing was changed.'),
  not_supported: msg('That cannot be changed from here.'),
  not_writable: msg(
    'Its file is not one this app may write: it is shared, owned by someone else, or provider-owned.',
  ),
  source_missing: msg('Its file is not there.'),
  source_unreadable: msg('Its file could not be read.'),
  source_malformed: msg('Its file is not valid JSON, so it is left alone.'),
  entry_gone: msg('The entry is no longer there.'),
  entry_changed: msg(
    'The entry is not the one that was planned, or has a value this app does not know how to change.',
  ),
  already_done: msg('It already is that way.'),
  already_exists: msg(
    'Something already has that name. It was not touched, and nothing was written.',
  ),
  restore_conflict: msg(
    'A different entry is where this one would go. Nothing was written.',
  ),
  key_store_unavailable: msg(
    'Your system’s credential store is not available, and this entry may hold a secret, so it is not turned off individually. You can turn off every hook, or edit the file yourself.',
  ),
  record_unreadable: msg('The kept entry is damaged and was not used.'),
  key_lost: msg(
    'The key that protected this entry is gone, so it cannot be put back from here.',
  ),
  plan_unknown: msg('That plan is gone. Plan it again.'),
  not_undoable: msg('That cannot be undone from here.'),
  import_invalid: msg(
    'That file is not a kept entry, or the passphrase is wrong.',
  ),
  no_passphrase: msg('A passphrase is needed.'),
  invalid_entry: msg(
    'That is not something that can be written: an event this app does not know, an empty or multi-line command, an unusable name or address.',
  ),
  cancelled: msg('Cancelled. Nothing was changed.'),
  project_refused: msg(
    'That folder cannot be the project: it must be your own, inside your home, with no link on the way and nobody else able to write it or the folders above it.',
  ),
};

export const VERDICT_LABEL: Record<Verdict, string> = {
  available: msg('can start'),
  external_hooks: msg('cannot start: a hook is enabled in Codex’s files'),
  mcp_unprovable: msg(
    'cannot start: an MCP server’s name or count cannot be proved',
  ),
};

export const NOTICE_LABEL = {
  exported: msg('Exported. Keep the file and the passphrase safe.'),
  imported: msg('Imported. It is listed under Turned off.'),
  not_done: msg('Cancelled. Nothing was changed.'),
  expired: msg('The plan was dropped because it was too old.'),
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
        ? { operation: 'codex_skill_off', label: msg('Turn off') }
        : { operation: 'codex_skill_on', label: msg('Turn on') };
    if (item.kind === 'mcp_server' && (on || off))
      return on
        ? { operation: 'codex_mcp_off', label: msg('Turn off') }
        : { operation: 'codex_mcp_on', label: msg('Turn on') };
    return null;
  }
  if (item.provider === 'opencode') {
    // OpenCode: an MCP server or a plugin of the user's own file. Never what is under `provider`.
    if (item.source !== 'opencode_user_config') return null;
    const on = item.persisted === 'present_enabled';
    const off = item.persisted === 'present_disabled';
    if (item.kind === 'mcp_server' && (on || off))
      return on
        ? { operation: 'opencode_mcp_off', label: msg('Turn off') }
        : { operation: 'opencode_mcp_on', label: msg('Turn on') };
    if (item.kind === 'plugin' && on)
      return { operation: 'park_plugin', label: msg('Turn off') };
    return null;
  }
  if (
    item.kind === 'hook' &&
    SETTINGS_SOURCES.includes(item.source) &&
    item.persisted === 'present_enabled'
  )
    return { operation: 'park_hook', label: msg('Turn off') };
  if (item.kind === 'skill') {
    if (item.persisted === 'present_enabled')
      return { operation: 'skill_off', label: msg('Turn off') };
    if (item.persisted === 'present_disabled')
      return { operation: 'skill_on', label: msg('Turn on') };
  }
  return null;
}

/** The events a new hook may name: the same closed list the desktop checks. */
export const HOOK_EVENTS = [
  'PreToolUse',
  'PostToolUse',
  'PostToolUseFailure',
  'UserPromptSubmit',
  'Notification',
  'Stop',
  'SubagentStop',
  'PreCompact',
  'SessionStart',
  'SessionEnd',
  'PermissionRequest',
  'SubagentStart',
] as const;

/** What can be created where nothing exists, as the page offers it. */
export const NEW_DOCUMENT_LABEL: Record<NewDocumentKind, string> = {
  claude_skill: msg('Claude Code skill'),
  claude_rule: msg('Claude Code rule (rules/…)'),
  claude_memory: msg('Claude Code CLAUDE.md'),
  codex_skill: msg('Codex skill (~/.agents/skills)'),
  codex_agents: msg('Codex AGENTS.md'),
  codex_override: msg('Codex AGENTS.override.md'),
  opencode_agents: msg('OpenCode AGENTS.md'),
};

/** The kinds a project can hold: Codex skills and the override file live in the account only. */
export const PROJECT_DOCUMENT_KINDS: readonly NewDocumentKind[] = [
  'claude_skill',
  'claude_rule',
  'claude_memory',
  'codex_agents',
  'opencode_agents',
];

/** A name OpenCode may key an MCP server by, as the desktop checks it. */
export const mcpNameOk = (name: string): boolean =>
  /^[A-Za-z0-9_-]{1,64}$/.test(name);

/** What the user can write for an item: change its command, or delete it. */
export function authoringFor(item: ConfigItem): {
  edit: 'hook' | 'mcp' | 'document';
  remove: 'park_mcp' | 'codex_park_mcp' | 'delete_document' | null;
} | null {
  if (item.not_writable !== 'not_in_this_phase') return null;
  if (
    (item.kind === 'rule_file' &&
      [
        'claude_user_rules',
        'codex_user_rules',
        'opencode_user_rules',
        'claude_project_rules',
        'codex_project_rules',
        'opencode_project_rules',
      ].includes(item.source)) ||
    (item.kind === 'skill' &&
      [
        'claude_user_skills',
        'codex_user_skills',
        'claude_project_skills',
      ].includes(item.source))
  )
    return { edit: 'document', remove: 'delete_document' };
  if (
    item.provider === 'claude_code' &&
    item.kind === 'hook' &&
    item.source === 'claude_user_settings'
  )
    return { edit: 'hook', remove: null };
  if (
    item.provider === 'opencode' &&
    item.kind === 'mcp_server' &&
    item.source === 'opencode_user_config'
  )
    return { edit: 'mcp', remove: 'park_mcp' };
  if (
    item.provider === 'codex' &&
    item.kind === 'mcp_server' &&
    item.source === 'codex_user_config'
  )
    return { edit: 'mcp', remove: 'codex_park_mcp' };
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
    ? { operation: 'enable_all_hooks', label: msg('Allow hooks again') }
    : { operation: 'disable_all_hooks', label: msg('Turn off every hook') };
}
