import { invoke } from '@tauri-apps/api/core';
import { parseAuthorityView, type AuthorityView } from '../contracts/authority';

/**
 * The desktop's projection now (`after` null) or once its revision differs from
 * `after`. This is the only authority call the window can make: it reads.
 */
export async function readAuthorityState(
  after: number | null,
): Promise<AuthorityView> {
  return parseAuthorityView(
    await invoke<unknown>('authority_state', { request: { after } }),
  );
}
