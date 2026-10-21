/**
 * A browser preview of the Runtimes interface, for looking at it: the application with a
 * scripted desktop behind it, answering what a real application really sent (the fixture
 * the Rust tests write). Nothing here is shipped: the build bundles `index.html` only.
 *
 *   npm run dev, then open /tests/preview/f8.html?s=approval&w=1200
 *
 * Scenarios (`s`): ready, approval, files, offline, replaced, empty, stopped, inactive.
 */
import { mockIPC } from '@tauri-apps/api/mocks';
import { mount } from 'svelte';
import App from '../../src/App.svelte';
import '../../src/app.css';
import fixture from '../fixtures/f8-gui-v1.json';

type Json = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
const fx = fixture as Json;
const copy = <T>(v: T): T => JSON.parse(JSON.stringify(v));
const query = new URLSearchParams(location.search);
const scenario = query.get('s') ?? 'ready';

// The fixture's own clock: an approval is open for the five minutes after its request.
const base = Date.parse(fx.approval.approval.requested_at) + 20_000;
const real = Date.now.bind(Date);
const started = real();
Date.now = () => base + (real() - started);

const detail = copy(fx.session_attached);
detail.turns = [copy(fx.turn_reading.turn)];
if (scenario === 'approval' || scenario === 'files')
  detail.approval = copy(fx.approval);
const approval = copy(fx.approval);
if (scenario === 'files') {
  const chunk =
    '@@ -1,4 +1,6 @@\n-export const answer = 41;\n+export const answer = 42;\n+// the preview of a change\n'.repeat(
      30,
    );
  const size = new TextEncoder().encode(chunk).length;
  approval.approval.kind = 'file_change';
  approval.command = null;
  approval.files = 3;
  approval.file = {
    index: 0,
    path: `${approval.workspace}/src/lib/example.ts`,
    kind: 'update',
    move_path: null,
    bytes: size * 2,
    offset: 0,
    chunk,
    next: size,
  };
  detail.approval = copy(approval);
}
const stopped = copy(fx.session_stopped);
const sessions = copy(fx.sessions);
if (scenario === 'empty') sessions.sessions = [];
if (scenario !== 'stopped' && sessions.sessions[0])
  sessions.sessions[0].runtime = copy(detail.session.runtime);
if (scenario === 'stopped') {
  sessions.sessions[0].runtime = copy(stopped.session.runtime);
}
let calls = 0;
let reads = 0;

const info = {
  terminal_id: '55555555-5555-4555-8555-555555555555',
  shell: '/bin/fish',
  cwd: '/example',
  observed_pid: 123,
  size: { rows: 24, cols: 80 },
  state: 'running',
  exit: null,
  error: null,
};
const never = () => new Promise(() => undefined);

mockIPC((command, payload) => {
  switch (command) {
    case 'f8_state':
      if (scenario === 'inactive') return fx.state_inactive;
      return fx.state;
    case 'f8_sessions':
      calls++;
      if (scenario === 'offline' && calls > 1)
        throw { code: 'unavailable', retry: true, retry_in_ms: 3000 };
      return sessions;
    case 'f8_session':
      if (scenario === 'offline' && calls > 1)
        throw { code: 'unavailable', retry: true, retry_in_ms: 3000 };
      if (scenario === 'replaced' && calls > 2) {
        const other = copy(stopped);
        other.authority = '99999999-9999-4999-8999-999999999999';
        return other;
      }
      return scenario === 'stopped' ? stopped : detail;
    case 'f8_turn_read':
      // The first reading has the events; after it nothing is new until the desktop's wait ends.
      return reads++ === 0 ? fx.turn_reading : never();
    case 'f8_approval_read': {
      // The change set has three files; the one asked for is the one answered.
      const asked = (
        payload as { request?: { file?: number | null } } | undefined
      )?.request?.file;
      const answer = copy(approval);
      if (answer.file && asked)
        answer.file = {
          ...answer.file,
          index: asked,
          path: answer.file.path.replace('example', `file${asked}`),
        };
      return answer;
    }
    case 'f8_models':
      return fx.models;
    case 'f8_audit':
      return fx.audit;
    case 'f8_audit_wait':
      return never();
    case 'get_app_info':
      return { schema_version: 1, name: 'Wyss Console', version: 'preview' };
    case 'authority_state':
    case 'terminal_read_output':
      return never();
    default:
      if (command.startsWith('f8_')) return {};
      return info;
  }
});

class Resize {
  observe() {}
  disconnect() {}
}
(globalThis as Json).ResizeObserver ??= Resize;

const target = document.getElementById('app');
if (!target) throw new Error('Application root is missing');
mount(App, { target });

// Opens the view and the session, as a person does, then the section asked for.
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
(async () => {
  await wait(300);
  const nav = [
    ...document.querySelectorAll<HTMLButtonElement>('.sidebar .nav-item'),
  ].find((b) => b.textContent?.includes('Runtimes'));
  nav?.click();
  await wait(300);
  document
    .querySelector<HTMLButtonElement>('.runtimes-feature .session-row')
    ?.click();
  await wait(300);
  const tab = query.get('tab');
  if (tab)
    [...document.querySelectorAll<HTMLButtonElement>('[role="tab"]')]
      .find((b) => b.textContent?.trim().toLowerCase() === tab)
      ?.click();
})();
