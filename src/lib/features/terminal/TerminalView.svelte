<script lang="ts">
  import { onMount } from 'svelte';
  import { mountWorkspace } from './mount';
  import type {
    WorkspaceState,
    TerminalSessionManager,
  } from './session-manager';
  import type { ConsoleSessionId } from '../../contracts/identities';
  import TerminalPanel from './TerminalPanel.svelte';
  let { onworkspace }: { onworkspace?: (state: WorkspaceState) => void } =
    $props();
  let workspace = $state<WorkspaceState>({ sessions: [], creating: false });
  let manager = $state<TerminalSessionManager>();
  let strip: HTMLDivElement;
  const activeId = $derived(workspace.active);
  const count = $derived(workspace.sessions.length);
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
    if (activeId && count) revealActiveTab();
  });
  onMount(() => {
    const observer = new ResizeObserver(revealActiveTab);
    observer.observe(strip);
    const lease = mountWorkspace((state) => {
      workspace = state;
      onworkspace?.(state);
    });
    manager = lease.manager;
    return () => {
      observer.disconnect();
      void lease.dispose();
    };
  });
  function tabKey(event: KeyboardEvent, id: ConsoleSessionId) {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    const sessions = workspace.sessions.filter(
      (s) => s.view.state !== 'closing',
    );
    const index = sessions.findIndex((s) => s.id === id);
    const next =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? sessions.length - 1
          : (index + (event.key === 'ArrowRight' ? 1 : -1) + sessions.length) %
            sessions.length;
    if (sessions[next]) {
      event.preventDefault();
      // Manual activation: arrows move tab focus; Enter/Space select naturally.
      document.getElementById(`tab-${sessions[next].id}`)?.focus();
    }
  }
</script>

<section class="terminal-feature" aria-label="Local terminal workspace">
  <div class="workspace-toolbar">
    <div
      bind:this={strip}
      class="terminal-tabs"
      role="tablist"
      aria-label="Terminal sessions"
    >
      {#each workspace.sessions as session (session.id)}
        <div class="terminal-tab">
          <button
            role="tab"
            id={`tab-${session.id}`}
            aria-controls={`panel-${session.id}`}
            aria-selected={workspace.active === session.id}
            disabled={session.view.state === 'closing'}
            onclick={() => manager?.activate(session.id)}
            onkeydown={(event) => tabKey(event, session.id)}
          >
            {session.title}{#if session.view.state !== 'running'}
              · {session.view.state}{/if}
          </button>
          <button
            class="tab-close"
            aria-label={`Close ${session.title}`}
            disabled={session.view.state === 'closing'}
            onclick={() => void manager?.close(session.id)}>×</button
          >
        </div>
      {/each}
    </div>
    <button
      class="new-terminal"
      disabled={workspace.creating}
      onclick={() => manager?.create()}>New Terminal</button
    >
  </div>
  {#if workspace.error}<p role="alert" class="workspace-error">
      {workspace.error}
    </p>{/if}
  {#if manager}
    {#each workspace.sessions as session (session.id)}
      <TerminalPanel
        {session}
        active={workspace.active === session.id}
        {manager}
      />
    {/each}
  {/if}
  {#if workspace.sessions.length === 0}<div class="terminal-empty">
      No terminals open. Create a new terminal to continue.
    </div>{/if}
</section>
