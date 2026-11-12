import { describe, expect, it } from 'vitest';
import type {
  Composite,
  LibraryItem,
  Preview,
  PreviewItem,
} from '../src/lib/contracts/arsenal';
import {
  filterChooser,
  familiesOf,
  groupRows,
  isDirty,
  keptComposites,
  meterFill,
  planKey,
  totalsOf,
  type Draft,
} from '../src/lib/features/arsenal/presets';

function item(name: string, overrides: Partial<LibraryItem> = {}): LibraryItem {
  return {
    id: `cap-${name}`,
    provider: 'claude_code',
    source: 'claude_user_skills',
    kind: 'skill',
    name,
    description: `Does ${name}`,
    origin: `~/.claude/skills/${name}`,
    revision: 1,
    revisions: 1,
    tree_hash: 'a'.repeat(64),
    updated_at: 1,
    compatibility: { status: 'supported', reason: 'own format' },
    ...overrides,
  };
}

const library = [
  item('frontend-design'),
  item('image-to-code'),
  item('tutor-quiz'),
  item('gsd-plan-phase'),
  item('gstack-qa'),
];

describe('groupRows', () => {
  it('puts every capability in exactly one group, in the order of the group table', () => {
    const rows = groupRows(library, new Set(), {});
    const placed = rows.flatMap((row) => row.items.map((entry) => entry.id));
    expect(placed.sort()).toEqual(library.map((entry) => entry.id).sort());
    const order = rows.map((row) => row.group);
    expect(order).toEqual(
      [...order].sort((a, b) => groupIndex(a) - groupIndex(b)),
    );
  });

  it('counts the chosen capabilities of each group and says whether the group is none, partial or all', () => {
    const rows = groupRows(
      library,
      new Set(['cap-frontend-design', 'cap-image-to-code']),
      {},
    );
    const design = rows.find((row) => row.group === 'design');
    expect(design).toMatchObject({ total: 2, chosen: 2, state: 'all' });
    const study = rows.find((row) => row.group === 'study');
    expect(study).toMatchObject({ total: 1, chosen: 0, state: 'none' });
    const mixed = groupRows(library, new Set(['cap-frontend-design']), {}).find(
      (row) => row.group === 'design',
    );
    expect(mixed?.state).toBe('partial');
  });

  it('leaves out a group the provider has no capability in', () => {
    const groups = groupRows(library, new Set(), {}).map((row) => row.group);
    expect(groups).not.toContain('security');
  });

  it('lets an owner correction win over the rules, and shows an unknown group as Other', () => {
    const moved = groupRows(library, new Set(), {
      'cap-frontend-design': 'study',
    });
    expect(
      moved.find((row) => row.group === 'study')?.items.map((i) => i.name),
    ).toContain('frontend-design');
    expect(
      moved.find((row) => row.group === 'design')?.items.map((i) => i.name),
    ).not.toContain('frontend-design');
    const unknown = groupRows(library, new Set(), {
      'cap-frontend-design': 'a-group-from-the-future',
    });
    expect(
      unknown.find((row) => row.group === 'other')?.items.map((i) => i.name),
    ).toContain('frontend-design');
  });
});

describe('meterFill', () => {
  it('fills one cell per chosen capability and never more than the five cells', () => {
    expect(meterFill(0)).toBe(0);
    expect(meterFill(3)).toBe(3);
    expect(meterFill(5)).toBe(5);
    expect(meterFill(9)).toBe(5);
  });
});

describe('filterChooser and familiesOf', () => {
  it('filters by text, kind and family, all at once', () => {
    expect(
      filterChooser(library, { query: 'qa', kind: 'all', family: 'all' }).map(
        (entry) => entry.name,
      ),
    ).toEqual(['gstack-qa']);
    expect(
      filterChooser(library, { query: '', kind: 'agent', family: 'all' }),
    ).toEqual([]);
    expect(
      filterChooser(library, { query: '', kind: 'all', family: 'GSD' }).map(
        (entry) => entry.name,
      ),
    ).toEqual(['gsd-plan-phase']);
  });

  it('lists only the families the library has, in contract order', () => {
    expect(familiesOf(library)).toEqual(['GSD', 'gstack']);
  });
});

