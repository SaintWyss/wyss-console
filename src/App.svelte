<script lang="ts">
  import { onMount } from 'svelte';
  import type { ManagedSessionId } from './lib/contracts/identities';
  import type { SessionSummary } from './lib/contracts/sessions';
  import { getAppInfo } from './lib/desktop/client';
  import AgentView from './lib/features/agent/AgentView.svelte';
  import { AgentController } from './lib/features/agent/controller';
  import ApprovalsView from './lib/features/approvals/ApprovalsView.svelte';
  import { ApprovalsController } from './lib/features/approvals/controller';
  import ConfigurationView from './lib/features/configuration/ConfigurationView.svelte';
  import { ConfigurationController } from './lib/features/configuration/controller';
  import { EventsController } from './lib/features/events/controller';
  import { SettingsController } from './lib/features/settings/controller';
  import SettingsView from './lib/features/settings/SettingsView.svelte';
  import { setAppearance } from './lib/features/terminal/appearance';
  import EventsView from './lib/features/events/EventsView.svelte';
  import { SessionsController } from './lib/features/sessions/controller';
  import {
    sessionLabel,
    sessionNames,
    shortId,
  } from './lib/features/sessions/present';
  import SessionsView from './lib/features/sessions/SessionsView.svelte';
  import TerminalView from './lib/features/terminal/TerminalView.svelte';
  import type { WorkspaceState } from './lib/features/terminal/session-manager';
  import { retryAuthority } from './lib/desktop/lifecycle-client';
  import AuthorityNotice from './lib/shell/AuthorityNotice.svelte';
  import {
    followLifecycle,
    lifecycleNotice,
    pendingLifecycle,
    type LifecycleObservation,
  } from './lib/shell/lifecycle';
  import Sidebar from './lib/shell/Sidebar.svelte';
  import TopBar from './lib/shell/TopBar.svelte';
  import {
    followAuthority,
    pendingAuthority,
    type AuthorityObservation,
  } from './lib/shell/authority';
  import { NAV_ITEMS, type ViewId } from './lib/shell/nav';
  import {
    idleRuntime,
    summarizeRuntime,
    type RuntimeSummary,
  } from './lib/shell/runtime';

  let version = $state('');
  let activeView = $state<ViewId>('console');
  let runtime = $state<RuntimeSummary>(idleRuntime);
  let authority = $state<AuthorityObservation>(pendingAuthority);
  // Whether the authority is being made available, and why not when it is not. The
  // shell follows it once; no view asks for it or starts anything for it.
  let lifecycle = $state<LifecycleObservation>(pendingLifecycle);
  let retrying = $state(false);
  const notice = $derived(lifecycleNotice(lifecycle, authority));
  // One controller for the application's lifetime, whichever view is shown.
  const sessionsController = new SessionsController((next) => {
    sessions = next;
  });
  let sessions = $state(sessionsController.state());
  // Likewise for turns: a turn goes on being read whichever view is shown.
  const agentController = new AgentController((next) => {
    agent = next;
  });
  let agent = $state(agentController.state());
  // And for approvals: one reader, for the Approvals view and for the card under a turn.
  const approvalsController = new ApprovalsController((next) => {
    approvals = next;
  });
  let approvals = $state(approvalsController.state());
  // History is read only while its view is shown, and has no watcher of its own.
  const eventsController = new EventsController((next) => {
    events = next;
  });
  let events = $state(eventsController.state());
  // What the agents are configured with: read, and followed for changes, only while its
  // view is shown. The desktop's watcher sees the files; the window only asks it.
  const configurationController = new ConfigurationController((next) => {
    configuration = next;
  });
  let configuration = $state(configurationController.state());
  // The desktop's own preferences: read once, written only when the user changes one.
  const settingsController = new SettingsController((next) => {
    settings = next;
  });
  let settings = $state(settingsController.state());
  const defaultFilter = $derived(settings.preferences.events.default_filter);
  let opened = false;
  let navigated = false;
  const current = $derived(NAV_ITEMS.find((item) => item.id === activeView));

  // Sessions are read only while their view is shown.
  $effect(() => {
    sessionsController.setVisible(activeView === 'sessions');
  });
  $effect(() => {
    agentController.setVisible(activeView === 'agents');
  });
  // Approvals are read while their view is shown or a turn is being followed: an
  // approval exists only for a turn that runs. Nothing is read for them otherwise.
  $effect(() => {
    approvalsController.setVisible(activeView === 'approvals');
  });
  $effect(() => {
    approvalsController.setWatching(agent.following);
  });
  $effect(() => {
    eventsController.setVisible(activeView === 'events');
  });
  $effect(() => {
    configurationController.setVisible(activeView === 'configuration');
  });

  // The window opens on the view the preferences name, unless the user has already
  // moved: a late answer never takes them somewhere they did not go.
  $effect(() => {
    if (settings.phase !== 'ready' || opened) return;
    opened = true;
    if (!navigated) activeView = settings.preferences.interface.landing_page;
  });
  // Presentation preferences, applied to every terminal, open or new.
  $effect(() => {
    const t = settings.preferences.terminal;
    setAppearance({
      fontSize: t.font_size,
      scrollback: t.scrollback,
      cursorStyle: t.cursor_style,
      cursorBlink: t.cursor_blink,
    });
  });
  // Where the history starts. It narrows what is shown and nothing the authority keeps.
  $effect(() => {
    eventsController.setFilter(defaultFilter);
  });

  onMount(() => {
    let active = true;
    settingsController.start();
    void getAppInfo().then(
      (info) => {
        if (active) version = info.version;
      },
      () => undefined,
    );
    // Separate from the terminal workspace: neither waits on the other. The one
    // follower also wakes the Sessions and Agent views; no other is started for them.
    const unfollow = followAuthority((observation) => {
      authority = observation;
      sessionsController.observe(observation);
      agentController.observe(observation);
      approvalsController.observe(observation);
      eventsController.observe(observation);
    });
    const unfollowLifecycle = followLifecycle((observation) => {
      lifecycle = observation;
    });
    return () => {
      active = false;
      unfollow();
      unfollowLifecycle();
      sessionsController.dispose();
      agentController.dispose();
      approvalsController.dispose();
      eventsController.dispose();
      configurationController.dispose();
      settingsController.dispose();
    };
  });

  function go(view: ViewId) {
    navigated = true;
    activeView = view;
  }

  // One name for each session the Sessions page has listed, unique among them. A name is
  // for reading: nothing is looked up, sent or decided by it.
  const names = $derived(sessionNames(sessions.page?.sessions ?? []));
  const nameOf = (id: ManagedSessionId): string | null => names.get(id) ?? null;

  /** Opens a session the user picked in Sessions in the Agent view, and shows it. */
  function openAgent(session: SessionSummary) {
    if (
      agentController.open(
        session.id,
        names.get(session.id) ?? sessionLabel(session),
        false,
      )
    )
      go('agents');
  }

  // From a record to where the thing it names lives. Nothing is taken from the record:
  // each view reads what the session is now from the authority.
  function showSession(session: ManagedSessionId) {
    sessionsController.select(session);
    go('sessions');
  }
  function showAgent(session: ManagedSessionId) {
    // A session the Sessions page has not listed is named from what the Agent view reads.
    const label = nameOf(session);
    if (
      agentController.open(
        session,
        label ?? `Session ${shortId(session)}`,
        label === null,
      )
    )
      go('agents');
  }
  /** The loaded history of one session, from its details in Sessions. */
  function showEvents(session: SessionSummary) {
    eventsController.setScope({ session: session.id, turn: null });
    go('events');
  }

  /** The user's Retry: one request at a time. What comes of it arrives as a view. */
  function retry() {
    if (retrying) return;
    retrying = true;
    void retryAuthority().then(
      () => (retrying = false),
      () => (retrying = false),
    );
  }

  function observeWorkspace(state: WorkspaceState) {
    runtime = summarizeRuntime(state);
  }
