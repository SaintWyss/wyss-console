import { invoke } from '@tauri-apps/api/core';
import type { ManagedSessionId, ManagedTurnId } from '../contracts/identities';
import { parseSessionFailure } from '../contracts/sessions';
import {
  parseTurnHistory,
  parseTurnList,
  parseTurnReceipt,
  parseTurnView,
  type HistoryCursor,
  type TurnHistory,
  type TurnList,
  type TurnReceipt,
  type TurnView,
} from '../contracts/turns';
import { SessionCallError } from './sessions-client';

/**
 * The turn calls the window can make: three reads and two intents. None names a control
 * method, and none answers an approval. The window sends a session or a turn it was
 * shown, the revision it saw, the identity of its own action and, for a start, the text
 * the user wrote, as one field.
 */
async function call<T>(
  command:
    | 'managed_turns'
    | 'managed_turns_older'
    | 'managed_turn_read'
    | 'managed_turn_start'
    | 'managed_turn_cancel',
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

/** The recent turns of a session, newest first, and the session as it is now. */
export function readTurns(sessionId: ManagedSessionId): Promise<TurnList> {
  return call('managed_turns', { session_id: sessionId }, (value) => {
    const list = parseTurnList(value);
    // An answer about another session is not an answer to this question.
    if (list.session.id.toLowerCase() !== sessionId.toLowerCase())
      throw new Error('turn_list: other_session');
    return list;
  });
}

/**
 * One page of the turns before those already shown, newest first, behind a position a list
 * or an earlier page handed out. The position is sent back as it was handed out.
 */
export function readOlderTurns(
  sessionId: ManagedSessionId,
  cursor: HistoryCursor,
): Promise<TurnHistory> {
  return call(
    'managed_turns_older',
    {
      session_id: sessionId,
      stream_id: cursor.stream_id,
      before: cursor.before,
    },
    (value) => {
      const page = parseTurnHistory(value);
      // An answer about another session is not an answer to this question.
      if (page.session.toLowerCase() !== sessionId.toLowerCase())
        throw new Error('turn_history: other_session');
      // History only goes back: the next position is of the same stream and before this one.
      if (
        page.older !== null &&
        (page.older.stream_id.toLowerCase() !==
          cursor.stream_id.toLowerCase() ||
          BigInt(page.older.before) >= BigInt(cursor.before))
      )
        throw new Error('turn_history: cursor');
      return page;
    },
  );
}

/**
 * One reading of a turn: the stream events after `after`, and its canonical record.
 * With `wait`, the desktop holds the answer a moment while there is nothing new.
 */
export function readTurn(
  turnId: ManagedTurnId,
  after: string | null,
  wait: boolean,
): Promise<TurnView> {
  return call(
    'managed_turn_read',
    { turn_id: turnId, after, wait },
    (value) => {
      const view = parseTurnView(value);
      if (view.turn.id.toLowerCase() !== turnId.toLowerCase())
        throw new Error('turn_view: other_turn');
      // Nothing at or before the cursor comes back, and the cursor never goes back.
      const from = after === null ? 0n : BigInt(after);
      if (
        view.events.some((event) => BigInt(event.sequence) <= from) ||
        (view.next === null ? after !== null : BigInt(view.next) < from)
      )
        throw new Error('turn_view: cursor');
      return view;
    },
  );
}

/** `intentId` names the user's action: sent again unchanged, it is the same turn. */
export function startTurn(
  intentId: string,
  sessionId: ManagedSessionId,
  expectedRevision: number,
  prompt: string,
): Promise<TurnReceipt> {
  return call(
    'managed_turn_start',
    {
      intent_id: intentId,
      session_id: sessionId,
      expected_revision: expectedRevision,
      prompt,
    },
    parseTurnReceipt,
  );
}

/** Asks for a turn to stop. The answer is a receipt: the turn stopped when a read says so. */
export function cancelTurn(
  intentId: string,
  turnId: ManagedTurnId,
  expectedRevision: number,
): Promise<TurnReceipt> {
  return call(
    'managed_turn_cancel',
    {
      intent_id: intentId,
      turn_id: turnId,
      expected_revision: expectedRevision,
    },
    (value) => {
      const receipt = parseTurnReceipt(value);
      if (receipt.id.toLowerCase() !== turnId.toLowerCase())
        throw new Error('turn_receipt: other_turn');
      return receipt;
    },
  );
}
