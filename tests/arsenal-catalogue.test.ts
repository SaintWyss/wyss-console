import { describe, expect, it } from 'vitest';
import type { Summary, SkillView } from '../src/lib/contracts/arsenal';
import {
  filterDiscover,
  filterLibrary,
  LIBRARY_COMPATIBILITY,
  librarySources,
} from '../src/lib/features/arsenal/catalogue';

const summary = (
  name: string,
  provider: Summary['provider'],
  source: string,
): Summary => ({
  id: `${provider}-${name}`,
  provider,
  source,
  kind: 'skill',
  name,
  description: `${name} description`,
  origin: `~/.claude/skills/${name}`,
  revision: 1,
  revisions: 1,
  tree_hash: 'a'.repeat(64),
  updated_at: 1,
});

const skill = (
  name: string,
  status: SkillView['status'],
  provider: SkillView['provider'],
): SkillView => ({
  id: `${provider}-${name}`,
  tree_hash: 'b'.repeat(64),
  status,
  capability: null,
  duplicates: [],
  name_collisions: [],
  provider,
  source: provider === 'codex' ? 'codex_user_skills' : 'claude_user_skills',
  name,
  description: `${name} description`,
  origin: `~/.claude/skills/${name}`,
  files: [],
  excluded: [],
  review: [],
  executes_code: false,
  blocked: null,
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

  it('reports compatibility as unverified until a launched session proves it, and filters on it', () => {
    expect(LIBRARY_COMPATIBILITY).toBe('unverified');
    expect(
      filterLibrary(library, { ...none, compatibility: 'supported' }),
    ).toEqual([]);
  });

  it('lists the sources present, once each, in first-seen order', () => {
    expect(librarySources(library)).toEqual([
      'claude_user_skills',
      'codex_user_skills',
    ]);
  });
});

describe('Arsenal discovery filters', () => {
  const scan = [
    skill('design', 'not_imported', 'claude_code'),
    skill('review', 'imported', 'codex'),
    skill('tokens', 'source_changed', 'claude_code'),
  ];

  it('filters a scan by status and provider; a scan is never a library entry', () => {
    expect(
      filterDiscover(scan, {
        query: '',
        provider: 'all',
        status: 'source_changed',
      }).map((entry) => entry.name),
    ).toEqual(['tokens']);
    expect(
      filterDiscover(scan, { query: '', provider: 'codex', status: 'all' }).map(
        (entry) => entry.name,
      ),
    ).toEqual(['review']);
    expect(scan).toHaveLength(3);
  });
});
