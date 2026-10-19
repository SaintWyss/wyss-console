import { invoke } from '@tauri-apps/api/core';
import {
  parseCredential,
  parseCredentials,
  parseFailure,
  type Credential,
  type CredentialFailure,
  type Provider,
} from '../contracts/api-keys';

export class CredentialCallError extends Error {
  constructor(readonly failure: CredentialFailure) {
    super(`credentials: ${failure}`);
  }
}
async function call(
  command: string,
  args: Record<string, unknown> = {},
): Promise<unknown> {
  try {
    return await invoke<unknown>(command, args);
  } catch (error) {
    throw new CredentialCallError(parseFailure(error));
  }
}
function checked<T>(answer: unknown, parse: (v: unknown) => T): T {
  try {
    return parse(answer);
  } catch {
    throw new CredentialCallError('internal');
  }
}

export const listCredentials = async (): Promise<Credential[]> =>
  checked(await call('credentials_list'), parseCredentials);
/** The key is asked for in a native window; `null` when it was dismissed. */
export async function addCredential(
  name: string,
  provider: Provider,
): Promise<Credential | null> {
  const answer = await call('credential_add', { request: { name, provider } });
  return answer === null ? null : checked(answer, parseCredential);
}
export async function replaceCredential(
  id: string,
): Promise<Credential | null> {
  const answer = await call('credential_replace', { request: { id } });
  return answer === null ? null : checked(answer, parseCredential);
}
export async function renameCredential(
  id: string,
  name: string,
): Promise<Credential> {
  return checked(
    await call('credential_rename', { request: { id, name } }),
    parseCredential,
  );
}
/** Asks again in a native window; `false` when it was declined. */
export async function deleteCredential(id: string): Promise<boolean> {
  return (await call('credential_delete', { request: { id } })) === true;
}
export async function verifyCredential(id: string): Promise<boolean> {
  return (await call('credential_verify', { request: { id } })) === true;
}
