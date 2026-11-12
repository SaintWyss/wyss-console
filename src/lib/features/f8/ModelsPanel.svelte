<script lang="ts">
  import type { SessionDetail } from '../../contracts/f8';
  import { t, tPhrase } from '../../i18n/index.svelte';
  import { observedModelLine } from '../sessions/present';
  import type { F8Controller, F8ViewState } from './controller';
  import { failureText } from './present';

  let {
    f8,
    detail,
    controller,
  }: {
    f8: F8ViewState;
    detail: SessionDetail;
    controller: F8Controller;
  } = $props();

  const runtime = $derived(detail.session.runtime);
  const models = $derived(f8.models);
</script>

<div class="rt-models">
  {#if runtime.actual !== 'ready'}
    <div class="sessions-empty">
      <p class="empty-title">{t('No models to show')}</p>
      <p>
        {t(
          'The models are what a running runtime listed when it started. A runtime that has stopped has no list, and an earlier one is never shown for a later one.',
        )}
      </p>
    </div>
  {:else if models.status === 'reading' || models.status === 'idle'}
    <div class="sessions-empty" role="status" aria-busy="true">
      <p class="empty-title">{t('Reading the models…')}</p>
    </div>
  {:else if models.status === 'failed' && models.failure}
    <div class="sessions-empty" role="alert">
      <p class="empty-title">{t(failureText(models.failure.code))}</p>
      <button
        type="button"
        class="action"
        onclick={() => controller.loadModels()}>{t('Try again')}</button
      >
    </div>
  {:else if models.info}
    <p class="rt-next" data-testid="observed-model">
      {tPhrase(
        observedModelLine({
          observed_model: models.info.observed,
          binding: detail.binding.state,
        }),
      )}
    </p>
    <p class="composer-note">
      {t('Runtime {version} · generation {generation}.', {
        version: models.info.version,
        generation: models.info.fence.generation,
      })}
      {#if models.info.complete === false}
        {t('The runtime listed more models than the authority took.')}
      {/if}
      {t(
        'A listed model is offered, not entitled: nothing here says the account may use it. Protocol 2 has no request that selects a model or a reasoning effort, so a turn uses what the runtime defaults to, and this view only reports what is offered and what the runtime says it uses.',
      )}
    </p>
    {#if models.info.entries.length === 0}
      <div class="sessions-empty">
        <p class="empty-title">{t('The runtime listed no models.')}</p>
      </div>
    {:else}
      <ul class="rt-model-list" aria-label={t('Models offered by the runtime')}>
        {#each models.info.entries as entry (entry.model_id)}
          <li class="rt-model">
            <p class="rt-model-name">
              {entry.display_name ?? entry.model_id}
              {#if entry.is_default}
                <span class="pill compact" data-tone="ok">
                  <span class="dot" aria-hidden="true"></span>
                  {t('Runtime default')}
                </span>
              {/if}
            </p>
            <p class="rt-model-id mono">
              {entry.model_id}{entry.provider_id
                ? ` · ${entry.provider_id}`
                : ''}
            </p>
            {#if entry.efforts !== null}
              <ul
                class="rt-chips"
                aria-label={t('Supported reasoning efforts')}
              >
                {#each entry.efforts as effort (effort)}
                  <li class:default={effort === entry.default_effort}>
                    {effort}{effort === entry.default_effort
                      ? ` · ${t('default')}`
                      : ''}
                  </li>
                {/each}
                {#if entry.efforts.length === 0}
                  <li>{t('no efforts listed')}</li>
                {/if}
              </ul>
            {/if}
            {#if entry.replaced_by}
              <p class="fact-note">
                {t('The runtime marks')}
                <span class="mono">{entry.replaced_by}</span>
                {t('as its replacement')}{entry.retires_at
                  ? t(', retiring {date}', { date: entry.retires_at })
                  : ''}.
              </p>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</div>
