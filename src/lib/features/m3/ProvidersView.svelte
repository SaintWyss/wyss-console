<script lang="ts">
  import FixtureConsole from './FixtureConsole.svelte';
  import HistorySettings from './HistorySettings.svelte';
  import ModelFacts from './ModelFacts.svelte';
  import ProfileCards from './ProfileCards.svelte';
  import UsagePanel from './UsagePanel.svelte';
  import type { M3Controller, M3ViewState } from './controller';
  import { t } from '../../i18n/index.svelte';
  import { reasonText } from './present';

  let {
    visible,
    view,
    controller,
  }: {
    visible: boolean;
    view: M3ViewState;
    controller: M3Controller;
  } = $props();

  const m3 = $derived(view.state);
  const report = $derived(m3?.fixture?.report ?? null);
</script>

<section
  class="providers-feature"
  aria-label={t('Providers')}
  hidden={!visible}
  inert={!visible}
>
  <div class="sessions-toolbar">
    <div class="sessions-heading">
      <h2>{t('Providers')}</h2>
      <p class="sessions-count">
        {#if m3}
          {t('Managed profiles')} · {m3.version} · {m3.claude_install.state ===
          'verified'
            ? t('Claude Agent SDK {sdk} (Claude Code {cli}) verified', {
                sdk: m3.claude_install.sdk_version,
                cli: m3.claude_install.cli_version,
              })
            : m3.claude_install.state === 'verifying'
              ? t('checking the Claude installation…')
              : m3.claude_install.state === 'refused'
                ? t('Claude installation refused: {reason}', {
                    reason: { text: reasonText(m3.claude_install.reason) },
                  })
                : t('no Claude installation named')}
        {:else}
          {t('Reading the providers…')}
        {/if}
      </p>
    </div>
    <button type="button" class="action" onclick={() => controller.refresh()}
      >{t('Refresh')}</button
    >
  </div>

  {#if view.stale}
    <div
      class="sessions-status failed unknown"
      role="status"
      data-testid="stale"
    >
      <p>
        {t(view.failure ?? '')}
        {t(
          'What is shown was read earlier and may have changed; no action is offered from it.',
        )}
      </p>
    </div>
  {/if}

  {#if !m3}
    <div
      class="sessions-empty"
      role="status"
      aria-busy={view.status === 'loading'}
    >
      <p class="empty-title">
        {view.status === 'failed'
          ? t(view.failure ?? '')
          : t('Reading the providers…')}
      </p>
    </div>
  {:else}
    <div class="rt-panel m3-body">
      {#if m3.fixture}
        <section class="rt-card" aria-labelledby="m3-fixture">
          <header>
            <h4 id="m3-fixture">{t('Claude runtime (fixture)')}</h4>
          </header>
          <FixtureConsole fixture={m3.fixture} {view} {controller} />
        </section>
      {/if}

      <section class="rt-card" aria-labelledby="m3-profiles">
        <header><h4 id="m3-profiles">{t('Profiles')}</h4></header>
        <p class="composer-note">
          {t(
            'A profile can start only when it is available. “Blocked” names the decision or evidence that is missing; fixtures never stand in for a run against the real provider.',
          )}
        </p>
        <ProfileCards {m3} />
      </section>

      <section class="rt-card" aria-labelledby="m3-models">
        <header><h4 id="m3-models">{t('Model and effort')}</h4></header>
        <ModelFacts {report} selection={m3.model_selection} />
      </section>

      <section class="rt-card" aria-labelledby="m3-usage">
        <header><h4 id="m3-usage">{t('Usage and limits')}</h4></header>
        <UsagePanel {report} />
      </section>

      <section class="rt-card" aria-labelledby="m3-history">
        <header><h4 id="m3-history">{t('Usage history')}</h4></header>
        <HistorySettings history={m3.history} />
      </section>
    </div>
  {/if}
</section>
