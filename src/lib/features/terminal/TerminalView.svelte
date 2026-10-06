<script lang="ts">
  import { onMount } from 'svelte';
  import { mountTerminal } from './mount';
  import type { TerminalViewState } from './controller';

  let host: HTMLDivElement;
  let view = $state<TerminalViewState>({ state: 'starting' });
  let lease: ReturnType<typeof mountTerminal> | undefined;
  onMount(() => {
    try {
      lease = mountTerminal(host, (state) => {
        view = state;
      });
    } catch {
      view = { state: 'failed', error: 'renderer_error' };
    }
    return () => {
      void lease?.dispose();
    };
  });
</script>

<section class="terminal-feature" aria-label="Local terminal">
  <div class="terminal-toolbar">
    <p role="status" aria-live="polite">
      {#if view.state === 'starting'}Starting terminal…
      {:else if view.state === 'running'}{view.info?.shell} · Running
      {:else if view.state === 'exited'}Shell exited{view.info?.exit
          ? ` (${view.info.exit.code})`
          : ''}
      {:else if view.state === 'closing'}Closing terminal…
      {:else if view.state === 'closed'}Terminal closed
      {:else}Terminal failed: {view.error ?? 'terminal_error'}{/if}
      {#if view.truncated}
        · Output truncated{/if}
    </p>
    <button
      onclick={() => void lease?.close()}
      disabled={view.state === 'starting' ||
        view.state === 'closing' ||
        view.state === 'closed'}>Close terminal</button
    >
  </div>
  <div bind:this={host} class="terminal-host"></div>
</section>
