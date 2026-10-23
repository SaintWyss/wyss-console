import { invoke } from '@tauri-apps/api/core';
import {
  parseAdoptionPreview,
  parseFailure,
  parseOrphans,
  parseOutput,
  parseOverview,
  parsePage,
  parsePick,
  parseProject,
  parseReceipt,
  parseRollback,
  parseSession,
  parseStoreStatus,
  parseSummary,
  parseWorkspace,
  type AdoptionPreview,
  type Envelope,
  type Failure,
  type MigrationSummary,
  type Orphan,
  type Output,
  type Overview,
  type Page,
  type PickView,
  type ProjectView,
  type Receipt,
  type RollbackReport,
  type SessionView,
  type StoreStatus,
  type WorkspaceView,
} from '../contracts/projects';

export class ProjectsCallError extends Error {
  constructor(readonly failure: Failure) {
    super(`projects: ${failure.code}`);
  }
}

/** The window's calls. The desktop's own, unless a test brings another. */
export interface ProjectsClient {
  overview(): Promise<Overview>;
  projects(cursor: string | null): Promise<Page<ProjectView>>;
  project(id: string): Promise<ProjectView>;
  workspace(id: string): Promise<WorkspaceView>;
  sessions(
    project: string | null | undefined,
    cursor: string | null,
  ): Promise<Page<SessionView>>;
  chooseFolder(
    purpose: 'register' | 'parent' | 'relink',
    selectionRevision: string,
  ): Promise<PickView | null>;
  forgetPicks(): Promise<void>;
  execute(envelope: Envelope): Promise<Receipt>;
  initializeEmpty(): Promise<StoreStatus>;
  previewMigration(): Promise<MigrationSummary>;
  commitMigration(
    previewId: string,
    acceptAliases: boolean,
  ): Promise<StoreStatus>;
  claimInput(associationId: string): Promise<void>;
  releaseInput(associationId: string): Promise<void>;
  readOutput(
    associationId: string,
    from: number,
    waitMs: number,
  ): Promise<Output>;
  writeInput(associationId: string, data: Uint8Array): Promise<void>;
  resize(associationId: string, rows: number, cols: number): Promise<void>;
  orphans(launchOperationId: string): Promise<Orphan[]>;
  /** Where the shell really is now; the answer is good for one adoption. */
  adoptionPreview(
    associationId: string,
    projectId: string,
  ): Promise<AdoptionPreview>;
  rollbackReport(): Promise<RollbackReport>;
  compactReceipts(): Promise<number>;
  setEnabled(on: boolean): Promise<Overview>;
  openWindow(): Promise<string>;
}

async function call(
  command: string,
  args: Record<string, unknown> = {},
): Promise<unknown> {
  try {
    return await invoke<unknown>(command, args);
  } catch (error) {
    throw new ProjectsCallError(parseFailure(error));
  }
}

function checked<T>(answer: unknown, parse: (v: unknown) => T): T {
  try {
    return parse(answer);
  } catch {
    // The desktop answered something that does not fit: nothing is believed.
    throw new ProjectsCallError(parseFailure(null));
  }
}

export const desktopProjects: ProjectsClient = {
  async overview() {
    return checked(await call('projects_overview'), parseOverview);
  },
  async projects(cursor) {
    return checked(await call('projects_list', { request: { cursor } }), (v) =>
      parsePage(v, parseProject),
    );
  },
  async project(id) {
    return checked(
      await call('projects_project', { request: { id } }),
      parseProject,
    );
  },
  async workspace(id) {
    return checked(
      await call('projects_workspace', { request: { id } }),
      parseWorkspace,
    );
  },
  async sessions(project, cursor) {
    const request = project === undefined ? { cursor } : { project, cursor };
    return checked(await call('projects_sessions', { request }), (v) =>
      parsePage(v, parseSession),
    );
  },
  async chooseFolder(purpose, selectionRevision) {
    const answer = await call('projects_choose_folder', {
      request: { purpose, selectionRevision: Number(selectionRevision) },
    });
    return answer === null ? null : checked(answer, parsePick);
  },
  async forgetPicks() {
    await call('projects_forget_picks');
  },
  async execute(envelope) {
    return checked(
      await call('projects_change', { request: envelope }),
      parseReceipt,
    );
  },
  async initializeEmpty() {
    return checked(await call('projects_initialize_empty'), parseStoreStatus);
  },
  async previewMigration() {
    return checked(await call('projects_migration_preview'), parseSummary);
  },
  async commitMigration(previewId, acceptAliases) {
    return checked(
      await call('projects_migration_commit', {
        request: { previewId, acceptAliases },
      }),
      parseStoreStatus,
    );
  },
  async claimInput(associationId) {
    await call('projects_input_claim', { request: { associationId } });
  },
  async releaseInput(associationId) {
    await call('projects_input_release', { request: { associationId } });
  },
  async readOutput(associationId, from, waitMs) {
    return checked(
      await call('projects_term_read', {
        request: { associationId, from, waitMs },
      }),
      parseOutput,
    );
  },
  async writeInput(associationId, data) {
    await call('projects_term_write', {
      request: { associationId, data: Array.from(data) },
    });
  },
  async resize(associationId, rows, cols) {
    await call('projects_term_resize', {
      request: { associationId, rows, cols },
    });
  },
  async orphans(launchOperationId) {
    return checked(
      await call('projects_orphans', { request: { launchOperationId } }),
      parseOrphans,
    );
  },
  async adoptionPreview(associationId, projectId) {
    return checked(
      await call('projects_adoption_preview', {
        request: { associationId, projectId },
      }),
      parseAdoptionPreview,
    );
  },
  async rollbackReport() {
    return checked(await call('projects_rollback_report'), parseRollback);
  },
  async compactReceipts() {
    const answer = await call('projects_compact_receipts');
    return typeof answer === 'number' ? answer : 0;
  },
  async setEnabled(on) {
    return checked(
      await call('projects_set_enabled', { request: { on } }),
      parseOverview,
    );
  },
  async openWindow() {
    const answer = await call('projects_open_window');
    if (typeof answer !== 'string')
      throw new ProjectsCallError(parseFailure(null));
    return answer;
  },
};
