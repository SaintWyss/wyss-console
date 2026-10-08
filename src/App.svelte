<script lang="ts">
  import { onMount } from 'svelte';
  import { getAppInfo } from './lib/desktop/client';
  import TerminalView from './lib/features/terminal/TerminalView.svelte';
  import type { WorkspaceState } from './lib/features/terminal/session-manager';
  import Sidebar from './lib/shell/Sidebar.svelte';
  import TopBar from './lib/shell/TopBar.svelte';
  import { NAV_ITEMS, type ViewId } from './lib/shell/nav';
  import {
    idleRuntime,
    summarizeRuntime,
    type RuntimeSummary,
  } from './lib/shell/runtime';

  let version = $state('');
  let activeView = $state<ViewId>('console');
  let runtime = $state<RuntimeSummary>(idleRuntime);
  const current = $derived(NAV_ITEMS.find((item) => item.id === activeView));

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
    {version}
  />
  <Sidebar active={activeView} onselect={(id) => (activeView = id)} />
  <main class="content">
    <TerminalView onworkspace={observeWorkspace} />
  </main>
</div>
