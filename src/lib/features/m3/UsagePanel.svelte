<script lang="ts">
  import type { ProviderReportInfo } from '../../contracts/m3';
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
    ['input', 'Input'],
    ['output', 'Output'],
    ['cache_creation', 'Cache write'],
    ['cache_read', 'Cache read'],
    ['reasoning', 'Reasoning'],
  ] as const;
</script>

<div class="m3-usage" data-testid="usage">
  {#if !usage || (usage.turns.length === 0 && usage.limits.length === 0)}
    <p class="composer-note">
      No usage reported yet. A figure appears only when the provider reports it;
      one it did not report is shown as “not reported”, never as zero.
    </p>
  {:else}
    <table class="m3-table" aria-label="Running totals">
      <caption>
        Running totals over {turns}
        {turns === 1 ? 'turn' : 'turns'}, as Claude reported them. Nothing is
        added across providers or sessions.
      </caption>
      <tbody>
        {#each ROWS as [key, label] (key)}
          <tr>
            <th scope="row">{label}</th>
            <td data-testid={`running-${key}`}>{running(usage.running[key])}</td
            >
          </tr>
        {/each}
      </tbody>
    </table>

    <ul class="m3-turns" aria-label="Usage per turn">
      {#each usage.turns as turn (turn.turn_id)}
        <li class="rt-card" data-testid="usage-turn">
          <header>
            <h4 class="mono">Turn {turn.turn_id.slice(0, 8)}</h4>
            <span
              class="pill compact"
              data-tone={turn.completeness === 'complete'
                ? 'ok'
                : turn.completeness === 'running'
                  ? 'pending'
                  : 'idle'}
            >
              <span class="dot" aria-hidden="true"></span>
              {COMPLETENESS_LABEL[turn.completeness]}
            </span>
          </header>
          <p class="m3-meta">{tokenLine(turn.total)}</p>
          {#if turn.echoed_models.length > 0}
            <p class="m3-meta">
              Answered by {turn.echoed_models.join(', ')} (named by the provider)
            </p>
          {/if}
        </li>
      {/each}
    </ul>

    {#if usage.limits.length > 0}
      <ul class="m3-limits" aria-label="Limits the provider reported">
        {#each usage.limits as l (l.window)}
          <li class="rt-card" data-testid="limit">
            <header>
              <h4>{WINDOW_LABEL[l.window]}</h4>
              <span
                class="pill compact"
                data-tone={l.status === 'allowed'
                  ? 'ok'
                  : l.status === 'rejected'
                    ? 'error'
                    : 'pending'}
              >
                <span class="dot" aria-hidden="true"></span>
                {l.status.replace('_', ' ')}
              </span>
            </header>
            <p class="m3-meta">
              {l.utilization === null
                ? 'Utilization not reported'
                : `${Math.round(l.utilization * 100)} % used`}
              · {l.resets_at === null
                ? 'reset time not reported'
                : `resets ${localTime(l.resets_at)}`}
            </p>
            <p class="m3-meta">
              An allowance, not a price. Reported {localTime(l.observed_at)}.
            </p>
          </li>
        {/each}
      </ul>
    {/if}

    {#if estimates.length > 0}
      <details class="m3-estimate" data-testid="estimate">
        <summary>Estimate computed by the SDK</summary>
        <p class="m3-meta">
          The Agent SDK adds a price estimate of its own to each result. It is
          not billing and not your subscription’s allowance, and this Console
          holds no price table to check it.
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
        {counter(report.tap.requests)} provider request{report.tap.requests ===
        1
          ? ''
          : 's'} checked · tools offered:
        {report.tap.tools_seen.length === 0
          ? 'none'
          : report.tap.tools_seen.join(', ')}
      </p>
    {/if}
  {/if}
</div>
