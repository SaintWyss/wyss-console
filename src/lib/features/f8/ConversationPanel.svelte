<script lang="ts">
  import { tick } from 'svelte';
  import type { SessionDetail } from '../../contracts/f8';
  import { PROMPT_MAX_BYTES, turnEnded } from '../../contracts/turns';
  import { t } from '../../i18n/index.svelte';
  import {
    activityLabel,
    localClock,
    outcomeLabel,
    TURN_REASON,
    TURN_STATE,
  } from '../agent/present';
  import ApprovalPanel from './ApprovalPanel.svelte';
  import type { F8Controller, F8ViewState } from './controller';
  import { failureText, gateText } from './present';

  let {
    f8,
    detail,
    controller,
    connected,
    pending,
  }: {
    f8: F8ViewState;
    detail: SessionDetail;
    controller: F8Controller;
    connected: boolean;
    pending: boolean;
  } = $props();

  const convo = $derived(f8.conversation);
  const turn = $derived(convo?.info?.turn ?? null);
  const running = $derived(turn !== null && !turnEnded(turn.state));
  const gate = $derived(detail.gates.find((g) => g.action === 'turn_start'));
  const bytes = $derived(new TextEncoder().encode(f8.draft).length);
  const fits = $derived(
    f8.draft.trim().length > 0 && bytes <= PROMPT_MAX_BYTES,
  );
  const hint = $derived.by(() => {
    if (!connected) return 'The authority is not connected.';
    if (detail.session.state === 'closed') return 'This session is closed.';
    if (running) return 'A turn is running. Cancel it to start another.';
    if (gate && gate.reason !== 'allowed') return t(gateText(gate.reason));
    if (bytes > PROMPT_MAX_BYTES) return 'The objective is too long.';
    return null;
  });
  const runnable = $derived(connected && !pending && hint === null && fits);
  const cancelling = $derived(
    f8.operation?.kind === 'cancel' && f8.operation.status === 'pending',
  );
  // An approval is decided against the runtime that is live and attested now; the
  // authority checks that the approval belongs to that very launch.
  const attested = $derived(
    detail.session.runtime.actual === 'ready' &&
      detail.session.runtime.fence.instance !== null,
  );

  let log = $state<HTMLElement>();
  let pinned = true;
  function scrolled() {
    if (!log) return;
    pinned = log.scrollHeight - log.scrollTop - log.clientHeight < 48;
  }
  // New output keeps the end in view, unless the reader scrolled away from it.
  $effect(() => {
    void convo?.transcript;
    if (!pinned) return;
    void tick().then(() => {
      if (log) log.scrollTop = log.scrollHeight;
    });
  });

  function submit(event?: Event) {
    event?.preventDefault();
    if (runnable) controller.startTurn();
  }
  /** Enter runs; Shift+Enter is a new line. Text being composed is never submitted. */
  function keydown(event: KeyboardEvent) {
    if (event.key !== 'Enter' || event.shiftKey || event.isComposing) return;
    event.preventDefault();
    submit();
  }
</script>

