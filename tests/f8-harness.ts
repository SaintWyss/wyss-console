import { vi } from 'vitest';
import {
  parseAuditInfo,
  parseF8State,
  parseModelsInfo,
  parseSessionDetail,
  parseSessionsView,
  parseTurnReading,
  type F8Failure,
  type SessionDetail,
} from '../src/lib/contracts/f8';
import { parseF8ApprovalDetail as parseApprovalDetail } from '../src/lib/contracts/f8';
import { F8CallError } from '../src/lib/desktop/f8-client';
import {
  F8Controller,
  type F8Client,
  type F8ViewState,
} from '../src/lib/features/f8/controller';
import fixture from './fixtures/f8-gui-v1.json';

/**
 * A payload as JSON: the tests change it in place and parse it again, so what they send
 * is as strict as what the desktop sends. Typing every key would only restate the parsers.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type Raw = any;
const fx = fixture as Record<string, Raw>;
export const copy = <T>(value: T): T => JSON.parse(JSON.stringify(value));

/** What the real application sent, parsed by the window's own parsers. */
export const real = {
  state: parseF8State(fx.state),
  inactive: parseF8State(fx.state_inactive),
  sessions: parseSessionsView(fx.sessions),
  stopped: parseSessionDetail(fx.session_stopped),
  ready: parseSessionDetail(fx.session_ready),
  attached: parseSessionDetail(fx.session_attached),
  reading: parseTurnReading(fx.turn_reading),
  approval: parseApprovalDetail(fx.approval),
  models: parseModelsInfo(fx.models),
  audit: parseAuditInfo(fx.audit),
};
export const AUTHORITY = real.state.authority!.instance_id;
export const SESSION = real.attached.session.id;
export const OTHER_AUTHORITY = '99999999-9999-4999-8999-999999999999';

export const failure = (
  code: F8Failure['code'],
  retry = false,
  retry_in_ms: number | null = null,
): F8CallError => new F8CallError({ code, retry, retry_in_ms });

/** Settles every promise that is ready, without waiting for a timer. */
export const settle = async () => {
  for (let i = 0; i < 40; i++) await Promise.resolve();
};
export const never = <T>() => new Promise<T>(() => undefined);

/** A detail changed in place and parsed again, so it is as valid as the real one. */
export function detailWith(
  base: SessionDetail,
  change: (raw: Raw) => void,
): SessionDetail {
  const raw = copy(base);
  change(raw);
  return parseSessionDetail(raw);
}
/** The attached detail, with its runtime in another state. */
export const runtimeAs = (
  actual:
    'stopped' | 'starting' | 'initializing' | 'ready' | 'stopping' | 'failed',
): SessionDetail =>
  actual === 'ready'
    ? real.attached
    : detailWith(real.attached, (d) => {
        const r = d.session.runtime;
        r.actual = actual;
        r.model_count = 0;
        r.profile = null;
        r.workspace = null;
        if (actual === 'stopped' || actual === 'failed') {
          r.instance = null;
          r.fence.instance = null;
          r.version = null;
        }
        d.binding = {
          state: 'unbound',
          workspace: null,
          mode: null,
          operation: null,
          observed_model: null,
          attached_instance: null,
        };
        d.lease = null;
        d.turns = [];
        d.approval = null;
      });

/** A desktop whose every call is a spy answering what a ready, attached session would. */
export function rig() {
  let minted = 0;
  const client = {
    state: vi.fn<F8Client['state']>(async () => real.state),
    reconnect: vi.fn<F8Client['reconnect']>(async () => real.state),
    sessions: vi.fn<F8Client['sessions']>(async () => real.sessions),
    session: vi.fn<F8Client['session']>(async () => real.attached),
    prepare: vi.fn<F8Client['prepare']>(async () => ({
      id: SESSION,
      authority: AUTHORITY,
    })),
    close: vi.fn<F8Client['close']>(async () => ({})),
    startRuntime: vi.fn<F8Client['startRuntime']>(async () => ({})),
    stopRuntime: vi.fn<F8Client['stopRuntime']>(async () => ({})),
    models: vi.fn<F8Client['models']>(async () => real.models),
    grantLease: vi.fn<F8Client['grantLease']>(async () => ({})),
    renewLease: vi.fn<F8Client['renewLease']>(async () => ({})),
    revokeLease: vi.fn<F8Client['revokeLease']>(async () => ({})),
    createBinding: vi.fn<F8Client['createBinding']>(async () => ({})),
    resumeBinding: vi.fn<F8Client['resumeBinding']>(async () => ({})),
    abandonBinding: vi.fn<F8Client['abandonBinding']>(async () => ({})),
    startTurn: vi.fn<F8Client['startTurn']>(async () => ({
      id: real.reading.turn.turn.id,
    })),
    cancelTurn: vi.fn<F8Client['cancelTurn']>(async () => ({})),
    // The desktop holds a read while nothing is new: the first answers, the next waits.
    readTurn: vi.fn<F8Client['readTurn']>(
      (() => {
        let first = true;
        return async () => {
          if (first) {
            first = false;
            return real.reading;
          }
          return never<typeof real.reading>();
        };
      })(),
    ),
    readApproval: vi.fn<F8Client['readApproval']>(async () => real.approval),
    decide: vi.fn<F8Client['decide']>(async () => ({})),
    audit: vi.fn<F8Client['audit']>(async () => real.audit),
    wait: vi.fn<F8Client['wait']>(() => never()),
    chooseWorkspace: vi.fn<F8Client['chooseWorkspace']>(
      async () => '/home/dev/project',
    ),
  };
  let state!: F8ViewState;
  const sleeps: number[] = [];
  // The fixture's own time: an approval is open for the five minutes after its request.
  let clock = Date.parse(real.approval.approval.requested_at) + 1000;
  const controller = new F8Controller(
    (next) => {
      state = next;
    },
    client as unknown as F8Client,
    () => `intent-${++minted}`,
    async (ms) => {
      sleeps.push(ms);
      clock += ms;
    },
    () => clock,
  );
  return {
    client,
    controller,
    sleeps,
    get state() {
      return state;
    },
    advance(ms: number) {
      clock += ms;
    },
    /** The controller after it learned the interface is active and read the session. */
    async open() {
      await controller.start();
      await controller.refresh();
      controller.select(SESSION);
      await settle();
      return controller;
    },
  };
}
