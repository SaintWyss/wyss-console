import * as desktop from '../../desktop/terminal-client';
import type { TerminalId } from '../../contracts/identities';
import type {
  Attach,
  ParserBrief,
  TerminalInfo,
  TerminalSize,
} from '../../contracts/terminal';
import type { TerminalRenderer } from './renderer';

export type ViewState =
  'starting' | 'running' | 'exited' | 'closing' | 'closed' | 'failed';
export interface TerminalViewState {
  state: ViewState;
  info?: TerminalInfo;
  error?: string;
  truncated?: boolean;
  /** How the host's parser for this terminal is doing, as of the last read. */
  parser?: ParserBrief;
  /** How this window came to show a terminal that was already running; absent for a new one. */
  attach?: Attach;
}
const INPUT_CAPACITY = 64 * 1024;
const WRITE_LIMIT = 4096;
/** An empty answer that took at least this long was a wait the desktop made for us (host_read). */
const LONG_POLL_SEEN_MS = 500;

/** How often, and how long apart, a write the host did not take is offered again. */
const BUSY_RETRIES = 40;
const BUSY_DELAY_MS = 25;

function isBusy(error: unknown): boolean {
  return (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    error.code === 'busy'
  );
}

/** One local human-terminal owner. No command interpretation or domain authority. */
export class TerminalController {
  private active = true;
  private focusPending = false;
  private id?: TerminalId;
  private info?: TerminalInfo;
  private state: ViewState = 'starting';
  private stopped = false;
  private disposed = false;
  private detached = false;
  private startTask?: Promise<void>;
  private closeTask?: Promise<void>;
  private disposeTask?: Promise<void>;
  private finishTask?: Promise<void>;
  private inputCleanup?: () => void;
  private metricsCleanup?: () => void;
  private observer?: ResizeObserver;
  private resizeTimer?: ReturnType<typeof setTimeout>;
  private sleepTimer?: ReturnType<typeof setTimeout>;
  private wakeSleep?: () => void;
  private desiredSize?: TerminalSize;
  private sentSize?: TerminalSize;
  private resizing = false;
  private inputs: Uint8Array[] = [];
  private inputBytes = 0;
  private writing = false;
  private readonly abort = new AbortController();
  private parser?: ParserBrief;
  private attached?: Attach;

  constructor(
    private readonly host: HTMLElement,
    private readonly renderer: TerminalRenderer,
    private readonly update: (state: TerminalViewState) => void,
    /** A terminal the desktop already made (a native session's): read, not created. */
    private readonly adopted?: TerminalInfo,
    /** The name the host keeps for a terminal this controller makes. */
    private readonly title?: string,
    /** Where a shell this controller makes starts. */
    private readonly cwd?: string,
    /** The terminal was already running (a host kept it): its screen is drawn before reading on. */
    private readonly attachFirst = false,
  ) {}

