<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import HandoffPanel from './HandoffPanel.svelte';
  import ArsenalTerminalPanel from '../arsenal/ArsenalTerminalPanel.svelte';
  import DelegationsPanel from './DelegationsPanel.svelte';
  import {
    onOffCounts,
    sessionArsenalLine,
    type ArsenalChoice,
  } from '../arsenal/terminal';
  import { arsenalClient } from '../../desktop/arsenal-client';
  import {
    deleteLaunch,
    LaunchError,
    listLaunches,
    saveLaunch,
    type Launch,
    type LaunchList,
  } from '../../desktop/launch-client';
  import { desktopLaunchDeps } from '../terminal/launch-deps';
  import {
    launchBlock,
    LaunchRefusal,
    openLaunch,
    type SelectionName,
  } from '../terminal/launch';
  import { whenWorkspaceReady } from './bridge';
  import { msg, t, tPhrase } from '../../i18n/index.svelte';
  import type { Binding } from '../../contracts/arsenal';
  import type { AgentId, NativeSession, Preview } from '../../contracts/native';
  import type { ConfigItem } from '../../contracts/configuration';
  import type { NativeController, NativeState } from './controller';
  import {
    AGENT_LABEL,
    STATE_NOTE,
    failureText,
    folderName,
    formatBytes,
    groupByProject,
    isM9Agent,
    modelFacts,
    switchHint,
    profilesFor,
    restorable,
    sharedFolders,
    since,
    stateLabel,
    profileRevisionKey,
  } from './present';

  let {
    native,
    controller,
    visible,
    onconsole,
    onupdate,
    configItems = null,
    onarsenal,
  }: {
    native: NativeState;
    controller: NativeController;
    visible: boolean;
    /** Shows the terminals. */
    onconsole: () => void;
    /** Opens a terminal in `cwd` that types `input`: used to update an agent's program. */
    onupdate?: (title: string, cwd: string, input: string) => void;
    /** What the agents' files hold, once Configuration has been read; else null. */
    configItems?: readonly ConfigItem[] | null;
    /** Opens Arsenal on its Presets tab: the preset card's "Edit in Arsenal". */
    onarsenal?: () => void;
  } = $props();

  let agent = $state<AgentId | null>(null);
  let delegate = $state(false);
  let project = $state<string | null>(null);
  let profile = $state<string | null>(null);
  let preview = $state<Preview | null>(null);
  let previewFor = $state<string | null>(null);
  let previewPendingFor = $state<string | null>(null);
  let previewFailedFor = $state<string | null>(null);
  let previewRetry = $state(0);
  let now = $state(Date.now());
  let arsenalChoice = $state<ArsenalChoice | null>(null);
  let arsenalNote = $state<string | null>(null);
  // The preset the start uses: '' is none. The card's select and the Skills panel share it.
  let presetId = $state('');
  // Skills stay folded; they open by themselves only when the choice blocks Start.
  let skillsOpen = $state(false);
  let evidenceOpen = $state(false);
  const skillsSummary = $derived(
    arsenalChoice === null
      ? t('None — all installed skills')
      : arsenalChoice.pending
        ? t('Checking…')
        : arsenalChoice.apply
          ? arsenalChoice.plan.selection_name
          : t('{name} · not applied', {
              name: arsenalChoice.plan.selection_name,
            }),
  );
  $effect(() => {
    if (arsenalChoice && !arsenalChoice.pending && !arsenalChoice.ready)
      skillsOpen = true;
  });
  const updateCommand = (id: AgentId) =>
    id === 'claude_code' ? 'claude update' : 'codex update';
  function updateAgent(id: AgentId) {
    const cwd = projects.find((p) => p.home)?.path ?? project ?? '~';
    onupdate?.(
      t('Update {agent}', { agent: AGENT_LABEL[id] }),
      cwd,
      updateCommand(id),
    );
  }
  // Bumped when the Arsenal plan must be computed again: the view is shown, or a start was refused.
  let arsenalRefresh = $state(0);
  $effect(() => {
    if (visible) arsenalRefresh = untrack(() => arsenalRefresh) + 1;
  });
  $effect(() => {
    if (native.failure?.code.startsWith('arsenal_'))
      arsenalRefresh = untrack(() => arsenalRefresh) + 1;
  });
  let bindings = $state<Binding[]>([]);
  async function readBindings() {
    try {
      bindings = await arsenalClient.bindings();
    } catch {
      // The library may be unavailable; sessions are still listed without their Arsenal line.
      bindings = [];
    }
  }
  let handingOff = $state<string | null>(null);
  const handoffSession = $derived(
    native.sessions.find((s) => s.id === handingOff) ?? null,
  );

  const launcher = $derived(native.launcher);
  const entries = $derived(launcher?.agents ?? []);
  const chosen = $derived(entries.find((e) => e.agent_id === agent) ?? null);
  const profiles = $derived(agent ? profilesFor(native.profiles, agent) : []);
  const previewContext = $derived(
    agent && profile
      ? JSON.stringify([
          agent,
          profile,
          profileRevisionKey(native.profiles, profile),
        ])
      : null,
  );
  const previewCurrent = $derived(
    !profile ||
      (!!preview &&
        previewFor === previewContext &&
        previewPendingFor !== previewContext),
  );
  const projects = $derived(launcher?.projects ?? []);
  const groups = $derived(groupByProject(native.sessions));
  const crowded = $derived(sharedFolders(native.sessions));
  const toRestore = $derived(restorable(native.sessions));
  const known = $derived(projects.find((p) => p.path === project) ?? null);
  const canStart = $derived(
    !!agent &&
      !!project &&
      !!chosen?.installed &&
      !native.starting &&
      !(native.m9Blocked && isM9Agent(agent)) &&
      (profile === null || (previewCurrent && preview?.can_start === true)) &&
      (arsenalChoice === null ||
        (!arsenalChoice.pending && arsenalChoice.ready)),
  );

  // The first time the launcher is known: an installed agent and the home folder.
  $effect(() => {
    if (!launcher) return;
    if (agent === null || !entries.some((e) => e.agent_id === agent))
      agent = entries.find((e) => e.installed)?.agent_id ?? null;
    if (project === null || !projects.some((p) => p.path === project))
      project = projects.find((p) => p.home)?.path ?? projects[0]?.path ?? null;
  });
  // A project remembers the agent and profile it was last used with.
  let lastProject: string | null = null;
  $effect(() => {
    if (!known || known.path === lastProject) return;
    lastProject = known.path;
    if (
      known.default_agent &&
      entries.some((e) => e.agent_id === known.default_agent && e.installed)
    )
      agent = known.default_agent;
    profile = known.default_profile;
  });
  // A profile written for another agent does not follow a change of agent.
  $effect(() => {
    if (profile && !profiles.some((p) => p.id === profile)) profile = null;
  });
  // What the profile would do for this agent, read from the desktop before starting.
  let previewSeq = 0;
  let lastPreviewRequest: string | null = null;
  let lastPreviewRetry = -1;
  $effect(() => {
    const a = agent;
    const p = profile;
    const key = previewContext;
    const retry = previewRetry;
    if (!a || !p || !key) {
      if (lastPreviewRequest !== null) {
        previewSeq++;
        lastPreviewRequest = null;
        lastPreviewRetry = -1;
        preview = null;
        previewFor = null;
        previewPendingFor = null;
        previewFailedFor = null;
      }
      return;
    }
    if (key === lastPreviewRequest && retry === lastPreviewRetry) return;
    lastPreviewRequest = key;
    lastPreviewRetry = retry;
    const seq = ++previewSeq;
    preview = null;
    previewFor = null;
    previewPendingFor = key;
    previewFailedFor = null;
    void controller.preview(a, p).then(
      (next) => {
        if (seq === previewSeq) {
          preview = next;
          previewFor = key;
          previewPendingFor = null;
        }
      },
      () => {
        if (seq === previewSeq) {
          previewPendingFor = null;
          previewFailedFor = key;
        }
      },
    );
  });
  function retryPreview() {
    if (previewFailedFor === previewContext) previewRetry++;
  }
  $effect(() => {
    if (!visible) return;
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 5000);
    return () => clearInterval(timer);
  });
  onMount(() => {
    controller.start0();
    void readBindings();
  });

  async function chooseFolder() {
    const path = await controller.choose();
    if (path) {
      lastProject = path;
      project = path;
      profile = null;
    }
  }
  async function launch() {
    if (!canStart || !agent || !project) return;
    controller.setDelegation(
      delegate && (agent === 'claude_code' || agent === 'codex'),
    );
    const choice = arsenalChoice;
    if (choice && !choice.pending) {
      // Applied: the preview that binds the exclusion. Recorded only: the profile's own preview.
      const reply = await controller.startWithArsenal(
        agent,
        project,
        profile,
        choice.apply ? choice.preview : preview,
        choice.apply ? choice.start : null,
      );
      if (!reply) return;
      try {
        const binding = await arsenalClient.bindTerminal(
          reply.session.id,
          choice.start.selection,
          choice.start.config_hash,
        );
        arsenalNote =
          binding.state === 'applied'
            ? t('Started with {name} applied.', {
                name: binding.selection_name,
              })
            : t('Started without applying {name}; the selection is recorded.', {
                name: binding.selection_name,
              });
      } catch {
        arsenalNote = t(
          'The terminal started, but Arsenal could not record its selection.',
        );
      }
      void readBindings();
      onconsole();
      return;
    }
    if (await controller.startAgent(agent, project, profile, null, preview))
      onconsole();
  }
  function open(session: NativeSession) {
    if (controller.showTerminal(session.id)) onconsole();
  }
  function ended(s: NativeSession): string {
    if (!s.ended_at) return '';
    const time = new Date(s.ended_at * 1000).toLocaleTimeString(undefined, {
      timeStyle: 'short',
    });
    return ` · ${t('ended {time}', { time })}`;
  }
  const profileName = (id: string) =>
    native.profiles.find((p) => p.id === id)?.body.name ?? t('profile');

  // Save as launch… (arsenal-UI-SPEC, phase D): the choices on this card, written as a session
  // launch that Launch ▾ lists. The desktop checks the profile and the preset again.
  let launchForm = $state<{
    name: string;
    busy: boolean;
    exists: boolean;
    failure: string | null;
  } | null>(null);
  let launchSaved = $state<string | null>(null);
  // The preset is the Arsenal choice when there is one; a choice still being computed saves nothing.
  const launchPreset = $derived(
    arsenalChoice === null || arsenalChoice.pending
      ? null
      : arsenalChoice.plan.selection_name,
  );
  const launchProfile = $derived(
    profile
      ? (native.profiles.find((p) => p.id === profile)?.body.name ?? null)
      : null,
  );
  function openLaunchForm() {
    if (!launchAgent || !project) return;
    launchSaved = null;
    launchForm = {
      name: `${AGENT_LABEL[launchAgent]} · ${folderName(project)}`,
      busy: false,
      exists: false,
      failure: null,
    };
  }
  // A launch names a Claude Code or a Codex session: the agents Arsenal applies presets to.
  const launchAgent = $derived(
    agent === 'claude_code' || agent === 'codex' ? agent : null,
  );
  async function saveAsLaunch(replace: boolean) {
    const form = launchForm;
    const chosen = launchAgent;
    if (!form || !chosen || !project || form.busy || !form.name.trim()) return;
    form.busy = true;
    form.exists = false;
    form.failure = null;
    try {
      const saved = await saveLaunch({
        name: form.name.trim(),
        directory: project,
        agent: chosen,
        profile: launchProfile,
        preset: launchPreset,
        replace,
      });
      launchForm = null;
      launchSaved = saved.name;
      void readSaved();
    } catch (error) {
      if (error instanceof LaunchError && error.code === 'exists')
        form.exists = true;
      else
        form.failure =
          error instanceof LaunchError && error.reasons[0]
            ? error.reasons[0]
            : t('The launch could not be saved.');
    } finally {
      form.busy = false;
    }
  }

  // Saved launches (arsenal-UI-SPEC, phase D): the owner's launch files, and the Arsenal selections
  // they name. Read while this view is shown, and again after a launch is saved or deleted.
  type Shown = {
    text: string;
    translatable: boolean;
    values?: Record<string, string>;
  };
  type RowShown = Shown & { id: string };
  let savedLaunches = $state<LaunchList | null>(null);
  let savedSelections = $state<SelectionName[]>([]);
  let savedFailure = $state<Shown | null>(null);
  let startingLaunch = $state<string | null>(null);
  let startFailure = $state<RowShown | null>(null);
  let confirmingDelete = $state<string | null>(null);
  let deletingLaunch = $state<string | null>(null);
  let deleteFailure = $state<RowShown | null>(null);
  const savedRows = $derived(savedLaunches?.launches ?? []);
  // The presets the card offers: the saved Arsenal selections of this agent's provider.
  const presetsOfAgent = $derived(
    launchAgent
      ? savedSelections.filter((s) => s.provider === launchAgent)
      : [],
  );
  /** What the desktop answered, as a sentence the view draws: its reason, or the window's own. */
  function explain(error: unknown, fallback: string): Shown {
    if (error instanceof LaunchRefusal)
      return { text: error.template, translatable: true, values: error.values };
    if (error instanceof LaunchError && error.reasons[0])
      return { text: error.reasons[0], translatable: false };
    if (
      error instanceof Error &&
      !(error instanceof LaunchError) &&
      error.message
    )
      return { text: error.message, translatable: false };
    return { text: fallback, translatable: true };
  }
  const shownText = (shown: Shown) =>
    shown.translatable ? t(shown.text, shown.values) : shown.text;
  async function readSaved() {
    try {
      savedLaunches = await listLaunches();
      savedSelections = await arsenalClient.selections();
      savedFailure = null;
    } catch (error) {
      savedFailure = explain(error, msg('The saved launches did not load.'));
    }
  }
  $effect(() => {
    if (visible) untrack(() => void readSaved());
  });
  /** Why a saved launch cannot start now, from the profiles and selections this window holds. */
  const blockOf = (launch: Launch) =>
    launchBlock(launch, native.profiles, savedSelections);
  function summaryOf(launch: Launch): string {
    if (launch.kind === 'commands')
      return t('Terminal commands · {folder}', {
        folder: folderName(launch.directory),
      });
    return [
      AGENT_LABEL[launch.agent],
      folderName(launch.directory),
      launch.profile ?? t('No profile — as configured'),
      launch.preset
        ? t('Preset {name}', { name: launch.preset })
        : t('None — all installed skills'),
    ].join(' · ');
  }
  async function startSaved(launch: Launch) {
    if (startingLaunch) return;
    startingLaunch = launch.id;
    startFailure = null;
    try {
      // The same start as Launch ▾: the tab is made once the Console is mounted, then the Console shows.
      const opened = await openLaunch(
        launch.id,
        desktopLaunchDeps({
          openTerminal: (title, cwd, input) => {
            whenWorkspaceReady((m) => {
              m.launch(title, cwd, input);
            });
          },
          openNative: (reply) => {
            whenWorkspaceReady((m) => {
              const tab = m.tabOf(reply.session.id);
              if (tab) m.activate(tab);
              else m.createNative(reply);
            });
          },
        }),
      );
      arsenalNote =
        opened.notes.map((note) => t(note.text, note.values)).join(' ') || null;
      onconsole();
    } catch (error) {
      startFailure = {
        id: launch.id,
        ...explain(error, msg('The launch could not be opened.')),
      };
    } finally {
      startingLaunch = null;
    }
  }
  async function removeLaunch(launch: Launch) {
    if (deletingLaunch) return;
    deletingLaunch = launch.id;
    deleteFailure = null;
    try {
      await deleteLaunch(launch.id);
      confirmingDelete = null;
      await readSaved();
    } catch (error) {
      deleteFailure = {
        id: launch.id,
        ...explain(error, msg('The launch could not be deleted.')),
      };
    } finally {
      deletingLaunch = null;
    }
  }
  /** The confirmation's Cancel takes the focus when it appears, as the contract asks. */
  function focusOnMount(node: HTMLElement) {
    node.focus();
  }
