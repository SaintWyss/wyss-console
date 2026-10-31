import type {
  ArsenalProvider,
  Candidate,
  CandidateKind,
  CandidateStatus,
  Compatibility,
  LibraryItem,
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
 * Compatibility of an imported capability with its own provider, as the desktop judged it from its
 * files. Whether a terminal uses it is the terminal plan's question, never this one's.
 */
export function libraryCompatibility(item: LibraryItem): Compatibility {
  return item.compatibility.status;
}

/** How a state is drawn: a tone for its color and a symbol, so it is never told by color alone. */
export type StateTone = 'ok' | 'pending' | 'warn' | 'error' | 'neutral';
export interface StateMark {
  tone: StateTone;
  symbol: string;
}

const COMPATIBILITY_MARK: Record<Compatibility, StateMark> = {
  supported: { tone: 'ok', symbol: '✓' },
  unverified: { tone: 'pending', symbol: '?' },
  requires_qualification: { tone: 'pending', symbol: '?' },
  missing_dependency: { tone: 'warn', symbol: '!' },
  conflicting: { tone: 'warn', symbol: '!' },
  unsupported: { tone: 'error', symbol: '✕' },
};
export function compatibilityMark(value: Compatibility): StateMark {
  return COMPATIBILITY_MARK[value];
}

const STATUS_MARK: Record<CandidateStatus, StateMark> = {
  imported: { tone: 'ok', symbol: '✓' },
  source_changed: { tone: 'warn', symbol: '!' },
  not_imported: { tone: 'neutral', symbol: '+' },
  read_only: { tone: 'neutral', symbol: '–' },
};
export function candidateMark(value: CandidateStatus): StateMark {
  return STATUS_MARK[value];
}

/** Filters the library. Matching is plain text over name, description and origin. */
export function filterLibrary(
  items: readonly LibraryItem[],
  filter: LibraryFilter,
): LibraryItem[] {
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
export function librarySources(items: readonly LibraryItem[]): string[] {
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
