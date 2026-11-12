import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import NativeHarness from './NativeHarness.svelte';
import { fakeClient } from './native-harness';
import {
  deleteLaunch,
  LaunchError,
  listLaunches,
  saveLaunch,
} from '../src/lib/desktop/launch-client';
import { arsenalClient } from '../src/lib/desktop/arsenal-client';
import { openLaunch } from '../src/lib/features/terminal/launch';
import { onOffCounts } from '../src/lib/features/arsenal/terminal';

vi.mock('../src/lib/desktop/launch-client', async (importOriginal) => {
  const actual =
    await importOriginal<typeof import('../src/lib/desktop/launch-client')>();
  return {
    ...actual,
    saveLaunch: vi.fn(),
    deleteLaunch: vi.fn(),
    listLaunches: vi.fn(async () => ({
      folder: '/home/u',
      launches: [],
      refused: [],
    })),
  };
});
vi.mock('../src/lib/features/terminal/launch', async (importOriginal) => {
  const actual =
    await importOriginal<
      typeof import('../src/lib/features/terminal/launch')
    >();
  return { ...actual, openLaunch: vi.fn(async () => ({ notes: [] })) };
});

let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  vi.clearAllMocks();
});
const settle = async () => {
  for (let i = 0; i < 40; i++) await Promise.resolve();
  flushSync();
};
const text = (node: Element | null | undefined) =>
  node?.textContent?.replace(/\s+/g, ' ').trim() ?? '';
const button = (label: string) =>
  Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find((b) =>
    text(b).includes(label),
  )!;
const formAlert = () =>
  document.querySelector('.launch-save [role="alert"]')?.textContent ?? null;

async function shown() {
  component = mount(NativeHarness, {
    target: document.body,
    props: { client: fakeClient() },
  });
  await settle();
}

describe('Save as launch… on Workspace', () => {
  it('writes the card as a session launch for the chosen agent and folder, and says where it is listed', async () => {
    vi.mocked(saveLaunch).mockResolvedValueOnce({
      id: 'claude-code-u',
      name: 'Claude Code · u',
    } as never);
    await shown();
    button('Save as launch…').click();
    await settle();
    const name = document.querySelector<HTMLInputElement>('#launch-save-name')!;
    expect(name.value).toBe('Claude Code · u');
    button('Save launch').click();
    await settle();
    // The window sends what the card holds: no command, no before line, no replace.
    expect(saveLaunch).toHaveBeenCalledWith({
      name: 'Claude Code · u',
      directory: '/home/u',
      agent: 'claude_code',
      profile: null,
      preset: null,
      replace: false,
    });
    expect(document.querySelector('.launch-save')).toBeNull();
    expect(text(document.querySelector('p.event-meta[role="status"]'))).toBe(
      'Launch saved as “Claude Code · u”. Find it in Launch ▾.',
    );
  });

  it('says a launch with that name exists and replaces it only when the owner asks', async () => {
    vi.mocked(saveLaunch)
      .mockRejectedValueOnce(new LaunchError('exists'))
      .mockResolvedValueOnce({
        id: 'claude-code-u',
        name: 'Claude Code · u',
      } as never);
    await shown();
    button('Save as launch…').click();
    await settle();
    button('Save launch').click();
    await settle();
    expect(formAlert()).toBe('A launch with that name exists.');
    expect(saveLaunch).toHaveBeenCalledTimes(1);
    button('Replace').click();
    await settle();
    expect(saveLaunch).toHaveBeenLastCalledWith(
      expect.objectContaining({ replace: true }),
    );
    expect(document.querySelector('.launch-save')).toBeNull();
  });

  it('keeps the form open and shows the desktop reason when it refuses the launch', async () => {
    vi.mocked(saveLaunch).mockRejectedValueOnce(
      new LaunchError('launch_invalid', [
        'no preset named “Estudio” for this agent.',
      ]),
    );
    await shown();
    button('Save as launch…').click();
    await settle();
    button('Save launch').click();
    await settle();
    expect(formAlert()).toBe('no preset named “Estudio” for this agent.');
    expect(document.querySelector('.launch-save')).not.toBeNull();
    expect(button('Save launch').disabled).toBe(false);
  });
});

