import { describe, expect, it } from 'vitest';
import {
  INPUT_CAPACITY,
  InputQueue,
  PIECE,
  inputReportWords,
  parseWriteResult,
  type InputReport,
  type WriteResult,
} from '../src/lib/features/projects/input-queue';

const settle = async () => {
  for (let i = 0; i < 400; i++) await Promise.resolve();
};
const of = (n: number, byte = 120) => new Uint8Array(n).fill(byte);

function world(
  answer?: (piece: Uint8Array, call: number) => Promise<WriteResult>,
) {
  const sent: Uint8Array[] = [];
  const reports: InputReport[] = [];
  const pending: ((result: WriteResult) => void)[] = [];
  const queue = new InputQueue(
    (piece) => {
      sent.push(piece.slice());
      if (answer) return answer(piece, sent.length);
      return new Promise((resolve) => pending.push(resolve));
    },
    (report) => reports.push(report),
  );
  const ok = () => {
    const piece = sent[sent.length - pending.length]!;
    pending.shift()!({ outcome: 'written', accepted: piece.length });
  };
  return { queue, sent, reports, pending, ok };
}
const written = async (piece: Uint8Array): Promise<WriteResult> => ({
  outcome: 'written',
  accepted: piece.length,
});

describe('project input queue (REQ-03)', () => {
  it('keeps one piece of at most 4096 bytes in flight and sends in order', async () => {
    const w = world();
    w.queue.push(of(PIECE * 2 + 5, 97));
    w.queue.push(of(3, 98));
    await settle();
    expect(w.sent.map((p) => p.length)).toEqual([PIECE]);
    w.ok();
    await settle();
    expect(w.sent.map((p) => p.length)).toEqual([PIECE, PIECE]);
    w.ok();
    await settle();
    w.ok();
    await settle();
    w.ok();
    await settle();
    expect(w.sent.map((p) => p.length)).toEqual([PIECE, PIECE, 5, 3]);
    expect([...w.sent[3]!]).toEqual([98, 98, 98]);
    expect(w.queue.pending).toBe(0);
    expect(w.reports).toEqual([]);
  });

  it('counts the piece in flight against 64 KiB and refuses a batch that does not fit, whole', async () => {
    const w = world();
    expect(w.queue.push(of(INPUT_CAPACITY))).toBe(true);
    await settle();
    // 4096 are in flight and still held: one more byte does not fit.
    expect(w.queue.pending).toBe(INPUT_CAPACITY);
    expect(w.queue.push(of(1))).toBe(false);
    expect(w.reports).toEqual([{ kind: 'refused_full', bytes: 1 }]);
    w.ok();
    await settle();
    // Room for exactly one piece now; a batch one byte larger is still refused whole.
    expect(w.queue.push(of(PIECE + 1))).toBe(false);
    expect(w.queue.push(of(PIECE))).toBe(true);
    expect(w.queue.pending).toBe(INPUT_CAPACITY);
    // Nothing of a refused batch was ever sent.
    expect(w.sent.every((p) => p.length === PIECE)).toBe(true);
  });

  it('reports a failure in the middle of a paste with what arrived before it, and sends no more', async () => {
    const w = world(async (piece, call) => {
      if (call === 3) throw new Error('owner_refused');
      return written(piece);
    });
    w.queue.push(of(PIECE * 4));
    await settle();
    expect(w.sent).toHaveLength(3);
    expect(w.reports).toEqual([
      {
        kind: 'not_sent',
        delivered: PIECE * 2,
        dropped: PIECE * 2,
        error: expect.any(Error),
      },
    ]);
    expect(w.queue.pending).toBe(0);
    expect(inputReportWords(w.reports[0]!)).toContain(
      `${PIECE * 2} bytes before it reached the terminal`,
    );
  });

  it('never resends a piece whose answer was lost and drops what waited behind it', async () => {
    const w = world(async (piece, call) =>
      call === 2 ? { outcome: 'unknown', accepted: null } : written(piece),
    );
    w.queue.push(of(PIECE * 3));
    await settle();
    expect(w.sent).toHaveLength(2);
    expect(w.reports).toEqual([
      { kind: 'unknown', delivered: PIECE, uncertain: PIECE, dropped: PIECE },
    ]);
    // New typing afterwards is new input, sent once.
    w.queue.push(of(2, 99));
    await settle();
    expect(w.sent).toHaveLength(3);
    expect([...w.sent[2]!]).toEqual([99, 99]);
  });

  it('says the known prefix of a partial write and does not send the rest', async () => {
    const w = world(async () => ({ outcome: 'partial', accepted: 10 }));
    w.queue.push(of(PIECE + 50));
    await settle();
    expect(w.sent).toHaveLength(1);
    expect(w.reports).toEqual([
      { kind: 'partial', delivered: 10, dropped: PIECE - 10 + 50 },
    ]);
  });

  it('drops unsent input when the generation ends and never lets a new one revive it', async () => {
    const w = world();
    w.queue.push(of(PIECE, 97));
    w.queue.push(of(100, 97));
    await settle();
    expect(w.queue.reset()).toBe(100);
    expect(w.reports).toEqual([{ kind: 'discarded', dropped: 100 }]);
    // The piece in flight cannot be taken back and still counts until it returns.
    expect(w.queue.pending).toBe(PIECE);
    // A new hold types while the old piece is still in flight.
    w.queue.push(of(2, 98));
    await settle();
    expect(w.sent).toHaveLength(1);
    w.ok();
    await settle();
    expect(w.sent).toHaveLength(2);
    expect([...w.sent[1]!]).toEqual([98, 98]);
    w.ok();
    await settle();
    expect(w.sent.flatMap((p) => [...p]).filter((b) => b === 97)).toHaveLength(
      PIECE,
    );
  });

  it('keeps the new generation’s input when the old piece in flight comes back uncertain', async () => {
    const w = world();
    w.queue.push(of(5, 97));
    await settle();
    w.queue.reset();
    w.queue.push(of(3, 98));
    w.pending.shift()!({ outcome: 'unknown', accepted: null });
    await settle();
    expect(w.reports).toEqual([
      { kind: 'unknown', delivered: 0, uncertain: 5, dropped: 0 },
    ]);
    expect(w.sent).toHaveLength(2);
    expect([...w.sent[1]!]).toEqual([98, 98, 98]);
  });

  it('believes a count only when it fits what was sent', () => {
    expect(parseWriteResult({ outcome: 'written', accepted: 4 }, 4)).toEqual({
      outcome: 'written',
      accepted: 4,
    });
    expect(parseWriteResult({ outcome: 'partial', accepted: 2 }, 4)).toEqual({
      outcome: 'partial',
      accepted: 2,
    });
    for (const odd of [
      undefined,
      null,
      {},
      { outcome: 'written', accepted: 3 },
      { outcome: 'written' },
      { outcome: 'partial', accepted: 4 },
      { outcome: 'partial', accepted: -1 },
      { outcome: 'done', accepted: 4 },
    ])
      expect(parseWriteResult(odd, 4).outcome).toBe('unknown');
  });
});
