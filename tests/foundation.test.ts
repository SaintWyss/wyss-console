import { afterEach, describe, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { mockIPC } from '@tauri-apps/api/mocks';
import App from '../src/App.svelte';
import { getAppInfo } from '../src/lib/desktop/client';
import type { AppInfo } from '../src/lib/contracts/app-info';
import fixture from './fixtures/app-info-v1.json';

let app: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (app) await unmount(app);
  app = undefined;
});

function render() {
  app = mount(App, { target: document.body });
  flushSync();
}

describe('AppInfo IPC', () => {
  it('uses get_app_info and matches the Rust serialization fixture', async () => {
    const commands: string[] = [];
    mockIPC((command) => {
      commands.push(command);
      return fixture;
    });

    const info = await getAppInfo();
    expect(commands).toEqual(['get_app_info']);
    expect(info).toEqual(fixture);
    expect(Object.keys(info).sort()).toEqual([
      'name',
      'schema_version',
      'version',
    ]);
  });

  it('shows readiness only after IPC resolves and renders its version', async () => {
    let resolve!: (info: AppInfo) => void;
    mockIPC(() => new Promise<AppInfo>((done) => (resolve = done)));
    render();

    expect(document.querySelector('[role="status"]')?.textContent).toContain(
      'Loading',
    );
    expect(document.body.textContent).not.toContain('Foundation ready');
    resolve({ schema_version: 1, name: 'Wyss Console', version: '9.8.7-test' });
    await expect
      .poll(() => document.querySelector('[role="status"]')?.textContent)
      .toBe('Foundation ready');

    expect(document.querySelector('h1')?.textContent).toBe('Wyss Console');
    expect(document.querySelector('[role="status"]')?.textContent).toBe(
      'Foundation ready',
    );
    expect(document.querySelector('dl')?.textContent).toContain('9.8.7-test');
    expect(document.querySelector('dl')?.textContent).not.toContain('0.1.0');
  });

  it('shows an error instead of readiness when IPC rejects without exposing details', async () => {
    mockIPC(() => Promise.reject(new Error('private backend detail')));
    render();
    await expect
      .poll(() => document.querySelector('[role="alert"]')?.textContent)
      .toContain('Unable to load');

    expect(document.querySelector('[role="alert"]')?.textContent).toContain(
      'Unable to load',
    );
    expect(document.body.textContent).not.toContain('Foundation ready');
    expect(document.body.textContent).not.toContain('private backend detail');
    expect(document.querySelector('dl')).toBeNull();
  });
});
