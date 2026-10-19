/**
 * API credentials (F14): what the desktop says about the keys it keeps. A name, a provider
 * word, a short fingerprint and two times. There is no field that holds a key and no call that
 * returns one: a key goes in through a native window and stays sealed.
 */
export const PROVIDERS = [
  'anthropic',
  'openai',
  'google',
  'openrouter',
  'other',
] as const;
export type Provider = (typeof PROVIDERS)[number];

export interface Credential {
  id: string;
  name: string;
  provider: Provider;
  fingerprint: string;
  created_at: number;
  updated_at: number;
}

export const FAILURES = [
  'name_invalid',
  'secret_invalid',
  'unknown',
  'too_many',
  'key_store_unavailable',
  'key_lost',
  'damaged',
  'store_unavailable',
] as const;
export type CredentialFailure = (typeof FAILURES)[number] | 'internal';

export function parseCredential(value: unknown): Credential {
  const o = value as Record<string, unknown>;
  if (
    typeof o !== 'object' ||
    o === null ||
    typeof o.id !== 'string' ||
    !/^[0-9a-f]{32}$/.test(o.id) ||
    typeof o.name !== 'string' ||
    !(PROVIDERS as readonly unknown[]).includes(o.provider) ||
    typeof o.fingerprint !== 'string' ||
    !/^[0-9a-f]{12}$/.test(o.fingerprint) ||
    typeof o.created_at !== 'number' ||
    typeof o.updated_at !== 'number'
  )
    throw new Error('credential');
  // Anything else the desktop sent is dropped: only these fields are kept.
  return {
    id: o.id,
    name: o.name,
    provider: o.provider as Provider,
    fingerprint: o.fingerprint,
    created_at: o.created_at,
    updated_at: o.updated_at,
  };
}
export const parseCredentials = (value: unknown): Credential[] => {
  if (!Array.isArray(value)) throw new Error('credentials');
  return value.map(parseCredential);
};
export function parseFailure(error: unknown): CredentialFailure {
  return typeof error === 'string' &&
    (FAILURES as readonly string[]).includes(error)
    ? (error as CredentialFailure)
    : 'internal';
}