describe('totalsOf', () => {
  const preview = (states: PreviewItem['state'][]): Preview => ({
    selection: {
      id: '',
      name: '',
      provider: 'claude_code',
      items: [],
      composites: [],
      created_at: 0,
      updated_at: 0,
    },
    status: 'not_applied',
    items: states.map((state, index) => ({
      capability: `cap-${index}`,
      state,
      kind: 'skill',
      name: `n${index}`,
      provider: 'claude_code',
      revision: null,
      pinned: false,
      tree_hash: null,
      compatibility: 'supported',
      reason: '',
      dependencies: [],
    })),
    conflicts: [],
    dependencies: 'analysed',
  });

  it('counts resolved items as on and the rest of the provider library as off', () => {
    expect(totalsOf(preview(['resolved', 'resolved', 'missing']), 10)).toEqual({
      on: 2,
      off: 8,
    });
  });

  it('never shows a negative count of off', () => {
    expect(totalsOf(preview(['resolved', 'resolved']), 1)).toEqual({
      on: 2,
      off: 0,
    });
  });
});

describe('keptComposites', () => {
  const composite = (overrides: Partial<Composite>): Composite => ({
    key: 'gsd-core',
    kind: 'gsd_core',
    provider: 'claude_code',
    name: 'GSD Core',
    version: '1.15.0',
    description: '',
    artifacts: 41,
    members: 41,
    origin: '',
    enabled: true,
    methodology: true,
    skills: [],
    agents: [],
    commands: 0,
    hooks: false,
    mcp: false,
    categories: [],
    limitations: [],
    ...overrides,
  });

  it('offers only GSD Core collections of this provider or shared by both', () => {
    const kept = keptComposites(
      [
        composite({}),
        composite({ key: 'codex-only', provider: 'codex' }),
        composite({ key: 'shared', provider: 'shared' }),
        composite({ key: 'plugin', kind: 'claude_plugin' }),
      ],
      'claude_code',
    ).map((entry) => entry.key);
    expect(kept).toEqual(['gsd-core', 'shared']);
  });
});

describe('isDirty', () => {
  const saved: Draft = {
    id: 'sel-1',
    name: 'Estudio',
    items: [
      { capability: 'a', revision: null },
      { capability: 'b', revision: 2 },
    ],
    composites: ['gsd'],
  };

  it('is clean for a new empty preset and dirty once it has a name', () => {
    const empty: Draft = { id: null, name: '', items: [], composites: [] };
    expect(isDirty(empty, null)).toBe(false);
    expect(isDirty({ ...empty, name: 'A' }, null)).toBe(true);
  });

  it('is clean when the draft equals the saved preset, whatever the order of its items', () => {
    expect(
      isDirty(
        {
          ...saved,
          items: [...saved.items].reverse(),
          composites: ['gsd'],
        },
        saved,
      ),
    ).toBe(false);
  });

  it('is dirty when a name, an item, a pinned revision or a composite changes', () => {
    expect(isDirty({ ...saved, name: 'Estudio 2' }, saved)).toBe(true);
    expect(
      isDirty(
        { ...saved, items: [{ capability: 'a', revision: null }] },
        saved,
      ),
    ).toBe(true);
    expect(
      isDirty(
        {
          ...saved,
          items: [
            { capability: 'a', revision: null },
            { capability: 'b', revision: 1 },
          ],
        },
        saved,
      ),
    ).toBe(true);
    expect(isDirty({ ...saved, composites: [] }, saved)).toBe(true);
  });
});

describe('planKey', () => {
  it('is the same for the same draft in any order, and differs when the draft differs', () => {
    const a = planKey(
      'codex',
      [
        { capability: 'a', revision: null },
        { capability: 'b', revision: null },
      ],
      ['x', 'y'],
    );
    const b = planKey(
      'codex',
      [
        { capability: 'b', revision: null },
        { capability: 'a', revision: null },
      ],
      ['y', 'x'],
    );
    expect(a).toBe(b);
    expect(planKey('claude_code', [], [])).not.toBe(planKey('codex', [], []));
  });
});

const ORDER = [
  'models',
  'security',
  'debug',
  'content',
  'motion',
  'design',
  'study',
  'decide',
  'research',
  'review',
  'docs',
  'planning',
  'build',
  'system',
  'other',
];
function groupIndex(group: string): number {
  return ORDER.indexOf(group);
}
