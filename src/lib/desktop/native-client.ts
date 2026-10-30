import { invoke } from '@tauri-apps/api/core';
import {
  parseFailure,
  parseLauncher,
  parseReadiness,
  parsePreview,
  parseProfiles,
  parseSessions,
  parseStartReply,
  type AgentId,
  type Launcher,
  type Readiness,
  type NativeFailure,
  type NativeSession,
  type Preview,
  type Profile,
  type ProfileBody,
  type StartReply,
} from '../contracts/native';
import type { TerminalSize } from '../contracts/terminal';
import { adoptTerminal } from './terminal-client';

export class NativeCallError extends Error {
  constructor(readonly failure: NativeFailure) {
    super(`native: ${failure.code}`);
  }
}

async function call(
  command: string,
  args: Record<string, unknown> = {},
): Promise<unknown> {
  try {
    return await invoke<unknown>(command, args);
  } catch (error) {
    throw new NativeCallError(parseFailure(error));
  }
}
function checked<T>(answer: unknown, parse: (v: unknown) => T): T {
  try {
    return parse(answer);
  } catch {
    // The desktop answered something that does not fit: nothing is believed.
    throw new NativeCallError({ code: 'internal', reasons: [] });
  }
}

export async function readLauncher(): Promise<Launcher> {
  return checked(await call('native_launcher'), parseLauncher);
}
export async function readReadiness(): Promise<Readiness> {
  return checked(await call('native_readiness'), parseReadiness);
}
/** What the window says about an Arsenal selection: which, the plan it saw, and the project. */
export interface ArsenalStart {
  selection: string;
  config_hash: string;
  project: string;
}
export async function previewProfile(
  agent_id: AgentId,
  profile_id: string | null,
  arsenal: ArsenalStart | null = null,
): Promise<Preview> {
  const request: Record<string, unknown> = { agent_id, profile_id };
  if (arsenal) request.arsenal = arsenal;
  return checked(await call('native_preview', { request }), parsePreview);
}
/** Starts an agent in a terminal. The terminal it runs in is adopted for reading. */
export async function startNative(
  agent_id: AgentId,
  project: string,
  profile_id: string | null,
  size: TerminalSize,
  brief: string | null = null,
  preview: Pick<Preview, 'plan_token' | 'provider_version'> | null = null,
  arsenal: ArsenalStart | null = null,
): Promise<StartReply> {
  const request: Record<string, unknown> = {
    agent_id,
    project,
    profile_id,
    size,
    brief,
    plan_token: preview?.plan_token ?? null,
    expected_cli_version: preview?.provider_version ?? null,
  };
  if (arsenal) request.arsenal = arsenal;
  const reply = checked(
    await call('native_start', { request }),
    parseStartReply,
  );
  adoptTerminal(reply.terminal);
  return reply;
}
export async function restartNative(
  id: string,
  size: TerminalSize,
): Promise<StartReply> {
  const reply = checked(
    await call('native_restart', { request: { id, size } }),
    parseStartReply,
  );
  adoptTerminal(reply.terminal);
  return reply;
}
export async function readNativeSessions(
  sample: boolean,
): Promise<NativeSession[]> {
  return checked(
    await call('native_sessions', { request: { sample } }),
    parseSessions,
  );
}
export async function stopNative(id: string): Promise<void> {
  await call('native_stop', { request: { id } });
}
export async function forgetNative(id: string): Promise<void> {
  await call('native_forget', { request: { id } });
}
/** The native folder dialog; `null` when it was dismissed. */
export async function chooseProject(): Promise<string | null> {
  const answer = await call('native_choose_project');
  return checked(answer, (v) => {
    const path = (v as { path?: unknown }).path;
    if (path === null) return null;
    if (typeof path !== 'string') throw new Error('path');
    return path;
  });
}
export async function setProjectDefault(
  project: string,
  profile_id: string | null,
): Promise<void> {
  await call('native_project_default', { request: { project, profile_id } });
}

export async function readProfiles(): Promise<Profile[]> {
  return checked(await call('native_profiles'), parseProfiles);
}
const created = (v: unknown): string => {
  const id = (v as { id?: unknown }).id;
  if (typeof id !== 'string') throw new Error('id');
  return id;
};
export async function createProfile(body: ProfileBody): Promise<string> {
  return checked(await call('native_profile_create', { body }), created);
}
export async function saveProfile(
  id: string,
  body: ProfileBody,
): Promise<void> {
  await call('native_profile_save', { request: { id, body } });
}
export async function deleteProfile(id: string): Promise<void> {
  await call('native_profile_delete', { request: { id } });
}
export async function duplicateProfile(id: string): Promise<string> {
  return checked(
    await call('native_profile_duplicate', { request: { id } }),
    created,
  );
}
export async function restoreProfile(
  id: string,
  version: number,
): Promise<void> {
  await call('native_profile_restore', { request: { id, version } });
}
export async function exportProfile(id: string): Promise<boolean> {
  const answer = await call('native_profile_export', { request: { id } });
  return (answer as { done?: unknown }).done === true;
}
export async function importProfile(): Promise<string | null> {
  const answer = await call('native_profile_import');
  return answer === null ? null : checked(answer, created);
}

export interface HandoffBrief {
  text: string;
  agent_id: AgentId;
  project: string;
  screen_included: boolean;
}
/** The brief for handing a session's work to another agent, to be reviewed. Starts nothing. */
export async function prepareHandoff(id: string): Promise<HandoffBrief> {
  return checked(
    await call('native_handoff_prepare', { request: { id } }),
    (v) => {
      const o = v as Record<string, unknown>;
      if (
        typeof o.text !== 'string' ||
        typeof o.project !== 'string' ||
        typeof o.screen_included !== 'boolean' ||
        (o.agent_id !== 'claude_code' &&
          o.agent_id !== 'codex' &&
          o.agent_id !== 'opencode' &&
          o.agent_id !== 'hermes')
      )
        throw new Error('handoff');
      return {
        text: o.text,
        agent_id: o.agent_id,
        project: o.project,
        screen_included: o.screen_included,
      };
    },
  );
}
export async function saveHandoff(text: string): Promise<boolean> {
  const answer = await call('native_handoff_save', { request: { text } });
  return (answer as { done?: unknown }).done === true;
}
