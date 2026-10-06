<script lang="ts">
  import { onMount } from 'svelte';
  import { getAppInfo } from './lib/desktop/client';
  import TerminalView from './lib/features/terminal/TerminalView.svelte';
  let version = $state('');
  onMount(() => {
    let active = true;
    void getAppInfo().then(
      (info) => {
        if (active) version = info.version;
      },
      () => undefined,
    );
    return () => {
      active = false;
    };
  });
</script>

<main>
  <header class="app-header">
    <h1>Wyss Console</h1>
    <span>{version}</span>
  </header>
  <TerminalView />
</main>
