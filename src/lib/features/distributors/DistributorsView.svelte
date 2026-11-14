<script lang="ts">
  import type { ConfigItem } from '../../contracts/configuration';
  import { t, tPhrase } from '../../i18n/index.svelte';
  import type { ModelsState } from '../models/controller';
  import type { NativeState } from '../native/controller';
  import { distributorCards } from './present';
  import { listedIds, loadSeen, reconcile, saveSeen } from './seen';

  let {
    visible,
    native,
    models,
    configItems,
    configStale,
    onrefresh,
  }: {
    visible: boolean;
    native: NativeState;
    models: ModelsState;
    configItems: readonly ConfigItem[] | null;
    /** The last read of the agents' files failed: what is held is from an earlier one. */
    configStale: boolean;
    /** Reads the tools, the configuration and the Codex list again; false when the tools did not answer. */
    onrefresh: () => Promise<boolean>;
  } = $props();

  let busy = $state(false);
  let lastRead = $state<string | null>(null);
  /** Codex models not seen before the last refresh. Gone at the next one. */
  let fresh = $state<string[]>([]);
  /** False after a refresh in which `native_launcher` did not answer: nothing is said of the tools. */
  let answered = $state(true);
  let compared = false;

  /** One comparison per read: what is new is marked, and what was listed is kept. */
  function compare() {
    const result = reconcile(loadSeen(), listedIds(models));
    if (result.save) saveSeen(result.save);
    fresh = result.fresh;
    lastRead = new Date().toLocaleTimeString();
  }
  // The first read of the Codex list, once it has an answer of either kind.
  $effect(() => {
    if (
      !visible ||
      compared ||
      (models.phase !== 'ready' && models.phase !== 'failed')
    )
      return;
    compared = true;
    compare();
  });

  async function refresh() {
    if (busy) return;
    busy = true;
    try {
      answered = await onrefresh();
    } catch {
      answered = false;
    }
    busy = false;
    compared = true;
    compare();
  }

  const cards = $derived(
    distributorCards(
      answered ? native.launcher : null,
      models,
      configItems,
      native.profiles,
      fresh,
      configStale,
    ),
  );
</script>

<section
  class="distributors-feature"
  aria-label={t('Distributors')}
  hidden={!visible}
  inert={!visible}
>
  <div class="dist-bar">
    <p class="dist-read" role="status">
      {lastRead ? t('Last read: {time}', { time: lastRead }) : ''}
    </p>
    <button
      type="button"
      class="action"
      disabled={busy || models.phase === 'loading'}
      aria-busy={busy}
      onclick={() => void refresh()}>{t('Refresh models')}</button
    >
  </div>
  <p class="dist-note">{t('Only Codex reports new models.')}</p>
  <div class="dist-grid">
    {#each cards as card (card.id)}
      <article class="dist-card" aria-label={card.name} data-id={card.id}>
        <div class="dist-head">
          <h3>{card.name}</h3>
          <span class="dist-status" class:ok={card.installed}
            >{tPhrase(card.status)}</span
          >
        </div>
        <p class="dist-tool">{card.tool}</p>
        {#if card.models.length > 0}
          <ul class="dist-models">
            {#each card.models as model (model.key)}
              <li>
                <span class="dist-model"
                  >{typeof model.label === 'string'
                    ? model.label
                    : tPhrase(model.label)}</span
                >
                {#if model.fresh}<span class="dist-new">{t('new model')}</span
                  >{/if}
              </li>
            {/each}
          </ul>
        {/if}
        {#each card.notes as note (note.text)}
          <p class="dist-note">{tPhrase(note)}</p>
        {/each}
      </article>
    {/each}
  </div>
</section>
