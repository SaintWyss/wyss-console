import { afterEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import TopBar from '../src/lib/shell/TopBar.svelte';

let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) void unmount(component);
  component = undefined;
  document.body.replaceChildren();
});

const base = {
  title: 'Console',
  subtitle: 'Local terminal workspace',
  version: '0.2.0',
};
const buttons = () => [
  ...document.querySelectorAll<HTMLButtonElement>('.top-actions button'),
];

it('puts the title and its subtitle on one line, with no visible product name or version', () => {
  component = mount(TopBar, { target: document.body, props: base });
  flushSync();
  expect(document.querySelector('.context-title')?.textContent).toBe('Console');
  expect(document.querySelector('.context-subtitle')?.textContent).toBe(
    'Local terminal workspace',
  );
  // Read out, not drawn.
  expect(document.querySelector('.visually-hidden')?.textContent).toContain(
    'Wyss Console',
  );
  expect(document.querySelector('.visually-hidden')?.textContent).toContain(
    'v0.2.0',
  );
  expect(document.querySelector('.top-actions')).toBeNull();
});

it('shows the two splits, the shortcuts and Launch, in that order, on the Console', () => {
  const onsplit = vi.fn();
  const ontogglelegend = vi.fn();
  const onlaunch = vi.fn();
  component = mount(TopBar, {
    target: document.body,
    props: {
      ...base,
      tools: true,
      canSplit: true,
      onsplit,
      ontogglelegend,
      onlaunch,
    },
  });
  flushSync();
  expect(
    buttons().map((b) => b.getAttribute('aria-label') ?? b.textContent),
  ).toEqual([
    'Split side by side',
    'Split one above the other',
    'Keyboard shortcuts',
    'Launch ▾',
  ]);
  buttons().forEach((b) => b.click());
  expect(onsplit.mock.calls).toEqual([['row'], ['col']]);
  expect(ontogglelegend).toHaveBeenCalledOnce();
  expect(onlaunch).toHaveBeenCalledOnce();
});

it('disables the splits while no terminal is open', () => {
  component = mount(TopBar, {
    target: document.body,
    props: { ...base, tools: true, canSplit: false },
  });
  flushSync();
  expect(buttons().map((b) => b.disabled)).toEqual([true, true, false, false]);
});
