import { TerminalSessionManager, type WorkspaceState } from './session-manager';

let owner: TerminalSessionManager | undefined;
let retirement: Promise<void> = Promise.resolve();

/** One workspace lease; each manager owns multiple independent terminal controllers. */
export function mountWorkspace(update: (state: WorkspaceState) => void) {
  if (owner) retirement = owner.dispose();
  const manager = new TerminalSessionManager(update, undefined, true);
  owner = manager;
  void retirement.then(() => manager.start());
  return {
    manager,
    dispose() {
      const cleanup = manager.dispose();
      if (owner === manager) {
        owner = undefined;
        retirement = cleanup;
      }
      return cleanup;
    },
  };
}
if (import.meta.hot)
  import.meta.hot.dispose(() => {
    void owner?.dispose();
  });
