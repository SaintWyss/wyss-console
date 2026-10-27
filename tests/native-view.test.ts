import { afterEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import NativeHarness from './NativeHarness.svelte';
import type { NativeClient } from '../src/lib/features/native/controller';
import type { Preview, Profile } from '../src/lib/contracts/native';
import {
  fakeClient,
  launcher,
  profiles,
  reply,
  session,
} from './native-harness';

let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
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
const profileId = 'b'.repeat(32);
const parentProfileId = 'c'.repeat(32);

function preview(token: string, applied = `Model: ${token}.`): Preview {
  return {
    applied: [applied],
    with_cost: [],
    not_applicable: [],
    can_start: true,
    through_shell: true,
    warnings: [],
    plan_token: token,
    provider_version: '2.1.309 (Claude Code)',
  };
}

function updateProfile(
  profiles: Profile[],
  id: string,
  body: Profile['body'],
): Profile[] {
  return profiles.map((profile) => {
    if (profile.id !== id) return profile;
    const next = profile.current + 1;
    return {
      ...profile,
      current: next,
      versions: [
        ...profile.versions,
        {
          n: next,
          saved_at: 1_700_000_200,
          hash: `revision-${next}`,
          name: body.name,
        },
      ],
      body,
    };
  });
}

async function selectProfile(id: string) {
  const select = document.querySelector<HTMLSelectElement>('#native-profile')!;
  select.value = id;
  select.dispatchEvent(new Event('change', { bubbles: true }));
  await settle();
}

async function editProfileInProfilesView(name: string, model: string) {
  Array.from(document.querySelectorAll('.native-profile-item'))
    .find((node) => text(node).includes(name))!
    .dispatchEvent(new MouseEvent('click', { bubbles: true }));
  await settle();
  const input = document.querySelector<HTMLInputElement>('#p-model')!;
  input.value = model;
  input.dispatchEvent(new Event('input', { bubbles: true }));
  input.dispatchEvent(new Event('change', { bubbles: true }));
  await settle();
  const save = button('Save as a new version');
  expect(save.disabled).toBe(false);
  save.click();
  await settle();
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

async function shown(
  client: NativeClient,
  props: Record<string, unknown> = {},
) {
  component = mount(NativeHarness, {
    target: document.body,
    props: { client, ...props },
  });
  await settle();
}

describe('the launcher', () => {
  it('offers the agents of the catalogue, says which are installed and what the shell does', async () => {
    await shown(fakeClient());
    const agents = Array.from(document.querySelectorAll('.native-agent'));
    expect(agents.map((a) => text(a))).toEqual([
      'Claude Code 2.1.303 (Claude Code)',
      'Codex Not installed',
    ]);
    expect(agents[1]!.querySelector('input')!.disabled).toBe(true);
    expect(text(document.querySelector('.native-launch'))).toContain(
      'Starts through your shell (fish)',
    );
    expect(text(document.querySelector('.native-note'))).toContain(
      'does not mediate',
    );
  });

  it('shows exact-build syntax evidence without claiming effective provider use', async () => {
    const current = launcher();
    const claude = current.agents[0]!;
    await shown(
      fakeClient({
        launcher: vi.fn(async () =>
          launcher({
            agents: [
              {
                ...claude,
                version: '2.1.309 (Claude Code)',
                profile_capability: {
                  status: 'syntax_verified',
                  qualified_version: '2.1.309 (Claude Code)',
                  checked_on: '2026-10-23',
                  operations: [
                    'model via --model',
                    'reasoning effort via --effort',
                  ],
                  interface: '--model and --effort',
                  format: 'argv flags',
                  scope: 'one process',
                  reload_semantics: 'Not tested',
                  source: 'installed CLI --help',
                  limitation:
                    'CLI syntax only; provider-resolved and effective values are not reported.',
                },
              },
              current.agents[1],
            ],
          }),
        ),
      }),
    );
    const evidence = text(document.querySelector('.native-capability'));
    expect(evidence).toContain('syntax is verified');
    expect(evidence).toContain('2.1.309 (Claude Code)');
    expect(evidence).toContain('effective values are not reported');
    const details = text(document.querySelector('.native-capability-details'));
    expect(details).toContain('reasoning effort via --effort');
    expect(details).toContain('Reload: Not tested.');
  });

  it('disables profile overrides for an unverified build while preserving As configured', async () => {
    const current = launcher();
    const client = fakeClient({
      launcher: vi.fn(async () =>
        launcher({
          agents: [{ ...current.agents[0]!, version: '2.1.310 (Claude Code)' }],
        }),
      ),
      preview: vi.fn(async () => ({
        applied: ['Model: sonnet.'],
        with_cost: [],
        not_applicable: [
          'Profile overrides are refused for an unverified provider build.',
        ],
        can_start: false,
        through_shell: true,
        warnings: [
          'Profile overrides are not verified for 2.1.310 (Claude Code). Choose As configured.',
        ],
        plan_token: 'preview-token',
        provider_version: '2.1.310 (Claude Code)',
      })),
    });
    await shown(client);
    const select =
      document.querySelector<HTMLSelectElement>('#native-profile')!;
    select.value = 'b'.repeat(32);
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();

    expect(text(document.querySelector('.native-capability'))).toContain(
      'unverified for 2.1.310 (Claude Code)',
    );
    expect(text(document.querySelector('.native-preview'))).toContain(
      'not verified for 2.1.310 (Claude Code)',
    );
    expect(button('Start Claude Code').disabled).toBe(true);
    expect(
      Array.from(select.options).find((option) => option.value === '')?.text,
    ).toContain('as configured');
  });

  it('starts the chosen agent on the chosen project with the chosen profile and shows the terminals', async () => {
    const client = fakeClient();
    const started = vi.fn();
    const onconsole = vi.fn();
    await shown(client, { started, onconsole });
    const project =
      document.querySelector<HTMLSelectElement>('#native-project')!;
    // The project the registry knows remembers its agent and profile.
    project.value = '/home/u/work/app';
    project.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();
    expect(
      document.querySelector<HTMLSelectElement>('#native-profile')!.value,
    ).toBe('template-review');
    button('Start Claude Code').click();
    await settle();
    expect(client.start).toHaveBeenCalledWith(
      'claude_code',
      '/home/u/work/app',
      'template-review',
      null,
      expect.objectContaining({
        plan_token: 'preview-token',
        provider_version: '2.1.309 (Claude Code)',
      }),
    );
    expect(started).toHaveBeenCalledTimes(1);
    expect(onconsole).toHaveBeenCalledTimes(1);
  });

  it('shows what a profile does for the agent and will not start one the agent cannot apply', async () => {
    const client = fakeClient({
      preview: vi.fn(async () => ({
        applied: ['Model: sonnet.'],
        with_cost: ['Hooks are off for this session only.'],
        not_applicable: ['Codex hooks cannot be switched off for one session.'],
        can_start: false,
        through_shell: true,
        warnings: [],
        plan_token: 'preview-token',
        provider_version: '2.1.309 (Claude Code)',
      })),
      launcher: vi.fn(async () =>
        launcher({
          agents: [
            {
              agent_id: 'codex',
              name: 'Codex',
              executable: 'codex',
              installed: true,
              path: '/x',
              version: '1',
            },
          ],
        }),
      ),
    });
    await shown(client);
    const select =
      document.querySelector<HTMLSelectElement>('#native-profile')!;
    select.value = 'template-review';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();
    const preview = text(document.querySelector('.native-preview'));
    expect(preview).toContain('Model: sonnet.');
    expect(preview).toContain('Hooks are off for this session only.');
    expect(preview).toContain('Codex hooks cannot be switched off');
    expect(button('Start Codex').disabled).toBe(true);
    expect(text(document.querySelector('.native-warning'))).toContain(
      'will not start',
    );
  });

  it('refreshes a selected profile after a direct edit while Profiles is open and starts only with the new token', async () => {
    let savedProfiles = profiles();
    const fresh = deferred<Preview>();
    let previewCalls = 0;
    const client = fakeClient({
      profiles: vi.fn(async () => structuredClone(savedProfiles)),
      saveProfile: vi.fn(async (id, body) => {
        savedProfiles = updateProfile(savedProfiles, id, body);
      }),
      preview: vi.fn(async () => {
        previewCalls++;
        return previewCalls === 1 ? preview('old-token') : fresh.promise;
      }),
    });
    await shown(client);
    await selectProfile(profileId);
    expect(previewCalls).toBe(1);
    expect(button('Start Claude Code').disabled).toBe(false);

    button('Profiles').click();
    await settle();
    await editProfileInProfilesView('Mine', 'opus');
    await settle();
    expect(client.saveProfile).toHaveBeenCalledTimes(1);
    expect(previewCalls).toBe(2);

    // WorkspaceView remains mounted while hidden, and its new token is required on return.
    button('Workspace').click();
    await settle();
    expect(button('Start Claude Code').disabled).toBe(true);
    expect(text(document.querySelector('[role="status"]'))).toContain(
      'Refreshing the profile preview',
    );
    expect(document.querySelector('.native-preview')).toBeNull();

    fresh.resolve(preview('new-token', 'Model: opus.'));
    await settle();
    expect(button('Start Claude Code').disabled).toBe(false);
    expect(text(document.querySelector('.native-preview'))).toContain(
      'Model: opus.',
    );
    expect(previewCalls).toBe(2);
    button('Start Claude Code').click();
    await settle();
    expect(client.start).toHaveBeenCalledWith(
      'claude_code',
      '/home/u',
      profileId,
      null,
      expect.objectContaining({ plan_token: 'new-token' }),
    );
  });

  it('refreshes a selected child profile when an inherited parent changes', async () => {
    const base = profiles();
    const child = base.find((item) => item.id === profileId)!;
    let savedProfiles: Profile[] = [
      ...base.filter((item) => item.id !== profileId),
      {
        ...child,
        current: 1,
        versions: [
          { n: 1, saved_at: 1_700_000_100, hash: 'child-v1', name: 'Child' },
        ],
        body: {
          ...child.body,
          name: 'Child',
          model: null,
          extends: parentProfileId,
        },
      },
      {
        ...child,
        id: parentProfileId,
        current: 1,
        versions: [
          { n: 1, saved_at: 1_700_000_100, hash: 'parent-v1', name: 'Parent' },
        ],
        body: { ...child.body, name: 'Parent', model: 'sonnet', extends: null },
      },
    ];
    const fresh = deferred<Preview>();
    let previewCalls = 0;
    const client = fakeClient({
      profiles: vi.fn(async () => structuredClone(savedProfiles)),
      saveProfile: vi.fn(async (id, body) => {
        savedProfiles = updateProfile(savedProfiles, id, body);
      }),
      preview: vi.fn(async () => {
        previewCalls++;
        return previewCalls === 1 ? preview('child-old') : fresh.promise;
      }),
    });
    await shown(client);
    await selectProfile(profileId);
    expect(previewCalls).toBe(1);

    button('Profiles').click();
    await settle();
    await editProfileInProfilesView('Parent', 'opus');
    await settle();
    expect(client.saveProfile).toHaveBeenCalledTimes(1);
    expect(previewCalls).toBe(2);

    button('Workspace').click();
    await settle();
    expect(button('Start Claude Code').disabled).toBe(true);
    expect(document.querySelector('.native-preview')).toBeNull();
    fresh.resolve(preview('child-new', 'Model: opus.'));
    await settle();
    expect(button('Start Claude Code').disabled).toBe(false);
    expect(text(document.querySelector('.native-preview'))).toContain(
      'Model: opus.',
    );
    expect(previewCalls).toBe(2);
  });

  it('ignores an older concurrent preview response and does not refetch an unchanged context', async () => {
    const older = deferred<Preview>();
    const newer = deferred<Preview>();
    let previewCalls = 0;
    const client = fakeClient({
      preview: vi.fn(async () => {
        previewCalls++;
        return previewCalls === 1 ? older.promise : newer.promise;
      }),
    });
    await shown(client);
    await selectProfile('template-review');
    expect(button('Start Claude Code').disabled).toBe(true);
    await selectProfile(profileId);
    expect(previewCalls).toBe(2);

    newer.resolve(preview('newer-token', 'Newest selected profile.'));
    await settle();
    expect(button('Start Claude Code').disabled).toBe(false);
    expect(text(document.querySelector('.native-preview'))).toContain(
      'Newest selected profile.',
    );
    older.resolve(preview('older-token', 'Stale earlier selection.'));
    await settle();
    expect(button('Start Claude Code').disabled).toBe(false);
    expect(text(document.querySelector('.native-preview'))).toContain(
      'Newest selected profile.',
    );
    expect(text(document.querySelector('.native-preview'))).not.toContain(
      'Stale earlier selection.',
    );
    expect(previewCalls).toBe(2);
  });

  it('retries a failed current preview only when requested and keeps Start gated until it succeeds', async () => {
    const recovered = deferred<Preview>();
    let previewCalls = 0;
    const client = fakeClient({
      preview: vi.fn(async () => {
        previewCalls++;
        if (previewCalls === 1) throw new Error('host unavailable');
        return recovered.promise;
      }),
    });
    await shown(client);
    await selectProfile(profileId);
    await settle();

    expect(previewCalls).toBe(1);
    expect(button('Start Claude Code').disabled).toBe(true);
    expect(button('Retry preview')).toBeDefined();
    await settle();
    expect(previewCalls).toBe(1);

    button('Retry preview').click();
    await settle();
    expect(previewCalls).toBe(2);
    expect(button('Start Claude Code').disabled).toBe(true);
    recovered.resolve(preview('recovered-token', 'Fresh after retry.'));
    await settle();
    expect(button('Start Claude Code').disabled).toBe(false);
    expect(text(document.querySelector('.native-preview'))).toContain(
      'Fresh after retry.',
    );
    await settle();
    expect(previewCalls).toBe(2);
  });

  it('says what went wrong in words, not a code', async () => {
    const { NativeCallError } =
      await import('../src/lib/desktop/native-client');
    const client = fakeClient({
      start: vi.fn(async () => {
        throw new NativeCallError({ code: 'limit_reached', reasons: [] });
      }),
    });
    await shown(client);
    button('Start Claude Code').click();
    await settle();
    expect(text(document.querySelector('[role="alert"]'))).toContain(
      'terminal limit (16)',
    );
  });
});

describe('the overview', () => {
  const sessions = [
    session({
      id: '1'.repeat(32),
      state: 'running',
      usage: { cpu_percent: 12.4, rss_bytes: 300 * 1024 * 1024, processes: 3 },
    }),
    session({
      id: '2'.repeat(32),
      state: 'running',
      agent_id: 'codex',
      agent_name: 'Codex',
    }),
    session({
      id: '3'.repeat(32),
      project: '/home/u/work/app',
      state: 'lost',
      terminal_id: null,
      pid: null,
    }),
    session({
      id: '4'.repeat(32),
      project: '/home/u/work/app',
      state: 'exited',
      exit_code: 3,
      terminal_id: null,
      pid: null,
      ended_at: 1_700_000_500,
    }),
    session({
      id: '5'.repeat(32),
      project: '/home/u/work/other',
      state: 'stopped',
      terminal_id: null,
      pid: null,
    }),
  ];
  const client = () => fakeClient({ sessions: vi.fn(async () => sessions) });

  it('lists every session with its plane, provider, process and resources, grouped by project', async () => {
    await shown(client());
    expect(text(document.querySelector('.native-heading'))).toContain(
      'Native plane',
    );
    const rows = Array.from(document.querySelectorAll('li.native-row')).map(
      (r) => text(r),
    );
    expect(rows).toHaveLength(5);
    expect(rows[0] ?? '').toContain('Claude Code');
    const joined = rows.join(' | ');
    expect(joined).toContain('process 4242');
    expect(joined).toContain('12% CPU · 300 MB · 3 processes');
    expect(joined).toContain('Active');
    expect(joined).toContain('Lost');
    expect(joined).toContain('Failed (3)');
    expect(joined).toContain('Stopped');
    expect(document.querySelectorAll('.native-group')).toHaveLength(3);
  });

  it('warns that two sessions on one folder can write the same files', async () => {
    await shown(client());
    expect(text(document.querySelector('.native-group'))).toContain(
      'can write the same files',
    );
  });

  it('explains a lost session instead of calling it finished', async () => {
    await shown(client());
    const lost = Array.from(document.querySelectorAll('li.native-row')).find(
      (r) => r.getAttribute('data-state') === 'lost',
    )!;
    expect(text(lost)).toContain('did not see how it ended');
    expect(text(lost)).not.toContain('Exited');
  });

  it('stops, restarts and removes through the desktop', async () => {
    const c = client();
    await shown(c);
    const stop = Array.from(
      document.querySelectorAll<HTMLButtonElement>('.native-actions button'),
    ).find((b) => text(b) === 'Stop')!;
    stop.click();
    await settle();
    expect(c.stop).toHaveBeenCalledWith('1'.repeat(32));
    const again = Array.from(
      document.querySelectorAll<HTMLButtonElement>('.native-actions button'),
    ).find((b) => text(b) === 'Start again')!;
    again.click();
    await settle();
    expect(c.restart).toHaveBeenCalled();
    const remove = Array.from(
      document.querySelectorAll<HTMLButtonElement>('.native-actions button'),
    ).find((b) => text(b) === 'Remove')!;
    remove.click();
    await settle();
    expect(c.forget).toHaveBeenCalled();
  });

  it('has nothing to say yet when there are no sessions, and says so', async () => {
    await shown(fakeClient());
    expect(text(document.querySelector('.sessions-empty'))).toContain(
      'No agent sessions yet',
    );
  });

  it('uses the reply of a start for the terminal tab', () => {
    expect(reply().terminal.shell).toBe('/usr/bin/fish');
  });
});

describe('the profiles view', () => {
  it('lists templates read only and a profile with its version', async () => {
    await shown(fakeClient(), { which: 'profiles' });
    const items = Array.from(
      document.querySelectorAll('.native-profile-item'),
    ).map((i) => text(i));
    expect(items[0]).toContain('Review, read only');
    expect(items[0]).toContain('Template');
    expect(items[2]).toContain('Version 2');
    (
      document.querySelectorAll('.native-profile-item')[0] as HTMLButtonElement
    ).click();
    await settle();
    expect(document.querySelector<HTMLInputElement>('#p-name')!.disabled).toBe(
      true,
    );
    expect(text(document.querySelector('.native-profile-editor'))).toContain(
      'Template',
    );
    expect(button('Save as a new version')).toBeUndefined();
    expect(button('Make a copy')).toBeDefined();
  });

  it('saves a new version of a profile and can bring an earlier one back', async () => {
    const client = fakeClient();
    await shown(client, { which: 'profiles' });
    (
      document.querySelectorAll('.native-profile-item')[2] as HTMLButtonElement
    ).click();
    await settle();
    const model = document.querySelector<HTMLInputElement>('#p-model')!;
    model.value = 'opus';
    model.dispatchEvent(new Event('input', { bubbles: true }));
    await settle();
    button('Save as a new version').click();
    await settle();
    expect(client.saveProfile).toHaveBeenCalledWith(
      'b'.repeat(32),
      expect.objectContaining({ name: 'Mine', model: 'opus' }),
    );
    button('Bring back').click();
    await settle();
    expect(client.restoreProfile).toHaveBeenCalledWith('b'.repeat(32), 1);
  });

  it('makes a new profile, with skills one per line', async () => {
    const client = fakeClient();
    await shown(client, { which: 'profiles' });
    button('New profile').click();
    await settle();
    const name = document.querySelector<HTMLInputElement>('#p-name')!;
    name.value = 'Quiet';
    name.dispatchEvent(new Event('input', { bubbles: true }));
    const skills = document.querySelector<HTMLTextAreaElement>('#p-skills')!;
    skills.value = 'noisy\n\n  other  ';
    skills.dispatchEvent(new Event('input', { bubbles: true }));
    await settle();
    button('Create').click();
    await settle();
    expect(client.createProfile).toHaveBeenCalledWith(
      expect.objectContaining({
        name: 'Quiet',
        skills_off: ['noisy', 'other'],
        applies_to: ['claude_code', 'codex'],
      }),
    );
  });

  it('shows a refusal of the desktop in words', async () => {
    const { NativeCallError } =
      await import('../src/lib/desktop/native-client');
    const client = fakeClient({
      createProfile: vi.fn(async () => {
        throw new NativeCallError({
          code: 'profile_notes_hold_secret',
          reasons: [],
        });
      }),
    });
    await shown(client, { which: 'profiles' });
    button('New profile').click();
    await settle();
    const name = document.querySelector<HTMLInputElement>('#p-name')!;
    name.value = 'X';
    name.dispatchEvent(new Event('input', { bubbles: true }));
    await settle();
    button('Create').click();
    await settle();
    expect(text(document.querySelector('[role="alert"]'))).toContain(
      'never holds one',
    );
  });
});

describe('a handoff', () => {
  it('prepares a brief to review, starts the other agent in the same folder with the edited text, and says it is not a transfer', async () => {
    const c = fakeClient({
      sessions: vi.fn(async () => [
        session({ id: '1'.repeat(32), project: '/home/u/app' }),
      ]),
      launcher: vi.fn(async () =>
        launcher({
          agents: [
            {
              agent_id: 'claude_code',
              name: 'Claude Code',
              executable: 'claude',
              installed: true,
              path: '/x',
              version: '1',
            },
            {
              agent_id: 'codex',
              name: 'Codex',
              executable: 'codex',
              installed: true,
              path: '/y',
              version: '2',
            },
          ],
        }),
      ),
    });
    const onconsole = vi.fn();
    await shown(c, { onconsole });
    button('Hand off…').click();
    await settle();
    expect(c.prepareHandoff).toHaveBeenCalledWith('1'.repeat(32));
    expect(text(document.querySelector('.native-handoff'))).toContain(
      'not a transfer',
    );
    const area = document.querySelector<HTMLTextAreaElement>('#handoff-text')!;
    expect(area.value.startsWith('# Handoff')).toBe(true);
    // The other agent is chosen by default.
    expect(
      document.querySelector<HTMLSelectElement>('#handoff-target')!.value,
    ).toBe('codex');
    area.value = '# Handoff from Claude Code\n\nFinish the parser.';
    area.dispatchEvent(new Event('input', { bubbles: true }));
    await settle();
    button('Start Codex with this brief').click();
    await settle();
    expect(c.start).toHaveBeenCalledWith(
      'codex',
      '/home/u/app',
      null,
      '# Handoff from Claude Code\n\nFinish the parser.',
      null,
    );
    expect(onconsole).toHaveBeenCalled();
  });

  it('will not start from a brief that lost its heading, and can save it to a file instead', async () => {
    const c = fakeClient({
      sessions: vi.fn(async () => [session()]),
      launcher: vi.fn(async () =>
        launcher({
          agents: [
            {
              agent_id: 'codex',
              name: 'Codex',
              executable: 'codex',
              installed: true,
              path: '/y',
              version: '2',
            },
          ],
        }),
      ),
    });
    await shown(c);
    button('Hand off…').click();
    await settle();
    const area = document.querySelector<HTMLTextAreaElement>('#handoff-text')!;
    area.value = 'no heading';
    area.dispatchEvent(new Event('input', { bubbles: true }));
    await settle();
    expect(button('Start Codex with this brief').disabled).toBe(true);
    area.value = '# ok';
    area.dispatchEvent(new Event('input', { bubbles: true }));
    await settle();
    button('Save to a file…').click();
    await settle();
    expect(c.saveHandoff).toHaveBeenCalledWith('# ok');
  });
});
