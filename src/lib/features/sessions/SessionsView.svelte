<script lang="ts">
  import {
    summarizeAuthority,
    type AuthorityObservation,
  } from '../../shell/authority';
  import type { SessionsController, SessionsState } from './controller';
  import {
    agentName,
    BINDING,
    failureMessage,
    localTime,
    PENDING,
    readFailureMessage,
    resumeHint,
    sessionLabel,
    STATE,
  } from './present';

  let {
    sessions,
    controller,
    authority,
    visible,
  }: {
    sessions: SessionsState;
    controller: SessionsController;
    /** The shell's one authority observation. Read here for wording only. */
    authority: AuthorityObservation;
    visible: boolean;
  } = $props();

  const connected = $derived(sessions.link === 'connected');
  const pending = $derived(sessions.operation?.status === 'pending');
  const page = $derived(sessions.page);
  const selected = $derived(
    page?.sessions.find((s) => s.id === sessions.selected),
  );
  const counts = $derived(
    authority.state === 'observed' ? authority.view.authority : null,
  );
  const hint = $derived(
    selected ? resumeHint(selected, sessions.agentReady) : null,
  );
  const acting = (id: string) => pending && sessions.operation?.session === id;
</script>

<section
  class="sessions-feature"
  aria-label="Managed sessions"
  hidden={!visible}
  inert={!visible}
