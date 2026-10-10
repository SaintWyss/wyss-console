import { invoke } from '@tauri-apps/api/core';
import { parseLifecycleView, type LifecycleView } from '../contracts/lifecycle';

/**
 * The lifecycle calls the window can make: one read, and Retry. Neither names a
 * service, a command, a path or a process: what is installed and what is asked of the
 * service manager is the desktop's own business.
 */

/** Where the lifecycle stands now (`after` null) or once it differs from `after`. */
export async function readLifecycle(
  after: number | null,
): Promise<LifecycleView> {
  return parseLifecycleView(
    await invoke<unknown>('authority_lifecycle', { request: { after } }),
  );
}

/**
 * Asks for one more attempt. It answers at once; the attempt, if one was started, is
 * followed through `readLifecycle`.
 */
export async function retryAuthority(): Promise<LifecycleView> {
  return parseLifecycleView(
    await invoke<unknown>('authority_retry', { request: {} }),
  );
}
