import { invoke } from '@tauri-apps/api/core';
import type { CustomCategory } from '../features/arsenal/presets';
import {
  parseOptions as parseEnvironmentOptions,
  parseSpec as parseEnvironment,
  type HomeSpec,
} from '../features/arsenal/environment';
import type { Section } from '../features/arsenal/groups';
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
  /** The launch environment a Claude Code preset chose (H1/H2). */
  environment: (selection: string) =>
    answer<HomeSpec>('arsenal_environment', parseEnvironment, { selection }),
  /** What a launch environment can choose from, read from the owner's ~/.claude now. */
  environmentOptions: () =>
    answer('arsenal_environment_options', parseEnvironmentOptions),
  writeEnvironment: (selection: string, environment: HomeSpec) =>
    call('arsenal_environment_write', { selection, environment }).then(
      () => undefined,
    ),
  /** Workspace roles: the owner's own and each launch's role, kept on this computer. */
  roles: () => answer<Roles>('arsenal_roles', parseRoles),
  writeRole: (change: RoleChange) =>
    answer<Roles>('arsenal_role_write', parseRoles, { change }),
  /** The categories the owner made, kept on this computer. */
  categories: () =>
    answer<CustomCategory[]>('arsenal_categories', parseCategories),
  /** Makes, renames or deletes a category of the owner's; answers the list after the change. */
  writeCategory: (change: CategoryChange) =>
    answer<CustomCategory[]>('arsenal_category_write', parseCategories, {
      change,
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

/** Workspace roles as the desktop answers them. */
export interface Roles {
  custom: { id: string; name: string }[];
  /** Launch id → role id. A launch not listed is a Worker. */
  assigned: Record<string, string>;
}
export type RoleChange =
  | { do: 'create'; name: string }
  | { do: 'delete'; id: string }
  | { do: 'assign'; launch: string; role: string }
  | { do: 'rename'; from: string; to: string }
  | { do: 'forget'; launch: string };

export function parseRoles(value: unknown): Roles {
  const v = value as Record<string, unknown>;
  if (typeof v !== 'object' || v === null || !Array.isArray(v.custom))
    throw new Error('roles');
  const custom = v.custom.map((entry: unknown) => {
    const c = entry as Record<string, unknown>;
    if (typeof c?.id !== 'string' || typeof c?.name !== 'string')
      throw new Error('role');
    return { id: c.id, name: c.name };
  });
  const assigned = v.assigned as Record<string, unknown>;
  if (
    typeof assigned !== 'object' ||
    assigned === null ||
    Array.isArray(assigned)
  )
    throw new Error('assigned');
  if (Object.values(assigned).some((r) => typeof r !== 'string'))
    throw new Error('assigned');
  return { custom, assigned: assigned as Record<string, string> };
}

/** A change to the owner's categories, as the desktop takes it. */
export type CategoryChange =
  | { action: 'create'; name: string; section: Section }
  | { action: 'rename'; id: string; name: string }
  | { action: 'delete'; id: string };

const SECTION_NAMES: readonly string[] = [
  'how',
  'before',
  'build',
  'after',
  'env',
];

function parseCategories(value: unknown): CustomCategory[] {
  if (!Array.isArray(value)) throw new Error('categories');
  return value.map((entry: unknown) => {
    const category = entry as Record<string, unknown>;
    if (
      typeof category !== 'object' ||
      category === null ||
      typeof category.id !== 'string' ||
      typeof category.name !== 'string' ||
      typeof category.section !== 'string' ||
      !SECTION_NAMES.includes(category.section)
    )
      throw new Error('category');
    return {
      id: category.id,
      name: category.name,
      section: category.section as Section,
    };
  });
}
