<script lang="ts">
  import StatusList from './StatusList.svelte';
  import type { AuthorityObservation } from './authority';
  import type { LifecycleObservation } from './lifecycle';
  import type { RuntimeSummary, RuntimeTone } from './runtime';
  let {
    hidden = false,
    runtime,
    authority,
    lifecycle,
    f8,
  }: {
    hidden?: boolean;
    runtime: RuntimeSummary;
    authority: AuthorityObservation;
    lifecycle: LifecycleObservation;
    f8: { label: string; tone: RuntimeTone; note: string | null } | null;
  } = $props();
</script>

<!-- Terminal, agent and authority, on every view but the Console (which has its session bar). -->
<footer class="status-bar" {hidden}>
  <StatusList {runtime} {authority} {lifecycle} {f8} />
</footer>
