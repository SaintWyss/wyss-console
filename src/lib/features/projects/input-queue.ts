/**
 * Input a project pane has taken from the keyboard and not yet seen the host answer for
 * (workspace contract §7, REQ-03).
 *
 * - At most 64 KiB is held, counting the piece in flight. A batch that does not fit is refused
 *   whole before any of it is queued.
 * - One piece of at most 4096 bytes is in flight at a time.
 * - `reset()` ends the input generation (blur, detach, claim loss): what was not sent is
 *   dropped. The piece in flight may already be in the terminal and cannot be taken back, and
 *   input queued by a later generation is never mixed with it.
 * - Nothing is ever sent twice. A piece the host took only part of, or did not confirm, ends
 *   the batch: what waited behind it is dropped and the report says what is known.
 */
export const INPUT_CAPACITY = 64 * 1024;
export const PIECE = 4096;

export type WriteResult =
  | { outcome: 'written'; accepted: number }
  | { outcome: 'partial'; accepted: number }
  | { outcome: 'unknown'; accepted: null };

export function parseWriteResult(value: unknown, sent: number): WriteResult {
  if (typeof value !== 'object' || value === null)
    return { outcome: 'unknown', accepted: null };
  const v = value as Record<string, unknown>;
  const n = v.accepted;
  const count = typeof n === 'number' && Number.isInteger(n) && n >= 0;
  // A count that does not fit what was sent is not believed: the outcome is unknown.
  if (v.outcome === 'written' && count && n === sent)
    return { outcome: 'written', accepted: sent };
  if (v.outcome === 'partial' && count && (n as number) < sent)
    return { outcome: 'partial', accepted: n as number };
  return { outcome: 'unknown', accepted: null };
}

export type InputReport =
  /** The batch did not fit: none of it was queued or sent. */
  | { kind: 'refused_full'; bytes: number }
  /** The host took a known prefix of a piece; the rest and what waited were not sent. */
  | { kind: 'partial'; delivered: number; dropped: number }
  /** A piece left and was not confirmed: it may or may not have arrived. */
  | { kind: 'unknown'; delivered: number; uncertain: number; dropped: number }
  /** A piece was refused before it was written; it and what waited were not sent. */
  | { kind: 'not_sent'; delivered: number; dropped: number; error: unknown }
  /** The generation ended with input still waiting. */
  | { kind: 'discarded'; dropped: number };

export class InputQueue {
  private queue: Uint8Array[] = [];
  private queued = 0;
  private inFlight = 0;
  private generation = 0;
  private pumping = false;
  /** Bytes confirmed since the queue was last empty: what a later failure can say arrived. */
  private delivered = 0;

  constructor(
    private readonly write: (piece: Uint8Array) => Promise<WriteResult>,
    private readonly report: (report: InputReport) => void,
  ) {}

  /** Bytes held, with the piece in flight. */
  get pending(): number {
    return this.queued + this.inFlight;
  }

  /** Queues a batch, or refuses it whole. */
  push(bytes: Uint8Array): boolean {
    if (!bytes.length) return true;
    if (this.pending + bytes.length > INPUT_CAPACITY) {
      this.report({ kind: 'refused_full', bytes: bytes.length });
      return false;
    }
    this.queue.push(bytes);
    this.queued += bytes.length;
    void this.pump();
    return true;
  }

  /** Ends the generation. Returns how many unsent bytes were dropped. */
  reset(): number {
    this.generation += 1;
    const dropped = this.drop();
    this.delivered = 0;
    if (dropped) this.report({ kind: 'discarded', dropped });
    return dropped;
  }

  private drop(): number {
    const dropped = this.queued;
    this.queue = [];
    this.queued = 0;
    return dropped;
  }

  private take(): Uint8Array {
    const head = this.queue[0]!;
    const piece = head.subarray(0, PIECE);
    if (piece.length === head.length) this.queue.shift();
    else this.queue[0] = head.subarray(piece.length);
    this.queued -= piece.length;
    return piece;
  }

  private async pump() {
    if (this.pumping) return;
    this.pumping = true;
    try {
      while (this.queue.length) {
        const generation = this.generation;
        const piece = this.take();
        this.inFlight = piece.length;
        let result: WriteResult | undefined;
        let error: unknown;
        try {
          result = await this.write(piece);
        } catch (caught) {
          error = caught;
        }
        this.inFlight = 0;
        // What is queued now belongs to a later generation: it is neither dropped nor blamed.
        const current = generation === this.generation;
        const delivered = current ? this.delivered : 0;
        if (result?.outcome === 'written') {
          if (current) this.delivered += piece.length;
          continue;
        }
        const waiting = current ? this.drop() : 0;
        if (current) this.delivered = 0;
        if (!result)
          this.report({
            kind: 'not_sent',
            delivered,
            dropped: piece.length + waiting,
            error,
          });
        else if (result.outcome === 'partial')
          this.report({
            kind: 'partial',
            delivered: delivered + result.accepted,
            dropped: piece.length - result.accepted + waiting,
          });
        else
          this.report({
            kind: 'unknown',
            delivered,
            uncertain: piece.length,
            dropped: waiting,
          });
      }
      this.delivered = 0;
    } finally {
      this.pumping = false;
    }
  }
}

const bytes = (n: number) => `${n} byte${n === 1 ? '' : 's'}`;

/** What a report means, for the person typing. `reason` words a refusal before writing. */
export function inputReportWords(report: InputReport, reason = ''): string {
  const arrived = (n: number) =>
    n ? ` ${bytes(n)} before it reached the terminal.` : '';
  switch (report.kind) {
    case 'refused_full':
      return `That input (${bytes(report.bytes)}) was not taken: more than 64 KiB would be waiting. None of it was sent.`;
    case 'partial':
      return `The terminal took only part of the input: ${bytes(report.delivered)} arrived, ${bytes(report.dropped)} were not sent and are not sent again.`;
    case 'unknown':
      return `The host did not confirm ${bytes(report.uncertain)} of input: they may or may not have reached the terminal and were not sent again.${arrived(report.delivered)}${report.dropped ? ` ${bytes(report.dropped)} still waiting were dropped.` : ''} Check the screen before typing on.`;
    case 'not_sent':
      return `${reason || 'The terminal host refused the input.'} ${bytes(report.dropped)} were not sent.${arrived(report.delivered)}`;
    case 'discarded':
      return `Input was released with ${bytes(report.dropped)} still waiting; they were not sent.`;
  }
}
