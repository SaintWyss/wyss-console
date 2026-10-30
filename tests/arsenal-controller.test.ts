import { describe, expect, it, vi } from 'vitest';
import {
  ArsenalShapeError,
  parseDiscover,
  parseImportFailure,
  parseSummaries,
  type Candidate,
  type Detail,
  type DiscoverReply,
  type Preset,
  type Preview,
  type Selection,
  type Summary,
} from '../src/lib/contracts/arsenal';
import {
  ArsenalCallError,
  type ArsenalClient,
} from '../src/lib/desktop/arsenal-client';
import {
  ArsenalController,
  type ArsenalState,
} from '../src/lib/features/arsenal/controller';

const design: Summary = {
  id: 'cap-design',
  provider: 'claude_code',
  source: 'claude_user_skills',
  kind: 'skill',
  name: 'design',
  description: 'Plans screens',
  origin: '~/.claude/skills/design',
  revision: 2,
  revisions: 2,
  tree_hash: 'c'.repeat(64),
  updated_at: 1,
};

const detail = (revision: number): Detail => ({
  summary: { ...design, revision: 2 },
  revision,
  revisions: [
    {
      number: 2,
      tree_hash: 'c'.repeat(64),
      origin: design.origin,
      created_at: 2,
      files: 1,
      bytes: 9,
      current: true,
    },
    {
      number: 1,
      tree_hash: 'd'.repeat(64),
      origin: design.origin,
      created_at: 1,
      files: 1,
      bytes: 5,
      current: false,
    },
  ],
  files: [{ path: 'SKILL.md', sha256: 'e'.repeat(64), size: 9 }],
});

const candidate: Candidate = {
  id: 'id-design',
  tree_hash: 'c'.repeat(64),
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
  description: 'Plans screens',
  origin: '~/.claude/skills/design',
  executes_code: false,
  blocked: null,
  managed_by: null,
  file_count: 1,
  excluded_count: 0,
  review_count: 0,
};

const discovered: DiscoverReply = {
  project: null,
  sources: [
    { source: 'claude_user_skills', state: 'read' },
    { source: 'codex_user_skills', state: 'absent' },
  ],
  candidates: [candidate],
  composites: [],
  truncated: false,
};

const selection: Selection = {
  id: 'sel-1',
  name: 'Frontend',
  provider: 'codex',
  items: [{ capability: 'cap-design', revision: null }],
  created_at: 1,
  updated_at: 1,
};

function fakeClient(overrides: Partial<ArsenalClient> = {}): ArsenalClient {
  return {
    discover: vi.fn(async () => discovered),
    capabilities: vi.fn(async () => [design]),
    capability: vi.fn(async (_id: string, revision: number | null) =>
      detail(revision ?? 2),
    ),
    file: vi.fn(async (_id: string, _revision: number, path: string) => ({
      path,
      text: '# design',
    })),
    importSkill: vi.fn(async () => ({
      capability: 'cap-design',
      revision: 1,
      outcome: 'created' as const,
    })),
    restore: vi.fn(async () => detail(1)),
    presets: vi.fn(async () => [] as Preset[]),
    savePreset: vi.fn(async () => ({
      id: 'p',
      name: 'x',
      description: '',
      items: [],
      created_at: 1,
      updated_at: 1,
    })),
    deletePreset: vi.fn(async () => undefined),
    selections: vi.fn(async () => [selection]),
    saveSelection: vi.fn(async () => selection),
    deleteSelection: vi.fn(async () => undefined),
    candidateDetail: vi.fn(async (id: string) => ({
      id,
      files: [],
      excluded: [],
      review: [],
    })),
    preview: vi.fn(async (): Promise<Preview> => ({
      selection,
      status: 'not_applied',
      items: [
        {
          capability: 'cap-design',
          state: 'resolved',
          kind: 'skill',
          name: 'design',
          provider: 'claude_code',
          revision: 2,
          pinned: false,
          tree_hash: 'c'.repeat(64),
          compatibility: 'unsupported',
          reason: 'Built for claude_code; this selection is for codex.',
        },
      ],
      conflicts: [],
      dependencies: 'not_analysed',
    })),
    ...overrides,
  } as ArsenalClient;
}

