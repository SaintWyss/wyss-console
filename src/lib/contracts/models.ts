/**
 * ModelCatalogue v1: what the managed Codex runtime offers, as the desktop projects it. It is
 * the runtime's own listing: a default mark, efforts and a replacement are what the runtime
 * said, and nothing here says an account may use a model. Never what a session uses.
 */
import { AGENT_MODES, type AgentMode } from './sessions';

export const CATALOGUE_STATES = ['listed', 'not_ready'] as const;
export type CatalogueState = (typeof CATALOGUE_STATES)[number];

export interface CatalogueEntry {
  model_id: string;
  display_name: string | null;
  provider_id: string | null;
  /** Null: the authority did not report it, which is not "no". */
  is_default: boolean | null;
  default_effort: string | null;
  efforts: string[] | null;
  /** The identifier of the model the runtime marks as replacing this one. */
  replaced_by: string | null;
  /** The date the runtime gives for retirement, as it gave it. */
  retires_at: string | null;
}

export interface ModelCatalogue {
  schema_version: 1;
  state: CatalogueState;
  mode: AgentMode | null;
  runtime_version: string | null;
  /** False: the runtime's listing continued past what the authority took. Null: not reported. */
  complete: boolean | null;
  entries: CatalogueEntry[];
  /** When the desktop read it from the authority, in Unix seconds. */
  read_at: number;
}

function invalid(field: string): Error {
  return new Error(`model_catalogue: invalid_${field}`);
}
function record(
  value: unknown,
  keys: readonly string[],
  field: string,
): Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value))
    throw invalid(field);
  const names = Object.keys(value);
  if (names.length !== keys.length || !keys.every((k) => names.includes(k)))
    throw invalid(field);
  return value as Record<string, unknown>;
}
/**
 * A provider identifier or effort as the desktop vouches for it: printable ASCII with no
 * space. A sentence, a link with words around it or a date in prose is never one.
 */
const IDENTIFIER = /^[\x21-\x7e]+$/;
function word(value: unknown, most: number, field: string): string | null {
  if (value === null) return null;
  if (
    typeof value !== 'string' ||
    value.length === 0 ||
    value.length > most ||
    !IDENTIFIER.test(value)
  )
    throw invalid(field);
  return value;
}
/** A display name may hold spaces; it is still one printable line. */
function name(value: unknown, field: string): string | null {
  if (value === null) return null;
  if (
    typeof value !== 'string' ||
    value.length === 0 ||
    value.length > 128 ||
    // eslint-disable-next-line no-control-regex
    /[\u0000-\u001f\u007f-\u009f]/.test(value)
  )
    throw invalid(field);
  return value;
}

export function parseCatalogueEntry(value: unknown): CatalogueEntry {
  const v = record(
    value,
    [
      'model_id',
      'display_name',
      'provider_id',
      'is_default',
      'default_effort',
      'efforts',
      'replaced_by',
      'retires_at',
    ],
    'entry',
  );
  const model = word(v.model_id, 256, 'model_id');
  if (model === null) throw invalid('model_id');
  let efforts: string[] | null = null;
  if (v.efforts !== null) {
    if (!Array.isArray(v.efforts) || v.efforts.length > 16)
      throw invalid('efforts');
    efforts = v.efforts.map((e) => {
      const effort = word(e, 64, 'efforts');
      if (effort === null) throw invalid('efforts');
      return effort;
    });
    if (new Set(efforts).size !== efforts.length) throw invalid('efforts');
  }
  if (v.is_default !== null && typeof v.is_default !== 'boolean')
    throw invalid('is_default');
  return {
    model_id: model,
    display_name: name(v.display_name, 'display_name'),
    provider_id: word(v.provider_id, 64, 'provider_id'),
    is_default: v.is_default,
    default_effort: word(v.default_effort, 64, 'default_effort'),
    efforts,
    replaced_by: word(v.replaced_by, 256, 'replaced_by'),
    retires_at: word(v.retires_at, 64, 'retires_at'),
  };
}

export function parseModelCatalogue(value: unknown): ModelCatalogue {
  const v = record(
    value,
    [
      'schema_version',
      'state',
      'mode',
      'runtime_version',
      'complete',
      'entries',
      'read_at',
    ],
    'catalogue',
  );
  if (v.schema_version !== 1) throw invalid('schema_version');
  if (
    typeof v.state !== 'string' ||
    !CATALOGUE_STATES.includes(v.state as never)
  )
    throw invalid('state');
  const state = v.state as CatalogueState;
  const mode =
    v.mode === null
      ? null
      : typeof v.mode === 'string' && AGENT_MODES.includes(v.mode as never)
        ? (v.mode as AgentMode)
        : (() => {
            throw invalid('mode');
          })();
  if (v.complete !== null && typeof v.complete !== 'boolean')
    throw invalid('complete');
  if (
    typeof v.read_at !== 'number' ||
    !Number.isSafeInteger(v.read_at) ||
    v.read_at < 0
  )
    throw invalid('read_at');
  if (!Array.isArray(v.entries) || v.entries.length > 100)
    throw invalid('entries');
  const entries = v.entries.map(parseCatalogueEntry);
  // A runtime that is not ready lists nothing and claims nothing about a list.
  if (state === 'not_ready' && (entries.length > 0 || v.complete !== null))
    throw invalid('state');
  return {
    schema_version: 1,
    state,
    mode,
    runtime_version: word(v.runtime_version, 128, 'runtime_version'),
    complete: v.complete,
    entries,
    read_at: v.read_at,
  };
}
