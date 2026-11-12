import type {
  ArsenalProvider,
  Composite,
  DiscoverReply,
  Item,
  LibraryItem,
  Preview,
} from '../../contracts/arsenal';
import { filterLibrary } from './catalogue';
import { GROUPS, familyFor, groupFor, type Family, type Group } from './groups';

/** The cells a group's meter draws: one per chosen capability, never more than this many. */
export const METER_CELLS = 5;

/** The longest text a capability row says it is for. */
export const SUMMARY_LIMIT = 110;

/**
 * What a capability row says it is for: the first sentence of its description, at most
 * `SUMMARY_LIMIT` characters. Empty when the file has no description.
 */
export function summaryOf(description: string): string {
  const firstLine = description.trim().split('\n')[0] ?? '';
  const sentence = firstLine.split(/(?<=[.!?])\s/)[0] ?? '';
  return sentence.slice(0, SUMMARY_LIMIT);
}

/** The families the chooser can filter by, in the order the contract shows them. */
export const FAMILIES: readonly Exclude<Family, null>[] = [
  'GSD',
  'gstack',
  'SEO',
  'HyperFrames',
];

/** One group of the provider's library: how many of it are chosen, and whether the group box is mixed. */
export interface GroupRow {
  group: Group;
  total: number;
  chosen: number;
  state: 'none' | 'partial' | 'all';
  items: LibraryItem[];
}

/**
 * The provider's capabilities by group, in the order of the group table. A group the provider has
 * no capability in is left out. A correction the owner made wins over the rules.
 */
export function groupRows(
  library: readonly LibraryItem[],
  chosen: ReadonlySet<string>,
  corrections: Readonly<Record<string, string>>,
): GroupRow[] {
  const byGroup = new Map<Group, LibraryItem[]>();
  for (const item of library) {
    const group = groupFor(item, corrections[item.id]);
    byGroup.set(group, [...(byGroup.get(group) ?? []), item]);
  }
  return GROUPS.flatMap((group) => {
    const items = byGroup.get(group);
    if (!items) return [];
    items.sort((a, b) => a.name.localeCompare(b.name));
    const count = items.filter((item) => chosen.has(item.id)).length;
    const state =
      count === 0 ? 'none' : count === items.length ? 'all' : 'partial';
    return [{ group, total: items.length, chosen: count, state, items }];
  });
}

/** How many cells a group's meter fills: one per chosen capability, capped at the meter's size. */
export function meterFill(chosen: number): number {
  return Math.min(Math.max(chosen, 0), METER_CELLS);
}

/** The families present in a library, in contract order. */
export function familiesOf(
  library: readonly LibraryItem[],
): Exclude<Family, null>[] {
  const present = new Set(
    library.map((item) => familyFor(item.name)).filter((f) => f !== null),
  );
  return FAMILIES.filter((family) => present.has(family));
}

export interface ChooserFilter {
  query: string;
  kind: string | 'all';
  family: Family | 'all';
}

/** Filters the provider's capabilities by text, kind and family. */
export function filterChooser(
  library: readonly LibraryItem[],
  filter: ChooserFilter,
): LibraryItem[] {
  return filterLibrary(library, {
    query: filter.query,
    provider: 'all',
    source: 'all',
    compatibility: 'all',
  }).filter(
    (item) =>
      (filter.kind === 'all' || item.kind === filter.kind) &&
      (filter.family === 'all' || familyFor(item.name) === filter.family),
  );
}

export interface Totals {
  on: number;
  off: number;
}

/**
 * The totals under a draft. On is what the plan resolves for the draft; off is the rest of the
 * provider's library. Composites that a preset keeps are not counted in either.
 */
export function totalsOf(preview: Preview, libraryOfProvider: number): Totals {
  const on = preview.items.filter((item) => item.state === 'resolved').length;
  return { on, off: Math.max(0, libraryOfProvider - on) };
}

