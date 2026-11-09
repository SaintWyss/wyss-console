import { afterEach, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { flushSync, mount, unmount } from 'svelte';
import Sidebar from '../src/lib/shell/Sidebar.svelte';
import { pendingAuthority } from '../src/lib/shell/authority';
import { pendingLifecycle } from '../src/lib/shell/lifecycle';
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
import {
  delegationMark,
  delegationTally,
} from '../src/lib/features/native/delegations';
import type { Delegation } from '../src/lib/desktop/native-client';

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
      runtime: { label: 'Running', tone: 'ok' as const },
      authority: pendingAuthority,
      lifecycle: pendingLifecycle,
      f8: null,
    },
  });
  flushSync();
  const groups = [...document.querySelectorAll('.nav-group[role="group"]')];
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

it('counts delegated agents by their own status, never folding one into another', () => {
  const d = (status: string) => ({ status }) as Delegation;
  const tally = delegationTally([
    d('running'),
    d('failed'),
    d('timed_out'),
    d('interrupted'),
    d('cancelled'),
    d('succeeded'),
    d('something_new'),
  ]).map((entry) => `${entry.count} ${entry.label}`);
  expect(tally).toEqual([
    '1 running',
    '1 done',
    '1 failed',
    '1 timed out',
    '1 interrupted',
    '1 cancelled',
    '1 other',
  ]);
  expect(delegationTally([d('cancelled'), d('cancelled')])).toHaveLength(1);
});

it('never stretches a checkbox or radio across its row in Arsenal forms', () => {
  // A regression seen in the installed app: `width: 100%` on every input of the selection
  // editor made each checkbox a full row and pushed the capability name to the far edge.
  const source = readFileSync(
    resolve(process.cwd(), 'src/lib/features/arsenal/ArsenalView.svelte'),
    'utf8',
  );
  const style = source.slice(source.indexOf('<style>'));
  for (const rule of style.matchAll(/([^{}]+)\{[^}]*\bwidth:\s*100%/g))
    for (const selector of rule[1]!.split(/,(?![^(]*\))/))
      if (/\binput\b/.test(selector))
        expect(selector, selector.trim()).toMatch(
          /:not\(\[type='checkbox'\], \[type='radio'\]\)/,
        );
});
