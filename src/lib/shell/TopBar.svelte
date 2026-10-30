<script lang="ts">
  import { summarizeAgent, type AuthorityObservation } from './authority';
  import { summarizeLink, type LifecycleObservation } from './lifecycle';
  import type { RuntimeSummary, RuntimeTone } from './runtime';

  let {
    title,
    subtitle,
    runtime,
    authority,
    lifecycle,
    f8,
    version,
  }: {
    title: string;
    subtitle: string;
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

<header class="top-bar">
  <div class="brand">
    <svg class="brand-mark" viewBox="0 0 24 24" aria-hidden="true">
      <defs>
        <linearGradient id="wyss-mark" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stop-color="#5fd4f5" />
          <stop offset="1" stop-color="#a78bfa" />
        </linearGradient>
      </defs>
      <path
        d="M12 2.8l8 4.6v9.2l-8 4.6-8-4.6V7.4z"
        fill="none"
        stroke="url(#wyss-mark)"
        stroke-width="1.6"
        stroke-linejoin="round"
      />
      <path d="M12 8.2l3.2 3.8-3.2 3.8-3.2-3.8z" fill="url(#wyss-mark)" />
    </svg>
    <h1>Wyss Console</h1>
  </div>
  <div class="context">
    <p class="context-title">{title}</p>
    <p class="context-subtitle">{subtitle}</p>
  </div>
  <!-- Every status stays at every width. A narrow window drops the state's word and then
       the status's name from view, never from the tooltip or from a screen reader. -->
  <div class="status-cluster">
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
</header>
