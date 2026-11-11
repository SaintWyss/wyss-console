import { invoke } from '@tauri-apps/api/core';
import {
  parseBinding,
  parseBindings,
  parseSyncStatus,
  parseTerminalPlan,
  parseCandidateDetail,
  parseDetail,
  parseDiscover,
  parseFileText,
  parseImportFailure,
  parseImported,
  parsePreset,
  parsePresets,
  parsePreview,
  parseSelection,
  parseSelections,
  parseSummaries,
  type ArsenalFailure,
  type ArsenalProvider,
  type Binding,
  type SyncStatus,
  type LibraryItem,
  type TerminalPlan,
  type CandidateDetail,
  type Detail,
  type FileText,
  type Imported,
  type Item,
  type Preset,
  type Preview,
  type Selection,
} from '../contracts/arsenal';
import type { AgentId } from '../contracts/native';

/** A command that failed, or answered with something that is not what it promises. */
export class ArsenalCallError extends Error {
  readonly code: string;
  readonly reason: string | null;

  constructor(failure: ArsenalFailure) {
    super(`arsenal: ${failure.code}`);
    this.code = failure.code;
    this.reason = failure.reason;
  }
}

async function call(
  command: string,
  args?: Record<string, unknown>,
): Promise<unknown> {
  try {
    return await invoke<unknown>(command, args);
  } catch (error) {
    let failure: ArsenalFailure;
    try {
      failure = parseImportFailure(error);
    } catch {
      failure = { code: 'internal', reason: null };
    }
    throw new ArsenalCallError(failure);
  }
}

/** Reads the answer of a command, and turns anything that is not its shape into `internal`. */
async function answer<T>(
  command: string,
  parse: (value: unknown) => T,
  args?: Record<string, unknown>,
): Promise<T> {
  const value = await call(command, args);
  try {
    return parse(value);
  } catch {
    throw new ArsenalCallError({ code: 'internal', reason: null });
  }
}

/** The entry an import names, as the last scan listed it. The backend reads that entry again. */
export interface ImportTarget {
  id: string;
  kind: string;
  source: string;
  name: string;
}

/** Every request names something the library or a fresh scan already holds, never a path. */
export const arsenalClient = {
  discover: () => answer('arsenal_discover', parseDiscover),
  candidateDetail: (id: string) =>
    answer<CandidateDetail>('arsenal_candidate', parseCandidateDetail, {
      request: { id },
    }),
  capabilities: () =>
    answer<LibraryItem[]>('arsenal_capabilities', parseSummaries),
  capability: (capability: string, revision: number | null) =>
    answer<Detail>('arsenal_capability', parseDetail, {
      request: { capability, revision },
    }),
  file: (capability: string, revision: number, path: string) =>
    answer<FileText>('arsenal_file', parseFileText, {
      request: { capability, revision, path },
    }),
  importSkill: (target: ImportTarget) =>
    answer<Imported>('arsenal_import', parseImported, { request: target }),
  restore: (capability: string, revision: number) =>
    answer<Detail>('arsenal_restore', parseDetail, {
      request: { capability, revision },
    }),
  presets: () => answer<Preset[]>('arsenal_presets', parsePresets),
  savePreset: (input: {
    id: string | null;
    name: string;
    description: string;
    items: Item[];
  }) => answer<Preset>('arsenal_preset_write', parsePreset, { request: input }),
  deletePreset: (id: string) =>
    call('arsenal_preset_delete', { request: { id } }).then(() => undefined),
  selections: () => answer<Selection[]>('arsenal_selections', parseSelections),
  saveSelection: (input: {
    id: string | null;
    name: string;
    provider: ArsenalProvider;
    items: Item[];
    composites: string[];
  }) =>
    answer<Selection>('arsenal_selection_write', parseSelection, {
      request: input,
    }),
  deleteSelection: (id: string) =>
    call('arsenal_selection_delete', { request: { id } }).then(() => undefined),
  preview: (id: string) =>
    answer<Preview>('arsenal_preview', parsePreview, { request: { id } }),
  planPreset: (
    provider: ArsenalProvider,
    items: Item[],
    composites: string[],
  ) =>
    answer<Preview>('arsenal_preset_plan', parsePreview, {
      request: { provider, items, composites },
    }),
  groups: () =>
    answer<Record<string, string>>('arsenal_groups', (value) => {
      if (typeof value !== 'object' || value === null || Array.isArray(value))
        throw new Error('groups');
      const entries = Object.entries(value);
      if (entries.some(([, group]) => typeof group !== 'string'))
        throw new Error('group');
      return Object.fromEntries(entries) as Record<string, string>;
    }),
  moveGroup: (identity: string, group_id: string | null) =>
    call('arsenal_group_move', { request: { identity, group_id } }).then(
      () => undefined,
    ),
  /** The configuration one terminal would start with, computed now by the desktop. */
  terminalPlan: (
    selection: string,
    agent_id: AgentId,
    project: string | null,
  ) =>
    answer<TerminalPlan>('arsenal_terminal_plan', parseTerminalPlan, {
      request: { selection, agent_id, project },
    }),
  /** Records what a started terminal carries; the desktop reads that from the host itself. */
  bindTerminal: (session_id: string, selection: string, config_hash: string) =>
    answer<Binding>('arsenal_terminal_bind', parseBinding, {
      request: { session_id, selection, config_hash },
    }),
  bindings: () => answer<Binding[]>('arsenal_terminal_bindings', parseBindings),
  /** Synchronization with the owner's other computers. Reading it changes nothing. */
  syncStatus: () => answer<SyncStatus>('arsenal_sync_status', parseSyncStatus),
  /** Asks for a synchronization run now; answers the status before it. */
  syncNow: () => answer<SyncStatus>('arsenal_sync_now', parseSyncStatus),
  projectSelection: (project: string, agent_id: AgentId) =>
    answer<string | null>(
      'arsenal_project_selection',
      (value) => {
        if (value === null) return null;
        if (typeof value !== 'string') throw new Error('selection');
        return value;
      },
      { request: { project, agent_id } },
    ),
};

export type ArsenalClient = typeof arsenalClient;
