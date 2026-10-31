import { afterEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import DelegationsPanel from '../src/lib/features/native/DelegationsPanel.svelte';
import type { Delegation } from '../src/lib/desktop/native-client';

let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  document.body.innerHTML = '';
});

const settle = async () => {
  for (let i = 0; i < 5; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
};

const delegation = (overrides: Partial<Delegation>): Delegation => ({
  id: 'a'.repeat(32),
  parent_session: '1234567890abcdef',
  parent_provider: 'claude_code',
  provider: 'codex',
  model: 'gpt-6.1-terra',
  effort: 'low',
  title: 'Write the helper',
  preset: 'Frontend preset',
  selection: 'Worker selection',
  workspace: '/s/delegations/a/workspace',
  created_at: 1,
  ended_at: null,
  timeout_minutes: 20,
  status: 'running',
  error: null,
  result: null,
  steps: 1,
  verification: { state: 'pending', problems: [], model: null },
  applied: { skills: [{ name: 'gsap-react', revision: 1 }], switched_off: 179 },
  artifacts: [],
  ...overrides,
});

describe('Delegated agents', () => {
  it('lists each agent with its provider, model, skills and check, and stops a running one', async () => {
    const read = vi.fn(async () => [
      delegation({}),
      delegation({
        id: 'b'.repeat(32),
        provider: 'claude_code',
        parent_provider: 'codex',
        model: 'haiku',
        status: 'succeeded',
        ended_at: 2,
        result: 'hello.py written',
        verification: {
          state: 'verified',
          problems: [],
          model: 'claude-haiku-5-5',
        },
        artifacts: [{ path: 'hello.py', size: 20 }],
      }),
    ]);
    const cancel = vi.fn(async () => undefined);
    component = mount(DelegationsPanel, {
      target: document.body,
      props: { visible: true, read, cancel },
    });
    await settle();
    const text = document.body.textContent?.replace(/\s+/g, ' ') ?? '';
    expect(text).toContain('Write the helper');
    expect(text).toContain('gpt-6.1-terra · low');
    expect(text).toContain('gsap-react r1');
    expect(text).toContain('179 installed skill(s) switched off');
    expect(text).toContain('configuration verified');
    expect(text).toContain('1 completed step(s)');
    expect(text).toContain('Preset: Frontend preset');
    expect(text).toContain('Selection: Worker selection');
    expect(text).toContain('Observed: claude-haiku-5-5');
    expect(text).toContain('effort not reported');
    expect(text).toContain('From Codex session 12345678');
    expect(text).toContain('hello.py');
    const labels = () =>
      [...document.querySelectorAll('button')].map((b) =>
        b.textContent?.trim(),
      );
    expect(labels()).toEqual(['Cancel']);
    // Stopping a running agent ends its work: it asks once more, and can be taken back.
    document.querySelector('button')!.click();
    await settle();
    expect(cancel).not.toHaveBeenCalled();
    expect(labels()).toEqual(['Stop agent', 'Keep running']);
    expect(document.body.textContent).toContain('Stop this agent?');
    [...document.querySelectorAll('button')][1]!.click();
    await settle();
    expect(labels()).toEqual(['Cancel']);
    expect(cancel).not.toHaveBeenCalled();
    document.querySelector('button')!.click();
    await settle();
    [...document.querySelectorAll('button')][0]!.click();
    await settle();
    expect(cancel).toHaveBeenCalledWith('a'.repeat(32));
  });

  it('keeps each parent’s agents together and counts them by state', async () => {
    const read = vi.fn(async () => [
      delegation({ id: '1'.repeat(32), title: 'First of A' }),
      delegation({
        id: '2'.repeat(32),
        parent_session: 'bbbbbbbbbbbbbbbb',
        parent_provider: 'codex',
        title: 'Only of B',
        status: 'failed',
        ended_at: 2,
      }),
      delegation({
        id: '3'.repeat(32),
        title: 'Second of A',
        status: 'succeeded',
        ended_at: 2,
      }),
    ]);
    component = mount(DelegationsPanel, {
      target: document.body,
      props: { visible: true, read, cancel: vi.fn() },
    });
    await settle();
    const groups = [...document.querySelectorAll('.delegation-group')].map(
      (g) => g.textContent?.replace(/\s+/g, ' ') ?? '',
    );
    expect(groups).toHaveLength(2);
    expect(groups[0]).toContain('From Claude Code session 12345678');
    expect(groups[0]).toContain('2 agents');
    expect(groups[0]!.indexOf('First of A')).toBeLessThan(
      groups[0]!.indexOf('Second of A'),
    );
    expect(groups[1]).toContain('From Codex session bbbbbbbb');
    const tally = document
      .querySelector('.delegations-tally')
      ?.textContent?.replace(/\s+/g, ' ');
    expect(tally).toContain('1 running');
    expect(tally).toContain('1 done');
    expect(tally).toContain('1 failed');
    expect(tally).not.toContain('cancelled');
    // Every state has its word and a tone, never the tone alone.
    for (const item of document.querySelectorAll('.delegation')) {
      expect(item.getAttribute('data-tone')).toMatch(/^(pending|ok|error)$/);
      expect(item.querySelector('.badge')?.textContent?.trim()).toMatch(
        /Running|Done|Failed/,
      );
    }
  });

  it('says when the host cannot list them', async () => {
    component = mount(DelegationsPanel, {
      target: document.body,
      props: {
        visible: true,
        read: vi.fn(async () => {
          throw new Error('down');
        }),
      },
    });
    await settle();
    expect(document.querySelector('[role="alert"]')?.textContent).toContain(
      'did not list',
    );
  });
});
