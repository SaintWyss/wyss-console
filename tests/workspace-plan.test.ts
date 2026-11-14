import { expect, it } from 'vitest';
import {
  editable,
  freeName,
  launchId,
  savePlan,
  type LaunchDraft,
  type PlanContext,
} from '../src/lib/features/workspace/plan';
import {
  BASE_TOKENS,
  estimate,
  kTokens,
} from '../src/lib/features/workspace/estimate';
import { DEFAULT_SPEC } from '../src/lib/features/arsenal/environment';
import type { Selection } from '../src/lib/contracts/arsenal';
import type { Launch } from '../src/lib/desktop/launch-client';

const selection = (over: Partial<Selection> = {}): Selection => ({
  id: 'sel-1',
  name: 'Estudio',
  provider: 'claude_code',
  items: [{ capability: 'a', revision: 3 }],
  composites: [],
  created_at: 0,
  updated_at: 0,
  ...over,
});
const launch = (
  id: string,
  preset: string | null,
  agent = 'claude_code',
): Launch =>
  ({
    id,
    name: id,
    directory: '/home/me',
    kind: 'session',
    agent,
    profile: null,
    before: [],
    preset,
  }) as Launch;
const draft = (over: Partial<LaunchDraft> = {}): LaunchDraft => ({
  id: 'estudiar',
  name: 'Estudiar',
  role: 'researcher',
  agent: 'claude_code',
  profile: null,
  directory: '/home/me/UTN',
  selection: selection(),
  items: [{ capability: 'a', revision: 3 }],
  composites: [],
  spec: { ...DEFAULT_SPEC },
  ...over,
});
const ctx = (over: Partial<PlanContext> = {}): PlanContext => ({
  launches: [launch('estudiar', 'Estudio')],
  selections: [selection()],
  folderSelection: null,
  savedSpec: null,
  allItems: [
    { capability: 'a', revision: null },
    { capability: 'x', revision: null },
  ],
  ...over,
});

it('derives the file id with the same rule as the desktop', () => {
  expect(launchId('Revisión con Codex')).toBe('revision-con-codex');
  expect(launchId('  UTN · Ñandú!  ')).toBe('utn-nandu');
  expect(launchId('Façade')).toBe('fa-ade');
  expect(launchId('x'.repeat(50))).toHaveLength(40);
});

it('keeps the preset when nothing in it changed, and writes it in place when only this launch uses it', () => {
  expect(savePlan(draft(), ctx()).preset).toEqual({ do: 'keep' });
  const changed = draft({ items: [{ capability: 'b', revision: null }] });
  expect(savePlan(changed, ctx()).preset).toEqual({
    do: 'write',
    id: 'sel-1',
    name: 'Estudio',
  });
});

it('copies a shared preset to one of its own, named after the launch, and never changes the other launch', () => {
  const changed = draft({ items: [] });
  const plan = savePlan(
    changed,
    ctx({
      launches: [launch('estudiar', 'Estudio'), launch('otro', 'Estudio')],
    }),
  );
  expect(plan.preset).toEqual({
    do: 'copy',
    name: 'Estudiar',
    sharedWith: ['otro'],
  });
  // A launch of the other agent with the same preset name is not sharing it.
  expect(
    savePlan(
      changed,
      ctx({
        launches: [
          launch('estudiar', 'Estudio'),
          launch('cx', 'Estudio', 'codex'),
        ],
      }),
    ).preset.do,
  ).toBe('write');
  // The folder remembers this preset: it is shared with the folder too.
  expect(savePlan(changed, ctx({ folderSelection: 'sel-1' })).preset.do).toBe(
    'copy',
  );
});

it('makes a preset for a launch that had none only when its choice differs from everything, with a free name', () => {
  const all = [
    { capability: 'a', revision: null },
    { capability: 'x', revision: null },
  ];
  // Everything on, as a launch without a preset loads: nothing to make.
  expect(
    savePlan(draft({ selection: null, items: all }), ctx()).preset,
  ).toEqual({
    do: 'none',
  });
  expect(
    savePlan(
      draft({ selection: null, items: [{ capability: 'x', revision: null }] }),
      ctx(),
    ).preset,
  ).toEqual({ do: 'create', name: 'Estudiar' });
  expect(freeName('Estudio', ['Estudio', 'estudio 2'])).toBe('Estudio 3');
});

