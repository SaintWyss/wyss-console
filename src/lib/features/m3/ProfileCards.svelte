<script lang="ts">
  import type { M3State } from '../../contracts/m3';
  import { msg, t } from '../../i18n/index.svelte';
  import { AVAILABILITY_LABEL, PROFILE_LABEL, reasonText } from './present';

  let { m3 }: { m3: M3State } = $props();
  const EVIDENCE: Record<string, string> = {
    passed: msg('passed'),
    not_run: msg('not run'),
    not_applicable: msg('not applicable'),
  };
</script>

<ul class="m3-cards" aria-label={t('Provider profiles')}>
  {#each m3.profiles as card (card.id)}
    {@const a = AVAILABILITY_LABEL[card.availability]}
    <li class="rt-card m3-card" data-testid={`profile-${card.id}`}>
      <header>
        <h4>{PROFILE_LABEL[card.id]}</h4>
        <span class="pill compact" data-tone={a.tone}>
          <span class="dot" aria-hidden="true"></span>
          {t(a.label)}
        </span>
      </header>
      <p class="m3-meta mono">{card.name}</p>
      {#if card.build}
        <p class="m3-meta">
          {#if card.build.sdk}Agent SDK {card.build.sdk} ·
          {/if}{card.provider === 'claude' ? 'Claude Code' : 'Codex'}
          {card.build.cli}
        </p>
      {:else}
        <p class="m3-meta">{t('No build verified on this machine.')}</p>
      {/if}
      {#if card.reasons.length > 0}
        <ul class="m3-reasons" aria-label={t('Why it is not available')}>
          {#each card.reasons as reason (reason)}
            <li>{t(reasonText(reason))}</li>
          {/each}
        </ul>
      {/if}
      <ul class="rt-chips" aria-label={t('Evidence')}>
        <li data-evidence={card.evidence.fixture}>
          {t('Fixtures:')}
          {t(EVIDENCE[card.evidence.fixture])}
        </li>
        <li data-evidence={card.evidence.real_provider}>
          {t('Real provider:')}
          {t(EVIDENCE[card.evidence.real_provider])}
        </li>
      </ul>
      {#if card.can_start}
        <p class="m3-meta">
          {t('Start it from Runtimes; this view does not start anything.')}
        </p>
      {/if}
    </li>
  {/each}
</ul>
