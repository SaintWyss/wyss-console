<script lang="ts">
  import { onMount } from 'svelte';
  import { localClock } from '../agent/present';
  import {
    APPROVAL_KIND,
    APPROVAL_REASON,
    APPROVAL_STATE,
    FILE_KIND,
    shownOf,
  } from '../approvals/present';
  import type { ApprovalView, F8Controller } from './controller';
  import { failureText, outcomeUnknown } from './present';

  let {
    approval,
    controller,
    connected,
    attested,
  }: {
    approval: ApprovalView;
    controller: F8Controller;
    connected: boolean;
    /** The runtime the approval belongs to is the one that is live and attested now. */
    attested: boolean;
  } = $props();

  const record = $derived(approval.detail.approval);
  const command = $derived(approval.detail.command);
  const files = $derived(approval.detail.files);
  const pending = $derived(record.state === 'pending');
  // The whole of what is to be approved was read: a command is presented whole, a change
  // set only when every file and every page of every diff was.
  const whole = $derived(
    command !== null ||
      (files !== null &&
        approval.files.length === files &&
        approval.files.every((f) => f.next === null)),
  );
  const deciding = $derived(approval.decision?.status === 'pending');

  // Past its deadline an approval is no longer offered, whatever the last read said.
  let now = $state(Date.now());
  onMount(() => {
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });
  const expired = $derived(Date.parse(record.expires_at) <= now);
  const decidable = $derived(
    connected && attested && record.decidable && !expired && !deciding,
  );
  const tone = $derived(APPROVAL_STATE[record.state]);
  const reason = $derived(record.reason ? APPROVAL_REASON[record.reason] : '');
  const title = $derived(
    record.kind === 'command_execution'
      ? 'Codex wants to run a command'
      : files === 1
        ? 'Codex wants to change a file'
        : files !== null
          ? `Codex wants to change ${files} files`
          : 'Codex wants to change files',
  );
  const DOING = {
    accept: 'Asking the authority to accept it…',
    decline: 'Asking the authority to decline it…',
    cancel: 'Asking the authority to cancel it…',
  } as const;
  const NOT = {
    accept: 'It was not accepted.',
    decline: 'It was not declined.',
    cancel: 'It was not cancelled.',
  } as const;
</script>

<div class="approval-card" role="group" aria-label="Approval">
  <p class="approval-title">
    {title}
    <span class="pill compact" data-tone={tone.tone}>
      <span class="dot" aria-hidden="true"></span>
      {tone.label}
    </span>
  </p>
  <p class="approval-context">
    {APPROVAL_KIND[record.kind]} · turn
    <span class="mono">{record.turn.slice(0, 8)}</span>
  </p>

  {#if command}
    <dl class="approval-fields">
      <dt>Command</dt>
      <!-- Plain text, as the authority presents it. It is never run or interpreted here. -->
      <dd><pre class="approval-code">{command.command}</pre></dd>
      <dt>Directory</dt>
      <dd class="mono">{command.cwd}</dd>
    </dl>
  {:else if files !== null}
    {#each approval.files as file (file.index)}
      <div class="approval-file">
        <p class="file-head">
          <span class="file-kind">{FILE_KIND[file.kind]}</span>
          <span class="mono">{file.path}</span>
          {#if file.move_path}
            <span aria-hidden="true">→</span>
            <span class="mono">{file.move_path}</span>
          {/if}
        </p>
        <pre class="approval-code">{file.text}</pre>
        {#if file.next !== null}
          <p class="approval-note">
            Preview truncated: {shownOf(file.text, file.bytes)}.
            <button
              type="button"
              class="action"
              disabled={!connected || file.reading}
              onclick={() => controller.moreOfFile(file.index)}
              >{file.reading ? 'Reading…' : 'Show the rest'}</button
            >
          </p>
        {/if}
        {#if file.failure}
          <p class="approval-note failed" role="alert">
            The rest of this change could not be read.
          </p>
        {/if}
      </div>
    {/each}
    {#if approval.files.length < files}
      <p class="approval-note">
        {approval.files.length} of {files} files shown.
        <button
          type="button"
          class="action"
          disabled={!connected || approval.listing}
          onclick={() => controller.moreFiles()}
          >{approval.listing ? 'Reading…' : 'Show more files'}</button
        >
      </p>
    {/if}
  {:else if pending}
    <p class="approval-note">
      The authority no longer holds what this approval was about. It cannot be
      accepted.
    </p>
  {/if}

  {#if pending}
    <p class="approval-deadline">
      {expired ? 'Its time ran out at' : 'Waits until'}
      <time datetime={record.expires_at}>{localClock(record.expires_at)}</time>
      — the authority checks the action again before anything runs.
    </p>
  {:else if reason}
    <p class="approval-note">{reason}</p>
  {/if}

  {#if !attested && pending}
    <p class="approval-note" role="note">
      The runtime this approval was asked on is no longer the live one, so it
      cannot be decided.
    </p>
  {/if}

  {#if approval.decision?.status === 'pending'}
    <p class="turn-activity live" role="status">
      <span class="dot" aria-hidden="true"></span>
      {DOING[approval.decision.kind]}
    </p>
  {:else if approval.decision?.failure}
    <div
      class="turn-alert"
      class:unknown={outcomeUnknown(approval.decision.failure.code)}
      role="alert"
    >
      <p>
        {approval.decision.failure.code === 'unavailable'
          ? 'The authority did not answer, so it is not known whether the decision was taken. Retrying repeats the same decision: it cannot decide twice.'
          : `${NOT[approval.decision.kind]} ${failureText(approval.decision.failure.code)}`}
      </p>
      {#if approval.decision.failure.retry}
        <button
          type="button"
          class="action"
          disabled={!connected}
          onclick={() => controller.retryDecision()}>Retry</button
        >
      {/if}
      <button
        type="button"
        class="action"
        onclick={() => controller.dismissDecision()}>Dismiss</button
      >
    </div>
  {/if}

  {#if pending}
    <div class="approval-actions">
      <button
        type="button"
        class="action primary"
        disabled={!decidable || !whole}
        onclick={() => controller.decide('accept')}>Accept</button
      >
      <button
        type="button"
        class="action danger"
        disabled={!decidable}
        onclick={() => controller.decide('decline')}>Decline</button
      >
      <button
        type="button"
        class="action"
        disabled={!decidable}
        onclick={() => controller.decide('cancel')}>Cancel action</button
      >
      {#if decidable && !whole}
        <p class="approval-note">Read the whole change to accept it.</p>
      {/if}
    </div>
  {/if}
</div>