</script>

<section
  class="native-feature"
  aria-label={t('Agents started from the Console')}
  hidden={!visible}
  inert={!visible}
>
  {#if chosen?.agent_id === 'hermes'}
    <p class="sessions-status failed" role="note">
      {t(
        'Hermes Native is a full-account provider session. Its approval prompts and hooks are not a Wyss security boundary. Hermes planner admission is separately blocked pending tested OS isolation. The installed version is not probed because Hermes version output performs an update check.',
      )}
    </p>
  {/if}

  {#if native.failure}
    <p class="sessions-status failed" role="alert">
      {t(failureText(native.failure.code))}
      {#each native.failure.reasons as reason (reason)}
        <br />{t(reason)}
      {/each}
    </p>
  {/if}

  {#if native.phase === 'loading' || native.phase === 'idle'}
    <div class="sessions-empty" role="status" aria-busy="true">
      <p class="empty-title">{t('Looking for your agents…')}</p>
    </div>
  {:else if native.phase === 'failed' || !launcher}
    <div class="sessions-empty" role="alert">
      <p class="empty-title">{t('Agents cannot be started here')}</p>
      <p>{t('The desktop did not answer. Nothing was started.')}</p>
      <button
        type="button"
        class="action"
        onclick={() => controller.checkLauncher()}>{t('Try again')}</button
      >
    </div>
  {:else}
    <div class="native-main">
      {#if native.m9Uncertain}
        <p class="sessions-status" role="status">
          {t(
            'OpenCode and Hermes wait until the Console confirms its private M9 session store again.',
          )}
          <button
            type="button"
            class="action"
            disabled={native.checkingLauncher}
            onclick={() => controller.checkLauncher()}
            >{t('Check again')}</button
          >
        </p>
      {:else if launcher.m9_native_status === 'storage_unavailable'}
        <p class="sessions-status failed" role="alert">
          {t(
            'OpenCode and Hermes are blocked: the Console could not confirm its private M9 session store, because a read or a write failed. Claude Code and Codex stay available where the Console allows them.',
          )}
        </p>
      {:else if launcher.m9_native_status === 'legacy_host'}
        <p class="sessions-status" role="note">
          {t(
            'This terminal host predates M9. OpenCode and Hermes Native require a matching M9 host; existing Native sessions remain available.',
          )}
        </p>
      {/if}
      {#if handoffSession}
        {#key handoffSession.id}
          <HandoffPanel
            session={handoffSession}
            {native}
            {controller}
            onclose={() => (handingOff = null)}
            onstarted={() => {
              handingOff = null;
              onconsole();
            }}
          />
        {/key}
      {/if}
      <section
        class="native-card native-launch"
        aria-label={t('Start an agent')}
      >
        <h3 class="visually-hidden">{t('Start an agent')}</h3>

        <div class="launch-block">
          <h4 id="step-agent">{t('Agent')}</h4>
          <div
            class="native-agents"
            role="radiogroup"
            aria-labelledby="step-agent"
          >
            {#each entries as entry (entry.agent_id)}
              <label
                class="native-agent"
                class:selected={agent === entry.agent_id}
                class:missing={!entry.installed}
              >
                <input
                  type="radio"
                  name="agent"
                  value={entry.agent_id}
                  disabled={!entry.installed}
                  checked={agent === entry.agent_id}
                  onchange={() => (agent = entry.agent_id)}
                />
                <span class="event-title">{entry.name}</span>
                <span class="event-meta">
                  {#if entry.installed}{entry.version ??
                      t('installed')}{:else}{t('Not installed')}{/if}
                </span>
              </label>
            {/each}
          </div>
        </div>

        <div class="launch-grid">
          <div class="launch-block">
            <h4 id="step-folder">{t('Folder')}</h4>
            <div class="native-field">
              <label class="visually-hidden" for="native-project"
                >{t('Project folder')}</label
              >
              <div class="native-row">
                <select id="native-project" bind:value={project}>
                  {#each projects as p (p.path)}
                    <option value={p.path}
                      >{p.home
                        ? t('Home · {path}', { path: p.path })
                        : p.path}</option
                    >
                  {/each}
                </select>
                <button type="button" class="action" onclick={chooseFolder}
                  >{t('Choose…')}</button
                >
              </div>
            </div>
          </div>

          <div class="launch-block">
            <h4 id="step-permissions">{t('Permissions')}</h4>
            {#if profile && !previewCurrent}
              <p class="native-warning" role="status">
                {#if previewFailedFor === previewContext}
                  {t(
                    'Could not verify this profile preview. Start stays disabled until a fresh preview succeeds.',
                  )}
                {:else if previewPendingFor === previewContext}
                  {t('Refreshing the profile preview…')}
                {:else}
                  {t(
                    'The selected profile changed. Checking a fresh preview before Start.',
                  )}
                {/if}
              </p>
              {#if previewFailedFor === previewContext}
                <button
                  type="button"
                  class="action"
                  disabled={native.starting}
                  onclick={retryPreview}>{t('Retry preview')}</button
                >
              {/if}
            {/if}
            <div class="native-field">
              <label class="visually-hidden" for="native-profile"
                >{t('Profile')}</label
              >
              <div class="native-row">
                <select id="native-profile" bind:value={profile}>
                  <option value={null}>{t('No profile — as configured')}</option
                  >
                  {#each profiles as p (p.id)}
                    <option value={p.id}
                      >{p.body.name}{p.template
                        ? ` ${t('(template)')}`
                        : ''}</option
                    >
                  {/each}
                </select>
                {#if project && known && (known.default_profile ?? null) !== profile}
                  <button
                    type="button"
                    class="action"
                    onclick={() =>
                      project && controller.setProjectDefault(project, profile)}
                    >{t('Use for this project')}</button
                  >
                {/if}
              </div>
              {#if preview && previewFor === previewContext}
                <ul
                  class="native-preview"
                  aria-label={t('What the profile does')}
                >
                  {#each preview.applied as line (line)}<li>{line}</li>{/each}
                  {#each preview.with_cost as line (line)}<li class="cost">
                      {line}
                    </li>{/each}
                  {#each preview.not_applicable as line (line)}<li
                      class="refused"
                    >
                      {line}
                    </li>{/each}
                  {#each preview.warnings as line (line)}<li class="evidence">
                      {line}
                    </li>{/each}
                </ul>
                {#if !preview.can_start}
                  <p class="native-warning">
                    {t(
                      'This profile cannot be applied to {agent}, so it will not start.',
                      { agent: agent ? AGENT_LABEL[agent] : t('this agent') },
                    )}
                  </p>
                {/if}
              {/if}
            </div>
          </div>

          {#if agent === 'claude_code' || agent === 'codex'}
            <div class="launch-block">
              <h4 id="step-delegation">{t('Delegation')}</h4>
              <label class="native-check">
                <input type="checkbox" bind:checked={delegate} />
                {t(
                  'Let it hand tasks to Claude Code and Codex through Wyss (2 agents at a time, 20 minutes each, writes only in the workspace).',
                )}
              </label>
            </div>
          {/if}
        </div>

        {#if launchAgent}
          <div class="launch-block launch-preset">
            <h4 id="step-preset">{t('Preset')}</h4>
            <div class="native-row">
              <label class="visually-hidden" for="native-preset"
                >{t('Preset')}</label
              >
              <select id="native-preset" bind:value={presetId}>
                <option value="">{t('None — all installed skills')}</option>
                {#each presetsOfAgent as preset (preset.id)}
                  <option value={preset.id}>{preset.name}</option>
                {/each}
              </select>
              {#if onarsenal}
                <button type="button" class="action" onclick={onarsenal}
                  >{t('Edit in Arsenal')}</button
                >
              {/if}
            </div>
            {#if presetsOfAgent.length === 0}
              <p class="event-meta">
                {t('No presets for {agent}.', {
                  agent: AGENT_LABEL[launchAgent],
                })}
              </p>
            {:else if arsenalChoice?.pending === true}
              <p class="event-meta" aria-live="polite">{t('Checking…')}</p>
            {:else if arsenalChoice && !arsenalChoice.pending && presetId}
              <p class="event-meta" aria-live="polite">
                {t('{on} on · {off} off', onOffCounts(arsenalChoice.plan))}
              </p>
            {/if}
          </div>
        {/if}
        <details class="launch-skills" bind:open={skillsOpen}>
          <summary>
            <h4 id="step-skills">{t('Skills')}</h4>
            <span class="launch-skills-state">{skillsSummary}</span>
            <span class="launch-change"
              >{skillsOpen ? t('Close') : t('Change')}
              <svg viewBox="0 0 24 24" aria-hidden="true"
                ><path d="M6 9l6 6 6-6" /></svg
              ></span
            >
          </summary>
          <ArsenalTerminalPanel
            {agent}
            {project}
            {profile}
            profileKey={previewContext}
            refreshKey={arsenalRefresh}
            previewer={(start) =>
              agent
                ? controller.preview(agent, profile, start)
                : Promise.reject(new Error('agent'))}
            bind:selected={presetId}
            choices={false}
            onchange={(choice) => (arsenalChoice = choice)}
          />
        </details>
        {#if arsenalNote}
          <p class="event-meta" role="status">{arsenalNote}</p>
        {/if}

        {#if evidenceOpen && chosen?.installed}
          <div class="native-capability-details">
            {#if chosen.profile_capability.status === 'syntax_verified'}
              <p>
                {t('Checked {date}.', {
                  date: chosen.profile_capability.checked_on ?? '',
                })}
              </p>
              <ul>
                {#each chosen.profile_capability.operations as operation (operation)}
                  <li>{operation}</li>
                {/each}
              </ul>
              <p>
                {t(
                  'Interface: {interface}. Format: {format}. Scope: {scope}.',
                  {
                    interface: chosen.profile_capability.interface ?? '',
                    format: chosen.profile_capability.format ?? '',
                    scope: chosen.profile_capability.scope ?? '',
                  },
                )}
              </p>
              <p>
                {t('Reload: {reload}.', {
                  reload: chosen.profile_capability.reload_semantics ?? '',
                })}
              </p>
              <p>
                {t('Source: {source}.', {
                  source: chosen.profile_capability.source ?? '',
                })}
              </p>
              <p>
                {t('Limit: {limit}.', {
                  limit: chosen.profile_capability.limitation ?? '',
                })}
              </p>
            {:else}
              <p>{chosen.profile_capability.limitation}</p>
            {/if}
          </div>
        {/if}

        <div class="launch-footer">
          <div class="launch-footer-tools">
            {#if chosen?.installed}
              {#if chosen.agent_id === 'claude_code' || chosen.agent_id === 'codex'}
                <button
                  type="button"
                  class="action"
                  title={t('Runs “{command}” in a new terminal', {
                    command: updateCommand(chosen.agent_id),
                  })}
                  onclick={() => updateAgent(chosen.agent_id)}
                  >{t('Update {agent}', {
                    agent: AGENT_LABEL[chosen.agent_id],
                  })}</button
                >
              {/if}
              <button
                type="button"
                class="action"
                aria-expanded={evidenceOpen}
                onclick={() => (evidenceOpen = !evidenceOpen)}
                >{t('Capability evidence and limits')}</button
              >
            {/if}
          </div>
          <button
            type="button"
            class="action"
            disabled={!launchAgent ||
              !project ||
              native.starting ||
              !!launchForm}
            onclick={openLaunchForm}>{t('Save as launch…')}</button
          >
          <button
            type="button"
            class="action primary"
            disabled={!canStart}
            title={launcher.shell?.through_shell
              ? t(
                  'Starts through your shell ({family}): same environment as typing the command yourself.',
                  { family: String(launcher.shell.family) },
                )
              : t('Starts directly with the Console’s own environment.')}
            onclick={launch}
          >
            {native.starting
              ? t('Starting…')
              : t('Start {agent}', {
                  agent: agent ? AGENT_LABEL[agent] : t('agent'),
                })}
          </button>
        </div>
        {#if launchForm && launchAgent && project}
          <div
            class="launch-save"
            role="group"
            aria-label={t('Save as launch')}
          >
            <label class="launch-save-name" for="launch-save-name"
              >{t('Launch name')}</label
            >
            <input
              id="launch-save-name"
              type="text"
              maxlength="60"
              bind:value={launchForm.name}
              disabled={launchForm.busy}
            />
            <p class="event-meta">
              {AGENT_LABEL[launchAgent]} · {folderName(project)} · {launchProfile ??
                t('No profile — as configured')} · {launchPreset ??
                t('None — all installed skills')}
            </p>
            {#if launchForm.exists}
              <p class="native-warning" role="alert">
                {t('A launch with that name exists.')}
              </p>
            {/if}
            {#if launchForm.failure}
              <p class="native-warning" role="alert">{launchForm.failure}</p>
            {/if}
            <div class="launch-footer">
              <button
                type="button"
                class="action"
                onclick={() => (launchForm = null)}>{t('Cancel')}</button
              >
              <button
                type="button"
                class="action primary"
                disabled={!launchForm.name.trim() ||
                  launchForm.busy ||
                  arsenalChoice?.pending === true}
                onclick={() => saveAsLaunch(launchForm?.exists === true)}
                >{launchForm.exists
                  ? t('Replace')
                  : launchForm.busy
                    ? t('Saving…')
                    : t('Save launch')}</button
              >
            </div>
          </div>
        {/if}
        {#if launchSaved}
          <p class="event-meta" role="status">
            {t('Launch saved as “{name}”. Find it in Launch ▾.', {
              name: launchSaved,
            })}
          </p>
        {/if}
        <div class="launch-block launch-saved">
          <h4 id="step-saved">{t('Saved launches')}</h4>
          {#if savedFailure}
            <p class="native-warning" role="alert">{shownText(savedFailure)}</p>
          {:else if savedLaunches === null}
            <p class="event-meta" role="status">{t('Reading launches…')}</p>
          {:else if savedRows.length === 0}
            <p class="event-meta">{t('No saved launches yet.')}</p>
          {:else}
            <ul class="events-list" aria-labelledby="step-saved">
              {#each savedRows as launch (launch.id)}
                {@const block = blockOf(launch)}
                <li class="native-row">
                  <span class="event-title">{launch.name}</span>
                  <span class="event-meta native-facts"
                    >{summaryOf(launch)}</span
                  >
                  {#if block}
                    <span class="native-warning" role="note"
                      >{t(block.template, block.values)}</span
                    >
                  {/if}
                  {#if startFailure?.id === launch.id}
                    <span class="native-warning" role="alert"
                      >{shownText(startFailure)}</span
                    >
                  {/if}
                  {#if deleteFailure?.id === launch.id}
                    <span class="native-warning" role="alert"
                      >{shownText(deleteFailure)}</span
                    >
                  {/if}
                  <span class="native-actions">
                    {#if confirmingDelete === launch.id}
                      <span class="event-meta"
                        >{t('Delete {name}?', { name: launch.name })}</span
                      >
                      <button
                        type="button"
                        class="action danger"
                        disabled={deletingLaunch !== null}
                        onclick={() => removeLaunch(launch)}
                        >{deletingLaunch === launch.id
                          ? t('Deleting…')
                          : t('Delete')}</button
                      >
                      <button
                        type="button"
                        class="action"
                        use:focusOnMount
                        onclick={() => (confirmingDelete = null)}
                        >{t('Cancel')}</button
                      >
                    {:else}
                      <button
                        type="button"
                        class="action"
                        disabled={startingLaunch !== null ||
                          native.starting ||
                          !!block}
                        onclick={() => startSaved(launch)}
                        >{startingLaunch === launch.id
                          ? t('Starting…')
                          : t('Start')}</button
                      >
                      <button
                        type="button"
                        class="action"
                        disabled={deletingLaunch !== null}
                        onclick={() => (confirmingDelete = launch.id)}
                        >{t('Delete')}</button
                      >
                    {/if}
                  </span>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      </section>

      <div class="native-side">
        <section class="native-sessions" aria-label={t('Native sessions')}>
          <h3 class="native-heading">
            {t('Sessions')}
            <span
              class="pill compact"
              data-tone="idle"
              title={t(
                "Native sessions: the provider's own tools, configuration and privileges stay active. The Console does not mediate, contain or approve their effects.",
              )}
              ><span class="dot" aria-hidden="true"></span>{t(
                'Native plane',
              )}</span
            >
            {#if toRestore.length > 0}
              <button
                type="button"
                class="action"
                disabled={native.starting}
                onclick={async () => {
                  if ((await controller.restoreLost()) > 0) onconsole();
                }}
                >{toRestore.length === 1
                  ? t('Start the lost session again')
                  : t('Start {count} lost sessions again', {
                      count: toRestore.length,
                    })}</button
              >
            {/if}
            {#if native.sessions.some((s) => s.state !== 'running' && s.state !== 'requested')}
              <button
                type="button"
                class="action"
                onclick={() => controller.forgetFinished()}
                >{t('Remove finished')}</button
              >
            {/if}
          </h3>
          {#if native.sessions.length === 0}
            <div class="sessions-empty">
              <p class="empty-title">{t('No agent sessions yet')}</p>
              <p>
                {t(
                  'Start one above. It opens in the Console as a terminal tab.',
                )}
              </p>
            </div>
          {:else}
            {#each groups as group (group.project)}
              <div class="native-group">
                <p class="event-meta">
                  <span class="mono">{group.project}</span>
                  {#if crowded.has(group.project)}
                    <span class="native-warning">
                      · {t(
                        'Two sessions run here and can write the same files. Nothing coordinates them.',
                      )}
                    </span>
                  {/if}
                </p>
                <ul
                  class="events-list"
                  aria-label={t('Sessions in {folder}', {
                    folder: folderName(group.project),
                  })}
                >
                  {#each group.sessions as session (session.id)}
                    {@const label = stateLabel(session)}
                    <li class="native-row" data-state={session.state}>
                      <span class="event-title">
                        {session.agent_name}
                        {#if session.version}<span class="event-meta"
                            >{session.version}</span
                          >{/if}
                      </span>
                      <span
                        class="pill compact"
                        data-tone={label.tone}
                        title={t(STATE_NOTE[session.state])}
                      >
                        <span class="dot" aria-hidden="true"></span>{tPhrase(
                          label.label,
                        )}
                      </span>
                      <span class="event-meta native-facts">
                        {#if session.profile}{t('Profile {name} (v{version})', {
                            name: profileName(session.profile.id),
                            version: session.profile.version,
                          })} ·
                        {:else}{t('No profile')} ·
                        {/if}
                        {#if session.state === 'running' || session.state === 'requested'}
                          {t('up {time}', {
                            time: since(session.started_at, now),
                          })}
                        {:else}
                          {t('started {date}', {
                            date: new Date(
                              session.started_at * 1000,
                            ).toLocaleString(undefined, {
                              dateStyle: 'short',
                              timeStyle: 'short',
                            }),
                          })}{ended(session)}
                        {/if}
                        {#if !session.through_shell}
                          · {t('started directly')}{/if}
                        {#if session.restarts > 0}
                          · {t('restart {count}', {
                            count: session.restarts,
                          })}{/if}
                        {#if sessionArsenalLine(session, bindings)}
                          · {sessionArsenalLine(session, bindings)}{/if}
                        {#if session.state === 'running' && session.pid}
                          · {t('process {pid}', { pid: session.pid })}{/if}
                        {#if session.usage}
                          · {session.usage.cpu_percent === null
                            ? '…'
                            : `${session.usage.cpu_percent.toFixed(0)}%`} CPU · {formatBytes(
                            session.usage.rss_bytes,
                          )}{session.usage.processes > 1
                            ? ` · ${t('{count} processes', { count: session.usage.processes })}`
                            : ''}
                        {/if}
                      </span>
                      {#if session.state === 'running' || session.state === 'requested'}
                        {@const facts = modelFacts(
                          session,
                          native.profiles,
                          configItems,
                        )}
                        <span
                          class="event-meta native-facts"
                          data-testid="models"
                        >
                          {t(
                            'Model: asked for {requested} · set in its files {configured} · in use: {inUse} · Effort: asked for {effort} · set in its files {configuredEffort}',
                            {
                              requested: t(facts.requested),
                              configured: t(facts.configured),
                              inUse: t(facts.inUse),
                              effort: t(facts.requestedEffort),
                              configuredEffort: t(facts.configuredEffort),
                            },
                          )}
                        </span>
                        <span
                          class="event-meta native-note"
                          data-testid="switch"
                        >
                          {t(switchHint(session.agent_id))}
                        </span>
                      {/if}
                      {#if session.state === 'lost'}
                        <span class="event-meta native-note"
                          >{t(STATE_NOTE.lost)}</span
                        >
                      {/if}
                      <span class="native-actions">
                        {#if session.state === 'running' || session.state === 'requested'}
                          <button
                            type="button"
                            class="action"
                            disabled={!controller.hasTab(session.id)}
                            onclick={() => open(session)}>{t('Open')}</button
                          >
                          <button
                            type="button"
                            class="action"
                            onclick={() => controller.stop(session.id)}
                            >{t('Stop')}</button
                          >
                          <button
                            type="button"
                            class="action"
                            disabled={native.starting ||
                              (native.m9Blocked && isM9Agent(session.agent_id))}
                            onclick={() => controller.restart(session.id)}
                            >{t('Restart')}</button
                          >
                        {:else}
                          {#if controller.hasTab(session.id)}
                            <button
                              type="button"
                              class="action"
                              onclick={() => open(session)}
                              >{t('Show terminal')}</button
                            >
                          {/if}
                          <button
                            type="button"
                            class="action"
                            disabled={native.starting ||
                              (native.m9Blocked && isM9Agent(session.agent_id))}
                            onclick={() => controller.restart(session.id)}
                            >{t('Start again')}</button
                          >
                          <button
                            type="button"
                            class="action"
                            onclick={() => controller.forget(session.id)}
                            >{t('Remove')}</button
                          >
                        {/if}
                        <button
                          type="button"
                          class="action"
                          disabled={native.starting}
                          onclick={() => (handingOff = session.id)}
                          >{t('Hand off…')}</button
                        >
                      </span>
                    </li>
                  {/each}
                </ul>
              </div>
            {/each}
          {/if}
        </section>

        <DelegationsPanel {visible} />
      </div>
    </div>
  {/if}
</section>
