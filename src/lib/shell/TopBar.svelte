<script lang="ts">
  import { summarizeAgent, type AuthorityObservation } from './authority';
  import { summarizeLink, type LifecycleObservation } from './lifecycle';
  import type { RuntimeSummary } from './runtime';

  let {
    title,
    subtitle,
    runtime,
    authority,
    lifecycle,
    version,
  }: {
    title: string;
    subtitle: string;
    /** The local human terminal. It never depends on the authority. */
    runtime: RuntimeSummary;
    authority: AuthorityObservation;
    /** What is being done to make the authority available, while it is not. */
    lifecycle: LifecycleObservation;
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
    <h1>Wyss AI</h1>
  </div>
  <div class="context">
    <p class="context-title">{title}</p>
    <p class="context-subtitle">{subtitle}</p>
  </div>
  <div class="status-cluster">
    <span
      class="pill terminal"
      data-tone={runtime.tone}
      title="Local terminal. It runs with or without the authority."
    >
      <span class="pill-key">Terminal</span>
      <span class="dot" aria-hidden="true"></span>
      {runtime.label}
    </span>
    <span class="pill agent" data-tone={agent.tone} title={agent.detail}>
      <span class="pill-key">{agent.key}</span>
      <span class="dot" aria-hidden="true"></span>
      {agent.label}
    </span>
    <span class="pill authority" data-tone={link.tone} title={link.detail}>
      <span class="pill-key">Authority</span>
      <span class="dot" aria-hidden="true"></span>
      {link.label}
      {#if link.note}<span class="pill-note">{link.note}</span>{/if}
    </span>
    {#if version}<span class="version">v{version}</span>{/if}
  </div>
</header>
