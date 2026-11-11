import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, expect, it } from 'vitest';
import { LANGUAGES } from '../src/lib/contracts/settings';
import { ES } from '../src/lib/i18n/es';
import {
  currentLanguage,
  msg,
  setLanguage,
  t,
  tJoin,
  tPhrase,
} from '../src/lib/i18n/index.svelte';
import { activityLabel } from '../src/lib/features/agent/present';

afterEach(() => setLanguage('en'));

/** Every literal sentence `src/` passes to `t()` or marks with `msg()`. */
function sentences(): Map<string, string> {
  const found = new Map<string, string>();
  const call = /\b(?:t|msg)\(\s*(['"`])((?:\\.|(?!\1)[^\\])*)\1/gs;
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (/\.(ts|svelte)$/.test(name) && !path.includes('/i18n/'))
        for (const match of readFileSync(path, 'utf8').matchAll(call))
          found.set(match[2].replace(/\\(['"`])/g, '$1'), path);
    }
  };
  walk(join(process.cwd(), 'src'));
  return found;
}
const places = (text: string) =>
  [...text.matchAll(/\{(\w+)\}/g)].map((match) => match[1]).sort();

it('is English, unchanged, until Spanish is chosen', () => {
  expect(currentLanguage()).toBe('en');
  expect(t('Settings')).toBe('Settings');
  expect(t('{count} waiting', { count: 3 })).toBe('3 waiting');
  setLanguage('es');
  expect(document.documentElement.lang).toBe('es');
  expect(t('Settings')).toBe('Ajustes');
  expect(t('{count} waiting', { count: 3 })).toBe('3 en espera');
  setLanguage('en');
  expect(t('Settings')).toBe('Settings');
});

it('shows a sentence it has no translation for in English, never as nothing', () => {
  setLanguage('es');
  expect(t('A sentence nobody translated {n}', { n: 2 })).toBe(
    'A sentence nobody translated 2',
  );
  // A place with no value is left as written rather than printed as "undefined".
  expect(t('Version {version}')).toBe('Versión {version}');
});

it('marks a sentence without changing it', () => {
  expect(msg('Console')).toBe('Console');
});

it('draws a phrase whose place takes another phrase, and joins phrases', () => {
  const nested = {
    text: 'The turn was not started. {reason}',
    values: { reason: { text: 'Codex refused the turn.' } },
  };
  setLanguage('es');
  expect(tPhrase(nested)).toBe(
    'El turno no se inició. Codex rechazó el turno.',
  );
  expect(tJoin(activityLabel('command_completed', 2))).toBe(
    'Comando terminado · 2',
  );
  setLanguage('en');
  expect(tPhrase(nested)).toBe(
    'The turn was not started. Codex refused the turn.',
  );
  expect(tJoin(activityLabel('command_completed', 2))).toBe(
    'Command finished · 2',
  );
});

it('has a Spanish sentence for every sentence the window can show', () => {
  const missing = [...sentences()]
    .filter(([text]) => !Object.hasOwn(ES, text))
    .map(([text, path]) => `${path}: ${text}`);
  expect(missing).toEqual([]);
});

it('keeps the same places in both languages', () => {
  const different = Object.entries(ES)
    .filter(([en, es]) => places(en).join() !== places(es).join())
    .map(([en]) => en);
  expect(different).toEqual([]);
});

it('never answers with an empty or untouched translation by accident', () => {
  for (const [en, es] of Object.entries(ES)) {
    expect(es.trim(), en).not.toBe('');
    expect(es, en).toBe(es.trim());
  }
});

it('offers exactly the languages that have a dictionary', () => {
  expect([...LANGUAGES]).toEqual(['en', 'es']);
});