/** Lets the reads a tab starts finish, without waiting for a timer the code does not own. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

function controllerWith(client: ArsenalClient) {
  const states: ArsenalState[] = [];
  const controller = new ArsenalController(
    (state) => states.push(state),
    client,
  );
  return { controller, last: () => states[states.length - 1] };
}

describe('Arsenal controller', () => {
  it('reads the library and reports an unavailable store as unavailable, not as empty', async () => {
    const ok = controllerWith(fakeClient());
    await ok.controller.refresh();
    expect(ok.last().phase).toBe('ready');
    expect(ok.last().library).toEqual([design]);

    const broken = controllerWith(
      fakeClient({
        capabilities: vi.fn(async () => {
          throw new ArsenalCallError({
            code: 'storage_unavailable',
            reason: null,
          });
        }),
      }),
    );
    await broken.controller.refresh();
    expect(broken.last().phase).toBe('unavailable');
    expect(broken.last().library).toEqual([]);
    expect(broken.last().notice?.tone).toBe('error');
    expect(broken.last().notice?.text).toContain('could not be opened');
  });

  it('imports a discovered capability, says what changed, and refreshes the scan and the library', async () => {
    const client = fakeClient();
    const { controller, last } = controllerWith(client);
    await controller.discover();
    await controller.importCandidate('id-design');
    expect(client.importSkill).toHaveBeenCalledWith('id-design');
    expect(last().notice).toEqual({
      tone: 'ok',
      text: 'Imported design as revision 1.',
    });
    expect(client.discover).toHaveBeenCalledTimes(2);
    expect(client.capabilities).toHaveBeenCalled();
  });

  it('says nothing changed when the same content is imported again', async () => {
    const client = fakeClient({
      importSkill: vi.fn(async () => ({
        capability: 'cap-design',
        revision: 2,
        outcome: 'unchanged' as const,
      })),
    });
    const { controller, last } = controllerWith(client);
    await controller.discover();
    await controller.importCandidate('id-design');
    expect(last().notice?.text).toBe(
      'design is already imported at revision 2. Nothing changed.',
    );
  });

  it('explains a blocked import with the reason the scan found', async () => {
    const client = fakeClient({
      importSkill: vi.fn(async () => {
        throw new ArsenalCallError({ code: 'blocked', reason: 'needs_secret' });
      }),
    });
    const { controller, last } = controllerWith(client);
    await controller.discover();
    await controller.importCandidate('id-design');
    expect(last().notice?.tone).toBe('error');
    expect(last().notice?.text).toContain('looks like a secret');
    expect(last().busy).toBeNull();
  });

  it('restores an earlier revision and says that no revision was created', async () => {
    const client = fakeClient();
    const { controller, last } = controllerWith(client);
    await controller.open('cap-design');
    await controller.restore(1);
    expect(client.restore).toHaveBeenCalledWith('cap-design', 1);
    expect(last().notice?.text).toBe(
      'Revision 1 is current again. No new revision was made.',
    );
  });

  it('opens a file of the shown revision and clears it when another capability opens', async () => {
    const client = fakeClient();
    const { controller, last } = controllerWith(client);
    await controller.open('cap-design');
    await controller.showFile('SKILL.md');
    expect(client.file).toHaveBeenCalledWith('cap-design', 2, 'SKILL.md');
    expect(last().file?.text).toBe('# design');
    await controller.open('cap-design', 1);
    expect(last().file).toBeNull();
    expect(last().opened?.revision).toBe(1);
  });

  it('saves presets and selections and reloads them; a selection is never reported as applied', async () => {
    const client = fakeClient();
    const { controller, last } = controllerWith(client);
    controller.setTab('selections');
    await settle();
    expect(last().selections).toEqual([selection]);
    const saved = await controller.saveSelection({
      id: null,
      name: 'Frontend',
      provider: 'codex',
      items: [{ capability: 'cap-design', revision: null }],
    });
    expect(saved).toBe(true);
    expect(last().notice?.text).toBe(
      'Saved selection Frontend. It is not applied.',
    );
    expect(client.selections).toHaveBeenCalledTimes(2);
  });

  it('reports a preset save the library refused and keeps the draft', async () => {
    const client = fakeClient({
      savePreset: vi.fn(async () => {
        throw new ArsenalCallError({ code: 'not_found', reason: null });
      }),
    });
    const { controller, last } = controllerWith(client);
    const saved = await controller.savePreset({
      id: null,
      name: 'Frontend',
      description: '',
      items: [{ capability: 'gone', revision: null }],
    });
    expect(saved).toBe(false);
    expect(last().notice?.text).toContain('no longer in the library');
  });

  it('loads a configuration preview exactly as the desktop states it', async () => {
    const { controller, last } = controllerWith(fakeClient());
    await controller.loadPreview('sel-1');
    expect(last().preview?.status).toBe('not_applied');
    expect(last().preview?.dependencies).toBe('not_analysed');
    expect(last().preview?.items[0].compatibility).toBe('unsupported');
  });
});

describe('Arsenal contracts', () => {
  it('rejects a reply that is not the shape the desktop promises', () => {
    expect(() => parseSummaries([{ id: 'x' }])).toThrow(ArsenalShapeError);
    expect(() =>
      parseDiscover({
        project: null,
        sources: [{ source: 's', state: 'maybe' }],
        candidates: [],
        composites: [],
        truncated: false,
      }),
    ).toThrow(ArsenalShapeError);
    expect(() => parseDiscover('not an object')).toThrow(ArsenalShapeError);
  });

  it('keeps read-only entries, composites and the project name the window shows', () => {
    const reply = parseDiscover({
      project: 'app',
      sources: [],
      candidates: [
        {
          ...candidate,
          id: 'id-hook',
          kind: 'hook',
          name: 'pre-commit',
          status: 'read_only',
          importable: false,
          compatibility: 'unsupported',
          blocked: 'read_only_kind',
        },
        {
          ...candidate,
          id: 'id-gsd',
          name: 'gsd-help',
          importable: false,
          blocked: 'symbolic_link',
          managed_by: {
            composite: 'GSD Core',
            version: '1.15.0',
            artifacts: 1950,
          },
        },
      ],
      composites: [
        {
          name: 'GSD Core 1.15.0',
          version: '1.15.0',
          artifacts: 1950,
          members: 1,
          origin: '~/.local/share/wyss/gsd-core/1.15.0',
        },
      ],
      truncated: false,
    });
    expect(reply.project).toBe('app');
    expect(reply.candidates[0].status).toBe('read_only');
    expect(reply.candidates[1].managed_by?.composite).toBe('GSD Core');
    expect(reply.composites[0].members).toBe(1);
  });

  it('keeps the failure code and the blocking reason of a refused call', () => {
    expect(
      parseImportFailure({ code: 'blocked', reason: 'needs_secret' }),
    ).toEqual({
      code: 'blocked',
      reason: 'needs_secret',
    });
    expect(parseImportFailure({ code: 'not_found' })).toEqual({
      code: 'not_found',
      reason: null,
    });
  });
});
