<script lang="ts">
  import type { SessionRow } from '../../contracts/f8';
  import { localClock } from '../agent/present';
  import { t, tPhrase } from '../../i18n/index.svelte';
  import { shortId } from '../sessions/present';
  import type { F8Controller, F8Tab, F8ViewState } from './controller';
  import ConversationPanel from './ConversationPanel.svelte';
  import HistoryPanel from './HistoryPanel.svelte';
  import ModelsPanel from './ModelsPanel.svelte';
  import OverviewPanel from './OverviewPanel.svelte';
  import {
    failureText,
    linkText,
    outcomeUnknown,
    RUNTIME_STATE,
    runtimeLine,
  } from './present';

  let {
    f8,
    controller,
    visible,
  }: {
    f8: F8ViewState;
    controller: F8Controller;
    visible: boolean;
  } = $props();

  const connected = $derived(f8.link?.state === 'connected');
  const link = $derived(f8.link ? linkText(f8.link) : null);
  const page = $derived(f8.sessions);
  const detail = $derived(f8.detail);
  const selected = $derived(
    detail && detail.session.id === f8.selected ? detail : null,
  );
  const pending = $derived(f8.operation?.status === 'pending');
  const TABS: readonly { id: F8Tab; label: string }[] = [
    { id: 'overview', label: 'Overview' },
    { id: 'conversation', label: 'Conversation' },
    { id: 'models', label: 'Models' },
    { id: 'history', label: 'History' },
  ];
  const name = (row: SessionRow) => `Session ${shortId(row.id)}`;

  /** Left and Right move between tabs, as a tab list does. */
  function tabKey(event: KeyboardEvent, index: number) {
    const step =
      event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
    if (step === 0 && event.key !== 'Home' && event.key !== 'End') return;
    event.preventDefault();
    const next =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? TABS.length - 1
          : (index + step + TABS.length) % TABS.length;
    controller.setTab(TABS[next].id);
    const bar = (event.currentTarget as HTMLElement).parentElement;
    (bar?.children[next] as HTMLElement | undefined)?.focus();
  }

  const OPERATION: Record<string, string> = {
    prepare: 'Creating the session…',
    close: 'Closing the session…',
    start: 'Starting the runtime…',
    stop: 'Stopping the runtime…',
    grant: 'Granting the execution lease…',
    renew: 'Renewing the execution lease…',
    revoke: 'Revoking the execution lease…',
    bind: 'Binding the workspace…',
    resume: 'Attaching the session…',
    abandon: 'Discarding the attempt…',
    turn: 'Asking the authority to start the turn…',
    cancel: 'Asking the authority to cancel the turn…',
  };
</script>

<section
  class="runtimes-feature"
  aria-label="Managed runtimes"
  hidden={!visible}
  inert={!visible}
