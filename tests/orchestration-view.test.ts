import { afterEach, beforeEach, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import PipelinesView from '../src/lib/features/orchestration/PipelinesView.svelte';
import type {
  F8State,
  SessionDetail,
  SessionsView,
} from '../src/lib/contracts/f8';
import {
  parseAgentRuntimeInstanceId,
  type ManagedSessionId,
} from '../src/lib/contracts/identities';
import { parseUuid } from '../src/lib/contracts/uuid';
import type { NativeSession } from '../src/lib/contracts/native';
import fixture from './fixtures/f8-gui-v1.json';

const fx = fixture as unknown as {
  session_attached: SessionDetail;
  session_ready: SessionDetail;
  sessions: SessionsView;
  state: F8State;
};
const SOURCE_ID = 'db50ad3a-9c70-45b4-8f63-8125979729ba';
const NATIVE_SOURCE_ID = '88888888888848888888888888888888';
const RECEIVER_ID = 'f0f0f0f0-f0f0-40f0-80f0-f0f0f0f0f0f0';
const SOURCE_ROOT =
  '/tmp/wyss-native-local-7cb9559c-c20d-4413-9501-a96cadfe0908/repo/work';
const TARGET_ROOT = '/tmp/m6-managed-receiver';
const SOURCE_IDENTITY = '1'.repeat(64);
const SOURCE_REVISION = '2'.repeat(64);
const TARGET_IDENTITY = '3'.repeat(64);
const TARGET_REVISION = '4'.repeat(64);
let component: ReturnType<typeof mount> | undefined;

const flush = async () => {
  for (let i = 0; i < 45; i++) await Promise.resolve();
  flushSync();
  await new Promise((resolve) => setTimeout(resolve, 10));
  for (let i = 0; i < 20; i++) await Promise.resolve();
  flushSync();
};
const text = (node: Element | null | undefined) =>
  node?.textContent?.replace(/\s+/g, ' ').trim() ?? '';
const button = (label: string) =>
  Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find(
    (item) => text(item).includes(label),
  );
const label = (fragment: string) =>
  Array.from(document.querySelectorAll('label')).find((item) =>
    text(item).includes(fragment),
  );

function managedSource(changed = false) {
  const detail = structuredClone(fx.session_attached);
  if (changed) {
    detail.session.revision = 2;
    detail.session.runtime.generation = 2;
    detail.session.runtime.fence.generation = 2;
  }
  return detail;
}

function managedReceiver(ready: boolean) {
  const detail = structuredClone(fx.session_ready);
  detail.session.id = RECEIVER_ID as ManagedSessionId;
  if (!ready) return detail;
  const instance = parseAgentRuntimeInstanceId(
    '15151515-1515-4515-8515-151515151515',
  );
  detail.binding = {
    ...detail.binding,
    state: 'bound_attached',
    workspace: TARGET_ROOT,
    mode: 'inference_only_v1',
    attached_instance: instance,
  };
  detail.session.runtime = {
    ...detail.session.runtime,
    desired: 'running',
    actual: 'ready',
    generation: 1,
    instance,
    profile: 'codex_inference_v1',
    workspace: null,
    fence: {
      session: RECEIVER_ID as ManagedSessionId,
      generation: 1,
      instance,
    },
  };
  detail.lease = {
    id: parseUuid('16161616-1616-4616-8616-161616161616'),
    state: 'active',
    issued_at: '2035-01-01T00:00:00Z',
    expires_at: '2035-01-01T01:00:00Z',
    revision: 1,
    revocation: null,
    scopes: ['session_create', 'turn_start'],
  };
  detail.gates = detail.gates.map((gate) =>
    gate.action === 'turn_start' ? { ...gate, reason: 'allowed' } : gate,
  );
  detail.turns = [];
  detail.turns_partial = false;
  return detail;
}

function snapshot() {
  const now = Math.floor(Date.now() / 1000);
  return {
    run_id: '17171717-1717-4717-8717-171717171717',
    revision: 2,
    plan_revision: 1,
    plan_digest: '5'.repeat(64),
    workspace_id: TARGET_IDENTITY,
    current_workspace_revision: TARGET_REVISION,
    state: 'running',
    failure: null,
    created_at_unix: now,
    updated_at_unix: now,
    steps: [],
  };
}

type IpcRequest = {
  method?: string;
  params?: Record<string, unknown>;
  session_id?: string;
};

function desktop(nativeRows: NativeSession[] = []) {
  const calls: { command: string; request: IpcRequest | undefined }[] = [];
  let receiverReady = false;
  let sourceChanged = false;
  mockIPC((command, payload) => {
    const request = (payload as { request?: IpcRequest } | undefined)?.request;
    calls.push({ command, request });
    if (command === 'choose_workspace') return { path: TARGET_ROOT };
    if (command === 'f8_sessions') return fx.sessions;
    if (command === 'f8_session') {
      return request?.session_id === SOURCE_ID
        ? managedSource(sourceChanged)
        : managedReceiver(receiverReady);
    }
    if (command === 'f8_session_prepare') {
      return {
        schema_version: 1,
        authority: parseUuid(fx.state.authority?.instance_id),
        id: RECEIVER_ID,
      };
    }
    if (command === 'native_sessions') return nativeRows;
    if (command === 'orchestration_call') {
      switch (request?.method) {
        case 'list':
          return [];
        case 'workspace_snapshot': {
          const root = request.params?.workspace_root;
          return root === SOURCE_ROOT
            ? {
                workspace_id: SOURCE_IDENTITY,
                workspace_revision: SOURCE_REVISION,
              }
            : {
                workspace_id: TARGET_IDENTITY,
                workspace_revision: TARGET_REVISION,
              };
        }
        case 'seal_handoff': {
          const envelope = request.params?.envelope as Record<string, unknown>;
          return {
            envelope: { ...envelope, content_digest: '6'.repeat(64) },
            first_message: '# Reviewed handoff\n\nnew session only',
          };
        }
        case 'submit_handoff':
          return snapshot();
        default:
          throw new Error(
            `unexpected orchestration method: ${request?.method}`,
          );
      }
    }
    return {};
  });
  return {
    calls,
    setReceiverReady: (ready: boolean) => {
      receiverReady = ready;
    },
    setSourceChanged: (changed: boolean) => {
      sourceChanged = changed;
    },
  };
}

async function beginManagedHandoff() {
  button('Structured handoff')!.click();
  await flush();
  const sourcePlane = label('Plano de origen')!.querySelector('select')!;
  sourcePlane.value = 'managed';
  sourcePlane.dispatchEvent(new Event('change', { bubbles: true }));
  await flush();
  const source = label('Sesión Managed de origen')!.querySelector('select')!;
  source.value = SOURCE_ID;
  source.dispatchEvent(new Event('change', { bubbles: true }));
  await flush();
  button('Elegir…')!.click();
  await flush();
  const objective = label('Objetivo confirmado por el dueño')!.querySelector(
    'input',
  )!;
  objective.value = 'Continuar el trabajo revisado';
  objective.dispatchEvent(new Event('input', { bubbles: true }));
  await flush();
}

beforeEach(() => {
  document.body.replaceChildren();
});
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  clearMocks();
  document.body.replaceChildren();
});

