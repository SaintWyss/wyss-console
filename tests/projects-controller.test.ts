import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Receipt } from '../src/lib/contracts/projects';
import { ProjectsController } from '../src/lib/features/projects/controller';
import {
  BINDING_A,
  HOST,
  LEAF_1,
  LEAF_2,
  PROJECT_A,
  SCRATCH,
  WORKSPACE_A,
  failure,
  fakeClient,
  overview,
  project,
  receipt,
  session,
  workspace,
  type Fake,
} from './projects-harness';

let ids = 0;
const newId = () =>
  `99999999-0000-4000-8000-${String(++ids).padStart(12, '0')}`;

function make(client: Fake = fakeClient()) {
  const changes: unknown[] = [];
  const controller = new ProjectsController(
    (s) => changes.push(s),
    client,
    { poll: 20, save: 400 },
    newId,
  );
  return { client, controller, changes };
}

const settle = async () => {
  for (let i = 0; i < 30; i++) await Promise.resolve();
};

beforeEach(() => {
  ids = 0;
});
afterEach(() => vi.useRealTimers());

describe('what is read', () => {
  it('reads nothing but the overview while the workspace is off', async () => {
    const client = fakeClient();
    client.state.overview = overview({ enabled: false });
    const { controller } = make(client);
    await controller.refresh();
    expect(controller.state().phase).toBe('ready');
    expect(client.projects).not.toHaveBeenCalled();
    expect(client.workspace).not.toHaveBeenCalled();
    expect(client.sessions).not.toHaveBeenCalled();
  });

  it('reads the store, the project in view and its sessions once it is on', async () => {
    const { controller, client } = make();
    await controller.refresh();
    const s = controller.state();
    expect(s.projects).toHaveLength(1);
    // Nothing selected is Scratch.
    expect(s.workspaceId).toBe(SCRATCH);
    expect(client.workspace).toHaveBeenCalledWith(SCRATCH);
    expect(s.revision).toBe('5');
  });

  it('does not show an alert when a read fails: nothing was asked for', async () => {
    const client = fakeClient();
    client.overview = vi.fn(async () => {
      throw failure('io_failure');
    });
    const { controller } = make(client);
    await controller.refresh();
    expect(controller.state().phase).toBe('failed');
    expect(controller.state().notice).toBeNull();
  });
});

describe('selecting is presentation only', () => {
  it('moves to a project without sending a change, and forgets pending folder picks', async () => {
    const { controller, client } = make();
    await controller.refresh();
    await controller.selectProject(PROJECT_A);
    expect(controller.state().project).toBe(PROJECT_A);
    expect(controller.state().workspaceId).toBe(WORKSPACE_A);
    expect(client.execute).not.toHaveBeenCalled();
    expect(client.forgetPicks).toHaveBeenCalledTimes(1);
  });

  it('names a new selection revision in what it sends next, so an older pick cannot land', async () => {
    const { controller, client } = make();
    await controller.refresh();
    await controller.selectProject(PROJECT_A);
    await controller.rename(PROJECT_A, 'One');
    await controller.selectProject(null);
    await controller.selectProject(PROJECT_A);
    await controller.rename(PROJECT_A, 'Two');
    const [first, second] = client.sent;
    expect(Number(second!.context.selectionRevision)).toBeGreaterThan(
      Number(first!.context.selectionRevision),
    );
  });

  it('falls back to Scratch when the project in view is gone', async () => {
    const { controller, client } = make();
    await controller.refresh();
    await controller.selectProject(PROJECT_A);
    client.state.projects = [];
    await controller.refresh();
    expect(controller.state().project).toBeNull();
    expect(controller.state().workspaceId).toBe(SCRATCH);
  });
});

