import * as desktop from '../../desktop/terminal-client';
import type { TerminalId } from '../../contracts/identities';
import type { TerminalInfo, TerminalSize } from '../../contracts/terminal';
import type { TerminalRenderer } from './renderer';

export type ViewState =
  'starting' | 'running' | 'exited' | 'closing' | 'closed' | 'failed';
export interface TerminalViewState {
  state: ViewState;
  info?: TerminalInfo;
  error?: string;
  truncated?: boolean;
}
const INPUT_CAPACITY = 64 * 1024;
const WRITE_LIMIT = 4096;

/** One local human-terminal owner. No command interpretation or domain authority. */
export class TerminalController {
  private id?: TerminalId;
  private info?: TerminalInfo;
  private state: ViewState = 'starting';
  private stopped = false;
  private disposed = false;
  private startTask?: Promise<void>;
  private closeTask?: Promise<void>;
  private disposeTask?: Promise<void>;
  private finishTask?: Promise<void>;
  private inputCleanup?: () => void;
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

  constructor(
    private readonly host: HTMLElement,
    private readonly renderer: TerminalRenderer,
    private readonly update: (state: TerminalViewState) => void,
  ) {}

  start(): Promise<void> {
    return (this.startTask ??= this.initialize());
  }
  private publish(extra: Partial<TerminalViewState> = {}) {
    if (!this.disposed)
      this.update({ state: this.state, info: this.info, ...extra });
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
      const size = this.renderer.fit() ?? { rows: 24, cols: 80 };
      this.info = await desktop.createTerminal(size);
      this.id = this.info.terminal_id;
      this.sentSize = size;
      if (this.stopped) {
        await this.closeOnce();
        return;
      }
      this.state = 'running';
      this.renderer.setInput(true);
      this.publish();
      this.renderer.focus();
      this.requestFit();
      void this.output();
    } catch (error) {
      if (!this.stopped) this.fail(error);
    }
  }
  private requestFit() {
    if (this.stopped || this.resizeTimer) return;
    this.resizeTimer = setTimeout(() => {
      this.resizeTimer = undefined;
      const size = this.renderer.fit();
      if (size) {
        this.desiredSize = size;
        void this.resize();
      }
    }, 60);
  }
  private async resize() {
    if (this.resizing || !this.id || this.state !== 'running' || this.stopped)
      return;
    this.resizing = true;
    try {
      while (!this.stopped && this.state === 'running' && this.desiredSize) {
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
    try {
      while (!this.stopped && this.state === 'running' && this.inputs.length) {
        const data = this.inputs[0];
        const chunk = data.subarray(0, WRITE_LIMIT);
        const written = await desktop.writeTerminal(this.id, chunk);
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
        const batch = await desktop.readTerminalOutput(this.id);
        if (this.stopped) return;
        this.info = batch.status;
        if (batch.eof || batch.status.state !== 'running') {
          this.state = batch.status.state === 'failed' ? 'failed' : 'exited';
          this.renderer.setInput(false);
          this.inputs = [];
          this.inputBytes = 0;
        }
        if (batch.data.length && !(await this.processed(batch.data))) return;
        if (this.stopped) return;
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
        if (batch.data.length) idle = 10;
        else {
          idle = Math.min(idle * 2, 320);
          await this.sleep(idle);
        }
      }
    } catch (error) {
      // Lost IPC replies/ACKs are ambiguous: do not blindly retransmit data.
      if (!this.stopped) this.fail(error);
    }
  }
  private stopIO() {
    this.stopped = true;
    this.abort.abort();
    this.renderer.setInput(false);
    this.inputCleanup?.();
    this.inputCleanup = undefined;
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
  dispose(): Promise<void> {
    if (this.disposeTask) return this.disposeTask;
    this.disposed = true;
    this.disposeTask = this.close();
    return this.disposeTask;
  }
}
