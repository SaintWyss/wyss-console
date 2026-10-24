import type { TerminalId } from './identities';
import type { Uuid } from './uuid';

export interface TerminalSize {
  rows: number;
  cols: number;
}
export type TerminalState =
  'starting' | 'running' | 'exited' | 'closing' | 'closed' | 'failed';
export interface TerminalInfo {
  terminal_id: TerminalId;
  shell: string;
  cwd: string;
  observed_pid: number | null;
  size: TerminalSize;
  state: TerminalState;
  exit: { code: number; signal: string | null } | null;
  error: string | null;
}
/** How the host's parser helper for a terminal is doing. Closed words and numbers only. */
export type ParserHealth =
  'starting' | 'healthy' | 'restarting' | 'unavailable';
/** What a screen drawn from the parser can claim: all of the output, only the end of it, or none. */
export type ParserFidelity = 'exact' | 'tail' | 'unavailable';
export interface ParserBrief {
  health: ParserHealth;
  fidelity: ParserFidelity;
  restarts: number;
}
export interface ParserStatus extends ParserBrief {
  reason: string | null;
  /** Output bytes at the start of the history that the screen does not account for. */
  missing_bytes: number;
  last_failure: string | null;
  sandbox: { layers: number; landlock_abi: number } | null;
}
export interface TerminalOutput {
  batch_id: Uuid | null;
  data: Uint8Array;
  eof: boolean;
  truncated: boolean;
  status: TerminalInfo;
  parser?: ParserBrief;
}
export interface TerminalOutputWire extends Omit<TerminalOutput, 'data'> {
  data: number[];
}

const HEALTH: readonly string[] = [
  'starting',
  'healthy',
  'restarting',
  'unavailable',
];
const FIDELITY: readonly string[] = ['exact', 'tail', 'unavailable'];

export function parseParserBrief(value: unknown): ParserBrief | undefined {
  if (typeof value !== 'object' || value === null) return undefined;
  const v = value as Record<string, unknown>;
  if (
    typeof v.health !== 'string' ||
    !HEALTH.includes(v.health) ||
    typeof v.fidelity !== 'string' ||
    !FIDELITY.includes(v.fidelity) ||
    !Number.isInteger(v.restarts)
  )
    return undefined;
  return {
    health: v.health as ParserHealth,
    fidelity: v.fidelity as ParserFidelity,
    restarts: v.restarts as number,
  };
}

function parseParserStatus(value: unknown): ParserStatus | null {
  const brief = parseParserBrief(value);
  if (!brief) return null;
  const v = value as Record<string, unknown>;
  const sandbox = v.sandbox as Record<string, unknown> | null | undefined;
  return {
    ...brief,
    reason: typeof v.reason === 'string' ? v.reason : null,
    missing_bytes:
      typeof v.missing_bytes === 'number' && v.missing_bytes >= 0
        ? v.missing_bytes
        : 0,
    last_failure: typeof v.last_failure === 'string' ? v.last_failure : null,
    sandbox:
      sandbox &&
      Number.isInteger(sandbox.layers) &&
      Number.isInteger(sandbox.landlock_abi)
        ? {
            layers: sandbox.layers as number,
            landlock_abi: sandbox.landlock_abi as number,
          }
        : null,
  };
}

/** Output a program asked the terminal to act on and the parser refused: counts only. */
export interface RefusedOutput {
  clipboard: number;
  hyperlink: number;
  window_op: number;
  query: number;
  oversized: number;
  string: number;
  invalid_utf8: number;
}

/** What attaching to a terminal that kept running without this window returned. */
export type Attach =
  | {
      mode: 'screen';
      /** The bytes that redraw the screen, to write into an empty terminal of `size`. */
      repaint: Uint8Array;
      size: TerminalSize;
      parser: ParserStatus;
      history: { included: number; omitted: number; evicted: number };
      refused: RefusedOutput;
    }
  | {
      /** No screen was kept: what the host holds of the output is read from its start. */
      mode: 'replay';
      reason: string;
      parser: ParserStatus | null;
    };

