<script lang="ts">
  import type {
    LaunchProvider,
    Mode,
    ProfileRef,
    SafetyProfile,
  } from '../../contracts/projects';
  import { t } from '../../i18n/index.svelte';

  export interface NativeProfileOption {
    id: string;
    name: string;
    version: number;
  }

  let {
    scratch,
    managedAvailable,
    profiles,
    busy,
    onlaunch,
  }: {
    /** Scratch has no folder and no provider: a plain shell only. */
    scratch: boolean;
    /** Whether a Managed authority answered. */
    managedAvailable: boolean;
    profiles: NativeProfileOption[];
    busy: boolean;
    onlaunch: (choice: {
      provider: LaunchProvider;
      mode: Mode;
      nativeProfile: ProfileRef | null;
      managedSafetyProfile: SafetyProfile | null;
    }) => void;
  } = $props();

  let provider = $state<LaunchProvider>('shell');
  let mode = $state<Mode>('native');
  let profile = $state('');
  // Nothing is preselected: a Managed launch always asks, and `local_tools` is never a default.
  let safety = $state<SafetyProfile | ''>('');

  const managed = $derived(mode === 'managed');
  const needsProvider = $derived(provider !== 'shell');
  const can = $derived(
    !busy &&
      (provider === 'shell'
        ? mode === 'native'
        : !managed || (provider === 'codex' && safety !== '')),
  );
  $effect(() => {
    // A shell is Native; Claude has no Managed adapter yet.
    if (provider === 'shell') mode = 'native';
    if (provider === 'claude' && mode === 'managed') mode = 'native';
  });
  function launch() {
    const chosen = profiles.find((p) => String(p.id) === profile);
    onlaunch({
      provider,
      mode,
      nativeProfile:
        !managed && needsProvider && chosen
          ? { id: chosen.id, version: String(chosen.version) }
          : null,
      managedSafetyProfile: managed && safety !== '' ? safety : null,
    });
  }
</script>

<form
  class="projects-launch"
  onsubmit={(event) => {
    event.preventDefault();
    if (can) launch();
  }}
>
  <label>
    {t('Start')}
    <select bind:value={provider} aria-label={t('What to start')}>
      <option value="shell">{t('A shell')}</option>
      {#if !scratch}
        <option value="claude">Claude Code</option>
        <option value="codex">Codex</option>
      {/if}
    </select>
  </label>
  {#if needsProvider}
    <fieldset>
      <legend>{t('How')}</legend>
      <label
        ><input type="radio" bind:group={mode} value="native" />
        {t("Native (the agent's own permissions)")}</label
      >
      <label
        ><input
          type="radio"
          bind:group={mode}
          value="managed"
          disabled={provider === 'claude' || !managedAvailable}
        />
        {provider === 'claude'
          ? t(
              'Managed (mediated by the authority) — not available for Claude yet',
            )
          : t('Managed (mediated by the authority)')}</label
      >
    </fieldset>
    {#if managed}
      <fieldset>
        <legend>{t('Managed profile — choose one')}</legend>
        <label
          ><input type="radio" bind:group={safety} value="inference" />
          {t('Inference (text only, no tools)')}</label
        >
        <label
          ><input type="radio" bind:group={safety} value="local_tools" />
          {t(
            'Local tools (commands and file changes, each waiting for approval)',
          )}</label
        >
        <p class="projects-note">
          {t(
            'This is a request. It grants nothing; the authority attaches the session in the profile it allows.',
          )}
        </p>
      </fieldset>
    {:else if profiles.length > 0}
      <label>
        {t('Native profile')}
        <select bind:value={profile} aria-label={t('Native profile')}>
          <option value="">{t('Default')}</option>
          {#each profiles as p (p.id)}<option value={p.id}>{p.name}</option
            >{/each}
        </select>
      </label>
    {/if}
  {/if}
  <button type="submit" class="action primary" disabled={!can}
    >{t('Start')}</button
  >
</form>
