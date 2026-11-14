import { afterEach, expect, it } from 'vitest';
import {
  parseModelCatalogue,
  type ModelCatalogue,
} from '../src/lib/contracts/models';
import { parseInventoryReply } from '../src/lib/contracts/configuration';
import type { ModelsState } from '../src/lib/features/models/controller';
import {
  distributorCards,
  distributorOf,
} from '../src/lib/features/distributors/present';
import {
  listedIds,
  loadSeen,
  parseSeen,
  reconcile,
  saveSeen,
} from '../src/lib/features/distributors/seen';
import { setLanguage, t, tPhrase } from '../src/lib/i18n/index.svelte';
import catalogues from './fixtures/model-catalogue-v1.json';
import inventory from './fixtures/configuration-inventory-v1.json';
import { launcher, profiles } from './native-harness';

const KEY = 'wyss-console.distributors-seen';
afterEach(() => {
  localStorage.clear();
  setLanguage('en');
});

const listed = parseModelCatalogue(catalogues.listed);
const notReady = parseModelCatalogue(catalogues.not_ready);
const partial = parseModelCatalogue(catalogues.partial_without_offers);
const ready = (
  catalogue: ModelCatalogue,
  over: Partial<ModelsState> = {},
): ModelsState => ({
  phase: 'ready',
  catalogue,
  stale: false,
  failure: null,
  ...over,
});
const withModel = (id: string): ModelCatalogue => ({
  ...listed,
  entries: [...listed.entries, { ...listed.entries[0], model_id: id }],
});
const items = parseInventoryReply(inventory).inventory.items;
const idle: ModelsState = {
  phase: 'idle',
  catalogue: null,
  stale: false,
  failure: null,
};
const card = (cards: ReturnType<typeof distributorCards>, id: string) =>
  cards.find((c) => c.id === id)!;
const labels = (c: ReturnType<typeof card>) =>
  c.models.map((m) =>
    typeof m.label === 'string' ? m.label : [m.label.text, m.label.values],
  );

/** One refresh as the view does it: compare with what is kept, keep what the read allows. */
function refresh(models: ModelsState): string[] {
  const result = reconcile(loadSeen(), listedIds(models));
  if (result.save) saveSeen(result.save);
  return result.fresh;
}

it('says installed with the version, not installed, or unknown, as the launcher answered', () => {
  // The launcher of the harness: Claude Code installed, Codex not, OpenCode not named.
  const cards = distributorCards(launcher(), idle, null, []);
  expect(cards.map((c) => c.name)).toEqual([
    'Anthropic',
    'OpenAI',
    'Kimi',
    'OpenCode',
  ]);
  expect(cards.map((c) => c.tool)).toEqual([
    'Claude Code',
    'Codex',
    'Kimi CLI',
    'OpenCode',
  ]);
  expect(card(cards, 'anthropic').status).toEqual({
    text: 'Installed · {version}',
    values: { version: '2.1.303 (Claude Code)' },
  });
  expect(card(cards, 'anthropic').installed).toBe(true);
  expect(card(cards, 'openai').status).toEqual({ text: 'Tool not installed' });
  expect(card(cards, 'openai').installed).toBe(false);
  // The launcher answered and did not name it: nothing is known of it.
  expect(card(cards, 'opencode').status).toEqual({ text: 'Unknown' });

  // An installed tool whose version was not reported is still installed.
  const unversioned = launcher();
  unversioned.agents[0].version = null;
  expect(
    card(distributorCards(unversioned, idle, null, []), 'anthropic').status,
  ).toEqual({ text: 'Installed · version not reported' });

  // The launcher did not answer: every tool is unknown, none is said to be missing.
  const unknown = distributorCards(null, idle, null, []);
  for (const id of ['anthropic', 'openai', 'opencode']) {
    expect(card(unknown, id).status, id).toEqual({ text: 'Unknown' });
    expect(card(unknown, id).installed, id).toBe(false);
  }
  // No card ever says "connected".
  expect(JSON.stringify([cards, unknown])).not.toMatch(/connected/i);
});

