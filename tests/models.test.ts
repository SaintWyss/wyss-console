/* eslint-disable @typescript-eslint/no-explicit-any -- a catalogue is damaged on purpose */
import { afterEach, describe, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import type { ConfigItem } from '../src/lib/contracts/configuration';
import { parseModelCatalogue } from '../src/lib/contracts/models';
import { parseSessionPage } from '../src/lib/contracts/sessions';
import { CatalogueCallError } from '../src/lib/desktop/models-client';
import {
  ModelsController,
  type ModelsState,
} from '../src/lib/features/models/controller';
import {
  CLAUDE_EFFORTS,
  completeness,
  configuredRows,
  entryRow,
  listedEfforts,
  listedModels,
  observedRows,
  provenance,
  selectedRows,
} from '../src/lib/features/models/present';
import ModelsHarness from './ModelsHarness.svelte';
import catalogues from './fixtures/model-catalogue-v1.json';
import pages from './fixtures/session-page-v4.json';
import { profiles } from './native-harness';

let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
});
const settle = async () => {
  for (let i = 0; i < 30; i++) await Promise.resolve();
  flushSync();
};
const text = (node: Element | null | undefined) =>
  node?.textContent?.replace(/\s+/g, ' ').trim() ?? '';
const damaged = (value: unknown) => structuredClone(value) as any;

describe('the catalogue contract', () => {
  it('reads the three shapes the desktop sends, exactly', () => {
    const listed = parseModelCatalogue(catalogues.listed);
    expect(listed.entries.map((e) => e.model_id)).toEqual([
      'model-a',
      'model-b',
    ]);
    expect(listed.entries[0]?.is_default).toBe(true);
    expect(
      parseModelCatalogue(catalogues.partial_without_offers),
    ).toMatchObject({ complete: false });
    expect(parseModelCatalogue(catalogues.not_ready).entries).toEqual([]);
  });
  it('refuses what is not exactly that shape or claims more than a runtime said', () => {
    const break_ = (
      mutate: (c: any) => void,
      from: unknown = catalogues.listed,
    ) => {
      const c = damaged(from);
      mutate(c);
      expect(() => parseModelCatalogue(c)).toThrow();
    };
    break_((c) => (c.extra = 1));
    break_((c) => delete c.read_at);
    break_((c) => (c.schema_version = 2));
    break_((c) => (c.state = 'ready'));
    break_((c) => (c.mode = 'full_access'));
    break_((c) => (c.complete = 'yes'));
    break_((c) => (c.read_at = -1));
    break_((c) => (c.read_at = 1.5));
    break_((c) => (c.entries[0].extra = 1));
    break_((c) => (c.entries[0].model_id = ''));
    break_((c) => (c.entries[0].model_id = 'a b'));
    break_((c) => (c.entries[0].model_id = 'x'.repeat(257)));
    break_((c) => (c.entries[0].efforts = ['low', 'low']));
    break_((c) => (c.entries[0].efforts = Array(17).fill('x')));
    break_((c) => (c.entries[0].efforts = ['a\nb']));
    break_((c) => (c.entries[0].is_default = 'true'));
    break_((c) => (c.entries[0].replaced_by = 'two words'));
    break_((c) => (c.entries[0].display_name = 'line\nbreak'));
    break_((c) => (c.entries = Array(101).fill(c.entries[0])));
    // A runtime that is not ready lists nothing and says nothing about a list.
    break_(
      (c) => (c.entries = catalogues.listed.entries),
      catalogues.not_ready,
    );
    break_((c) => (c.complete = true), catalogues.not_ready);
  });
});

