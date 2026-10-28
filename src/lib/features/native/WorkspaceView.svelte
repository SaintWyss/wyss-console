<script lang="ts">
  import { onMount } from 'svelte';
  import HandoffPanel from './HandoffPanel.svelte';
  import { hostIsRemote } from '../../desktop/terminal-client';
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
    configItems = null,
  }: {
    native: NativeState;
    controller: NativeController;
    visible: boolean;
    /** Shows the terminals. */
    onconsole: () => void;
    /** What the agents' files hold, once Configuration has been read; else null. */
    configItems?: readonly ConfigItem[] | null;
  } = $props();

  let agent = $state<AgentId | null>(null);
  let project = $state<string | null>(null);
  let profile = $state<string | null>(null);
  let preview = $state<Preview | null>(null);
  let previewFor = $state<string | null>(null);
  let previewPendingFor = $state<string | null>(null);
  let previewFailedFor = $state<string | null>(null);
  let previewRetry = $state(0);
  let now = $state(Date.now());
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
      (profile === null || (previewCurrent && preview?.can_start === true)),
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
  // Whether a host process holds the sessions (and so keeps them when the window closes).
  let held = $state(false);
  onMount(() => {
    controller.start0();
    void hostIsRemote().then((remote) => (held = remote));
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
    if (await controller.startAgent(agent, project, profile, null, preview))
      onconsole();
  }
  function open(session: NativeSession) {
    if (controller.showTerminal(session.id)) onconsole();
  }
  function ended(s: NativeSession): string {
    if (!s.ended_at) return '';
    return ` · ended ${new Date(s.ended_at * 1000).toLocaleTimeString(undefined, { timeStyle: 'short' })}`;
  }
  const profileName = (id: string) =>
    native.profiles.find((p) => p.id === id)?.body.name ?? 'profile';
</script>

<section
  class="native-feature"
  aria-label="Agents started from the Console"
  hidden={!visible}
  inert={!visible}
>
  <p class="native-note">
    Start Claude Code, Codex, OpenCode or Hermes in a terminal of this window.
    They run as <strong>Native</strong> sessions: the provider's own tools,
    configuration and privileges remain active; the Console does not mediate,
    contain or approve their effects.
    {#if held}They keep running when this window closes: a host process holds
      them, and they end when it stops or you log out.{:else}Here they end when
      the window closes: no host process could be started.{/if}
  </p>
  {#if chosen?.agent_id === 'hermes'}
    <p class="sessions-status failed" role="note">
      Hermes Native is a full-account provider session. Its approval prompts and
      hooks are not a Wyss security boundary. Hermes planner admission is
      separately blocked pending tested OS isolation. The installed version is
      not probed because Hermes version output performs an update check.
    </p>
  {/if}

  {#if native.failure}
    <p class="sessions-status failed" role="alert">
      {failureText(native.failure.code)}
      {#each native.failure.reasons as reason (reason)}
        <br />{reason}
      {/each}
    </p>
  {/if}

  {#if native.phase === 'loading' || native.phase === 'idle'}
    <div class="sessions-empty" role="status" aria-busy="true">
      <p class="empty-title">Looking for your agents…</p>
    </div>
  {:else if native.phase === 'failed' || !launcher}
    <div class="sessions-empty" role="alert">
      <p class="empty-title">Agents cannot be started here</p>
      <p>The desktop did not answer. Nothing was started.</p>
      <button
        type="button"
        class="action"
        onclick={() => controller.refreshLauncher()}>Try again</button
      >
    </div>
  {:else}
    <div class="native-main">
      {#if native.m9Uncertain}
        <p class="sessions-status" role="status">
          OpenCode and Hermes wait until the Console confirms its private M9
          session store again.
          <button
            type="button"
            class="action"
            onclick={() => controller.refreshLauncher()}>Check again</button
          >
        </p>
      {:else if launcher.m9_native_status === 'storage_unavailable'}
        <p class="sessions-status failed" role="alert">
          OpenCode and Hermes are blocked: the Console could not confirm its
          private M9 session store, because a read or a write failed. Claude
          Code and Codex stay available where the Console allows them.
        </p>
      {:else if launcher.m9_native_status === 'legacy_host'}
        <p class="sessions-status" role="note">
          This terminal host predates M9. OpenCode and Hermes Native require a
          matching M9 host; existing Native sessions remain available.
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
      <section class="native-card native-launch" aria-label="Start an agent">
        <h3>Start an agent</h3>
        <div class="native-agents" role="radiogroup" aria-label="Agent">
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
                {#if entry.installed}{entry.version ?? 'installed'}{:else}Not
                  installed{/if}
              </span>
            </label>
          {/each}
        </div>
        {#if chosen?.installed}
          <p class="event-meta native-capability" aria-live="polite">
            {#if chosen.profile_capability.status === 'syntax_verified'}
              Profile override syntax is verified for {chosen.profile_capability
                .qualified_version};
              {chosen.profile_capability.limitation}.
            {:else}
              Profile overrides are unverified for {chosen.version ??
                'an unknown provider build'}. “No profile — as configured”
              remains available.
            {/if}
          </p>
          <details class="native-capability-details">
            <summary>Capability evidence and limits</summary>
            {#if chosen.profile_capability.status === 'syntax_verified'}
              <p>Checked {chosen.profile_capability.checked_on}.</p>
              <ul>
                {#each chosen.profile_capability.operations as operation (operation)}
                  <li>{operation}</li>
                {/each}
              </ul>
              <p>
                Interface: {chosen.profile_capability.interface}. Format:
                {chosen.profile_capability.format}. Scope:
                {chosen.profile_capability.scope}.
              </p>
              <p>Reload: {chosen.profile_capability.reload_semantics}.</p>
              <p>Source: {chosen.profile_capability.source}.</p>
              <p>Limit: {chosen.profile_capability.limitation}.</p>
            {:else}
              <p>{chosen.profile_capability.limitation}</p>
            {/if}
          </details>
        {/if}

        <div class="native-field">
          <label for="native-project">Project folder</label>
          <div class="native-row">
            <select id="native-project" bind:value={project}>
              {#each projects as p (p.path)}
                <option value={p.path}
                  >{p.home ? `Home · ${p.path}` : p.path}</option
                >
              {/each}
            </select>
            <button type="button" class="action" onclick={chooseFolder}
              >Choose folder…</button
            >
          </div>
        </div>
        {#if profile && !previewCurrent}
          <p class="native-warning" role="status">
            {#if previewFailedFor === previewContext}
              Could not verify this profile preview. Start stays disabled until
              a fresh preview succeeds.
            {:else if previewPendingFor === previewContext}
              Refreshing the profile preview…
            {:else}
              The selected profile changed. Checking a fresh preview before
              Start.
            {/if}
          </p>
          {#if previewFailedFor === previewContext}
            <button
              type="button"
              class="action"
              disabled={native.starting}
              onclick={retryPreview}>Retry preview</button
            >
          {/if}
        {/if}

        <div class="native-field">
          <label for="native-profile">Profile</label>
          <div class="native-row">
            <select id="native-profile" bind:value={profile}>
              <option value={null}>No profile — as configured</option>
              {#each profiles as p (p.id)}
                <option value={p.id}
                  >{p.body.name}{p.template ? ' (template)' : ''}</option
                >
              {/each}
            </select>
            {#if project && known && (known.default_profile ?? null) !== profile}
              <button
                type="button"
                class="action"
                onclick={() =>
                  project && controller.setProjectDefault(project, profile)}
                >Use for this project</button
              >
            {/if}
          </div>
          {#if preview && previewFor === previewContext}
            <ul class="native-preview" aria-label="What the profile does">
              {#each preview.applied as line (line)}<li>{line}</li>{/each}
              {#each preview.with_cost as line (line)}<li class="cost">
                  {line}
                </li>{/each}
              {#each preview.not_applicable as line (line)}<li class="refused">
                  {line}
                </li>{/each}
              {#each preview.warnings as line (line)}<li class="evidence">
                  {line}
                </li>{/each}
            </ul>
            {#if !preview.can_start}
              <p class="native-warning">
                This profile cannot be applied to {agent
                  ? AGENT_LABEL[agent]
                  : 'this agent'}, so it will not start.
              </p>
            {/if}
          {/if}
        </div>

        {#if launcher.shell}
          <p class="event-meta">
            {#if launcher.shell.through_shell}
              Starts through your shell ({launcher.shell.family}), so the
              environment is the one you get when you type the command yourself.
            {:else}
              Your shell has no start recipe here, so the agent is started
              directly with the Console’s own environment.
            {/if}
          </p>
        {/if}

        <div class="native-plan-actions">
          <button
            type="button"
            class="action primary"
            disabled={!canStart}
            onclick={launch}
          >
            {native.starting
              ? 'Starting…'
              : `Start ${agent ? AGENT_LABEL[agent] : 'agent'}`}
          </button>
        </div>
      </section>

      <section aria-label="Native sessions">
        <h3 class="native-heading">
          Sessions <span class="pill compact" data-tone="idle"
            ><span class="dot" aria-hidden="true"></span>Native plane</span
          >
          {#if toRestore.length > 0}
            <button
              type="button"
              class="action"
              disabled={native.starting}
              onclick={async () => {
                if ((await controller.restoreLost()) > 0) onconsole();
              }}
              >Start {toRestore.length === 1
                ? 'the lost session'
                : `${toRestore.length} lost sessions`} again</button
            >
          {/if}
          {#if native.sessions.some((s) => s.state !== 'running' && s.state !== 'requested')}
            <button
              type="button"
              class="action"
              onclick={() => controller.forgetFinished()}
              >Remove finished</button
            >
          {/if}
        </h3>
        {#if native.sessions.length === 0}
          <div class="sessions-empty">
            <p class="empty-title">No agent sessions yet</p>
            <p>Start one above. It opens in the Console as a terminal tab.</p>
          </div>
        {:else}
          {#each groups as group (group.project)}
            <div class="native-group">
              <p class="event-meta">
                <span class="mono">{group.project}</span>
                {#if crowded.has(group.project)}
                  <span class="native-warning">
                    · Two sessions run here and can write the same files.
                    Nothing coordinates them.
                  </span>
                {/if}
              </p>
              <ul
                class="events-list"
                aria-label={`Sessions in ${folderName(group.project)}`}
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
                      title={STATE_NOTE[session.state]}
                    >
                      <span class="dot" aria-hidden="true"></span>{label.text}
                    </span>
                    <span class="event-meta native-facts">
                      {#if session.profile}Profile {profileName(
                          session.profile.id,
                        )} (v{session.profile.version}) ·
                      {:else}No profile ·
                      {/if}
                      {#if session.state === 'running' || session.state === 'requested'}
                        up {since(session.started_at, now)}
                      {:else}
                        started {new Date(
                          session.started_at * 1000,
                        ).toLocaleString(undefined, {
                          dateStyle: 'short',
                          timeStyle: 'short',
                        })}{ended(session)}
                      {/if}
                      {#if !session.through_shell}
                        · started directly{/if}
                      {#if session.restarts > 0}
                        · restart {session.restarts}{/if}
                      {#if session.state === 'running' && session.pid}
                        · process {session.pid}{/if}
                      {#if session.usage}
                        · {session.usage.cpu_percent === null
                          ? '…'
                          : `${session.usage.cpu_percent.toFixed(0)}%`} CPU · {formatBytes(
                          session.usage.rss_bytes,
                        )}{session.usage.processes > 1
                          ? ` · ${session.usage.processes} processes`
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
                        Model: asked for {facts.requested} · set in its files {facts.configured}
                        · in use: {facts.inUse} · Effort: asked for {facts.requestedEffort}
                        · set in its files {facts.configuredEffort}
                      </span>
                      <span class="event-meta native-note" data-testid="switch">
                        {switchHint(session.agent_id)}
                      </span>
                    {/if}
                    {#if session.state === 'lost'}
                      <span class="event-meta native-note"
                        >{STATE_NOTE.lost}</span
                      >
                    {/if}
                    <span class="native-actions">
                      {#if session.state === 'running' || session.state === 'requested'}
                        <button
                          type="button"
                          class="action"
                          disabled={!controller.hasTab(session.id)}
                          onclick={() => open(session)}>Open</button
                        >
                        <button
                          type="button"
                          class="action"
                          onclick={() => controller.stop(session.id)}
                          >Stop</button
                        >
                        <button
                          type="button"
                          class="action"
                          disabled={native.starting ||
                            (native.m9Blocked && isM9Agent(session.agent_id))}
                          onclick={() => controller.restart(session.id)}
                          >Restart</button
                        >
                      {:else}
                        {#if controller.hasTab(session.id)}
                          <button
                            type="button"
                            class="action"
                            onclick={() => open(session)}>Show terminal</button
                          >
                        {/if}
                        <button
                          type="button"
                          class="action"
                          disabled={native.starting ||
                            (native.m9Blocked && isM9Agent(session.agent_id))}
                          onclick={() => controller.restart(session.id)}
                          >Start again</button
                        >
                        <button
                          type="button"
                          class="action"
                          onclick={() => controller.forget(session.id)}
                          >Remove</button
                        >
                      {/if}
                      <button
                        type="button"
                        class="action"
                        disabled={native.starting}
                        onclick={() => (handingOff = session.id)}
                        >Hand off…</button
                      >
                    </span>
                  </li>
                {/each}
              </ul>
            </div>
          {/each}
        {/if}
      </section>
    </div>
  {/if}
</section>
