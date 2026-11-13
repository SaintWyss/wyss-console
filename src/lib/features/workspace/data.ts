import type { Item, LibraryItem, Selection } from '../../contracts/arsenal';
import type { Launch } from '../../desktop/launch-client';
import {
  DEFAULT_SPEC,
  type EnvironmentOptions,
  type HomeSpec,
} from '../arsenal/environment';
import { estimate, type Estimate } from './estimate';
import type { Agent } from './plan';

/** What the Workspace editor and the launch menu read to show a launch's estimate. */
export interface EstimateData {
  library: readonly LibraryItem[];
  selections: readonly Selection[];
  options: EnvironmentOptions | null;
  /** Environment by selection id; a preset not listed keeps the owner's own (default). */
  environments: ReadonlyMap<string, HomeSpec>;
}

/** Skills and agents of a provider in the library: what a launch without a preset loads. */
export function libraryItems(
  library: readonly LibraryItem[],
  agent: Agent,
): LibraryItem[] {
  return library.filter(
    (item) =>
      item.provider === agent &&
      (item.kind === 'skill' || item.kind === 'agent'),
  );
}

export function allItemsOf(
  library: readonly LibraryItem[],
  agent: Agent,
): Item[] {
  return libraryItems(library, agent).map((item) => ({
    capability: item.id,
    revision: null,
  }));
}

/** The preset a launch names, for its agent. */
export function selectionOf(
  launch: { agent: Agent; preset: string | null },
  selections: readonly Selection[],
): Selection | null {
  if (!launch.preset) return null;
  return (
    selections.find(
      (s) => s.provider === launch.agent && s.name === launch.preset,
    ) ?? null
  );
}

/** The estimate of what a Claude Code session starts with; `null` for Codex or a command launch. */
export function estimateOf(
  agent: Agent,
  items: readonly Item[],
  spec: HomeSpec | null,
  data: EstimateData,
): Estimate | null {
  if (agent !== 'claude_code' || !data.options) return null;
  const chosen = new Set(items.map((i) => i.capability));
  const of = (kind: string) =>
    libraryItems(data.library, agent)
      .filter((item) => item.kind === kind && chosen.has(item.id))
      .map((item) => ({ name: item.name, description: item.description }));
  return estimate({
    spec: spec ?? DEFAULT_SPEC,
    sizes: data.options.sizes,
    rules: data.options.rules,
    accountSkills: data.options.account_skills,
    plugins: data.options.plugins,
    skills: of('skill'),
    agents: of('agent'),
  });
}

export function estimateLaunch(
  launch: Launch,
  data: EstimateData,
): Estimate | null {
  if (launch.kind !== 'session') return null;
  const selection = selectionOf(launch, data.selections);
  const items = selection
    ? selection.items
    : allItemsOf(data.library, launch.agent);
  const spec = selection ? (data.environments.get(selection.id) ?? null) : null;
  return estimateOf(launch.agent, items, spec, data);
}

/** Reads what an estimate needs. Never throws: what cannot be read leaves no estimate. */
export async function loadEstimateData(client: {
  capabilities: () => Promise<LibraryItem[]>;
  selections: () => Promise<Selection[]>;
  environmentOptions: () => Promise<EnvironmentOptions>;
  environment: (selection: string) => Promise<HomeSpec>;
}): Promise<EstimateData> {
  const [library, selections, options] = await Promise.all([
    client.capabilities().catch(() => []),
    client.selections().catch(() => []),
    client.environmentOptions().catch(() => null),
  ]);
  const environments = new Map<string, HomeSpec>();
  await Promise.all(
    selections
      .filter((s) => s.provider === 'claude_code')
      .map((s) =>
        client.environment(s.id).then(
          (spec) => environments.set(s.id, spec),
          () => undefined,
        ),
      ),
  );
  return { library, selections, options, environments };
}

/** Nothing read yet: no estimate. */
export const NO_DATA: EstimateData = {
  library: [],
  selections: [],
  options: null,
  environments: new Map(),
};