describe('what is said about a catalogue', () => {
  it('says where it came from and when, or why there is none', () => {
    const listed = parseModelCatalogue(catalogues.listed);
    expect(provenance(listed)).toContain(
      'Listed once by the managed Codex runtime',
    );
    expect(provenance(listed)).toContain('0.159.0');
    expect(provenance(listed)).toContain('Inference');
    expect(
      provenance(parseModelCatalogue(catalogues.partial_without_offers)),
    ).toContain('Local Tools');
    expect(provenance(parseModelCatalogue(catalogues.not_ready))).toContain(
      'never shown',
    );
    expect(completeness(listed)).toBeNull();
    expect(
      completeness(parseModelCatalogue(catalogues.partial_without_offers)),
    ).toContain('partial');
    const unknown = damaged(catalogues.listed);
    unknown.complete = null;
    expect(completeness(parseModelCatalogue(unknown))).toContain(
      'not reported',
    );
    expect(completeness(parseModelCatalogue(catalogues.not_ready))).toBeNull();
  });
  it('marks the default as the runtime’s, shows efforts and a replacement as marked, and claims nothing else', () => {
    const [a, b] = parseModelCatalogue(catalogues.listed).entries.map(entryRow);
    expect(a?.isDefault).toBe(true);
    expect(a?.efforts).toBe('low · medium (default) · high');
    expect(a?.title).toBe('Model A · model-a');
    expect(b?.isDefault).toBe(false);
    expect(b?.provider).toBe('custom');
    expect(b?.replacement).toBe(
      'The runtime marks it for replacement by model-a, retiring 2027-01-31T00:00:00Z.',
    );
    // An authority that reported no offer says nothing, which is not "not the default".
    const bare = entryRow(
      parseModelCatalogue(catalogues.partial_without_offers).entries[0]!,
    );
    expect(bare.isDefault).toBe(false);
    expect(bare.efforts).toBeNull();
    expect(bare.replacement).toBeNull();
    const none = damaged(catalogues.listed);
    none.entries[0].efforts = [];
    expect(entryRow(parseModelCatalogue(none).entries[0]!).efforts).toBe(
      'none listed',
    );
  });
  it('offers only what the runtime listed, and only while it is listing', () => {
    const listed = parseModelCatalogue(catalogues.listed);
    expect(listedModels(listed)).toEqual(['model-a', 'model-b']);
    expect(listedEfforts(listed)).toEqual(['low', 'medium', 'high']);
    const stopped = parseModelCatalogue(catalogues.not_ready);
    expect(listedModels(stopped)).toEqual([]);
    expect(listedEfforts(stopped)).toEqual([]);
    expect(listedModels(null)).toEqual([]);
    expect(CLAUDE_EFFORTS).toEqual(['low', 'medium', 'high', 'xhigh', 'max']);
  });
});

const setting = (
  key: string,
  value: string,
  provider: string,
  over: Partial<ConfigItem> = {},
): ConfigItem =>
  ({
    id: `${provider}${key}`.padEnd(32, '0'),
    provider,
    source: provider === 'codex' ? 'codex_user_config' : 'claude_user_settings',
    scope: 'global',
    kind: 'model_preference',
    name: key,
    persisted: 'present_enabled',
    effective: 'documented',
    session_use: 'unknown',
    not_writable: 'not_in_this_phase',
    detail: { kind: 'text', value: { text: value, found: [] } },
    hash: null,
    ...over,
  }) as ConfigItem;

describe('configured, selected and observed are three lists', () => {
  it('shows what the files say, with where and at which scope, and only model settings', () => {
    const rows = configuredRows([
      setting('model', 'opus', 'claude_code'),
      setting('effortLevel', 'high', 'claude_code'),
      setting('model_reasoning_effort', 'low', 'codex'),
      setting('hooks', 'x', 'claude_code', { kind: 'hook' } as never),
    ]);
    expect(rows.map((r) => [r.providerName, r.key, r.value])).toEqual([
      ['Claude Code', 'model', 'opus'],
      ['Claude Code', 'effortLevel', 'high'],
      ['Codex', 'model_reasoning_effort', 'low'],
    ]);
    expect(rows[0]?.source).toBe('User settings');
    expect(rows[0]?.scope).toBe('Your account');
    expect(configuredRows(null)).toEqual([]);
    // A detail that is not plain text is not guessed at.
    expect(
      configuredRows([setting('model', 'x', 'codex', { detail: null })])[0]
        ?.value,
    ).toBe('not shown here');
  });
  it('shows what a profile would pass, and which fields it inherits', () => {
    const all = profiles();
    const mine = all.find((p) => !p.template)!;
    mine.body.effort = 'high';
    mine.from = { model: mine.id, effort: 'c'.repeat(32) };
    const rows = selectedRows(all);
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({
      profile: 'Mine',
      model: 'sonnet',
      effort: 'high',
      inherited: ['effort'],
      template: false,
    });
    // A profile that names neither is not a selection.
    expect(selectedRows(all.filter((p) => p.template))).toEqual([]);
  });
  it('shows only what a runtime reported for an attached session, in the Sessions view’s words', () => {
    const page = damaged(pages.loaded);
    const s = page.sessions[1];
    s.binding = 'bound_attached';
    s.mode = 'inference';
    s.observed_model = {
      model_id: 'model-b',
      provider_id: null,
      reasoning_effort: null,
      source: 'turn_rerouted',
      rerouted_from: 'model-a',
      reroute_reason: 'high_risk_cyber_activity',
      rerouted_turn: '33333333-3333-4333-8333-333333333333',
    };
    const rows = observedRows(parseSessionPage(page).sessions);
    const rerouted = rows.filter((r) => r.rerouted);
    expect(rerouted).toHaveLength(1);
    expect(rerouted[0]?.line).toContain('rerouted a turn from model-a');
    // A detached session with no report is not listed at all: nothing is invented for it.
    expect(
      observedRows(parseSessionPage(damaged(pages.loaded)).sessions),
    ).toEqual([]);
    expect(observedRows(null)).toEqual([]);
  });
});

