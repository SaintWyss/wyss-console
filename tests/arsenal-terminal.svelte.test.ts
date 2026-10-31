import { afterEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import ArsenalTerminalPanel from '../src/lib/features/arsenal/ArsenalTerminalPanel.svelte';
import {
  sessionArsenalLine,
  type ArsenalChoice,
} from '../src/lib/features/arsenal/terminal';
import type { ArsenalClient } from '../src/lib/desktop/arsenal-client';
import type {
  Binding,
  Selection,
  TerminalPlan,
} from '../src/lib/contracts/arsenal';
import type { Preview } from '../src/lib/contracts/native';

let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  document.body.innerHTML = '';
});

const text = () => document.body.textContent?.replace(/\s+/g, ' ') ?? '';
const settle = async () => {
  for (let i = 0; i < 5; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
};

const selection: Selection = {
  id: 'sel-1',
  name: 'Frontend',
  provider: 'claude_code',
  items: [{ capability: 'cap-design', revision: 2 }],
  composites: [],
  created_at: 1,
  updated_at: 1,
};

function plan(overrides: Partial<TerminalPlan> = {}): TerminalPlan {
  return {
    selection: 'sel-1',
    selection_name: 'Frontend',
    provider: 'claude_code',
    provider_version: '2.1.314 (Claude Code)',
    project: 'app',
    status: 'ready',
    can_apply: false,
    enforcement: {
      state: 'unqualified',
      reason: 'Skill exclusion is not qualified for Claude Code 2.1.314.',
      evidence: null,
    },
    items: [
      {
        capability: 'cap-design',
        name: 'design',
        kind: 'skill',
        provider: 'claude_code',
        revision: 2,
        pinned: true,
        tree_hash: 'c'.repeat(64),
        state: 'prepared',
        reason: 'The installed copy is exactly revision 2. It stays exposed.',
        compatibility: { status: 'supported', reason: 'own format' },
        dependencies: [],
      },
    ],
    kept: ['design'],
    excluded: ['tokens', 'gsd-help'],
    inherited_off: [],
    excluded_files: [],
    untouched: ['Hooks, permissions and MCP servers: as your settings say.'],
    composites: [],
    conflicts: [],
    warnings: [],
    overlay: '{"skillOverrides":{"gsd-help":"off","tokens":"off"}}',
    config_hash: 'e'.repeat(64),
    ...overrides,
  };
}

const preview = (can_start: boolean): Preview => ({
  applied: ['Arsenal selection "Frontend": 2 installed skill(s) switched off.'],
  with_cost: [],
  not_applicable: can_start ? [] : ['Skill exclusion is not qualified.'],
  can_start,
  through_shell: true,
  warnings: [],
  plan_token: 'token',
  provider_version: '2.1.314 (Claude Code)',
});

function client(next: TerminalPlan, remembered: string | null = null) {
  return {
    selections: vi.fn(async () => [selection]),
    terminalPlan: vi.fn(async () => next),
    projectSelection: vi.fn(async () => remembered),
  } as unknown as ArsenalClient;
}

function render(
  fake: ArsenalClient,
  previewer = vi.fn(async () => preview(true)),
) {
  const choices: (ArsenalChoice | null)[] = [];
  component = mount(ArsenalTerminalPanel, {
    target: document.body,
    props: {
      agent: 'claude_code',
      project: '/home/u/app',
      profile: null,
      previewer,
      onchange: (choice: ArsenalChoice | null) => choices.push(choice),
      client: fake,
    },
  });
  flushSync();
  return { choices, previewer, last: () => choices[choices.length - 1] };
}

function choose(id: string) {
  document
    .querySelector<HTMLInputElement>(`#native-arsenal input[value="${id}"]`)!
    .click();
  flushSync();
}

describe('Arsenal in a terminal start', () => {
  it('restores the project selection, says NOT APPLIED on an unqualified build, and lets Start go only when recording is chosen', async () => {
    const fake = client(plan(), 'sel-1');
    const { last, previewer } = render(fake);
    await settle();
    expect(fake.projectSelection).toHaveBeenCalledWith(
      '/home/u/app',
      'claude_code',
    );
    expect(fake.terminalPlan).toHaveBeenCalledWith(
      'sel-1',
      'claude_code',
      '/home/u/app',
    );
    expect(text()).toContain('Not qualified for this build');
    expect(text()).toContain('NOT APPLIED');
    expect(text()).toContain('2 would be switched off');
    expect(previewer).not.toHaveBeenCalled();
    let choice = last();
    expect(choice && !choice.pending && choice.ready).toBe(false);
    const record = [
      ...document.querySelectorAll<HTMLInputElement>('input[type=checkbox]'),
    ].find((box) => box.parentElement?.textContent?.includes('record'))!;
    record.click();
    flushSync();
    choice = last();
    expect(choice && !choice.pending && choice.ready).toBe(true);
    expect(choice && !choice.pending && choice.apply).toBe(false);
  });

  it('on a qualified build, previews the launch with exactly the plan it shows and is ready only when the preview can start', async () => {
    const applicable = plan({
      can_apply: true,
      enforcement: {
        state: 'available',
        reason: 'Skill exclusion is qualified.',
        evidence: {
          qualified_version: '2.1.314 (Claude Code)',
          checked_on: 'd',
        },
      },
    });
    const { last, previewer } = render(client(applicable));
    await settle();
    choose('sel-1');
    await settle();
    expect(previewer).toHaveBeenCalledWith({
      selection: 'sel-1',
      config_hash: 'e'.repeat(64),
      project: '/home/u/app',
    });
    const choice = last();
    expect(choice && !choice.pending && choice.apply).toBe(true);
    expect(choice && !choice.pending && choice.ready).toBe(true);
    expect(text()).not.toContain('NOT APPLIED');
  });

  it('is not ready while a refused launch preview stands', async () => {
    const applicable = plan({
      can_apply: true,
      enforcement: { state: 'available', reason: 'ok', evidence: null },
    });
    const { last } = render(
      client(applicable, 'sel-1'),
      vi.fn(async () => preview(false)),
    );
    await settle();
    const choice = last();
    expect(choice && !choice.pending && choice.ready).toBe(false);
    expect(text()).toContain('The launch preview refuses this combination');
  });

  it('computes the plan again when the view asks, so a changed build is never shown as the old plan', async () => {
    const fake = client(plan(), 'sel-1');
    const choices: (ArsenalChoice | null)[] = [];
    const props = $state({
      agent: 'claude_code' as const,
      project: '/home/u/app',
      profile: null,
      refreshKey: 0,
      previewer: vi.fn(async () => preview(true)),
      onchange: (choice: ArsenalChoice | null) => choices.push(choice),
      client: fake,
    });
    component = mount(ArsenalTerminalPanel, { target: document.body, props });
    await settle();
    expect(fake.terminalPlan).toHaveBeenCalledTimes(1);
    props.refreshKey = 1;
    await settle();
    expect(fake.terminalPlan).toHaveBeenCalledTimes(2);
  });

  it('offers a selection saved meanwhile when the view is shown again, without a Reload', async () => {
    const fake = client(plan());
    const later = { ...selection, id: 'sel-2', name: 'Later' };
    const props = $state({
      agent: 'claude_code' as const,
      project: '/home/u/app',
      profile: null,
      refreshKey: 0,
      previewer: vi.fn(async () => preview(true)),
      onchange: () => {},
      client: fake,
    });
    component = mount(ArsenalTerminalPanel, { target: document.body, props });
    await settle();
    expect(text()).not.toContain('Later');
    vi.mocked(fake.selections).mockResolvedValue([selection, later]);
    props.refreshKey = 1;
    await settle();
    const options = [
      ...document.querySelectorAll<HTMLLabelElement>('#native-arsenal label'),
    ].map((option) => option.textContent?.trim());
    expect(options).toContain('Later');
  });

  it('counts a kept skill its settings switch off as off, and names the Codex flag it adds', async () => {
    const codex = plan({
      provider: 'codex',
      provider_version: 'codex-cli 0.159.0',
      kept: ['design', 'old'],
      inherited_off: ['old'],
      excluded: ['tokens'],
      excluded_files: ['/h/.agents/skills/tokens/SKILL.md'],
      overlay:
        'skills.config=[{ path = "/h/.agents/skills/tokens/SKILL.md", enabled = false }]',
    });
    const fake = {
      selections: vi.fn(async () => [
        { ...selection, provider: 'codex' as const },
      ]),
      terminalPlan: vi.fn(async () => codex),
      projectSelection: vi.fn(async () => 'sel-1'),
    } as unknown as ArsenalClient;
    component = mount(ArsenalTerminalPanel, {
      target: document.body,
      props: {
        agent: 'codex',
        project: '/home/u/app',
        profile: null,
        previewer: vi.fn(async () => preview(true)),
        onchange: () => {},
        client: fake,
      },
    });
    await settle();
    expect(text()).toContain(
      '1 installed skill(s) stay · 1 already off in your settings',
    );
    expect(text()).toContain('Session flag it would add (with --no-daemon)');
  });

  it('says a session line from the host record first, and never says a skill was used', () => {
    const binding: Binding = {
      session_id: 's1',
      selection_id: 'sel-1',
      selection_name: 'Frontend',
      provider: 'claude_code',
      provider_version: null,
      project: '/p',
      config_hash: 'e'.repeat(64),
      state: 'not_applied',
      reason: 'unqualified',
      plan: null,
      created_at: 1,
      verification: 'not_observed',
    };
    expect(sessionArsenalLine({ id: 's1', arsenal: null }, [binding])).toBe(
      'Arsenal: Frontend · NOT APPLIED',
    );
    expect(
      sessionArsenalLine(
        {
          id: 's2',
          arsenal: {
            selection: 'sel-1',
            selection_name: 'Frontend',
            config_hash: 'e'.repeat(64),
            skills_off: 3,
          },
        },
        [],
      ),
    ).toContain('applied · 3 installed skill(s) off · use not observed');
    expect(sessionArsenalLine({ id: 's3', arsenal: null }, [binding])).toBe(
      null,
    );
  });
});