it('TS-HAND-01 transfers only an owner-selected excerpt, not source conversation output', async () => {
  const ipc = desktop();
  component = mount(PipelinesView, {
    target: document.body,
    props: { visible: true, f8Active: true, onruntimes: () => undefined },
  });
  await flush();
  await beginManagedHandoff();
  const context = label('Extractos elegidos')!.querySelector('textarea')!;
  context.value = 'Explicitly selected excerpt';
  context.dispatchEvent(new Event('input', { bubbles: true }));
  await flush();

  expect(label('Sesión Managed destino')).toBeUndefined();
  button('Preparar receptor Managed nuevo mediante F8')!.click();
  await flush();
  expect(
    ipc.calls.filter((call) => call.command === 'f8_session_prepare'),
  ).toHaveLength(1);
  expect(button('Configurar receptor en Runtimes')).toBeDefined();
  button('Sellar y revisar bytes exactos')!.click();
  await flush();
  expect(
    ipc.calls.filter((call) => call.request?.method === 'seal_handoff'),
  ).toHaveLength(0);

  ipc.setReceiverReady(true);
  button('Sellar y revisar bytes exactos')!.click();
  await flush();
  expect(
    ipc.calls.filter((call) => call.request?.method === 'seal_handoff'),
  ).toHaveLength(1);
  button('Confirmar digest y entregar al Managed nuevo')!.click();
  await flush();
  const submission = ipc.calls.find(
    (call) => call.request?.method === 'submit_handoff',
  )!;
  const submitted = submission.request?.params as {
    envelope: {
      target: { managed_session_id: string };
      selected_context_items: { text: string }[];
    };
    confirmation: { reviewed_digest: string };
  };
  expect(submitted.envelope.target.managed_session_id).toBe(RECEIVER_ID);
  expect(
    submitted.envelope.selected_context_items.map((item) => item.text),
  ).toEqual(['Explicitly selected excerpt']);
  expect(ipc.calls.some((call) => call.command === 'f8_turn_read')).toBe(false);
  expect(submitted.confirmation.reviewed_digest).toBe('6'.repeat(64));
});

