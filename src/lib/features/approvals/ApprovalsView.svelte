<script lang="ts">
  import {
    summarizeAuthority,
    type AuthorityObservation,
  } from '../../shell/authority';
  import { localTime } from '../sessions/present';
  import ApprovalCard from './ApprovalCard.svelte';
  import type { ApprovalsController, ApprovalsState } from './controller';
  import {
    APPROVAL_KIND,
    APPROVAL_REASON,
    APPROVAL_STATE,
    approvalsReadFailure,
  } from './present';

  let {
    approvals,
    controller,
    authority,
    visible,
  }: {
    approvals: ApprovalsState;
    controller: ApprovalsController;
    /** The shell's one authority observation. Read here for wording only. */
    authority: AuthorityObservation;
    visible: boolean;
  } = $props();

  const connected = $derived(approvals.link === 'connected');
  const pending = $derived(approvals.pending);
</script>

<section
  class="approvals-feature"
  aria-label="Approvals"
  hidden={!visible}
  inert={!visible}
>
  <div class="sessions-toolbar">
    <div class="sessions-heading">
      <h2>Approvals</h2>
      <p class="sessions-count">
        {#if !connected}
          {approvals.link === 'connecting'
            ? 'Connecting to the authority'
            : 'Authority unavailable'}
        {:else if pending === null}
          Reading
        {:else}
          {pending.length} waiting for a decision
        {/if}
      </p>
    </div>
    <button
      type="button"
      class="action"
      disabled={!connected || approvals.reading}
      onclick={() => controller.refresh()}>Refresh</button
    >
  </div>

  {#if approvals.link === 'connecting'}
    <div class="sessions-empty" role="status">
      <p class="empty-title">Connecting to wyss-control…</p>
    </div>
  {:else if !connected}
    <div class="sessions-empty" role="status">
      <p class="empty-title">wyss-control is unavailable</p>
      <p>{summarizeAuthority(authority).detail}</p>
      <p>
        Approvals are read from the authority and cannot be decided until it is
        connected. Nothing is approved or denied in the meantime.
      </p>
    </div>
  {:else if pending === null && approvals.readFailure}
    <div class="sessions-empty" role="alert">
      <p class="empty-title">
        {approvalsReadFailure(approvals.readFailure.code)}
      </p>
      <button type="button" class="action" onclick={() => controller.refresh()}
        >Try again</button
      >
    </div>
  {:else if pending === null}
    <div class="sessions-empty" role="status" aria-busy="true">
      <p class="empty-title">Reading approvals…</p>
    </div>
  {:else}
    <div class="approvals-body">
      {#if pending.length === 0}
        <div class="approvals-none">
          <p class="empty-title">Nothing is waiting for a decision.</p>
          <p>
            In Local Tools, every command and file change Codex proposes stops
            here until you approve or deny it. Inference sessions propose none.
          </p>
        </div>
      {:else}
        <ul class="approvals-list" aria-label="Pending approvals">
          {#each pending as card (card.approval.id)}
            <li>
              <ApprovalCard {card} {controller} {connected} context />
            </li>
          {/each}
        </ul>
      {/if}

      {#if approvals.recent.length > 0}
        <h3 class="approvals-heading">Recently ended</h3>
        <ul class="approvals-recent" aria-label="Ended approvals">
          {#each approvals.recent as approval (approval.id)}
            <li class="approval-row">
              <span
                class="pill compact"
                data-tone={APPROVAL_STATE[approval.state].tone}
              >
                <span class="dot" aria-hidden="true"></span>
                {APPROVAL_STATE[approval.state].label}
              </span>
              <span class="approval-kind">{APPROVAL_KIND[approval.kind]}</span>
              <span class="session-meta">
                turn <span class="mono">{approval.turn.slice(0, 8)}</span>
                {#if approval.ended_at}
                  · <time datetime={approval.ended_at}
                    >{localTime(approval.ended_at)}</time
                  >
                {/if}
              </span>
              {#if approval.reason && APPROVAL_REASON[approval.reason]}
                <span class="session-meta"
                  >{APPROVAL_REASON[approval.reason]}</span
                >
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
      {#if approvals.partial}
        <p class="approval-note">Older approvals are not listed.</p>
      {/if}
    </div>
  {/if}
</section>
