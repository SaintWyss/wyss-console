import type {
  AgentId,
  Profile,
  Preview,
  StartReply,
} from '../../contracts/native';
import type { TerminalPlan } from '../../contracts/arsenal';
import type { ArsenalStart } from '../../desktop/native-client';
import type { BeforeOutcome, Launch } from '../../desktop/launch-client';

/** What opening a launch needs; the desktop's functions in the window, fakes in tests. */
export interface LaunchDeps {
  getLaunch(id: string): Promise<Launch>;
  runBefore(id: string): Promise<BeforeOutcome[]>;
  profiles(): Promise<Profile[]>;
  /** The Arsenal selection the folder's last terminal of this agent started with. */
  remembered(project: string, agent: AgentId): Promise<string | null>;
  plan(
    selection: string,
    agent: AgentId,
    project: string,
  ): Promise<TerminalPlan>;
  preview(
    agent: AgentId,
    profile: string | null,
    arsenal: ArsenalStart | null,
  ): Promise<Preview>;
  start(
    agent: AgentId,
    project: string,
    profile: string | null,
    preview: Pick<Preview, 'plan_token' | 'provider_version'> | null,
    arsenal: ArsenalStart | null,
  ): Promise<StartReply>;
  bind(
    session: string,
    selection: string,
    configHash: string,
  ): Promise<{ state: 'applied' | 'not_applied'; selection_name: string }>;
  /** A new tab in `cwd` titled `title`, with `input` typed once its shell runs. */
  openTerminal(title: string, cwd: string, input: string): void;
  /** A tab for the agent the desktop started. */
  openNative(reply: StartReply): void;
}

export interface Opened {
  /** Said to the owner after opening: a `before` command that did not succeed, for instance. */
  notes: string[];
}

/** The profile a session launch names, by its name, for its agent. */
export function profileFor(
  profiles: Profile[],
  name: string,
  agent: AgentId,
): Profile | undefined {
  return profiles.find(
    (p) => p.body.name === name && p.body.applies_to.includes(agent),
  );
}

/** Whether the profile a session launch names gives full access. */
export function isFullAccess(launch: Launch, profiles: Profile[]): boolean {
  if (launch.kind !== 'session' || !launch.profile) return false;
  return (
    profileFor(profiles, launch.profile, launch.agent)?.body.permission ===
    'bypass'
  );
}

/**
 * Opens a launch by its id: re-read from its file, then a terminal with its commands typed, or
 * its `before` commands run and its agent started with the profile it names and the Arsenal
 * selection its folder remembers, exactly as a start from the Agents view.
 */
export async function openLaunch(
  id: string,
  deps: LaunchDeps,
): Promise<Opened> {
  const launch = await deps.getLaunch(id);
  if (launch.kind === 'commands') {
    deps.openTerminal(
      launch.name,
      launch.directory,
      launch.commands.map((c) => `${c}\r`).join(''),
    );
    return { notes: [] };
  }
  const { agent, directory } = launch;
  let profileId: string | null = null;
  if (launch.profile) {
    const profile = profileFor(await deps.profiles(), launch.profile, agent);
    if (!profile)
      throw new Error(
        `No profile named “${launch.profile}” for this agent. Create it in Profiles, or change the launch file.`,
      );
    profileId = profile.id;
  }
  // The selection is planned before anything runs, so a plan that fails stops the launch whole.
  const selection = await deps.remembered(directory, agent);
  const plan = selection ? await deps.plan(selection, agent, directory) : null;
  const arsenal: ArsenalStart | null = plan
    ? {
        selection: plan.selection,
        config_hash: plan.config_hash,
        project: directory,
      }
    : null;
  const apply = !!plan?.can_apply;
  const notes = (await deps.runBefore(launch.id))
    .filter((o) => o.result !== 'ok')
    .map(
      (o) =>
        `A command before the start did not succeed (${o.result}); the agent started anyway.`,
    );
  const preview = await deps.preview(agent, profileId, apply ? arsenal : null);
  if (apply && !preview.can_start)
    throw new Error(
      `The Arsenal selection ${plan!.selection_name} cannot be applied with this profile now; start it from Agents to choose.`,
    );
  const reply = await deps.start(
    agent,
    directory,
    profileId,
    preview,
    apply ? arsenal : null,
  );
  deps.openNative(reply);
  if (plan) {
    try {
      const binding = await deps.bind(
        reply.session.id,
        plan.selection,
        plan.config_hash,
      );
      notes.push(
        binding.state === 'applied'
          ? `Started with ${binding.selection_name} applied.`
          : `Started without applying ${binding.selection_name}; the selection is recorded.`,
      );
    } catch {
      notes.push(
        'The terminal started, but Arsenal could not record its selection.',
      );
    }
  }
  return { notes };
}
