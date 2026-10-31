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
    expect(text).toContain('From Codex session 12345678');
    expect(text).toContain('hello.py');
    const buttons = [...document.querySelectorAll('button')];
    expect(buttons.map((b) => b.textContent?.trim())).toEqual(['Cancel']);
    buttons[0].click();
    await settle();
    expect(cancel).toHaveBeenCalledWith('a'.repeat(32));
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
