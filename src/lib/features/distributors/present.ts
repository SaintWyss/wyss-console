import type { ConfigItem } from '../../contracts/configuration';
import type { AgentId, Launcher, Profile } from '../../contracts/native';
import { msg, type Phrase } from '../../i18n/msg';
import type { ModelsState } from '../models/controller';
import {
  catalogueFailure,
  completeness,
  configuredRows,
  provenance,
} from '../models/present';
import { AGENT_LABEL } from '../native/present';

/**
 * Who makes each tool the Console can start. Kimi has no tool the Console knows: its card is
 * fixed and nothing is looked for.
 */
export const DISTRIBUTORS = [
  { id: 'anthropic', name: 'Anthropic', agent: 'claude_code' },
  { id: 'openai', name: 'OpenAI', agent: 'codex' },
  { id: 'kimi', name: 'Kimi', agent: null },
  { id: 'opencode', name: 'OpenCode', agent: 'opencode' },
] as const satisfies readonly {
  id: string;
  name: string;
  agent: AgentId | null;
}[];

/** The distributor of an agent, by name; the agent's own name when it has none here. */
export function distributorOf(agent: AgentId): string {
  return (
    DISTRIBUTORS.find((d) => d.agent === agent)?.name ?? AGENT_LABEL[agent]
  );
}

export interface ModelRow {
  key: string;
  /** A model id as the runtime listed it, or a sentence that says where a name comes from. */
  label: string | Phrase;
  /** Listed by Codex and not seen before the last refresh. */
  fresh: boolean;
}

export interface DistributorCard {
  id: (typeof DISTRIBUTORS)[number]['id'];
  name: string;
  status: Phrase;
  /** Only an installed tool is drawn in the ok tone. */
  installed: boolean;
  tool: string;
  models: ModelRow[];
  /** What cannot be known, or why there is no list. */
  notes: Phrase[];
}

const NOT_LISTED: Phrase = {
  text: msg('This tool does not list its models to the Console.'),
};

const EARLIER_READ: Phrase = {
  text: msg('Configured models are from an earlier read.'),
};

/** Installed or not as `native_launcher` answered; unknown when it did not, or did not name it. */
function statusOf(
  launcher: Launcher | null,
  agent: AgentId,
): { status: Phrase; installed: boolean } {
  const entry = launcher?.agents.find((a) => a.agent_id === agent);
  if (!entry) return { status: { text: msg('Unknown') }, installed: false };
  if (!entry.installed)
    return { status: { text: msg('Tool not installed') }, installed: false };
  return {
    status: entry.version
      ? {
          text: msg('Installed · {version}'),
          values: { version: entry.version },
        }
      : { text: msg('Installed · version not reported') },
    installed: true,
  };
}

/**
 * The names an agent would be asked for: the `model` its own files hold and the one each of
 * the owner's profiles passes. Neither is a list of what exists, and the card says so.
 */
function namedModels(
  agent: AgentId,
  items: readonly ConfigItem[] | null,
  profiles: readonly Profile[],
): ModelRow[] {
  const configured = [
    ...new Set(
      configuredRows(items)
        .filter((row) => row.provider === agent && row.key === 'model')
        .flatMap((row) => (row.value === null ? [] : [row.value])),
    ),
  ];
  return [
    ...configured.map((model): ModelRow => ({
      key: `configured:${model}`,
      label: { text: msg('Configured model: {model}'), values: { model } },
      fresh: false,
    })),
    ...profiles
      // A template ships with the Console: it is not the owner's.
      .filter((p) => !p.template && p.body.applies_to.includes(agent))
      .flatMap((p): ModelRow[] =>
        p.body.model === null
          ? []
          : [
              {
                key: `profile:${p.id}`,
                label: {
                  text: msg('Profile {name}: {model}'),
                  values: { name: p.body.name, model: p.body.model },
                },
                fresh: false,
              },
            ],
      ),
  ];
}

/** What the managed Codex runtime listed, or the sentence that says why there is no list. */
function codexModels(
  models: ModelsState,
  fresh: readonly string[],
): { models: ModelRow[]; notes: Phrase[] } {
  const catalogue = models.catalogue;
  if (catalogue === null)
    return {
      models: [],
      notes:
        models.phase === 'failed' ? [catalogueFailure(models.failure)] : [],
    };
  const notes: Phrase[] = [];
  if (catalogue.state === 'not_ready') notes.push(provenance(catalogue));
  if (models.stale) notes.push(catalogueFailure(models.failure));
  const partial = completeness(catalogue);
  if (partial) notes.push(partial);
  return {
    models: catalogue.entries.map((entry) => ({
      key: entry.model_id,
      label: entry.model_id,
      fresh: fresh.includes(entry.model_id),
    })),
    notes,
  };
}

/**
 * The four cards. Every fact is one the desktop answered: `launcher` is null when
 * `native_launcher` did not answer, and then nothing is said to be installed or missing.
 */
export function distributorCards(
  launcher: Launcher | null,
  models: ModelsState,
  items: readonly ConfigItem[] | null,
  profiles: readonly Profile[],
  fresh: readonly string[] = [],
  configStale = false,
): DistributorCard[] {
  return DISTRIBUTORS.map((d): DistributorCard => {
    if (d.agent === null)
      return {
        id: d.id,
        name: d.name,
        status: { text: msg('Not configured') },
        installed: false,
        tool: 'Kimi CLI',
        models: [],
        notes: [],
      };
    const named =
      d.agent === 'codex' ? [] : namedModels(d.agent, items, profiles);
    const listing =
      d.agent === 'codex'
        ? codexModels(models, fresh)
        : {
            models: named,
            // A configured model that is shown and was not read again is said to be older.
            notes:
              configStale && named.some((m) => m.key.startsWith('configured:'))
                ? [NOT_LISTED, EARLIER_READ]
                : [NOT_LISTED],
          };
    return {
      id: d.id,
      name: d.name,
      ...statusOf(launcher, d.agent),
      tool: AGENT_LABEL[d.agent],
      ...listing,
    };
  });
}
