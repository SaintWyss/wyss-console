<script lang="ts">
  import { t } from '../i18n/index.svelte';
  import type { LifecycleNotice } from './lifecycle';

  let {
    notice,
    retrying,
    onretry,
  }: {
    notice: LifecycleNotice;
    /** A Retry was asked for and has not been answered yet. */
    retrying: boolean;
    onretry: () => void;
  } = $props();
</script>

<!-- Why the authority is not available, and the one thing to do about it. It sits above
     every view and takes none of them away: the terminal works with or without it. -->
<section
  class="authority-notice"
  data-tone={notice.tone}
  aria-label={t('Authority status')}
>
  <div class="authority-notice-main" role="status">
    <p class="authority-notice-title">{t(notice.title)}</p>
    <p class="authority-notice-text">{t(notice.text)}</p>
  </div>
  {#if notice.retry}
    <button type="button" class="action" disabled={retrying} onclick={onretry}
      >{retrying ? t('Retrying…') : t('Retry')}</button
    >
  {/if}
  <details class="authority-notice-details">
    <summary>{t('Details')}</summary>
    <dl>
      {#each notice.details as detail (detail.term)}
        <dt>{t(detail.term)}</dt>
        <dd>{t(detail.value)}</dd>
      {/each}
    </dl>
  </details>
</section>
