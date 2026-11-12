<script lang="ts">
  import { onMount } from 'svelte';
  import {
    GATE_ACTIONS,
    LEASE_SCOPES,
    LEASE_TTLS,
    type SessionDetail,
  } from '../../contracts/f8';
  import { AGENT_MODES } from '../../contracts/sessions';
  import { localClock } from '../agent/present';
  import { t, tPhrase } from '../../i18n/index.svelte';
  import {
    AGENT_MODE,
    folderName,
    observedModelLine,
  } from '../sessions/present';
  import type { F8Controller, F8ViewState } from './controller';
  import {
    ATTACHMENT_LABEL,
    BINDING_STATE,
    budgetText,
    diagnosticText,
    failureText,
    gateText,
    leaseLine,
    PROFILE_LABEL,
    RUNTIME_STATE,
    SCOPE_LABEL,
    SCOPE_NOTE,
    ttlLabel,
    visible,
  } from './present';

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

  const row = $derived(detail.session);
  const runtime = $derived(row.runtime);
  const binding = $derived(detail.binding);
  const lease = $derived(detail.lease);
  const open = $derived(row.state === 'prepared');
  const ready = $derived(runtime.actual === 'ready');
  const busy = $derived(
    runtime.actual === 'starting' ||
      runtime.actual === 'initializing' ||
      runtime.actual === 'stopping',
  );
  const idle = $derived(
    runtime.actual === 'stopped' || runtime.actual === 'failed',
  );
  const leaseActive = $derived(lease?.state === 'active');
  const acts = $derived(connected && !pending && open);
  const mode = $derived(f8.composer.mode);
  const folder = $derived(f8.composer.workspace);
  const confirming = $derived(f8.confirming?.kind ?? null);

  // The clock a lease is read against: ticks only while a lease is on screen.
  let now = $state(Date.now());
  onMount(() => {
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });

  /** What to do next, said once, from what was read: never a guess about what will succeed. */
  const next = $derived.by(() => {
    if (!open) return t('This session is closed.');
    if (!ready)
      return busy
        ? t('The runtime is changing state.')
        : t('Start the runtime to work in this session.');
    if (!leaseActive)
      return t(
        'Grant an execution lease so the session may bind a workspace and run turns.',
      );
    if (binding.state === 'unbound')
      return t('Bind a project folder to this session.');
    if (binding.state === 'bound_detached')
      return t('Attach the session to the running runtime.');
    if (binding.state === 'indeterminate')
      return t(
        'The authority could not confirm the last binding attempt. Nothing asks again by itself.',
      );
    if (binding.state === 'creating')
      return t('The authority is binding the workspace.');
    return t('Ready. Open the Conversation tab to give Codex an objective.');
  });
</script>

