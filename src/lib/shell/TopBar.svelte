<script lang="ts">
  import { t } from '../i18n/index.svelte';
  let {
    title,
    subtitle,
    version,
    tools = false,
    canSplit = false,
    onsplit,
    legend = false,
    ontogglelegend,
    launchOpen = false,
    onlaunch,
  }: {
    title: string;
    subtitle: string;
    version: string;
    /** The Console is shown: its terminal tools are in the bar. */
    tools?: boolean;
    /** A terminal is open, so it can be split. */
    canSplit?: boolean;
    onsplit?: (dir: 'row' | 'col') => void;
    /** The shortcuts list is shown. */
    legend?: boolean;
    ontogglelegend?: () => void;
    launchOpen?: boolean;
    onlaunch?: () => void;
  } = $props();
</script>

<header class="top-bar">
  <!-- The product's name and version stay for screen readers; the window's title bar shows the name. -->
  <div class="visually-hidden">
    <h1>Wyss Console</h1>
    {#if version}<span>v{version}</span>{/if}
  </div>
  <div class="context">
    <p class="context-title">{t(title)}</p>
    <p class="context-subtitle">{t(subtitle)}</p>
  </div>
  {#if tools}
    <div class="top-actions" role="group" aria-label={t('Terminal tools')}>
      <button
        type="button"
        class="top-action icon"
        disabled={!canSplit}
        title={t('Split side by side (Ctrl+Shift+D)')}
        aria-label={t('Split side by side')}
        onclick={() => onsplit?.('row')}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"
          ><path d="M4 5h16v14H4zM12 5v14" /></svg
        >
      </button>
      <button
        type="button"
        class="top-action icon"
        disabled={!canSplit}
        title={t('Split, one above the other (Ctrl+Shift+E)')}
        aria-label={t('Split one above the other')}
        onclick={() => onsplit?.('col')}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"
          ><path d="M4 5h16v14H4zM4 12h16" /></svg
        >
      </button>
      <button
        type="button"
        class="top-action icon"
        aria-expanded={legend}
        aria-label={t('Keyboard shortcuts')}
        title={t('Keyboard shortcuts')}
        onclick={() => ontogglelegend?.()}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"
          ><path d="M3 7h18v10H3zM7 11h.01M11 11h.01M15 11h.01M7 14h10" /></svg
        >
      </button>
      <button
        type="button"
        class="top-action launch-trigger"
        aria-expanded={launchOpen}
        aria-haspopup="true"
        aria-controls="launch-panel"
        title={t('Launches from your files in ~/.config/wyss-console/launch')}
        onclick={() => onlaunch?.()}>{t('Launch')} ▾</button
      >
    </div>
  {/if}
</header>
