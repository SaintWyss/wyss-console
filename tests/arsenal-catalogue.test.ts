import { describe, expect, it } from 'vitest';
import type {
  Candidate,
  Compatibility,
  LibraryItem,
} from '../src/lib/contracts/arsenal';
import {
  IMPORTABLE_KINDS,
  filterDiscover,
  filterLibrary,
  libraryCompatibility,
  librarySources,
} from '../src/lib/features/arsenal/catalogue';

const summary = (
  name: string,
  provider: LibraryItem['provider'],
  source: string,
  kind: string = 'skill',
  status: Compatibility = 'supported',
): LibraryItem => ({
  id: `${provider}-${name}`,
  provider,
  source,
  kind,
  name,
  description: `${name} description`,
  origin: `~/.claude/skills/${name}`,
  revision: 1,
  revisions: 1,
  tree_hash: 'a'.repeat(64),
  updated_at: 1,
  compatibility: { status, reason: 'from the desktop' },
});

const candidate = (overrides: Partial<Candidate>): Candidate => ({
  id: 'id',
  tree_hash: 'b'.repeat(64),
  status: 'not_imported',
  importable: true,
  compatibility: 'unverified',
  capability: null,
  duplicates: [],
  name_collisions: [],
  kind: 'skill',
  provider: 'claude_code',
  source: 'claude_user_skills',
  scope: 'user',
  name: 'design',
  description: 'design description',
  origin: '~/.claude/skills/design',
  file_count: 0,
  excluded_count: 0,
  review_count: 0,
  executes_code: false,
  blocked: null,
  managed_by: null,
  ...overrides,
});

const library = [
  summary('design', 'claude_code', 'claude_user_skills'),
  summary('review', 'codex', 'codex_user_skills'),
  summary('tokens', 'claude_code', 'claude_user_skills'),
];

const none = {
  query: '',
  provider: 'all',
  source: 'all',
  compatibility: 'all',
} as const;

describe('Arsenal library filters', () => {
  it('filters by provider, source and text without changing the library', () => {
    expect(filterLibrary(library, none).map((item) => item.name)).toEqual([
      'design',
      'review',
      'tokens',
    ]);
    expect(
      filterLibrary(library, { ...none, provider: 'codex' }).map(
        (item) => item.name,
      ),
    ).toEqual(['review']);
    expect(
      filterLibrary(library, {
        ...none,
        source: 'claude_user_skills',
        query: 'TOK',
      }).map((item) => item.name),
    ).toEqual(['tokens']);
    expect(library).toHaveLength(3);
  });

  it('shows the compatibility the desktop judged, and filters by it', () => {
    expect(libraryCompatibility(library[0])).toBe('supported');
    const missing = summary(
      'x',
      'codex',
      'codex_user_rules',
      'rule',
      'missing_dependency',
    );
    expect(libraryCompatibility(missing)).toBe('missing_dependency');
    expect(
      filterLibrary([...library, missing], {
        ...none,
        compatibility: 'missing_dependency',
      }).map((item) => item.name),
    ).toEqual(['x']);
    expect(
      filterLibrary(library, { ...none, compatibility: 'unverified' }),
    ).toEqual([]);
  });

  it('lists the sources present, once each, in first-seen order', () => {
    expect(librarySources(library)).toEqual([
      'claude_user_skills',
      'codex_user_skills',
    ]);
  });

  it('holds only skills, agents and rules: the kinds that can be copied', () => {
    expect([...IMPORTABLE_KINDS]).toEqual(['skill', 'agent', 'rule']);
  });
});

describe('Arsenal discovery filters', () => {
  const scan = [
    candidate({ id: 'a', name: 'design', status: 'not_imported' }),
    candidate({
      id: 'b',
      name: 'review',
      provider: 'codex',
      source: 'codex_user_skills',
      status: 'imported',
    }),
    candidate({ id: 'c', name: 'tokens', status: 'source_changed' }),
    candidate({
      id: 'd',
      name: 'pre-commit',
      kind: 'hook',
      importable: false,
      status: 'read_only',
      blocked: 'read_only_kind',
      compatibility: 'unsupported',
    }),
    candidate({
      id: 'e',
      name: 'reviewer',
      kind: 'agent',
      scope: 'project',
      source: 'claude_project_agents',
      status: 'not_imported',
    }),
  ];
  const names = (filter: Partial<Parameters<typeof filterDiscover>[1]>) =>
    filterDiscover(scan, {
      query: '',
      provider: 'all',
      kind: 'all',
      scope: 'all',
      status: 'all',
      ...filter,
    }).map((entry) => entry.name);

  it('filters a scan by status, provider, kind and scope; a scan is never a library entry', () => {
    expect(names({ status: 'source_changed' })).toEqual(['tokens']);
    expect(names({ provider: 'codex' })).toEqual(['review']);
    expect(names({ kind: 'agent' })).toEqual(['reviewer']);
    expect(names({ kind: 'hook' })).toEqual(['pre-commit']);
    expect(names({ scope: 'project' })).toEqual(['reviewer']);
    expect(names({ status: 'read_only' })).toEqual(['pre-commit']);
    expect(scan).toHaveLength(5);
  });
});
