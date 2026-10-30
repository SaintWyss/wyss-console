import { afterEach, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import TopBar from '../src/lib/shell/TopBar.svelte';
import { pendingAuthority } from '../src/lib/shell/authority';
import { pendingLifecycle } from '../src/lib/shell/lifecycle';
import { linkText } from '../src/lib/features/f8/present';

let component: ReturnType<typeof mount> | undefined;

afterEach(() => {
  if (component) void unmount(component);
  component = undefined;
  document.body.replaceChildren();
});

const base = {
  title: 'Arsenal',
  subtitle: '',
  runtime: { label: 'Running', tone: 'ok' as const },
  authority: pendingAuthority,
  lifecycle: pendingLifecycle,
  version: '0.2.0',
};

const authorityPill = () => document.querySelector('.pill.authority');

it('shows the protocol-2 link when F8 is active, not the unused protocol 1', () => {
  component = mount(TopBar, {
    target: document.body,
    props: {
      ...base,
      f8: linkText({ state: 'connected', fault: null, retry_in_ms: null }),
    },
  });
  flushSync();
  const pill = authorityPill();
  expect(pill?.querySelector('.pill-key')?.textContent).toBe('Authority P2');
  expect(pill?.querySelector('.pill-value')?.textContent).toBe('Connected');
  expect(pill?.getAttribute('data-tone')).toBe('ok');
  expect(pill?.getAttribute('title')).toMatch(
    /^Protocol 2 authority · Connected/,
  );
  expect(document.body.textContent).not.toContain('Authority P1');
});

it('shows the real waiting state of F8 instead of a protocol 1 Offline', () => {
  component = mount(TopBar, {
    target: document.body,
    props: {
      ...base,
      f8: linkText({ state: 'waiting', fault: 'lost', retry_in_ms: 1000 }),
    },
  });
  flushSync();
  const pill = authorityPill();
  expect(pill?.querySelector('.pill-key')?.textContent).toBe('Authority P2');
  expect(pill?.querySelector('.pill-value')?.textContent).toBe('Offline');
  expect(pill?.getAttribute('data-tone')).toBe('warn');
});

it('keeps the protocol 1 pill when F8 is not active', () => {
  component = mount(TopBar, {
    target: document.body,
    props: { ...base, f8: null },
  });
  flushSync();
  const pill = authorityPill();
  expect(pill?.querySelector('.pill-key')?.textContent).toBe('Authority P1');
  expect(pill?.getAttribute('title')).toMatch(/^Protocol 1 authority · /);
});
