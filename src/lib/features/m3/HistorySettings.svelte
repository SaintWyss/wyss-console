<script lang="ts">
  import type { HistoryInfo } from '../../contracts/m3';
  import { msg, t } from '../../i18n/index.svelte';
  import { reasonText } from './present';

  let { history }: { history: HistoryInfo } = $props();
  const STATE: Record<string, string> = {
    off: msg('Off'),
    unavailable: msg('Unavailable'),
    recording: msg('Recording'),
    partial: msg('Partial: a storage problem stopped it'),
  };
</script>

<div class="m3-history" data-testid="history-settings">
  <label class="mode-option">
    <input
      type="checkbox"
      checked={history.enabled}
      disabled={!history.can_enable}
      aria-describedby="m3-history-why"
    />
    <span class="mode-name">{t('Keep a local usage history')}</span>
  </label>
  <p class="m3-meta" data-testid="history-state">{t(STATE[history.state])}</p>
  <div id="m3-history-why">
    {#if !history.can_enable}
      <ul class="m3-reasons" aria-label={t('Why it cannot be turned on')}>
        {#each history.reasons as r (r)}<li>{t(reasonText(r))}</li>{/each}
      </ul>
    {/if}
    <p class="m3-meta">
      {t(
        'It is off by default and nothing is stored while it is off. If you turn it on it keeps provider-reported counters and limits only: no prompts, no answers, no reasoning, no commands, no paths and no account names. At most {records} records or {days} days, whichever comes first, and what it removes is counted. It is a separate file; it never touches the audit trail or the vault.',
        {
          records: history.max_records.toLocaleString('en-US'),
          days: history.retention_days,
        },
      )}
    </p>
  </div>
  <div class="m3-actions">
    <button type="button" class="action" disabled={!history.enabled}
      >{t('Export')}</button
    >
    <button type="button" class="action danger" disabled={!history.enabled}
      >{t('Delete the history')}</button
    >
  </div>
</div>
