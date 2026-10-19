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
  let {
    onworkspace,
    onagents,
    visible = true,
  }: {
    onworkspace?: (state: WorkspaceState) => void;
    /** Opens the place where an agent is started. */
    onagents?: () => void;
    /** False while another view is shown. The terminals keep running. */
    visible?: boolean;
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
  let legend = $state(false);
  const activeId = $derived(workspace.active);
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
    return `${focus?.title ?? 'Terminal'}${panes > 1 ? ` ▦ ${panes}` : ''}`;
  }
  function tabState(tab: TabView): string {
    return workspace.sessions.find((s) => s.id === tab.focus)?.view.state ?? '';
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

<section
  class="terminal-feature"
  aria-label="Local terminal workspace"
  hidden={!visible}
  inert={!visible}
  onkeydowncapture={chord}
>
  <div class="workspace-toolbar">
    <div
      bind:this={strip}
      class="terminal-tabs"
      role="tablist"
      aria-label="Terminal sessions"
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
              >{/if}{tabTitle(tab)}{#if tabState(tab) !== 'running'}&#32;· {tabState(
                tab,
              )}{/if}
          </button>
          <button
            class="tab-close"
            aria-label={`Close ${tabTitle(tab)}`}
            disabled={tabState(tab) === 'closing'}
            onclick={() => void manager?.closeTab(tab.id)}>×</button
          >
        </div>
      {/each}
    </div>
    {#if onagents}
      <button class="new-terminal" onclick={() => onagents?.()}
        >Start agent…</button
      >
    {/if}
    <button
      class="new-terminal"
      disabled={!activeId}
      title="Split side by side (Ctrl+Shift+D)"
      aria-label="Split side by side"
      onclick={() => activeId && manager?.split(activeId, 'row')}>▥</button
    >
    <button
      class="new-terminal"
      disabled={!activeId}
      title="Split, one above the other (Ctrl+Shift+E)"
      aria-label="Split one above the other"
      onclick={() => activeId && manager?.split(activeId, 'col')}>▤</button
    >
    <button
      class="new-terminal"
      disabled={workspace.creating}
      onclick={() => manager?.create()}>New Terminal</button
    >
    <button
      class="new-terminal"
      aria-expanded={legend}
      aria-label="Keyboard shortcuts"
      onclick={() => (legend = !legend)}>⌨</button
    >
  </div>
  {#if legend}
    <dl class="terminal-legend" aria-label="Keyboard shortcuts">
      {#each KEY_LEGEND as [chord_, what] (chord_)}
        <dt>{chord_}</dt>
        <dd>{what}</dd>
      {/each}
      <dd class="terminal-legend-note">
        Every other key goes to the program in the terminal.
      </dd>
    </dl>
  {/if}
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
      No terminals open. Create a new terminal to continue.
    </div>{/if}
</section>
