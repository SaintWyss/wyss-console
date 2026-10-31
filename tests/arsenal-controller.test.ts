import { describe, expect, it, vi } from 'vitest';
import {
  ArsenalShapeError,
  parseBinding,
  parseDiscover,
  parseImportFailure,
  parseSummaries,
  parseTerminalPlan,
  type Candidate,
  type Detail,
  type DiscoverReply,
  type Preset,
  type Preview,
  type Selection,
  type LibraryItem,
} from '../src/lib/contracts/arsenal';
import {
  ArsenalCallError,
  type ArsenalClient,
} from '../src/lib/desktop/arsenal-client';
import {
  ArsenalController,
  type ArsenalState,
} from '../src/lib/features/arsenal/controller';

const design: LibraryItem = {
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
  compatibility: { status: 'supported', reason: 'own format' },
};

const detail = (revision: number): Detail => ({
  summary: { ...design, revision: 2 },
  compatibility: { status: 'supported', reason: 'own format' },
  dependencies: [],
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
  composites: [],
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
    bindings: vi.fn(async () => []),
    terminalPlan: vi.fn(async () => {
      throw new Error('not in this test');
    }),
    bindTerminal: vi.fn(async () => {
      throw new Error('not in this test');
    }),
    projectSelection: vi.fn(async () => null),
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
          compatibility: 'requires_qualification',
          reason: 'A Claude Code skill; Codex needs a qualified way in.',
          dependencies: [],
        },
      ],
      conflicts: [],
      dependencies: 'analysed',
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
    expect(client.importSkill).toHaveBeenCalledWith({
      id: 'id-design',
      kind: 'skill',
      source: 'claude_user_skills',
      name: 'design',
    });
    expect(last().notice).toEqual({
      tone: 'ok',
      text: 'Imported design as revision 1.',
    });
    expect(client.discover).toHaveBeenCalledTimes(2);
    expect(client.capabilities).toHaveBeenCalled();
  });

  it('refuses to import an entry the last scan did not list, and sends nothing to the desktop', async () => {
    const client = fakeClient();
    const { controller, last } = controllerWith(client);
    await controller.discover();
    await controller.importCandidate('id-not-listed');
    expect(client.importSkill).not.toHaveBeenCalled();
    expect(last().notice?.tone).toBe('error');
    expect(last().busy).toBeNull();
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

  describe('bulk import', () => {
    const entry = (
      id: string,
      name: string,
      patch: Partial<Candidate> = {},
    ): Candidate => ({ ...candidate, id, name, ...patch });

    const batch: DiscoverReply = {
      ...discovered,
      candidates: [
        entry('id-new', 'alpha'),
        entry('id-changed', 'beta', { status: 'source_changed' }),
        entry('id-done', 'gamma', { status: 'imported', capability: 'cap-g' }),
        entry('id-hook', 'pre-commit', {
          kind: 'hook',
          status: 'read_only',
          importable: false,
          blocked: 'read_only_kind',
        }),
        entry('id-secret', 'delta', {
          importable: false,
          blocked: 'needs_secret',
        }),
        entry('id-fails', 'epsilon'),
      ],
    };

    it('imports only eligible entries, counts each outcome and rescans once at the end', async () => {
      const outcomes = ['created', 'new_revision', 'created'] as const;
      let call = 0;
      const client = fakeClient({
        discover: vi.fn(async () => batch),
        importSkill: vi.fn(async () => ({
          capability: 'cap',
          revision: 1,
          outcome: outcomes[call++ % outcomes.length],
        })),
      });
      const { controller, last } = controllerWith(client);
      await controller.discover();
      await controller.importEligible();
      expect(client.importSkill).toHaveBeenCalledTimes(3);
      expect(client.importSkill).toHaveBeenCalledWith({
        id: 'id-new',
        kind: 'skill',
        source: 'claude_user_skills',
        name: 'alpha',
      });
      expect(client.importSkill).not.toHaveBeenCalledWith(
        expect.objectContaining({ id: 'id-hook' }),
      );
      expect(client.importSkill).not.toHaveBeenCalledWith(
        expect.objectContaining({ id: 'id-done' }),
      );
      expect(last().bulk).toMatchObject({
        total: 3,
        done: 3,
        created: 2,
        newRevision: 1,
        unchanged: 0,
        failures: [],
        stopped: null,
        finished: true,
      });
      expect(last().notice).toEqual({
        tone: 'ok',
        text: 'Bulk import: 2 imported, 1 updated to a new revision, 0 already in the library, unchanged.',
      });
      expect(last().busy).toBeNull();
      // One scan at the start, one after the run: the run itself never rescans.
      expect(client.discover).toHaveBeenCalledTimes(2);
    });

    it('records a failed entry and keeps importing the rest', async () => {
      const client = fakeClient({
        discover: vi.fn(async () => batch),
        importSkill: vi.fn(async (target: { id: string }) => {
          if (target.id === 'id-new') {
            throw new ArsenalCallError({
              code: 'blocked',
              reason: 'tree_too_large',
            });
          }
          return {
            capability: 'cap',
            revision: 1,
            outcome: 'created' as const,
          };
        }),
      });
      const { controller, last } = controllerWith(client);
      await controller.discover();
      await controller.importEligible();
      expect(client.importSkill).toHaveBeenCalledTimes(3);
      expect(last().bulk?.failures).toEqual([
        {
          name: 'alpha',
          text: expect.stringContaining('larger than the import limit'),
        },
      ]);
      expect(last().bulk?.created).toBe(2);
      expect(last().notice?.tone).toBe('error');
      expect(last().notice?.text).toBe(
        'Bulk import: 2 imported, 0 updated to a new revision, 0 already in the library, unchanged, 1 failed.',
      );
    });

    it('keeps going past a busy library, because the next entry can still succeed', async () => {
      const client = fakeClient({
        discover: vi.fn(async () => batch),
        importSkill: vi.fn(async () => {
          throw new ArsenalCallError({ code: 'storage_busy', reason: null });
        }),
      });
      const { controller, last } = controllerWith(client);
      await controller.discover();
      await controller.importEligible();
      expect(client.importSkill).toHaveBeenCalledTimes(3);
      expect(last().bulk?.failures).toHaveLength(3);
      expect(last().bulk?.stopped).toBeNull();
      expect(last().busy).toBeNull();
    });

    it('stops at once and says so when the library cannot be opened', async () => {
      const client = fakeClient({
        discover: vi.fn(async () => batch),
        importSkill: vi.fn(async () => {
          throw new ArsenalCallError({
            code: 'storage_unavailable',
            reason: null,
          });
        }),
      });
      const { controller, last } = controllerWith(client);
      await controller.discover();
      await controller.importEligible();
      expect(client.importSkill).toHaveBeenCalledTimes(1);
      expect(last().bulk?.stopped).toContain('could not be opened');
      expect(last().notice?.text).toContain(
        'Stopped: The Arsenal library could not be opened',
      );
    });

    it('says nothing is left to import without calling the desktop', async () => {
      const client = fakeClient({
        discover: vi.fn(async () => ({
          ...discovered,
          candidates: [entry('id-done', 'gamma', { status: 'imported' })],
        })),
      });
      const { controller, last } = controllerWith(client);
      await controller.discover();
      await controller.importEligible();
      expect(client.importSkill).not.toHaveBeenCalled();
      expect(last().notice).toEqual({
        tone: 'ok',
        text: 'Nothing to import: every eligible skill is already in the library.',
      });
    });
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
      composites: [],
    });
    expect(saved).toBe(true);
    expect(last().notice?.text).toBe(
      'Saved selection Frontend. It is not applied.',
    );
    expect(client.selections).toHaveBeenCalledTimes(2);
  });

  it('drops the preview of a selection once it is saved, so no preview shows the version before the save', async () => {
    const { controller, last } = controllerWith(fakeClient());
    await controller.loadPreview('sel-1');
    expect(last().preview?.selection.id).toBe('sel-1');
    await controller.saveSelection({
      id: 'sel-1',
      name: 'Frontend',
      provider: 'codex',
      items: [],
      composites: [],
    });
    expect(last().preview).toBeNull();
    expect(last().notice?.text).toBe(
      'Saved selection Frontend. It is not applied.',
    );
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
    expect(last().preview?.dependencies).toBe('analysed');
    expect(last().preview?.items[0].compatibility).toBe(
      'requires_qualification',
    );
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
          key: 'gsd_core@1.15.0',
          kind: 'gsd_core',
          provider: 'claude_code',
          name: 'GSD Core 1.15.0',
          version: '1.15.0',
          description: '',
          artifacts: 1950,
          members: 1,
          origin: '~/.local/share/wyss/gsd-core/1.15.0',
          enabled: null,
          methodology: true,
          skills: ['gsd-help'],
          agents: [],
          commands: 0,
          hooks: false,
          mcp: false,
          categories: [['skill', 216]],
          limitations: ['Installed by GSD Core as links.'],
        },
      ],
      truncated: false,
    });
    expect(reply.project).toBe('app');
    expect(reply.composites[0].categories).toEqual([['skill', 216]]);
    expect(reply.candidates[0].status).toBe('read_only');
    expect(reply.candidates[1].managed_by?.composite).toBe('GSD Core');
    expect(reply.composites[0].members).toBe(1);
  });

  it('reads a terminal plan and a binding, and refuses a plan that claims more than it can', () => {
    const plan = {
      selection: 'sel-1',
      selection_name: 'Frontend',
      provider: 'claude_code',
      provider_version: '2.1.314 (Claude Code)',
      project: 'app',
      status: 'ready',
      can_apply: false,
      enforcement: {
        state: 'unqualified',
        reason: 'Skill exclusion is not qualified.',
        evidence: null,
      },
      items: [
        {
          capability: 'cap-design',
          name: 'design',
          kind: 'skill',
          provider: 'claude_code',
          revision: 2,
          pinned: true,
          tree_hash: 'c'.repeat(64),
          state: 'prepared',
          reason: 'The installed copy is exactly revision 2.',
          compatibility: { status: 'supported', reason: 'own format' },
          dependencies: [{ kind: 'tool', name: 'Read', state: 'declared' }],
        },
      ],
      kept: ['design'],
      excluded: ['tokens'],
      inherited_off: [],
      excluded_files: [],
      untouched: ['Hooks, permissions and MCP servers: as your settings say.'],
      composites: [],
      conflicts: [],
      warnings: [],
      overlay: '{"skillOverrides":{"tokens":"off"}}',
      config_hash: 'e'.repeat(64),
    };
    const parsed = parseTerminalPlan(plan);
    expect(parsed.enforcement.state).toBe('unqualified');
    expect(parsed.items[0].dependencies[0].state).toBe('declared');
    expect(() =>
      parseTerminalPlan({ ...plan, enforcement: { state: 'verified' } }),
    ).toThrow(ArsenalShapeError);
    const binding = parseBinding({
      session_id: 'a'.repeat(32),
      selection_id: 'sel-1',
      selection_name: 'Frontend',
      provider: 'claude_code',
      provider_version: null,
      project: '/home/u/app',
      config_hash: 'e'.repeat(64),
      state: 'not_applied',
      reason: 'Skill exclusion is not qualified.',
      plan,
      created_at: 1,
      verification: 'not_observed',
    });
    expect(binding.plan?.excluded).toEqual(['tokens']);
    // A binding never says a configuration was verified.
    expect(() =>
      parseBinding({
        ...binding,
        plan,
        state: 'applied',
        verification: 'verified',
      }),
    ).toThrow(ArsenalShapeError);
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
