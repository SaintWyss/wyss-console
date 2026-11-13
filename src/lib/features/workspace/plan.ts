import type { Item, Selection } from '../../contracts/arsenal';
import type { Launch } from '../../desktop/launch-client';
import { isDefault, type HomeSpec } from '../arsenal/environment';

export type Agent = 'claude_code' | 'codex';

/** What the Workspace editor holds for one launch: launch, preset and environment together. */
export interface LaunchDraft {
  /** The launch file's id when editing a saved one; `null` for a new launch. */
  id: string | null;
  name: string;
  role: string;
  agent: Agent;
  profile: string | null;
  directory: string;
  /** The preset the launch names now, if any. */
  selection: Selection | null;
  /** What the preset is to hold. Revisions, composites and order of the rest are kept. */
  items: Item[];
  composites: string[];
  /** Claude Code only: the launch environment of the preset. */
  spec: HomeSpec | null;
}

const FOLD: Record<string, string> = {
  á: 'a',
  à: 'a',
  ä: 'a',
  â: 'a',
  é: 'e',
  è: 'e',
  ë: 'e',
  ê: 'e',
  í: 'i',
  ì: 'i',
  ï: 'i',
  î: 'i',
  ó: 'o',
  ò: 'o',
  ö: 'o',
  ô: 'o',
  ú: 'u',
  ù: 'u',
  ü: 'u',
  û: 'u',
  ñ: 'n',
};
/** The launch file's id derived from its name: the same rule as `slug` in `launch.rs`. */
export function launchId(name: string): string {
  let out = '';
  for (const c of name.toLowerCase()) {
    const mapped = FOLD[c] ?? (/^[a-z0-9]$/.test(c) ? c : '-');
    if (mapped === '-') {
      if (out !== '' && !out.endsWith('-')) out += '-';
    } else out += mapped;
  }
  return out.slice(0, 40).replace(/-+$/, '');
}

const sortedItems = (items: readonly Item[]) =>
  JSON.stringify(
    [...items]
      .map((i) => [i.capability, i.revision])
      .sort((a, b) => String(a[0]).localeCompare(String(b[0]))),
  );
const sameSpec = (a: HomeSpec | null, b: HomeSpec | null) =>
  JSON.stringify(a) === JSON.stringify(b);

/**
 * Whether a preset is used by something other than this launch: another launch of the same
 * agent names it, or it is the preset this folder remembers for the agent.
 */
export function sharedBy(
  selection: Selection,
  agent: Agent,
  launches: readonly Launch[],
  editing: string | null,
  folderSelection: string | null,
): string[] {
  const others = launches
    .filter(
      (l) =>
        l.kind === 'session' &&
        l.id !== editing &&
        l.agent === agent &&
        l.preset === selection.name,
    )
    .map((l) => l.name);
  return folderSelection === selection.id ? [...others, '·folder'] : others;
}

/** A preset name not taken among `taken`: the launch's name, then "name 2", "name 3"… */
export function freeName(name: string, taken: readonly string[]): string {
  const used = new Set(taken.map((t) => t.toLowerCase()));
  if (!used.has(name.toLowerCase())) return name;
  for (let n = 2; ; n++) {
    const candidate = `${name} ${n}`;
    if (!used.has(candidate.toLowerCase())) return candidate;
  }
}

export type PresetStep =
  | { do: 'keep' }
  | { do: 'none' }
  /** The preset is this launch's only: written in place. */
  | { do: 'write'; id: string; name: string }
  /** Shared (decision of 2026-11-13): copied to a preset of this launch's own. */
  | { do: 'copy'; name: string; sharedWith: string[] }
  | { do: 'create'; name: string };

export interface SavePlan {
  preset: PresetStep;
  /** Write the environment to the preset that results (Claude Code only). */
  environment: boolean;
  /** Save the launch file; `replace` only over the file being edited. */
  launch: { id: string; replace: boolean };
  /** The file was saved under another name: the old one is deleted after the new one. */
  removeOld: string | null;
  role: { assign: string; renamedFrom: string | null };
}

export interface PlanContext {
  launches: readonly Launch[];
  /** Every preset of this agent's provider, for free names. */
  selections: readonly Selection[];
  /** The preset the draft's folder remembers for this agent, if any. */
  folderSelection: string | null;
  /** The environment the draft's preset has now. */
  savedSpec: HomeSpec | null;
  /** Everything of this provider in the library: a launch without a preset loads all of it. */
  allItems: readonly Item[];
}

/** What one Save does, in order: preset → environment → launch → role. */
export function savePlan(draft: LaunchDraft, ctx: PlanContext): SavePlan {
  const id = launchId(draft.name);
  const sel = draft.selection;
  const contentChanged = sel
    ? sortedItems(sel.items) !== sortedItems(draft.items) ||
      JSON.stringify([...sel.composites].sort()) !==
        JSON.stringify([...draft.composites].sort())
    : sortedItems(draft.items) !== sortedItems(ctx.allItems);
  const specChanged =
    draft.agent === 'claude_code' &&
    draft.spec !== null &&
    !sameSpec(draft.spec, ctx.savedSpec) &&
    !(ctx.savedSpec === null && isDefault(draft.spec));
  const names = ctx.selections.map((s) => s.name);
  let preset: PresetStep;
  if (!sel) {
    preset =
      contentChanged || specChanged
        ? { do: 'create', name: freeName(draft.name.trim(), names) }
        : { do: 'none' };
  } else if (!contentChanged && !specChanged) {
    preset = { do: 'keep' };
  } else {
    const shared = sharedBy(
      sel,
      draft.agent,
      ctx.launches,
      draft.id,
      ctx.folderSelection,
    );
    preset =
      shared.length > 0
        ? {
            do: 'copy',
            name: freeName(draft.name.trim(), names),
            sharedWith: shared,
          }
        : { do: 'write', id: sel.id, name: sel.name };
  }
  const environment =
    draft.agent === 'claude_code' &&
    draft.spec !== null &&
    (specChanged || preset.do === 'copy' || preset.do === 'create') &&
    !isDefault(draft.spec);
  const renamed = draft.id !== null && draft.id !== id ? draft.id : null;
  return {
    preset,
    environment,
    launch: { id, replace: draft.id === id },
    removeOld: renamed,
    role: { assign: draft.role, renamedFrom: renamed },
  };
}

/** A launch the window can save: a session launch with no `before` lines. Others are opened only. */
export function editable(launch: Launch): boolean {
  return launch.kind === 'session' && launch.before.length === 0;
}
