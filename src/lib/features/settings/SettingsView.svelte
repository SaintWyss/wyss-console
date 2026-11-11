<script lang="ts">
  import type { Snippet } from 'svelte';
  import {
    CURSOR_STYLES,
    APPEARANCE_THEMES,
    EVENT_FILTER_PREFERENCES,
    LANDING_PAGES,
    LANGUAGES,
    TIME_FORMATS,
    type CursorStyle,
    type AppearanceTheme,
    type EventFilterPreference,
    type LandingPage,
    type Language,
    type TimeFormat,
  } from '../../contracts/settings';
  import { currentLanguage, msg, t } from '../../i18n/index.svelte';
  import type { AuthorityObservation } from '../../shell/authority';
  import type { LifecycleObservation } from '../../shell/lifecycle';
  import type { SettingsController, SettingsState } from './controller';
  import {
    CURSOR_LABEL,
    FAILURE_TEXT,
    FILTER_PREFERENCE_LABEL,
    FONT_SIZES,
    LANDING_LABEL,
    STORAGE_NOTICE,
    TIME_LABEL,
    authorityFacts,
    platformLabel,
    scrollbackChoices,
  } from './present';

  let {
    settings,
    controller,
    authority,
    lifecycle,
    retrying,
    onretry,
    visible,
    children,
  }: {
    settings: SettingsState;
    controller: SettingsController;
    /** The shell's one observations. Read here for wording only. */
    authority: AuthorityObservation;
    lifecycle: LifecycleObservation;
    retrying: boolean;
    onretry: () => void;
    visible: boolean;
    /** Cards another feature adds to this page (the project workspace's switch). */
    children?: Snippet;
  } = $props();

  const prefs = $derived(settings.preferences);
  const ready = $derived(settings.phase === 'ready');
  // Nothing can be changed while the saved preferences are a newer version's, or while
  // a reset is going on.
  const editable = $derived(
    ready && settings.view?.storage !== 'newer' && !settings.confirmingReset,
  );
  const notice = $derived(
    settings.view ? STORAGE_NOTICE[settings.view.storage] : null,
  );
  const facts = $derived(authorityFacts(authority, lifecycle));
  const canRetry = $derived(
    lifecycle.state === 'observed' && lifecycle.view.retry,
  );
  const about = $derived(settings.view?.about ?? null);

  // The confirmation replaces the button at the foot of the page and is taller than it,
  // so at 960×640 its buttons fell below the fold. It is brought into view when it opens,
  // and Cancel, the answer that changes nothing, has the focus.
  let resetConfirm: HTMLDivElement | undefined = $state();
  $effect(() => {
    if (!visible || !settings.confirmingReset || !resetConfirm) return;
    resetConfirm.scrollIntoView?.({ block: 'nearest' });
    resetConfirm
      .querySelector<HTMLButtonElement>('button[data-reset-cancel]')
      ?.focus();
  });
  const protocol = $derived(
    authority.state === 'observed' && authority.view.authority
      ? t('Version {version}', {
          version: authority.view.authority.protocol_version,
        })
      : null,
  );

  const THEME_LABEL: Record<AppearanceTheme, string> = {
    dark: msg('Dark'),
    light: msg('Light'),
    high_contrast: msg('High contrast'),
  };
  // Each language is named in itself, so it can be found by someone who reads no other.
  const LANGUAGE_LABEL: Record<Language, string> = {
    en: 'English',
    es: 'Español',
  };

  function landing(event: Event) {
    const page = (event.currentTarget as HTMLSelectElement)
      .value as LandingPage;
    controller.change((p) => ({
      ...p,
      interface: { ...p.interface, landing_page: page },
    }));
  }
  function language(event: Event) {
    const next = (event.currentTarget as HTMLSelectElement).value as Language;
    controller.change((p) => ({
      ...p,
      interface: { ...p.interface, language: next },
    }));
  }
  function appearance(event: Event) {
    const theme = (event.currentTarget as HTMLSelectElement)
      .value as AppearanceTheme;
    controller.change((p) => ({
      ...p,
      interface: { ...p.interface, theme },
    }));
  }
  function fontSize(event: Event) {
    const size = Number((event.currentTarget as HTMLSelectElement).value);
    controller.change((p) => ({
      ...p,
      terminal: { ...p.terminal, font_size: size },
    }));
  }
  function scrollback(event: Event) {
    const lines = Number((event.currentTarget as HTMLSelectElement).value);
    controller.change((p) => ({
      ...p,
      terminal: { ...p.terminal, scrollback: lines },
    }));
  }
  function cursor(event: Event) {
    const style = (event.currentTarget as HTMLSelectElement)
      .value as CursorStyle;
    controller.change((p) => ({
      ...p,
      terminal: { ...p.terminal, cursor_style: style },
    }));
  }
  function blink(event: Event) {
    const on = (event.currentTarget as HTMLInputElement).checked;
    controller.change((p) => ({
      ...p,
      terminal: { ...p.terminal, cursor_blink: on },
    }));
  }
  function time(event: Event) {
    const format = (event.currentTarget as HTMLSelectElement)
      .value as TimeFormat;
    controller.change((p) => ({
      ...p,
      events: { ...p.events, time_format: format },
    }));
  }
  function filter(event: Event) {
    const value = (event.currentTarget as HTMLSelectElement)
      .value as EventFilterPreference;
    controller.change((p) => ({
      ...p,
      events: { ...p.events, default_filter: value },
    }));
  }