describe('the catalogue controller', () => {
  const never = () => new Promise<never>(() => undefined);
  it('reads once when first shown, again only when asked, and never on a timer', async () => {
    let reads = 0;
    const seen: ModelsState[] = [];
    const controller = new ModelsController((s) => seen.push(s), {
      catalogue: async () => {
        reads += 1;
        return parseModelCatalogue(catalogues.listed);
      },
    });
    controller.setVisible(false);
    expect(reads).toBe(0);
    controller.setVisible(true);
    await settle();
    controller.setVisible(false);
    controller.setVisible(true);
    await settle();
    expect(reads).toBe(1);
    expect(seen.at(-1)?.phase).toBe('ready');
    await controller.refresh();
    expect(reads).toBe(2);
    // A read in flight is not doubled.
    const slow = new ModelsController(() => undefined, { catalogue: never });
    void slow.refresh();
    void slow.refresh();
  });
  it('keeps what it showed and says it is older when a later read fails; shows nothing invented', async () => {
    let fail = false;
    const seen: ModelsState[] = [];
    const controller = new ModelsController((s) => seen.push(s), {
      catalogue: async () => {
        if (fail)
          throw new CatalogueCallError({ code: 'unavailable', retry: true });
        return parseModelCatalogue(catalogues.listed);
      },
    });
    await controller.refresh();
    fail = true;
    await controller.refresh();
    const last = seen.at(-1)!;
    expect(last.phase).toBe('ready');
    expect(last.stale).toBe(true);
    expect(last.failure).toBe('unavailable');
    expect(last.catalogue?.entries).toHaveLength(2);
    // With nothing shown before, a failure is a failure, not an empty list.
    const first = new ModelsController((s) => seen.push(s), {
      catalogue: async () => {
        throw new Error('anything else');
      },
    });
    await first.refresh();
    expect(seen.at(-1)).toMatchObject({
      phase: 'failed',
      catalogue: null,
      failure: 'internal',
    });
  });
});

describe('the models view', () => {
  const listed = () => parseModelCatalogue(catalogues.listed);
  it('shows the four facts apart, with provenance and the entitlement note', async () => {
    const page = damaged(pages.loaded);
    page.sessions[1].binding = 'bound_attached';
    page.sessions[1].mode = 'inference';
    component = mount(ModelsHarness, {
      target: document.body,
      props: {
        client: { catalogue: async () => listed() },
        configItems: [setting('model', 'opus', 'claude_code')],
        profiles: profiles(),
        sessions: parseSessionPage(page).sessions,
      },
    });
    await settle();
    const root = document.querySelector('.models-view')!;
    expect(text(root.querySelector('[data-testid="provenance"]'))).toContain(
      'Listed once by the managed Codex runtime',
    );
    expect(text(root)).toContain('does not mean your account can use it');
    const headings = [...root.querySelectorAll('h3')].map((h) => text(h));
    expect(headings).toEqual([
      'Offered by the managed Codex runtime',
      'Configured in the agents’ own files',
      'Selected by a profile',
      'Reported in use',
    ]);
    const offered = root.querySelectorAll(
      'ul[aria-label="Models the runtime offers"] li',
    );
    expect(offered).toHaveLength(2);
    expect(text(offered[0])).toContain('default of this runtime');
    expect(text(offered[1])).toContain('marks it for replacement by model-a');
    expect(
      text(root.querySelector('ul[aria-label="Configured models"]')),
    ).toContain('Claude Code · model: opus');
    expect(
      text(root.querySelector('ul[aria-label="Models selected by profiles"]')),
    ).toContain('Mine');
    // The attached session is shown as the Sessions view would say it: not reported.
    expect(
      text(
        root.querySelector('ul[aria-label="Models reported by the runtime"]'),
      ),
    ).toContain('Model in use: not reported by the runtime');
    // Nothing on this view changes anything: it only reads again.
    const buttons = [...root.querySelectorAll('button')].map((b) => text(b));
    expect(buttons).toEqual(['Read again']);
  });
  it('says there is no list for a runtime that is not running, and never shows an old one', async () => {
    component = mount(ModelsHarness, {
      target: document.body,
      props: {
        client: {
          catalogue: async () => parseModelCatalogue(catalogues.not_ready),
        },
      },
    });
    await settle();
    const root = document.querySelector('.models-view')!;
    expect(text(root)).toContain('there is no list');
    expect(
      root.querySelector('ul[aria-label="Models the runtime offers"]'),
    ).toBeNull();
    expect(text(root)).toContain(
      'Not read yet. The Configuration view reads these files.',
    );
    expect(text(root)).toContain('Managed sessions have not been read yet.');
  });
  it('says a failed read in words, with a way to try again', async () => {
    component = mount(ModelsHarness, {
      target: document.body,
      props: {
        client: {
          catalogue: async () => {
            throw new CatalogueCallError({ code: 'unavailable', retry: true });
          },
        },
      },
    });
    await settle();
    const root = document.querySelector('.models-view')!;
    expect(text(root.querySelector('[role="alert"]'))).toBe(
      'The list could not be read: the authority did not answer.',
    );
    expect(text(root.querySelector('button'))).toBe('Try again');
  });
});
