import type { ModelCatalogue } from '../../contracts/models';
import type { SessionFailureCode } from '../../contracts/sessions';
import {
  CatalogueCallError,
  readModelCatalogue,
} from '../../desktop/models-client';

export interface ModelsClient {
  catalogue(): Promise<ModelCatalogue>;
}
const desktopClient: ModelsClient = { catalogue: readModelCatalogue };

export interface ModelsState {
  phase: 'idle' | 'loading' | 'ready' | 'failed';
  /** What the managed runtime offers, as it was last read. Null before the first read. */
  catalogue: ModelCatalogue | null;
  /** What is shown is older than the last read, which failed. */
  stale: boolean;
  failure: SessionFailureCode | null;
}

/**
 * The one owner of the managed runtime's catalogue in the window. It reads once when a view
 * that needs it is shown, and again only when asked. The catalogue is the runtime's own
 * listing, held in memory and never written anywhere; it is not what any session uses.
 */
export class ModelsController {
  private phase: ModelsState['phase'] = 'idle';
  private catalogue: ModelCatalogue | null = null;
  private stale = false;
  private failure: SessionFailureCode | null = null;
  private disposed = false;
  /** The read in flight, so that whoever asks again waits for the same one. */
  private reading: Promise<void> | null = null;

  constructor(
    private readonly publish: (state: ModelsState) => void,
    private readonly client: ModelsClient = desktopClient,
  ) {}

  state(): ModelsState {
    return {
      phase: this.phase,
      catalogue: this.catalogue,
      stale: this.stale,
      failure: this.failure,
    };
  }
  private emit() {
    if (!this.disposed) this.publish(this.state());
  }
  /** A view that needs the catalogue was shown: read it once. */
  setVisible(visible: boolean) {
    if (visible && this.phase === 'idle') void this.refresh();
  }
  /** Reads the list now. Settles when the read has answered; a read in flight is the one awaited. */
  refresh(): Promise<void> {
    if (this.disposed) return Promise.resolve();
    return (this.reading ??= this.read());
  }
  private async read() {
    if (this.phase === 'idle') {
      this.phase = 'loading';
      this.emit();
    }
    try {
      this.catalogue = await this.client.catalogue();
      this.phase = 'ready';
      this.stale = false;
      this.failure = null;
    } catch (error) {
      this.failure =
        error instanceof CatalogueCallError ? error.failure.code : 'internal';
      // What was shown stays, said to be older; nothing is invented to fill the gap.
      this.stale = this.catalogue !== null;
      this.phase = this.catalogue !== null ? 'ready' : 'failed';
    } finally {
      this.reading = null;
    }
    this.emit();
  }
  dispose() {
    this.disposed = true;
  }
}
