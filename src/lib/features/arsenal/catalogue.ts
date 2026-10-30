import type {
  ArsenalProvider,
  Compatibility,
  SkillStatus,
  SkillView,
  Summary,
} from '../../contracts/arsenal';

export type ProviderFilter = ArsenalProvider | 'all';

export interface LibraryFilter {
  query: string;
  provider: ProviderFilter;
  source: string | 'all';
  compatibility: Compatibility | 'all';
}

export interface DiscoverFilter {
  query: string;
  provider: ProviderFilter;
  status: SkillStatus | 'all';
}

/**
 * Compatibility of every imported skill with its provider. The format is known; whether a
 * launched session uses it is not verified until AM1b, so it is `unverified` for all of them.
 */
export const LIBRARY_COMPATIBILITY: Compatibility = 'unverified';

/** Filters the library. Matching is plain text over name, description and origin. */
export function filterLibrary(
  items: readonly Summary[],
  filter: LibraryFilter,
): Summary[] {
  const needle = filter.query.trim().toLocaleLowerCase();
  return items.filter(
    (item) =>
      (filter.provider === 'all' || item.provider === filter.provider) &&
      (filter.source === 'all' || item.source === filter.source) &&
      (filter.compatibility === 'all' ||
        LIBRARY_COMPATIBILITY === filter.compatibility) &&
      (!needle ||
        [item.name, item.description, item.origin]
          .join(' ')
          .toLocaleLowerCase()
          .includes(needle)),
  );
}

/** The sources present in the library, in the order they first appear. */
export function librarySources(items: readonly Summary[]): string[] {
  return [...new Set(items.map((item) => item.source))];
}

/** Filters a scan of the account's skill folders. A scan is never a library entry. */
export function filterDiscover(
  skills: readonly SkillView[],
  filter: DiscoverFilter,
): SkillView[] {
  const needle = filter.query.trim().toLocaleLowerCase();
  return skills.filter(
    (skill) =>
      (filter.provider === 'all' || skill.provider === filter.provider) &&
      (filter.status === 'all' || skill.status === filter.status) &&
      (!needle ||
        [skill.name, skill.description, skill.origin]
          .join(' ')
          .toLocaleLowerCase()
          .includes(needle)),
  );
}
