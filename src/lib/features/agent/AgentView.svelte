<script lang="ts">
  import { tick } from 'svelte';
  import { turnEnded } from '../../contracts/turns';
  import {
    summarizeAuthority,
    type AuthorityObservation,
  } from '../../shell/authority';
  import { tJoin } from '../../i18n/index.svelte';
  import { BINDING, localTime } from '../sessions/present';
  import ApprovalCard from '../approvals/ApprovalCard.svelte';
  import type {
    ApprovalsController,
    ApprovalsState,
  } from '../approvals/controller';
  import {
    promptFits,
    type AgentController,
    type AgentState,
  } from './controller';
  import {
    activityLabel,
    agentLabel,
    approvalMessage,
    cancelFailureMessage,
    historyFailureMessage,
    localClock,
    outcomeLabel,
    outcomeUnknown,
    overLimit,
    readFailureMessage,
    runHint,
    startFailureMessage,
    TURN_REASON,
    TURN_STATE,
  } from './present';

  let {
    agent,
    controller,
    authority,
    visible,
    onsessions,
    approvals,
    approvalsController,
  }: {
    agent: AgentState;
    controller: AgentController;
    /** The shell's one authority observation. Read here for wording only. */
    authority: AuthorityObservation;
    visible: boolean;
    /** Shows the Sessions view, where a session is created, activated and resumed. */
    onsessions: () => void;
    /** Approvals as the shell's one approvals controller holds them. */
    approvals: ApprovalsState;
    approvalsController: ApprovalsController;
  } = $props();

  const view = $derived(agent.current);
  const history = $derived(view?.history);
  const connected = $derived(agent.link === 'connected');
  const hint = $derived(view ? runHint(agent.link, view) : null);
  const runnable = $derived(
    view !== null && hint === null && promptFits(view.draft),
  );
  const active = $derived(view?.turns.find((t) => t.id === view.active));
  /** The approval a turn waits on, once what it is about was read. Never another turn's. */
  const cardOf = (turn: string, approval: string) =>
    approvals.pending?.find(
      (c) => c.approval.id === approval && c.approval.turn === turn,
    );
  // A turn names an approval that is not shown yet: it is read, by the one reader.
  $effect(() => {
    for (const entry of view?.turns ?? [])
      if (entry.approval && !cardOf(entry.id, entry.approval.id))
        approvalsController.want(entry.approval.id);
  });

  let transcript = $state<HTMLElement>();
  /** Whether the reader is at the end of the transcript, and so follows it. */
  let pinned = true;
  function scrolled() {
    if (!transcript) return;
    pinned =
      transcript.scrollHeight - transcript.scrollTop - transcript.clientHeight <
      48;
  }
  // New output keeps the end in view, unless the reader scrolled away from it.
  $effect(() => {
    void view;
    if (!visible || !pinned) return;
    void tick().then(() => {
      if (transcript) transcript.scrollTop = transcript.scrollHeight;
    });
  });
  // Another session is another transcript: it opens at its end. Only a change of session
  // does that. The view is a new object on every update, so tracking it alone would pin a
  // reader who scrolled up to read, and pull them to the end when the next output arrives.
  let shownSession: string | undefined;
  $effect(() => {
    const session = view?.session;
    if (session === shownSession) return;
    shownSession = session;
    pinned = true;
  });

  /**
   * Loads the page of turns before the first one shown, and keeps the reader where they
   * were: what is inserted above them must not move what they are looking at.
   */
  async function older() {
    const node = transcript;
    const kept = node ? node.scrollHeight - node.scrollTop : 0;
    await controller.loadOlder();
    await tick();
    if (node && !pinned) node.scrollTop = node.scrollHeight - kept;
  }

  function submit(event?: Event) {
    event?.preventDefault();
    if (runnable) controller.start();
  }
  /** Enter runs; Shift+Enter is a new line. Text being composed is never submitted. */
  function keydown(event: KeyboardEvent) {
    if (event.key !== 'Enter' || event.shiftKey || event.isComposing) return;
    event.preventDefault();
    submit();
  }
</script>

<section
  class="agent-feature"
  aria-label="Agent"
  hidden={!visible}
  inert={!visible}
