import { vi } from 'vitest';
import {
  parseOverview,
  parseProject,
  parseSession,
  parseWorkspace,
  type Envelope,
  type MigrationSummary,
  type Overview,
  type Page,
  type ProjectView,
  type Receipt,
  type SessionView,
  type WorkspaceView,
} from '../src/lib/contracts/projects';
import {
  ProjectsCallError,
  type ProjectsClient,
} from '../src/lib/desktop/projects-client';

export const HOST = 'aaaaaaaa-0000-4000-8000-000000000001';
export const SCRATCH = 'bbbbbbbb-0000-4000-8000-000000000001';
export const PROJECT_A = 'cccccccc-0000-4000-8000-00000000000a';
export const WORKSPACE_A = 'dddddddd-0000-4000-8000-00000000000a';
export const BINDING_A = 'eeeeeeee-0000-4000-8000-00000000000a';
export const LEAF_1 = 'ffffffff-0000-4000-8000-000000000001';
export const LEAF_2 = 'ffffffff-0000-4000-8000-000000000002';
export const ASSOCIATION_1 = '11111111-0000-4000-8000-000000000001';

export function overview(over: Record<string, unknown> = {}): Overview {
  return parseOverview({
    enabled: true,
    store: {
      state: 'ready',
      revision: '5',
      hostId: HOST,
      storeId: 'bbbbbbbb-0000-4000-8000-0000000000ff',
      scratchWorkspaceId: SCRATCH,
    },
    host: { reachable: true, epoch: 'h1:1:1:1:1', features: [], reason: null },
    ...over,
  });
}

export function project(over: Record<string, unknown> = {}): ProjectView {
  return parseProject({
    id: PROJECT_A,
    label: 'Alpha',
    archived: false,
    tombstone: false,
    revision: '1',
    defaultWorkspaceId: WORKSPACE_A,
    root: {
      bindingId: BINDING_A,
      bindingRevision: '3',
      label: 'alpha',
      state: 'valid',
      availability: 'available',
      nativeEligible: true,
      configEligible: true,
    },
    workspaceIds: [WORKSPACE_A],
    sessions: 0,
    ...over,
  });
}

export function session(over: Record<string, unknown> = {}): SessionView {
  return parseSession({
    id: ASSOCIATION_1,
    originProjectId: PROJECT_A,
    state: 'attached',
    kind: 'shell',
    availability: 'live',
    reason: null,
    nativeSessionId: null,
    terminalId: '22222222-0000-4000-8000-000000000001',
    protocol: null,
    storeStreamId: null,
    managedSessionId: null,
    requestedSafetyProfile: null,
    effectiveSafetyProfile: null,
    profileMismatch: false,
    operationId: null,
    input: 'none',
    ...over,
  });
}

export function workspace(over: Record<string, unknown> = {}): WorkspaceView {
  return parseWorkspace({
    id: WORKSPACE_A,
    projectId: PROJECT_A,
    label: 'Main',
    revision: '2',
    pinned: false,
    layout: { kind: 'leaf', id: LEAF_1 },
    focus: LEAF_1,
    leaves: [{ id: LEAF_1, title: 'Pane', session: null }],
    preferences: {
      provider: null,
      mode: 'native',
      nativeProfile: null,
      managedSafetyProfile: null,
    },
    unfinished: [],
    ...over,
  });
}

export const page = <T>(items: T[]): Page<T> => ({
  items,
  cursor: null,
  storeRevision: '5',
});

export function receipt(over: Partial<Receipt> = {}): Receipt {
  return {
    operationId: 'op',
    context: {
      hostId: HOST,
      projectId: null,
      workspaceId: null,
      bindingId: null,
      bindingRevision: null,
      selectionRevision: '1',
    },
    storeRevision: '6',
    created: [],
    replayed: false,
    ...over,
  };
}

export function failure(code: string, over: Record<string, unknown> = {}) {
  return new ProjectsCallError({
    code: code as never,
    effect: 'none',
    nextAction: 'none',
    operationId: null,
    context: null,
    field: null,
    ...over,
  });
}

export const summary = (
  over: Partial<MigrationSummary> = {},
): MigrationSummary => ({
  previewId: 'aaaaaaaa-0000-4000-8000-0000000000aa',
  mode: 'initial',
  counts: {
    projects: 2,
    workspaces: 3,
    sessions: 1,
    placedLeaves: 1,
    emptyLeaves: 1,
    tabsSkipped: 0,
  },
  conflicts: [],
  sources: [],
  canCommit: true,
  needsAliasAcceptance: false,
  legacyChanged: false,
  ...over,
});

export interface Fake extends ProjectsClient {
  sent: Envelope[];
  state: {
    overview: Overview;
    projects: ProjectView[];
    workspace: WorkspaceView;
    sessions: SessionView[];
    unassigned: SessionView[];
    executeError: ProjectsCallError | null;
    receipt: Receipt;
    output: {
      data: Uint8Array;
      next: number;
      eof: boolean;
      truncated: boolean;
    }[];
    claimError: ProjectsCallError | null;
  };
}

export function fakeClient(): Fake {
  const state: Fake['state'] = {
    overview: overview(),
    projects: [project()],
    workspace: workspace(),
    sessions: [],
    unassigned: [],
    executeError: null,
    receipt: receipt(),
    output: [],
    claimError: null,
  };
  const sent: Envelope[] = [];
  const client: Fake = {
    sent,
    state,
    overview: vi.fn(async () => state.overview),
    projects: vi.fn(async () => page(state.projects)),
    project: vi.fn(async () => state.projects[0]!),
    workspace: vi.fn(async () => state.workspace),
    sessions: vi.fn(async (project) =>
      page(project === null ? state.unassigned : state.sessions),
    ),
    chooseFolder: vi.fn(async () => ({
      token: 'pick-1',
      label: 'beta',
      nativeEligible: true,
      configEligible: true,
    })),
    forgetPicks: vi.fn(async () => undefined),
    execute: vi.fn(async (envelope: Envelope) => {
      sent.push(envelope);
      if (state.executeError) throw state.executeError;
      return state.receipt;
    }),
    initializeEmpty: vi.fn(async () => state.overview.store),
    previewMigration: vi.fn(async () => summary()),
    commitMigration: vi.fn(async () => state.overview.store),
    claimInput: vi.fn(async () => {
      if (state.claimError) throw state.claimError;
    }),
    releaseInput: vi.fn(async () => undefined),
    readOutput: vi.fn(async () => {
      const next = state.output.shift();
      if (next) return next;
      await new Promise((resolve) => setTimeout(resolve, 5));
      return { data: new Uint8Array(), next: 0, eof: false, truncated: false };
    }),
    writeInput: vi.fn(async () => undefined),
    resize: vi.fn(async () => undefined),
    rollbackReport: vi.fn(async () => ({
      canDisable: true,
      unfinishedLaunches: 0,
      projects: 1,
      workspaces: 2,
      sessions: 0,
      mappedLegacyRecords: 0,
      note: 'kept',
    })),
    compactReceipts: vi.fn(async () => 0),
    setEnabled: vi.fn(async () => state.overview),
    openWindow: vi.fn(async () => 'w-12345678'),
  };
  return client;
}
