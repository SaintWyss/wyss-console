import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, tick, unmount } from 'svelte';
import { mockIPC } from '@tauri-apps/api/mocks';
import LaunchEditor from '../src/lib/features/workspace/LaunchEditor.svelte';
import type { Launch, LaunchList } from '../src/lib/desktop/launch-client';
import type { LibraryItem, Selection } from '../src/lib/contracts/arsenal';
import type { Roles, ArsenalClient } from '../src/lib/desktop/arsenal-client';
import {
  DEFAULT_SPEC,
  type HomeSpec,
} from '../src/lib/features/arsenal/environment';

let component: ReturnType<typeof mount> | undefined;
let ipc: { command: string; args: unknown }[] = [];
afterEach(() => {
  if (component) void unmount(component);
  component = undefined;
  document.body.replaceChildren();
});
beforeEach(() => {
  ipc = [];
  mockIPC((command, args) => {
    ipc.push({ command, args });
    if (command === 'launch_save') {
      const r = (args as { request: Record<string, unknown> }).request;
      return {
        id: String(r.name)
          .toLowerCase()
          .replace(/[^a-z0-9]+/g, '-'),
        name: r.name,
        directory: r.directory,
        kind: 'session',
        agent: r.agent,
        profile: r.profile,
        before: [],
        preset: r.preset,
      };
    }
    return null;
  });
});

const lib = (id: string, kind: string, name = id): LibraryItem =>
  ({
    id,
    provider: 'claude_code',
    kind,
    name,
    description: `${name} does things`,
  }) as unknown as LibraryItem;
const session = (id: string, preset: string | null): Launch => ({
  id,
  name: id === 'estudiar' ? 'Estudiar' : id,
  directory: '/home/me/UTN',
  kind: 'session',
  agent: 'claude_code',
  profile: null,
  before: [],
  preset,
});
const sel: Selection = {
  id: 'sel-1',
  name: 'Estudio',
  provider: 'claude_code',
  // A pinned revision the editor does not show: it must come back as it was.
  items: [
    { capability: 's1', revision: 7 },
    { capability: 's2', revision: null },
  ],
  composites: ['gsd-core'],
  created_at: 0,
  updated_at: 0,
};
// Choices the editor does not draw (memory, plugins): they must be saved as they were.
const env: HomeSpec = {
  ...DEFAULT_SPEC,
  memory: false,
  plugins_off: ['caveman@caveman'],
};

function fake(
  over: Partial<{
    roles: Roles;
    launches: Launch[];
    failRole: boolean;
    failEnvironment: boolean;
  }> = {},
): {
  client: ArsenalClient;
  calls: Record<string, unknown[]>;
} {
  const calls: Record<string, unknown[]> = {};
  const note = (name: string, value: unknown) =>
    (calls[name] ??= []).push(value);
  const roles: Roles = over.roles ?? {
    custom: [],
    assigned: { estudiar: 'researcher' },
    damaged: false,
  };
  const client = {
    roles: async () => roles,
    writeRole: async (change: unknown) => {
      note('writeRole', change);
      if (over.failRole) throw new Error('roles.json could not be written.');
      return roles;
    },
    capabilities: async () => [
      lib('s1', 'skill', 'frontend-design'),
      lib('s2', 'skill', 'systematic-debugging'),
      lib('a1', 'agent', 'gsd-executor'),
    ],
    selections: async () => [sel],
    environmentOptions: async () => ({
      hooks: [],
      mcp: ['context7'],
      plugins: ['caveman@caveman'],
      rules: ['ux.md'],
      account_skills: [],
      instructions: true,
      token_stored: true,
      sizes: { instructions: 8000, 'rule:ux.md': 400 },
    }),
    environment: async () => env,
    projectSelection: async () => null,
    saveSelection: async (input: Record<string, unknown>) => {
      note('saveSelection', input);
      return { ...sel, ...input, id: input.id ?? 'sel-new' } as Selection;
    },
    writeEnvironment: async (id: string, spec: HomeSpec) => {
      note('writeEnvironment', { id, spec });
      if (over.failEnvironment)
        throw new Error('environment.json could not be written.');
    },
  } as unknown as ArsenalClient;
  void over.launches;
  return { client, calls };
}

async function show(launches: Launch[], over: Parameters<typeof fake>[0] = {}) {
  const f = fake(over);
  component = mount(LaunchEditor, {
    target: document.body,
    props: {
      visible: true,
      profiles: [],
      recent: [{ path: '/home/me/UTN', last_used: 2 }],
      choose: async () => null,
      onopen: vi.fn(),
      client: f.client,
      launches: async () =>
        ({ folder: '/c', launches, refused: [] }) as LaunchList,
    },
  });
  for (let i = 0; i < 6; i++) await tick();
  flushSync();
  return f;
}
const text = (s: string) => document.querySelector(s)?.textContent ?? '';
const button = (label: string) =>
  [...document.querySelectorAll<HTMLButtonElement>('button')].find(
    (b) => b.textContent?.trim() === label,
  )!;
