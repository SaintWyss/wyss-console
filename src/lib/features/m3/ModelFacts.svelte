<script lang="ts">
  import type {
    FeatureInfo,
    ModelFactsInfo,
    ProviderReportInfo,
  } from '../../contracts/m3';
  import { msg, t, tPhrase } from '../../i18n/index.svelte';
  import { effortLine, localTime, reasonText, rerouteLine } from './present';

  let {
    report,
    selection,
  }: {
    report: ProviderReportInfo | null;
    selection: FeatureInfo;
  } = $props();
  const facts = $derived<ModelFactsInfo | null>(report?.model ?? null);
  const FRESHNESS: Record<string, string> = {
    fresh: msg('fresh'),
    stale: msg('stale'),
  };
</script>

<div class="m3-facts" data-testid="model-facts">
  {#if !facts}
    <p class="composer-note">
      {t(
        'No Claude runtime is running, so there are no model facts to show. A listed model is offered, not entitled, and a request is not proof of use: when a runtime runs, this view keeps what is offered, configured, requested and reported in use apart.',
      )}
    </p>
  {:else}
    <div class="m3-four">
      <section aria-labelledby="mf-offered">
        <h5 id="mf-offered">{t('Offered')}</h5>
        {#if facts.offered}
          <p class="m3-meta" data-testid="offered-source">
            {t(
              'The provider’s own list · revision {revision} · {freshness} · taken {time}',
              {
                revision: facts.offered.revision,
                freshness: t(FRESHNESS[facts.offered.freshness]),
                time: localTime(facts.offered.observed_at),
              },
            )}
            {#if !facts.offered.complete}· {t('incomplete')}{/if}
          </p>
          <ul class="rt-model-list" aria-label={t('Models offered')}>
            {#each facts.offered.entries as e (e.model_id)}
              <li class="rt-model">
                <p class="rt-model-name">
                  {e.alias ?? e.model_id}
                  {#if e.is_default}<span class="pill compact" data-tone="ok"
                      ><span class="dot" aria-hidden="true"></span>{t(
                        'Default',
                      )}</span
                    >{/if}
                </p>
                <p class="rt-model-id mono">{e.model_id}</p>
                <p class="m3-meta">
                  {t('Efforts it lists: {efforts}', {
                    efforts: e.efforts.join(', ') || t('none'),
                  })}
                </p>
              </li>
            {/each}
          </ul>
          <p class="m3-meta" data-testid="not-entitlement">
            {t(
              'Offered is not entitled: nothing here says this account may use a model.',
            )}
          </p>
        {:else}
          <p class="m3-meta">{t('Not listed yet.')}</p>
        {/if}
      </section>
      <section aria-labelledby="mf-configured">
        <h5 id="mf-configured">{t('Configured')}</h5>
        {#if facts.configured}
          <p class="mono">{facts.configured.model}</p>
          <p class="m3-meta">
            {t('Effort: {effort}', {
              effort: facts.configured.effort ?? t('the provider default'),
            })}
          </p>
        {:else}
          <p class="m3-meta">
            {t(
              'Nothing was pinned when the runtime started: the provider’s default.',
            )}
          </p>
        {/if}
      </section>
      <section aria-labelledby="mf-requested">
        <h5 id="mf-requested">{t('Requested')}</h5>
        {#if facts.requested}
          <p class="mono">{facts.requested.resolved_model}</p>
          <p class="m3-meta">
            {t('For turn {turn} · generation {generation}', {
              turn: facts.requested.turn_id.slice(0, 8),
              generation: facts.requested.generation,
            })}
          </p>
        {:else}
          <p class="m3-meta">
            {t('No model was requested for the latest turn.')}
          </p>
        {/if}
      </section>
      <section aria-labelledby="mf-reported">
        <h5 id="mf-reported">{t('Reported in use')}</h5>
        {#if facts.reported_in_use.length > 0}
          <ul class="rt-chips">
            {#each facts.reported_in_use as m (m)}<li class="mono">
                {m}
              </li>{/each}
          </ul>
        {:else}
          <p class="m3-meta">{t('The provider has not named a model yet.')}</p>
        {/if}
        <p
          class="m3-meta"
          data-testid="reroute"
          data-state={facts.reroute.state}
        >
          {tPhrase(rerouteLine(facts.reroute))}
        </p>
      </section>
    </div>
    <p
      class="m3-meta"
      data-testid="effort-evidence"
      data-state={facts.effort.state}
    >
      {tPhrase(effortLine(facts.effort))}
    </p>
  {/if}

  <div class="m3-select" data-testid="model-selection">
    <label>
      <span class="mode-name">{t('Model for the next turn')}</span>
      <select disabled={!selection.available}>
        <option>{t('The provider’s default')}</option>
        {#each facts?.offered?.entries ?? [] as e (e.model_id)}
          <option>{e.alias ?? e.model_id}</option>
        {/each}
      </select>
    </label>
    <label>
      <span class="mode-name">{t('Reasoning effort')}</span>
      <select disabled>
        <option>{t('Fixed when the runtime starts')}</option>
      </select>
    </label>
    {#if !selection.available}
      <ul class="m3-reasons" aria-label={t('Why model selection is off')}>
        {#each selection.reasons as r (r)}<li>{t(reasonText(r))}</li>{/each}
      </ul>
    {/if}
    <p class="m3-meta">
      {t(
        'Claude’s SDK can change the model for a turn, but reads the effort once, when its runtime starts. A request that cannot be honored is refused; it is never replaced by another model.',
      )}
    </p>
  </div>
</div>
