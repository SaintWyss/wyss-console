<script lang="ts">
  import type { Snippet } from 'svelte';
  import {
    CURSOR_STYLES,
    EVENT_FILTER_PREFERENCES,
    LANDING_PAGES,
    TIME_FORMATS,
    type CursorStyle,
    type EventFilterPreference,
    type LandingPage,
    type TimeFormat,
  } from '../../contracts/settings';
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
    scrollbackLabel,
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
      ? `Version ${authority.view.authority.protocol_version}`
      : null,
  );

  function landing(event: Event) {
    const page = (event.currentTarget as HTMLSelectElement)
      .value as LandingPage;
    controller.change((p) => ({ ...p, interface: { landing_page: page } }));
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
  aria-label="Settings"
  hidden={!visible}
  inert={!visible}
>
  <div class="sessions-toolbar">
    <div class="sessions-heading">
      <h2>Settings</h2>
      <p class="sessions-count">
        {#if settings.phase === 'loading'}
          Reading preferences
        {:else if settings.phase === 'failed'}
          Preferences unavailable
        {:else if settings.saving}
          Saving…
        {:else}
          Saved on this computer
        {/if}
      </p>
    </div>
  </div>

  <div class="settings-body">
    {#if settings.phase === 'failed'}
      <div class="sessions-status failed" role="alert">
        <p>
          Preferences could not be read, so the defaults are shown. Nothing is
          saved until they can be.
        </p>
        <button type="button" class="action" onclick={() => controller.retry()}
          >Try again</button
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
          <strong>{notice.title}.</strong>
          {notice.text}
        </p>
      </div>
    {/if}
    {#if settings.failure}
      <div class="sessions-status failed" role="alert">
        <p>{FAILURE_TEXT[settings.failure]}</p>
      </div>
    {/if}

    <p class="settings-lede">
      Preferences change how Wyss Console looks and where it opens. They never
      change what Codex may do: sessions start in Inference, Local Tools is
      chosen for each session, and every command or file change it proposes
      waits for your approval.
    </p>

    <section class="settings-section" aria-labelledby="settings-appearance">
      <h3 id="settings-appearance">Appearance</h3>
      <p class="settings-note">
        Wyss Console uses its dark theme. There is nothing to choose here yet.
      </p>
    </section>

    <section class="settings-section" aria-labelledby="settings-interface">
      <h3 id="settings-interface">Interface</h3>
      <div class="setting-row">
        <label for="setting-landing">Open on</label>
        <select
          id="setting-landing"
          disabled={!editable}
          value={prefs.interface.landing_page}
          onchange={landing}
        >
          {#each LANDING_PAGES as page (page)}
            <option value={page}>{LANDING_LABEL[page]}</option>
          {/each}
        </select>
        <p class="setting-hint">The view shown when Wyss Console starts.</p>
      </div>
    </section>

    <section class="settings-section" aria-labelledby="settings-terminal">
      <h3 id="settings-terminal">Terminal</h3>
      <div class="setting-row">
        <label for="setting-font">Font size</label>
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
        <label for="setting-scrollback">History kept</label>
        <select
          id="setting-scrollback"
          disabled={!editable}
          value={String(prefs.terminal.scrollback)}
          onchange={scrollback}
        >
          {#each scrollbackChoices(prefs.terminal.scrollback) as lines (lines)}
            <option value={String(lines)}>{scrollbackLabel(lines)}</option>
          {/each}
        </select>
        <p class="setting-hint">
          How far back each terminal can scroll. Larger values use more memory.
        </p>
      </div>
      <div class="setting-row">
        <label for="setting-cursor">Cursor</label>
        <select
          id="setting-cursor"
          disabled={!editable}
          value={prefs.terminal.cursor_style}
          onchange={cursor}
        >
          {#each CURSOR_STYLES as style (style)}
            <option value={style}>{CURSOR_LABEL[style]}</option>
          {/each}
        </select>
      </div>
      <div class="setting-row">
        <label for="setting-blink">Blinking cursor</label>
        <input
          id="setting-blink"
          type="checkbox"
          disabled={!editable}
          checked={prefs.terminal.cursor_blink}
          onchange={blink}
        />
      </div>
      <p class="setting-hint">
        These apply to every terminal, open or new. What a terminal runs is not
        a preference.
      </p>
    </section>

    <section class="settings-section" aria-labelledby="settings-events">
      <h3 id="settings-events">Events</h3>
      <div class="setting-row">
        <label for="setting-time">Time</label>
        <select
          id="setting-time"
          disabled={!editable}
          value={prefs.events.time_format}
          onchange={time}
        >
          {#each TIME_FORMATS as format (format)}
            <option value={format}>{TIME_LABEL[format]}</option>
          {/each}
        </select>
      </div>
      <div class="setting-row">
        <label for="setting-filter">Show first</label>
        <select
          id="setting-filter"
          disabled={!editable}
          value={prefs.events.default_filter}
          onchange={filter}
        >
          {#each EVENT_FILTER_PREFERENCES as value (value)}
            <option {value}>{FILTER_PREFERENCE_LABEL[value]}</option>
          {/each}
        </select>
        <p class="setting-hint">
          Only narrows what is shown. The authority records every event either
          way.
        </p>
      </div>
    </section>

    <section class="settings-section" aria-labelledby="settings-authority">
      <h3 id="settings-authority">Authority</h3>
      <dl class="settings-facts">
        {#each facts as fact (fact.term)}
          <dt>{fact.term}</dt>
          <dd>
            {#if fact.tone}<span
                class="dot"
                data-tone={fact.tone}
                aria-hidden="true"
              ></span>{/if}
            {fact.value}
          </dd>
        {/each}
      </dl>
      {#if canRetry}
        <div class="settings-actions">
          <button
            type="button"
            class="action"
            disabled={retrying}
            onclick={onretry}>{retrying ? 'Retrying…' : 'Retry'}</button
          >
        </div>
      {/if}
      <p class="setting-hint">
        The authority keeps your sessions, approvals and history, and keeps
        running when this window is closed. Local terminals and these
        preferences work without it.
      </p>
    </section>

    <section class="settings-section" aria-labelledby="settings-about">
      <h3 id="settings-about">About</h3>
      <dl class="settings-facts">
        <dt>Wyss Console</dt>
        <dd>{about ? `Version ${about.version}` : 'Not available'}</dd>
        {#if about?.revision}
          <dt>Build</dt>
          <dd class="mono">{about.revision}</dd>
        {/if}
        <dt>Platform</dt>
        <dd>{about ? platformLabel(about.platform) : 'Not available'}</dd>
        {#if protocol}
          <dt>Authority protocol</dt>
          <dd>{protocol}</dd>
        {/if}
      </dl>
    </section>

    <section class="settings-section" aria-labelledby="settings-reset">
      <h3 id="settings-reset">Reset</h3>
      {#if settings.confirmingReset}
        <div
          class="close-confirm"
          role="alertdialog"
          aria-labelledby="reset-q"
          bind:this={resetConfirm}
        >
          <p id="reset-q">Reset all preferences to their defaults?</p>
          <p class="setting-hint">
            Your sessions, history, approvals, workspaces and Codex login are
            not touched.
          </p>
          <div class="settings-actions">
            <button
              type="button"
              class="action danger"
              onclick={() => controller.confirmReset()}
              >Reset preferences</button
            >
            <button
              type="button"
              class="action"
              data-reset-cancel
              onclick={() => controller.cancelReset()}>Cancel</button
            >
          </div>
        </div>
      {:else}
        <div class="settings-actions">
          <button
            type="button"
            class="action"
            disabled={!ready || settings.saving}
            onclick={() => controller.askReset()}>Reset preferences…</button
          >
        </div>
        <p class="setting-hint">
          Restores the defaults on this screen. Nothing else is deleted.
        </p>
      {/if}
    </section>
    {@render children?.()}
  </div>
</section>
