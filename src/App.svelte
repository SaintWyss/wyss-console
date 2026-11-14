<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type { ManagedSessionId } from './lib/contracts/identities';
  import type { SessionSummary } from './lib/contracts/sessions';
  import { getAppInfo } from './lib/desktop/client';
  import AgentView from './lib/features/agent/AgentView.svelte';
  import { AgentController } from './lib/features/agent/controller';
  import ApprovalsView from './lib/features/approvals/ApprovalsView.svelte';
  import { ApprovalsController } from './lib/features/approvals/controller';
  import ArsenalView from './lib/features/arsenal/ArsenalView.svelte';
  import { ArsenalController } from './lib/features/arsenal/controller';
  import ConfigurationView from './lib/features/configuration/ConfigurationView.svelte';
  import { ChangesController } from './lib/features/configuration/changes-controller';
  import { ConfigurationController } from './lib/features/configuration/controller';
  import { CredentialsController } from './lib/features/credentials/controller';
  import CredentialsView from './lib/features/credentials/CredentialsView.svelte';
  import { EventsController } from './lib/features/events/controller';
  import { F8Controller } from './lib/features/f8/controller';
  import { linkText } from './lib/features/f8/present';
  import { M3Controller } from './lib/features/m3/controller';
  import ProvidersView from './lib/features/m3/ProvidersView.svelte';
  import PipelinesView from './lib/features/orchestration/PipelinesView.svelte';
  import RemoteView from './lib/features/remote/RemoteView.svelte';
  import { desktopM3 } from './lib/desktop/m3-client';
  import DistributorsView from './lib/features/distributors/DistributorsView.svelte';
  import { ProjectsController } from './lib/features/projects/controller';
  import ProjectsGate from './lib/features/projects/ProjectsGate.svelte';
  import ProjectsView from './lib/features/projects/ProjectsView.svelte';
  import { desktopProjects } from './lib/desktop/projects-client';
  import RuntimesView from './lib/features/f8/RuntimesView.svelte';
  import { ModelsController } from './lib/features/models/controller';
  import ModelsView from './lib/features/models/ModelsView.svelte';
  import { NativeController } from './lib/features/native/controller';
  import ProfilesView from './lib/features/native/ProfilesView.svelte';
  import WorkspaceView from './lib/features/native/WorkspaceView.svelte';
  import LaunchEditor from './lib/features/workspace/LaunchEditor.svelte';
  import { SettingsController } from './lib/features/settings/controller';
  import SettingsView from './lib/features/settings/SettingsView.svelte';
  import { whenWorkspaceReady } from './lib/features/native/bridge';
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
  import LifecycleCard from './lib/features/terminal/LifecycleCard.svelte';
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
  import { msg, setLanguage, t } from './lib/i18n/index.svelte';
  import StatusBar from './lib/shell/StatusBar.svelte';
  import {
    followAuthority,
    pendingAuthority,
    type AuthorityObservation,
  } from './lib/shell/authority';
  import { navItems, type ShownView } from './lib/shell/nav';
  import {
    idleRuntime,
    summarizeRuntime,
    type RuntimeSummary,
  } from './lib/shell/runtime';

  let version = $state('');
  // Activity is not a view of its own: it shows Approvals over Events, both unchanged.
  let activeView = $state<ShownView>('console');
  // The terminal workspace is mounted on the first visit to the Console, and stays mounted after
  // it. The provisional 'console' above does not count: the preference decides where the window opens.
  let consoleOpened = $state(false);
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
  // The Arsenal library: read while its view is shown; it changes only what the owner asks for.
  const arsenalController = new ArsenalController((next) => {
    arsenal = next;
  });
  let arsenal = $state(arsenalController.snapshot());
  // Changing it: a plan is drafted, confirmed here, and applied by the desktop, which asks
  // again in a window of its own when the change is risky.
  const changesController = new ChangesController(
    (next) => {
      changes = next;
    },
    () => configurationController.refresh(),
  );
  let changes = $state(changesController.state());
  // Agents started in terminals and the profiles they start with. An agent that was started
  // opens as a tab of the Console, which is shown.
  const nativeController = new NativeController(
    (next) => {
      native = next;
    },
    (reply) => {
      // The Console may not be mounted yet: the tab is made once its terminals are restored.
      whenWorkspaceReady((workspace) => {
        const tab = workspace.tabOf(reply.session.id);
        if (tab) workspace.activate(tab);
        else workspace.createNative(reply);
      });
      go('console');
    },
  );
  let native = $state(nativeController.state());
  // What the managed runtime offers: read once when a view that needs it is first shown.
  const modelsController = new ModelsController((next) => {
    models = next;
  });
  let models = $state(modelsController.state());
  // API keys: listed by name while their view is shown; a key never reaches the window.
  const credentialsController = new CredentialsController((next) => {
    credentials = next;
  });
  let credentials = $state(credentialsController.state());
  // The desktop's own preferences: read once, written only when the user changes one.
  const settingsController = new SettingsController((next) => {
    settings = next;
  });
  let settings = $state(settingsController.state());
  const defaultFilter = $derived(settings.preferences.events.default_filter);
  // Managed runtimes of protocol 2. Whether the interface exists is the desktop's answer,
  // asked once: the installed desktop answers inactive and shows nothing of it.
  // Read only while its view is shown; it carries the fixture runtime's few actions where a build hosts one.
  const m3Controller = new M3Controller(desktopM3, (next) => {
    m3 = next;
  });
  let m3 = $state(m3Controller.state());
  const f8Controller = new F8Controller((next) => {
    f8 = next;
  });
  let f8 = $state(f8Controller.state());
  // The project-centered workspace: off until the owner turns it on in Settings. Its state is
  // read once at start (the gate), and then only while its view is shown.
  const projectsController = new ProjectsController((next) => {
    projects = next;
  });
  let projects = $state(projectsController.state());
  const items = $derived(
    navItems(f8.gate === 'active', projects.overview?.enabled === true, true),
  );
  let opened = false;
  let navigated = false;
  const activity = $derived(activeView === 'activity');
  // The name of the view, alone, over Activity and over the views that draw no heading of
  // their own: there it is the only place that says which view is open. No subtitle.
  const TITLED: readonly ShownView[] = [
    'workspace',
    'agents',
    'projects',
    'models',
    'profiles',
    'credentials',
  ];
  const title = $derived(
    activity
      ? msg('Activity')
      : TITLED.includes(activeView)
        ? (items.find((item) => item.id === activeView)?.label ?? '')
        : '',
  );

  // Sessions are read only while their view is shown.
  $effect(() => {
    agentController.setVisible(activeView === 'agents');
  });
  $effect(() => {
    f8Controller.setVisible(activeView === 'runtimes');
  });
  $effect(() => {
    m3Controller.setVisible(activeView === 'providers');
  });
  $effect(() => {
    projectsController.setVisible(activeView === 'projects');
  });
  // Approvals are read while their view is shown or a turn is being followed: an
  // approval exists only for a turn that runs. Nothing is read for them otherwise.
  $effect(() => {
    approvalsController.setVisible(activeView === 'approvals' || activity);
  });
  $effect(() => {
    approvalsController.setWatching(agent.following);
  });
  $effect(() => {
    eventsController.setVisible(activeView === 'events' || activity);
  });
  // The Models view reads the same inventory as Configuration, the managed sessions of the
  // Sessions view and the profiles, and the runtime's list: it asks for nothing of its own.
  $effect(() => {
    configurationController.setVisible(
      activeView === 'configuration' ||
        activeView === 'models' ||
        activeView === 'distributors',
    );
  });
  $effect(() => {
    arsenalController.setVisible(activeView === 'arsenal');
  });
  $effect(() => {
    if (activeView === 'models') sessionsController.setVisible(true);
    else sessionsController.setVisible(activeView === 'sessions');
  });
  $effect(() => {
    modelsController.setVisible(
      activeView === 'models' ||
        activeView === 'profiles' ||
        activeView === 'distributors',
    );
  });
  $effect(() => {
    credentialsController.setVisible(activeView === 'credentials');
  });
  // The overview reads the registry (with resources) while it is shown, and the profiles
  // are read once when either of their views first is.
  $effect(() => {
    nativeController.setVisible(
      activeView === 'workspace' ||
        activeView === 'launches' ||
        activeView === 'profiles' ||
        activeView === 'models' ||
        activeView === 'distributors' ||
        activeView === 'projects',
    );
  });

  // The window opens on the view the preferences name, unless the user has already
  // moved: a late answer never takes them somewhere they did not go.
  $effect(() => {
    if (settings.phase === 'loading' || opened) return;
    // Preferences that cannot be read leave the window on its default view, the Console, as before.
    if (settings.phase === 'failed') {
      opened = true;
      if (activeView === 'console') consoleOpened = true;
      return;
    }
    opened = true;
    if (!navigated) activeView = settings.preferences.interface.landing_page;
    if (activeView === 'console') consoleOpened = true;
  });
  // Presentation preferences, applied to every terminal, open or new.
  $effect(() => {
    const theme = settings.preferences.interface.theme;
    document.documentElement.dataset.theme = theme;
    setLanguage(settings.preferences.interface.language);
    const t = settings.preferences.terminal;
    setAppearance({
      theme,
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
    void f8Controller.start();
    void projectsController.refresh();
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
      arsenalController.dispose();
      changesController.dispose();
      nativeController.dispose();
      modelsController.dispose();
      credentialsController.dispose();
      settingsController.dispose();
      f8Controller.dispose();
      m3Controller.dispose();
      projectsController.dispose();
    };
  });

  function go(view: ShownView) {
    navigated = true;
    if (view === 'console') consoleOpened = true;
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
        label ?? t('Session {id}', { id: shortId(session) }),
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

  let terminal = $state<TerminalView>();
  /** Shows the Console and opens a saved launch there (the launch editor's Try). */
  async function openInConsole(id: string) {
    go('console');
    for (let attempt = 0; attempt < 40; attempt++) {
      await tick();
      if (terminal) {
        await terminal.open(id);
        return;
      }
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
  }
  /** Shows the Console and opens a terminal that types `input`; waits for the workspace to mount. */
  async function runInConsole(title: string, cwd: string, input: string) {
    go('console');
    for (let attempt = 0; attempt < 40; attempt++) {
      await tick();
      if (terminal?.runCommand(title, cwd, input)) return;
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
  }
  /** Distributors' refresh: the tools, the agents' files, the profiles and the Codex list. */
  async function refreshDistributors(): Promise<boolean> {
    const [answered] = await Promise.all([
      nativeController.rediscover(),
      configurationController.refresh(),
      nativeController.refreshProfiles(),
      modelsController.refresh(),
    ]);
    return answered;
  }
  let legend = $state(false);
  let launchOpen = $state(false);
  function observeWorkspace(state: WorkspaceState) {
    runtime = summarizeRuntime(state);
  }
  const f8Link = $derived(
    f8.gate === 'active' && f8.link ? linkText(f8.link) : null,
  );
</script>

<div class="shell">
  <a class="skip-link" href="#main-content">{t('Skip to main content')}</a>
  <!-- The product's name and version are read out; the window's title bar shows the name. -->
  <div class="visually-hidden">
    <h1>Wyss Console</h1>
    {#if version}<span>v{version}</span>{/if}
  </div>
  <Sidebar
    active={activeView}
    onselect={go}
    waiting={approvals.pending?.length ?? 0}
    {items}
  />
  <main id="main-content" class="content" tabindex="-1" data-view={activeView}>
    {#if title}
      <p class="context-title">{t(title)}</p>
    {/if}
    {#if notice}
      <AuthorityNotice {notice} {retrying} onretry={retry} />
    {/if}
    <!-- Mounted on the first visit to the Console, then kept: leaving the Console never ends a terminal. -->
    {#if consoleOpened}
      <TerminalView
        bind:this={terminal}
        bind:legend
        bind:launchesOpen={launchOpen}
        visible={activeView === 'console'}
        onworkspace={observeWorkspace}
      />
    {/if}
    <LaunchEditor
      visible={activeView === 'launches'}
      profiles={native.profiles}
      recent={native.launcher?.projects ?? []}
      choose={() => nativeController.choose()}
      onopen={(id) => void openInConsole(id)}
    />
    <WorkspaceView
      visible={activeView === 'workspace'}
      {native}
      controller={nativeController}
      onconsole={() => go('console')}
      onupdate={runInConsole}
      onarsenal={() => go('arsenal')}
      configItems={configuration.reply?.inventory.items ?? null}
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
    <PipelinesView
      visible={activeView === 'pipelines'}
      f8Active={f8.gate === 'active'}
      onruntimes={() => go('runtimes')}
    />
    <RemoteView visible={activeView === 'remote'} />
    {#if projects.overview?.enabled}
      <ProjectsView
        visible={activeView === 'projects'}
        {projects}
        controller={projectsController}
        client={desktopProjects}
        nativeProfiles={native.profiles}
      />
    {/if}
    <DistributorsView
      visible={activeView === 'distributors'}
      {native}
      {models}
      configItems={configuration.reply?.inventory.items ?? null}
      configStale={configuration.stale}
      onrefresh={refreshDistributors}
    />
    <ProvidersView
      visible={activeView === 'providers'}
      view={m3}
      controller={m3Controller}
    />
    {#if f8.gate === 'active'}
      <RuntimesView
        visible={activeView === 'runtimes'}
        {f8}
        controller={f8Controller}
      />
    {/if}
    <ApprovalsView
      visible={activeView === 'approvals' || activity}
      {approvals}
      controller={approvalsController}
      {authority}
      {names}
    />
    <EventsView
      visible={activeView === 'events' || activity}
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
      {changes}
      {changesController}
    />
    <ArsenalView
      visible={activeView === 'arsenal'}
      {arsenal}
      controller={arsenalController}
    />
    <ModelsView
      visible={activeView === 'models'}
      {models}
      controller={modelsController}
      configItems={configuration.reply?.inventory.items ?? null}
      profiles={native.profiles}
      sessions={sessions.page?.sessions ?? null}
    />
    <ProfilesView
      visible={activeView === 'profiles'}
      {native}
      controller={nativeController}
      catalogue={models.catalogue}
    />
    <CredentialsView
      visible={activeView === 'credentials'}
      {credentials}
      controller={credentialsController}
    />
    <SettingsView
      visible={activeView === 'settings'}
      {settings}
      controller={settingsController}
      {authority}
      {lifecycle}
      {retrying}
      onretry={retry}
    >
      <ProjectsGate {projects} controller={projectsController} />
      <LifecycleCard />
    </SettingsView>
  </main>
  <!-- On the Console the session bar takes its place; it stays mounted, and read out, there. -->
  <StatusBar
    hidden={activeView === 'console'}
    {runtime}
    {authority}
    {lifecycle}
    f8={f8Link}
  />
</div>
