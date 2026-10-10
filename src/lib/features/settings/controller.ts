import {
  DEFAULT_PREFERENCES,
  type Preferences,
  type SettingsFailureCode,
  type SettingsView,
} from '../../contracts/settings';
import {
  readSettings,
  resetSettings,
  SettingsCallError,
  updateSettings,
} from '../../desktop/settings-client';

export interface SettingsClient {
  read(): Promise<SettingsView>;
  update(preferences: Preferences): Promise<SettingsView>;
  reset(): Promise<SettingsView>;
}
const desktopClient: SettingsClient = {
  read: readSettings,
  update: updateSettings,
  reset: resetSettings,
};

export interface SettingsState {
  /** `failed`: the desktop could not say. The defaults are in force in the window. */
  phase: 'loading' | 'ready' | 'failed';
  view: SettingsView | null;
  /**
   * The preferences the window shows and applies: what was last confirmed, or the
   * defaults while nothing was. A change shows as soon as it is asked for and is taken
   * back if the desktop does not keep it.
   */
  preferences: Preferences;
  saving: boolean;
  /** Why the last change or reset did not happen. Cleared by the next one. */
  failure: SettingsFailureCode | null;
  /** The user was asked whether to reset and has not answered. */
  confirmingReset: boolean;
}

/**
 * The one owner of the preferences in the window. It reads them once and writes only
 * when the user changes one: nothing polls and nothing is watched. One call is in flight
 * at a time; changes asked for meanwhile are folded into the one that follows.
 */
export class SettingsController {
  private view: SettingsView | null = null;
  private phase: SettingsState['phase'] = 'loading';
  private draft: Preferences | null = null;
  private saving = false;
  private failure: SettingsFailureCode | null = null;
  private confirming = false;
  private disposed = false;
  private started = false;
  private resetting = false;
  /** The write in flight, or the last one: a reset is sent after it. */
  private writing: Promise<void> = Promise.resolve();

  constructor(
    private readonly publish: (state: SettingsState) => void,
    private readonly client: SettingsClient = desktopClient,
  ) {}

  state(): SettingsState {
    return {
      phase: this.phase,
      view: this.view,
      preferences: this.draft ?? this.view?.preferences ?? DEFAULT_PREFERENCES,
      saving: this.saving || this.resetting,
      failure: this.failure,
      confirmingReset: this.confirming,
    };
  }
  private emit() {
    if (!this.disposed) this.publish(this.state());
  }

  /** Reads the preferences. Once per window, and again only if the first read failed. */
  start() {
    if (this.started || this.disposed) return;
    this.started = true;
    this.read();
  }
  /** Asks again after a read that failed. */
  retry() {
    if (this.disposed || this.phase !== 'failed') return;
    this.phase = 'loading';
    this.emit();
    this.read();
  }
  private read() {
    void this.client.read().then(
      (view) => {
        if (this.disposed) return;
        this.view = view;
        this.phase = 'ready';
        this.emit();
      },
      () => {
        if (this.disposed) return;
        this.phase = 'failed';
        this.emit();
      },
    );
  }

  /** Asks for a change to the preferences in force. `edit` returns the whole next set. */
  change(edit: (current: Preferences) => Preferences) {
    if (this.disposed || this.phase !== 'ready' || this.resetting) return;
    this.draft = edit(
      this.draft ?? this.view?.preferences ?? DEFAULT_PREFERENCES,
    );
    this.failure = null;
    // Sent first, shown after: what is published already says it is being saved.
    this.flush();
    this.emit();
  }
  private flush() {
    if (this.saving || this.draft === null) return;
    const next = this.draft;
    this.saving = true;
    this.writing = this.client.update(next).then(
      (view) => {
        this.saving = false;
        if (this.disposed) return;
        this.view = view;
        // A change asked for while this one was written is sent next.
        if (this.draft === next) this.draft = null;
        this.emit();
        this.flush();
      },
      (error: unknown) => {
        this.saving = false;
        if (this.disposed) return;
        // Whatever is shown that the desktop did not keep is taken back.
        this.draft = null;
        this.failure =
          error instanceof SettingsCallError ? error.code : 'internal';
        this.emit();
      },
    );
  }

  /** The user wants to reset. Nothing happens until it is confirmed. */
  askReset() {
    if (this.disposed || this.phase !== 'ready' || this.confirming) return;
    this.confirming = true;
    this.emit();
  }
  cancelReset() {
    if (this.disposed || !this.confirming) return;
    this.confirming = false;
    this.emit();
  }
  /** Back to the defaults. Only after `askReset`. Sessions and history are not touched. */
  confirmReset() {
    if (this.disposed || !this.confirming || this.resetting) return;
    this.confirming = false;
    this.resetting = true;
    this.failure = null;
    // A change that has not been written yet is not written after a reset.
    this.draft = null;
    this.emit();
    void this.writing.then(() =>
      this.client.reset().then(
        (view) => {
          this.resetting = false;
          if (this.disposed) return;
          this.view = view;
          this.emit();
        },
        (error: unknown) => {
          this.resetting = false;
          if (this.disposed) return;
          this.failure =
            error instanceof SettingsCallError ? error.code : 'internal';
          this.emit();
        },
      ),
    );
  }
  dispose() {
    this.disposed = true;
  }
}