>
  {#if !view}
    <div class="sessions-empty">
      <p class="empty-title">No session is open.</p>
      <p>
        Open a Ready session from Sessions to give Codex an objective in its
        project.
      </p>
      <button type="button" class="action primary" onclick={onsessions}
        >Go to Sessions</button
      >
    </div>
  {:else}
    <div class="agent-toolbar">
      <div class="agent-tabs" role="tablist" aria-label="Open sessions">
        {#each agent.tabs as tab (tab.session)}
          <div class="agent-tab">
            <button
              type="button"
              role="tab"
              aria-selected={tab.session === view.session}
              onclick={() => controller.select(tab.session)}>{tab.label}</button
            >
            <button
              type="button"
              class="tab-close"
              aria-label={`Close ${tab.label}`}
              disabled={tab.session === view.session &&
                view.start?.status === 'pending'}
              onclick={() => controller.close(tab.session)}>×</button
            >
          </div>
        {/each}
      </div>
      <button
        type="button"
        class="action"
        disabled={!connected || view.reading}
        onclick={() => controller.refresh()}>Refresh</button
      >
    </div>

    <!-- The session every turn below belongs to, and every turn started here goes to. -->
    <header class="agent-session" aria-label="Active session">
      <h2>{view.label}</h2>
      {#if view.facts}
        <span
          class="pill compact"
          data-tone={view.facts.state === 'closed'
            ? 'idle'
            : BINDING[view.facts.binding].tone}
        >
          <span class="dot" aria-hidden="true"></span>
          {view.facts.state === 'closed'
            ? 'Closed'
            : BINDING[view.facts.binding].label}
        </span>
        {#if view.facts.workspace}
          <span class="agent-path">{view.facts.workspace}</span>
        {/if}
      {/if}
      <span class="agent-who">{agentLabel(view.facts?.mode)}</span>
    </header>

    {#if !connected}
      <p class="sessions-status failed unknown" role="status">
        {agent.link === 'connecting'
          ? 'Connecting to the authority…'
          : `${tJoin(summarizeAuthority(authority).detail)} What is shown was read before the connection was lost.`}
      </p>
    {:else if view.readFailure}
      <div class="sessions-status failed" role="alert">
        <p>{readFailureMessage(view.readFailure.code)}</p>
        <button
          type="button"
          class="action"
          onclick={() => controller.refresh()}>Try again</button
        >
      </div>
    {/if}

    <div
      class="agent-transcript"
      role="log"
      aria-label="Turns"
      bind:this={transcript}
      onscroll={scrolled}
    >
      {#if !history}
        <!-- Nothing is known of older turns until the first list. -->
      {:else if history.status === 'loading'}
        <p class="turn-activity live" role="status">
          <span class="dot" aria-hidden="true"></span>
          Loading older turns…
        </p>
      {:else if history.status === 'failed' && history.failure}
        <div
          class="turn-alert"
          class:unknown={outcomeUnknown(history.failure.code)}
          role="alert"
        >
          <p>{historyFailureMessage(history.failure.code)}</p>
          {#if history.failure.code === 'changed'}
            <button
              type="button"
              class="action"
              onclick={() => controller.restartHistory()}>Reload turns</button
            >
          {:else}
            <button
              type="button"
              class="action"
              disabled={!connected}
              onclick={older}>Try again</button
            >
          {/if}
        </div>
      {:else}
        {#if history.searched}
          <p class="turn-note">
            No turns in the part of the history just searched. There is more
            further back.
          </p>
        {/if}
        {#if history.more}
          <button
            type="button"
            class="action history-more"
            disabled={!connected}
            onclick={older}
            >{history.searched ? 'Keep looking' : 'Load older turns'}</button
          >
        {:else if history.capped}
          <p class="turn-note">
            {history.loaded} older turns are shown, the most this view keeps.
            <button
              type="button"
              class="action"
              onclick={() => controller.restartHistory()}
              >Start over from the newest</button
            >
          </p>
        {:else if history.reachedStart}
          <p class="turn-note">This is the start of this session's history.</p>
        {/if}
      {/if}
      {#if view.turns.length === 0 && !view.start}
        <div class="agent-blank">
          {#if view.facts === null && !view.readFailure}
            <p class="empty-title">Reading turns…</p>
          {:else}
            <p class="empty-title">No turns yet.</p>
            <p>
              Write an objective below. The authority decides whether the turn
              may start and what Codex is allowed to do in it.
            </p>
          {/if}
        </div>
      {/if}

      {#each view.turns as entry (entry.id)}
        {@const turn = entry.turn}
        {@const ended = turn !== null && turnEnded(turn.state)}
        <article class="turn" aria-label="Turn">
          <header class="turn-head">
            {#if turn}
              <span
                class="pill compact"
                data-tone={TURN_STATE[turn.state].tone}
              >
                <span class="dot" aria-hidden="true"></span>
                {TURN_STATE[turn.state].label}
              </span>
              <time
                datetime={turn.started_at}
                title={localTime(turn.started_at)}
                >{localClock(turn.started_at)}</time
              >
              {#if turn.reason && TURN_REASON[turn.reason]}
                <span class="turn-reason">{TURN_REASON[turn.reason]}</span>
              {/if}
            {:else}
              <span class="pill compact" data-tone="pending">
                <span class="dot" aria-hidden="true"></span>
                Accepted
              </span>
            {/if}
          </header>

          {#if entry.prompt !== null}
            <div class="turn-prompt">
              <p class="turn-who">You</p>
              <p class="turn-text">{entry.prompt}</p>
            </div>
          {:else}
            <p class="turn-note">
              The input of this turn is not shown: Wyss keeps no conversation
              text.
            </p>
          {/if}

          <div class="turn-answer">
            <p class="turn-who">Codex</p>
            {#if entry.clipped}
              <p class="turn-note">
                The beginning of this output is no longer shown.
              </p>
            {/if}
            {#each entry.items as item, index (index)}
              {#if item.type === 'text'}
                <!-- Plain text, as written. It is never interpreted as markup. -->
                <p class="turn-text">{item.text}</p>
              {:else if item.type === 'activity'}
                <p class="turn-activity">
                  {activityLabel(item.kind, item.count)}
                </p>
              {:else if item.type === 'resolved'}
                <p class="turn-activity">{outcomeLabel(item.outcome)}</p>
              {:else}
                <p class="turn-gap">
                  Output here was not kept by the authority.
                </p>
              {/if}
            {/each}
            {#if ended && entry.items.length === 0}
              <p class="turn-note">
                {turn?.output
                  ? 'This turn produced no answer.'
                  : 'The authority no longer holds the output of this turn.'}
              </p>
            {/if}
            {#if !ended && entry.thinking && !entry.approval}
              <p class="turn-activity live">
                <span class="dot" aria-hidden="true"></span>
                Thinking…
              </p>
            {/if}
            {#if !ended && entry.approval}
              {@const card = cardOf(entry.id, entry.approval.id)}
              {#if card}
                <ApprovalCard
                  {card}
                  controller={approvalsController}
                  {connected}
                />
              {:else}
                <div class="turn-approval" role="status">
                  <p class="approval-title">Waiting for approval</p>
                  <p>
                    {approvalMessage(
                      entry.approval.kind,
                      localClock(entry.approval.expires_at),
                    )}
                  </p>
                </div>
              {/if}
            {/if}
            {#if !ended && entry.cancel !== 'idle'}
              <p class="turn-activity live" role="status">
                <span class="dot" aria-hidden="true"></span>
                Cancelling…
              </p>
            {/if}
            {#if entry.cancelFailure}
              <p
                class="turn-alert"
                class:unknown={outcomeUnknown(entry.cancelFailure.code)}
                role="alert"
              >
                {cancelFailureMessage(entry.cancelFailure.code)}
              </p>
            {/if}
            {#if entry.readFailure}
              <div
                class="turn-alert"
                class:unknown={outcomeUnknown(entry.readFailure.code)}
                role="alert"
              >
                <p>{readFailureMessage(entry.readFailure.code)}</p>
                <button
                  type="button"
                  class="action"
                  disabled={!connected}
                  onclick={() => controller.refresh()}>Read again</button
                >
              </div>
            {/if}
          </div>
        </article>
      {/each}

      {#if view.start}
        <article class="turn" aria-label="Turn being started">
          <div class="turn-prompt">
            <p class="turn-who">You</p>
            <p class="turn-text">{view.start.prompt}</p>
          </div>
          {#if view.start.status === 'pending'}
            <p class="turn-activity live" role="status">
              <span class="dot" aria-hidden="true"></span>
              Asking the authority to start the turn…
            </p>
          {:else if view.start.failure}
            <div
              class="turn-alert"
              class:unknown={outcomeUnknown(view.start.failure.code)}
              role="alert"
            >
              <p>{startFailureMessage(view.start.failure.code)}</p>
              {#if view.start.failure.retry}
                <button
                  type="button"
                  class="action"
                  disabled={!connected}
                  onclick={() => controller.retryStart()}>Retry</button
                >
              {/if}
              <button
                type="button"
                class="action"
                onclick={() => controller.dismissStart()}>Dismiss</button
              >
            </div>
          {/if}
        </article>
      {/if}
    </div>

    <form class="agent-composer" aria-label="New turn" onsubmit={submit}>
      <textarea
        aria-label="Objective"
        placeholder="What should Codex do in this project?"
        rows="3"
        spellcheck="false"
        value={view.draft}
        oninput={(event) => controller.setDraft(event.currentTarget.value)}
        onkeydown={keydown}></textarea>
      <div class="composer-row">
        <p class="composer-hint" class:warn={overLimit(view.draft)}>
          {hint ?? 'Enter to run · Shift+Enter for a new line'}
        </p>
        {#if active?.turn}
          <button
            type="button"
            class="action danger"
            disabled={!connected || active.cancel !== 'idle'}
            onclick={() => controller.cancel(active.id)}
            >{active.cancel === 'idle' ? 'Cancel turn' : 'Cancelling…'}</button
          >
        {/if}
        <button type="submit" class="action primary" disabled={!runnable}
          >Run</button
        >
      </div>
    </form>
  {/if}
</section>