>
  <div class="sessions-toolbar">
    <div class="sessions-heading">
      <h2>Managed sessions</h2>
      <p class="sessions-count">
        {#if connected && counts}
          {counts.prepared_sessions} prepared · {counts.closed_sessions} closed
        {:else if sessions.link === 'connecting'}
          Connecting to the authority
        {:else}
          Authority unavailable
        {/if}
      </p>
    </div>
    <button
      type="button"
      class="action"
      disabled={!connected || sessions.reading}
      onclick={() => controller.refresh()}>Refresh</button
    >
    <button
      type="button"
      class="action primary"
      disabled={!connected || pending || sessions.composing}
      onclick={() => controller.compose(true)}>New session</button
    >
  </div>

  {#if sessions.operation?.status === 'pending'}
    <p class="sessions-status" role="status">
      <span class="dot" aria-hidden="true"></span>
      {PENDING[sessions.operation.kind]}
    </p>
  {:else if sessions.operation?.failure}
    <div class="sessions-status failed" role="alert">
      <p>
        {failureMessage(
          sessions.operation.kind,
          sessions.operation.failure.code,
        )}
      </p>
      {#if sessions.operation.failure.retry}
        <button
          type="button"
          class="action"
          disabled={!connected}
          onclick={() => controller.retry()}>Retry</button
        >
      {/if}
      <button type="button" class="action" onclick={() => controller.dismiss()}
        >Dismiss</button
      >
    </div>
  {/if}

  {#if sessions.composing}
    <div class="session-composer" role="group" aria-label="New managed session">
      <div class="composer-text">
        <p class="composer-title">New managed session</p>
        <p class="composer-note">
          Agent: Codex. wyss-control prepares the session and keeps its record.
        </p>
      </div>
      <button
        type="button"
        class="action primary"
        disabled={!connected || pending}
        onclick={() => controller.prepare()}>Create session</button
      >
      <button
        type="button"
        class="action"
        onclick={() => controller.compose(false)}>Cancel</button
      >
    </div>
  {/if}

  {#if sessions.link === 'connecting'}
    <div class="sessions-empty" role="status">
      <p class="empty-title">Connecting to wyss-control…</p>
    </div>
  {:else if !connected}
    <div class="sessions-empty" role="status">
      <p class="empty-title">wyss-control is unavailable</p>
      <p>{summarizeAuthority(authority).detail}</p>
      <p>
        Sessions are read from the authority and cannot be created, resumed or
        closed until it is connected. The Console keeps working.
      </p>
    </div>
  {:else if !page && sessions.readFailure}
    <div class="sessions-empty" role="alert">
      <p class="empty-title">{readFailureMessage(sessions.readFailure.code)}</p>
      <button type="button" class="action" onclick={() => controller.refresh()}
        >Try again</button
      >
    </div>
  {:else if !page}
    <div class="sessions-empty" role="status" aria-busy="true">
      <p class="empty-title">Reading sessions…</p>
    </div>
  {:else if page.sessions.length === 0 && sessions.depth === 0}
    <div class="sessions-empty">
      <p class="empty-title">No managed sessions yet.</p>
      <p>Create a session to let Wyss supervise an agent workspace.</p>
      <button
        type="button"
        class="action primary"
        disabled={pending || sessions.composing}
        onclick={() => controller.compose(true)}>New session</button
      >
    </div>
  {:else}
    <div class="sessions-body" class:has-details={!!selected}>
      <div class="sessions-main">
        <ul class="sessions-list" aria-label="Sessions">
          {#each page.sessions as session (session.id)}
            <li>
              <button
                type="button"
                class="session-row"
                aria-pressed={session.id === sessions.selected}
                aria-busy={acting(session.id)}
                onclick={() =>
                  controller.select(
                    session.id === sessions.selected ? null : session.id,
                  )}
              >
                <span class="session-name">{sessionLabel(session)}</span>
                <span
                  class="pill compact"
                  data-tone={STATE[session.state].tone}
                >
                  <span class="dot" aria-hidden="true"></span>
                  {STATE[session.state].label}
                </span>
                <span class="session-meta">
                  {agentName(session.adapter)} ·
                  {BINDING[session.binding].label}
                  {#if session.workspace}
                    · <span class="session-path">{session.workspace}</span>
                  {/if}
                </span>
                <span class="session-meta">
                  Updated <time datetime={session.updated_at}
                    >{localTime(session.updated_at)}</time
                  >
                </span>
              </button>
            </li>
          {/each}
        </ul>
        {#if sessions.depth > 0 || page.older || page.truncated}
          <div class="sessions-pager">
            <p>
              {#if page.truncated}
                Only the first 2,000 sessions can be listed.
              {:else if sessions.depth === 0}
                Newest sessions
              {:else}
                Older sessions · page {sessions.depth + 1}
              {/if}
            </p>
            <button
              type="button"
              class="action"
              disabled={sessions.depth === 0 || sessions.reading}
              onclick={() => controller.newer()}>Newer</button
            >
            <button
              type="button"
              class="action"
              disabled={!page.older || sessions.reading}
              onclick={() => controller.older()}>Older</button
            >
          </div>
        {/if}
      </div>

      {#if selected}
        <aside class="session-details" aria-label="Session details">
          <div class="details-head">
            <h3>{sessionLabel(selected)}</h3>
            <span class="pill compact" data-tone={STATE[selected.state].tone}>
              <span class="dot" aria-hidden="true"></span>
              {STATE[selected.state].label}
            </span>
          </div>
          {#if sessions.confirmingClose === selected.id}
            <div class="close-confirm" role="group" aria-label="Confirm close">
              <p>
                Close this session? It cannot be reopened, and its execution
                lease is revoked.
              </p>
              <div class="session-actions">
                <button
                  type="button"
                  class="action danger"
                  disabled={!connected || pending}
                  onclick={() => controller.close(selected.id)}
                  >Close session</button
                >
                <button
                  type="button"
                  class="action"
                  onclick={() => controller.confirmClose(null)}
                  >Keep open</button
                >
              </div>
            </div>
          {:else}
            <div class="session-actions">
              <button
                type="button"
                class="action primary"
                disabled={!connected || pending || hint !== null}
                onclick={() => controller.resume(selected.id)}>Resume</button
              >
              <button
                type="button"
                class="action"
                disabled={!connected || pending || !selected.closable}
                onclick={() => controller.confirmClose(selected.id)}
                >Close</button
              >
            </div>
            {#if hint}<p class="session-hint">{hint}</p>{/if}
          {/if}

          <dl class="session-facts">
            <dt>Agent</dt>
            <dd>{agentName(selected.adapter)}</dd>
            <dt>Runtime</dt>
            <dd>
              {BINDING[selected.binding].label}
              <span class="fact-note">{BINDING[selected.binding].detail}</span>
            </dd>
            <dt>Workspace</dt>
            <dd class:mono={!!selected.workspace}>
              {selected.workspace ?? 'None'}
            </dd>
            <dt>Revision</dt>
            <dd>{selected.revision}</dd>
            <dt>Created</dt>
            <dd>
              <time datetime={selected.created_at}
                >{localTime(selected.created_at)}</time
              >
            </dd>
            <dt>Updated</dt>
            <dd>
              <time datetime={selected.updated_at}
                >{localTime(selected.updated_at)}</time
              >
            </dd>
            <dt>Session</dt>
            <dd class="mono">{selected.id}</dd>
          </dl>
        </aside>
      {/if}
    </div>
  {/if}
</section>
