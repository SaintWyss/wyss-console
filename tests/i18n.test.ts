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
import {
  composeImpact,
  failureMessage,
  observedModelLine,
  pendingMessage,
  runtimeLine,
} from '../src/lib/features/sessions/present';
import { catalogueFailure, entryRow } from '../src/lib/features/models/present';
import { parseModelCatalogue } from '../src/lib/contracts/models';
import catalogues from './fixtures/model-catalogue-v1.json';

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

it('draws the sessions sentences in Spanish, with their nested places', () => {
  const runtime = { mode: 'local_tools' as const, workspace: '/a/rag-corp' };
  setLanguage('es');
  const line = runtimeLine(runtime);
  expect(line && tPhrase(line)).toBe(
    'Codex corre en Local Tools para rag-corp',
  );
  expect(
    tPhrase(
      observedModelLine({ binding: 'bound_attached', observed_model: null }),
    ),
  ).toBe('Modelo en uso: el runtime no lo informó');
  expect(tPhrase(pendingMessage('activate', 'starting', 'local_tools'))).toBe(
    'Iniciando Codex en Local Tools…',
  );
  expect(tPhrase(failureMessage('resume', 'rejected'))).toBe(
    'La sesión no se reanudó. Codex rechazó reanudar esta sesión. Una sesión solo se reanuda en el modo en que se creó: si Codex corre ahora en el otro, cambiá el modo de esta sesión.',
  );
  const impact = composeImpact('inference', null, runtime);
  expect(impact && tPhrase(impact)).toBe(
    'Codex corre en Local Tools para rag-corp ahora, y la autoridad corre un solo runtime de Codex. Activar esta sesión lo reinicia en Inference, y las sesiones conectadas a él quedan desconectadas. Una sesión creada en Local Tools no se puede reanudar en Inference.',
  );
  setLanguage('en');
  expect(impact && tPhrase(impact)).toBe(
    'Codex runs in Local Tools for rag-corp now, and the authority runs one Codex runtime. Activating this session restarts it in Inference, and the sessions attached to it are detached. A session created in Local Tools cannot be resumed in Inference.',
  );
});

it('draws the models sentences in Spanish, with their efforts and places', () => {
  const [listed] = parseModelCatalogue(catalogues.listed).entries.map(entryRow);
  setLanguage('es');
  expect(listed?.efforts && tJoin(listed.efforts)).toBe(
    'low · medium (predeterminado) · high',
  );
  expect(tPhrase(catalogueFailure('busy'))).toBe(
    'No se pudo leer la lista: la autoridad está ocupada. Probá de nuevo.',
  );
  setLanguage('en');
  expect(listed?.efforts && tJoin(listed.efforts)).toBe(
    'low · medium (default) · high',
  );
});

it('draws the credentials notices and their dates in Spanish', () => {
  setLanguage('es');
  expect(t('Key kept, sealed.')).toBe('Clave guardada, sellada.');
  expect(t('kept {date}', { date: '11 nov 2026' })).toBe(
    'guardada el 11 nov 2026',
  );
  setLanguage('en');
  expect(t('kept {date}', { date: '11 nov 2026' })).toBe('kept 11 nov 2026');
});

it('draws the remote view sections and its first sentence in Spanish', () => {
  setLanguage('es');
  expect(t('Hosts')).toBe('Equipos');
  expect(t('Connect to a host')).toBe('Conectar con un equipo');
  setLanguage('en');
  expect(t('Hosts')).toBe('Hosts');
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
