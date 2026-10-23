<script lang="ts">
  import type {
    LaunchProvider,
    Mode,
    ProfileRef,
    SafetyProfile,
  } from '../../contracts/projects';

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
    Start
    <select bind:value={provider} aria-label="What to start">
      <option value="shell">A shell</option>
      {#if !scratch}
        <option value="claude">Claude Code</option>
        <option value="codex">Codex</option>
      {/if}
    </select>
  </label>
  {#if needsProvider}
    <fieldset>
      <legend>How</legend>
      <label
        ><input type="radio" bind:group={mode} value="native" /> Native (the agent's
        own permissions)</label
      >
      <label
        ><input
          type="radio"
          bind:group={mode}
          value="managed"
          disabled={provider === 'claude' || !managedAvailable}
        />
        Managed (mediated by the authority){provider === 'claude'
          ? ' — not available for Claude yet'
          : ''}</label
      >
    </fieldset>
    {#if managed}
      <fieldset>
        <legend>Managed profile — choose one</legend>
        <label
          ><input type="radio" bind:group={safety} value="inference" /> Inference
          (text only, no tools)</label
        >
        <label
          ><input type="radio" bind:group={safety} value="local_tools" /> Local tools
          (commands and file changes, each waiting for approval)</label
        >
        <p class="projects-note">
          This is a request. It grants nothing; the authority attaches the
          session in the profile it allows.
        </p>
      </fieldset>
    {:else if profiles.length > 0}
      <label>
        Native profile
        <select bind:value={profile} aria-label="Native profile">
          <option value="">Default</option>
          {#each profiles as p (p.id)}<option value={p.id}>{p.name}</option
            >{/each}
        </select>
      </label>
    {/if}
  {/if}
  <button type="submit" class="action primary" disabled={!can}>Start</button>
</form>