it('TS-HAND-05 refuses a stale managed source generation before sealing or submitting', async () => {
  const ipc = desktop();
  component = mount(PipelinesView, {
    target: document.body,
    props: { visible: true, f8Active: true, onruntimes: () => undefined },
  });
  await flush();
  await beginManagedHandoff();
  ipc.setSourceChanged(true);
  button('Sellar y revisar bytes exactos')!.click();
  await flush();
  expect(
    ipc.calls.filter((call) => call.request?.method === 'seal_handoff'),
  ).toHaveLength(0);
  expect(
    ipc.calls.filter((call) => call.request?.method === 'submit_handoff'),
  ).toHaveLength(0);
  expect(text(document.querySelector('.m6-alert'))).not.toBe('');
});

it('TS-HAND-04 accepts selected Native context only into a new empty F8 recipient', async () => {
  const nativeSource: NativeSession = {
    id: NATIVE_SOURCE_ID,
    agent_id: 'codex',
    agent_name: 'Codex',
    version: '0.159.0',
    project: SOURCE_ROOT,
    profile: null,
    through_shell: false,
    state: 'running',
    started_at: 1_700_000_000,
    ended_at: null,
    exit_code: null,
    exit_signal: null,
    error: null,
    restart_of: null,
    restarts: 0,
    plane: 'native',
    terminal_id: null,
    pid: 4242,
    usage: null,
  };
  const ipc = desktop([nativeSource]);
  component = mount(PipelinesView, {
    target: document.body,
    props: { visible: true, f8Active: true, onruntimes: () => undefined },
  });
  await flush();
  button('Structured handoff')!.click();
  await flush();
  const sourcePlane = label('Plano de origen')!.querySelector('select')!;
  sourcePlane.value = 'native';
  sourcePlane.dispatchEvent(new Event('change', { bubbles: true }));
  await flush();
  const source = label('Sesión Native de origen')!.querySelector('select')!;
  source.value = NATIVE_SOURCE_ID;
  source.dispatchEvent(new Event('change', { bubbles: true }));
  await flush();
  button('Elegir…')!.click();
  await flush();
  const objective = label('Objetivo confirmado por el dueño')!.querySelector(
    'input',
  )!;
  objective.value = 'Continuar el trabajo revisado';
  objective.dispatchEvent(new Event('input', { bubbles: true }));
  await flush();
  button('Preparar receptor Managed nuevo mediante F8')!.click();
  await flush();
  ipc.setReceiverReady(true);
  button('Sellar y revisar bytes exactos')!.click();
  await flush();
  button('Confirmar digest y entregar al Managed nuevo')!.click();
  await flush();

  const submission = ipc.calls.find(
    (call) => call.request?.method === 'submit_handoff',
  )!;
  const submitted = submission.request?.params as {
    envelope: {
      source: { plane: string; session_id: string };
      target: { managed_session_id: string };
    };
  };
  expect(submitted.envelope.source.plane).toBe('native');
  expect(submitted.envelope.source.session_id).toBe(NATIVE_SOURCE_ID);
  expect(submitted.envelope.target.managed_session_id).toBe(RECEIVER_ID);
});

it('TS-HAND-02 clears the reviewed handoff when selected context changes', async () => {
  const ipc = desktop();
  component = mount(PipelinesView, {
    target: document.body,
    props: { visible: true, f8Active: true, onruntimes: () => undefined },
  });
  await flush();
  await beginManagedHandoff();
  const context = label('Extractos elegidos')!.querySelector('textarea')!;
  context.value = 'Owner-selected excerpt';
  context.dispatchEvent(new Event('input', { bubbles: true }));
  await flush();
  button('Preparar receptor Managed nuevo mediante F8')!.click();
  await flush();
  ipc.setReceiverReady(true);
  button('Sellar y revisar bytes exactos')!.click();
  await flush();
  expect(button('Confirmar digest y entregar al Managed nuevo')).toBeDefined();

  context.value = 'Changed after review';
  context.dispatchEvent(new Event('input', { bubbles: true }));
  await flush();

  expect(
    button('Confirmar digest y entregar al Managed nuevo'),
  ).toBeUndefined();
  expect(
    ipc.calls.filter((call) => call.request?.method === 'submit_handoff'),
  ).toHaveLength(0);
});