describe('every change is a closed envelope in the context the window was looking at', () => {
  it('names the host, project, workspace, binding and its revision, and the store revision', async () => {
    const { controller, client } = make();
    await controller.refresh();
    await controller.selectProject(PROJECT_A);
    await controller.addPane(null, 'row');
    const sent = client.sent[0]!;
    expect(sent).toMatchObject({
      schema: 1,
      expectedStoreRevision: '5',
      context: {
        hostId: HOST,
        projectId: PROJECT_A,
        workspaceId: WORKSPACE_A,
        bindingId: BINDING_A,
        bindingRevision: '3',
      },
    });
    expect(sent.operationId).toMatch(/^[0-9a-f-]{36}$/);
    expect(Object.keys(sent).sort()).toEqual([
      'command',
      'context',
      'expectedStoreRevision',
      'operationId',
      'schema',
    ]);
  });

  it('gives each change an operation ID of its own', async () => {
    const { controller, client } = make();
    await controller.refresh();
    await controller.selectProject(PROJECT_A);
    await controller.rename(PROJECT_A, 'a');
    await controller.rename(PROJECT_A, 'b');
    expect(new Set(client.sent.map((e) => e.operationId)).size).toBe(2);
  });

  it('refreshes after a refusal, says so, and does not send the change again', async () => {
    const { controller, client } = make();
    await controller.refresh();
    await controller.selectProject(PROJECT_A);
    client.state.executeError = failure('revision_conflict', {
      nextAction: 'refetch',
    });
    const before = vi.mocked(client.overview).mock.calls.length;
    const answer = await controller.rename(PROJECT_A, 'x');
    expect('code' in answer && answer.code).toBe('revision_conflict');
    expect(client.execute).toHaveBeenCalledTimes(1);
    expect(vi.mocked(client.overview).mock.calls.length).toBeGreaterThan(
      before,
    );
    expect(controller.state().notice?.text).toMatch(/refreshed/i);
  });

  it('never sends a launch again by itself when the outcome is unknown', async () => {
    const { controller, client } = make();
    await controller.refresh();
    await controller.selectProject(PROJECT_A);
    client.state.executeError = failure('effect_ambiguous', {
      effect: 'unknown',
      nextAction: 'reconcile',
    });
    await controller.launch({
      leafId: LEAF_1,
      provider: 'codex',
      mode: 'native',
      nativeProfile: null,
      managedSafetyProfile: null,
    });
    await settle();
    expect(client.execute).toHaveBeenCalledTimes(1);
    expect(controller.state().notice?.failure?.effect).toBe('unknown');
  });

  it('refuses a second command while one is in flight', async () => {
    const { controller, client } = make();
    await controller.refresh();
    await controller.selectProject(PROJECT_A);
    let release!: () => void;
    client.execute = vi.fn(
      () =>
        new Promise<Receipt>((resolve) => {
          release = () => resolve(receipt());
        }),
    );
    const first = controller.rename(PROJECT_A, 'a');
    await settle();
    const second = await controller.rename(PROJECT_A, 'b');
    expect('code' in second && second.code).toBe('leaf_busy');
    release();
    await first;
    expect(client.execute).toHaveBeenCalledTimes(1);
  });

  it('sends both profile fields apart, and a Native launch carries no Managed profile', async () => {
    const { controller, client } = make();
    await controller.refresh();
    await controller.selectProject(PROJECT_A);
    await controller.launch({
      leafId: LEAF_1,
      provider: 'claude',
      mode: 'native',
      nativeProfile: { id: 'a'.repeat(32), version: '2' },
      managedSafetyProfile: null,
    });
    await controller.launch({
      leafId: LEAF_1,
      provider: 'codex',
      mode: 'managed',
      nativeProfile: null,
      managedSafetyProfile: 'inference',
    });
    const [native, managed] = client.sent.map((e) => e.command);
    expect(native).toMatchObject({
      kind: 'launch',
      mode: 'native',
      nativeProfile: { version: '2' },
      managedSafetyProfile: null,
    });
    expect(managed).toMatchObject({
      kind: 'launch',
      mode: 'managed',
      nativeProfile: null,
      managedSafetyProfile: 'inference',
    });
  });
});