<div class="rt-conversation">
  {#if detail.turns.length > 0}
    <div class="rt-turns" role="group" aria-label="Recent turns">
      {#each detail.turns as entry (entry.turn.id)}
        <button
          type="button"
          class="action chip"
          aria-pressed={convo?.turn === entry.turn.id}
          title={entry.turn.id}
          onclick={() => controller.openTurn(entry.turn.id)}
        >
          {localClock(entry.turn.started_at)} · {TURN_STATE[entry.turn.state]
            .label}
          {#if entry.generation !== null}· gen {entry.generation}{/if}
        </button>
      {/each}
      {#if detail.turns_partial}
        <span class="events-note">Older turns are in History.</span>
      {/if}
    </div>
  {/if}

  <div
    class="agent-transcript"
    role="log"
    aria-label="Turn"
    bind:this={log}
    onscroll={scrolled}
  >
    {#if !convo}
      <div class="agent-blank">
        <p class="empty-title">No turn is open.</p>
        <p>
          Write an objective below. The authority decides whether the turn may
          start and what Codex is allowed to do in it.
        </p>
      </div>
    {:else}
      <article class="turn" aria-label="Turn">
        <header class="turn-head">
          {#if turn}
            <span class="pill compact" data-tone={TURN_STATE[turn.state].tone}>
              <span class="dot" aria-hidden="true"></span>
              {TURN_STATE[turn.state].label}
            </span>
            <time datetime={turn.started_at}>{localClock(turn.started_at)}</time
            >
            {#if convo.info?.generation !== null && convo.info?.generation !== undefined}
              <span class="turn-reason">generation {convo.info.generation}</span
              >
            {/if}
            {#if turn.reason && TURN_REASON[turn.reason]}
              <span class="turn-reason">{TURN_REASON[turn.reason]}</span>
            {/if}
          {:else}
            <span class="pill compact" data-tone="pending">
              <span class="dot" aria-hidden="true"></span>
              Reading
            </span>
          {/if}
        </header>
        <p class="turn-note">
          The input of a turn is not shown: Wyss keeps no conversation text.
        </p>
        <div class="turn-answer">
          <p class="turn-who">Codex</p>
          {#if convo.transcript.truncated}
            <p class="turn-note">
              The beginning of this output is no longer shown.
            </p>
          {/if}
          {#each convo.transcript.items as item, index (index)}
            {#if item.kind === 'assistant'}
              <!-- Plain text, as written. It is never interpreted as markup. -->
              <p class="turn-text">{item.text}</p>
            {:else if item.kind === 'activity'}
              <p class="turn-activity">
                {activityLabel(item.event, item.count)}
              </p>
            {:else if item.kind === 'approval'}
              <p class="turn-activity">{outcomeLabel(item.outcome)}</p>
            {:else if item.kind === 'thinking'}
              <p class="turn-activity">Thinking</p>
            {:else}
              <p class="turn-gap">Output here was not kept by the authority.</p>
            {/if}
          {/each}
          {#if turn && turnEnded(turn.state) && convo.transcript.items.length === 0}
            <p class="turn-note">
              {turn.output
                ? 'This turn produced no answer.'
                : 'The authority no longer holds the output of this turn.'}
            </p>
          {/if}
          {#if convo.following && !convo.approval && !convo.reconnecting}
            <p class="turn-activity live" role="status">
              <span class="dot" aria-hidden="true"></span>
              Following the turn…
            </p>
          {/if}
          {#if convo.reconnecting}
            <p class="turn-activity live" role="status">
              <span class="dot" aria-hidden="true"></span>
              The authority stopped answering; reading the turn again…
            </p>
          {/if}
          {#if convo.lost}
            <div class="turn-alert unknown" role="alert">
              <p>
                This turn can no longer be followed: the authority does not have
                it or did not answer. What it did is read from History.
              </p>
              <button
                type="button"
                class="action"
                onclick={() => controller.refresh()}>Read again</button
              >
            </div>
          {/if}
          {#if cancelling}
            <p class="turn-activity live" role="status">
              <span class="dot" aria-hidden="true"></span>
              Cancelling…
            </p>
          {/if}
        </div>
        {#if f8.approval && (!turn || f8.approval.detail.approval.turn.toLowerCase() === turn.id.toLowerCase())}
          <ApprovalPanel
            approval={f8.approval}
            {controller}
            {connected}
            {attested}
          />
        {:else if convo.approval}
          <div class="turn-approval" role="status">
            <p class="approval-title">Waiting for approval</p>
            <p>Reading what Codex asked for…</p>
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
      value={f8.draft}
      oninput={(event) => controller.setDraft(event.currentTarget.value)}
      onkeydown={keydown}></textarea>
    <div class="composer-row">
      <p class="composer-hint" class:warn={bytes > PROMPT_MAX_BYTES}>
        {hint ?? 'Enter to run · Shift+Enter for a new line'}
      </p>
      {#if running}
        <button
          type="button"
          class="action danger"
          disabled={!connected || cancelling || pending}
          onclick={() => controller.cancelTurn()}
          >{cancelling ? 'Cancelling…' : 'Cancel turn'}</button
        >
      {/if}
      <button type="submit" class="action primary" disabled={!runnable}
        >Run</button
      >
    </div>
    {#if f8.operation?.kind === 'turn' && f8.operation.failure}
      <p class="composer-note failed" role="alert">
        {t(failureText(f8.operation.failure.code))}
      </p>
    {/if}
  </form>
</div>
