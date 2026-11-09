<script lang="ts">
  import { summarizeAgent, type AuthorityObservation } from './authority';
  import { summarizeLink, type LifecycleObservation } from './lifecycle';
  import type { RuntimeSummary, RuntimeTone } from './runtime';

  let {
    runtime,
    authority,
    lifecycle,
    f8,
    version,
  }: {
    /** The local human terminal. It never depends on the authority. */
    runtime: RuntimeSummary;
    authority: AuthorityObservation;
    /** What is being done to make the authority available, while it is not. */
    lifecycle: LifecycleObservation;
    /** The protocol-2 link, when this desktop is started against it. It replaces the pill. */
    f8: { label: string; tone: RuntimeTone; note: string | null } | null;
    version: string;
  } = $props();
  const agent = $derived(summarizeAgent(authority));
  const link = $derived(summarizeLink(authority, lifecycle));
</script>

<!-- One status per line. Collapsed, only the dots stay; the words are read out and in the tooltip. -->
<div class="status-cluster" role="group" aria-label="Status">
  <span
    class="pill terminal"
    data-tone={runtime.tone}
    title={`${runtime.label} · Local terminal. It runs with or without the authority.`}
  >
    <span class="pill-key">Terminal</span>
    <span class="dot" aria-hidden="true"></span>
    <span class="pill-value">{runtime.label}</span>
  </span>
  <span
    class="pill agent"
    data-tone={agent.tone}
    title={`${agent.label} · ${agent.detail}`}
  >
    <span class="pill-key">{agent.key}</span>
    <span class="dot" aria-hidden="true"></span>
    <span class="pill-value">{agent.label}</span>
  </span>
  {#if f8}
    <span
      class="pill authority"
      data-tone={f8.tone}
      title={`Protocol 2 authority · ${f8.label}${f8.note ? ` · ${f8.note}` : ''}`}
    >
      <span class="pill-key">Authority P2</span>
      <span class="dot" aria-hidden="true"></span>
      <span class="pill-value">{f8.label}</span>
      {#if f8.note}<span class="pill-note">{f8.note}</span>{/if}
    </span>
  {:else}
    <span
      class="pill authority"
      data-tone={link.tone}
      title={`Protocol 1 authority · ${link.label} · ${link.detail}`}
    >
      <span class="pill-key">Authority P1</span>
      <span class="dot" aria-hidden="true"></span>
      <span class="pill-value">{link.label}</span>
      {#if link.note}<span class="pill-note">{link.note}</span>{/if}
    </span>
  {/if}
  {#if version}<span class="version">v{version}</span>{/if}
</div>