<div class="rt-overview">
  <p class="rt-next" role="status">{next}</p>

  <section class="rt-card" aria-labelledby="rt-provider-support">
    <header>
      <h4 id="rt-provider-support">{t('Provider support')}</h4>
    </header>
    <p class="rt-alert" data-testid="claude-managed-unavailable" role="note">
      {t(
        'Claude Code Managed is not implemented in this authority. This session remains Codex; Native use requires a separate, explicit launch.',
      )}
    </p>
  </section>

  <section class="rt-card" aria-labelledby="rt-runtime">
    <header>
      <h4 id="rt-runtime">{t('Runtime')}</h4>
      <span class="pill compact" data-tone={RUNTIME_STATE[runtime.actual].tone}>
        <span class="dot" aria-hidden="true"></span>
        {t(RUNTIME_STATE[runtime.actual].label)}
      </span>
    </header>
    <dl class="session-facts">
      <dt>{t('Generation')}</dt>
      <dd>
        {runtime.generation === 0 ? t('None yet') : runtime.generation}
        <span class="fact-note"
          >{t(
            'Each start is a new generation; an earlier one never comes back.',
          )}</span
        >
      </dd>
      <dt>{t('Mode')}</dt>
      <dd>{runtime.profile ? PROFILE_LABEL[runtime.profile] : '—'}</dd>
      <dt>{t('Version')}</dt>
      <dd>{runtime.version ?? '—'}</dd>
      {#if runtime.workspace}
        <dt>{t('Isolated for')}</dt>
        <dd class="mono" dir="ltr">{visible(runtime.workspace)}</dd>
      {/if}
      <dt>{t('Wanted')}</dt>
      <dd>
        {runtime.desired === 'running' ? t('Running') : t('Stopped')} · {t(
          'revision',
        )}
        {runtime.revision}
      </dd>
    </dl>
    {#if runtime.diagnostic}
      <p class="rt-alert" role="note">
        {t(diagnosticText(runtime.diagnostic))}
      </p>
    {/if}
    {#if runtime.budget_failure}
      <p class="rt-alert" role="note">
        {tPhrase(budgetText(runtime.budget_failure))}
      </p>
    {/if}
    {#if runtime.exhausted}
      <p class="rt-alert" role="note">
        {t(
          'This session has used every launch the authority allows it. No further runtime is started for it; a new session can be created.',
        )}
      </p>
    {/if}

    {#if confirming === 'stop'}
      <div class="close-confirm" role="group" aria-label={t('Confirm stop')}>
        <p>
          {t(
            'Stop this runtime? Its process is ended and reaped. A turn that runs is not stopped under it: the authority refuses until that turn ends.',
          )}
        </p>
        <div class="session-actions">
          <button
            type="button"
            class="action danger"
            disabled={!acts}
            onclick={() => controller.stopRuntime()}>{t('Stop runtime')}</button
          >
          <button
            type="button"
            class="action"
            onclick={() => controller.confirm(null)}>{t('Keep running')}</button
          >
        </div>
      </div>
    {:else if open && idle}
      <fieldset class="mode-choice">
        <legend>{t('Mode')}</legend>
        {#each AGENT_MODES as m (m)}
          <label class="mode-option">
            <input
              type="radio"
              name="rt-mode"
              value={m}
              checked={mode === m}
              disabled={!acts}
              onchange={() => controller.chooseMode(m)}
            />
            <span class="mode-name">{AGENT_MODE[m].label}</span>
            <span class="mode-detail">{t(AGENT_MODE[m].detail)}</span>
          </label>
        {/each}
      </fieldset>
      {#if mode === 'local_tools'}
        <p class="composer-note">
          {t(
            'Local Tools gives Codex more to work with, not more authority: every command and file change waits for your approval. The runtime is isolated for one folder.',
          )}
        </p>
      {/if}
      <div class="session-actions">
        {#if mode === 'local_tools'}
          <button
            type="button"
            class="action"
            disabled={!acts || f8.composer.choosing}
            aria-busy={f8.composer.choosing}
            onclick={() => controller.chooseFolder()}
            >{folder ? t('Change folder') : t('Choose folder')}</button
          >
        {/if}
        <button
          type="button"
          class="action primary"
          disabled={!acts ||
            runtime.exhausted ||
            (mode === 'local_tools' && folder === null)}
          onclick={() => controller.startRuntime()}>{t('Start runtime')}</button
        >
        {#if runtime.desired === 'running'}
          <button
            type="button"
            class="action"
            disabled={!acts}
            onclick={() => controller.confirm('stop')}
            >{t('Stop runtime')}</button
          >
        {/if}
      </div>
      {#if runtime.desired === 'running'}
        <p class="composer-note">
          {t(
            'The runtime is not running but is still wanted, for example after the authority restarted. Stop it to close this session.',
          )}
        </p>
      {/if}
      {#if mode === 'local_tools'}
        <p class="composer-note">
          {t('Folder:')}
          <span class="mono">{folder ?? t('none selected')}</span>
        </p>
      {/if}
      {#if f8.composer.chooseFailure}
        <p class="composer-note failed" role="alert">
          {t(failureText(f8.composer.chooseFailure.code))}
        </p>
      {/if}
    {:else if open}
      <div class="session-actions">
        <button
          type="button"
          class="action"
          disabled={!acts || busy}
          onclick={() => controller.confirm('stop')}>{t('Stop runtime')}</button
        >
      </div>
    {/if}
  </section>

  <section class="rt-card" aria-labelledby="rt-lease">
    <header>
      <h4 id="rt-lease">{t('Execution lease')}</h4>
      {#if lease}
        <span
          class="pill compact"
          data-tone={lease.state === 'active' ? 'ok' : 'idle'}
        >
          <span class="dot" aria-hidden="true"></span>
          {tPhrase(leaseLine(lease, now))}
        </span>
      {:else}
        <span class="pill compact" data-tone="idle">
          <span class="dot" aria-hidden="true"></span>
          {t('None')}
        </span>
      {/if}
    </header>
    {#if lease && leaseActive}
      <ul class="rt-chips" aria-label={t('Granted')}>
        {#each lease.scopes as scope (scope)}
          <li title={t(SCOPE_NOTE[scope])}>{t(SCOPE_LABEL[scope])}</li>
        {/each}
      </ul>
      <p class="composer-note">
        {t('Valid until')}
        <time datetime={lease.expires_at}>{localClock(lease.expires_at)}</time
        >{t(
          '. When it ends or is revoked, a turn that runs under it ends, and so does the approval it waits on.',
        )}
      </p>
      {#if confirming === 'revoke'}
        <div
          class="close-confirm"
          role="group"
          aria-label={t('Confirm revoke')}
        >
          <p>
            {t(
              'Revoke the lease? The turn that runs under it ends and its pending approval is invalidated, in the same commit.',
            )}
          </p>
          <div class="session-actions">
            <button
              type="button"
              class="action danger"
              disabled={!acts}
              onclick={() => controller.revokeLease()}
              >{t('Revoke lease')}</button
            >
            <button
              type="button"
              class="action"
              onclick={() => controller.confirm(null)}>{t('Keep it')}</button
            >
          </div>
        </div>
      {:else}
        <div class="session-actions">
          <label class="rt-select">
            <span>{t('Renew for')}</span>
            <select
              value={f8.lease.ttl}
              disabled={!acts}
              onchange={(e) => controller.setTtl(Number(e.currentTarget.value))}
            >
              {#each LEASE_TTLS as ttl (ttl)}
                <option value={ttl}>{tPhrase(ttlLabel(ttl))}</option>
              {/each}
            </select>
          </label>
          <button
            type="button"
            class="action"
            disabled={!acts || !ready}
            onclick={() => controller.renewLease()}>{t('Renew')}</button
          >
          <button
            type="button"
            class="action danger"
            disabled={!acts}
            onclick={() => controller.confirm('revoke')}>{t('Revoke')}</button
          >
        </div>
      {/if}
    {:else if open}
      <fieldset class="rt-scopes">
        <legend>{t('Allow')}</legend>
        {#each LEASE_SCOPES as scope (scope)}
          <label class="mode-option">
            <input
              type="checkbox"
              checked={f8.lease.scopes.includes(scope)}
              disabled={!acts}
              onchange={() => controller.toggleScope(scope)}
            />
            <span class="mode-name">{t(SCOPE_LABEL[scope])}</span>
            <span class="mode-detail">{t(SCOPE_NOTE[scope])}</span>
          </label>
        {/each}
      </fieldset>
      <div class="session-actions">
        <label class="rt-select">
          <span>{t('For')}</span>
          <select
            value={f8.lease.ttl}
            disabled={!acts}
            onchange={(e) => controller.setTtl(Number(e.currentTarget.value))}
          >
            {#each LEASE_TTLS as ttl (ttl)}
              <option value={ttl}>{tPhrase(ttlLabel(ttl))}</option>
            {/each}
          </select>
        </label>
        <button
          type="button"
          class="action primary"
          disabled={!acts || !ready || f8.lease.scopes.length === 0}
          onclick={() => controller.grantLease()}>{t('Grant lease')}</button
        >
      </div>
      <p class="composer-note">
        {t(
          'A lease is a permission with an end, never a standing one. It needs a ready runtime',
        )}{ready ? '' : ` — ${t('start it first')}`}.
      </p>
    {/if}
  </section>

  <section class="rt-card" aria-labelledby="rt-workspace">
    <header>
      <h4 id="rt-workspace">{t('Workspace')}</h4>
      <span class="pill compact" data-tone={BINDING_STATE[binding.state].tone}>
        <span class="dot" aria-hidden="true"></span>
        {t(BINDING_STATE[binding.state].label)}
      </span>
    </header>
    <dl class="session-facts">
      <dt>{t('Folder')}</dt>
      <dd class:mono={!!binding.workspace}>
        {binding.workspace ? visible(binding.workspace) : t('None')}
        {#if binding.workspace}
          <span class="fact-note">{folderName(binding.workspace)}</span>
        {/if}
      </dd>
      <dt>{t('Attached as')}</dt>
      <dd>{binding.mode ? ATTACHMENT_LABEL[binding.mode] : '—'}</dd>
      <dt>{t('Model in use')}</dt>
      <dd data-testid="observed-model">
        {tPhrase(
          observedModelLine({
            observed_model: binding.observed_model,
            binding: binding.state,
          }),
        )}
      </dd>
    </dl>
    {#if binding.state === 'indeterminate'}
      <div class="session-uncertain" role="status">
        <p class="uncertain-title">{t('Binding outcome uncertain')}</p>
        <p>
          {t(
            'The authority asked Codex to create a thread and could not confirm what Codex did. A second request could create a second thread, so nothing asks again by itself.',
          )}
        </p>
      </div>
    {/if}
    {#if confirming === 'abandon'}
      <div class="close-confirm" role="group" aria-label={t('Confirm discard')}>
        <p>
          {t(
            'Discard the uncertain attempt? The authority records that it was abandoned. If Codex did create a thread for it, the empty thread is left behind. The session can then be bound again.',
          )}
        </p>
        <div class="session-actions">
          <button
            type="button"
            class="action danger"
            disabled={!acts}
            onclick={() => controller.abandonBinding()}
            >{t('Discard attempt')}</button
          >
          <button
            type="button"
            class="action"
            onclick={() => controller.confirm(null)}>{t('Keep it')}</button
          >
        </div>
      </div>
    {:else if open}
      <div class="session-actions">
        {#if binding.state === 'unbound'}
          <button
            type="button"
            class="action"
            disabled={!acts || f8.composer.choosing}
            aria-busy={f8.composer.choosing}
            onclick={() => controller.chooseFolder()}
            >{folder ? t('Change folder') : t('Choose folder')}</button
          >
          <button
            type="button"
            class="action primary"
            disabled={!acts || !ready || !leaseActive || folder === null}
            onclick={() => controller.bindWorkspace()}
            >{t('Bind workspace')}</button
          >
        {:else if binding.state === 'bound_detached'}
          <button
            type="button"
            class="action primary"
            disabled={!acts || !ready || !leaseActive}
            onclick={() => controller.resumeBinding()}
            >{t('Attach to runtime')}</button
          >
        {:else if binding.state === 'indeterminate'}
          <button
            type="button"
            class="action"
            disabled={!acts}
            onclick={() => controller.confirm('abandon')}
            >{t('Discard attempt')}</button
          >
        {/if}
      </div>
      {#if binding.state === 'unbound'}
        <p class="composer-note">
          {t('Folder:')}
          <span class="mono">{folder ?? t('none selected')}</span>. {t(
            'The authority decides whether it can be a workspace.',
          )}
        </p>
      {/if}
    {/if}
  </section>

  <section class="rt-card" aria-labelledby="rt-gates">
    <header>
      <h4 id="rt-gates">{t('What the authority would allow now')}</h4>
    </header>
    <ul class="rt-gates">
      {#each GATE_ACTIONS as action (action)}
        {@const gate = detail.gates.find((g) => g.action === action)}
        <li>
          <span
            class="pill compact"
            data-tone={gate?.reason === 'allowed' ? 'ok' : 'idle'}
          >
            <span class="dot" aria-hidden="true"></span>
            {gate?.reason === 'allowed' ? t('Allowed') : t('Not now')}
          </span>
          <span class="rt-gate-name">{t(SCOPE_LABEL[action])}</span>
          {#if gate && gate.reason !== 'allowed'}
            <span class="fact-note">{t(gateText(gate.reason))}</span>
          {/if}
        </li>
      {/each}
    </ul>
    <p class="composer-note">
      {t(
        'This is a reading, not a permission: the authority checks again at the moment of every action.',
      )}
    </p>
  </section>

  {#if open}
    <section class="rt-card" aria-labelledby="rt-close">
      <header>
        <h4 id="rt-close">{t('Close session')}</h4>
      </header>
      {#if confirming === 'close'}
        <div class="close-confirm" role="group" aria-label={t('Confirm close')}>
          <p>
            {t(
              'Close this session? It cannot be reopened, and its execution lease is revoked. The authority refuses while its runtime runs or a turn is active.',
            )}
          </p>
          <div class="session-actions">
            <button
              type="button"
              class="action danger"
              disabled={!acts}
              onclick={() => controller.closeSession()}
              >{t('Close session')}</button
            >
            <button
              type="button"
              class="action"
              onclick={() => controller.confirm(null)}>{t('Keep open')}</button
            >
          </div>
        </div>
      {:else}
        <div class="session-actions">
          <button
            type="button"
            class="action"
            disabled={!acts || !idle}
            onclick={() => controller.confirm('close')}>{t('Close…')}</button
          >
        </div>
        {#if !idle}
          <p class="composer-note">{t('Stop the runtime first.')}</p>
        {/if}
      {/if}
    </section>
  {/if}
</div>
