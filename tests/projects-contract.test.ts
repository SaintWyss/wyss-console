import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import {
  parseFailure,
  parseOrphans,
  parseRollback,
  parseOutput,
  parseOverview,
  parsePage,
  parseProject,
  parseSession,
  parseSummary,
  parseWorkspace,
} from '../src/lib/contracts/projects';
import {
  failure,
  overview,
  project,
  session,
  summary,
  workspace,
} from './projects-harness';

const bad = (fn: () => unknown) => expect(fn).toThrow(/projects/);

describe('what the desktop says about projects is checked, not believed', () => {
  it('reads the shapes the desktop sends', () => {
    expect(overview().store.state).toBe('ready');
    expect(project().root?.label).toBe('alpha');
    expect(session().kind).toBe('shell');
    expect(workspace().leaves).toHaveLength(1);
    expect(parseSummary(summary())).toEqual(summary());
  });

  it('refuses a store, project, session or workspace that does not fit', () => {
    bad(() => parseOverview({ ...overview(), store: { state: 'invented' } }));
    bad(() => parseOverview({ ...overview(), enabled: 'yes' }));
    bad(() => parseProject({ ...project(), revision: '0' }));
    bad(() => parseProject({ ...project(), revision: 5 }));
    bad(() =>
      parseProject({
        ...project(),
        root: { ...project().root, availability: 'fine' },
      }),
    );
    bad(() => parseSession({ ...session(), kind: 'remote' }));
    bad(() => parseSession({ ...session(), protocol: 3 }));
    bad(() =>
      parseSession({ ...session(), requestedSafetyProfile: 'full_access' }),
    );
    bad(() => parseSession({ ...session(), input: 'maybe' }));
    bad(() => parseWorkspace({ ...workspace(), layout: { kind: 'tab' } }));
    bad(() =>
      parseWorkspace({
        ...workspace(),
        layout: {
          kind: 'split',
          dir: 'row',
          ratio: NaN,
          a: { kind: 'leaf', id: 'a' },
          b: { kind: 'leaf', id: 'b' },
        },
      }),
    );
    bad(() =>
      parsePage({ items: 'x', cursor: null, storeRevision: '1' }, parseProject),
    );
    bad(() =>
      parseSummary({
        ...summary(),
        counts: { ...summary().counts, projects: -1 },
      }),
    );
  });

  it('refuses a layout deeper than the contract allows', () => {
    let node: unknown = { kind: 'leaf', id: 'x' };
    for (let n = 0; n < 8; n++)
      node = {
        kind: 'split',
        dir: 'row',
        ratio: 0.5,
        a: node,
        b: { kind: 'leaf', id: 'y' },
      };
    bad(() => parseWorkspace({ ...workspace(), layout: node }));
  });

  it('reads terminal output as bytes and nothing else', () => {
    expect(
      parseOutput({ data: [104, 105], next: 2, eof: false, truncated: false })
        .data,
    ).toEqual(new Uint8Array([104, 105]));
    bad(() =>
      parseOutput({ data: [256], next: 1, eof: false, truncated: false }),
    );
    bad(() =>
      parseOutput({ data: 'hi', next: 1, eof: false, truncated: false }),
    );
    bad(() =>
      parseOutput({ data: [], next: 1.5, eof: false, truncated: false }),
    );
  });

  it('shows a failure as the desktop sent it, and an unreadable one as an unknown outcome', () => {
    const known = parseFailure(
      failure('leaf_busy', { effect: 'unknown' }).failure,
    );
    expect(known).toMatchObject({ code: 'leaf_busy', effect: 'unknown' });
    for (const garbage of [null, 'boom', { code: 'nope' }, 42]) {
      expect(parseFailure(garbage)).toMatchObject({
        code: 'io_failure',
        effect: 'unknown',
      });
    }
  });

  it('has no field that could carry a path, a command line or a credential', () => {
    const text = JSON.stringify([
      overview(),
      project(),
      session(),
      workspace(),
    ]);
    expect(text).not.toMatch(/"(path|argv|args|env|prompt|password|secret)"/);
  });
});

describe('what the desktop really sends', () => {
  // Written by the Rust test `the_answers_the_window_parses_match_the_recorded_shapes`.
  const recorded = JSON.parse(
    readFileSync('tests/fixtures/projects-v1.json', 'utf8'),
  ) as Record<string, unknown>;

  it('is read by the window as the desktop wrote it', () => {
    expect(parseOverview(recorded.overview).store.state).toBe('ready');
    expect(parsePage(recorded.projects, parseProject).items).toHaveLength(1);
    const view = parseWorkspace(recorded.workspace);
    expect(view.unfinished).toEqual([
      expect.objectContaining({ state: 'ambiguous', mode: 'native' }),
    ]);
    expect(parseOrphans(recorded.orphans)).toEqual([
      expect.objectContaining({ folder: 'repo', nativeSessionId: null }),
    ]);
    expect(parseRollback(recorded.rollback).canDisable).toBe(false);
  });
});