it('writes a default environment over a changed one, so the desktop removes the entry', () => {
  const off = { ...DEFAULT_SPEC, memory: false };
  const back = savePlan(
    draft({ spec: { ...DEFAULT_SPEC } }),
    ctx({ savedSpec: off }),
  );
  expect(back.preset.do).toBe('write');
  expect(back.environment).toBe(true);
  // A new preset with the default environment needs no entry.
  expect(
    savePlan(
      draft({ selection: null, items: [{ capability: 'x', revision: null }] }),
      ctx(),
    ).environment,
  ).toBe(false);
});

it('writes the environment only when it changed, and never for Codex', () => {
  const off = { ...DEFAULT_SPEC, memory: false };
  expect(savePlan(draft({ spec: off }), ctx()).environment).toBe(true);
  expect(
    savePlan(draft({ spec: off }), ctx({ savedSpec: off })).environment,
  ).toBe(false);
  expect(
    savePlan(draft({ agent: 'codex', spec: off }), ctx()).environment,
  ).toBe(false);
});

it('replaces only the file it edits; a rename saves the new name, removes the old one and moves the role', () => {
  expect(savePlan(draft(), ctx()).launch).toEqual({
    id: 'estudiar',
    replace: true,
  });
  const renamed = savePlan(draft({ name: 'Estudiar SIG' }), ctx());
  expect(renamed.launch).toEqual({ id: 'estudiar-sig', replace: false });
  expect(renamed.removeOld).toBe('estudiar');
  expect(renamed.role).toEqual({
    assign: 'researcher',
    renamedFrom: 'estudiar',
  });
  expect(savePlan(draft({ id: null }), ctx()).launch.replace).toBe(false);
});

it('opens a launch with commands or before lines but does not save over it', () => {
  expect(editable(launch('ok', null))).toBe(true);
  expect(editable({ ...launch('b', null), before: ['x'] } as Launch)).toBe(
    false,
  );
  expect(
    editable({
      id: 'c',
      name: 'c',
      directory: '/',
      kind: 'commands',
      commands: ['ls'],
    }),
  ).toBe(false);
});

it('estimates the context by group from what is loaded, and leaves MCP unmeasured', () => {
  const sizes = {
    instructions: 4000,
    'rule:a.md': 400,
    'rule:b.md': 800,
    'account:pdf': 40,
    'plugin:cave@x': 400,
  };
  const base = {
    sizes,
    rules: ['a.md', 'b.md'],
    accountSkills: ['pdf'],
    plugins: ['cave@x'],
    skills: [{ name: 'x', description: 'y'.repeat(75) }],
    agents: [{ name: 'g', description: 'z'.repeat(75) }],
  };
  const all = estimate({ ...base, spec: { ...DEFAULT_SPEC } });
  expect(all).toEqual({
    base: BASE_TOKENS,
    instructions: 1000 + 100 + 200,
    skills: 25,
    agents: 25,
    account: 10,
    plugins: 100,
    hooks: 0,
    mcp: null,
    total: BASE_TOKENS + 1300 + 25 + 25 + 10 + 100,
  });
  const lean = estimate({
    ...base,
    skills: [],
    spec: {
      ...DEFAULT_SPEC,
      instructions: false,
      agents: false,
      account_skills_off: ['pdf'],
      plugins_off: ['cave@x'],
    },
  });
  expect(lean.total).toBe(BASE_TOKENS);
  // A rule switched off is not counted.
  expect(
    estimate({ ...base, spec: { ...DEFAULT_SPEC, rules_off: ['b.md'] } })
      .instructions,
  ).toBe(1100);
  // Same input, same answer.
  expect(estimate({ ...base, spec: { ...DEFAULT_SPEC } })).toEqual(all);
  expect(kTokens(15_360)).toBe('15k');
  expect(kTokens(240)).toBe('<1k');
  // Bytes, not UTF-16 units: "ñ" is two bytes, an emoji four.
  const wide = estimate({
    ...base,
    spec: { ...DEFAULT_SPEC },
    agents: [],
    skills: [{ name: 'ñ', description: '😀'.repeat(18) }],
  });
  expect(wide.skills).toBe(Math.ceil((2 + 72 + 24) / 4));
});
