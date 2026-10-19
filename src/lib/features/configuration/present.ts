import type {
  ConfigItem,
  Effective,
  ItemKind,
  NotWritable,
  Persisted,
  Projection,
  Provider,
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
      return source.unrecognised > 0 ? 'warn' : 'ok';
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
