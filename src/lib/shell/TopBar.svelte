<script lang="ts">
  import type { RuntimeSummary } from './runtime';

  let {
    title,
    subtitle,
    runtime,
    version,
  }: {
    title: string;
    subtitle: string;
    runtime: RuntimeSummary;
    version: string;
  } = $props();
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
    <span class="pill" data-tone={runtime.tone}>
      <span class="pill-key">Runtime</span>
      <span class="dot" aria-hidden="true"></span>
      {runtime.label}
    </span>
    <span
      class="pill authority"
      title="The console does not read authority state yet"
    >
      <span class="pill-key">Authority</span>
      <span class="dot" aria-hidden="true"></span>
      Not linked
    </span>
    {#if version}<span class="version">v{version}</span>{/if}
  </div>
</header>