/** Most bytes of a repaint a window will write at once. */
const MAX_REPAINT = 1024 * 1024;

function decodeBase64(text: string): Uint8Array {
  if (text.length % 4 !== 0 || !/^[A-Za-z0-9+/]*={0,2}$/.test(text))
    throw new Error('invalid_attach');
  const binary = atob(text);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i);
  return out;
}

const nat = (v: unknown): number => {
  if (typeof v !== 'number' || !Number.isInteger(v) || v < 0)
    throw new Error('invalid_attach');
  return v;
};

/** Strict: anything that is not exactly an attach answer is an error, never a guess. */
export function parseAttach(value: unknown): Attach {
  if (typeof value !== 'object' || value === null)
    throw new Error('invalid_attach');
  const v = value as Record<string, unknown>;
  if (v.mode === 'replay') {
    return {
      mode: 'replay',
      reason: typeof v.reason === 'string' ? v.reason : 'unknown',
      parser: parseParserStatus(v.parser),
    };
  }
  if (v.mode !== 'screen' || typeof v.repaint !== 'string')
    throw new Error('invalid_attach');
  const repaint = decodeBase64(v.repaint);
  const size = v.size as Record<string, unknown> | undefined;
  const history = v.history as Record<string, unknown> | undefined;
  const refused = v.refused as Record<string, unknown> | undefined;
  const parser = parseParserStatus(v.parser);
  if (
    repaint.length > MAX_REPAINT ||
    !size ||
    !history ||
    !refused ||
    !parser ||
    nat(size.rows) < 1 ||
    nat(size.rows) > 512 ||
    nat(size.cols) < 1 ||
    nat(size.cols) > 512
  )
    throw new Error('invalid_attach');
  return {
    mode: 'screen',
    repaint,
    size: { rows: size.rows as number, cols: size.cols as number },
    parser,
    history: {
      included: nat(history.included),
      omitted: nat(history.omitted),
      evicted: nat(history.evicted),
    },
    refused: {
      clipboard: nat(refused.clipboard),
      hyperlink: nat(refused.hyperlink),
      window_op: nat(refused.window_op),
      query: nat(refused.query),
      oversized: nat(refused.oversized),
      string: nat(refused.string),
      invalid_utf8: nat(refused.invalid_utf8),
    },
  };
}

/** What a native session's tab says about it (from the host). */
export interface HostNativeTag {
  session_id: string;
  agent_name: string;
  project: string;
  profile: string | null;
  through_shell: boolean;
}
/** A terminal a host holds: what a window that opens later shows again. */
export interface HostTerminal {
  terminal: TerminalInfo;
  title: string;
  created_at: number;
  native: HostNativeTag | null;
}

/** The host's list, or nothing when the answer is not a list of terminals (no host). */
export function parseHostTerminals(value: unknown): HostTerminal[] {
  if (!Array.isArray(value)) return [];
  const out: HostTerminal[] = [];
  for (const row of value) {
    if (typeof row !== 'object' || row === null) continue;
    const r = row as Record<string, unknown>;
    const t = r.terminal as Record<string, unknown> | undefined;
    if (
      !t ||
      typeof t.terminal_id !== 'string' ||
      typeof r.title !== 'string' ||
      typeof r.created_at !== 'number'
    )
      continue;
    const n = r.native;
    const native =
      n && typeof n === 'object'
        ? (() => {
            const x = n as Record<string, unknown>;
            return typeof x.session_id === 'string' &&
              typeof x.agent_name === 'string' &&
              typeof x.project === 'string' &&
              typeof x.through_shell === 'boolean'
              ? {
                  session_id: x.session_id,
                  agent_name: x.agent_name,
                  project: x.project,
                  profile: typeof x.profile === 'string' ? x.profile : null,
                  through_shell: x.through_shell,
                }
              : null;
          })()
        : null;
    out.push({
      terminal: t as unknown as TerminalInfo,
      title: r.title,
      created_at: r.created_at,
      native,
    });
  }
  return out;
}
