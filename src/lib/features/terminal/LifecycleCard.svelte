<script lang="ts">
  import { onMount } from 'svelte';
  import type { HostLifecycle } from '../../contracts/terminal';
  import { hostLifecycle, setLingering } from '../../desktop/terminal-client';

  const TITLES: Record<string, string> = {
    window_close: 'Closing this window',
    window_detach: 'Leaving a terminal (detach)',
    screen_lock: 'Locking the screen',
    logout_with_lingering: 'Logging out, lingering on',
    logout_without_lingering: 'Logging out, lingering off',
    suspend_resume: 'Suspend and resume',
    host_crash: 'The host stopping or crashing',
    reboot_or_power_off: 'Reboot or power-off',
  };
  const WORDS = {
    survives: 'Keeps running',
    ends: 'Ends',
    uncertain: 'Not certain here',
  } as const;

  let facts = $state<HostLifecycle | null>(null);
  let failed = $state(false);
  let confirming = $state<'enable' | 'disable' | null>(null);
  let busy = $state(false);
  let message = $state('');

  onMount(() => {
    void load();
  });
  async function load() {
    try {
      facts = await hostLifecycle();
      failed = false;
    } catch {
      failed = true;
    }
  }
  async function apply(enable: boolean) {
    busy = true;
    message = '';
    try {
      facts = await setLingering(enable);
      message = enable
        ? 'Lingering is on for your account.'
        : 'Lingering is off for your account.';
    } catch (error) {
      const code =
        typeof error === 'object' && error && 'code' in error
          ? String(error.code)
          : '';
      message =
        code === 'unsupported'
          ? 'This system does not expose a lingering setting the Console can read.'
          : 'The setting could not be changed. Nothing was changed.';
      await load();
    } finally {
      busy = false;
      confirming = null;
    }
  }
</script>

<section
  class="settings-section"
  aria-labelledby="lifecycle-title"
  data-testid="lifecycle"
>
  <h3 id="lifecycle-title">Terminals after the window closes</h3>
  {#if failed || !facts}
    <p class="setting-hint">
      {failed
        ? 'What survives what could not be read from the host.'
        : 'Reading…'}
    </p>
  {:else}
    <p class="setting-hint">
      Your terminals belong to a host process, not to this window. Closing the
      window does not stop them; this is what happens in the other cases on this
      machine.
    </p>
    <table class="lifecycle-table">
      <caption class="lifecycle-caption">What survives what</caption>
      <thead>
        <tr><th>When</th><th>Terminals</th><th>Why</th></tr>
      </thead>
      <tbody>
        {#each facts.events as event (event.id)}
          <tr data-current={event.current}>
            <td>{TITLES[event.id] ?? event.id}</td>
            <td data-outcome={event.outcome}>{WORDS[event.outcome]}</td>
            <td title={event.known_by}>{event.basis}</td>
          </tr>
        {/each}
      </tbody>
    </table>
    <p class="setting-hint">
      Lingering keeps your user session manager running after you log out. It is
      {facts.lingering === null
        ? 'not something this system lets the Console read'
        : facts.lingering
          ? 'on'
          : 'off'}. The Console never turns it on by itself.
    </p>
    {#if facts.lingering !== null}
      {#if confirming}
        <div
          class="settings-actions"
          role="alertdialog"
          aria-label="Confirm lingering"
        >
          <p>
            {confirming === 'enable'
              ? 'This runs “loginctl enable-linger” for your account: your user services will start at boot and stay after you log out. You can turn it off here at any time.'
              : 'This runs “loginctl disable-linger” for your account: your user manager will stop when you log out, and anything running inside it with it.'}
          </p>
          <button
            type="button"
            class="action"
            disabled={busy}
            onclick={() => apply(confirming === 'enable')}
            >{confirming === 'enable'
              ? 'Turn lingering on'
              : 'Turn lingering off'}</button
          >
          <button
            type="button"
            class="action"
            disabled={busy}
            onclick={() => (confirming = null)}>Cancel</button
          >
        </div>
      {:else}
        <button
          type="button"
          class="action"
          onclick={() => (confirming = facts?.lingering ? 'disable' : 'enable')}
          >{facts.lingering
            ? 'Turn lingering off…'
            : 'Keep terminals after logout…'}</button
        >
      {/if}
    {/if}
    {#if message}<p role="status">{message}</p>{/if}
  {/if}
</section>
