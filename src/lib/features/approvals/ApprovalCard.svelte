<script lang="ts">
  import { outcomeUnknown, localClock } from '../agent/present';
  import type { ApprovalCard, ApprovalsController } from './controller';
  import {
    approvalTitle,
    approvalWhere,
    decidingMessage,
    decisionFailureMessage,
    FILE_KIND,
    shownOf,
    wholeShown,
  } from './present';
  import { t } from '../../i18n/index.svelte';

  let {
    card,
    controller,
    connected,
    context = false,
    where,
  }: {
    card: ApprovalCard;
    controller: ApprovalsController;
    connected: boolean;
    /** Put the session's name before its identity: where the card is not shown under its turn. */
    context?: boolean;
    /** The session's name among the sessions shown with it, when it needs more than its folder. */
    where?: string;
  } = $props();

  const id = $derived(card.approval.id);
  const whole = $derived(wholeShown(card));
  // A decision is offered for a pending approval nobody is deciding. Approve also needs
  // everything it is about to have been read. The authority decides either way.
  const open = $derived(
    connected && card.approval.decidable && card.decision === null,
  );
  function title(): string {
    const fact = approvalTitle(card);
    return t(fact.text, fact.values);
  }
  function location(): string {
    if (where !== undefined) return where;
    const fact = approvalWhere(card.workspace, card.approval.session);
    return fact.translate === false ? fact.text : t(fact.text, fact.values);
  }
</script>

<div class="approval-card" role="group" aria-label={t('Approval')}>
  <p class="approval-title">{title()}</p>
  <!-- The session is its canonical identity, as the authority names it on the approval. The
       name before it is for reading; this is what says whose action it is. -->
  <p class="approval-context">
    {#if context}
      <span class="approval-where">{location()}</span>
      ·
    {/if}
    {t('session')}
    <span class="mono">{card.approval.session}</span> · {t('turn')}
    <span class="mono">{card.approval.turn.slice(0, 8)}</span>
  </p>

  {#if card.command}
    <dl class="approval-fields">
      <dt>{t('Command')}</dt>
      <!-- Plain text, as the authority presents it. It is never run or interpreted here. -->
      <dd><pre class="approval-code">{card.command.command}</pre></dd>
      <dt>{t('Directory')}</dt>
      <dd class="mono">{card.command.cwd}</dd>
    </dl>
  {:else if card.files !== null}
    {#each card.shown as file (file.index)}
      <div class="approval-file">
        <p class="file-head">
          <span class="file-kind">{t(FILE_KIND[file.kind])}</span>
          <span class="mono">{file.path}</span>
          {#if file.move_path}
            <span aria-hidden="true">→</span>
            <span class="mono">{file.move_path}</span>
          {/if}
        </p>
        <pre class="approval-code">{file.text}</pre>
        {#if file.next !== null}
          <p class="approval-note">
            {t(
              'Preview truncated: {shown} of {total} bytes shown.',
              shownOf(file.text, file.bytes),
            )}
            <button
              type="button"
              class="action"
              disabled={!connected || file.reading}
              onclick={() => controller.more(id, file.index)}
              >{file.reading ? t('Reading…') : t('Show the rest')}</button
            >
          </p>
        {/if}
        {#if file.failure}
          <p class="approval-note failed" role="alert">
            {t('The rest of this change could not be read.')}
          </p>
        {/if}
      </div>
    {/each}
    {#if card.shown.length < card.files}
      <p class="approval-note">
        {t('{shown} of {total} files shown.', {
          shown: card.shown.length,
          total: card.files,
        })}
        <button
          type="button"
          class="action"
          disabled={!connected || card.listing}
          onclick={() => controller.moreFiles(id)}
          >{card.listing ? t('Reading…') : t('Show more files')}</button
        >
      </p>
    {/if}
  {:else}
    <p class="approval-note">
      {t('The authority no longer holds what this approval was about.')}
    </p>
  {/if}

  <p class="approval-deadline">
    {t('Waits until')}
    <time datetime={card.approval.expires_at}
      >{localClock(card.approval.expires_at)}</time
    >. {t('The authority checks the action again before anything runs.')}
  </p>

  {#if card.decision?.status === 'pending'}
    <p class="turn-activity live" role="status">
      <span class="dot" aria-hidden="true"></span>
      {decidingMessage(card.decision.kind)}
    </p>
  {:else if card.decision?.failure}
    <div
      class="turn-alert"
      class:unknown={outcomeUnknown(card.decision.failure.code)}
      role="alert"
    >
      <p>
        {#each decisionFailureMessage(card.decision.kind, card.decision.failure.code) as sentence, index}{index >
          0
            ? ' '
            : ''}{t(sentence.text, sentence.values)}{/each}
      </p>
      {#if card.decision.failure.retry}
        <button
          type="button"
          class="action"
          disabled={!connected}
          onclick={() => controller.retry(id)}>{t('Retry')}</button
        >
      {/if}
      <button
        type="button"
        class="action"
        disabled={!card.decision.reread}
        onclick={() => controller.dismiss(id)}>{t('Dismiss')}</button
      >
    </div>
  {/if}

  <div class="approval-actions">
    <button
      type="button"
      class="action primary"
      disabled={!open || !whole}
      onclick={() => controller.decide(id, 'approve')}>{t('Approve')}</button
    >
    <button
      type="button"
      class="action danger"
      disabled={!open}
      onclick={() => controller.decide(id, 'deny')}>{t('Deny')}</button
    >
    {#if open && !whole}
      <p class="approval-note">{t('Read the whole change to approve it.')}</p>
    {/if}
  </div>
</div>
