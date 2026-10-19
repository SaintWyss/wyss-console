import { afterEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  parseCredential,
  parseCredentials,
  parseFailure,
} from '../src/lib/contracts/api-keys';
import {
  CredentialsController,
  type CredentialsClient,
} from '../src/lib/features/credentials/controller';
import CredentialsView from '../src/lib/features/credentials/CredentialsView.svelte';
import { addCredential } from '../src/lib/desktop/credentials-client';

const item = {
  id: 'a'.repeat(32),
  name: 'Work',
  provider: 'anthropic',
  fingerprint: 'abcdef012345',
  created_at: 1_700_000_000,
  updated_at: 1_700_000_000,
};
const settle = async () => {
  for (let i = 0; i < 40; i++) await Promise.resolve();
  flushSync();
};
let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
});

describe('the contract', () => {
  it('keeps the listed fields and drops anything else the desktop sent, such as a key', () => {
    const parsed = parseCredential({ ...item, secret: 'sk-leak', value: 'x' });
    expect(Object.keys(parsed).sort()).toEqual([
      'created_at',
      'fingerprint',
      'id',
      'name',
      'provider',
      'updated_at',
    ]);
    expect(JSON.stringify(parsed)).not.toContain('sk-leak');
  });
  it('refuses what is not a credential, and reads a failure word', () => {
    expect(() => parseCredential({ ...item, id: '../x' })).toThrow();
    expect(() => parseCredential({ ...item, provider: 'evil' })).toThrow();
    expect(() => parseCredential({ ...item, fingerprint: 'zz' })).toThrow();
    expect(() => parseCredentials('x')).toThrow();
    expect(parseFailure('key_lost')).toBe('key_lost');
    expect(parseFailure({ code: 'x' })).toBe('internal');
  });
  it('asks the desktop for a name and a service only: the key is typed in a native window', async () => {
    const calls: [string, unknown][] = [];
    mockIPC((command, args) => {
      calls.push([command, args]);
      return null;
    });
    expect(await addCredential('Work', 'openai')).toBeNull();
    expect(calls).toEqual([
      ['credential_add', { request: { name: 'Work', provider: 'openai' } }],
    ]);
  });
});

function client(over: Partial<CredentialsClient> = {}): CredentialsClient {
  return {
    list: vi.fn(async () => [parseCredential(item)]),
    add: vi.fn(async () => parseCredential(item)),
    replace: vi.fn(async () => parseCredential(item)),
    rename: vi.fn(async () => parseCredential(item)),
    remove: vi.fn(async () => true),
    verify: vi.fn(async () => true),
    ...over,
  };
}

describe('the controller', () => {
  it('lists, adds with a notice, and says nothing when the native window was dismissed', async () => {
    let last = new CredentialsController(() => undefined).state();
    const c = client({ add: vi.fn(async () => null) });
    const controller = new CredentialsController((s) => (last = s), c);
    await controller.refresh();
    expect(last.items).toHaveLength(1);
    await controller.add('Work', 'anthropic');
    expect(c.add).toHaveBeenCalledWith('Work', 'anthropic');
    expect(last.notice).toBeNull();
    expect(last.failure).toBeNull();
  });
  it('shows a refusal as a word, and one call at a time', async () => {
    const { CredentialCallError } =
      await import('../src/lib/desktop/credentials-client');
    let last = new CredentialsController(() => undefined).state();
    const c = client({
      add: vi.fn(async () => {
        throw new CredentialCallError('key_store_unavailable');
      }),
    });
    const controller = new CredentialsController((s) => (last = s), c);
    await controller.add('Work', 'anthropic');
    expect(last.failure).toBe('key_store_unavailable');
    expect(last.busy).toBe(false);
  });
});

describe('the view', () => {
  it('lists names, services and checksums, asks for a key only through the desktop, and has nowhere to type one', async () => {
    const c = client();
    let state = new CredentialsController(() => undefined).state();
    const controller = new CredentialsController((s) => (state = s), c);
    await controller.refresh();
    component = mount(CredentialsView, {
      target: document.body,
      props: { credentials: state, controller, visible: true },
    });
    await settle();
    const text = document.body.textContent ?? '';
    expect(text).toContain('Work');
    expect(text).toContain('abcdef012345');
    // The only text field takes a name; there is no password or key field in the page.
    expect(document.querySelectorAll('input[type="password"]')).toHaveLength(0);
    const name = document.querySelector<HTMLInputElement>('#cred-name')!;
    name.value = 'Home';
    name.dispatchEvent(new Event('input', { bubbles: true }));
    await settle();
    document.querySelector<HTMLFormElement>('form')!.requestSubmit();
    await settle();
    expect(c.add).toHaveBeenCalledWith('Home', 'anthropic');
  });
});
