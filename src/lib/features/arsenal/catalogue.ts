import type {
  ArsenalProvider,
  Candidate,
  CandidateKind,
  CandidateStatus,
  Compatibility,
  Summary,
} from '../../contracts/arsenal';

export type ProviderFilter = ArsenalProvider | 'all';
export type KindFilter = CandidateKind | 'all';
export type ScopeFilter = 'user' | 'project' | 'all';

export interface LibraryFilter {
  query: string;
  provider: ProviderFilter;
  source: string | 'all';
  compatibility: Compatibility | 'all';
}

export interface DiscoverFilter {
  query: string;
  provider: ProviderFilter;
  kind: KindFilter;
  scope: ScopeFilter;
  status: CandidateStatus | 'all';
}

/** The kinds the library can hold. Everything else is listed in Discover and never copied. */
export const IMPORTABLE_KINDS: readonly CandidateKind[] = [
  'skill',
  'agent',
  'rule',
];

/**
 * Compatibility of an imported capability with its provider. The format is known for the kinds the
 * library holds; whether a launched session uses it is not verified until AM1b.
 */
export function libraryCompatibility(item: Summary): Compatibility {
  return IMPORTABLE_KINDS.includes(item.kind as CandidateKind)
    ? 'unverified'
    : 'unsupported';
}

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
        libraryCompatibility(item) === filter.compatibility) &&
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

/** Filters a scan of the account's and the project's folders. A scan is never a library entry. */
export function filterDiscover(
  candidates: readonly Candidate[],
  filter: DiscoverFilter,
): Candidate[] {
  const needle = filter.query.trim().toLocaleLowerCase();
  return candidates.filter(
    (candidate) =>
      (filter.provider === 'all' || candidate.provider === filter.provider) &&
      (filter.kind === 'all' || candidate.kind === filter.kind) &&
      (filter.scope === 'all' || candidate.scope === filter.scope) &&
      (filter.status === 'all' || candidate.status === filter.status) &&
      (!needle ||
        [candidate.name, candidate.description, candidate.origin]
          .join(' ')
          .toLocaleLowerCase()
          .includes(needle)),
  );
}