async function settle() {
  for (let i = 0; i < 12; i++) await tick();
  flushSync();
}

it('groups launches by role and shows the estimated context', async () => {
  await show([session('estudiar', 'Estudio'), session('otro', null)]);
  const roles = [...document.querySelectorAll('.le-role')].map(
    (r) => r.textContent,
  );
  expect(roles).toEqual(['Worker', 'Researcher']);
  expect(text('.le-total')).toMatch(/~\d+k/);
  expect(text('.le-total')).toContain('MCP not measured');
});

it('copies a shared preset to the launch’s own, and keeps what it does not show', async () => {
  const { calls } = await show([
    session('estudiar', 'Estudio'),
    session('otro', 'Estudio'),
  ]);
  // Untick one skill of the preset: Estudio is shared with "otro".
  document
    .querySelector<HTMLButtonElement>('.le-gtoggle[aria-expanded="true"]')
    ?.click();
  flushSync();
  const skills = [...document.querySelectorAll('.le-group')].find((g) =>
    g.textContent?.includes('Skills'),
  )!;
  skills.querySelector<HTMLButtonElement>('.le-gtoggle')!.click();
  flushSync();
  const row = [...skills.querySelectorAll<HTMLLabelElement>('.le-row')].find(
    (r) => r.textContent?.includes('systematic-debugging'),
  )!;
  row.querySelector('input')!.click();
  flushSync();
  button('Save').click();
  await settle();
  // A new preset named after the launch; the shared one is untouched.
  expect(calls.saveSelection).toEqual([
    {
      id: null,
      name: 'Estudiar',
      provider: 'claude_code',
      items: [{ capability: 's1', revision: 7 }],
      composites: ['gsd-core'],
    },
  ]);
  // The copy carries the environment, with what the editor does not draw kept.
  // The environment goes to the new preset, never to the shared one.
  expect(calls.writeEnvironment).toEqual([{ id: 'sel-new', spec: env }]);
  const written = (calls.writeEnvironment as { spec: HomeSpec }[])[0]!.spec;
  expect(written.memory).toBe(false);
  expect(written.plugins_off).toEqual(['caveman@caveman']);
  const save = ipc.find((c) => c.command === 'launch_save')!.args as {
    request: Record<string, unknown>;
  };
  expect(save.request).toMatchObject({
    name: 'Estudiar',
    preset: 'Estudiar',
    replace: true,
  });
  expect(calls.writeRole).toEqual([
    { do: 'assign', launch: 'estudiar', role: 'researcher' },
  ]);
});

it('opens a launch with commands but does not save over it', async () => {
  await show([
    {
      id: 'cmds',
      name: 'cmds',
      directory: '/home/me',
      kind: 'commands',
      commands: ['ls'],
    },
  ]);
  expect(text('.notice[data-tone="warn"]')).toContain('edit it in its file');
  expect(button('Save').disabled).toBe(true);
  expect(document.querySelector('.le-delete')).toBeNull();
});

it('says what was saved when a later step fails, and keeps the draft', async () => {
  mockIPC((command) => {
    if (command === 'launch_save')
      return Promise.reject({ code: 'launch_invalid', reasons: ['no folder'] });
    return null;
  });
  await show([session('estudiar', 'Estudio')]);
  const skills = [...document.querySelectorAll('.le-group')].find((g) =>
    g.textContent?.includes('Skills'),
  )!;
  const first = skills.querySelector<HTMLInputElement>('.le-ghead input')!;
  first.click();
  flushSync();
  button('Save').click();
  await settle();
  expect(text('.notice[data-tone="error"]')).toContain(
    'Saved so far: preset “Estudio”',
  );
  // The draft is still there to save again.
  expect(button('Save').disabled).toBe(false);
});

const skillsGroup = () =>
  [...document.querySelectorAll('.le-group')].find((g) =>
    g.textContent?.includes('Skills'),
  )!;
function untickAllSkills() {
  skillsGroup().querySelector<HTMLInputElement>('.le-ghead input')!.click();
  flushSync();
}
function rename(name: string) {
  const title = document.querySelector<HTMLInputElement>('.le-title')!;
  title.value = name;
  title.dispatchEvent(new Event('input'));
  flushSync();
}

