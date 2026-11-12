import { afterEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import NativeHarness from './NativeHarness.svelte';
import { fakeClient } from './native-harness';
import { LaunchError, saveLaunch } from '../src/lib/desktop/launch-client';

vi.mock('../src/lib/desktop/launch-client', async (importOriginal) => {
  const actual =
    await importOriginal<typeof import('../src/lib/desktop/launch-client')>();
  return { ...actual, saveLaunch: vi.fn() };
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
