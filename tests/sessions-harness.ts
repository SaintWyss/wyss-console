import { parseAuthorityView } from '../src/lib/contracts/authority';
import {
  parseSessionPage,
  type ActivationProgress,
  type SessionFailure,
  type SessionPage,
  type SessionReceipt,
} from '../src/lib/contracts/sessions';
import { SessionCallError } from '../src/lib/desktop/sessions-client';
import {
  SessionsController,
  type SessionsClient,
  type SessionsState,
} from '../src/lib/features/sessions/controller';
import type { AuthorityObservation } from '../src/lib/shell/authority';
import views from './fixtures/authority-view-v1.json';
import pages from './fixtures/session-page-v3.json';

export const loaded = parseSessionPage(pages.loaded);
export const empty = parseSessionPage(pages.empty);
export const [fresh, bound, closed] = loaded.sessions;
export const settle = async () => {
  for (let i = 0; i < 25; i++) await Promise.resolve();
};

export function connected(
  head: string | null,
  over: { restarts?: number; actual?: string } = {},
): AuthorityObservation {
  return {
    state: 'observed',
    view: parseAuthorityView({
      ...views.connected,
      authority: {
        ...views.connected.authority,
        last_sequence: head,
        restarts: over.restarts ?? 0,
      },
      agent: { ...views.connected.agent, actual: over.actual ?? 'ready' },
    }),
  };
}
export const offline: AuthorityObservation = {
  state: 'observed',
  view: parseAuthorityView(views.waiting_absent),
};
export const at = (page: SessionPage, head: string | null): SessionPage => ({
  ...page,
  head,
});

export interface Deferred<T> {
  resolve: (value: T) => void;
  reject: (failure: SessionFailure) => void;
}
/** A client whose every call waits to be answered by the test. */
export function harness() {
  const reads: (Deferred<SessionPage> & { before: string | null })[] = [];
  const intents: (Deferred<SessionReceipt> & {
    kind: string;
    args: unknown[];
  })[] = [];
  /** Activation steps, each one request of the same action. */
  const steps: (Deferred<ActivationProgress> & { args: unknown[] })[] = [];
  const dialogs: Deferred<string | null>[] = [];
  const deferred = <T>(): Deferred<T> & { promise: Promise<T> } => {
    let resolve!: (value: T) => void;
    let fail!: (error: unknown) => void;
    const promise = new Promise<T>((res, rej) => {
      resolve = res;
      fail = rej;
    });
    return {
      promise,
      resolve,
      reject: (failure) => fail(new SessionCallError(failure)),
    };
  };
  const intent =
    (kind: string) =>
    (...args: unknown[]) => {
      const d = deferred<SessionReceipt>();
      intents.push({ ...d, kind, args });
      return d.promise;
    };
  const client: SessionsClient = {
    page(before) {
      const d = deferred<SessionPage>();
      reads.push({ ...d, before });
      return d.promise;
    },
    prepare: intent('prepare'),
    resume: intent('resume'),
    close: intent('close'),
    abandon: intent('abandon'),
    activate(...args) {
      const d = deferred<ActivationProgress>();
      steps.push({ ...d, args });
      return d.promise;
    },
    // A change of mode is the same kind of step, asked without a folder.
    enter(...args) {
      const d = deferred<ActivationProgress>();
      steps.push({ ...d, args });
      return d.promise;
    },
    chooseWorkspace() {
      const d = deferred<string | null>();
      dialogs.push(d);
      return d.promise;
    },
  };
  let state!: SessionsState;
  let minted = 0;
  const controller = new SessionsController(
    (next) => {
      state = next;
    },
    client,
    () => `intent-${++minted}`,
  );
  state = controller.state();
  return { controller, reads, intents, steps, dialogs, state: () => state };
}
/** Shown and connected at `head`, with its first read answered by `page`. */
export async function showing(
  page: SessionPage,
  head: string | null = page.head,
) {
  const h = harness();
  h.controller.setVisible(true);
  h.controller.observe(connected(head));
  h.reads[0].resolve(page);
  await settle();
  return h;
}
/** The composer open for a new session (or `session`) with `folder` chosen in the dialog. */
export async function composed(
  h: ReturnType<typeof harness>,
  folder: string,
  session: Parameters<SessionsController['compose']>[1] = null,
) {
  h.controller.compose(true, session);
  h.controller.chooseWorkspace();
  h.dialogs.at(-1)!.resolve(folder);
  await settle();
}
