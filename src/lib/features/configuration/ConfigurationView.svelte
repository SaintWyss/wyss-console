<script lang="ts">
  import { PROVIDERS, type ConfigItem } from '../../contracts/configuration';
  import { ITEM_KINDS } from '../../contracts/configuration';
  import type { ChangesController, ChangesState } from './changes-controller';
  import type {
    ConfigurationController,
    ConfigurationState,
  } from './controller';
  import {
    CHANGE_FAILURE_TEXT,
    EFFECT_LABEL,
    NOTICE_LABEL,
    OPERATION_LABEL,
    OUTCOME_LABEL,
    RISK_LABEL,
    STORAGE_LABEL,
    VERDICT_LABEL,
    allHooksSwitch,
    changeFor,
    EFFECTIVE_LABEL,
    NOT_WRITABLE_LABEL,
    PERSISTED_LABEL,
    PROVIDER_LABEL,
    SCOPE_LABEL,
    SOURCE_LABEL,
    detailLines,
    kindCounts,
    kindName,
    KIND_LABEL,
    mayHoldSecret,
    outcomeText,
    outcomeTone,
    visibleItems,
  } from './present';

  let {
    configuration,
    controller,
    changes,
    changesController,
    visible,
  }: {
    configuration: ConfigurationState;
    controller: ConfigurationController;
    changes: ChangesState;
    changesController: ChangesController;
    visible: boolean;
  } = $props();

  const parked = $derived(
    (changes.records ?? []).filter((r) => r.state === 'parked'),
  );
  const day = (seconds: number) =>
    new Date(seconds * 1000).toLocaleString(undefined, {
      dateStyle: 'medium',
      timeStyle: 'short',
    });

  const reply = $derived(configuration.reply);
  const allHooks = $derived(allHooksSwitch(reply?.inventory.items ?? []));
  const items = $derived(
    reply
      ? visibleItems(
          reply.inventory.items,
          configuration.provider,
          configuration.kind,
        )
      : [],
  );
  const counts = $derived(
    kindCounts(reply?.inventory.items ?? [], configuration.provider),
  );
  const selected = $derived(
    configuration.selected === null
      ? null
      : (reply?.inventory.items.find(
          (item) => item.id === configuration.selected,
        ) ?? null),
  );
  const lines = $derived(selected ? detailLines(selected.detail) : []);
  const selectedChange = $derived(selected ? changeFor(selected) : null);
  const problems = $derived(
    (reply?.inventory.sources ?? []).filter(
      (source) =>
        source.outcome === 'failed' && source.failure.failure !== 'missing',
    ).length,
  );

  // The details open above the list: the view goes back to the top so they are seen.
  let main = $state<HTMLElement | null>(null);
  function toggle(item: ConfigItem) {
    controller.select(configuration.selected === item.id ? null : item.id);
    if (main) main.scrollTop = 0;
  }
</script>

<section
  class="configuration-feature"
  aria-label="Configuration"
  hidden={!visible}
  inert={!visible}
