<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type {
    TerminalSession,
    TerminalSessionManager,
  } from './session-manager';
  import { failureWords, inputNoticeWords } from './failure-words';
  import { parserNote } from './parser-status';
  import { t } from '../../i18n/index.svelte';
  let {
    session,
    tabId,
    visible,
    focused,
    place,
    searching,
    manager,
    onsearchclose,
  }: {
    session: TerminalSession;
    /** The tab the pane belongs to: the panel is labelled by it. */
    tabId: string;
    /** In the active tab: drawn. */
    visible: boolean;
    /** The pane the keyboard goes to. */
    focused: boolean;
    /** Where the pane sits in its tab, as fractions of the area. */
    place: { x: number; y: number; w: number; h: number };
    searching: boolean;
    manager: TerminalSessionManager;
    onsearchclose: () => void;
  } = $props();
  let host: HTMLDivElement;
  const note = $derived(parserNote(session.view));
  let query = $state('');
  let caseSensitive = $state(false);
  let result = $state({ total: 0, current: 0 });
  let field = $state<HTMLInputElement>();
  onMount(() => {
    manager.attach(session.id, host);
  });
  function find(step: 1 | -1 | 0) {
    result = manager.search(session.id, query, { caseSensitive, step });
  }
  $effect(() => {
    if (searching) void tick().then(() => field?.select());
  });
  function close() {
    manager.clearSearch(session.id);
    result = { total: 0, current: 0 };
    onsearchclose();
  }
  function key(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      close();
    } else if (event.key === 'Enter') {
      event.preventDefault();
      find(event.shiftKey ? -1 : 1);
    }
  }
</script>

<div
  tabindex="-1"
  class="terminal-panel"
  class:focused
  data-state={session.view.state}
  role="tabpanel"
  id={`panel-${session.id}`}
  aria-labelledby={`tab-${tabId}`}
  hidden={!visible}
  inert={!visible}
  style:left={`${place.x * 100}%`}
  style:top={`${place.y * 100}%`}
  style:width={`${place.w * 100}%`}
  style:height={`${place.h * 100}%`}
  onpointerdowncapture={() => {
    if (!focused) manager.activate(session.id);
  }}
>
  <div class="terminal-toolbar">
    <p role="status" aria-live={focused ? 'polite' : 'off'}>
      <span class="terminal-state-dot" aria-hidden="true"></span>
      {#if session.native}
        <span
          class="pill compact"
          data-tone="idle"
          title={t(
            'Started by the Console in your shell. Not mediated, contained or approved by the Console.',
          )}><span class="dot" aria-hidden="true"></span>Native</span
        >
        {session.native.agent}{session.native.profile
          ? ` · ${session.native.profile}`
          : ''} ·
      {/if}
      {#if session.view.state === 'starting'}{session.native
          ? t('Starting agent…')
          : t('Starting terminal…')}
      {:else if session.view.state === 'running'}{session.native
          ? t('Running')
          : `${session.view.info?.shell} · ${t('Running')}`}
      {:else if session.view.state === 'exited'}{session.native
          ? t('Agent exited')
          : t('Shell exited')}{session.view.info?.exit
          ? ` (${session.view.info.exit.code})`
          : ''}
      {:else if session.view.state === 'closing'}{t('Closing terminal…')}
      {:else if session.view.state === 'closed'}{t('Terminal closed')}
      {:else}<span title={session.view.error ?? 'terminal_error'}
          >{t('Terminal failed: {reason}', {
            reason: t(failureWords(session.view.error)),
          })}</span
        >{/if}
      {#if session.view.truncated}
        · {t('Output truncated')}{/if}
    </p>
    {#if note}
      <p
        class="terminal-parser"
        data-tone={note.tone}
        title={t(note.title, note.values)}
      >
        {t(note.text, note.values)}
      </p>
    {/if}
  </div>
  {#if session.view.inputNotice}
    <p class="terminal-input-notice" role="alert">
      {t(inputNoticeWords(session.view.inputNotice))}
    </p>
  {/if}
  {#if searching}
    <div class="terminal-search" role="search">
      <input
        bind:this={field}
        type="text"
        aria-label={t('Search this terminal')}
        placeholder={t('Search…')}
        bind:value={query}
        oninput={() => find(0)}
        onkeydown={key}
      />
      <span class="terminal-search-count" role="status">
        {#if query === ''}{:else if result.total === 0}{t(
            'No matches',
          )}{:else}{t('{current} of {total}{suffix}', {
            current: result.current,
            total: result.total,
            suffix: result.total >= 5000 ? '+' : '',
          })}{/if}
      </span>
      <button
        type="button"
        aria-label={t('Previous match')}
        onclick={() => find(-1)}>↑</button
      >
      <button type="button" aria-label={t('Next match')} onclick={() => find(1)}
        >↓</button
      >
      <button
        type="button"
        aria-pressed={caseSensitive}
        title={t('Match case')}
        onclick={() => {
          caseSensitive = !caseSensitive;
          find(0);
        }}>Aa</button
      >
      <button type="button" aria-label={t('Close search')} onclick={close}
        >×</button
      >
    </div>
  {/if}
  <div class="terminal-surface">
    <div bind:this={host} class="terminal-host"></div>
  </div>
</div>
