import { afterEach, describe, expect, it } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  closeTerminal,
  createTerminal,
  readTerminalOutput,
  resizeTerminal,
  terminalStatus,
  writeTerminal,
} from '../src/lib/desktop/terminal-client';
import { parseTerminalId } from '../src/lib/contracts/identities';
import type {
  TerminalInfo,
  TerminalOutputWire,
} from '../src/lib/contracts/terminal';
import { parseUuid } from '../src/lib/contracts/uuid';
import { runPtySmoke } from '../src/lib/features/pty-diagnostic/smoke';
import { mount, unmount, flushSync } from 'svelte';
import PtyDiagnostic from '../src/lib/features/pty-diagnostic/PtyDiagnostic.svelte';

const id = parseTerminalId('11111111-1111-4111-8111-111111111111');
const receipt = parseUuid('22222222-2222-4222-8222-222222222222');
const info: TerminalInfo = {
  terminal_id: id,
  shell: '/bin/fish',
  cwd: '/example',
  observed_pid: 123,
  size: { rows: 24, cols: 80 },
  state: 'running',
  exit: null,
  error: null,
};
const bytes = new TextEncoder();
function output(data: number[], batch_id = receipt): TerminalOutputWire {
  return {
    batch_id: data.length ? batch_id : null,
    data,
    eof: false,
    truncated: false,
    status: info,
  };
}
let component: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  mockIPC(() => ({ ...info, state: 'closed' }));
  await closeTerminal(id);
});

describe('narrow terminal IPC bridge', () => {
  it('sends only size/cwd, byte input and exact logical TerminalId', async () => {
    const calls: [string, unknown][] = [];
    mockIPC((command, args) => {
      calls.push([command, args]);
      return command === 'terminal_write' ? 2 : info;
    });
    await createTerminal({ rows: 24, cols: 80 }, '/example', 'Work');
    expect(await writeTerminal(id, Uint8Array.from([0, 255]))).toBe(2);
    await resizeTerminal(id, { rows: 37, cols: 111 });
    await terminalStatus(id);
    await closeTerminal(id);
    expect(calls).toEqual([
      [
        'create_terminal',
        {
          request: {
            size: { rows: 24, cols: 80 },
            cwd: '/example',
            title: 'Work',
          },
        },
      ],
      ['terminal_write', { request: { terminal_id: id, data: [0, 255] } }],
      [
        'terminal_resize',
        { request: { terminal_id: id, size: { rows: 37, cols: 111 } } },
      ],
      ['terminal_status', { request: { terminal_id: id } }],
      ['close_terminal', { request: { terminal_id: id } }],
    ]);
  });
  it('coalesces concurrent reads and acknowledges exactly one bounded batch', async () => {
    let resolve!: (value: TerminalOutputWire) => void;
    const acknowledgements: unknown[] = [];
    mockIPC((command, args) => {
      if (command === 'create_terminal') return info;
      acknowledgements.push(args);
      return new Promise<TerminalOutputWire>((done) => (resolve = done));
    });
    await createTerminal(info.size);
    const first = readTerminalOutput(id);
    const same = readTerminalOutput(id);
    expect(same).toBe(first);
    expect(acknowledgements).toHaveLength(1);
    resolve(output([0xe2, 0x82, 0xac, 0x1b, 255]));
    expect(Array.from((await first).data)).toEqual([
      0xe2, 0x82, 0xac, 0x1b, 255,
    ]);
    const next = readTerminalOutput(id);
    resolve(output([]));
    await next;
    expect(acknowledgements).toEqual([
      { request: { terminal_id: id, ack: null } },
      { request: { terminal_id: id, ack: receipt } },
    ]);
  });
  it.each([
    { data: [256] },
    { data: [-1] },
    { data: [1.5] },
    { data: Array(16385).fill(0) },
  ])('rejects malformed byte transport', async ({ data: invalid }) => {
    mockIPC((command) =>
      command === 'create_terminal' ? info : output(invalid),
    );
    await createTerminal(info.size);
    await expect(readTerminalOutput(id)).rejects.toThrow('invalid_output');
  });
  it('rejects empty/oversized input and unknown stream without IPC', async () => {
    let calls = 0;
    mockIPC(() => {
      calls++;
      return info;
    });
    await expect(writeTerminal(id, new Uint8Array())).rejects.toThrow(
      'invalid_input',
    );
    await expect(writeTerminal(id, new Uint8Array(4097))).rejects.toThrow(
      'invalid_input',
    );
    await expect(
      readTerminalOutput(
        parseTerminalId('33333333-3333-4333-8333-333333333333'),
      ),
    ).rejects.toThrow('unknown_terminal');
    expect(calls).toBe(0);
  });
});

describe('removable PTY diagnostic', () => {
  it('exercises create/write/output/resize/close and handles split UTF-8', async () => {
    const commands: string[] = [];
    let reads = 0;
    mockIPC((command) => {
      commands.push(command);
      if (command === 'terminal_read_output') {
        const data = [
          [0xe2],
          [0x82, 0xac, ...bytes.encode('\rWYSS_PTY_OK\n/example\n')],
          [...bytes.encode('32 100\nWYSS_RESIZE_DONE\n')],
        ][reads++];
        return output(data);
      }
      return command === 'terminal_write'
        ? 30
        : command === 'close_terminal'
          ? { ...info, state: 'closed' }
          : info;
    });
    const result = await runPtySmoke(() => undefined);
    expect(result.preview).toContain('€');
    expect(result.preview).not.toContain('�');
    expect(result.info.state).toBe('closed');
    expect(commands).toEqual([
      'create_terminal',
      'terminal_write',
      'terminal_read_output',
      'terminal_read_output',
      'terminal_resize',
      'terminal_write',
      'terminal_read_output',
      'close_terminal',
    ]);
  });
  it('requests cleanup after a failed operation without exposing backend details', async () => {
    const commands: string[] = [];
    mockIPC((command) => {
      commands.push(command);
      if (command === 'terminal_write')
        return Promise.reject(new Error('private detail'));
      return info;
    });
    component = mount(PtyDiagnostic, { target: document.body });
    flushSync();
    document.querySelector('button')!.click();
    await expect
      .poll(() => document.body.textContent)
      .toContain('PTY smoke failed');
    expect(document.body.textContent).not.toContain('private detail');
    expect(commands).toEqual([
      'create_terminal',
      'terminal_write',
      'close_terminal',
    ]);
  });
  it('renders observed smoke success and never starts a terminal on mount', async () => {
    let calls = 0;
    mockIPC((command) => {
      calls++;
      if (command === 'terminal_read_output')
        return output([
          ...bytes.encode(
            '\r\nWYSS_PTY_OK\r\n/example\r\n32 100\r\nWYSS_RESIZE_DONE\r\n',
          ),
        ]);
      return command === 'close_terminal' ? { ...info, state: 'closed' } : info;
    });
    component = mount(PtyDiagnostic, { target: document.body });
    flushSync();
    expect(calls).toBe(0);
    document.querySelector('button')!.click();
    await expect
      .poll(() => document.body.textContent)
      .toContain('PTY smoke PASS');
    expect(document.body.textContent).toContain('/bin/fish');
    expect(document.body.textContent).toContain('State: closed');
  });
});