describe('a session this window started', () => {
  it('is remembered, so its pane takes input at once, and forgotten when the view moves', async () => {
    const { controller, client } = make();
    await controller.refresh();
    await controller.selectProject(PROJECT_A);
    client.state.receipt = receipt({
      created: [{ role: 'association', id: session().id }],
    });
    await controller.launch({
      leafId: LEAF_1,
      provider: 'shell',
      mode: 'native',
      nativeProfile: null,
      managedSafetyProfile: null,
    });
    expect(controller.state().launched).toBe(session().id);
    await controller.selectWorkspace(WORKSPACE_A);
    expect(controller.state().launched).toBeNull();
    // A change that created no session does not mark one.
    await controller.rename(PROJECT_A, 'x');
    expect(controller.state().launched).toBeNull();
  });
});

describe('panes', () => {
  const withTwo = () => {
    const client = fakeClient();
    client.state.workspace = workspace({
      layout: {
        kind: 'split',
        dir: 'row',
        ratio: 0.5,
        a: { kind: 'leaf', id: LEAF_1 },
        b: { kind: 'leaf', id: LEAF_2 },
      },
      leaves: [
        { id: LEAF_1, title: 'Pane', session: session() },
        { id: LEAF_2, title: 'Pane', session: null },
      ],
    });
    return make(client);
  };

  it('adds a pane beside the focused one and keeps the sessions the others show', async () => {
    const { controller, client } = withTwo();
    await controller.refresh();
    await controller.selectProject(PROJECT_A);
    await controller.addPane(LEAF_2, 'col');
    const command = client.sent[0]!.command;
    if (command.kind !== 'saveWorkspace') throw new Error('save');
    expect(command.leaves).toHaveLength(3);
    expect(command.leaves[0]).toMatchObject({
      id: LEAF_1,
      associationId: session().id,
    });
    expect(command.leaves[1]!.associationId).toBeNull();
    expect(command.expectedWorkspaceRevision).toBe('2');
    expect(command.layout).toMatchObject({ kind: 'split' });
  });

  it('removes only an empty pane; a pane that shows a session is detached first', async () => {
    const { controller, client } = withTwo();
    await controller.refresh();
    await controller.selectProject(PROJECT_A);
    const refused = await controller.closePane(LEAF_1);
    expect('code' in refused && refused.code).toBe('leaf_busy');
    expect(client.sent).toHaveLength(0);
    await controller.closePane(LEAF_2);
    const command = client.sent[0]!.command;
    if (command.kind !== 'saveWorkspace') throw new Error('save');
    expect(command.layout).toEqual({ kind: 'leaf', id: LEAF_1 });
    expect(command.leaves.map((l) => l.id)).toEqual([LEAF_1]);
  });

  it('keeps a resized ratio inside the allowed range', async () => {
    const { controller, client } = withTwo();
    await controller.refresh();
    await controller.selectProject(PROJECT_A);
    controller.resizeSplit('', 0.99);
    await controller.flushLayout();
    const command = client.sent[0]!.command;
    if (command.kind !== 'saveWorkspace') throw new Error('save');
    expect(command.layout).toMatchObject({ ratio: 0.9 });
  });

  it('shows a moved divider at once and saves the latest ratio once, after the moves stop', async () => {
    const { controller, client } = withTwo();
    await controller.refresh();
    await controller.selectProject(PROJECT_A);
    vi.useFakeTimers();
    for (const ratio of [0.4, 0.45, 0.5, 0.55, 0.6]) {
      controller.resizeSplit('', ratio);
      await vi.advanceTimersByTimeAsync(100);
    }
    // Shown now, written to no store yet.
    expect(controller.state().workspace?.layout).toMatchObject({ ratio: 0.6 });
    expect(client.sent).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(400);
    expect(client.sent).toHaveLength(1);
    const command = client.sent[0]!.command;
    if (command.kind !== 'saveWorkspace') throw new Error('save');
    expect(command.layout).toMatchObject({ ratio: 0.6 });
    // Nothing is written again by itself.
    await vi.advanceTimersByTimeAsync(5000);
    expect(client.sent).toHaveLength(1);
  });

  it('writes a moved divider before anything else is sent, before the view goes quiet and before another is selected', async () => {
    for (const away of [
      async (c: ProjectsController) => {
        await c.rename(PROJECT_A, 'Next');
      },
      async (c: ProjectsController) => {
        await c.selectProject(null);
      },
      async (c: ProjectsController) => {
        c.setVisible(false);
        await settle();
      },
    ]) {
      const { controller, client } = withTwo();
      await controller.refresh();
      await controller.selectProject(PROJECT_A);
      controller.setVisible(true);
      await settle();
      client.sent.length = 0;
      controller.resizeSplit('', 0.7);
      expect(client.sent).toHaveLength(0);
      await away(controller);
      await settle();
      const kinds = client.sent.map((e) => e.command.kind);
      expect(kinds[0]).toBe('saveWorkspace');
      const first = client.sent[0]!.command;
      if (first.kind !== 'saveWorkspace') throw new Error('save');
      expect(first.layout).toMatchObject({ ratio: 0.7 });
      controller.dispose();
    }
  });

  it('does not let a read of the store undo a divider that is not saved yet', async () => {
    const { controller, client } = withTwo();
    await controller.refresh();
    await controller.selectProject(PROJECT_A);
    client.sent.length = 0;
    controller.resizeSplit('', 0.3);
    // A read arriving now saves it first and shows it after.
    await controller.refresh();
    expect(client.sent).toHaveLength(1);
    expect(controller.state().workspace?.layout).toBeTruthy();
  });
});

