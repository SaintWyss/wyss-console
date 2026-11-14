import { invoke } from '@tauri-apps/api/core';

/** A launch as its file says (ADR 0059). The window shows it and names it by `id`. */
export type Launch =
  | {
      id: string;
      name: string;
      directory: string;
      kind: 'commands';
      /** What runs on this computer: its distribution's variant, else the file's own commands. */
      commands: string[];
      variants: Variant[];
      /** The variant `commands` came from; it cannot be removed. */
      active: string | null;
      template: string | null;
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
/** One distribution's commands for a launch, by its os-release ID. */
export interface Variant {
  distro: string;
  commands: string[];
  /** The switches the commands were composed from. */
  options: string[];
  /** Written by hand: the switches no longer make the commands. */
  manual: boolean;
}
/** The distribution this computer runs, from os-release. */
export interface SystemInfo {
  id: string;
  like: string[];
  name: string;
}
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
  system: SystemInfo;
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
/** Replaces a commands launch's distributions; this computer's one cannot be removed. */
export const saveVariants = (id: string, variants: Variant[]) =>
  call<Launch>('launch_save_variants', { request: { id, variants } });
