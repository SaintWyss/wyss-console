import type {
  ArsenalProvider,
  Composite,
  Item,
  LibraryItem,
  Preview,
} from '../../contracts/arsenal';
import { filterLibrary } from './catalogue';
import { GROUPS, familyFor, groupFor, type Family, type Group } from './groups';

/** The cells a group's meter draws: one per chosen capability, never more than this many. */
export const METER_CELLS = 5;

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