describe('adoption of the feature, import and rollback', () => {
  it('does not complain when the confirmation is declined', async () => {
    const client = fakeClient();
    client.setEnabled = vi.fn(async () => {
      throw failure('owner_refused', { field: 'declined' });
    });
    const { controller } = make(client);
    await controller.setEnabled(true);
    expect(controller.state().notice).toBeNull();
  });

  it('drops a preview the desktop no longer accepts and asks for a new one', async () => {
    const { controller, client } = make();
    await controller.previewMigration();
    expect(controller.state().preview?.canCommit).toBe(true);
    client.commitMigration = vi.fn(async () => {
      throw failure('context_stale', { field: 'sources_changed' });
    });
    await controller.commitMigration(false);
    expect(controller.state().preview).toBeNull();
    expect(controller.state().notice?.failure?.code).toBe('context_stale');
  });
});

describe('the one timer', () => {
  it('polls the overview only while the view is shown and reads everything again only when the revision moves', async () => {
    vi.useFakeTimers();
    const { controller, client } = make();
    controller.setVisible(true);
    await vi.advanceTimersByTimeAsync(5);
    const reads = vi.mocked(client.workspace).mock.calls.length;
    await vi.advanceTimersByTimeAsync(100);
    expect(vi.mocked(client.overview).mock.calls.length).toBeGreaterThan(2);
    expect(vi.mocked(client.workspace).mock.calls.length).toBe(reads);
    client.state.overview = overview({
      store: {
        state: 'ready',
        revision: '6',
        hostId: HOST,
        storeId: 'bbbbbbbb-0000-4000-8000-0000000000ff',
        scratchWorkspaceId: SCRATCH,
      },
    });
    await vi.advanceTimersByTimeAsync(60);
    expect(vi.mocked(client.workspace).mock.calls.length).toBeGreaterThan(
      reads,
    );
    controller.setVisible(false);
    const asked = vi.mocked(client.overview).mock.calls.length;
    await vi.advanceTimersByTimeAsync(200);
    expect(vi.mocked(client.overview).mock.calls.length).toBe(asked);
    controller.dispose();
  });
});

describe('project data on screen', () => {
  it('holds a root by name and availability and never a path', async () => {
    const client = fakeClient();
    client.state.projects = [project()];
    const { controller } = make(client);
    await controller.refresh();
    expect(JSON.stringify(controller.state())).not.toMatch(/\/home\//);
  });
});
