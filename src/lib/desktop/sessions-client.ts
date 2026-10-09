import { invoke } from '@tauri-apps/api/core';
import type { ManagedSessionId } from '../contracts/identities';
import {
  parseSessionFailure,
  parseSessionPage,
  parseSessionReceipt,
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
 * The four session calls the window can make. None names a control method: the window
 * sends a session it was shown, the revision it saw and the identity of its own action.
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
