<script lang="ts">
  import type { Decision, FixtureInfo } from '../../contracts/m3';
  import type { M3Controller, M3ViewState } from './controller';
  import { msg, t } from '../../i18n/index.svelte';
  import {
    APPROVAL_STATE_LABEL,
    describeAction,
    REFUSAL_LABEL,
    visible,
  } from './present';

  let {
    fixture,
    view,
    controller,
  }: {
    fixture: FixtureInfo;
    view: M3ViewState;
    controller: M3Controller;
  } = $props();

  let draft = $state('');
  const running = $derived(
    fixture.phase === 'starting' || fixture.phase === 'ready',
  );
  const turnActive = $derived(
    fixture.turn?.state === 'starting' || fixture.turn?.state === 'running',
  );
  const canSend = $derived(
    fixture.phase === 'ready' &&
      !turnActive &&
      !view.busy &&
      draft.trim() !== '',
  );
  const PHASE: Record<
    string,
    { label: string; tone: 'ok' | 'pending' | 'error' | 'idle' }
  > = {
    stopped: { label: msg('Stopped'), tone: 'idle' },
    starting: { label: msg('Starting'), tone: 'pending' },
    ready: { label: msg('Ready'), tone: 'ok' },
    refused: { label: msg('Refused'), tone: 'error' },
    failed: { label: msg('Failed'), tone: 'error' },
  };
  const DECISION_LABEL: Record<Decision, string> = {
    accept: msg('Accept'),
    decline: msg('Decline'),
    cancel: msg('Cancel'),
  };
  const TURN_STATE: Record<string, string> = {
    starting: msg('starting'),
    running: msg('running'),
    completed: msg('completed'),
    failed: msg('failed'),
    interrupted: msg('interrupted'),
  };

  function send() {
    const text = draft.trim();
    if (!canSend) return;
    draft = '';
    void controller.fixturePrompt(text);
  }
</script>

