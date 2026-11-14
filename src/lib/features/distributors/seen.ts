import type { ModelsState } from '../models/controller';

/**
 * The Codex models this window has already shown, so that a refresh can say which are new.
 * Presentation only: kept in the window's own storage, never sent anywhere.
 */
const KEY = 'wyss-console.distributors-seen';

/** What was kept, or null when nothing was: "never read" is not "an empty list was seen". */
export function parseSeen(raw: string | null): string[] | null {
  if (!raw) return null;
  try {
    const value: unknown = JSON.parse(raw);
    return Array.isArray(value)
      ? value.filter((id): id is string => typeof id === 'string')
      : null;
  } catch {
    return null;
  }
}

export function loadSeen(): string[] | null {
  try {
    return parseSeen(globalThis.localStorage?.getItem(KEY) ?? null);
  } catch {
    return null;
  }
}

export function saveSeen(seen: readonly string[]): void {
  try {
    globalThis.localStorage?.setItem(KEY, JSON.stringify(seen));
  } catch {
    /* A list that cannot be kept is not an error: nothing is marked new next time. */
  }
}

/**
 * The model ids of a read that may be compared with what was seen, or null when the read says
 * nothing about what exists: the runtime is not ready, the last read failed (what is held is
 * older), or the runtime's list continued past what was taken.
 */
export function listedIds(models: ModelsState): string[] | null {
  const catalogue = models.catalogue;
  if (
    catalogue === null ||
    models.phase !== 'ready' ||
    models.stale ||
    models.failure !== null ||
    catalogue.state !== 'listed' ||
    catalogue.complete === false
  )
    return null;
  return catalogue.entries.map((entry) => entry.model_id);
}

export interface Reconciled {
  /** What to keep; null when nothing is to be written. */
  save: string[] | null;
  /** The ids to mark as new until the next refresh. */
  fresh: string[];
}

/**
 * One refresh against what was seen. A read that cannot be compared marks nothing and writes
 * nothing. The first read that can is kept without marking anything. After that, an id not
 * seen before is new, and what is kept only grows: a model that left a list and came back
 * is not new again.
 */
export function reconcile(
  seen: readonly string[] | null,
  listed: readonly string[] | null,
): Reconciled {
  if (listed === null) return { save: null, fresh: [] };
  if (seen === null) return { save: [...listed], fresh: [] };
  const known = new Set(seen);
  const fresh = listed.filter((id) => !known.has(id));
  return {
    save: fresh.length > 0 ? [...seen, ...fresh] : null,
    fresh,
  };
}
