import { invoke } from '@tauri-apps/api/core';
import {
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
  type Detail,
  type FileText,
  type Imported,
  type Item,
  type Preset,
  type Preview,
  type Selection,
  type Summary,
} from '../contracts/arsenal';

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

/** Every request names something the library or a fresh scan already holds, never a path. */
export const arsenalClient = {
  discover: () => answer('arsenal_discover', parseDiscover),
  capabilities: () => answer<Summary[]>('arsenal_capabilities', parseSummaries),
  capability: (capability: string, revision: number | null) =>
    answer<Detail>('arsenal_capability', parseDetail, {
      request: { capability, revision },
    }),
  file: (capability: string, revision: number, path: string) =>
    answer<FileText>('arsenal_file', parseFileText, {
      request: { capability, revision, path },
    }),
  importSkill: (id: string) =>
    answer<Imported>('arsenal_import', parseImported, { request: { id } }),
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
  }) =>
    answer<Selection>('arsenal_selection_write', parseSelection, {
      request: input,
    }),
  deleteSelection: (id: string) =>
    call('arsenal_selection_delete', { request: { id } }).then(() => undefined),
  preview: (id: string) =>
    answer<Preview>('arsenal_preview', parsePreview, { request: { id } }),
};

export type ArsenalClient = typeof arsenalClient;
