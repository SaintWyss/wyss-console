<script lang="ts">
  import {
    cancelDelegation,
    readDelegations,
    type Delegation,
  } from '../../desktop/native-client';
  import {
    delegationGroups,
    delegationMark,
    delegationTally,
  } from './delegations';
  import { t } from '../../i18n/index.svelte';
  import { msg } from '../../i18n/msg';

  /** Agents that orchestrator sessions delegated work to, read from the terminal host. */
  let {
    visible,
    read = readDelegations,
    cancel = cancelDelegation,
  }: {
    visible: boolean;
    read?: () => Promise<Delegation[]>;
    cancel?: (id: string) => Promise<void>;
  } = $props();

  let delegations = $state<Delegation[]>([]);
  let failed = $state(false);
  /** The running agent whose stop waits for a second click. */
  let confirming = $state<string | null>(null);
  let stopping = $state<string | null>(null);

  const PROVIDER: Record<string, string> = {
    codex: 'Codex',
    claude_code: 'Claude Code',
  };
  const STATUS: Record<string, string> = {
    running: msg('Running'),
    succeeded: msg('Done'),
    failed: msg('Failed'),
    cancelled: msg('Cancelled'),
    timed_out: msg('Timed out'),
    interrupted: msg('Interrupted'),
  };
  const CHECK: Record<string, string> = {
    verified: msg('configuration verified'),
    mismatch: msg('configuration did not match'),
    pending: msg('configuration not checked yet'),
    unavailable: msg('configuration not verifiable'),
  };

  const groups = $derived(delegationGroups(delegations));
  const tally = $derived(delegationTally(delegations));

  async function refresh() {
    try {
      delegations = await read();
      failed = false;
    } catch {
      failed = true;
    }
  }

  $effect(() => {
    if (!visible) return;
    void refresh();
    const timer = setInterval(() => void refresh(), 3000);
    return () => clearInterval(timer);
  });

  async function stop(id: string) {
    confirming = null;
    stopping = id;
    try {
      await cancel(id);
    } finally {
      stopping = null;
      await refresh();
    }
  }

  const when = (seconds: number) =>
    new Date(seconds * 1000).toLocaleTimeString(undefined, {
      timeStyle: 'short',
    });
</script>

