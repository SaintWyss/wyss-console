import { afterEach, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import Sidebar from '../src/lib/shell/Sidebar.svelte';
import {
  NAV_ITEMS,
  navItems,
  navSections,
  type ViewId,
} from '../src/lib/shell/nav';
import {
  candidateMark,
  compatibilityMark,
} from '../src/lib/features/arsenal/catalogue';
import type {
  CandidateStatus,
  Compatibility,
} from '../src/lib/contracts/arsenal';
import { delegationMark } from '../src/lib/features/native/delegations';

let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  document.body.innerHTML = '';
});

it('groups the sidebar without reordering it, every optional item included', () => {
  const all = navItems(true, true, true);
  const sections = navSections(all);
  // The same order as the flat list: grouping never moves an item.
  expect(sections.flatMap((s) => s.items.map((i) => i.id))).toEqual(
    all.map((i) => i.id),
  );
  // Each group appears once: its items are consecutive.
  expect(sections.map((s) => s.label)).toEqual([
    'Terminals',
    'Agents & activity',
    'Capabilities',
    'System',
  ]);
  // Providers, Runtimes and Projects are optional: the base set has 2 + 6 + 4 + 2.
  expect(navSections(NAV_ITEMS).map((s) => s.items.length)).toEqual([
    2, 6, 4, 2,
  ]);
});

it('renders each group as a labelled group and marks only the open view', () => {
  component = mount(Sidebar, {
    target: document.body,
    props: {
      active: 'arsenal' as ViewId,
      onselect: () => undefined,
      waiting: 2,
    },
  });
  flushSync();
  const groups = [...document.querySelectorAll('[role="group"]')];
  expect(groups).toHaveLength(4);
  for (const group of groups) {
    const label = document.getElementById(
      group.getAttribute('aria-labelledby')!,
    );
    expect(label?.textContent?.trim().length).toBeGreaterThan(0);
  }
  const current = [...document.querySelectorAll('[aria-current="page"]')];
  expect(current.map((b) => b.textContent?.trim())).toEqual(['Arsenal']);
  // What waits for a decision is counted on its item.
  expect(
    document.querySelector('.nav-tag.waiting')?.getAttribute('aria-label'),
  ).toBe('2 waiting');
});

it('draws every compatibility, discovery and delegation state with a symbol, not color alone', () => {
  const compatibility: Compatibility[] = [
    'supported',
    'unsupported',
    'unverified',
    'requires_qualification',
    'conflicting',
    'missing_dependency',
  ];
  const statuses: CandidateStatus[] = [
    'not_imported',
    'imported',
    'source_changed',
    'read_only',
  ];
  const marks = [
    ...compatibility.map(compatibilityMark),
    ...statuses.map(candidateMark),
    ...[
      'running',
      'succeeded',
      'failed',
      'cancelled',
      'timed_out',
      'interrupted',
    ].map(delegationMark),
  ];
  for (const mark of marks) {
    expect(mark.symbol.trim()).not.toBe('');
    expect(['ok', 'pending', 'warn', 'error', 'neutral']).toContain(mark.tone);
  }
  // Tones that mean different things never share a symbol.
  const bySymbol = new Map<string, Set<string>>();
  for (const { tone, symbol } of marks)
    bySymbol.set(symbol, (bySymbol.get(symbol) ?? new Set()).add(tone));
  for (const [symbol, tones] of bySymbol)
    expect([...tones], `symbol ${symbol}`).toHaveLength(1);
  expect(compatibilityMark('supported').tone).toBe('ok');
  expect(compatibilityMark('unsupported').tone).toBe('error');
  expect(delegationMark('timed_out').tone).toBe('error');
});
