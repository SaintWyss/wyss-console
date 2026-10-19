import { describe, expect, it } from 'vitest';
import {
  MAX_PANES,
  MIN_RATIO,
  arrange,
  cycle,
  has,
  leaf,
  leaves,
  neighbour,
  readLayout,
  remove,
  resize,
  split,
  type Node,
} from '../src/lib/features/terminal/layout';

const three = (): Node =>
  // a | (b over c)
  split(split(leaf('a'), 'a', 'b', 'row'), 'b', 'c', 'col');

describe('splitting and removing panes', () => {
  it('splits a leaf into itself and a new one, in reading order', () => {
    expect(leaves(three())).toEqual(['a', 'b', 'c']);
    const right = split(leaf('a'), 'a', 'b', 'row');
    expect(right).toMatchObject({ kind: 'split', dir: 'row', ratio: 0.5 });
  });
  it('does not split what is not there, and refuses a full tab and too deep a nesting', () => {
    const one = leaf('a');
    expect(split(one, 'zzz', 'b', 'row')).toBe(one);
    // Four rounds of splitting every pane make sixteen, within the nesting limit.
    let full: Node = leaf('p0');
    let count = 1;
    for (let round = 0; round < 4; round++)
      for (const id of leaves(full))
        full = split(full, id, `p${count++}`, round % 2 ? 'col' : 'row');
    // Nesting stops the tree before the pane limit is reached by splitting one pane forever.
    let deep: Node = leaf('d0');
    for (let n = 1; n < 12; n++)
      deep = split(deep, `d${n - 1}`, `d${n}`, 'row');
    expect(leaves(deep).length).toBeLessThan(12);
    expect(leaves(full).length).toBeLessThanOrEqual(MAX_PANES);
    expect(split(full, 'p0', 'extra', 'row')).toBe(full);
  });
  it('gives the sibling the whole space when a pane goes, and ends with null', () => {
    const tree = three();
    expect(leaves(remove(tree, 'b')!)).toEqual(['a', 'c']);
    expect(remove(remove(tree, 'b')!, 'c')).toEqual(leaf('a'));
    expect(remove(leaf('a'), 'a')).toBeNull();
    expect(remove(tree, 'nope')).toEqual(tree);
  });
});

describe('where panes sit', () => {
  it('divides the area by the ratios and keeps every pane inside it', () => {
    const { panes, dividers } = arrange(three());
    expect(panes.get('a')).toEqual({ x: 0, y: 0, w: 0.5, h: 1 });
    expect(panes.get('b')).toEqual({ x: 0.5, y: 0, w: 0.5, h: 0.5 });
    expect(panes.get('c')).toEqual({ x: 0.5, y: 0.5, w: 0.5, h: 0.5 });
    expect(dividers.map((d) => [d.path, d.dir])).toEqual([
      ['', 'row'],
      ['b', 'col'],
    ]);
    const area = [...panes.values()].reduce((sum, r) => sum + r.w * r.h, 0);
    expect(area).toBeCloseTo(1);
  });
  it('resizes one divider and never below the minimum share', () => {
    const wide = resize(three(), '', 0.7);
    expect(arrange(wide).panes.get('a')!.w).toBeCloseTo(0.7);
    expect(arrange(resize(three(), '', 0)).panes.get('a')!.w).toBeCloseTo(
      MIN_RATIO,
    );
    expect(arrange(resize(three(), '', 5)).panes.get('a')!.w).toBeCloseTo(
      1 - MIN_RATIO,
    );
    expect(arrange(resize(three(), 'b', 0.25)).panes.get('b')!.h).toBeCloseTo(
      0.25,
    );
  });
});

describe('moving between panes', () => {
  it('goes to the pane that is there, by position', () => {
    const tree = three();
    expect(neighbour(tree, 'a', 'right')).toBe('b');
    expect(neighbour(tree, 'b', 'down')).toBe('c');
    expect(neighbour(tree, 'c', 'up')).toBe('b');
    expect(neighbour(tree, 'c', 'left')).toBe('a');
    expect(neighbour(tree, 'a', 'left')).toBeUndefined();
    expect(neighbour(tree, 'a', 'up')).toBeUndefined();
  });
  it('cycles through the panes and wraps', () => {
    const tree = three();
    expect(cycle(tree, 'a', 1)).toBe('b');
    expect(cycle(tree, 'c', 1)).toBe('a');
    expect(cycle(tree, 'a', -1)).toBe('c');
    expect(has(tree, 'b')).toBe(true);
  });
});

describe('a saved layout', () => {
  const saved = {
    version: 1,
    active: 1,
    tabs: [
      { root: leaf('t1'), focus: 't1' },
      { root: split(leaf('t2'), 't2', 't3', 'row'), focus: 't3' },
    ],
    terminals: {
      t1: { title: 'Work', cwd: '/home/u' },
      t2: { title: 'Logs\u0007', cwd: '/var/log' },
      t3: { title: 'x'.repeat(500), cwd: '' },
    },
  };
  it('reads back what was written, cleaned and bounded', () => {
    const layout = readLayout(JSON.parse(JSON.stringify(saved)))!;
    expect(layout.active).toBe(1);
    expect(layout.tabs[1]!.focus).toBe('t3');
    expect(layout.terminals.t2!.title).toBe('Logs');
    expect(layout.terminals.t2!.native).toBe(false);
    expect(layout.terminals.t3!.title).toHaveLength(80);
  });
  it('refuses what is not a layout of this version, or is not a tree', () => {
    expect(readLayout(null)).toBeNull();
    expect(readLayout({ ...saved, version: 2 })).toBeNull();
    expect(readLayout({ ...saved, tabs: 'x' })).toBeNull();
    expect(
      readLayout({ ...saved, tabs: [{ root: { kind: 'leaf' }, focus: 'x' }] }),
    ).toBeNull();
    // The same terminal in two places.
    expect(
      readLayout({ ...saved, tabs: [saved.tabs[0], saved.tabs[0]] }),
    ).toBeNull();
    expect(
      readLayout({
        ...saved,
        tabs: Array.from({ length: 17 }, (_, n) => ({
          root: leaf(`q${n}`),
          focus: `q${n}`,
        })),
      }),
    ).toBeNull();
  });
  it('puts a focus that is not in the tab on its first pane, and clamps a wild ratio', () => {
    const layout = readLayout({
      ...saved,
      tabs: [
        {
          root: {
            kind: 'split',
            dir: 'row',
            ratio: 9,
            a: leaf('t1'),
            b: leaf('t2'),
          },
          focus: 'nope',
        },
      ],
      active: 5,
    })!;
    expect(layout.tabs[0]!.focus).toBe('t1');
    expect(layout.active).toBe(0);
    expect(arrange(layout.tabs[0]!.root).panes.get('t1')!.w).toBeCloseTo(
      1 - MIN_RATIO,
    );
  });
});
