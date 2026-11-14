import type {
  Changes,
  ConfigurationFailure,
  InventoryReply,
  ItemKind,
  Provider,
} from '../../contracts/configuration';
import {
  ConfigurationCallError,
  readConfigurationChanges,
  readConfigurationInventory,
} from '../../desktop/configuration-client';

export interface ConfigurationClient {
  inventory(): Promise<InventoryReply>;
  changes(since: number): Promise<Changes>;
}
const desktopClient: ConfigurationClient = {
  inventory: readConfigurationInventory,
  changes: readConfigurationChanges,
};

/** How often the desktop is asked whether anything changed, while the view is shown. */
export const CHANGE_CHECK_MS = 1500;

export interface ConfigurationState {
  /**
   * `unavailable`: the desktop has no configuration to read from (it is not an empty
   * inventory, and is never shown as one). `failed`: a read did not come back whole.
   */
  phase: 'idle' | 'loading' | 'ready' | 'unavailable' | 'failed';
  reply: InventoryReply | null;
  /** What was shown is older than the last read, which failed. */
  stale: boolean;
  provider: Provider | 'all';
  kind: ItemKind | 'all';
  /** The id of the item whose details are open. */
  selected: string | null;
  failure: ConfigurationFailure | null;
}

/**
 * The one owner of the inventory in the window. It reads while its view is shown and
 * asks the desktop what changed, never the files: the desktop's watcher does that. It
 * writes nothing, and nothing it holds is sent back.
 */
export class ConfigurationController {
  private phase: ConfigurationState['phase'] = 'idle';
  private reply: InventoryReply | null = null;
  private stale = false;
  private provider: Provider | 'all' = 'all';
  private kind: ItemKind | 'all' = 'all';
  private selected: string | null = null;
  private failure: ConfigurationFailure | null = null;
  private visible = false;
  private disposed = false;
  private reading = false;
  /** The inventory read in flight, so that whoever asks again waits for the same one. */
  private read: Promise<void> | null = null;
  /** The change check in flight: a read asked for meanwhile starts once it has answered. */
  private checking: Promise<void> | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;

  constructor(
    private readonly publish: (state: ConfigurationState) => void,
    private readonly client: ConfigurationClient = desktopClient,
    private readonly every: number = CHANGE_CHECK_MS,
  ) {}

  state(): ConfigurationState {
    return {
      phase: this.phase,
      reply: this.reply,
      stale: this.stale,
      provider: this.provider,
      kind: this.kind,
      selected: this.selected,
      failure: this.failure,
    };
  }
  private emit() {
    if (!this.disposed) this.publish(this.state());
  }

  /** Whether the view is shown. Nothing is read, and nothing is asked, while it is not. */
  setVisible(visible: boolean) {
    if (this.disposed || visible === this.visible) return;
    this.visible = visible;
    if (!visible) {
      this.stopTimer();
      return;
    }
    // Shown again: what is held is shown at once and read again.
    void this.refresh();
  }

  /**
   * Reads the inventory now. One read at a time: the promise settles when the read that
   * answers this request has, whether it came back whole or not (`stale`/`failure` say which).
   */
  refresh(): Promise<void> {
    if (this.disposed) return Promise.resolve();
    if (this.read) return this.read;
    if (this.checking) return this.checking.then(() => this.refresh());
    this.stopTimer();
    this.reading = true;
    if (this.reply === null) {
      this.phase = 'loading';
      this.emit();
    }
    this.read = this.client.inventory().then(
      (reply) => {
        this.reading = false;
        this.read = null;
        if (this.disposed) return;
        this.reply = reply;
        this.phase = 'ready';
        this.stale = false;
        this.failure = null;
        // An item that is gone closes its details.
        if (
          this.selected !== null &&
          !reply.inventory.items.some((item) => item.id === this.selected)
        )
          this.selected = null;
        this.emit();
        this.schedule();
      },
      (error: unknown) => {
        this.reading = false;
        this.read = null;
        if (this.disposed) return;
        this.failure =
          error instanceof ConfigurationCallError ? error.code : 'internal';
        if (this.reply === null) {
          this.phase =
            this.failure === 'unavailable' ? 'unavailable' : 'failed';
        } else {
          this.stale = true;
        }
        this.emit();
        // A desktop that cannot read configuration will not start to by being asked.
        if (this.failure !== 'unavailable') this.schedule();
      },
    );
    return this.read;
  }

  private schedule() {
    if (this.disposed || !this.visible || this.timer !== null) return;
    this.timer = setTimeout(() => {
      this.timer = null;
      this.check();
    }, this.every);
  }
  private stopTimer() {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }

  /** Asks whether anything changed since the revision held; reads again only if so. */
  private check() {
    if (this.disposed || !this.visible || this.reading) return;
    const held = this.reply;
    if (held === null) {
      void this.refresh();
      return;
    }
    this.reading = true;
    this.checking = this.client.changes(held.revision).then(
      (changes) => {
        this.reading = false;
        this.checking = null;
        if (this.disposed) return;
        if (changes.result === 'same' && !this.stale) this.schedule();
        else void this.refresh();
      },
      () => {
        this.reading = false;
        this.checking = null;
        if (this.disposed) return;
        this.stale = true;
        this.emit();
        this.schedule();
      },
    );
  }

  setProvider(provider: Provider | 'all') {
    if (this.disposed || provider === this.provider) return;
    this.provider = provider;
    this.emit();
  }
  setKind(kind: ItemKind | 'all') {
    if (this.disposed || kind === this.kind) return;
    this.kind = kind;
    this.emit();
  }
  /** Opens an item's details, or closes them with `null`. */
  select(id: string | null) {
    if (this.disposed || id === this.selected) return;
    this.selected = id;
    this.emit();
  }

  dispose() {
    this.disposed = true;
    this.stopTimer();
  }
}