it('keeps Kimi not configured whatever is read', () => {
  for (const cards of [
    distributorCards(null, idle, null, []),
    distributorCards(launcher(), ready(listed), items, profiles(), ['model-a']),
  ]) {
    const kimi = card(cards, 'kimi');
    expect(kimi.status).toEqual({ text: 'Not configured' });
    expect(kimi.installed).toBe(false);
    expect(kimi.models).toEqual([]);
    expect(kimi.notes).toEqual([]);
  }
  // Hermes has no card.
  expect(distributorCards(null, idle, null, []).map((c) => c.id)).not.toContain(
    'hermes',
  );
});

it('names the configured model and the owner’s profiles for the tools that list nothing', () => {
  const cards = distributorCards(launcher(), idle, items, profiles());
  const anthropic = card(cards, 'anthropic');
  // The same value in two files is one row; a template is not the owner's profile.
  expect(labels(anthropic)).toEqual([
    ['Configured model: {model}', { model: 'opus' }],
    ['Profile {name}: {model}', { name: 'Mine', model: 'sonnet' }],
  ]);
  expect(anthropic.models.every((m) => !m.fresh)).toBe(true);
  expect(anthropic.notes).toEqual([
    { text: 'This tool does not list its models to the Console.' },
  ]);
  const opencode = card(cards, 'opencode');
  expect(labels(opencode)).toEqual([]);
  expect(opencode.notes).toEqual(anthropic.notes);
  // Codex is never filled from a file: with no list read it shows none.
  expect(card(cards, 'openai').models).toEqual([]);
});

it('lists what Codex offers, and says why when there is no list', () => {
  const offered = card(
    distributorCards(launcher(), ready(listed), items, [], ['model-b']),
    'openai',
  );
  expect(labels(offered)).toEqual(['model-a', 'model-b']);
  expect(offered.models.map((m) => m.fresh)).toEqual([false, true]);
  expect(offered.notes).toEqual([]);

  const stopped = card(
    distributorCards(launcher(), ready(notReady), items, []),
    'openai',
  );
  expect(stopped.models).toEqual([]);
  expect(stopped.notes.map((n) => n.text)).toEqual([
    'The managed Codex runtime is not running, so there is no list. A list of a runtime that has stopped is never shown. Start a managed session and it is listed again.',
  ]);

  const cut = card(
    distributorCards(launcher(), ready(partial), items, []),
    'openai',
  );
  expect(labels(cut)).toEqual(['model-only']);
  expect(cut.notes.map((n) => n.text)).toEqual([
    'The runtime’s list continued past what was taken: this list is partial.',
  ]);

  const failed = card(
    distributorCards(
      launcher(),
      { phase: 'failed', catalogue: null, stale: false, failure: 'busy' },
      items,
      [],
    ),
    'openai',
  );
  expect(failed.notes.map((n) => n.text)).toEqual([
    'The list could not be read: the authority is busy. Try again.',
  ]);
});

it('maps an agent to who makes it', () => {
  expect(distributorOf('claude_code')).toBe('Anthropic');
  expect(distributorOf('codex')).toBe('OpenAI');
  expect(distributorOf('opencode')).toBe('OpenCode');
});

it('keeps the first read without marking anything', () => {
  expect(loadSeen()).toBeNull();
  expect(refresh(ready(listed))).toEqual([]);
  expect(loadSeen()).toEqual(['model-a', 'model-b']);
});

it('marks a model not seen before, and clears the mark at the next refresh', () => {
  refresh(ready(listed));
  expect(refresh(ready(withModel('model-c')))).toEqual(['model-c']);
  expect(loadSeen()).toEqual(['model-a', 'model-b', 'model-c']);
  expect(refresh(ready(withModel('model-c')))).toEqual([]);
  // A model that left the list and came back is not new again.
  expect(refresh(ready(listed))).toEqual([]);
  expect(refresh(ready(withModel('model-c')))).toEqual([]);
});