<div class="m3-fixture" data-testid="fixture-console">
  <p class="m3-banner" role="note" data-testid="fixture-label">
    {t(
      '{label}. It drives the real Claude Agent SDK against a mock provider on this machine. Nothing here is leased, fenced or recorded by the authority, and no real provider is contacted.',
      { label: fixture.label },
    )}
  </p>

  <div class="m3-toolbar">
    <span
      class="pill compact"
      data-tone={PHASE[fixture.phase].tone}
      data-testid="fixture-phase"
    >
      <span class="dot" aria-hidden="true"></span>
      {PHASE[fixture.phase].label}
    </span>
    {#if fixture.profile}<span class="m3-meta mono">{fixture.profile}</span
      >{/if}
    {#if !running}
      <button
        type="button"
        class="action primary"
        disabled={view.busy}
        onclick={() => controller.fixtureStart('claude_inference_v1')}
        >{t('Start Claude Inference')}</button
      >
      <button
        type="button"
        class="action primary"
        disabled={view.busy}
        onclick={() => controller.fixtureStart('claude_local_tools_v1')}
        >{t('Start Claude Local tools')}</button
      >
    {:else}
      <button
        type="button"
        class="action danger"
        disabled={view.busy}
        onclick={() => controller.fixtureStop()}>{t('Stop the runtime')}</button
      >
    {/if}
  </div>

  {#if fixture.phase === 'refused' || fixture.phase === 'failed'}
    <div
      class="sessions-status failed"
      role="alert"
      data-testid="fixture-failure"
    >
      <p>
        {fixture.phase === 'refused'
          ? t('The runtime was refused and stopped before it could be used')
          : t('The runtime failed and was stopped')}{fixture.detail
          ? t(': {detail}', { detail: fixture.detail })
          : '.'}
      </p>
    </div>
  {/if}
  {#if view.refused}
    <p class="composer-note failed" role="alert" data-testid="fixture-refused">
      {t(REFUSAL_LABEL[view.refused] ?? view.refused)}
    </p>
  {/if}

  <ol
    class="m3-transcript"
    aria-label={t('Conversation')}
    data-testid="transcript"
  >
    {#each fixture.conversation as m, i (i)}
      <li data-role={m.role}>
        <span class="m3-role">{m.role === 'user' ? t('You') : 'Claude'}</span>
        <p class="m3-text">{m.text}</p>
      </li>
    {:else}
      <li class="m3-empty">
        {fixture.phase === 'ready'
          ? t('Send a prompt to the runtime.')
          : t('Start a runtime to talk to it.')}
      </li>
    {/each}
  </ol>

  {#each fixture.approvals as ap (ap.item)}
    {@const shape = describeAction(ap.action)}
    {@const st = APPROVAL_STATE_LABEL[ap.state]}
    <section
      class="rt-card m3-approval"
      data-testid="held-action"
      data-state={ap.state}
    >
      <header>
        <h4>
          {ap.kind === 'file_change'
            ? t('File change proposed by Claude')
            : t('Command proposed by Claude')}
        </h4>
        <span class="pill compact" data-tone={st.tone}>
          <span class="dot" aria-hidden="true"></span>{t(st.label)}
        </span>
      </header>
      {#if shape.kind === 'command'}
        <p class="m3-meta">
          {t('Runs in')} <span class="mono">{visible(shape.cwd)}</span>{t(
            ', with no network.',
          )}
        </p>
        <pre class="m3-code" data-testid="held-command">{visible(
            shape.command,
          )}</pre>
      {:else if shape.kind === 'files'}
        {#each shape.changes as c (c.path)}
          <p class="m3-meta">
            {c.change} <span class="mono">{visible(c.path)}</span>
          </p>
          <pre class="m3-code" data-testid="held-diff">{visible(c.diff)}</pre>
        {/each}
      {:else}
        <p class="rt-alert">
          {t(
            'This action is not one this window can show completely, so it cannot be accepted.',
          )}
        </p>
      {/if}
      <p
        class="m3-meta mono"
        title={t('The fingerprint the decision is bound to')}
      >
        {t('fingerprint {fingerprint}…', {
          fingerprint: ap.fingerprint.slice(0, 16),
        })}
      </p>
      {#if ap.decisions.length > 0}
        <div class="m3-actions">
          {#each ap.decisions as d (d)}
            {#if d !== 'accept' || shape.kind !== 'unknown'}
              <button
                type="button"
                class={d === 'accept' ? 'action primary' : 'action'}
                disabled={view.busy}
                data-decision={d}
                onclick={() => controller.fixtureDecide(ap.item, d)}
                >{t(DECISION_LABEL[d])}</button
              >
            {/if}
          {/each}
        </div>
      {/if}
      {#if ap.state === 'indeterminate'}
        <p class="rt-alert" role="alert">
          {t(
            'Nobody can say whether this happened. It will not be run again; check the workspace before asking for anything similar.',
          )}
        </p>
      {/if}
    </section>
  {/each}

  <form
    class="m3-compose"
    onsubmit={(e) => {
      e.preventDefault();
      send();
    }}
  >
    <label>
      <span class="mode-name">{t('Prompt')}</span>
      <textarea
        rows="2"
        bind:value={draft}
        disabled={fixture.phase !== 'ready'}
        placeholder={fixture.phase === 'ready'
          ? t('Ask Claude something')
          : t('Start a runtime first')}
        data-testid="prompt"></textarea>
    </label>
    <div class="m3-actions">
      <button
        type="submit"
        class="action primary"
        disabled={!canSend}
        data-testid="send">{t('Send')}</button
      >
      <button
        type="button"
        class="action"
        disabled={!turnActive || view.busy}
        data-testid="interrupt"
        onclick={() => controller.fixtureInterrupt()}
        >{t('Cancel the turn')}</button
      >
      {#if fixture.turn}
        <span class="m3-meta" data-testid="turn-state"
          >{t('Turn {state}', {
            state: { text: TURN_STATE[fixture.turn.state] },
          })}</span
        >
      {/if}
    </div>
  </form>
</div>
