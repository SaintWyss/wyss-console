import { afterEach, expect, it, vi } from 'vitest';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import {
  TerminalSessionManager,
  type WorkspaceState,
} from '../src/lib/features/terminal/session-manager';
import {
  registerWorkspace,
  whenWorkspaceReady,
} from '../src/lib/features/native/bridge';
import { reply } from './native-harness';

vi.mock('../src/lib/features/terminal/renderer', () => ({
  createRenderer: vi.fn(),
}));
let manager: TerminalSessionManager | undefined;
let release: (() => void) | undefined;
afterEach(async () => {
  release?.();
  await manager?.dispose();
  clearMocks();
});
const flush = async () => {
  for (let i = 0; i < 60; i++) await Promise.resolve();
};

it.each([false, true])(
  'adopts a Native session exactly once when its reply arrives before ready (registered=%j)',
  async (registered) => {
    const started = reply();
    let finish!: (held: unknown) => void;
    const held = new Promise((resolve) => {
      finish = resolve;
    });
    const commands: string[] = [];
    mockIPC((command) => {
      commands.push(command);
      if (command === 'host_terminals') return held;
      if (command === 'layout_read') return null;
      return null;
    });
    let state: WorkspaceState = { sessions: [], tabs: [], creating: false };
    manager = new TerminalSessionManager(
      (value) => {
        state = value;
      },
      undefined,
      true,
    );
    if (registered) {
      release = registerWorkspace(manager);
      manager.start();
    }
    let deliveries = 0;
    const deliver = () =>
      whenWorkspaceReady((workspace) => {
        deliveries++;
        const tab = workspace.tabOf(started.session.id);
        if (tab) workspace.activate(tab);
        else workspace.createNative(started);
      });
    deliver();
    deliver();
    if (!registered) {
      release = registerWorkspace(manager);
      manager.start();
    }
    expect(deliveries).toBe(0);
    finish([
      {
        terminal: started.terminal,
        title: 'Claude Code',
        created_at: 1,
        native: {
          session_id: started.session.id,
          agent_name: started.session.agent_name,
          project: started.session.project,
          profile: null,
          through_shell: true,
        },
      },
    ]);
    await flush();
    expect(deliveries).toBe(2);
    expect(state.tabs).toHaveLength(1);
    expect(state.sessions).toHaveLength(1);
    expect(state.sessions[0].native?.sessionId).toBe(started.session.id);
    expect(
      commands.filter((command) => command === 'create_terminal'),
    ).toHaveLength(0);
    expect(
      commands.filter((command) => command === 'host_terminals'),
    ).toHaveLength(1);
  },
);
