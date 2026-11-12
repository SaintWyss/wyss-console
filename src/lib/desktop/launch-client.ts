import { invoke } from '@tauri-apps/api/core';

/** A launch as its file says (ADR 0059). The window shows it and names it by `id`. */
export type Launch =
  | {
      id: string;
      name: string;
      directory: string;
      kind: 'commands';
      commands: string[];
    }
  | {
      id: string;
      name: string;
      directory: string;
      kind: 'session';
      agent: 'claude_code' | 'codex';
      profile: string | null;
      before: string[];
      /** The Arsenal preset's name, for this agent (arsenal-UI-SPEC, phase D). */
      preset: string | null;
    };
/** What Workspace sends to save a session launch. There is no field for a command. */
export interface SaveLaunchInput {
  name: string;
  directory: string;
  agent: 'claude_code' | 'codex';
  profile: string | null;
  preset: string | null;
  /** Only with the owner's say-so: an existing launch with that name is replaced. */
  replace: boolean;
}
export interface LaunchList {
  folder: string;
  launches: Launch[];
  refused: { id: string; error: string }[];
}
export interface BeforeOutcome {
  command: string;
  result: string;
}

export class LaunchError extends Error {
  constructor(
    readonly code: string,
    readonly reasons: string[] = [],
  ) {
    super(reasons[0] ?? code);
  }
}

async function call<T>(command: string, args?: Record<string, unknown>) {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    const e = error as { code?: unknown; reasons?: unknown };
    throw new LaunchError(
      typeof e?.code === 'string' ? e.code : 'internal',
      Array.isArray(e?.reasons)
        ? e.reasons.filter((r): r is string => typeof r === 'string')
        : [],
    );
  }
}

export const listLaunches = () => call<LaunchList>('launch_list');
/** The launch as its file says now: what is opened is never the window's copy. */
export const getLaunch = (id: string) =>
  call<Launch>('launch_get', { request: { id } });
export const runBefore = (id: string) =>
  call<BeforeOutcome[]>('launch_before', { request: { id } });
/** Saves a session launch. `exists` when a launch with that name is saved and `replace` is off. */
export const saveLaunch = (input: SaveLaunchInput) =>
  call<Launch>('launch_save', { request: input });
/** Deletes a launch Workspace saved. The owner's own files are refused. */
export const deleteLaunch = (id: string) =>
  call<void>('launch_delete', { request: { id } });
