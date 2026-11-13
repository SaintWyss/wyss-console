<script lang="ts">
  import { onMount } from 'svelte';
  import { mountWorkspace } from './mount';
  import { KEY_LEGEND, leaderAction } from './keymap';
  import { arrange, leaves, type Rect } from './layout';
  import type {
    TabView,
    TerminalSessionManager,
    WorkspaceState,
  } from './session-manager';
  import type { ConsoleSessionId } from '../../contracts/identities';
  import { registerWorkspace } from '../native/bridge';
  import TerminalPanel from './TerminalPanel.svelte';
  import { listLaunches, type LaunchList } from '../../desktop/launch-client';
  import { readProfiles } from '../../desktop/native-client';
  import type { Profile } from '../../contracts/native';
  import { desktopLaunchDeps } from './launch-deps';
  import { BYPASS_WARNING, failureText } from '../native/present';
  import {
    isFullAccess,
    LaunchRefusal,
    openLaunch,
    type LaunchNote,
  } from './launch';
  import { applyOrder, loadOrder, moveLaunch, saveOrder } from './launch-order';
  import { msg, t } from '../../i18n/index.svelte';
  import SessionBar from './SessionBar.svelte';
  import { arsenalClient } from '../../desktop/arsenal-client';
  import {
    estimateLaunch,
    loadEstimateData,
    type EstimateData,
  } from '../workspace/data';
  import { kTokens } from '../workspace/estimate';
  import { terminalGitStatus } from '../../desktop/terminal-client';
  let {
    onworkspace,
    visible = true,
    legend = $bindable(false),
    launchesOpen = $bindable(false),
  }: {
    onworkspace?: (state: WorkspaceState) => void;
    /** False while another view is shown. The terminals keep running. */
    visible?: boolean;
    /** The shortcuts list, opened from the Console's toolbar. */
    legend?: boolean;
    /** The Launch menu, opened from the Console's toolbar. */
    launchesOpen?: boolean;
  } = $props();
  let workspace = $state<WorkspaceState>({
    sessions: [],
    tabs: [],
    creating: false,
  });
  let manager = $state<TerminalSessionManager>();
  let strip: HTMLDivElement;
  let area = $state<HTMLDivElement>();
  let searchFor = $state<ConsoleSessionId>();
  /** The launch panel (ADR 0059): the owner's launch files, read when it opens. */
  let launches = $state<LaunchList | null>(null);
  let launchProfiles = $state<Profile[]>([]);
  /** For each launch's estimated context; read when the menu opens, best effort. */
  let estimates = $state<EstimateData | null>(null);
  let launchBusy = $state<string | null>(null);
  let launchNote = $state<LaunchNote[] | null>(null);
  let launchError = $state<{
    text: string;
    translatable: boolean;
    values?: Record<string, string>;
  } | null>(null);
  let launchOrder = $state<string[]>(loadOrder());
  let reordering = $state(false);
  const ordered = $derived(
    Array.isArray(launches?.launches)
      ? applyOrder(launches.launches, launchOrder)
      : [],
  );
  function shift(id: string, by: -1 | 1) {
    launchOrder = moveLaunch(
      ordered.map((l) => l.id),
      id,
      by,
    );
    saveOrder(launchOrder);
  }
  function closeMenu() {
    launchesOpen = false;
    reordering = false;
  }
  function onWindowClick(event: MouseEvent) {
    if (
      launchesOpen &&
      !(event.target as Element | null)?.closest(
        '.launch-panel, .launch-trigger',
      )
    )
      closeMenu();
  }
  function onWindowKey(event: KeyboardEvent) {
    if (launchesOpen && event.key === 'Escape') {
      closeMenu();
      document.querySelector<HTMLElement>('.launch-trigger')?.focus();
    }
  }
  /** A new tab in `cwd` titled `title` that types `input` once its shell runs. False before the workspace is ready. */
  export function runCommand(
    title: string,
    cwd: string,
    input: string,
  ): boolean {
    return !!manager?.launch(title, cwd, input);
  }
  /** Used by the toolbar's split buttons. */
  export function split(dir: 'row' | 'col') {
    if (activeId) manager?.split(activeId, dir);
  }
  function explain(error: unknown): {
    text: string;
    translatable: boolean;
    values?: Record<string, string>;
  } {
    if (error instanceof LaunchRefusal)
      return {
        text: error.template,
        translatable: true,
        values: error.values,
      };
    const e = error as {
      code?: string;
      reasons?: string[];
      failure?: { code: string };
    };
    if (e?.failure?.code)
      return { text: failureText(e.failure.code), translatable: true };
    if (e?.reasons?.length) return { text: e.reasons[0], translatable: false };
    return error instanceof Error
      ? { text: error.message, translatable: false }
      : { text: msg('The launch could not be opened.'), translatable: true };
  }
  /** Opens or closes the Launch menu (the toolbar's + Launch). */
  export async function toggleLaunches() {
    launchesOpen = !launchesOpen;
    if (!launchesOpen) return;
    launchError = null;
    try {
      launches = await listLaunches();
      launchProfiles = await readProfiles().catch(() => []);
      void loadEstimateData(arsenalClient).then((data) => (estimates = data));
    } catch (error) {
      launches = null;
      launchError = explain(error);
    }
  }
  /** Opens a saved launch by its id (the launch editor's Try). */
  export async function open(id: string) {
    const m = manager;
    if (!m || launchBusy) return;
    launchBusy = id;
    launchNote = null;
    launchError = null;
    try {
      const opened = await openLaunch(
        id,
        desktopLaunchDeps({
          openTerminal: (title, cwd, input) => m.launch(title, cwd, input),
          openNative: (reply) => m.createNative(reply),
        }),
      );
      launchesOpen = false;
      launchNote = opened.notes.length > 0 ? opened.notes : null;
    } catch (error) {
      launchError = explain(error);
    } finally {
      launchBusy = null;
    }
  }
  const activeId = $derived(workspace.active);
  const focused = $derived(
    workspace.sessions.find((s) => s.id === workspace.active),
  );
  const count = $derived(workspace.sessions.length);
  const activeTab = $derived(
    workspace.tabs.find((t) => t.id === workspace.activeTab),
  );
  const arrangement = $derived(activeTab ? arrange(activeTab.root) : undefined);
  const WHOLE: Rect = { x: 0, y: 0, w: 1, h: 1 };
  /** The tab a session is in. */
  const tabOf = (id: ConsoleSessionId): TabView | undefined =>
    workspace.tabs.find((t) => leaves(t.root).includes(id));
  function revealActiveTab() {
    const tab = strip?.querySelector('[aria-selected="true"]')?.parentElement;
    if (!tab) return;
    const viewport = strip.getBoundingClientRect();
    const bounds = tab.getBoundingClientRect();
    if (viewport.width <= 0) return;
    if (bounds.left < viewport.left)
      strip.scrollLeft -= viewport.left - bounds.left;
    else if (bounds.right > viewport.right)
      strip.scrollLeft += bounds.right - viewport.right;
  }
  $effect(() => {
    if (workspace.activeTab && count) revealActiveTab();
  });
  let shown = true;
  $effect(() => {
    // Back in view: the active terminal fits its host again and takes focus.
    if (visible && !shown && manager && activeId) manager.activate(activeId);
    shown = visible;
  });
  onMount(() => {
    const observer = new ResizeObserver(revealActiveTab);
    observer.observe(strip);
    const lease = mountWorkspace((state) => {
      workspace = state;
      onworkspace?.(state);
    });
    manager = lease.manager;
    const unregister = registerWorkspace(lease.manager);
    return () => {
      unregister();
      observer.disconnect();
      void lease.dispose();
    };
  });
  function tabKey(event: KeyboardEvent, id: string) {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    const tabs = workspace.tabs;
    const index = tabs.findIndex((t) => t.id === id);
    const next =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? tabs.length - 1
          : (index + (event.key === 'ArrowRight' ? 1 : -1) + tabs.length) %
            tabs.length;
    if (tabs[next]) {
      event.preventDefault();
      // Manual activation: arrows move tab focus; Enter/Space select naturally.
      document.getElementById(`tab-${tabs[next].id}`)?.focus();
    }
  }
  /** The Console's own chords, taken before the terminal sees them (C-12). */
  function chord(event: KeyboardEvent) {
    if (!visible || !manager) return;
    const action = leaderAction(event);
    if (!action) return;
    event.preventDefault();
    event.stopPropagation();
    switch (action.do) {
      case 'new-tab':
        manager.create();
        break;
      case 'split':
        if (activeId) manager.split(activeId, action.dir);
        break;
      case 'close-pane':
        if (activeId) void manager.close(activeId);
        break;
      case 'search':
        searchFor = activeId;
        break;
      case 'focus':
        manager.focusPane(action.way);
        break;
      case 'tab':
        manager.stepTab(action.step);
        break;
      case 'jump':
        manager.jumpTab(action.n);
        break;
    }
  }
  function tabTitle(tab: TabView): string {
    const focus = workspace.sessions.find((s) => s.id === tab.focus);
    const panes = leaves(tab.root).length;
    return `${focus?.title ?? t('Terminal')}${panes > 1 ? ` ▦ ${panes}` : ''}`;
  }
  function tabState(tab: TabView): string {
    return workspace.sessions.find((s) => s.id === tab.focus)?.view.state ?? '';
  }
  const TAB_STATE: Record<string, string> = {
    starting: msg('starting'),
    running: msg('running'),
    closing: msg('closing'),
    exited: msg('exited'),
    closed: msg('closed'),
    failed: msg('failed'),
  };
  function tabStatus(tab: TabView): string {
    const state = tabState(tab);
    return TAB_STATE[state] ? t(TAB_STATE[state]) : state;
  }
  function tabNative(tab: TabView): boolean {
    return !!workspace.sessions.find((s) => s.id === tab.focus)?.native;
  }
  /** A divider is dragged: the split takes the share the pointer is at. */
  function drag(
    event: PointerEvent,
    divider: { path: string; dir: 'row' | 'col'; area: Rect },
  ) {
    if (!area || !activeTab || !manager) return;
    event.preventDefault();
    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture(event.pointerId);
    const box = area.getBoundingClientRect();
    const tabId = activeTab.id;
    const move = (e: PointerEvent) => {
      const ratio =
        divider.dir === 'row'
          ? ((e.clientX - box.left) / box.width - divider.area.x) /
            divider.area.w
          : ((e.clientY - box.top) / box.height - divider.area.y) /
            divider.area.h;
      manager?.resizeSplit(tabId, divider.path, ratio);
    };
    const up = () => {
      target.removeEventListener('pointermove', move);
      target.removeEventListener('pointerup', up);
      target.removeEventListener('pointercancel', up);
    };
    target.addEventListener('pointermove', move);
    target.addEventListener('pointerup', up);
    target.addEventListener('pointercancel', up);
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={onWindowKey} />

<section
  class="terminal-feature"
  aria-label={t('Local terminal workspace')}
  hidden={!visible}
  inert={!visible}
  onkeydowncapture={chord}
>
  <div class="workspace-toolbar">
    <div
      bind:this={strip}
      class="terminal-tabs"
      role="tablist"
      aria-label={t('Terminal sessions')}
    >
      {#each workspace.tabs as tab (tab.id)}
        <div class="terminal-tab">
          <button
            role="tab"
            id={`tab-${tab.id}`}
            aria-controls={`panel-${tab.focus}`}
            aria-selected={workspace.activeTab === tab.id}
            disabled={tabState(tab) === 'closing'}
            onclick={() => manager?.activateTab(tab.id)}
            onkeydown={(event) => tabKey(event, tab.id)}
          >
            {#if tabNative(tab)}<span class="tab-agent" aria-hidden="true"
                >◆</span
              >{/if}{tabTitle(tab)}{#if tabState(tab) !== 'running'}&#32;· {tabStatus(
                tab,
              )}{/if}
          </button>
          <button
            class="tab-close"
            aria-label={t('Close {name}', { name: tabTitle(tab) })}
            disabled={tabState(tab) === 'closing'}
            onclick={() => void manager?.closeTab(tab.id)}>×</button
          >
        </div>
      {/each}
    </div>
    <div class="top-actions" role="group" aria-label={t('Terminal tools')}>
      <button
        type="button"
        class="top-action icon"
        disabled={!activeId}
        title={t('Split side by side (Ctrl+Shift+D)')}
        aria-label={t('Split side by side')}
        onclick={() => split('row')}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"
          ><path d="M4 5h16v14H4zM12 5v14" /></svg
        >
      </button>
      <button
        type="button"
        class="top-action icon"
        disabled={!activeId}
        title={t('Split, one above the other (Ctrl+Shift+E)')}
        aria-label={t('Split one above the other')}
        onclick={() => split('col')}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"
          ><path d="M4 5h16v14H4zM4 12h16" /></svg
        >
      </button>
      <button
        type="button"
        class="top-action icon legend-trigger"
        aria-expanded={legend}
        aria-label={t('Keyboard shortcuts')}
        title={t('Keyboard shortcuts')}
        onclick={() => (legend = !legend)}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"
          ><path d="M3 7h18v10H3zM7 11h.01M11 11h.01M15 11h.01M7 14h10" /></svg
        >
      </button>
      <span class="toolbar-sep" aria-hidden="true"></span>
      <button
        type="button"
        class="top-action launch-trigger"
        aria-expanded={launchesOpen}
        aria-haspopup="true"
        aria-controls="launch-panel"
        title={t('Your saved launches')}
        onclick={() => void toggleLaunches()}>+ {t('Launch')}</button
      >
    </div>
  </div>
  {#if launchesOpen}
    <div id="launch-panel" class="launch-panel" aria-label={t('Launches')}>
      {#if launches === null && !launchError}
        <p class="launch-empty">{t('Reading launches…')}</p>
      {:else if launches}
        {#if ordered.length === 0}
          <p class="launch-empty">
            {t('No launches. Add a file in {folder}.', {
              folder: launches.folder,
            })}
          </p>
        {:else}
          <ul class="launch-list">
            {#each ordered as launch, index (launch.id)}
              {@const full = isFullAccess(launch, launchProfiles)}
              <li class="launch-row">
                <button
                  class="launch-item"
                  disabled={launchBusy !== null || reordering}
                  aria-busy={launchBusy === launch.id}
                  title={launch.kind === 'commands'
                    ? launch.commands.join(' ; ')
                    : `${launch.agent === 'claude_code' ? 'Claude Code' : 'Codex'}${launch.profile ? ` · ${launch.profile}` : ''} in ${launch.directory}`}
                  onclick={() => void open(launch.id)}
                >
                  <span class="launch-name"
                    >{launchBusy === launch.id
                      ? `${launch.name}…`
                      : launch.name}</span
                  >
                  <span class="launch-kind"
                    >{launch.kind === 'commands'
                      ? t('Terminal')
                      : launch.agent === 'claude_code'
                        ? 'Claude Code'
                        : 'Codex'}</span
                  >
                  {#if estimates && estimateLaunch(launch, estimates)}
                    <span class="launch-tokens"
                      >~{kTokens(
                        estimateLaunch(launch, estimates)!.total,
                      )}</span
                    >
                  {/if}
                  {#if full}
                    <span
                      class="badge"
                      data-tone="warn"
                      title={t(BYPASS_WARNING)}>{t('Full access')}</span
                    >
                  {/if}
                </button>
                {#if reordering}
                  <button
                    class="launch-move"
                    aria-label={t('Move {name} up', { name: launch.name })}
                    disabled={index === 0}
                    onclick={() => shift(launch.id, -1)}>▲</button
                  >
                  <button
                    class="launch-move"
                    aria-label={t('Move {name} down', { name: launch.name })}
                    disabled={index === ordered.length - 1}
                    onclick={() => shift(launch.id, 1)}>▼</button
                  >
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
        {#each launches.refused as bad (bad.id)}
          <p class="launch-refused">
            <code>{bad.id}.toml</code>: {bad.error}
          </p>
        {/each}
        <div class="launch-foot">
          <span class="launch-folder"
            >{t('Files in {folder}', { folder: launches.folder })}</span
          >
          {#if ordered.length > 1}
            <button
              class="launch-order"
              aria-pressed={reordering}
              onclick={() => (reordering = !reordering)}
              >{reordering ? t('Done') : t('Order')}</button
            >
          {/if}
        </div>
      {/if}
    </div>
  {/if}
  {#if legend}
    <dl class="terminal-legend" aria-label={t('Keyboard shortcuts')}>
      {#each KEY_LEGEND as [chord_, what] (chord_)}
        <dt>{chord_}</dt>
        <dd>{t(what)}</dd>
      {/each}
      <dd class="terminal-legend-note">
        {t('Every other key goes to the program in the terminal.')}
      </dd>
    </dl>
  {/if}
  {#if launchError}<p role="alert" class="workspace-error">
      {launchError.translatable
        ? t(launchError.text, launchError.values)
        : launchError.text}
    </p>{/if}
  {#if launchNote}<p role="status" class="launch-note">
      {#each launchNote as note, index (index)}{index > 0 ? ' ' : ''}{t(
          note.text,
          note.values,
        )}{/each}
    </p>{/if}
  {#if workspace.error}<p role="alert" class="workspace-error">
      {workspace.error}
    </p>{/if}
  <div class="pane-area" bind:this={area}>
    {#if manager}
      {#each workspace.sessions as session (session.id)}
        {@const tab = tabOf(session.id)}
        <TerminalPanel
          {session}
          tabId={tab?.id ?? ''}
          visible={!!tab && tab.id === workspace.activeTab}
          focused={workspace.active === session.id}
          place={(tab && tab.id === workspace.activeTab
            ? arrangement?.panes.get(session.id)
            : undefined) ?? WHOLE}
          searching={searchFor === session.id}
          {manager}
          onsearchclose={() => (searchFor = undefined)}
        />
      {/each}
    {/if}
    {#if arrangement}
      {#each arrangement.dividers as divider (divider.path)}
        <div
          class="pane-divider"
          class:row={divider.dir === 'row'}
          role="separator"
          aria-orientation={divider.dir === 'row' ? 'vertical' : 'horizontal'}
          style:left={divider.dir === 'row'
            ? `${(divider.area.x + divider.area.w * divider.ratio) * 100}%`
            : `${divider.area.x * 100}%`}
          style:top={divider.dir === 'row'
            ? `${divider.area.y * 100}%`
            : `${(divider.area.y + divider.area.h * divider.ratio) * 100}%`}
          style:width={divider.dir === 'row'
            ? undefined
            : `${divider.area.w * 100}%`}
          style:height={divider.dir === 'row'
            ? `${divider.area.h * 100}%`
            : undefined}
          onpointerdown={(event) => drag(event, divider)}
        ></div>
      {/each}
    {/if}
  </div>
  {#if workspace.sessions.length === 0}<div class="terminal-empty">
      <p>{t('No terminals open.')}</p>
      <button
        type="button"
        class="action"
        disabled={workspace.creating || !manager}
        onclick={() => void manager?.create()}
        >{t('New terminal')} <kbd>Ctrl+Shift+T</kbd></button
      >
    </div>{/if}
  <SessionBar
    session={focused}
    {visible}
    read={terminalGitStatus}
    profiles={readProfiles}
  />
</section>
