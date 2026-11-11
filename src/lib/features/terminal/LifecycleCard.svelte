<script lang="ts">
  import { onMount } from 'svelte';
  import type { HostLifecycle } from '../../contracts/terminal';
  import { hostLifecycle, setLingering } from '../../desktop/terminal-client';
  import { msg, t } from '../../i18n/index.svelte';

  const TITLES: Record<string, string> = {
    window_close: msg('Closing this window'),
    window_detach: msg('Leaving a terminal (detach)'),
    screen_lock: msg('Locking the screen'),
    logout_with_lingering: msg('Logging out, lingering on'),
    logout_without_lingering: msg('Logging out, lingering off'),
    suspend_resume: msg('Suspend and resume'),
    host_crash: msg('The host stopping or crashing'),
    reboot_or_power_off: msg('Reboot or power-off'),
  };
  const WORDS = {
    survives: msg('Keeps running'),
    ends: msg('Ends'),
    uncertain: msg('Not certain here'),
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
        ? msg('Lingering is on for your account.')
        : msg('Lingering is off for your account.');
    } catch (error) {
      const code =
        typeof error === 'object' && error && 'code' in error
          ? String(error.code)
          : '';
      message =
        code === 'unsupported'
          ? msg(
              'This system does not expose a lingering setting the Console can read.',
            )
          : msg('The setting could not be changed. Nothing was changed.');
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
  <h3 id="lifecycle-title">{t('Terminals after the window closes')}</h3>
  {#if failed || !facts}
    <p class="setting-hint">
      {failed
        ? t('What survives what could not be read from the host.')
        : t('Reading…')}
    </p>
  {:else}
    <p class="setting-hint">
      {t(
        'Your terminals belong to a host process, not to this window. Closing the window does not stop them; this is what happens in the other cases on this machine.',
      )}
    </p>
    <table class="lifecycle-table">
      <caption class="lifecycle-caption">{t('What survives what')}</caption>
      <thead>
        <tr><th>{t('When')}</th><th>{t('Terminals')}</th><th>{t('Why')}</th></tr
        >
      </thead>
      <tbody>
        {#each facts.events as event (event.id)}
          <tr data-current={event.current}>
            <td>{TITLES[event.id] ? t(TITLES[event.id]) : event.id}</td>
            <td data-outcome={event.outcome}>{t(WORDS[event.outcome])}</td>
            <td title={event.known_by}>{event.basis}</td>
          </tr>
        {/each}
      </tbody>
    </table>
    <p class="setting-hint">
      {t(
        'Lingering keeps your user session manager running after you log out. It is {state}. The Console never turns it on by itself.',
        {
          state:
            facts.lingering === null
              ? t('not something this system lets the Console read')
              : facts.lingering
                ? t('on')
                : t('off'),
        },
      )}
    </p>
    {#if facts.lingering !== null}
      {#if confirming}
        <div
          class="settings-actions"
          role="alertdialog"
          aria-label={t('Confirm lingering')}
        >
          <p>
            {confirming === 'enable'
              ? t(
                  'This runs “loginctl enable-linger” for your account: your user services will start at boot and stay after you log out. You can turn it off here at any time.',
                )
              : t(
                  'This runs “loginctl disable-linger” for your account: your user manager will stop when you log out, and anything running inside it with it.',
                )}
          </p>
          <button
            type="button"
            class="action"
            disabled={busy}
            onclick={() => apply(confirming === 'enable')}
            >{confirming === 'enable'
              ? t('Turn lingering on')
              : t('Turn lingering off')}</button
          >
          <button
            type="button"
            class="action"
            disabled={busy}
            onclick={() => (confirming = null)}>{t('Cancel')}</button
          >
        </div>
      {:else}
        <button
          type="button"
          class="action"
          onclick={() => (confirming = facts?.lingering ? 'disable' : 'enable')}
          >{facts.lingering
            ? t('Turn lingering off…')
            : t('Keep terminals after logout…')}</button
        >
      {/if}
    {/if}
    {#if message}<p role="status">{t(message)}</p>{/if}
  {/if}
</section>
