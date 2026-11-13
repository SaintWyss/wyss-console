/** What a Claude Code preset changes in the terminal's launch environment (H1/H2). */
export interface HomeSpec {
  instructions: boolean;
  rules_off: string[];
  hooks_off: string[];
  /** `null` keeps every MCP server as the owner's own configuration has them. */
  mcp: string[] | null;
  plugins_off: string[];
  memory: boolean;
  agents: boolean;
  account_skills_off: string[];
}

export interface HookOption {
  key: string;
  event: string;
  matcher: string;
  command: string;
}

export interface EnvironmentOptions {
  hooks: HookOption[];
  mcp: string[];
  plugins: string[];
  rules: string[];
  account_skills: string[];
  instructions: boolean;
  token_stored: boolean;
  /** Bytes for the context estimate: `instructions`, `rule:<n>`, `account:<n>`, `plugin:<id>`. */
  sizes: Record<string, number>;
}

export const DEFAULT_SPEC: HomeSpec = {
  instructions: true,
  rules_off: [],
  hooks_off: [],
  mcp: null,
  plugins_off: [],
  memory: true,
  agents: true,
  account_skills_off: [],
};

function record(value: unknown, what: string): Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value))
    throw new Error(what);
  return value as Record<string, unknown>;
}

function strings(value: unknown, what: string): string[] {
  if (!Array.isArray(value) || value.some((v) => typeof v !== 'string'))
    throw new Error(what);
  return value as string[];
}

function flag(value: unknown, what: string): boolean {
  if (typeof value !== 'boolean') throw new Error(what);
  return value;
}

export function parseSpec(value: unknown): HomeSpec {
  const v = record(value, 'environment');
  return {
    instructions: flag(v.instructions, 'instructions'),
    rules_off: strings(v.rules_off, 'rules_off'),
    hooks_off: strings(v.hooks_off, 'hooks_off'),
    mcp: v.mcp === null ? null : strings(v.mcp, 'mcp'),
    plugins_off: strings(v.plugins_off, 'plugins_off'),
    memory: flag(v.memory, 'memory'),
    agents: flag(v.agents, 'agents'),
    account_skills_off: strings(v.account_skills_off, 'account_skills_off'),
  };
}

export function parseOptions(value: unknown): EnvironmentOptions {
  const v = record(value, 'options');
  if (!Array.isArray(v.hooks)) throw new Error('hooks');
  return {
    hooks: v.hooks.map((item) => {
      const h = record(item, 'hook');
      for (const key of ['key', 'event', 'matcher', 'command'])
        if (typeof h[key] !== 'string') throw new Error(key);
      return h as unknown as HookOption;
    }),
    mcp: strings(v.mcp, 'mcp'),
    plugins: strings(v.plugins, 'plugins'),
    rules: strings(v.rules, 'rules'),
    account_skills: strings(v.account_skills, 'account_skills'),
    instructions: flag(v.instructions, 'instructions'),
    token_stored: flag(v.token_stored, 'token_stored'),
    sizes: sizesOf(v.sizes),
  };
}

/** A desktop older than the estimate sends no sizes: every size reads as 0. */
function sizesOf(value: unknown): Record<string, number> {
  if (typeof value !== 'object' || value === null || Array.isArray(value))
    return {};
  return Object.fromEntries(
    Object.entries(value).filter(
      ([, n]) => typeof n === 'number' && Number.isSafeInteger(n) && n >= 0,
    ),
  ) as Record<string, number>;
}

/** Whether the spec changes anything, i.e. whether the terminal gets its own launch folder. */
export function isDefault(spec: HomeSpec): boolean {
  return (
    spec.instructions &&
    spec.memory &&
    spec.agents &&
    spec.account_skills_off.length === 0 &&
    spec.mcp === null &&
    spec.rules_off.length === 0 &&
    spec.hooks_off.length === 0 &&
    spec.plugins_off.length === 0
  );
}

/** Adds or removes `value` from a list, keeping it sorted so equal choices compare equal. */
export function toggle(list: string[], value: string, on: boolean): string[] {
  const set = new Set(list);
  if (on) set.add(value);
  else set.delete(value);
  return [...set].sort();
}

/** A short name for a hook: the script it runs, without its interpreter and folders. */
export function hookLabel(hook: HookOption): string {
  const words = hook.command.split(/\s+/).filter((w) => w !== '');
  const script =
    words.find((w) => /\.(py|sh|js|ts|mjs)"?$/.test(w)) ?? words[0] ?? '';
  const name = script.replace(/^"|"$/g, '').split('/').pop() ?? script;
  return hook.matcher === ''
    ? `${hook.event} · ${name}`
    : `${hook.event} (${hook.matcher}) · ${name}`;
}