  /** Searches this terminal's text; see the renderer. */
  search(query: string, options: { caseSensitive: boolean; step: 1 | -1 | 0 }) {
    return this.renderer.search?.(query, options) ?? { total: 0, current: 0 };
  }
  clearSearch() {
    this.renderer.clearSearch?.();
  }
  /**
   * `active`: the terminal is shown and takes part in layout. `focus`: it is also the one the
   * keyboard goes to. Panes of one tab are all shown, and only one has the focus.
   */
  setActive(active: boolean, focus = true) {
    this.active = active;
    this.renderer.setActive(active);
    if (!active) {
      clearTimeout(this.resizeTimer);
      this.resizeTimer = undefined;
      this.desiredSize = undefined;
      this.focusPending = false;
    } else if (!this.stopped) {
      this.focusPending = focus;
      this.requestFit();
    }
  }
  start(): Promise<void> {
    return (this.startTask ??= this.initialize());
  }
  private publish(extra: Partial<TerminalViewState> = {}) {
    if (!this.disposed)
      this.update({
        state: this.state,
        info: this.info,
        parser: this.parser,
        attach: this.attached,
        ...extra,
      });
  }
  private async initialize() {
    if (this.stopped) return;
    this.publish();
    try {
      this.inputCleanup = this.renderer.input(
        (data) => this.enqueueText(data),
        (data) => this.enqueueBinary(data),
      );
      this.observer = new ResizeObserver(() => this.requestFit());
      this.observer.observe(this.host);
      // A preference that changes the size of a character changes how many fit.
      this.metricsCleanup = this.renderer.onMetricsChange?.(() =>
        this.requestFit(),
      );
      const size = (this.active ? this.renderer.fit() : undefined) ?? {
        rows: 24,
        cols: 80,
      };
      this.info =
        this.adopted ??
        (await desktop.createTerminal(
          size,
          this.cwd ?? null,
          this.title ?? null,
        ));
      this.id = this.info.terminal_id;
      this.sentSize = this.adopted ? this.adopted.size : size;
      if (this.stopped) {
        // A window that is leaving keeps the terminal; one that was closed does not.
        if (!this.detached) await this.closeOnce();
        return;
      }
      if (this.attachFirst && this.adopted && !(await this.attachScreen()))
        return;
      if (this.stopped) return;
      this.state = 'running';
      this.renderer.setInput(true);
      this.publish();
      if (this.active) this.renderer.focus();
      this.requestFit();
      void this.output();
    } catch (error) {
      if (!this.stopped) this.fail(error);
    }
  }
  /**
   * Draws the screen the host's parser kept for a terminal that ran while this window was away.
   * The read position moves to where that screen ends, so nothing is drawn twice and nothing is
   * missed; when no screen was kept the host's output is read from its start, as before. Nothing
   * is sent to the program.
   */
  private async attachScreen(): Promise<boolean> {
    if (!this.id || !this.renderer.restore) return true;
    const id = this.id;
    let result: Attach | undefined;
    let drawn = false;
    try {
      result = await desktop.attachTerminal(id);
      if (result.mode === 'screen') {
        drawn = await this.renderer.restore(result.repaint, result.size);
      }
    } catch {
      // A host that cannot say what its screen is, or one the window cannot draw: below.
    }
    // The host moves its read position only on this, so a screen that was not drawn loses nothing.
    if (result?.mode === 'screen') {
      try {
        await desktop.commitAttach(id, result.token, drawn);
      } catch (error) {
        if (drawn) {
          // The window shows a screen the host does not know about: reading on would draw output
          // twice. Said, not guessed.
          this.fail(error);
          return false;
        }
      }
    } else if (!result) {
      try {
        await desktop.commitAttach(id, null, false);
      } catch {
        // Nothing was staged that the window could have used.
      }
    }
    if (result?.mode === 'screen' && drawn) {
      // The program is at the size the repaint was made for until a resize says otherwise.
      this.sentSize = result.size;
      this.parser = {
        health: result.parser.health,
        fidelity: result.parser.fidelity,
        restarts: result.parser.restarts,
      };
      this.attached = result;
    } else if (result?.mode === 'replay') {
      this.attached = result;
    } else {
      this.attached = {
        mode: 'replay',
        reason: result ? 'restore_failed' : 'attach_failed',
        parser: null,
      };
    }
    this.publish();
    return true;
  }
  private requestFit() {
    if (this.stopped || !this.active || this.resizeTimer) return;
    this.resizeTimer = setTimeout(() => {
      this.resizeTimer = undefined;
      if (this.stopped || !this.active) return;
      const size = this.renderer.fit();
      if (this.focusPending) {
        this.focusPending = false;
        this.renderer.focus();
      }
      if (size) {
        this.desiredSize = size;
        void this.resize();
      }
    }, 60);
  }
  private async resize() {
    if (
      this.resizing ||
      !this.active ||
      !this.id ||
      this.state !== 'running' ||
      this.stopped
    )
      return;
    this.resizing = true;
    try {
      while (
        !this.stopped &&
        this.active &&
        this.state === 'running' &&
        this.desiredSize
      ) {
        const size = this.desiredSize;
        this.desiredSize = undefined;
        if (
          size.rows === this.sentSize?.rows &&
          size.cols === this.sentSize.cols
        )
          continue;
        this.info = await desktop.resizeTerminal(this.id, size);
        this.sentSize = size;
      }
    } catch (error) {
      if (!this.stopped && this.state === 'running') this.fail(error);
    } finally {
      this.resizing = false;
    }
  }
  private enqueueText(data: string) {
    if (data.length > INPUT_CAPACITY) {
      this.fail(new Error('input_limit'));
      return;
    }
    this.enqueue(new TextEncoder().encode(data));
  }
  private enqueueBinary(data: string) {
    if (data.length > INPUT_CAPACITY) {
      this.fail(new Error('input_limit'));
      return;
    }
    // xterm onBinary is a binary string, not UTF-8 text (protocol replies/mouse).
    this.enqueue(Uint8Array.from(data, (char) => char.charCodeAt(0) & 255));
  }
  private enqueue(data: Uint8Array) {
    if (this.stopped || this.state !== 'running' || !data.length) return;
    if (this.inputBytes + data.length > INPUT_CAPACITY) {
      this.fail(new Error('input_limit'));
      return;
    }
    this.inputs.push(data);
    this.inputBytes += data.length;
    this.wakeSleep?.();
    void this.writeInput();
  }
  private async writeInput() {
    if (this.writing || !this.id) return;
    this.writing = true;
    let busy = 0;
    try {
      while (!this.stopped && this.state === 'running' && this.inputs.length) {
        const data = this.inputs[0];
        const chunk = data.subarray(0, WRITE_LIMIT);
        let written: number;
        try {
          written = await desktop.writeTerminal(this.id, chunk);
        } catch (error) {
          // The host takes one write at a time per terminal. `busy` means this one was not taken
          // (nothing was written), typically because another window was writing at that moment:
          // it is asked again shortly, in order, instead of failing and closing the terminal that
          // other window is using.
          if (isBusy(error) && ++busy <= BUSY_RETRIES && !this.stopped) {
            await this.sleep(BUSY_DELAY_MS);
            continue;
          }
          throw error;
        }
        busy = 0;
        if (this.stopped || this.state !== 'running') return;
        if (written !== chunk.length) throw new Error('write_failed');
        this.inputBytes -= chunk.length;
        if (data.length === chunk.length) this.inputs.shift();
        else this.inputs[0] = data.subarray(chunk.length);
      }
    } catch (error) {
      if (!this.stopped && this.state === 'running') this.fail(error);
    } finally {
      this.writing = false;
    }
  }
  private sleep(ms: number): Promise<void> {
    return new Promise((resolve) => {
      const done = () => {
        clearTimeout(this.sleepTimer);
        this.sleepTimer = undefined;
        this.wakeSleep = undefined;
        resolve();
      };
      this.wakeSleep = done;
      this.sleepTimer = setTimeout(done, ms);
    });
  }
  private processed(data: Uint8Array): Promise<boolean> {
    // Disposal cancels waiting even if an external renderer never calls back.
    return new Promise((resolve, reject) => {
      const cancel = () => resolve(false);
      this.abort.signal.addEventListener('abort', cancel, { once: true });
      this.renderer
        .write(data)
        .then(resolve, reject)
        .finally(() => {
          this.abort.signal.removeEventListener('abort', cancel);
        });
    });
  }
  private async output() {
    let idle = 10;
    try {
      while (!this.stopped && this.id) {
        // Next pull returns the previous ACK only after renderer write resolves.
        const asked = Date.now();
        const batch = await desktop.readTerminalOutput(this.id);
        // The desktop waits for output before it answers empty; an answer that took this long
        // came from a wait, and there is no interval left to pace.
        const waited = Date.now() - asked >= LONG_POLL_SEEN_MS;
        if (this.stopped) return;
        this.info = batch.status;
        const parser = batch.parser;
        const changed =
          parser?.health !== this.parser?.health ||
          parser?.fidelity !== this.parser?.fidelity ||
          parser?.restarts !== this.parser?.restarts;
        if (changed) this.parser = parser;
        if (batch.eof || batch.status.state !== 'running') {
          this.state = batch.status.state === 'failed' ? 'failed' : 'exited';
          this.renderer.setInput(false);
          this.inputs = [];
          this.inputBytes = 0;
        }
        if (batch.data.length && !(await this.processed(batch.data))) return;
        if (this.stopped) return;
        if (changed || batch.data.length)
          this.publish({ truncated: batch.truncated });
        // EOF may precede child reaping. Drain data and wait for the backend's
        // final lifecycle snapshot before stopping the loop.
        if (
          batch.eof &&
          !batch.data.length &&
          batch.status.state !== 'running' &&
          batch.status.state !== 'starting'
        ) {
          this.state = batch.status.state === 'failed' ? 'failed' : 'exited';
          this.stopIO();
          this.publish({ truncated: batch.truncated });
          return;
        }
        if (batch.data.length || waited) idle = 10;
        else {
          // An answer that came at once is paced as before, so a desktop that does not wait
          // is not asked in a tight loop.
          idle = Math.min(idle * 2, 320);
          await this.sleep(idle);
        }
      }
    } catch (error) {
      if (this.stopped) return;
      // The desktop closed this terminal itself (Stop on a native session): the tab keeps
      // what it showed and says it was closed, which is not a failure of the terminal.
      if (
        typeof error === 'object' &&
        error &&
        'code' in error &&
        error.code === 'already_closed'
      ) {
        this.state = 'closed';
        this.stopIO();
        this.publish();
        return;
      }
      // Lost IPC replies/ACKs are ambiguous: do not blindly retransmit data.
      this.fail(error);
    }
  }
  private stopIO() {
    this.stopped = true;
    this.abort.abort();
    this.renderer.setInput(false);
    this.inputCleanup?.();
    this.inputCleanup = undefined;
    this.metricsCleanup?.();
    this.metricsCleanup = undefined;
    this.observer?.disconnect();
    this.observer = undefined;
    clearTimeout(this.resizeTimer);
    this.resizeTimer = undefined;
    this.wakeSleep?.();
    this.inputs = [];
    this.inputBytes = 0;
  }
  private fail(error: unknown) {
    this.state = 'failed';
    this.stopIO();
    const code =
      typeof error === 'object' && error && 'code' in error
        ? String(error.code)
        : error instanceof Error
          ? error.message
          : '';
    const known = new Set([
      'invalid_dimensions',
      'invalid_cwd',
      'spawn_failed',
      'write_failed',
      'resize_failed',
      'busy',
      'limit_reached',
      'operation_timeout',
      'close_timeout',
      'input_limit',
      'host_unavailable',
    ]);
    this.publish({ error: known.has(code) ? code : 'terminal_error' });
    void this.closeOnce().catch(() => undefined);
  }
  private closeOnce(): Promise<void> {
    if (!this.id) return Promise.resolve();
    return (this.closeTask ??= desktop
      .closeTerminal(this.id)
      .then(() => undefined));
  }
  close(): Promise<void> {
    return (this.finishTask ??= this.finish());
  }
  private async finish(): Promise<void> {
    if (this.state === 'closed') return;
    this.state = 'closing';
    this.stopIO();
    this.publish();
    // A create already in flight must be resolved and closed rather than leaked.
    await this.startTask;
    try {
      await this.closeOnce();
      this.state = 'closed';
      this.publish();
    } catch {
      this.state = 'failed';
      this.publish({ error: 'close_failed' });
    }
    this.renderer.dispose();
  }
  /**
   * Stops reading and drawing and leaves the terminal as it is: the window is going away and
   * the terminal belongs to a host that outlives it. Nothing is closed.
   */
  detach(): Promise<void> {
    if (this.disposeTask) return this.disposeTask;
    this.disposed = true;
    this.detached = true;
    this.disposeTask = (async () => {
      this.stopIO();
      await this.startTask?.catch(() => undefined);
      this.renderer.dispose();
    })();
    return this.disposeTask;
  }
  dispose(): Promise<void> {
    if (this.disposeTask) return this.disposeTask;
    this.disposed = true;
    this.disposeTask = this.close();
    return this.disposeTask;
  }
}
