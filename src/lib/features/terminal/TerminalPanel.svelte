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
      {#if session.native}
        <span
          class="pill compact"
          data-tone="idle"
          title="Started by the Console in your shell. Not mediated, contained or approved by the Console."
          ><span class="dot" aria-hidden="true"></span>Native</span
        >
        {session.native.agent}{session.native.profile
          ? ` · ${session.native.profile}`
          : ''} ·
      {/if}
      {#if session.view.state === 'starting'}Starting {session.native
          ? 'agent'
          : 'terminal'}…
      {:else if session.view.state === 'running'}{session.native
          ? 'Running'
          : `${session.view.info?.shell} · Running`}
      {:else if session.view.state === 'exited'}{session.native
          ? 'Agent'
          : 'Shell'} exited{session.view.info?.exit
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