it('never marks from, or overwrites what was seen with, a read that is not ready, failed or partial', () => {
  refresh(ready(listed));
  const kept = localStorage.getItem(KEY);
  for (const [name, models] of Object.entries({
    not_ready: ready(notReady),
    // The last read failed: what is held is the older list.
    failed: ready(withModel('model-x'), {
      stale: true,
      failure: 'unavailable',
    }),
    never_read: {
      phase: 'failed',
      catalogue: null,
      stale: false,
      failure: 'internal',
    } satisfies ModelsState,
    partial: ready(partial),
    loading: { ...idle, phase: 'loading' } satisfies ModelsState,
  })) {
    expect(listedIds(models), name).toBeNull();
    expect(refresh(models), name).toEqual([]);
    expect(localStorage.getItem(KEY), name).toBe(kept);
  }
  // What was seen survived: the next whole list marks only what really is new.
  expect(refresh(ready(withModel('model-c')))).toEqual(['model-c']);
});

it('does not take a read that cannot be compared as the first one', () => {
  expect(refresh(ready(notReady))).toEqual([]);
  expect(refresh(ready(partial))).toEqual([]);
  expect(localStorage.getItem(KEY)).toBeNull();
  // The first whole list is the first read: kept, nothing marked.
  expect(refresh(ready(listed))).toEqual([]);
  expect(loadSeen()).toEqual(['model-a', 'model-b']);
});

it('reads storage that is missing or damaged as never read, and survives storage that throws', () => {
  expect(parseSeen(null)).toBeNull();
  expect(parseSeen('')).toBeNull();
  expect(parseSeen('{')).toBeNull();
  expect(parseSeen('{"a":1}')).toBeNull();
  expect(parseSeen('[]')).toEqual([]);
  expect(parseSeen('["a",1,"b"]')).toEqual(['a', 'b']);
  // "An empty list was seen" is not "never read": a model after it is new.
  expect(reconcile([], ['model-a'])).toEqual({
    save: ['model-a'],
    fresh: ['model-a'],
  });
  const real = globalThis.localStorage;
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    get() {
      throw new Error('denied');
    },
  });
  try {
    expect(loadSeen()).toBeNull();
    expect(() => saveSeen(['a'])).not.toThrow();
  } finally {
    Object.defineProperty(globalThis, 'localStorage', {
      configurable: true,
      value: real,
    });
  }
});

it('says each state in Spanish in the words the owner fixed', () => {
  setLanguage('es');
  const cards = distributorCards(launcher(), ready(listed), items, profiles());
  expect(tPhrase(card(cards, 'anthropic').status)).toBe(
    'Instalado · 2.1.303 (Claude Code)',
  );
  expect(tPhrase(card(cards, 'openai').status)).toBe('No instalado');
  expect(tPhrase(card(cards, 'kimi').status)).toBe('Sin configurar');
  expect(tPhrase(card(cards, 'opencode').status)).toBe('Desconocido');
  expect(t('new model')).toBe('nuevo');
  expect(t('Refresh models')).toBe('Actualizar modelos');
  expect(t('Last read: {time}', { time: '10:00' })).toBe(
    'Última lectura: 10:00',
  );
  expect(t('Only Codex reports new models.')).toBe(
    'Solo Codex informa modelos nuevos.',
  );
  const anthropic = card(cards, 'anthropic');
  expect(
    anthropic.models.map((m) =>
      typeof m.label === 'string' ? m.label : tPhrase(m.label),
    ),
  ).toEqual(['Modelo configurado: opus', 'Perfil Mine: sonnet']);
  expect(tPhrase(anthropic.notes[0])).toBe(
    'Esta herramienta no le lista sus modelos a la Console.',
  );
});