describe('Saved launches on Workspace', () => {
  const launchFile = (over: Record<string, unknown> = {}) => ({
    id: 'estudiar',
    name: 'Estudiar',
    directory: '/home/u',
    kind: 'session',
    agent: 'claude_code',
    profile: null,
    before: [],
    preset: 'Estudio',
    ...over,
  });
  const listing = (...launches: unknown[]) =>
    ({ folder: '/home/u', launches, refused: [] }) as never;
  const row = (name: string) =>
    Array.from(document.querySelectorAll<HTMLElement>('.launch-saved li')).find(
      (li) => text(li).includes(name),
    ) ?? null;
  const rowButton = (name: string, label: string) =>
    Array.from(
      row(name)?.querySelectorAll<HTMLButtonElement>('button') ?? [],
    ).find((b) => text(b) === label) ?? null;
  const oneEstudio = [
    { id: 'sel-estudio', name: 'Estudio', provider: 'claude_code' },
  ];

  beforeEach(() => {
    vi.spyOn(arsenalClient, 'selections').mockResolvedValue(
      oneEstudio as never,
    );
  });

  it('lists the folder launches with what each one uses, and starts one through the same opening as Launch ▾', async () => {
    vi.mocked(listLaunches).mockResolvedValue(listing(launchFile()));
    await shown();
    expect(text(row('Estudiar'))).toContain(
      'Claude Code · u · No profile — as configured · Preset Estudio',
    );
    expect(rowButton('Estudiar', 'Start')!.disabled).toBe(false);
    rowButton('Estudiar', 'Start')!.click();
    await settle();
    expect(openLaunch).toHaveBeenCalledWith(
      'estudiar',
      expect.objectContaining({ getLaunch: expect.any(Function) }),
    );
  });

  it('says so when the folder has no launch yet', async () => {
    vi.mocked(listLaunches).mockResolvedValue(listing());
    await shown();
    expect(text(document.querySelector('.launch-saved'))).toContain(
      'No saved launches yet.',
    );
  });

  it('a preset name held twice blocks Start on that launch and says why', async () => {
    vi.mocked(listLaunches).mockResolvedValue(listing(launchFile()));
    vi.spyOn(arsenalClient, 'selections').mockResolvedValue([
      { id: 'a', name: 'Estudio', provider: 'claude_code' },
      { id: 'b', name: 'Estudio', provider: 'claude_code' },
    ] as never);
    await shown();
    expect(rowButton('Estudiar', 'Start')!.disabled).toBe(true);
    expect(text(row('Estudiar'))).toContain(
      'More than one preset is named “Estudio” for this agent.',
    );
  });

  it('Delete asks in line with Cancel focused, and a refusal shows the desktop reason and keeps the row', async () => {
    vi.mocked(listLaunches).mockResolvedValue(listing(launchFile()));
    vi.mocked(deleteLaunch).mockRejectedValueOnce(
      new LaunchError('launch_delete_refused', [
        'it has a before line, so it is not removed here.',
      ]),
    );
    await shown();
    rowButton('Estudiar', 'Delete')!.click();
    await settle();
    expect(text(row('Estudiar'))).toContain('Delete Estudiar?');
    expect(document.activeElement).toBe(rowButton('Estudiar', 'Cancel'));
    rowButton('Estudiar', 'Delete')!.click();
    await settle();
    expect(deleteLaunch).toHaveBeenCalledWith('estudiar');
    expect(row('Estudiar')).not.toBeNull();
    expect(text(row('Estudiar'))).toContain(
      'it has a before line, so it is not removed here.',
    );
  });

  it('a launch the desktop deletes leaves the list', async () => {
    vi.mocked(listLaunches)
      .mockResolvedValueOnce(listing(launchFile()))
      .mockResolvedValue(listing());
    vi.mocked(deleteLaunch).mockResolvedValueOnce(undefined);
    await shown();
    rowButton('Estudiar', 'Delete')!.click();
    await settle();
    rowButton('Estudiar', 'Delete')!.click();
    await settle();
    expect(row('Estudiar')).toBeNull();
    expect(text(document.querySelector('.launch-saved'))).toContain(
      'No saved launches yet.',
    );
  });

  it('when the folder cannot be read the list says so, without the window guessing', async () => {
    vi.mocked(listLaunches).mockRejectedValueOnce(new LaunchError('internal'));
    await shown();
    expect(
      document.querySelector('.launch-saved [role="alert"]')?.textContent,
    ).toBe('The saved launches did not load.');
    expect(row('Estudiar')).toBeNull();
  });
});

describe('The preset card on Workspace', () => {
  const presetSelect = () =>
    document.querySelector<HTMLSelectElement>('#native-preset');
  beforeEach(() => {
    vi.mocked(listLaunches).mockResolvedValue({
      folder: '/home/u',
      launches: [],
      refused: [],
    } as never);
    vi.spyOn(arsenalClient, 'selections').mockResolvedValue([
      { id: 'sel-estudio', name: 'Estudio', provider: 'claude_code' },
      { id: 'sel-otro', name: 'Otro', provider: 'codex' },
    ] as never);
  });

  it('offers the presets of this agent after None, and Edit in Arsenal goes to Arsenal', async () => {
    const onarsenal = vi.fn();
    component = mount(NativeHarness, {
      target: document.body,
      props: { client: fakeClient(), onarsenal },
    });
    await settle();
    // The card is for Claude Code (the start's agent): Codex's preset is not offered here.
    expect(Array.from(presetSelect()!.options).map((o) => o.text)).toEqual([
      'None — all installed skills',
      'Estudio',
    ]);
    button('Edit in Arsenal').click();
    expect(onarsenal).toHaveBeenCalledTimes(1);
  });

  it('choosing a preset plans it for this agent and this folder', async () => {
    vi.spyOn(arsenalClient, 'terminalPlan').mockRejectedValue(
      new Error('no plan'),
    );
    await shown();
    const select = presetSelect()!;
    select.value = 'sel-estudio';
    select.dispatchEvent(new Event('change'));
    await settle();
    expect(arsenalClient.terminalPlan).toHaveBeenCalledWith(
      'sel-estudio',
      'claude_code',
      '/home/u',
    );
  });

  it('a folder with no preset for this agent says so, and still offers Edit in Arsenal', async () => {
    vi.spyOn(arsenalClient, 'selections').mockResolvedValue([
      { id: 'sel-otro', name: 'Otro', provider: 'codex' },
    ] as never);
    component = mount(NativeHarness, {
      target: document.body,
      props: { client: fakeClient(), onarsenal: () => undefined },
    });
    await settle();
    expect(text(document.querySelector('.launch-preset'))).toContain(
      'No presets for Claude Code.',
    );
    expect(button('Edit in Arsenal')).toBeDefined();
  });
});

describe('Preset totals', () => {
  it('counts the skills a plan leaves on, and those it switches off or the settings already did', () => {
    expect(
      onOffCounts({
        kept: ['a', 'b', 'c'],
        inherited_off: ['b'],
        excluded: ['d', 'e'],
      } as never),
    ).toEqual({ on: 2, off: 3 });
  });
});
