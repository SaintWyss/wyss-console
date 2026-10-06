<script lang="ts">
  import { onMount } from 'svelte';
  import { getAppInfo } from '../../desktop/client';
  import type { AppInfo } from '../../contracts/app-info';

  let info = $state<AppInfo | null>(null);
  let failed = $state(false);

  onMount(() => {
    let active = true;
    void getAppInfo().then(
      (result) => {
        if (active) info = result;
      },
      () => {
        if (active) failed = true;
      },
    );
    return () => {
      active = false;
    };
  });
</script>

{#if failed}
  <p role="alert">
    Unable to load application information. Restart Wyss Console to try again.
  </p>
{:else if info}
  <p role="status">Foundation ready</p>
  <dl aria-label="Application information from IPC">
    <dt>Name</dt>
    <dd>{info.name}</dd>
    <dt>Version</dt>
    <dd>{info.version}</dd>
    <dt>Schema</dt>
    <dd>{info.schema_version}</dd>
  </dl>
{:else}
  <p role="status">Loading application information…</p>
{/if}
