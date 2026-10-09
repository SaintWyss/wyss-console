<script lang="ts">
  import { onMount } from 'svelte';
  import type { SessionSummary } from './lib/contracts/sessions';
  import { getAppInfo } from './lib/desktop/client';
  import AgentView from './lib/features/agent/AgentView.svelte';
  import { AgentController } from './lib/features/agent/controller';
  import { SessionsController } from './lib/features/sessions/controller';
  import { sessionLabel } from './lib/features/sessions/present';
  import SessionsView from './lib/features/sessions/SessionsView.svelte';
  import TerminalView from './lib/features/terminal/TerminalView.svelte';
  import type { WorkspaceState } from './lib/features/terminal/session-manager';
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
  const current = $derived(NAV_ITEMS.find((item) => item.id === activeView));

  // Sessions are read only while their view is shown.
  $effect(() => {
    sessionsController.setVisible(activeView === 'sessions');
  });
  $effect(() => {
    agentController.setVisible(activeView === 'agents');
  });

  onMount(() => {
    let active = true;
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
    });
    return () => {
      active = false;
      unfollow();
      sessionsController.dispose();
      agentController.dispose();
    };
  });

  /** Opens a session the user picked in Sessions in the Agent view, and shows it. */
  function openAgent(session: SessionSummary) {
    if (agentController.open(session.id, sessionLabel(session)))
      activeView = 'agents';
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
    {version}
  />
  <Sidebar active={activeView} onselect={(id) => (activeView = id)} />
  <main class="content">
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
    />
    <AgentView
      visible={activeView === 'agents'}
      {agent}
      controller={agentController}
      {authority}
      onsessions={() => (activeView = 'sessions')}
    />
  </main>
</div>