for (const state of ['verifying', 'needs_review'] as const) {
  it(`TS-ORC-review refuses unseen workspace drift during ${state} confirmation`, async () => {
    let changed = false;
    const calls: string[] = [];
    const time = Math.floor(Date.now() / 1000);
    const stepId = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa';
    const run = {
      ...snapshot(),
      state: state === 'verifying' ? 'running' : 'needs_review',
      steps: [
        {
          step_id: stepId,
          dependencies: [],
          required: true,
          provider: 'managed_codex',
          effect: 'managed_inference',
          managed_session_id: null,
          model_id: null,
          state,
          attempt_count: 1,
          attempt_id: null,
          managed_turn_id: null,
          authority_instance_id: null,
          runtime_instance_id: null,
          runtime_generation: null,
          turn_revision: null,
          workspace_revision_after: null,
          failure: null,
          updated_at_unix: time,
        },
      ],
    };
    mockIPC((command, payload) => {
      const request = (payload as { request?: IpcRequest } | undefined)?.request;
      if (command === 'native_sessions') return [];
      if (command === 'choose_workspace') return { path: TARGET_ROOT };
      if (command !== 'orchestration_call') return {};
      if (request?.method === 'list') return [run];
      if (request?.method === 'workspace_snapshot')
        return {
          workspace_id: TARGET_IDENTITY,
          workspace_revision: changed ? '9'.repeat(64) : TARGET_REVISION,
        };
      calls.push(request?.method ?? '');
      if (request?.method === 'verify_step' || request?.method === 'reconcile')
        return run;
      throw new Error('unexpected orchestration call');
    });

    component = mount(PipelinesView, {
      target: document.body,
      props: { visible: true, f8Active: false, onruntimes: () => undefined },
    });
    await flush();
    button('Elegir y observar workspace')!.click();
    await flush();
    expect(document.body.textContent).toContain(TARGET_REVISION);
    changed = true;
    const confirm =
      state === 'verifying'
        ? 'Confirmar salida tras revisar la revisión actual'
        : 'Confirmo que completó · revisar revisión actual';
    button(confirm)!.click();
    await flush();
    expect(calls).not.toContain('verify_step');
    expect(calls).not.toContain('reconcile');
    expect(document.body.textContent).not.toContain(TARGET_REVISION);
    expect(document.querySelector('[role="alert"]')).not.toBeNull();
  });
}

it('TS-ORC-review confirms the exact previously observed workspace digest', async () => {
  const calls: IpcRequest[] = [];
  const time = Math.floor(Date.now() / 1000);
  const run = {
    ...snapshot(),
    steps: [
      {
        step_id: 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa',
        dependencies: [],
        required: true,
        provider: 'managed_codex',
        effect: 'managed_inference',
        managed_session_id: null,
        model_id: null,
        state: 'verifying',
        attempt_count: 1,
        attempt_id: null,
        managed_turn_id: null,
        authority_instance_id: null,
        runtime_instance_id: null,
        runtime_generation: null,
        turn_revision: null,
        workspace_revision_after: null,
        failure: null,
        updated_at_unix: time,
      },
    ],
  };
  mockIPC((command, payload) => {
    const request = (payload as { request?: IpcRequest } | undefined)?.request;
    if (command === 'native_sessions') return [];
    if (command === 'choose_workspace') return { path: TARGET_ROOT };
    if (command !== 'orchestration_call') return {};
    if (request?.method === 'list') return [run];
    if (request?.method === 'workspace_snapshot')
      return { workspace_id: TARGET_IDENTITY, workspace_revision: TARGET_REVISION };
    if (request?.method === 'verify_step') {
      calls.push(request);
      return run;
    }
    throw new Error('unexpected orchestration call');
  });
  component = mount(PipelinesView, {
    target: document.body,
    props: { visible: true, f8Active: false, onruntimes: () => undefined },
  });
  await flush();
  button('Elegir y observar workspace')!.click();
  await flush();
  button('Confirmar salida tras revisar la revisión actual')!.click();
  await flush();
  expect(calls).toHaveLength(1);
  expect(calls[0].params?.reviewed_workspace_revision).toBe(TARGET_REVISION);
});