<section aria-label={t('Delegated agents')} class="delegations">
  <div class="delegations-head">
    <h3 class="native-heading">{t('Delegated agents')}</h3>
    {#if delegations.length > 0}
      <p class="delegations-tally" aria-live="polite">
        {#each tally as entry (entry.label)}
          <span class="badge" data-tone={entry.tone}
            ><span aria-hidden="true">{entry.symbol}</span>
            {entry.count}
            {t(entry.label)}</span
          >
        {/each}
      </p>
    {/if}
  </div>
  {#if failed}
    <p class="native-warning" role="alert">
      {t('The terminal host did not list delegated agents.')}
    </p>
  {:else if delegations.length === 0}
    <p class="delegations-empty">
      {t(
        'None yet. A Claude Code or Codex session started with delegation on can give a task to the other one.',
      )}
    </p>
  {/if}
  {#each groups as group (group.key)}
    <div class="delegation-group">
      <p class="delegation-parent">
        <span class="mono"
          >{t('From {provider} session {id}', {
            provider: PROVIDER[group.provider] ?? group.provider,
            id: group.parent.slice(0, 8),
          })}</span
        >
        <span class="delegation-parent-count"
          >{group.items.length === 1
            ? t('{count} agent', { count: group.items.length })
            : t('{count} agents', { count: group.items.length })}</span
        >
      </p>
      <ul class="delegation-list">
        {#each group.items as d (d.id)}
          {@const mark = delegationMark(d.status)}
          <li class="delegation stripe" data-tone={mark.tone}>
            <div class="delegation-line">
              <strong class="delegation-title">{d.title}</strong>
              <span class="badge" data-tone={mark.tone}
                ><span aria-hidden="true">{mark.symbol}</span>
                {STATUS[d.status] ? t(STATUS[d.status]) : d.status}</span
              >
            </div>
            <p class="delegation-route">
              <span class="delegation-provider"
                >{PROVIDER[d.provider] ?? d.provider}</span
              >
              <span class="mono">{d.model} · {d.effort}</span>
            </p>
            <dl class="delegation-facts">
              <dt>{t('Time')}</dt>
              <dd>
                {when(d.created_at)}{d.ended_at
                  ? ` – ${when(d.ended_at)}`
                  : ` · ${t('running')}`} · {t('{n} completed step(s)', {
                  n: d.steps,
                })}
              </dd>
              <dt>{t('Check')}</dt>
              <dd>
                {CHECK[d.verification.state]
                  ? t(CHECK[d.verification.state])
                  : d.verification.state}
                {#if d.verification.model}
                  · {t('Observed:')}
                  {d.verification.model} · {d.verification.effort ??
                    t('effort not reported')}
                {/if}
              </dd>
              {#if d.preset || d.selection}
                <dt>{t('Arsenal')}</dt>
                <dd>
                  {#if d.preset}{t('Preset: {name}', { name: d.preset })}{/if}
                  {#if d.preset && d.selection}
                    ·
                  {/if}
                  {#if d.selection}{t('Selection: {name}', {
                      name: d.selection,
                    })}{/if}
                </dd>
              {/if}
              <dt>{t('Skills')}</dt>
              <dd>
                {d.applied.skills.length > 0
                  ? d.applied.skills
                      .map((s) => `${s.name} r${s.revision ?? '?'}`)
                      .join(', ')
                  : t('none of your own')} · {t(
                  '{n} installed skill(s) switched off',
                  { n: d.applied.switched_off },
                )}
              </dd>
              {#if d.artifacts.length > 0}
                <dt>{t('Files')}</dt>
                <dd>
                  {d.artifacts.map((a) => a.path).join(', ')}
                  {t('in')}
                  <code>{d.workspace}</code>
                </dd>
              {/if}
            </dl>
            {#if d.verification.problems.length > 0}
              <p class="native-warning">
                {d.verification.problems.join('; ')}
              </p>
            {/if}
            {#if d.error}<p class="native-warning">{d.error}</p>{/if}
            {#if d.result}
              <details class="delegation-details">
                <summary>{t('Result')}</summary>
                <pre class="delegation-result">{d.result}</pre>
              </details>
            {/if}
            {#if d.status === 'running'}
              <div class="delegation-actions">
                {#if confirming === d.id}
                  <span class="delegation-confirm" role="status"
                    >{t('Stop this agent? Its unfinished work is lost.')}</span
                  >
                  <button
                    type="button"
                    class="action danger"
                    onclick={() => stop(d.id)}>{t('Stop agent')}</button
                  >
                  <button
                    type="button"
                    class="action"
                    onclick={() => (confirming = null)}
                    >{t('Keep running')}</button
                  >
                {:else}
                  <button
                    type="button"
                    class="action"
                    disabled={stopping === d.id}
                    onclick={() => (confirming = d.id)}
                    >{stopping === d.id ? t('Stopping…') : t('Cancel')}</button
                  >
                {/if}
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    </div>
  {/each}
</section>

<style>
  .delegations {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
  }
  .delegations-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 6px 12px;
  }
  .delegations-head .native-heading {
    margin: 0;
  }
  .delegations-tally {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .delegations-empty {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .delegation-group {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  /* The parent, then its children hanging from one line: who delegated what. */
  .delegation-parent {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: 4px 10px;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .delegation-parent-count {
    color: var(--faint);
  }
  .delegation-list {
    list-style: none;
    margin: 0 0 0 6px;
    padding: 0 0 0 12px;
    border-left: 1px solid var(--line-strong);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .delegation {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
    padding: 12px 14px 12px 16px;
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background: var(--surface);
  }
  .delegation-line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 6px 10px;
  }
  .delegation-title {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .delegation-route {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 10px;
    font-size: var(--text-sm);
  }
  .delegation-provider {
    font-weight: 600;
    color: var(--accent-strong);
  }
  .delegation-route .mono {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .delegation-facts {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: 3px 12px;
    margin: 0;
    font-size: var(--text-xs);
  }
  .delegation-facts dt {
    color: var(--faint);
  }
  .delegation-facts dd {
    margin: 0;
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .delegation-details summary {
    width: fit-content;
    font-size: var(--text-sm);
    font-weight: 500;
  }
  .delegation-result {
    margin: 6px 0 0;
    padding: 10px 12px;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 240px;
    overflow-y: auto;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    background: var(--terminal-bg);
    color: var(--terminal-fg);
  }
  .delegation-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 8px;
  }
  .delegation-actions .action {
    min-height: 30px;
    padding: 0 12px;
    font-size: var(--text-sm);
  }
  .delegation-confirm {
    flex: 1 1 100%;
    font-size: var(--text-sm);
    color: var(--warn);
  }
</style>
