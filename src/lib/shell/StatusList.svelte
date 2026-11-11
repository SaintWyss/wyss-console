<script lang="ts">
  import { t } from '../i18n/index.svelte';
  import { summarizeAgent, type AuthorityObservation } from './authority';
  import { summarizeLink, type LifecycleObservation } from './lifecycle';
  import type { RuntimeSummary, RuntimeTone } from './runtime';

  let {
    runtime,
    authority,
    lifecycle,
    f8,
  }: {
    /** The local human terminal. It never depends on the authority. */
    runtime: RuntimeSummary;
    authority: AuthorityObservation;
    /** What is being done to make the authority available, while it is not. */
    lifecycle: LifecycleObservation;
    /** The protocol-2 link, when this desktop is started against it. It replaces the pill. */
    f8: { label: string; tone: RuntimeTone; note: string | null } | null;
  } = $props();
  const agent = $derived(summarizeAgent(authority));
  const link = $derived(summarizeLink(authority, lifecycle));
</script>

<!-- One status per line. Collapsed, only the dots stay; the words are read out and in the tooltip. -->
<div class="status-cluster" role="group" aria-label={t('Status')}>
  <span
    class="pill terminal"
    data-tone={runtime.tone}
    title={`${t(runtime.label)} · ${t('Local terminal. It runs with or without the authority.')}`}
  >
    <span class="pill-key">{t('Terminal')}</span>
    <span class="dot" aria-hidden="true"></span>
    <span class="pill-value">{t(runtime.label)}</span>
  </span>
  <span
    class="pill agent"
    data-tone={agent.tone}
    title={`${t(agent.label)} · ${agent.detail}`}
  >
    <span class="pill-key">{agent.key}</span>
    <span class="dot" aria-hidden="true"></span>
    <span class="pill-value">{t(agent.label)}</span>
  </span>
  {#if f8}
    <span
      class="pill authority"
      data-tone={f8.tone}
      title={`${t('Protocol 2 authority')} · ${t(f8.label)}${f8.note ? ` · ${f8.note}` : ''}`}
    >
      <span class="pill-key">{t('Authority P2')}</span>
      <span class="dot" aria-hidden="true"></span>
      <span class="pill-value">{t(f8.label)}</span>
      {#if f8.note}<span class="pill-note">{f8.note}</span>{/if}
    </span>
  {:else}
    <span
      class="pill authority"
      data-tone={link.tone}
      title={`${t('Protocol 1 authority')} · ${t(link.label)} · ${link.detail}`}
    >
      <span class="pill-key">{t('Authority P1')}</span>
      <span class="dot" aria-hidden="true"></span>
      <span class="pill-value">{t(link.label)}</span>
      {#if link.note}<span class="pill-note">{link.note}</span>{/if}
    </span>
  {/if}
</div>
