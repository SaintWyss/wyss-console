import { afterEach, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { flushSync, mount, tick, unmount } from 'svelte';
import Sidebar from '../src/lib/shell/Sidebar.svelte';
import {
  NAV_ITEMS,
  RAIL_ITEMS,
  moreItems,
  navItems,
  railOf,
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

it('offers seven entries on the rail, and every other view under More', () => {
  expect(RAIL_ITEMS.map((r) => r.label)).toEqual([
    'Console',
    'Workspace',
    'Arsenal',
    'Distributors',
    'Activity',
    'Settings',
    'More',
  ]);
  // No view is lost: what the rail does not open itself is under More, optional ones included.
  const all = navItems(true, true, true);
  const more = moreItems(all).map((i) => i.id);
  expect(more).toEqual([
    'projects',
    'workspace',
    'sessions',
    'agents',
    'runtimes',
    'pipelines',
    'remote',
    'approvals',
    'events',
    'configuration',
    'models',
    'providers',
    'profiles',
    'credentials',
    'settings',
  ]);
  for (const item of all)
    expect(
      more.includes(item.id) || RAIL_ITEMS.some((r) => r.view === item.id),
      item.id,
    ).toBe(true);
  // Without the optional views, More has no entry for them.
  expect(moreItems(NAV_ITEMS).map((i) => i.id)).not.toContain('runtimes');
  expect(moreItems(NAV_ITEMS).map((i) => i.id)).not.toContain('projects');
  // A view only reachable from More lights More.
  expect(railOf('sessions')).toBe('more');
  expect(railOf('providers')).toBe('distributors');
  expect(railOf('activity')).toBe('activity');
});

it('names every rail icon for screen readers and marks only the open view', async () => {
  component = mount(Sidebar, {
    target: document.body,
    props: {
      active: 'arsenal' as const,
      onselect: () => undefined,
      waiting: 2,
    },
  });
  flushSync();
  const rail = [...document.querySelectorAll('.rail-list .nav-item')];
  expect(rail.map((b) => b.getAttribute('title'))).toEqual(
    RAIL_ITEMS.map((r) => r.label),
  );
  expect(rail.map((b) => b.querySelector('.nav-label')?.textContent)).toEqual(
    RAIL_ITEMS.map((r) => r.label),
  );
  const current = [...document.querySelectorAll('[aria-current="page"]')];
  expect(current.map((b) => b.textContent?.trim())).toEqual(['Arsenal']);
  // What waits for a decision is counted on Activity, and on Approvals under More.
  expect(
    [...document.querySelectorAll('.nav-tag.waiting')].map((t) =>
      t.getAttribute('aria-label'),
    ),
  ).toEqual(['2 waiting', '2 waiting']);
  // More opens its menu and closes it.
  const menu = document.querySelector<HTMLElement>('.rail-menu')!;
  expect(menu.hidden).toBe(true);
  const more = document.querySelector<HTMLButtonElement>('.rail-more')!;
  expect(more.getAttribute('aria-controls')).toBe(menu.id);
  // A disclosure, not an ARIA menu: plain buttons in the tab order.
  expect(menu.getAttribute('role')).toBeNull();
  expect(menu.querySelector('[role="menuitem"]')).toBeNull();
  more.click();
  flushSync();
  await tick();
  expect(menu.hidden).toBe(false);
  expect(more.getAttribute('aria-expanded')).toBe('true');
  expect(document.activeElement).toBe(menu.querySelector('button'));
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
  flushSync();
  expect(menu.hidden).toBe(true);
  // Choosing a view from More sends the keyboard to the view, not to a hidden button.
  const main = document.createElement('main');
  main.id = 'main-content';
  main.tabIndex = -1;
  document.body.append(main);
  more.click();
  flushSync();
  await tick();
  menu.querySelector<HTMLButtonElement>('button')!.click();
  flushSync();
  await tick();
  expect(menu.hidden).toBe(true);
  expect(document.activeElement).toBe(main);
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
