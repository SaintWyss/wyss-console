import type { Language } from '../contracts/settings';
import { ES } from './es';

export { msg } from './msg';

/**
 * The language of the window's own text. English is the source: every sentence is written in
 * English where it is used and is its own key, so the English window is byte for byte what it
 * was before there was a choice. Spanish is a dictionary from those sentences.
 *
 * Only the window's own text is translated. What the desktop, the authority or a provider wrote
 * (a reason, a path, a profile preview) is shown as it arrived.
 */
let language = $state<Language>('en');

const DICTIONARIES: Record<Language, Readonly<Record<string, string>> | null> =
  { en: null, es: ES };

/** Applies the preference. Called by the shell when the preferences are read or changed. */
export function setLanguage(next: Language) {
  language = next;
  if (typeof document !== 'undefined') document.documentElement.lang = next;
}
export function currentLanguage(): Language {
  return language;
}

function fill(template: string, values?: Record<string, string | number>) {
  if (!values) return template;
  return template.replace(/\{(\w+)\}/g, (whole, name: string) =>
    Object.hasOwn(values, name) ? String(values[name]) : whole,
  );
}

/**
 * The sentence in the language in force. `{name}` is replaced by `values.name`. A sentence
 * the dictionary does not have is shown in English: missing text is never an empty screen.
 * Reads the language, so a component that calls it is redrawn when the language changes.
 */
export function t(text: string, values?: Record<string, string | number>) {
  return fill(DICTIONARIES[language]?.[text] ?? text, values);
}
