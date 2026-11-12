<script lang="ts">
  import type { SyncNote, SyncStatus } from '../../contracts/arsenal';
  import { msg, t } from '../../i18n/index.svelte';
  import { ago, syncSummary } from './sync-present';

  let {
    sync,
    onsyncnow,
  }: {
    sync: SyncStatus;
    onsyncnow: () => void;
  } = $props();

  const now = () => Math.floor(Date.now() / 1000);
  const summary = $derived(syncSummary(sync, now()));
  const notes: { title: string; items: SyncNote[] }[] = $derived(
    [
      { title: msg('Waiting'), items: sync.pending },
      { title: msg('Conflicts kept both versions'), items: sync.conflicts },
      { title: msg('Refused'), items: sync.rejected },
      { title: msg('Deleted on another computer'), items: sync.removed },
    ].filter((group) => group.items.length > 0),
  );
</script>

<section
  class="arsenal-sync stripe"
  data-tone={summary.tone}
  aria-label={t('Synchronization')}
>
  <p class="arsenal-sync-line" role="status" aria-live="polite">
    <span class="badge" data-tone={summary.tone}
      ><span aria-hidden="true">{summary.symbol}</span> {summary.label}</span
    >
    <span class="arsenal-sync-text">{summary.text}</span>
  </p>
  <details class="arsenal-sync-details">
    <summary>{t('Synchronization details')}</summary>
    <dl class="sync-facts">
      <dt>{t('This computer')}</dt>
      <dd>
        {t('{host} · {count} change(s) published', {
          host: sync.host,
          count: sync.published,
        })}
      </dd>
      <dt>{t('Shared folder')}</dt>
      <dd><code>{sync.folder}</code> {t('(Syncthing carries it)')}</dd>
      <dt>{t('Last check')}</dt>
      <dd>
        {sync.last_success ? ago(sync.last_success, now()) : t('not yet')}
      </dd>
      {#each sync.peers as peer (peer.device)}
        <dt>{peer.host}</dt>
        <dd>
          {t('applied {applied} of {published} change(s) from here', {
            applied: peer.applied_mine,
            published: peer.published_mine,
          })}
          {#if peer.last_heard}· {t('last heard {time}', {
              time: ago(peer.last_heard, now()),
            })}{/if}
        </dd>
      {/each}
    </dl>
    {#each notes as group (group.title)}
      <h4>{t(group.title)}</h4>
      <ul class="arsenal-sync-notes">
        {#each group.items as item (item.at + item.kind + item.name + item.detail)}
          <li>
            <strong>{item.name}</strong>
            <span class="sync-muted">({item.kind}, {ago(item.at, now())})</span>
            — {item.detail}
          </li>
        {/each}
      </ul>
    {/each}
    <button class="action" type="button" onclick={onsyncnow}
      >{t('Check now')}</button
    >
  </details>
</section>

<style>
  .arsenal-sync {
    display: grid;
    gap: 6px;
    padding: 8px 12px 8px 16px;
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background: var(--surface);
  }
  .arsenal-sync-line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 10px;
    font-size: var(--text-sm);
  }
  .arsenal-sync-text {
    color: var(--muted);
  }
  .arsenal-sync-details summary {
    width: fit-content;
    font-size: var(--text-sm);
    font-weight: 500;
  }
  .arsenal-sync-details {
    display: grid;
    gap: 8px;
  }
  .arsenal-sync-details[open] {
    padding-bottom: 4px;
  }
  .arsenal-sync-details h4 {
    margin: 6px 0 0;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .arsenal-sync-notes {
    margin: 0;
    padding-left: 18px;
    display: grid;
    gap: 4px;
    font-size: var(--text-sm);
    overflow-wrap: anywhere;
  }
  .arsenal-sync-details .action {
    justify-self: start;
    min-height: 30px;
    font-size: var(--text-sm);
  }
  .sync-facts {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: 4px 14px;
    margin: 0;
    font-size: var(--text-sm);
  }
  .sync-facts dt {
    color: var(--muted);
  }
  .sync-facts dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .sync-muted {
    color: var(--muted);
  }
  .arsenal-sync-details code {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }
</style>