</script>

<div class="shell">
  <div class="ambient" aria-hidden="true">
    <span class="orb orb-a"></span>
    <span class="orb orb-b"></span>
    <span class="orb orb-c"></span>
  </div>
  <TopBar
    title={current?.label ?? ''}
    subtitle={current?.subtitle ?? ''}
    {runtime}
    {authority}
    {lifecycle}
    {version}
  />
  <Sidebar
    active={activeView}
    onselect={go}
    waiting={approvals.pending?.length ?? 0}
  />
  <main class="content">
    {#if notice}
      <AuthorityNotice {notice} {retrying} onretry={retry} />
    {/if}
    <!-- Every view stays mounted: leaving the Console never ends a terminal. -->
    <TerminalView
      visible={activeView === 'console'}
      onworkspace={observeWorkspace}
    />
    <SessionsView
      visible={activeView === 'sessions'}
      {sessions}
      controller={sessionsController}
      {authority}
      onagent={openAgent}
      onevents={showEvents}
    />
    <AgentView
      visible={activeView === 'agents'}
      {agent}
      controller={agentController}
      {authority}
      {approvals}
      {approvalsController}
      onsessions={() => go('sessions')}
    />
    <ApprovalsView
      visible={activeView === 'approvals'}
      {approvals}
      controller={approvalsController}
      {authority}
      {names}
    />
    <EventsView
      visible={activeView === 'events'}
      {events}
      controller={eventsController}
      {authority}
      timeFormat={settings.preferences.events.time_format}
      {nameOf}
      onsession={showSession}
      onagent={showAgent}
      onapprovals={() => go('approvals')}
    />
    <ConfigurationView
      visible={activeView === 'configuration'}
      {configuration}
      controller={configurationController}
    />
    <SettingsView
      visible={activeView === 'settings'}
      {settings}
      controller={settingsController}
      {authority}
      {lifecycle}
      {retrying}
      onretry={retry}
    />
  </main>
</div>
