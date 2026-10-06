import { TerminalController, type TerminalViewState } from './controller';
import { createRenderer } from './renderer';

let owner: TerminalController | undefined;
let retirement: Promise<void> = Promise.resolve();

/** One view lease, not a single-terminal backend assumption. Serialize remount cleanup. */
export function mountTerminal(
  host: HTMLElement,
  update: (state: TerminalViewState) => void,
) {
  if (owner) retirement = owner.dispose();
  const controller = new TerminalController(host, createRenderer(host), update);
  owner = controller;
  void retirement.then(() => controller.start());
  return {
    close: () => controller.close(),
    dispose() {
      const cleanup = controller.dispose();
      if (owner === controller) {
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
