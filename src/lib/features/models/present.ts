import type { ConfigItem, Provider } from '../../contracts/configuration';
import type { CatalogueEntry, ModelCatalogue } from '../../contracts/models';
import type { Profile } from '../../contracts/native';
import type {
  SessionFailureCode,
  SessionSummary,
} from '../../contracts/sessions';
import {
  PROVIDER_LABEL,
  SCOPE_LABEL,
  SOURCE_LABEL,
} from '../configuration/present';
import { AGENT_LABEL } from '../native/present';
import { observedModelLine, sessionLabel } from '../sessions/present';

/**
 * The four facts about "the model", kept apart on purpose (12 §1.1): what a runtime
 * OFFERS, what an agent's files are CONFIGURED with, what a profile SELECTS for a session it
 * starts, and what a runtime REPORTED in use. None is filled from another: a default in a
 * list, a value in a file or a flag passed at start says what was asked for, not what answered.
 */

/** Why the list could not be read, in the words of a person, never a code. */
export function catalogueFailure(code: SessionFailureCode | null): string {
  switch (code) {
    case 'unavailable':
      return 'The list could not be read: the authority did not answer.';
    case 'incompatible':
      return 'The list could not be read: the authority speaks another version than this app.';
    case 'busy':
      return 'The list could not be read: the authority is busy. Try again.';
    default:
      return 'The list could not be read. Nothing is shown rather than something unverified.';
  }
}

export const ENTITLEMENT_NOTE =
  'This is the runtime’s own list. A model being listed does not mean your account can use it: that is not known here.';

/** When a value was read, as a person reads it. */
export function readAt(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString(undefined, {
    dateStyle: 'medium',
    timeStyle: 'medium',
  });
}

/** One sentence of where the list comes from and when, or why there is none. */
export function provenance(catalogue: ModelCatalogue): string {
  if (catalogue.state === 'not_ready')
    return 'The managed Codex runtime is not running, so there is no list. A list of a runtime that has stopped is never shown. Start a managed session and it is listed again.';
  const version = catalogue.runtime_version
    ? ` (version ${catalogue.runtime_version})`
    : '';
  return `Listed once by the managed Codex runtime${version} when it started, in ${catalogue.mode === 'local_tools' ? 'Local Tools' : 'Inference'}. Read from the authority at ${readAt(catalogue.read_at).replace(/\.$/, '')}.`;
}

/** Whether the list is the whole of what the runtime listed. Said only when it is known. */
export function completeness(catalogue: ModelCatalogue): string | null {
  if (catalogue.state !== 'listed') return null;
  if (catalogue.complete === false)
    return 'The runtime’s list continued past what was taken: this list is partial.';
  if (catalogue.complete === null)
    return 'Whether this list is complete is not reported by this authority version.';
  return null;
}

export interface EntryRow {
  id: string;
  title: string;
  provider: string | null;
  isDefault: boolean;
  /** `low · medium · high`, with the runtime's default marked; null when not reported. */
  efforts: string | null;
  replacement: string | null;
}

export function entryRow(entry: CatalogueEntry): EntryRow {
  const efforts =
    entry.efforts === null
      ? null
      : entry.efforts.length === 0
        ? 'none listed'
        : entry.efforts
            .map((e) => (e === entry.default_effort ? `${e} (default)` : e))
            .join(' · ');
  const replacement =
    entry.replaced_by === null
      ? null
      : `The runtime marks it for replacement by ${entry.replaced_by}${entry.retires_at ? `, retiring ${entry.retires_at}` : ''}.`;
  return {
    id: entry.model_id,
    title:
      entry.display_name && entry.display_name !== entry.model_id
        ? `${entry.display_name} · ${entry.model_id}`
        : entry.model_id,
    provider: entry.provider_id,
    isDefault: entry.is_default === true,
    efforts,
    replacement,
  };
}

/** The efforts the managed runtime lists for any model, once each, in the order first seen. */
export function listedEfforts(catalogue: ModelCatalogue | null): string[] {
  if (catalogue === null || catalogue.state !== 'listed') return [];
  const seen: string[] = [];
  for (const entry of catalogue.entries)
    for (const effort of entry.efforts ?? [])
      if (!seen.includes(effort)) seen.push(effort);
  return seen;
}

