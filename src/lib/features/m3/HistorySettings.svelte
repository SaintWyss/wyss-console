<script lang="ts">
  import type { HistoryInfo } from '../../contracts/m3';
  import { reasonText } from './present';

  let { history }: { history: HistoryInfo } = $props();
  const STATE: Record<string, string> = {
    off: 'Off',
    unavailable: 'Unavailable',
    recording: 'Recording',
    partial: 'Partial: a storage problem stopped it',
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
    <span class="mode-name">Keep a local usage history</span>
  </label>
  <p class="m3-meta" data-testid="history-state">{STATE[history.state]}</p>
  <div id="m3-history-why">
    {#if !history.can_enable}
      <ul class="m3-reasons" aria-label="Why it cannot be turned on">
        {#each history.reasons as r (r)}<li>{reasonText(r)}</li>{/each}
      </ul>
    {/if}
    <p class="m3-meta">
      It is off by default and nothing is stored while it is off. If you turn it
      on it keeps provider-reported counters and limits only: no prompts, no
      answers, no reasoning, no commands, no paths and no account names. At most
      {history.max_records.toLocaleString('en-US')} records or {history.retention_days}
      days, whichever comes first, and what it removes is counted. It is a separate
      file; it never touches the audit trail or the vault.
    </p>
  </div>
  <div class="m3-actions">
    <button type="button" class="action" disabled={!history.enabled}
      >Export</button
    >
    <button type="button" class="action danger" disabled={!history.enabled}
      >Delete the history</button
    >
  </div>
</div>
