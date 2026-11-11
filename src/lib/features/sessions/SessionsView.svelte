<script lang="ts">
  import {
    summarizeAuthority,
    type AuthorityObservation,
  } from '../../shell/authority';
  import { tJoin } from '../../i18n/index.svelte';
  import type { SessionSummary } from '../../contracts/sessions';
  import type { SessionsController, SessionsState } from './controller';
  import { AGENT_MODES } from '../../contracts/sessions';
  import {
    AGENT_MODE,
    agentName,
    BINDING,
    chooseFailureMessage,
    failureMessage,
    localTime,
    composeImpact,
    modeImpact,
    modeLabel,
    observedModelLine,
    modeOffers,
    outcomeUnknown,
    pendingMessage,
    readFailureMessage,
    resumeHint,
    runtimeLine,
    sessionNames,
    STATE,
  } from './present';

  let {
    sessions,
    controller,
    authority,
    visible,
    onagent,
    onevents,
  }: {
    sessions: SessionsState;
    controller: SessionsController;
    /** The shell's one authority observation. Read here for wording only. */
    authority: AuthorityObservation;
    visible: boolean;
    /** Opens a session in the Agent view. Offered for a session with a workspace. */
    onagent?: (session: SessionSummary) => void;
    /** Shows the loaded history of a session in Events. */
    onevents?: (session: SessionSummary) => void;
  } = $props();

  const connected = $derived(sessions.link === 'connected');
  /** Unique among the sessions listed, so two sessions of one project can be told apart. */
  const names = $derived(sessionNames(sessions.page?.sessions ?? []));
  const nameOf = (session: SessionSummary) =>
    names.get(session.id) ?? session.id.slice(0, 8);
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
  /** The existing session the composer activates, when it is not creating one. */
  const target = $derived(
    page?.sessions.find((s) => s.id === sessions.composeFor),
  );
  const acting = (id: string) => pending && sessions.operation?.session === id;
  /** The one managed runtime every session shares, as the page read it. */
  const runtime = $derived(page?.runtime ?? { mode: null, workspace: null });
  const offers = $derived(selected ? modeOffers(selected, runtime) : []);
  const running = $derived(page ? runtimeLine(page.runtime) : null);
  /** What the composer's action does to the runtime that runs now, when it is known. */
  const restart = $derived(
    page && sessions.composing
      ? composeImpact(sessions.mode, sessions.workspace, page.runtime)
      : null,
  );
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
          {#if running}· {running}{/if}
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
      {pendingMessage(
        sessions.operation.kind,
        sessions.operation.step,
        sessions.operation.mode,
      )}
    </p>
  {:else if sessions.operation?.failure}
    <div
      class="sessions-status failed"
      class:unknown={outcomeUnknown(sessions.operation.failure.code)}
      role="alert"
    >
      <p>
        {failureMessage(
          sessions.operation.kind,
          sessions.operation.failure.code,
          sessions.operation.mode,
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
    <div
      class="session-composer"
      role="group"
      aria-label={target ? 'Activate session' : 'New managed session'}
    >
      <div class="composer-text">
        <p class="composer-title">
          {target ? `Activate ${nameOf(target)}` : 'New managed session'}
        </p>
        <dl class="composer-fields">
          <dt>Project</dt>
          <dd class:mono={!!sessions.workspace}>
            {sessions.workspace ?? 'No folder selected'}
          </dd>
          <dt>Agent</dt>
          <dd>Codex</dd>
        </dl>
        <!-- An explicit choice, every time: Inference unless the user says otherwise. -->
        <fieldset class="mode-choice">
          <legend>Mode</legend>
          {#each AGENT_MODES as mode (mode)}
            <label class="mode-option">
              <input
                type="radio"
                name="agent-mode"
                value={mode}
                checked={sessions.mode === mode}
                disabled={pending}
                onchange={() => controller.setMode(mode)}
              />
              <span class="mode-name">{AGENT_MODE[mode].label}</span>
              <span class="mode-detail">{AGENT_MODE[mode].detail}</span>
            </label>
          {/each}
        </fieldset>
        <p class="composer-note">
          The authority decides whether the folder can be a workspace, starts
          Codex if it is not running, and binds the session to it. A session
          keeps the mode it is created in.
        </p>
        {#if sessions.mode === 'local_tools'}
          <p class="composer-note">
            Local Tools gives Codex more to work with, not more authority: every
            command and file change waits for your approval. The authority runs
            one Codex runtime, in one mode and for one folder at a time.
          </p>
        {/if}
        {#if restart}
          <p class="composer-note caution" role="note">{restart}</p>
        {/if}
        {#if sessions.chooseFailure}
          <p class="composer-note failed" role="alert">
            {chooseFailureMessage(sessions.chooseFailure.code)}
          </p>
        {/if}
      </div>
      <button
        type="button"
        class="action"
        disabled={!connected || pending || sessions.choosing}
        aria-busy={sessions.choosing}
        onclick={() => controller.chooseWorkspace()}
        >{sessions.workspace ? 'Change folder' : 'Choose folder'}</button
      >
      <button
        type="button"
        class="action primary"
        disabled={!connected ||
          pending ||
          sessions.choosing ||
          !sessions.workspace}
        onclick={() => controller.activate()}
        >{restart
          ? 'Restart Codex & activate'
          : target
            ? 'Activate'
            : 'Create & activate'}</button
      >
      <button
        type="button"
        class="action"
        disabled={sessions.choosing}
        onclick={() => controller.compose(false)}>Cancel</button
      >
    </div>
  {/if}

  {#if sessions.link === 'connecting'}
    <div class="sessions-empty" role="status">
      <p class="empty-title">Connecting to the authority…</p>
    </div>
  {:else if !connected}
    <div class="sessions-empty" role="status">
      <p class="empty-title">The authority is unavailable</p>
      <p>{tJoin(summarizeAuthority(authority).detail)}</p>
      <p>
        Sessions are read from the authority and cannot be created, activated,
        resumed or closed until it is connected. The Console keeps working.
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
      <p>
        Create a session to let Wyss supervise Codex in one of your projects.
      </p>
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
                <span class="session-name">{nameOf(session)}</span>
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
                  {#if session.mode !== 'unknown'}
                    · {modeLabel(session.mode)}
                  {/if}
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
            <h3>{nameOf(selected)}</h3>
            <span class="pill compact" data-tone={STATE[selected.state].tone}>
              <span class="dot" aria-hidden="true"></span>
              {STATE[selected.state].label}
            </span>
            <button
              type="button"
              class="details-close"
              aria-label="Hide details"
              onclick={() => controller.select(null)}>×</button
            >
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
          {:else if sessions.confirmingDiscard === selected.id}
            <div
              class="close-confirm"
              role="group"
              aria-label="Confirm discard"
            >
              <p>
                Discard the uncertain attempt? The authority records that it was
                abandoned and stops holding the session for it. If Codex did
                create a thread for that attempt, the empty thread is left
                behind. The session can then be activated again.
              </p>
              <div class="session-actions">
                <button
                  type="button"
                  class="action danger"
                  disabled={!connected || pending}
                  onclick={() => controller.discard(selected.id)}
                  >Discard attempt</button
                >
                <button
                  type="button"
                  class="action"
                  onclick={() => controller.confirmDiscard(null)}
                  >Keep it</button
                >
              </div>
            </div>
          {:else if sessions.confirmingMode?.session === selected.id}
            {@const asked = sessions.confirmingMode.mode}
            <div
              class="close-confirm mode-confirm"
              role="group"
              aria-label="Confirm mode"
            >
              <p>
                {asked === 'local_tools'
                  ? 'Enable Local Tools'
                  : 'Use Inference'}
                for {nameOf(selected)}? {modeImpact(asked)}
              </p>
              <div class="session-actions">
                <button
                  type="button"
                  class="action primary"
                  disabled={!connected || pending}
                  onclick={() => controller.enterMode()}
                  >{asked === 'local_tools'
                    ? 'Enable Local Tools'
                    : 'Use Inference'}</button
                >
                <button
                  type="button"
                  class="action"
                  onclick={() => controller.confirmMode(null)}>Cancel</button
                >
              </div>
            </div>
          {:else}
            {#if selected.recoverable}
              <div class="session-uncertain" role="status">
                <p class="uncertain-title">Activation outcome uncertain</p>
                <p>
                  The authority asked Codex to bind a workspace and could not
                  confirm what Codex did. Nothing asks again by itself: a second
                  request could create a second thread.
                </p>
              </div>
            {/if}
            <div class="session-actions">
              {#if onagent && selected.state === 'prepared' && selected.workspace}
                <button
                  type="button"
                  class="action"
                  class:primary={selected.binding === 'bound_attached'}
                  onclick={() => onagent(selected)}>Open agent</button
                >
              {/if}
              {#if onevents}
                <button
                  type="button"
                  class="action"
                  onclick={() => onevents(selected)}>Events</button
                >
              {/if}
              {#if selected.activatable}
                <button
                  type="button"
                  class="action primary"
                  disabled={!connected || pending || sessions.composing}
                  onclick={() => controller.compose(true, selected.id)}
                  >Activate</button
                >
              {:else if selected.recoverable}
                <button
                  type="button"
                  class="action"
                  disabled={!connected || pending}
                  onclick={() => controller.confirmDiscard(selected.id)}
                  >Discard attempt</button
                >
              {:else}
                <button
                  type="button"
                  class="action primary"
                  disabled={!connected || pending || hint !== null}
                  onclick={() => controller.resume(selected.id)}>Resume</button
                >
              {/if}
              {#each offers as mode (mode)}
                <button
                  type="button"
                  class="action"
                  disabled={!connected || pending}
                  onclick={() => controller.confirmMode(selected.id, mode)}
                  >{mode === 'local_tools'
                    ? 'Enable Local Tools'
                    : 'Use Inference'}</button
                >
              {/each}
              <button
                type="button"
                class="action"
                disabled={!connected || pending || !selected.closable}
                onclick={() => controller.confirmClose(selected.id)}
                >Close</button
              >
            </div>
            <p class="session-model" data-testid="observed-model">
              {observedModelLine(selected)}
            </p>
            {#if selected.mode === 'inference'}
              <p class="session-hint">
                This session runs in Inference: Codex is offered no tool. Local
                Tools needs a session of its own, because a conversation stays
                in the mode it was created in.
                <button
                  type="button"
                  class="link"
                  disabled={!connected || pending || sessions.composing}
                  onclick={() => controller.compose(true, null, 'local_tools')}
                  >New Local Tools session</button
                >
              </p>
            {:else if selected.mode === 'local_tools'}
              <p class="session-hint">
                This session runs in Local Tools. Every command and file change
                Codex proposes waits for your approval.
              </p>
            {/if}
            {#if selected.activatable}
              <p class="session-hint">
                Activate binds a project folder and attaches the session to
                Codex.
              </p>
            {:else if hint && !selected.recoverable}
              <p class="session-hint">{hint}</p>
            {/if}
          {/if}

          <dl class="session-facts">
            <dt>Agent</dt>
            <dd>{agentName(selected.adapter)}</dd>
            <dt>Runtime</dt>
            <dd>
              {BINDING[selected.binding].label}
              <span class="fact-note">{BINDING[selected.binding].detail}</span>
            </dd>
            <dt>Mode</dt>
            <dd>
              {modeLabel(selected.mode)}
              {#if selected.mode === 'unknown' && selected.workspace}
                <span class="fact-note"
                  >The authority reports a session's mode while it is attached.</span
                >
              {/if}
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
