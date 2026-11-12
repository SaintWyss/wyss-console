<script lang="ts">
  import { PROVIDERS, type Provider } from '../../contracts/api-keys';
  import { t } from '../../i18n/index.svelte';
  import { msg } from '../../i18n/msg';
  import type { CredentialsController, CredentialsState } from './controller';

  let {
    credentials,
    controller,
    visible,
  }: {
    credentials: CredentialsState;
    controller: CredentialsController;
    visible: boolean;
  } = $props();

  /** The services' names are their own; only the word for the one that is not named is ours. */
  const PROVIDER_LABEL: Record<Provider, string> = {
    anthropic: 'Anthropic',
    openai: 'OpenAI',
    google: 'Google',
    openrouter: 'OpenRouter',
    other: msg('Other'),
  };
  const FAILURE_TEXT: Record<string, string> = {
    name_invalid: msg(
      'The name must be 1 to 60 characters, without control characters.',
    ),
    secret_invalid: msg(
      'The key may not be empty, hold spaces or line breaks, or be longer than 4096 characters.',
    ),
    unknown: msg('That key is not kept any more.'),
    too_many: msg('There are too many keys (100).'),
    key_store_unavailable: msg(
      'No key store (the Secret Service) is available, so nothing was kept. A key is never kept unsealed.',
    ),
    key_lost: msg(
      'The key that sealed this record is gone from the key store. Replace the key.',
    ),
    damaged: msg('The record is damaged. Replace the key.'),
    store_unavailable: msg('The Console could not write its own state.'),
    internal: msg('Something went wrong in the desktop.'),
  };
  let name = $state('');
  let provider = $state<Provider>('anthropic');
  let renaming = $state<string | null>(null);
  let renamed = $state('');
  const day = (seconds: number) =>
    new Date(seconds * 1000).toLocaleString(undefined, {
      dateStyle: 'medium',
      timeStyle: 'short',
    });
  async function add() {
    await controller.add(name, provider);
    if (!credentials.failure) name = '';
  }
</script>

<section
  class="native-feature"
  aria-label={t('API credentials')}
  hidden={!visible}
  inert={!visible}
>
  <p class="native-note">
    {t(
      'API keys for the AI services, kept sealed on this computer (the key that seals them is in your Secret Service). A key is typed in a window of its own and is never shown again, never sent to this page and never written unsealed. Only a name, the service and a short checksum are listed. Nothing here starts an agent with a key.',
    )}
  </p>
  {#if credentials.failure}
    <p class="sessions-status failed" role="alert">
      {t(FAILURE_TEXT[credentials.failure])}
    </p>
  {/if}
  {#if credentials.notice}
    <p class="sessions-status" role="status">{t(credentials.notice)}</p>
  {/if}

  <form
    class="native-card native-launch"
    onsubmit={(event) => {
      event.preventDefault();
      void add();
    }}
  >
    <h3>{t('Add a key')}</h3>
    <div class="native-field">
      <label for="cred-name">{t('Name')}</label>
      <input
        id="cred-name"
        type="text"
        maxlength="60"
        bind:value={name}
        required
      />
    </div>
    <div class="native-field">
      <label for="cred-provider">{t('Service')}</label>
      <select id="cred-provider" bind:value={provider}>
        {#each PROVIDERS as word (word)}
          <option value={word}>{t(PROVIDER_LABEL[word])}</option>
        {/each}
      </select>
    </div>
    <div class="native-plan-actions">
      <button
        type="submit"
        class="action primary"
        disabled={credentials.busy || name.trim() === ''}
      >
        {t('Enter the key…')}
      </button>
    </div>
  </form>

  {#if credentials.phase === 'loading' || credentials.phase === 'idle'}
    <div class="sessions-empty" role="status" aria-busy="true">
      <p class="empty-title">{t('Reading…')}</p>
    </div>
  {:else if credentials.items.length === 0}
    <div class="sessions-empty">
      <p class="empty-title">{t('No keys yet')}</p>
      <p>{t('Add one above. Its value is asked for in a separate window.')}</p>
    </div>
  {:else}
    <ul class="events-list" aria-label={t('Keys')}>
      {#each credentials.items as item (item.id)}
        <li class="native-row" data-state="running">
          <span class="event-title">
            {#if renaming === item.id}
              <input
                type="text"
                maxlength="60"
                aria-label={t('New name')}
                bind:value={renamed}
              />
            {:else}{item.name}{/if}
          </span>
          <span class="pill compact" data-tone="idle"
            ><span class="dot" aria-hidden="true"></span>{t(
              PROVIDER_LABEL[item.provider],
            )}</span
          >
          <span class="event-meta native-facts">
            {t('checksum')} <span class="mono">{item.fingerprint}</span> · {t(
              'kept {date}',
              { date: day(item.created_at) },
            )}{item.updated_at !== item.created_at
              ? ` · ${t('changed {date}', { date: day(item.updated_at) })}`
              : ''}
          </span>
          <span class="native-actions">
            {#if renaming === item.id}
              <button
                type="button"
                class="action"
                disabled={credentials.busy || renamed.trim() === ''}
                onclick={async () => {
                  await controller.rename(item.id, renamed);
                  renaming = null;
                }}>{t('Save name')}</button
              >
              <button
                type="button"
                class="action"
                onclick={() => (renaming = null)}>{t('Cancel')}</button
              >
            {:else}
              <button
                type="button"
                class="action"
                disabled={credentials.busy}
                onclick={() => controller.replace(item.id)}
                >{t('Replace key…')}</button
              >
              <button
                type="button"
                class="action"
                disabled={credentials.busy}
                onclick={() => {
                  renaming = item.id;
                  renamed = item.name;
                }}>{t('Rename')}</button
              >
              <button
                type="button"
                class="action"
                disabled={credentials.busy}
                onclick={() => controller.verify(item.id)}>{t('Check')}</button
              >
              <button
                type="button"
                class="action"
                disabled={credentials.busy}
                onclick={() => controller.remove(item.id)}
                >{t('Delete…')}</button
              >
            {/if}
          </span>
        </li>
      {/each}
    </ul>
  {/if}
</section>
