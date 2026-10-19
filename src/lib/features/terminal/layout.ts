/**
 * The layout of one tab: a tree of splits whose leaves are terminals (T1, TF1). Pure data and
 * functions, so the shape of a tab can be tested without a terminal: how a leaf is split,
 * removed, resized and found, where each leaf and each divider sits (as fractions of the tab's
 * area), and how a layout is written down and read back (T2).
 */
export type Direction = 'row' | 'col';

export type Node<Id extends string = string> =
  | { kind: 'leaf'; id: Id }
  | {
      kind: 'split';
      /** `row`: side by side; `col`: one above the other. */
      dir: Direction;
      /** The share of the first child, between MIN and 1 − MIN. */
      ratio: number;
      a: Node<Id>;
      b: Node<Id>;
    };

/** A pane is never given less than this share of a split. */
export const MIN_RATIO = 0.1;
/** A tab holds at most this many panes (the terminal limit applies besides). */
export const MAX_PANES = 16;
/** Splits nest at most this deep. */
export const MAX_DEPTH = 6;

export const leaf = <Id extends string>(id: Id): Node<Id> => ({
  kind: 'leaf',
  id,
});

export function leaves<Id extends string>(node: Node<Id>): Id[] {
  return node.kind === 'leaf'
    ? [node.id]
    : [...leaves(node.a), ...leaves(node.b)];
}

export function has<Id extends string>(node: Node<Id>, id: Id): boolean {
  return leaves(node).includes(id);
}

function depth<Id extends string>(node: Node<Id>): number {
  return node.kind === 'leaf' ? 0 : 1 + Math.max(depth(node.a), depth(node.b));
}

const clamp = (ratio: number) =>
  Math.min(1 - MIN_RATIO, Math.max(MIN_RATIO, ratio));

/**
 * `target` becomes a split of itself and `added`, `added` second. Refused (the same tree is
 * returned) when the tab is full or would nest too deep.
 */
export function split<Id extends string>(
  root: Node<Id>,
  target: Id,
  added: Id,
  dir: Direction,
): Node<Id> {
  if (leaves(root).length >= MAX_PANES || !has(root, target)) return root;
  const next = (node: Node<Id>): Node<Id> => {
    if (node.kind === 'leaf')
      return node.id === target
        ? { kind: 'split', dir, ratio: 0.5, a: node, b: leaf(added) }
        : node;
    return { ...node, a: next(node.a), b: next(node.b) };
  };
  const result = next(root);
  return depth(result) > MAX_DEPTH ? root : result;
}

/** The tree without `id`: its sibling takes the whole of the space. `null` when none is left. */
export function remove<Id extends string>(
  root: Node<Id>,
  id: Id,
): Node<Id> | null {
  if (root.kind === 'leaf') return root.id === id ? null : root;
  const a = remove(root.a, id);
  const b = remove(root.b, id);
  if (a === null) return b;
  if (b === null) return a;
  return { ...root, a, b };
}

/** The share of the `n`-th split in reading order changed (clamped). */
export function resize<Id extends string>(
  root: Node<Id>,
  path: string,
  ratio: number,
): Node<Id> {
  const next = (node: Node<Id>, here: string): Node<Id> => {
    if (node.kind === 'leaf') return node;
    if (here === path) return { ...node, ratio: clamp(ratio) };
    return {
      ...node,
      a: next(node.a, `${here}a`),
      b: next(node.b, `${here}b`),
    };
  };
  return next(root, '');
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}
export interface Divider {
  /** Where this split is in the tree: `''` is the root, then `a` or `b` per level. */
  path: string;
  dir: Direction;
  ratio: number;
  /** The area the whole split covers: the divider is drawn across it. */
  area: Rect;
}

/** Where every leaf and every divider sits, as fractions of the tab's area (0 to 1). */
export function arrange<Id extends string>(
  root: Node<Id>,
): { panes: Map<Id, Rect>; dividers: Divider[] } {
  const panes = new Map<Id, Rect>();
  const dividers: Divider[] = [];
  const walk = (node: Node<Id>, area: Rect, path: string) => {
    if (node.kind === 'leaf') {
      panes.set(node.id, area);
      return;
    }
    dividers.push({ path, dir: node.dir, ratio: node.ratio, area });
    if (node.dir === 'row') {
      const w = area.w * node.ratio;
      walk(node.a, { ...area, w }, `${path}a`);
      walk(node.b, { ...area, x: area.x + w, w: area.w - w }, `${path}b`);
    } else {
      const h = area.h * node.ratio;
      walk(node.a, { ...area, h }, `${path}a`);
      walk(node.b, { ...area, y: area.y + h, h: area.h - h }, `${path}b`);
    }
  };
  walk(root, { x: 0, y: 0, w: 1, h: 1 }, '');
  return { panes, dividers };
}

/**
 * The pane next to `from` in a direction, by where the panes sit: the nearest whose edge
 * touches the way out and whose span overlaps `from`'s middle.
 */
