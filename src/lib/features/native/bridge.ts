import type { TerminalSessionManager } from '../terminal/session-manager';

/**
 * Where a native session's tab is made: the terminal workspace registers its manager here
 * while it is mounted, so that an agent started from anywhere in the window opens in it.
 */
let workspace: TerminalSessionManager | undefined;
/** Work asked for while no workspace was mounted yet: it runs once one is ready. */
let waiting: Array<(manager: TerminalSessionManager) => void> = [];

export function registerWorkspace(manager: TerminalSessionManager): () => void {
  workspace = manager;
  const queued = waiting;
  waiting = [];
  for (const run of queued) manager.whenReady(() => run(manager));
  return () => {
    if (workspace === manager) workspace = undefined;
  };
}
export function currentWorkspace(): TerminalSessionManager | undefined {
  return workspace;
}
/**
 * Runs `run` on the terminal workspace once it is mounted and has its terminals in place. When the
 * Console has never been shown, the work waits for the first visit, so it never reaches a null workspace.
 */
export function whenWorkspaceReady(
  run: (manager: TerminalSessionManager) => void,
) {
  const manager = workspace;
  if (manager) manager.whenReady(() => run(manager));
  else waiting.push(run);
}
