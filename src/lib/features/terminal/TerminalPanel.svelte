<script lang="ts">
  import { onMount } from 'svelte';
  import type {
    TerminalSession,
    TerminalSessionManager,
  } from './session-manager';
  let {
    session,
    active,
    manager,
  }: {
    session: TerminalSession;
    active: boolean;
    manager: TerminalSessionManager;
  } = $props();
  let host: HTMLDivElement;
  onMount(() => {
    manager.attach(session.id, host);
  });
</script>

<div
  tabindex="0"
  class="terminal-panel"
  role="tabpanel"
  id={`panel-${session.id}`}
  aria-labelledby={`tab-${session.id}`}
  hidden={!active}
  inert={!active}
>
  <div class="terminal-toolbar">
    <p role="status" aria-live={active ? 'polite' : 'off'}>
      {#if session.view.state === 'starting'}Starting terminal…
      {:else if session.view.state === 'running'}{session.view.info?.shell} · Running
      {:else if session.view.state === 'exited'}Shell exited{session.view.info
          ?.exit
          ? ` (${session.view.info.exit.code})`
          : ''}
      {:else if session.view.state === 'closing'}Closing terminal…
      {:else if session.view.state === 'closed'}Terminal closed
      {:else}Terminal failed: {session.view.error ?? 'terminal_error'}{/if}
      {#if session.view.truncated}
        · Output truncated{/if}
    </p>
  </div>
  <div class="terminal-surface">
    <div bind:this={host} class="terminal-host"></div>
  </div>
</div>
