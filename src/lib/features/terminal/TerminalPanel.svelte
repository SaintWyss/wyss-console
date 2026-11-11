<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type {
    TerminalSession,
    TerminalSessionManager,
  } from './session-manager';
  import { failureWords, inputNoticeWords } from './failure-words';
  import { parserNote } from './parser-status';
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
          title="Started by the Console in your shell. Not mediated, contained or approved by the Console."
          ><span class="dot" aria-hidden="true"></span>Native</span
        >
        {session.native.agent}{session.native.profile
          ? ` · ${session.native.profile}`
          : ''} ·
      {/if}
      {#if session.view.state === 'starting'}Starting {session.native
          ? 'agent'
          : 'terminal'}…
      {:else if session.view.state === 'running'}{session.native
          ? 'Running'
          : `${session.view.info?.shell} · Running`}
      {:else if session.view.state === 'exited'}{session.native
          ? 'Agent'
          : 'Shell'} exited{session.view.info?.exit
          ? ` (${session.view.info.exit.code})`
          : ''}
      {:else if session.view.state === 'closing'}Closing terminal…
      {:else if session.view.state === 'closed'}Terminal closed
      {:else}<span title={session.view.error ?? 'terminal_error'}
          >Terminal failed: {failureWords(session.view.error)}</span
        >{/if}
      {#if session.view.truncated}
        · Output truncated{/if}
    </p>
    {#if note}
      <p class="terminal-parser" data-tone={note.tone} title={note.title}>
        {note.text}
      </p>
    {/if}
  </div>
  {#if session.view.inputNotice}
    <p class="terminal-input-notice" role="alert">
      {inputNoticeWords(session.view.inputNotice)}
    </p>
  {/if}
  {#if searching}
    <div class="terminal-search" role="search">
      <input
        bind:this={field}
        type="text"
        aria-label="Search this terminal"
        placeholder="Search…"
        bind:value={query}
        oninput={() => find(0)}
        onkeydown={key}
      />
      <span class="terminal-search-count" role="status">
        {#if query === ''}{:else if result.total === 0}No matches{:else}{result.current}
          of {result.total}{result.total >= 5000 ? '+' : ''}{/if}
      </span>
      <button type="button" aria-label="Previous match" onclick={() => find(-1)}
        >↑</button
      >
      <button type="button" aria-label="Next match" onclick={() => find(1)}
        >↓</button
      >
      <button
        type="button"
        aria-pressed={caseSensitive}
        title="Match case"
        onclick={() => {
          caseSensitive = !caseSensitive;
          find(0);
        }}>Aa</button
      >
      <button type="button" aria-label="Close search" onclick={close}>×</button>
    </div>
  {/if}
  <div class="terminal-surface">
    <div bind:this={host} class="terminal-host"></div>
  </div>
</div>
