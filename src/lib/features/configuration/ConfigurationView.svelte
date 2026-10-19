<script lang="ts">
  import { PROVIDERS, type ConfigItem } from '../../contracts/configuration';
  import { ITEM_KINDS } from '../../contracts/configuration';
  import type {
    ConfigurationController,
    ConfigurationState,
  } from './controller';
  import {
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
    visible,
  }: {
    configuration: ConfigurationState;
    controller: ConfigurationController;
    visible: boolean;
  } = $props();

  const reply = $derived(configuration.reply);
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
  const problems = $derived(
    (reply?.inventory.sources ?? []).filter(
      (source) =>
        source.outcome === 'failed' && source.failure.failure !== 'missing',
    ).length,
  );

  function toggle(item: ConfigItem) {
    controller.select(configuration.selected === item.id ? null : item.id);
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
    files. It is read-only: nothing here changes a file, and secrets in
    commands, environments and headers are never shown.
  </p>

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

    <div class="configuration-main">
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
            <dd>{NOT_WRITABLE_LABEL[selected.not_writable]}</dd>
            {#if selected.hash}
              <dt>Fingerprint</dt>
              <dd class="mono">{selected.hash.slice(0, 12)}</dd>
            {/if}
          </dl>
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
</section>
