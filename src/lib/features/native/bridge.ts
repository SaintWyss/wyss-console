import type { TerminalSessionManager } from '../terminal/session-manager';

/**
 * Where a native session's tab is made: the terminal workspace registers its manager here
 * while it is mounted, so that an agent started from anywhere in the window opens in it.
 */
let workspace: TerminalSessionManager | undefined;

export function registerWorkspace(manager: TerminalSessionManager): () => void {
  workspace = manager;
  return () => {
    if (workspace === manager) workspace = undefined;
  };
}
export function currentWorkspace(): TerminalSessionManager | undefined {
  return workspace;
}