/** The model ids the managed runtime lists, as suggestions and nothing more. */
export function listedModels(catalogue: ModelCatalogue | null): string[] {
  if (catalogue === null || catalogue.state !== 'listed') return [];
  return catalogue.entries.map((e) => e.model_id);
}

/**
 * Claude Code documents these effort levels (its own `--help`). Aliases of its models are
 * documented but resolved by Claude Code, never here: `opus` is a word it decides the meaning of.
 */
export const CLAUDE_EFFORTS = [
  'low',
  'medium',
  'high',
  'xhigh',
  'max',
] as const;
export const CLAUDE_MODEL_ALIASES = [
  'default',
  'opus',
  'sonnet',
  'haiku',
  'opusplan',
] as const;

export interface ConfiguredRow {
  provider: Provider;
  providerName: string;
  /** `model`, `effortLevel`, `model_reasoning_effort`, … as the file names it. */
  key: string;
  value: string;
  source: string;
  scope: string;
}

/** The model settings the agents' own files hold, as the inventory read them. */
export function configuredRows(
  items: readonly ConfigItem[] | null,
): ConfiguredRow[] {
  if (items === null) return [];
  return (
    items
      // OpenCode's provider entries are options of a provider, not a model setting.
      .filter(
        (i) => i.kind === 'model_preference' && !i.name.startsWith('provider'),
      )
      .map((i) => ({
        provider: i.provider,
        providerName: PROVIDER_LABEL[i.provider],
        key: i.name,
        value:
          i.detail?.kind === 'text' ? i.detail.value.text : 'not shown here',
        source: SOURCE_LABEL[i.source],
        scope: SCOPE_LABEL[i.scope],
      }))
  );
}

export interface SelectedRow {
  profile: string;
  agents: string;
  /** What the profile itself names; null when it names none. */
  model: string | null;
  effort: string | null;
  /** When the profile names none itself: the profile it takes the field from. */
  modelFrom: string | null;
  effortFrom: string | null;
  template: boolean;
}

/**
 * What each profile would pass when a session starts with it (a flag, never a check). A value the
 * profile does not name itself but takes from a profile it extends is said as inherited, by the
 * name of that profile: the value itself is shown in the profile it comes from.
 */
export function selectedRows(profiles: readonly Profile[]): SelectedRow[] {
  const from = (p: Profile, field: 'model' | 'effort'): string | null => {
    const source = p.from[field];
    return p.body[field] === null &&
      source !== undefined &&
      source !== p.body.name
      ? source
      : null;
  };
  return profiles
    .map((p) => ({
      profile: p.body.name,
      agents: p.body.applies_to.map((a) => AGENT_LABEL[a]).join(' · '),
      model: p.body.model,
      effort: p.body.effort,
      modelFrom: from(p, 'model'),
      effortFrom: from(p, 'effort'),
      template: p.template,
    }))
    .filter(
      (row) =>
        row.model !== null ||
        row.effort !== null ||
        row.modelFrom !== null ||
        row.effortFrom !== null,
    );
}

export const SELECTED_NOTE =
  'A profile passes these names to the agent when a session starts. Wyss Console cannot check them: the agent says so in its own session if it does not know one. Nothing changes the agent’s own files.';

export interface ObservedRow {
  id: string;
  label: string;
  line: string;
  rerouted: boolean;
}

/** What a managed runtime reported for each session, in the words of the Sessions view. */
export function observedRows(
  sessions: readonly SessionSummary[] | null,
): ObservedRow[] {
  if (sessions === null) return [];
  return sessions
    .filter((s) => s.binding === 'bound_attached' || s.observed_model !== null)
    .map((s) => ({
      id: s.id,
      label: sessionLabel(s),
      line: observedModelLine(s),
      rerouted: s.observed_model?.source === 'turn_rerouted',
    }));
}

export const NATIVE_NOTE =
  'Agents you start in a terminal do not report the model they use, and this app adds no hook and reads no terminal to find out. What you see for them is what was asked for and what their files say, never what is in use.';
