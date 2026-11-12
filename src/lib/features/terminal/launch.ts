import type {
  AgentId,
  Profile,
  Preview,
  StartReply,
} from '../../contracts/native';
import type { TerminalPlan } from '../../contracts/arsenal';
import type { ArsenalStart } from '../../desktop/native-client';
import type { BeforeOutcome, Launch } from '../../desktop/launch-client';
import { msg } from '../../i18n/msg';

/** What opening a launch needs; the desktop's functions in the window, fakes in tests. */
export interface LaunchDeps {
  getLaunch(id: string): Promise<Launch>;
  runBefore(id: string): Promise<BeforeOutcome[]>;
  profiles(): Promise<Profile[]>;
  /** The Arsenal selection the folder's last terminal of this agent started with. */
  remembered(project: string, agent: AgentId): Promise<string | null>;
  /** The selection id of the preset with this name for this agent; null when there is none. */
  presetNamed(name: string, agent: AgentId): Promise<string | null>;
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

export interface LaunchNote {
  text: string;
  values?: Record<string, string | number>;
}

/**
 * A launch that cannot start: the window's own sentence and its values, so the view can draw it
 * through t() in the language in force. The message is the English sentence with the values in.
 */
export class LaunchRefusal extends Error {
  constructor(
    readonly template: string,
    readonly values: Record<string, string> = {},
  ) {
    super(
      Object.entries(values).reduce(
        (text, [name, value]) => text.replace(`{${name}}`, value),
        template,
      ),
    );
  }
}

export interface Opened {
  /** Said to the owner after opening: a `before` command that did not succeed, for instance. */
  notes: LaunchNote[];
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

/**
 * The permission a profile starts with once what it extends is merged in, as the host merges it:
 * the nearest profile that sets one decides. A chain that is broken, too deep or a cycle gives
 * `null`, and the host refuses to start it anyway.
 */
export function effectivePermission(
  profiles: Profile[],
  profile: Profile,
): Profile['body']['permission'] {
  const byId = new Map(profiles.map((p) => [p.id, p]));
  const seen = new Set<string>();
  let current: Profile | undefined = profile;
  for (let depth = 0; current && depth < 8; depth++) {
    if (seen.has(current.id)) return null;
    seen.add(current.id);
    if (current.body.permission) return current.body.permission;
    current = current.body.extends ? byId.get(current.body.extends) : undefined;
  }
  return null;
}

/** Whether the profile a session launch names gives full access, itself or by what it extends. */
export function isFullAccess(launch: Launch, profiles: Profile[]): boolean {
  if (launch.kind !== 'session' || !launch.profile) return false;
  const profile = profileFor(profiles, launch.profile, launch.agent);
  return !!profile && effectivePermission(profiles, profile) === 'bypass';
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
      throw new LaunchRefusal(
        msg(
          'No profile named “{name}” for this agent. Create it in Profiles, or change the launch file.',
        ),
        { name: launch.profile },
      );
    profileId = profile.id;
  }
  // The preset the launch names wins over the one its folder remembers. A name with no preset
  // for this agent stops the launch before anything runs, with the reason.
  let selection: string | null;
  if (launch.preset) {
    selection = await deps.presetNamed(launch.preset, agent);
    if (!selection)
      throw new LaunchRefusal(
        msg(
          'No preset named “{name}” for this agent. Create it in Arsenal, or change the launch file.',
        ),
        { name: launch.preset },
      );
  } else {
    selection = await deps.remembered(directory, agent);
  }
  // The selection is planned before anything runs, so a plan that fails stops the launch whole.
  const plan = selection ? await deps.plan(selection, agent, directory) : null;
  const arsenal: ArsenalStart | null = plan
    ? {
        selection: plan.selection,
        config_hash: plan.config_hash,
        project: directory,
      }
    : null;
  const apply = !!plan?.can_apply;
  const notes: LaunchNote[] = (await deps.runBefore(launch.id))
    .filter((o) => o.result !== 'ok')
    .map((o) => ({
      text: msg(
        'A command before the start did not succeed ({result}); the agent started anyway.',
      ),
      values: { result: o.result },
    }));
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
          ? {
              text: msg('Started with {selection} applied.'),
              values: { selection: binding.selection_name },
            }
          : {
              text: msg(
                'Started without applying {selection}; the selection is recorded.',
              ),
              values: { selection: binding.selection_name },
            },
      );
    } catch {
      notes.push({
        text: msg(
          'The terminal started, but Arsenal could not record its selection.',
        ),
      });
    }
  }
  return { notes };
}
