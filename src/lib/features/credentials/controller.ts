import type {
  Credential,
  CredentialFailure,
  Provider,
} from '../../contracts/api-keys';
import * as desktop from '../../desktop/credentials-client';
import { CredentialCallError } from '../../desktop/credentials-client';

export interface CredentialsClient {
  list(): Promise<Credential[]>;
  add(name: string, provider: Provider): Promise<Credential | null>;
  replace(id: string): Promise<Credential | null>;
  rename(id: string, name: string): Promise<Credential>;
  remove(id: string): Promise<boolean>;
  verify(id: string): Promise<boolean>;
}
const desktopClient: CredentialsClient = {
  list: desktop.listCredentials,
  add: desktop.addCredential,
  replace: desktop.replaceCredential,
  rename: desktop.renameCredential,
  remove: desktop.deleteCredential,
  verify: desktop.verifyCredential,
};

export interface CredentialsState {
  phase: 'idle' | 'loading' | 'ready' | 'failed';
  items: Credential[];
  busy: boolean;
  failure: CredentialFailure | null;
  notice: string | null;
}

/** The keys the desktop holds, by name. It reads while its view is shown and holds no key. */
export class CredentialsController {
  private phase: CredentialsState['phase'] = 'idle';
  private items: Credential[] = [];
  private busy = false;
  private failure: CredentialFailure | null = null;
  private notice: string | null = null;
  private disposed = false;

  constructor(
    private readonly publish: (state: CredentialsState) => void,
    private readonly client: CredentialsClient = desktopClient,
  ) {}
  state(): CredentialsState {
    return {
      phase: this.phase,
      items: this.items,
      busy: this.busy,
      failure: this.failure,
      notice: this.notice,
    };
  }
  private emit() {
    if (!this.disposed) this.publish(this.state());
  }
  private fail(error: unknown) {
    this.failure =
      error instanceof CredentialCallError ? error.failure : 'internal';
  }
  setVisible(visible: boolean) {
    if (visible) void this.refresh();
  }
  async refresh() {
    if (this.disposed) return;
    if (this.phase === 'idle') {
      this.phase = 'loading';
      this.emit();
    }
    try {
      this.items = await this.client.list();
      this.phase = 'ready';
    } catch (error) {
      this.phase = 'failed';
      this.fail(error);
    }
    this.emit();
  }
  private async act(work: () => Promise<string | null>) {
    if (this.busy || this.disposed) return;
    this.busy = true;
    this.failure = null;
    this.notice = null;
    this.emit();
    try {
      this.notice = await work();
      this.items = await this.client.list();
    } catch (error) {
      this.fail(error);
    } finally {
      this.busy = false;
      this.emit();
    }
  }
  add(name: string, provider: Provider) {
    return this.act(async () =>
      (await this.client.add(name, provider)) ? 'Key kept, sealed.' : null,
    );
  }
  replace(id: string) {
    return this.act(async () =>
      (await this.client.replace(id)) ? 'Key replaced.' : null,
    );
  }
  rename(id: string, name: string) {
    return this.act(async () => {
      await this.client.rename(id, name);
      return 'Renamed.';
    });
  }
  remove(id: string) {
    return this.act(async () =>
      (await this.client.remove(id)) ? 'Key deleted.' : null,
    );
  }
  verify(id: string) {
    return this.act(async () =>
      (await this.client.verify(id))
        ? 'The key opens and matches its checksum.'
        : 'The record is damaged: replace the key.',
    );
  }
  dispose() {
    this.disposed = true;
  }
}
