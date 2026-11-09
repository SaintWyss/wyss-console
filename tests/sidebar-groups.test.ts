import { afterEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import Sidebar from '../src/lib/shell/Sidebar.svelte';
import { pendingAuthority } from '../src/lib/shell/authority';
import { pendingLifecycle } from '../src/lib/shell/lifecycle';
import type { NavGroup, ViewId } from '../src/lib/shell/nav';

let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) void unmount(component);
  component = undefined;
  document.body.replaceChildren();
});

function open(closed: NavGroup[], ontogglegroup = vi.fn()) {
  const onselect = vi.fn();
  component = mount(Sidebar, {
    target: document.body,
    props: {
      active: 'console' as ViewId,
      onselect,
      closed,
      ontogglegroup,
      runtime: { label: 'Running', tone: 'ok' as const },
      authority: pendingAuthority,
      lifecycle: pendingLifecycle,
      f8: null,
    },
  });
  flushSync();
  return { onselect, ontogglegroup };
}
const group = (name: NavGroup) =>
  document.querySelector<HTMLElement>(
    `.nav-group[aria-labelledby="nav-${name}"]`,
  )!;

it('a folded group is closed, and shows as closed even with the active view inside it', () => {
  open(['terminals']);
  expect(group('terminals').dataset.open).toBe('false');
  expect(group('agents').dataset.open).toBe('true');
});

it('folding a group by click holds it folded under the pointer until the pointer leaves', () => {
  const { ontogglegroup } = open([]);
  const header =
    group('agents').querySelector<HTMLButtonElement>('.nav-section')!;
  header.click();
  flushSync();
  expect(ontogglegroup).toHaveBeenCalledWith('agents');
  expect(group('agents').dataset.folded).toBe('true');
  group('agents').dispatchEvent(new MouseEvent('mouseleave'));
  flushSync();
  expect(group('agents').dataset.folded).toBe('false');
});

it('choosing an item of a folded group pins that group open', () => {
  const { onselect, ontogglegroup } = open(['agents']);
  group('agents').querySelector<HTMLButtonElement>('.nav-item')!.click();
  expect(ontogglegroup).toHaveBeenCalledWith('agents');
  expect(onselect).toHaveBeenCalledWith('sessions');
});