it('stops at a failed environment and says the preset was saved, launch untouched', async () => {
  const { calls } = await show([session('estudiar', 'Estudio')], {
    failEnvironment: true,
  });
  // Memory off is drawn nowhere: change a hook-less thing the editor shows instead.
  const mcp = [...document.querySelectorAll('.le-group')].find((g) =>
    g.textContent?.includes('MCP servers'),
  )!;
  mcp.querySelector<HTMLInputElement>('.le-ghead input')!.click();
  flushSync();
  button('Save').click();
  await settle();
  expect(calls.writeEnvironment).toHaveLength(1);
  expect(ipc.some((c) => c.command === 'launch_save')).toBe(false);
  expect(text('.notice[data-tone="error"]')).toContain(
    'environment.json could not be written.',
  );
});

it('on a rename, keeps the old file when the role cannot follow, and says so', async () => {
  const { calls } = await show([session('estudiar', 'Estudio')], {
    failRole: true,
  });
  rename('Estudiar SIG');
  button('Save').click();
  await settle();
  expect(ipc.some((c) => c.command === 'launch_save')).toBe(true);
  // The role did not follow: the old file is not deleted.
  expect(ipc.some((c) => c.command === 'launch_delete')).toBe(false);
  expect(calls.writeRole).toEqual([
    { do: 'rename', from: 'estudiar', to: 'estudiar-sig' },
  ]);
  expect(text('.notice[data-tone="error"]')).toContain('Saved so far:');
  expect(text('.notice[data-tone="error"]')).toContain('launch');
});

it('on a rename that saves, moves the role before deleting the old file', async () => {
  const { calls } = await show([session('estudiar', 'Estudio')]);
  rename('Estudiar SIG');
  button('Save').click();
  await settle();
  expect(calls.writeRole).toEqual([
    { do: 'rename', from: 'estudiar', to: 'estudiar-sig' },
    { do: 'assign', launch: 'estudiar-sig', role: 'researcher' },
  ]);
  const order = ipc
    .map((c) => c.command)
    .filter((c) => c === 'launch_save' || c === 'launch_delete');
  expect(order).toEqual(['launch_save', 'launch_delete']);
});

it('saves the draft as it was when Save was pressed, with the fields disabled meanwhile', async () => {
  let release: () => void = () => undefined;
  const { client, calls } = fake();
  const gate = new Promise<void>((r) => (release = r));
  const slow = {
    ...client,
    projectSelection: async () => {
      await gate;
      return null;
    },
  } as ArsenalClient;
  component = mount(LaunchEditor, {
    target: document.body,
    props: {
      visible: true,
      profiles: [],
      recent: [],
      choose: async () => null,
      onopen: vi.fn(),
      client: slow,
      launches: async () =>
        ({
          folder: '/c',
          launches: [session('estudiar', 'Estudio')],
          refused: [],
        }) as LaunchList,
    },
  });
  await settle();
  untickAllSkills();
  button('Save').click();
  flushSync();
  expect(document.querySelector<HTMLInputElement>('.le-title')!.disabled).toBe(
    true,
  );
  expect(
    document.querySelector<HTMLFieldSetElement>('.le-fieldset')!.disabled,
  ).toBe(true);
  // Typed while saving: not saved.
  rename('Otro nombre');
  release();
  await settle();
  const save = ipc.find((c) => c.command === 'launch_save')!.args as {
    request: Record<string, unknown>;
  };
  expect(save.request.name).toBe('Estudiar');
  expect(calls.saveSelection).toHaveLength(1);
});

it('says roles.json is damaged and that roles are not saved', async () => {
  await show([session('estudiar', null)], {
    roles: { custom: [], assigned: {}, damaged: true },
  });
  expect(text('.notice[data-tone="warn"]')).toContain(
    'roles.json could not be read',
  );
});

it('shows a command launch as its name, a notice and Try: no distributor, model or estimate', async () => {
  await show([
    {
      id: 'upd',
      name: 'Actualizar',
      directory: '/home/me',
      kind: 'commands',
      commands: ['paru'],
    },
  ]);
  expect(document.querySelector('.le-fields')).toBeNull();
  expect(document.querySelector('.le-loads')).toBeNull();
  expect(text('.le-total')).toBe('');
  expect(button('Try').disabled).toBe(false);
});

it('says before saving that a file under another name is written anew and the old one removed', async () => {
  await show([{ ...session('a-mano', null), name: 'Mi Lanzamiento' }]);
  expect(text('.notice[data-tone="warn"]')).toContain(
    'saved as a-mano.toml. Saving writes mi-lanzamiento.toml and removes a-mano.toml',
  );
});
