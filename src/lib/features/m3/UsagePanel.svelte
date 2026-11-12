<script lang="ts">
  import type { ProviderReportInfo } from '../../contracts/m3';
  import { msg, t, tJoin, tPhrase } from '../../i18n/index.svelte';
  import {
    COMPLETENESS_LABEL,
    counter,
    localTime,
    running,
    tokenLine,
    WINDOW_LABEL,
  } from './present';

  let { report }: { report: ProviderReportInfo | null } = $props();
  const usage = $derived(report?.usage ?? null);
  const turns = $derived(
    (usage?.turns.length ?? 0) + (usage?.dropped_turns ?? 0),
  );
  const estimates = $derived(
    (usage?.turns ?? []).filter((t) => t.estimate_usd !== null),
  );
  const ROWS = [
    ['input', msg('Input')],
    ['output', msg('Output')],
    ['cache_creation', msg('Cache write')],
    ['cache_read', msg('Cache read')],
    ['reasoning', msg('Reasoning')],
  ] as const;
  const LIMIT_STATUS: Record<string, string> = {
    allowed: msg('allowed'),
    allowed_warning: msg('allowed warning'),
    rejected: msg('rejected'),
  };
  const toolsLine = (names: string[]) =>
    names.length === 0 ? t('none') : names.join(', ');
</script>

<div class="m3-usage" data-testid="usage">
  {#if !usage || (usage.turns.length === 0 && usage.limits.length === 0)}
    <p class="composer-note">
      {t(
        'No usage reported yet. A figure appears only when the provider reports it; one it did not report is shown as “not reported”, never as zero.',
      )}
    </p>
  {:else}
    <table class="m3-table" aria-label={t('Running totals')}>
      <caption>
        {turns === 1
          ? t(
              'Running totals over {count} turn, as Claude reported them. Nothing is added across providers or sessions.',
              { count: turns },
            )
          : t(
              'Running totals over {count} turns, as Claude reported them. Nothing is added across providers or sessions.',
              { count: turns },
            )}
      </caption>
      <tbody>
        {#each ROWS as [key, label] (key)}
          <tr>
            <th scope="row">{t(label)}</th>
            <td data-testid={`running-${key}`}
              >{tPhrase(running(usage.running[key]))}</td
            >
          </tr>
        {/each}
      </tbody>
    </table>

    <ul class="m3-turns" aria-label={t('Usage per turn')}>
      {#each usage.turns as turn (turn.turn_id)}
        <li class="rt-card" data-testid="usage-turn">
          <header>
            <h4 class="mono">
              {t('Turn {id}', { id: turn.turn_id.slice(0, 8) })}
            </h4>
            <span
              class="pill compact"
              data-tone={turn.completeness === 'complete'
                ? 'ok'
                : turn.completeness === 'running'
                  ? 'pending'
                  : 'idle'}
            >
              <span class="dot" aria-hidden="true"></span>
              {t(COMPLETENESS_LABEL[turn.completeness])}
            </span>
          </header>
          <p class="m3-meta">{tJoin(tokenLine(turn.total))}</p>
          {#if turn.echoed_models.length > 0}
            <p class="m3-meta">
              {t('Answered by {models} (named by the provider)', {
                models: turn.echoed_models.join(', '),
              })}
            </p>
          {/if}
        </li>
      {/each}
    </ul>

    {#if usage.limits.length > 0}
      <ul class="m3-limits" aria-label={t('Limits the provider reported')}>
        {#each usage.limits as l (l.window)}
          <li class="rt-card" data-testid="limit">
            <header>
              <h4>{t(WINDOW_LABEL[l.window])}</h4>
              <span
                class="pill compact"
                data-tone={l.status === 'allowed'
                  ? 'ok'
                  : l.status === 'rejected'
                    ? 'error'
                    : 'pending'}
              >
                <span class="dot" aria-hidden="true"></span>
                {t(LIMIT_STATUS[l.status])}
              </span>
            </header>
            <p class="m3-meta">
              {l.utilization === null
                ? t('Utilization not reported')
                : t('{percent} % used', {
                    percent: Math.round(l.utilization * 100),
                  })}
              · {l.resets_at === null
                ? t('reset time not reported')
                : t('resets {time}', { time: localTime(l.resets_at) })}
            </p>
            <p class="m3-meta">
              {t('An allowance, not a price. Reported {time}.', {
                time: localTime(l.observed_at),
              })}
            </p>
          </li>
        {/each}
      </ul>
    {/if}

    {#if estimates.length > 0}
      <details class="m3-estimate" data-testid="estimate">
        <summary>{t('Estimate computed by the SDK')}</summary>
        <p class="m3-meta">
          {t(
            'The Agent SDK adds a price estimate of its own to each result. It is not billing and not your subscription’s allowance, and this Console holds no price table to check it.',
          )}
        </p>
        <ul>
          {#each estimates as t (t.turn_id)}
            <li class="mono">
              {t.turn_id.slice(0, 8)} · ≈ ${t.estimate_usd?.toFixed(4)}
            </li>
          {/each}
        </ul>
      </details>
    {/if}
    {#if report && report.tap.requests > 0}
      <p class="m3-meta" data-testid="tap-summary">
        {report.tap.requests === 1
          ? t('{count} provider request checked · tools offered: {tools}', {
              count: counter(report.tap.requests),
              tools: toolsLine(report.tap.tools_seen),
            })
          : t('{count} provider requests checked · tools offered: {tools}', {
              count: counter(report.tap.requests),
              tools: toolsLine(report.tap.tools_seen),
            })}
      </p>
    {/if}
  {/if}
</div>