>
  <div class="sessions-toolbar">
    <div class="sessions-heading">
      <h2>Configuration</h2>
      <p class="sessions-count">
        {#if reply}
          {reply.inventory.items.length}
          {reply.inventory.items.length === 1 ? 'item' : 'items'} from {reply
            .inventory.sources.length} places
        {:else if configuration.phase === 'unavailable'}
          Unavailable
        {:else}
          Reading
        {/if}
      </p>
    </div>
    <button
      type="button"
      class="action"
      disabled={configuration.phase === 'loading'}
      onclick={() => controller.refresh()}>Refresh</button
    >
  </div>

  <p class="configuration-note">
    This is what Claude Code and Codex are set up with, read from their own
    files. Nothing changes until you confirm a plan, and Claude Code hooks and
    skills and Codex skills and MCP servers can be changed here. Secrets in
    commands, environments and headers are never shown.
  </p>

  {#if changes.plan}
    {@const plan = changes.plan}
    <section class="configuration-plan" aria-label="Change to confirm">
      <h3>{OPERATION_LABEL[plan.operation]}: {plan.name}</h3>
      <p class="event-meta">
        {PROVIDER_LABEL[plan.provider]} · {SOURCE_LABEL[plan.source]}
      </p>
      {#if plan.diff.length > 0}
        <ul class="configuration-diff" aria-label="What would change">
          {#each plan.diff as line, index (index)}
            <li data-kind={line.kind}>
              <span aria-hidden="true"
                >{line.kind === 'removed'
                  ? '−'
                  : line.kind === 'added'
                    ? '+'
                    : ''}</span
              >
              <span class="mono">{line.text}</span>
            </li>
          {/each}
        </ul>
      {/if}
      {#if plan.holds_hidden_values}
        <p class="configuration-warning">
          This entry holds values that are not shown here.
        </p>
      {/if}
      {#if plan.storage}
        <p class="event-meta">{STORAGE_LABEL[plan.storage]}</p>
      {/if}
      <p class="event-meta">{EFFECT_LABEL[plan.effect]}</p>
      {#if plan.managed_effect}
        <p class="event-meta">
          Prediction, not a reading: managed Inference {VERDICT_LABEL[
            plan.managed_effect.before
          ]} now
          {#if plan.managed_effect.before === plan.managed_effect.after}
            and this change does not alter that.
          {:else}
            and would {VERDICT_LABEL[plan.managed_effect.after]} after it.
          {/if}
        </p>
      {/if}
      {#if RISK_LABEL[plan.risk]}
        <p class="configuration-warning">{RISK_LABEL[plan.risk]}</p>
      {/if}
      <div class="configuration-plan-actions">
        <button
          type="button"
          class="action primary"
          disabled={changes.applying}
          onclick={() => changesController.apply()}
          >{changes.applying
            ? plan.needs_native_dialog
              ? 'Waiting for the confirmation window…'
              : 'Applying…'
            : plan.needs_native_dialog
              ? 'Continue to confirm'
              : 'Apply'}</button
        >
        <button
          type="button"
          class="action"
          disabled={changes.applying}
          onclick={() => changesController.cancel()}>Cancel</button
        >
      </div>
    </section>
  {/if}
  {#if changes.planning}
    <p class="sessions-status" role="status">Preparing the change…</p>
  {/if}
  {#if changes.result}
    <p
      class="sessions-status"
      class:failed={OUTCOME_LABEL[changes.result].tone === 'error' ||
        OUTCOME_LABEL[changes.result].tone === 'warn'}
      role="status"
    >
      {OUTCOME_LABEL[changes.result].text}
    </p>
  {/if}
  {#if changes.failure}
    <p class="sessions-status failed" role="alert">
      {CHANGE_FAILURE_TEXT[changes.failure]}
    </p>
  {/if}
  {#if changes.notice}
    <p class="sessions-status" role="status">{NOTICE_LABEL[changes.notice]}</p>
  {/if}

  {#if configuration.stale}
    <p class="sessions-status failed" role="status">
      This could not be read again just now. What is shown is from the last
      successful read.
    </p>
  {/if}
  {#if reply?.inventory.truncated}
    <p class="sessions-status" role="status">
      There are more items than this app reads at once. Some are not listed.
    </p>
  {/if}

  {#if configuration.phase === 'loading' || configuration.phase === 'idle'}
    <div class="sessions-empty" role="status" aria-busy="true">
      <p class="empty-title">Reading configuration…</p>
    </div>
  {:else if configuration.phase === 'unavailable'}
    <div class="sessions-empty" role="alert">
      <p class="empty-title">There is no configuration to read here</p>
      <p>
        The app could not find where your account keeps its files. Nothing is
        shown, which does not mean there is nothing configured.
      </p>
    </div>
  {:else if configuration.phase === 'failed'}
    <div class="sessions-empty" role="alert">
      <p class="empty-title">The configuration could not be read</p>
      <button type="button" class="action" onclick={() => controller.refresh()}
        >Try again</button
      >
    </div>
  {:else if reply}
    <div class="events-filters" role="group" aria-label="View">
      <button
        type="button"
        class="action chip"
        aria-pressed={changes.panel === 'items'}
        onclick={() => changesController.show('items')}>Items</button
      >
      <button
        type="button"
        class="action chip"
        aria-pressed={changes.panel === 'parked'}
        onclick={() => changesController.show('parked')}>Turned off</button
      >
      <button
        type="button"
        class="action chip"
        aria-pressed={changes.panel === 'history'}
        onclick={() => changesController.show('history')}>History</button
      >
      {#if allHooks && changes.panel === 'items'}
        <button
          type="button"
          class="action chip"
          disabled={changes.planning || changes.applying}
          onclick={() =>
            changesController.draft(allHooks.operation, 'claude_user_settings')}
          >{allHooks.label}</button
        >
      {/if}
    </div>
    {#if changes.panel === 'history'}
      <div class="configuration-main">
        {#if changes.history === null}
          <div class="sessions-empty" role="status" aria-busy="true">
            <p class="empty-title">Reading the history…</p>
          </div>
        {:else if changes.history.changes.length === 0}
          <div class="sessions-empty">
            <p class="empty-title">No changes yet</p>
            <p>Changes you make here are listed, without their content.</p>
          </div>
        {:else}
          <ul class="events-list" aria-label="Changes">
            {#each changes.history.changes as change (change.id)}
              <li class="configuration-history-row">
                <span class="event-title"
                  >{OPERATION_LABEL[change.operation]}</span
                >
                <span class="event-meta">
                  {PROVIDER_LABEL[change.provider]} · {SOURCE_LABEL[
                    change.source
                  ]} · {day(change.at)}
                  {#if change.undoes}
                    · an undo
                  {/if}
                </span>
                <span
                  class="pill compact"
                  data-tone={change.outcome
                    ? OUTCOME_LABEL[change.outcome].tone
                    : 'pending'}
                >
                  <span class="dot" aria-hidden="true"></span>
                  {change.outcome
                    ? OUTCOME_LABEL[change.outcome].text
                    : 'Not finished'}
                </span>
                {#if (change.outcome === 'applied' || change.outcome === 'recovered_applied') && !change.undoes}
                  <button
                    type="button"
                    class="action chip"
                    disabled={changes.planning || changes.applying}
                    onclick={() => changesController.undo(change.id)}
                    >Undo</button
                  >
                {/if}
              </li>
            {/each}
          </ul>
          {#if changes.history.next}
            <button
              type="button"
              class="action"
              onclick={() => changesController.loadHistory(true)}
              >Older changes</button
            >
          {/if}
        {/if}
      </div>
    {:else if changes.panel === 'parked'}
      <div class="configuration-main">
        <div class="configuration-plan-actions">
          <button
            type="button"
            class="action"
            disabled={changes.busy}
            onclick={() => changesController.importRecord()}
            >Import a kept hook…</button
          >
        </div>
        {#if changes.records === null}
          <div class="sessions-empty" role="status" aria-busy="true">
            <p class="empty-title">Reading…</p>
          </div>
        {:else if parked.length === 0}
          <div class="sessions-empty">
            <p class="empty-title">Nothing is turned off</p>
            <p>
              A hook you turn off is removed from its file and kept here, so it
              can be put back exactly.
            </p>
          </div>
        {:else}
          <ul class="events-list" aria-label="Turned-off hooks">
            {#each parked as record (record.id)}
              <li class="configuration-history-row">
                <span class="event-title">{record.label}</span>
                <span class="event-meta">
                  Kept {day(record.created)} ·
                  {record.protection === 'sealed'
                    ? 'encrypted'
                    : 'not encrypted (no secret in it)'}
                  {#if record.secret_bearing}
                    · holds hidden values
                  {/if}
                  {#if record.past_retention}
                    · kept past the usual time; it stays until you decide
                  {/if}
                </span>
                <span class="configuration-record-actions">
                  <button
                    type="button"
                    class="action chip"
                    disabled={changes.planning ||
                      changes.applying ||
                      changes.busy}
                    onclick={() =>
                      changesController.draft('restore_hook', record.id)}
                    >Put back</button
                  >
                  <button
                    type="button"
                    class="action chip"
                    disabled={changes.busy}
                    onclick={() => changesController.exportRecord(record.id)}
                    >Export…</button
                  >
                  <button
                    type="button"
                    class="action chip"
                    disabled={changes.busy}
                    onclick={() => changesController.deleteRecord(record.id)}
                    >Forget</button
                  >
                </span>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {:else}
      <div class="events-filters" role="group" aria-label="Filter by agent">
        <button
          type="button"
          class="action chip"
          aria-pressed={configuration.provider === 'all'}
          onclick={() => controller.setProvider('all')}>All agents</button
        >
        {#each PROVIDERS as provider (provider)}
          <button
            type="button"
            class="action chip"
            aria-pressed={configuration.provider === provider}
            onclick={() => controller.setProvider(provider)}
            >{PROVIDER_LABEL[provider]}</button
          >
        {/each}
      </div>
      <div class="events-filters" role="group" aria-label="Filter by kind">
        <button
          type="button"
          class="action chip"
          aria-pressed={configuration.kind === 'all'}
          onclick={() => controller.setKind('all')}
          >Everything {counts.all}</button
        >
        {#each ITEM_KINDS as kind (kind)}
          <button
            type="button"
            class="action chip"
            aria-pressed={configuration.kind === kind}
            onclick={() => controller.setKind(kind)}
            >{KIND_LABEL[kind]} {counts[kind]}</button
          >
        {/each}
      </div>

      <div class="configuration-main" bind:this={main}>
        {#if selected}
          <aside class="configuration-detail" aria-label="Details">
            <h3>{selected.name}</h3>
            <dl>
              <dt>Agent</dt>
              <dd>{PROVIDER_LABEL[selected.provider]}</dd>
              <dt>Kind</dt>
              <dd>{kindName(selected.kind)}</dd>
              <dt>Found in</dt>
              <dd>{SOURCE_LABEL[selected.source]}</dd>
              <dt>In the file</dt>
              <dd>{PERSISTED_LABEL[selected.persisted].label}</dd>
              <dt>In use</dt>
              <dd>{EFFECTIVE_LABEL[selected.effective]}</dd>
              <dt>Changing it</dt>
              <dd>
                {#if selectedChange}
                  You can do it here, with a plan you confirm.
                {:else}
                  {NOT_WRITABLE_LABEL[selected.not_writable]}
                {/if}
              </dd>
              {#if selected.hash}
                <dt>Fingerprint</dt>
                <dd class="mono">{selected.hash.slice(0, 12)}</dd>
              {/if}
            </dl>
            {#if selectedChange}
              <div class="configuration-plan-actions">
                <button
                  type="button"
                  class="action"
                  disabled={changes.planning || changes.applying}
                  onclick={() =>
                    changesController.draft(
                      selectedChange.operation,
                      selected.id,
                    )}>{selectedChange.label}…</button
                >
              </div>
            {/if}
            {#if lines.length > 0}
              <h4>Settings</h4>
              <ul class="configuration-lines">
                {#each lines as line (line.name)}
                  <li>
                    <span class="mono"
                      >{line.name === '' ? 'value' : line.name}</span
                    >
                    <span class="configuration-value" class:hidden={line.hidden}
                      >{line.value}</span
                    >
                  </li>
                {/each}
              </ul>
            {/if}
          </aside>
        {/if}
        {#if items.length === 0}
          <div class="sessions-empty">
            <p class="empty-title">Nothing to show</p>
            <p>
              {#if reply.inventory.items.length === 0}
                No hooks, skills, servers, rules or model settings were found in
                the places that were read.
              {:else}
                Nothing matches this filter.
              {/if}
            </p>
          </div>
        {:else}
          <ul class="events-list" aria-label="Configuration items">
            {#each items as item (item.id)}
              <li>
                <button
                  type="button"
                  class="event-row configuration-row"
                  aria-pressed={configuration.selected === item.id}
                  onclick={() => toggle(item)}
                >
                  <span class="configuration-provider"
                    >{PROVIDER_LABEL[item.provider]}</span
                  >
                  <span class="event-title">{item.name}</span>
                  <span
                    class="pill compact"
                    data-tone={PERSISTED_LABEL[item.persisted].tone}
                  >
                    <span class="dot" aria-hidden="true"></span>
                    {PERSISTED_LABEL[item.persisted].label}
                  </span>
                  <span class="event-meta">
                    {kindName(item.kind)} · {SCOPE_LABEL[item.scope]}
                    {#if mayHoldSecret(item.detail)}
                      · values hidden
                    {/if}
                  </span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}

        <details class="configuration-sources">
          <summary>
            Where this was read from
            {#if problems > 0}
              · {problems} {problems === 1 ? 'problem' : 'problems'}
            {/if}
          </summary>
          <ul>
            {#each reply.inventory.sources as source (source.key)}
              <li>
                <span class="pill compact" data-tone={outcomeTone(source)}>
                  <span class="dot" aria-hidden="true"></span>
                  {SOURCE_LABEL[source.key]}
                </span>
                <span class="event-meta">
                  {PROVIDER_LABEL[source.provider]} · {outcomeText(source)}
                  {#if source.outcome === 'read' && source.trust.trust === 'read_only'}
                    Its folder is shared with a group, so it is only read.
                  {/if}
                </span>
              </li>
            {/each}
          </ul>
        </details>
      </div>
    {/if}
  {/if}
</section>
