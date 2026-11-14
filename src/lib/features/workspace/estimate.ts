import type { HomeSpec } from '../arsenal/environment';

/**
 * The estimated context a Claude Code session starts with, by group (decision of 2026-11-13: an
 * estimate by files, not a measurement). A fixed rule, so the same choice always gives the same
 * number: bytes ÷ 4 of what is loaded, over the base measured with nothing loaded.
 *
 * - base: 14,002 tokens, measured with Claude Code 2.1.323 with no instructions, skills, agents,
 *   MCP, hooks, plugins or account skills (arsenal-UI-SPEC, H2).
 * - instructions: the instruction files with their `@` imports, and each rule left on.
 * - skills and agents: the name and description each is listed with (not the whole file).
 * - account skills: the name and description of each one left on.
 * - plugins: what each enabled plugin left on lists (its skills, agents and commands).
 * - hooks: nothing in the context.
 * - MCP: cannot be known without starting the session: not measured (`null`).
 */
export const BASE_TOKENS = 14_002;
/** Words around each listed skill or agent in the list Claude Code builds (name, separators). */
const LISTED_OVERHEAD = 24;

export interface Listed {
  name: string;
  description: string;
}

export interface EstimateInput {
  spec: HomeSpec;
  /** From `arsenal_environment_options`: `instructions`, `rule:<name>`, `account:<name>`. */
  sizes: Readonly<Record<string, number>>;
  rules: readonly string[];
  accountSkills: readonly string[];
  plugins: readonly string[];
  skills: readonly Listed[];
  agents: readonly Listed[];
}

export interface Estimate {
  base: number;
  instructions: number;
  skills: number;
  agents: number;
  account: number;
  plugins: number;
  hooks: number;
  /** Not measured: depends on what each server declares when it starts. */
  mcp: null;
  /** Everything but MCP. */
  total: number;
}

const tokens = (bytes: number) => Math.ceil(Math.max(0, bytes) / 4);
const utf8 = new TextEncoder();
/** Bytes, not UTF-16 units: an accented or wide character weighs what the file holds. */
const bytesOf = (text: string) => utf8.encode(text).length;
const listed = (items: readonly Listed[]) =>
  items.reduce(
    (sum, item) =>
      sum +
      tokens(bytesOf(item.name) + bytesOf(item.description) + LISTED_OVERHEAD),
    0,
  );

export function estimate(input: EstimateInput): Estimate {
  const { spec, sizes } = input;
  const instructions = spec.instructions
    ? tokens(sizes.instructions ?? 0) +
      input.rules
        .filter((rule) => !spec.rules_off.includes(rule))
        .reduce((sum, rule) => sum + tokens(sizes[`rule:${rule}`] ?? 0), 0)
    : 0;
  const account = input.accountSkills
    .filter((name) => !spec.account_skills_off.includes(name))
    .reduce((sum, name) => sum + tokens(sizes[`account:${name}`] ?? 0), 0);
  const plugins = input.plugins
    .filter((id) => !spec.plugins_off.includes(id))
    .reduce((sum, id) => sum + tokens(sizes[`plugin:${id}`] ?? 0), 0);
  const skills = listed(input.skills);
  const agents = spec.agents ? listed(input.agents) : 0;
  return {
    base: BASE_TOKENS,
    instructions,
    skills,
    agents,
    account,
    plugins,
    hooks: 0,
    mcp: null,
    total: BASE_TOKENS + instructions + skills + agents + account + plugins,
  };
}

/** `12345` → `12k`, for the header and the launch menu. */
export function kTokens(n: number): string {
  return n > 0 && n < 1000 ? '<1k' : `${Math.round(n / 1000)}k`;
}
