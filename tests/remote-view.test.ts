import { afterEach, describe, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import RemoteView from '../src/lib/features/remote/RemoteView.svelte';

let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  document.body.innerHTML = '';
});

const render = (visible = true) => {
  component = mount(RemoteView, { target: document.body, props: { visible } });
  flushSync();
};
const text = () => document.body.textContent?.replace(/\s+/g, ' ').trim() ?? '';

describe('remote PC client gate', () => {
  it('keeps the unavailable transport state visible and effectful controls disabled', () => {
    render();
    expect(text()).toContain('OD01 transport choice is open');
    expect(text()).toContain('No remote host is enrolled');
    expect(
      Array.from(document.querySelectorAll('button'))
        .filter((button) =>
          ['Connect to a host', 'Begin pairing'].includes(
            button.textContent?.trim() ?? '',
          ),
        )
        .every((button) => (button as HTMLButtonElement).disabled),
    ).toBe(true);
  });

  it('shows the no-replay contract for terminal input and keeps absent-host input blocked', () => {
    render();
    const terminal = Array.from(document.querySelectorAll('button')).find(
      (button) => button.textContent?.trim() === 'Terminal',
    )!;
    terminal.click();
    flushSync();
    expect(text()).toContain('OUTCOME_UNKNOWN');
    expect(text()).toContain('never replayed');
    expect(text()).toContain('Locked-host full-account input remains blocked');
    expect(
      Array.from(document.querySelectorAll('button'))
        .filter((button) =>
          ['Attach read-only terminal', 'Claim terminal input'].includes(
            button.textContent?.trim() ?? '',
          ),
        )
        .every((button) => (button as HTMLButtonElement).disabled),
    ).toBe(true);
  });

  it('hides the remote screen when it is not active', () => {
    render(false);
    expect(
      document.querySelector('section.remote')?.hasAttribute('hidden'),
    ).toBe(true);
  });
});
