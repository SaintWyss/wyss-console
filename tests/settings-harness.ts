import {
  DEFAULT_PREFERENCES,
  FONT_SIZE,
  SCROLLBACK,
  parsePreferences,
  type Preferences,
  type SettingsView,
  type StorageState,
} from '../src/lib/contracts/settings';
import type { SettingsClient } from '../src/lib/features/settings/controller';
import { SettingsCallError } from '../src/lib/desktop/settings-client';

/** The view the desktop sends, with the defaults in force unless said otherwise. */
export function settingsView(
  over: Partial<SettingsView> & { preferences?: Preferences } = {},
): SettingsView {
  return {
    schema_version: 1,
    storage: 'ready',
    preferences: DEFAULT_PREFERENCES,
    about: {
      version: '0.2.0-test',
      revision: '0123456789ab',
      platform: 'linux-x86_64',
    },
    ...over,
  };
}

export function withPreferences(
  edit: (p: Preferences) => Preferences,
): Preferences {
  return edit(structuredClone(DEFAULT_PREFERENCES));
}

/**
 * A desktop that keeps preferences the way the real one does: it judges every value
 * again, refuses a newer file's owner any change, and forgets nothing between calls.
 */
export class FakeDesktopSettings {
  storage: StorageState;
  preferences: Preferences;
  calls: { command: 'read' | 'update' | 'reset'; sent?: unknown }[] = [];
  /** Held writes: each update waits for `release` when set. */
  hold = false;
  private releases: (() => void)[] = [];
  failNext: 'not_saved' | 'busy' | 'internal' | null = null;
  failReads = false;

  constructor(
    preferences: Preferences = DEFAULT_PREFERENCES,
    storage: StorageState = 'ready',
  ) {
    this.preferences = preferences;
    this.storage = storage;
  }
  view(): SettingsView {
    return settingsView({
      storage: this.storage,
      preferences: this.preferences,
    });
  }
  release() {
    this.releases.shift()?.();
  }
  private async maybeHold() {
    if (this.hold)
      await new Promise<void>((resolve) => this.releases.push(resolve));
  }
  async read(): Promise<SettingsView> {
    this.calls.push({ command: 'read' });
    if (this.failReads) throw new SettingsCallError('internal');
    return this.view();
  }
  async update(sent: Preferences): Promise<SettingsView> {
    this.calls.push({ command: 'update', sent: structuredClone(sent) });
    await this.maybeHold();
    if (this.failNext) {
      const code = this.failNext;
      this.failNext = null;
      throw new SettingsCallError(code);
    }
    if (this.storage === 'newer') throw new SettingsCallError('read_only');
    // Rust judges bounds again whatever the window checked.
    const checked = parsePreferences(sent);
    if (
      checked.terminal.font_size < FONT_SIZE.min ||
      checked.terminal.scrollback < SCROLLBACK.min
    )
      throw new SettingsCallError('invalid');
    this.preferences = checked;
    this.storage = 'ready';
    return this.view();
  }
  async reset(): Promise<SettingsView> {
    this.calls.push({ command: 'reset' });
    await this.maybeHold();
    this.preferences = DEFAULT_PREFERENCES;
    this.storage = 'ready';
    return this.view();
  }
  client(): SettingsClient {
    return {
      read: () => this.read(),
      update: (p) => this.update(p),
      reset: () => this.reset(),
    };
  }
  /** The same desktop, answering the three commands of a mocked IPC. */
  ipc(
    command: string,
    request: Record<string, unknown>,
  ): Promise<unknown> | undefined {
    if (command === 'settings_read') return this.read();
    if (command === 'settings_update')
      return this.update(request.preferences as Preferences).catch((error) =>
        Promise.reject({ code: (error as SettingsCallError).code }),
      );
    if (command === 'settings_reset') return this.reset();
    return undefined;
  }
}
