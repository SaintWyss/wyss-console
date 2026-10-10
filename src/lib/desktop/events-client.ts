import { invoke } from '@tauri-apps/api/core';
import { parseEventsPage, type EventsPage } from '../contracts/events';
import { parseSessionFailure } from '../contracts/sessions';
import { SessionCallError } from './sessions-client';

/**
 * The history calls the window can make: two reads. Neither changes anything, and
 * neither performs again what an event describes.
 */
async function call(
  command: 'managed_events' | 'managed_events_older',
  request: Record<string, unknown>,
  check: (page: EventsPage) => boolean,
): Promise<EventsPage> {
  let answer: unknown;
  try {
    answer = await invoke<unknown>(command, { request });
  } catch (error) {
    throw new SessionCallError(parseSessionFailure(error));
  }
  let page: EventsPage;
  try {
    page = parseEventsPage(answer);
  } catch {
    // An answer that is not the projection is a failed bridge, never data.
    throw new SessionCallError({ code: 'internal', retry: false });
  }
  if (!check(page))
    throw new SessionCallError({ code: 'internal', retry: false });
  return page;
}

/** The newest window of durable events. */
export function readEvents(): Promise<EventsPage> {
  return call('managed_events', {}, () => true);
}

/**
 * The window before `before`, a position an earlier page handed back, in the stream
 * that page was read from. The answer has to be that window: the same stream, and
 * ending exactly one position before `before`.
 */
export function readOlderEvents(
  stream: string,
  before: string,
): Promise<EventsPage> {
  return call(
    'managed_events_older',
    { stream_id: stream, before },
    (page) =>
      page.stream === stream.toLowerCase() &&
      (page.events.length === 0 ||
        BigInt(before) - BigInt(page.events[0].sequence) === 1n),
  );
}