</script>

<section
  class="settings-feature"
  aria-label={t('Settings')}
  hidden={!visible}
  inert={!visible}
>
  <div class="sessions-toolbar">
    <div class="sessions-heading">
      <h2>{t('Settings')}</h2>
      <p class="sessions-count">
        {#if settings.phase === 'loading'}
          {t('Reading preferences')}
        {:else if settings.phase === 'failed'}
          {t('Preferences unavailable')}
        {:else if settings.saving}
          {t('Saving…')}
        {:else}
          {t('Saved on this computer')}
        {/if}
      </p>
    </div>
  </div>

  <div class="settings-body">
    {#if settings.phase === 'failed'}
      <div class="sessions-status failed" role="alert">
        <p>
          {t(
            'Preferences could not be read, so the defaults are shown. Nothing is saved until they can be.',
          )}
        </p>
        <button type="button" class="action" onclick={() => controller.retry()}
          >{t('Try again')}</button
        >
      </div>
    {/if}
    {#if notice}
      <div
        class="sessions-status settings-notice"
        data-tone={notice.tone}
        role="status"
      >
        <p>
          <strong>{t(notice.title)}.</strong>
          {t(notice.text)}
        </p>
      </div>
    {/if}
    {#if settings.failure}
      <div class="sessions-status failed" role="alert">
        <p>{t(FAILURE_TEXT[settings.failure])}</p>
      </div>
    {/if}

    <p class="settings-lede">
      {t(
        'Preferences change how Wyss Console looks and where it opens. They never change what Codex may do: sessions start in Inference, Local Tools is chosen for each session, and every command or file change it proposes waits for your approval.',
      )}
    </p>

    <section class="settings-section" aria-labelledby="settings-appearance">
      <h3 id="settings-appearance">{t('Appearance')}</h3>
      <div class="setting-row">
        <label for="setting-theme">{t('Color theme')}</label>
        <select
          id="setting-theme"
          aria-describedby="setting-theme-note"
          disabled={!editable}
          value={prefs.interface.theme}
          onchange={appearance}
        >
          {#each APPEARANCE_THEMES as theme (theme)}
            <option value={theme}>{t(THEME_LABEL[theme])}</option>
          {/each}
        </select>
        <p id="setting-theme-note" class="settings-note setting-hint">
          {t(
            'Changes appearance only. It does not change provider or approval permissions.',
          )}
        </p>
      </div>
    </section>

    <section class="settings-section" aria-labelledby="settings-interface">
      <h3 id="settings-interface">{t('Interface')}</h3>
      <div class="setting-row">
        <label for="setting-language">{t('Language')}</label>
        <select
          id="setting-language"
          aria-describedby="setting-language-note"
          disabled={!editable}
          value={prefs.interface.language}
          onchange={language}
        >
          {#each LANGUAGES as code (code)}
            <option value={code} lang={code}>{LANGUAGE_LABEL[code]}</option>
          {/each}
        </select>
        <p id="setting-language-note" class="setting-hint">
          {t(
            'The language of Wyss Console itself. What an agent, a provider or the system writes is shown as it arrives.',
          )}
        </p>
      </div>
      <div class="setting-row">
        <label for="setting-landing">{t('Open on')}</label>
        <select
          id="setting-landing"
          disabled={!editable}
          value={prefs.interface.landing_page}
          onchange={landing}
        >
          {#each LANDING_PAGES as page (page)}
            <option value={page}>{t(LANDING_LABEL[page])}</option>
          {/each}
        </select>
        <p class="setting-hint">
          {t('The view shown when Wyss Console starts.')}
        </p>
      </div>
    </section>

    <section class="settings-section" aria-labelledby="settings-terminal">
      <h3 id="settings-terminal">{t('Terminal')}</h3>
      <div class="setting-row">
        <label for="setting-font">{t('Font size')}</label>
        <select
          id="setting-font"
          disabled={!editable}
          value={String(prefs.terminal.font_size)}
          onchange={fontSize}
        >
          {#each FONT_SIZES as size (size)}
            <option value={String(size)}>{size} px</option>
          {/each}
        </select>
      </div>
      <div class="setting-row">
        <label for="setting-scrollback">{t('History kept')}</label>
        <select
          id="setting-scrollback"
          disabled={!editable}
          value={String(prefs.terminal.scrollback)}
          onchange={scrollback}
        >
          {#each scrollbackChoices(prefs.terminal.scrollback) as lines (lines)}
            <option value={String(lines)}
              >{t('{count} lines', {
                count: lines.toLocaleString(
                  currentLanguage() === 'es' ? 'es-AR' : 'en-US',
                ),
              })}</option
            >
          {/each}
        </select>
        <p class="setting-hint">
          {t(
            'How far back each terminal can scroll. Larger values use more memory.',
          )}
        </p>
      </div>
      <div class="setting-row">
        <label for="setting-cursor">{t('Cursor')}</label>
        <select
          id="setting-cursor"
          disabled={!editable}
          value={prefs.terminal.cursor_style}
          onchange={cursor}
        >
          {#each CURSOR_STYLES as style (style)}
            <option value={style}>{t(CURSOR_LABEL[style])}</option>
          {/each}
        </select>
      </div>
      <div class="setting-row">
        <label for="setting-blink">{t('Blinking cursor')}</label>
        <input
          id="setting-blink"
          type="checkbox"
          disabled={!editable}
          checked={prefs.terminal.cursor_blink}
          onchange={blink}
        />
      </div>
      <p class="setting-hint">
        {t(
          'These apply to every terminal, open or new. What a terminal runs is not a preference.',
        )}
      </p>
    </section>

    <section class="settings-section" aria-labelledby="settings-events">
      <h3 id="settings-events">{t('Events')}</h3>
      <div class="setting-row">
        <label for="setting-time">{t('Time')}</label>
        <select
          id="setting-time"
          disabled={!editable}
          value={prefs.events.time_format}
          onchange={time}
        >
          {#each TIME_FORMATS as format (format)}
            <option value={format}>{t(TIME_LABEL[format])}</option>
          {/each}
        </select>
      </div>
      <div class="setting-row">
        <label for="setting-filter">{t('Show first')}</label>
        <select
          id="setting-filter"
          disabled={!editable}
          value={prefs.events.default_filter}
          onchange={filter}
        >
          {#each EVENT_FILTER_PREFERENCES as value (value)}
            <option {value}>{t(FILTER_PREFERENCE_LABEL[value])}</option>
          {/each}
        </select>
        <p class="setting-hint">
          {t(
            'Only narrows what is shown. The authority records every event either way.',
          )}
        </p>
      </div>
    </section>

    <section class="settings-section" aria-labelledby="settings-authority">
      <h3 id="settings-authority">{t('Authority · protocol 1')}</h3>
      <dl class="settings-facts">
        {#each facts as fact (fact.term)}
          <dt>{t(fact.term)}</dt>
          <dd>
            {#if fact.tone}<span
                class="dot"
                data-tone={fact.tone}
                aria-hidden="true"
              ></span>{/if}
            {t(fact.value, fact.values)}
          </dd>
        {/each}
      </dl>
      {#if canRetry}
        <div class="settings-actions">
          <button
            type="button"
            class="action"
            disabled={retrying}
            onclick={onretry}>{retrying ? t('Retrying…') : t('Retry')}</button
          >
        </div>
      {/if}
      <p class="setting-hint">
        {t(
          'The protocol 1 authority keeps its sessions, approvals and history, and keeps running when this window is closed. Local terminals and these preferences work without it. Managed runtimes use a separate protocol 2 connection, shown in Runtimes.',
        )}
      </p>
    </section>

    <section class="settings-section" aria-labelledby="settings-about">
      <h3 id="settings-about">{t('About')}</h3>
      <dl class="settings-facts">
        <dt>Wyss Console</dt>
        <dd>
          {about
            ? t('Version {version}', { version: about.version })
            : t('Not available')}
        </dd>
        {#if about?.revision}
          <dt>{t('Build')}</dt>
          <dd class="mono">{about.revision}</dd>
        {/if}
        <dt>{t('Platform')}</dt>
        <dd>{about ? platformLabel(about.platform) : t('Not available')}</dd>
        {#if protocol}
          <dt>{t('Authority protocol')}</dt>
          <dd>{protocol}</dd>
        {/if}
      </dl>
    </section>

    <section class="settings-section" aria-labelledby="settings-reset">
      <h3 id="settings-reset">{t('Reset')}</h3>
      {#if settings.confirmingReset}
        <div
          class="close-confirm"
          role="alertdialog"
          aria-labelledby="reset-q"
          bind:this={resetConfirm}
        >
          <p id="reset-q">{t('Reset all preferences to their defaults?')}</p>
          <p class="setting-hint">
            {t(
              'Your sessions, history, approvals, workspaces and Codex login are not touched.',
            )}
          </p>
          <div class="settings-actions">
            <button
              type="button"
              class="action danger"
              onclick={() => controller.confirmReset()}
              >{t('Reset preferences')}</button
            >
            <button
              type="button"
              class="action"
              data-reset-cancel
              onclick={() => controller.cancelReset()}>{t('Cancel')}</button
            >
          </div>
        </div>
      {:else}
        <div class="settings-actions">
          <button
            type="button"
            class="action"
            disabled={!ready || settings.saving}
            onclick={() => controller.askReset()}
            >{t('Reset preferences…')}</button
          >
        </div>
        <p class="setting-hint">
          {t('Restores the defaults on this screen. Nothing else is deleted.')}
        </p>
      {/if}
    </section>
    {@render children?.()}
  </div>
</section>
