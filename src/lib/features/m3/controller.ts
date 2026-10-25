import type { M3Client } from '../../desktop/m3-client';
import type { Ack, Decision, M3State } from '../../contracts/m3';

export type M3Status = 'idle' | 'loading' | 'ready' | 'failed';

export interface M3ViewState {
  status: M3Status;
  state: M3State | null;
  /** Why the last read failed, in words; the previous state, if any, is kept and labelled stale. */
  failure: string | null;
  /** True while the shown state could not be refreshed. */
  stale: boolean;
  /** An action of the fixture runtime is in flight. */
  busy: boolean;
  /** The refusal of the last action, as a stable code. */
  refused: string | null;
}

const SLOW_MS = 1500;
const FAST_MS = 300;

/**
 * Reads the M3 projection while its view is shown, and carries the few fixture-runtime actions.
 *
 * Nothing is read while the view is hidden. A read that comes back after a later one was started
 * is dropped, so an older answer can never replace a newer one. An action is never repeated by
 * this controller: it reports the refusal and reads again.
 */
export class M3Controller {
  private view: M3ViewState = {
    status: 'idle',
    state: null,
    failure: null,
    stale: false,
    busy: false,
    refused: null,
  };
  private visible = false;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private generation = 0;
  private disposed = false;

  constructor(
    private readonly client: M3Client,
    private readonly onChange: (state: M3ViewState) => void,
  ) {}

  state(): M3ViewState {
    return this.view;
  }

  setVisible(visible: boolean): void {
    if (this.visible === visible || this.disposed) return;
    this.visible = visible;
    if (visible) void this.refresh();
    else this.stopTimer();
  }

  dispose(): void {
    this.disposed = true;
    this.stopTimer();
  }

  private set(next: Partial<M3ViewState>): void {
    this.view = { ...this.view, ...next };
    this.onChange(this.view);
  }

  private stopTimer(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }

  private delay(): number {
    const fixture = this.view.state?.fixture;
    if (!fixture) {
      // The installation check runs in the background and ends within a few seconds.
      return this.view.state?.claude_install.state === 'verifying'
        ? FAST_MS
        : SLOW_MS;
    }
    const live =
      fixture.phase === 'starting' ||
      fixture.phase === 'ready' ||
      fixture.turn?.state === 'running' ||
      fixture.turn?.state === 'starting' ||
      fixture.approvals.some((a) => a.state === 'accepted');
    return live ? FAST_MS : SLOW_MS;
  }

  private schedule(): void {
    this.stopTimer();
    if (!this.visible || this.disposed) return;
    this.timer = setTimeout(() => void this.refresh(), this.delay());
  }

  async refresh(): Promise<void> {
    if (this.disposed) return;
    const mine = ++this.generation;
    if (this.view.state === null) this.set({ status: 'loading' });
    try {
      const state = await this.client.state();
      if (mine !== this.generation || this.disposed) return;
      this.set({ status: 'ready', state, failure: null, stale: false });
    } catch {
      if (mine !== this.generation || this.disposed) return;
      this.set({
        status: this.view.state ? 'ready' : 'failed',
        failure: 'The provider state could not be read.',
        stale: this.view.state !== null,
      });
    }
    this.schedule();
  }

  private async act(run: () => Promise<Ack>): Promise<void> {
    if (this.view.busy || this.disposed) return;
    this.set({ busy: true, refused: null });
    let refused: string | null;
    try {
      const ack: Ack = await run();
      refused = ack.ok ? null : ack.error;
    } catch {
      refused = 'unreachable';
    }
    this.set({ busy: false, refused });
    await this.refresh();
  }

  fixtureStart(profile: string): Promise<void> {
    return this.act(() => this.client.fixtureStart(profile));
  }
  fixtureStop(): Promise<void> {
    return this.act(() => this.client.fixtureStop());
  }
  fixturePrompt(text: string): Promise<void> {
    return this.act(() => this.client.fixturePrompt(text));
  }
  fixtureInterrupt(): Promise<void> {
    return this.act(() => this.client.fixtureInterrupt());
  }
  fixtureDecide(item: string, decision: Decision): Promise<void> {
    return this.act(() => this.client.fixtureDecide(item, decision));
  }
}