export function neighbour<Id extends string>(
  root: Node<Id>,
  from: Id,
  way: 'left' | 'right' | 'up' | 'down',
): Id | undefined {
  const { panes } = arrange(root);
  const here = panes.get(from);
  if (!here) return undefined;
  const cx = here.x + here.w / 2;
  const cy = here.y + here.h / 2;
  let best: { id: Id; gap: number } | undefined;
  for (const [id, r] of panes) {
    if (id === from) continue;
    const gap =
      way === 'right'
        ? r.x - (here.x + here.w)
        : way === 'left'
          ? here.x - (r.x + r.w)
          : way === 'down'
            ? r.y - (here.y + here.h)
            : here.y - (r.y + r.h);
    const across =
      way === 'left' || way === 'right'
        ? cy >= r.y - 1e-9 && cy <= r.y + r.h + 1e-9
        : cx >= r.x - 1e-9 && cx <= r.x + r.w + 1e-9;
    if (gap >= -1e-9 && across && (!best || gap < best.gap)) best = { id, gap };
  }
  return best?.id;
}

/** The next or previous leaf in reading order, wrapping. */
export function cycle<Id extends string>(
  root: Node<Id>,
  from: Id,
  step: 1 | -1,
): Id | undefined {
  const all = leaves(root);
  const at = all.indexOf(from);
  return at < 0 ? undefined : all[(at + step + all.length) % all.length];
}

// ---- writing a layout down and reading it back (T2) ---------------------------------------

export const LAYOUT_VERSION = 1;
/** At most this many tabs are kept. */
export const MAX_TABS = 16;

export interface SavedLeaf {
  /** The host's terminal the pane showed. */
  terminal: string;
  /** What to call a terminal that has to be made again. */
  title: string;
  /** Where its shell started. */
  cwd: string;
}
export interface SavedTab {
  root: Node<string>;
  /** The terminal that had the focus. */
  focus: string;
}
export interface SavedLayout {
  version: typeof LAYOUT_VERSION;
  tabs: SavedTab[];
  /** The index of the active tab. */
  active: number;
  /** What each terminal was (leaves are ids into this). */
  terminals: Record<string, { title: string; cwd: string; native: boolean }>;
}

const word = (text: string, most: number) =>
  [...text]
    .filter((c) => c >= ' ' && c !== '\u007f')
    .slice(0, most)
    .join('');

function readNode(
  value: unknown,
  ids: Set<string>,
  level: number,
): Node<string> {
  if (typeof value !== 'object' || value === null || level > MAX_DEPTH)
    throw new Error('layout');
  const v = value as Record<string, unknown>;
  if (v.kind === 'leaf') {
    if (typeof v.id !== 'string' || v.id.length > 64 || ids.has(v.id))
      throw new Error('layout');
    ids.add(v.id);
    return leaf(v.id);
  }
  if (
    v.kind === 'split' &&
    (v.dir === 'row' || v.dir === 'col') &&
    typeof v.ratio === 'number' &&
    Number.isFinite(v.ratio)
  )
    return {
      kind: 'split',
      dir: v.dir,
      ratio: clamp(v.ratio),
      a: readNode(v.a, ids, level + 1),
      b: readNode(v.b, ids, level + 1),
    };
  throw new Error('layout');
}

/**
 * A saved layout, or `null` when what was read is not one. Bounded in tabs, panes, nesting and
 * text; titles lose control characters; nothing in it is ever used as a command.
 */
export function readLayout(value: unknown): SavedLayout | null {
  try {
    if (typeof value !== 'object' || value === null) return null;
    const v = value as Record<string, unknown>;
    if (v.version !== LAYOUT_VERSION || !Array.isArray(v.tabs)) return null;
    if (v.tabs.length > MAX_TABS) return null;
    const known = new Set<string>();
    const tabs: SavedTab[] = v.tabs.map((t: unknown) => {
      const tab = t as Record<string, unknown>;
      const ids = new Set<string>();
      const root = readNode(tab.root, ids, 0);
      if (ids.size > MAX_PANES) throw new Error('layout');
      for (const id of ids) {
        if (known.has(id)) throw new Error('layout');
        known.add(id);
      }
      const focus =
        typeof tab.focus === 'string' && ids.has(tab.focus)
          ? tab.focus
          : (leaves(root)[0] as string);
      return { root, focus };
    });
    const terminals: SavedLayout['terminals'] = {};
    const given = (v.terminals ?? {}) as Record<string, unknown>;
    for (const id of known) {
      const t = (given[id] ?? {}) as Record<string, unknown>;
      terminals[id] = {
        title:
          word(typeof t.title === 'string' ? t.title : 'Terminal', 80) ||
          'Terminal',
        cwd: word(typeof t.cwd === 'string' ? t.cwd : '', 1024),
        // An agent is never made again from a layout.
        native: t.native === true,
      };
    }
    const active =
      Number.isInteger(v.active) &&
      (v.active as number) >= 0 &&
      (v.active as number) < tabs.length
        ? (v.active as number)
        : 0;
    return { version: LAYOUT_VERSION, tabs, active, terminals };
  } catch {
    return null;
  }
}