>
  <div class="sessions-toolbar">
    <div class="sessions-heading">
      <h2>Managed runtimes · protocol 2</h2>
      <p class="sessions-count">
        {#if f8.authority && connected}
          {f8.authority.prepared_count} prepared · {f8.authority.closed_count} closed
          · protocol 2
        {:else if link}
          {t(link.label)}
        {:else}
          Connecting to the authority
        {/if}
      </p>
    </div>
    {#if link}
      <span class="pill compact" data-tone={link.tone}>
        <span class="dot" aria-hidden="true"></span>
        {t(link.label)}
      </span>
    {/if}
    <button
      type="button"
      class="action"
      disabled={f8.reading}
      onclick={() => controller.refresh()}>Refresh</button
    >
    <button
      type="button"
      class="action primary"
      disabled={!connected || pending}
      onclick={() => controller.prepare()}>New session</button
    >
  </div>

  {#if f8.notice === 'authority_changed'}
    <div class="sessions-status failed unknown" role="alert">
      <p>
        The authority restarted or was replaced. Its runtimes, leases and turns
        are not the ones shown before, so everything was read again and nothing
        from before is acted on.
      </p>
      <button
        type="button"
        class="action"
        onclick={() => controller.dismissNotice()}>Dismiss</button
      >
    </div>
  {:else if f8.notice === 'stale'}
    <div class="sessions-status failed" role="alert">
      <p>{t(failureText('stale'))}</p>
      <button
        type="button"
        class="action"
        onclick={() => controller.dismissNotice()}>Dismiss</button
      >
    </div>
  {/if}

  {#if f8.operation?.status === 'pending'}
    <p class="sessions-status" role="status">
      <span class="dot" aria-hidden="true"></span>
      {f8.operation.step === 'starting'
        ? 'Waiting for the runtime to be ready…'
        : f8.operation.step === 'stopping'
          ? 'Waiting for the runtime to stop…'
          : f8.operation.step === 'binding'
            ? 'The authority is binding the workspace…'
            : f8.operation.step === 'confirming'
              ? 'Waiting for your answer in the confirmation window. Nothing is allowed until you choose Allow. If you cannot see it, it may be behind another window.'
              : OPERATION[f8.operation.kind]}
    </p>
  {:else if f8.operation?.failure}
    <div
      class="sessions-status failed"
      class:unknown={outcomeUnknown(f8.operation.failure.code)}
      role="alert"
    >
      <p>{t(failureText(f8.operation.failure.code))}</p>
      {#if f8.operation.failure.retry}
        <button
          type="button"
          class="action"
          disabled={!connected}
          onclick={() => controller.retry()}>Retry</button
        >
      {/if}
      <button
        type="button"
        class="action"
        onclick={() => controller.dismissOperation()}>Dismiss</button
      >
    </div>
  {/if}

  {#if f8.link && f8.link.state !== 'connected'}
    <div class="sessions-status failed unknown" role="status">
      <p>
        {#if f8.link.state === 'waiting'}
          {link?.note ? t(link.note) : 'The authority is not answering.'}
          {f8.link.retry_in_ms !== null
            ? 'The console asks again by itself.'
            : ''}
          What is shown was read before the connection was lost; no action is offered.
        {:else}
          Connecting to the authority…
        {/if}
      </p>
      {#if f8.link.state === 'waiting'}
        <button
          type="button"
          class="action"
          onclick={() => controller.reconnect()}>Reconnect now</button
        >
      {/if}
    </div>
  {/if}

  {#if !page && f8.readFailure && !connected}
    <div class="sessions-empty" role="status">
      <p class="empty-title">The authority is unavailable</p>
      <p>
        Managed runtimes are read from the authority and cannot be created,
        started or stopped until it is connected. The Console keeps working.
      </p>
    </div>
  {:else if !page && f8.readFailure}
    <div class="sessions-empty" role="alert">
      <p class="empty-title">{t(failureText(f8.readFailure.code))}</p>
      <button type="button" class="action" onclick={() => controller.refresh()}
        >Try again</button
      >
    </div>
  {:else if !page}
    <div class="sessions-empty" role="status" aria-busy="true">
      <p class="empty-title">Reading managed sessions…</p>
    </div>
  {:else if page.sessions.length === 0 && f8.depth === 0}
    <div class="sessions-empty">
      <p class="empty-title">No managed sessions yet.</p>
      <p>
        A session gets a runtime of its own. Create one, start its runtime, give
        it an execution lease and bind a project folder to work in.
      </p>
      <button
        type="button"
        class="action primary"
        disabled={!connected || pending}
        onclick={() => controller.prepare()}>New session</button
      >
    </div>
  {:else}
    <div class="runtimes-body">
      <div class="runtimes-list sessions-main">
        <ul class="sessions-list" aria-label="Managed sessions">
          {#each page.sessions as row (row.id)}
            <li>
              <button
                type="button"
                class="session-row"
                aria-pressed={row.id === f8.selected}
                onclick={() =>
                  controller.select(row.id === f8.selected ? null : row.id)}
              >
                <span class="session-name">{name(row)}</span>
                <span
                  class="pill compact"
                  data-tone={row.state === 'closed'
                    ? 'idle'
                    : RUNTIME_STATE[row.runtime.actual].tone}
                >
                  <span class="dot" aria-hidden="true"></span>
                  {row.state === 'closed'
                    ? 'Closed'
                    : t(RUNTIME_STATE[row.runtime.actual].label)}
                </span>
                <span class="session-meta"
                  >{tPhrase(runtimeLine(row.runtime))}</span
                >
                <span class="session-meta">
                  Updated <time datetime={row.updated_at}
                    >{localClock(row.updated_at)}</time
                  >
                </span>
              </button>
            </li>
          {/each}
        </ul>
        {#if f8.depth > 0 || page.older}
          <div class="sessions-pager">
            <p>
              {f8.depth === 0
                ? 'Newest sessions'
                : `Older sessions · page ${f8.depth + 1}`}
            </p>
            <button
              type="button"
              class="action"
              disabled={f8.depth === 0 || f8.reading}
              onclick={() => controller.newer()}>Newer</button
            >
            <button
              type="button"
              class="action"
              disabled={!page.older || f8.reading}
              onclick={() => controller.older()}>Older</button
            >
          </div>
        {/if}
      </div>

      <div class="runtimes-detail" aria-label="Session details">
        {#if !f8.selected}
          <div class="sessions-empty">
            <p class="empty-title">Select a session</p>
            <p>
              Its runtime, workspace, execution lease, conversation, models and
              history are shown here, exactly as the authority holds them.
            </p>
          </div>
        {:else if !selected}
          {#if f8.readFailure}
            <div class="sessions-empty" role="alert">
              <p class="empty-title">{t(failureText(f8.readFailure.code))}</p>
              <button
                type="button"
                class="action"
                onclick={() => controller.refresh()}>Try again</button
              >
            </div>
          {:else}
            <div class="sessions-empty" role="status" aria-busy="true">
              <p class="empty-title">Reading the session…</p>
            </div>
          {/if}
        {:else}
          <header class="rt-head">
            <h3>{name(selected.session)}</h3>
            <span
              class="pill compact"
              data-tone={selected.session.state === 'closed'
                ? 'idle'
                : RUNTIME_STATE[selected.session.runtime.actual].tone}
            >
              <span class="dot" aria-hidden="true"></span>
              {selected.session.state === 'closed'
                ? 'Closed'
                : t(RUNTIME_STATE[selected.session.runtime.actual].label)}
            </span>
            <span class="rt-id mono" title={selected.session.id}
              >{selected.session.id}</span
            >
            <button
              type="button"
              class="details-close"
              aria-label="Hide details"
              onclick={() => controller.select(null)}>×</button
            >
          </header>
          <div class="rt-tabs" role="tablist" aria-label="Session sections">
            {#each TABS as tab, index (tab.id)}
              <button
                type="button"
                role="tab"
                id={`rt-tab-${tab.id}`}
                aria-selected={f8.tab === tab.id}
                aria-controls={`rt-panel-${tab.id}`}
                tabindex={f8.tab === tab.id ? 0 : -1}
                onclick={() => controller.setTab(tab.id)}
                onkeydown={(event) => tabKey(event, index)}>{tab.label}</button
              >
            {/each}
          </div>
          <div
            class="rt-panel"
            role="tabpanel"
            id={`rt-panel-${f8.tab}`}
            aria-labelledby={`rt-tab-${f8.tab}`}
          >
            {#if f8.tab === 'overview'}
              <OverviewPanel
                {f8}
                detail={selected}
                {controller}
                {connected}
                {pending}
              />
            {:else if f8.tab === 'conversation'}
              <ConversationPanel
                {f8}
                detail={selected}
                {controller}
                {connected}
                {pending}
              />
            {:else if f8.tab === 'models'}
              <ModelsPanel {f8} detail={selected} {controller} />
            {:else}
              <HistoryPanel {f8} {controller} {connected} />
            {/if}
          </div>
        {/if}
      </div>
    </div>
  {/if}
</section>