/** The GSD Core collections a preset may keep, for this provider or shared by both. */
export function keptComposites(
  composites: readonly Composite[],
  provider: ArsenalProvider,
): Composite[] {
  return composites.filter(
    (composite) =>
      composite.kind === 'gsd_core' &&
      (composite.provider === provider || composite.provider === 'shared'),
  );
}

/** The types the chooser offers, in the order the contract shows them. */
export type ChooserKind =
  | 'all'
  | 'rule'
  | 'skill'
  | 'agent'
  | 'hook'
  | 'mcp_server'
  | 'plugin'
  | 'command';

/** Something the chooser shows but a preset cannot hold yet: hooks, MCP servers, plugins, commands. */
export interface ViewOnlyEntry {
  key: string;
  kind: 'agent' | 'hook' | 'mcp_server' | 'plugin' | 'command';
  name: string;
  description: string;
  /** Where it comes from, when it belongs to a package (GSD Core). */
  partOf: string | null;
}

/**
 * The provider's entries the chooser lists without a checkbox. Claude Code's GSD agents are links
 * into GSD Core, so they are listed from that package, marked as part of it; Codex's are ordinary
 * files already in the library.
 */
export function viewOnlyEntries(
  discover: DiscoverReply | null,
  provider: ArsenalProvider,
  kind: ChooserKind,
  query: string,
): ViewOnlyEntry[] {
  if (discover === null) return [];
  const entries: ViewOnlyEntry[] = [];
  for (const candidate of discover.candidates) {
    if (candidate.provider !== provider) continue;
    if (
      candidate.kind === 'hook' ||
      candidate.kind === 'mcp_server' ||
      candidate.kind === 'command'
    )
      entries.push({
        key: candidate.id,
        kind: candidate.kind,
        name: candidate.name,
        description: candidate.description,
        partOf: null,
      });
  }
  for (const composite of discover.composites) {
    const mine =
      composite.provider === provider || composite.provider === 'shared';
    if (!mine) continue;
    entries.push({
      key: composite.key,
      kind: 'plugin',
      name: composite.name,
      description: composite.description,
      partOf: null,
    });
    if (composite.kind === 'gsd_core' && provider === 'claude_code')
      for (const agent of composite.agents)
        entries.push({
          key: `${composite.key}/agent/${agent}`,
          kind: 'agent',
          name: agent,
          description: '',
          partOf: 'GSD Core',
        });
  }
  const needle = query.trim().toLowerCase();
  return entries.filter(
    (entry) =>
      (kind === 'all' || entry.kind === kind) &&
      (needle === '' ||
        `${entry.name} ${entry.description}`.toLowerCase().includes(needle)),
  );
}

/** What a preset is made of, as it is saved. */
export interface Draft {
  id: string | null;
  name: string;
  items: Item[];
  composites: string[];
}

/** The same content in one order, so that the order of a list never reads as a change. */
function canonical(draft: Draft): string {
  return JSON.stringify({
    name: draft.name,
    items: [...draft.items]
      .map((item) => [item.capability, item.revision])
      .sort((a, b) => String(a[0]).localeCompare(String(b[0]))),
    composites: [...draft.composites].sort(),
  });
}

/** True when the draft differs from the saved preset, or from an empty new one. */
export function isDirty(draft: Draft, saved: Draft | null): boolean {
  const base: Draft = saved ?? {
    id: null,
    name: '',
    items: [],
    composites: [],
  };
  return canonical(draft) !== canonical(base);
}

/** The key of a plan request: the same key means the same answer, so a late answer can be told apart. */
export function planKey(
  provider: ArsenalProvider,
  items: readonly Item[],
  composites: readonly string[],
): string {
  return JSON.stringify({
    provider,
    items: [...items]
      .map((item) => [item.capability, item.revision])
      .sort((a, b) => String(a[0]).localeCompare(String(b[0]))),
    composites: [...composites].sort(),
  });
}
