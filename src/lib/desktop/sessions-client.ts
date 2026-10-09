import { invoke } from '@tauri-apps/api/core';
import type { ManagedSessionId } from '../contracts/identities';
import {
  parseActivationProgress,
  parseSessionFailure,
  parseSessionPage,
  parseSessionReceipt,
  parseWorkspaceChoice,
  type ActivationProgress,
  type AgentMode,
  type SessionFailure,
  type SessionPage,
  type SessionReceipt,
} from '../contracts/sessions';

/** A session call the desktop refused or could not complete, in product terms. */
export class SessionCallError extends Error {
  constructor(readonly failure: SessionFailure) {
    super(`session: ${failure.code}`);
  }
}

/**
 * The session calls the window can make. None names a control method: the window sends
 * a session it was shown, the revision it saw and the identity of its own action. The
 * only path it sends is a folder the native dialog returned.
 */
async function call<T>(
  command: string,
  request: Record<string, unknown>,
  parse: (value: unknown) => T,
): Promise<T> {
  let answer: unknown;
  try {
    answer = await invoke<unknown>(command, { request });
  } catch (error) {
    throw new SessionCallError(parseSessionFailure(error));
  }
  try {
    return parse(answer);
  } catch {
    // An answer that is not the projection is a failed bridge, never data.
    throw new SessionCallError({ code: 'internal', retry: false });
  }
}

/** The newest page (`before` null) or the page created before a position. */
export function readSessionsPage(before: string | null): Promise<SessionPage> {
  return call('managed_sessions_page', { before }, parseSessionPage);
}
/** `intentId` names the user's action: sent again unchanged, it is the same action. */
export function prepareSession(intentId: string): Promise<SessionReceipt> {
  return call(
    'managed_session_prepare',
    { intent_id: intentId },
    parseSessionReceipt,
  );
}
export function resumeSession(
  intentId: string,
  sessionId: ManagedSessionId,
  expectedRevision: number,
): Promise<SessionReceipt> {
  return call(
    'managed_session_resume',
    {
      intent_id: intentId,
      session_id: sessionId,
      expected_revision: expectedRevision,
    },
    parseSessionReceipt,
  );
}
export function closeSession(
  intentId: string,
  sessionId: ManagedSessionId,
  expectedRevision: number,
): Promise<SessionReceipt> {
  return call(
    'managed_session_close',
    {
      intent_id: intentId,
      session_id: sessionId,
      expected_revision: expectedRevision,
    },
    parseSessionReceipt,
  );
}

/**
 * One step of activating a session in `workspace` and `mode`. `operationId` names the
 * whole user action: the same request is sent again until the stage is `ready`.
 */
export function activateSession(
  operationId: string,
  sessionId: ManagedSessionId,
  expectedRevision: number,
  workspace: string,
  mode: AgentMode,
): Promise<ActivationProgress> {
  return call(
    'managed_session_activate',
    {
      operation_id: operationId,
      session_id: sessionId,
      expected_revision: expectedRevision,
      workspace,
      mode,
    },
    parseActivationProgress,
  );
}
/**
 * One step of entering `mode` with a session that has a workspace. The request names
 * the mode and nothing else: the workspace is the one the authority holds for the
 * session, and which runtime profile the mode means is the desktop's to say.
 */
export function enterSessionMode(
  operationId: string,
  sessionId: ManagedSessionId,
  expectedRevision: number,
  mode: AgentMode,
): Promise<ActivationProgress> {
  return call(
    'managed_session_mode',
    {
      operation_id: operationId,
      session_id: sessionId,
      expected_revision: expectedRevision,
      mode,
    },
    parseActivationProgress,
  );
}
/** Gives up on the binding attempt the authority holds as uncertain for the session. */
export function abandonSession(
  intentId: string,
  sessionId: ManagedSessionId,
): Promise<SessionReceipt> {
  return call(
    'managed_session_abandon',
    { intent_id: intentId, session_id: sessionId },
    parseSessionReceipt,
  );
}
/**
 * Shows the native folder dialog. Answers the folder the user selected, or null when
 * the dialog was dismissed. The window suggests nothing and reads nothing.
 */
export async function chooseWorkspace(): Promise<string | null> {
  let answer: unknown;
  try {
    answer = await invoke<unknown>('choose_workspace');
  } catch (error) {
    throw new SessionCallError(parseSessionFailure(error));
  }
  try {
    return parseWorkspaceChoice(answer)?.path ?? null;
  } catch {
    throw new SessionCallError({ code: 'internal', retry: false });
  }
}
