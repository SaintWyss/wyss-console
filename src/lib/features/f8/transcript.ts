import type {
  ApprovalOutcome,
  TurnEvent,
  TurnEventKind,
} from '../../contracts/turns';

/**
 * A turn's stream, as a person reads it. The only provider text in it is the assistant's
 * answer; a mediated action is named by what happened to it, never by its content, and the
 * model's reasoning is only said to be happening.
 */
export type TranscriptItem =
  | { kind: 'assistant'; text: string; open: boolean }
  | { kind: 'activity'; event: TurnEventKind; count: number }
  | { kind: 'approval'; outcome: ApprovalOutcome | null }
  | { kind: 'thinking' }
  | { kind: 'gap' };

/** Items a transcript keeps, and the answer text it keeps in all (UTF-16 units). */
export const MAX_ITEMS = 400;
export const MAX_TEXT = 256 * 1024;

const TERMINAL: readonly TurnEventKind[] = [
  'started',
  'completed',
  'interrupted',
  'failed',
];

export interface Transcript {
  items: TranscriptItem[];
  /** Earlier output was dropped to stay within the bounds above. */
  truncated: boolean;
}
export const emptyTranscript: Transcript = { items: [], truncated: false };

/** The transcript with `events` after it. Pure: the first argument is never changed. */
export function appendEvents(
  transcript: Transcript,
  events: readonly TurnEvent[],
): Transcript {
  if (events.length === 0) return transcript;
  const items = transcript.items.map((item) => ({ ...item }));
  for (const event of events) {
    if (TERMINAL.includes(event.kind)) continue;
    const last = items.at(-1);
    switch (event.kind) {
      case 'text':
        if (last?.kind === 'assistant' && last.open)
          last.text += event.text ?? '';
        else
          items.push({ kind: 'assistant', text: event.text ?? '', open: true });
        break;
      case 'message_end':
        if (last?.kind === 'assistant') last.open = false;
        break;
      case 'thinking':
        if (last?.kind !== 'thinking') items.push({ kind: 'thinking' });
        break;
      case 'gap':
        if (last?.kind !== 'gap') items.push({ kind: 'gap' });
        break;
      case 'approval_resolved':
        items.push({ kind: 'approval', outcome: approvalOutcome(event.text) });
        break;
      default:
        if (last?.kind === 'activity' && last.event === event.kind)
          last.count++;
        else items.push({ kind: 'activity', event: event.kind, count: 1 });
    }
  }
  return bounded({ items, truncated: transcript.truncated });
}

function approvalOutcome(text: string | null): ApprovalOutcome | null {
  return text as ApprovalOutcome | null;
}

function bounded(transcript: Transcript): Transcript {
  let { items, truncated } = transcript;
  let text = items.reduce(
    (sum, item) => sum + (item.kind === 'assistant' ? item.text.length : 0),
    0,
  );
  while (items.length > MAX_ITEMS || text > MAX_TEXT) {
    const [first, ...rest] = items;
    if (first === undefined) break;
    // One answer longer than the bound keeps its end, which is the part being written.
    if (rest.length === 0 && first.kind === 'assistant') {
      items = [
        { ...first, text: first.text.slice(first.text.length - MAX_TEXT) },
      ];
      truncated = true;
      break;
    }
    if (first.kind === 'assistant') text -= first.text.length;
    items = rest;
    truncated = true;
  }
  return { items, truncated };
}
