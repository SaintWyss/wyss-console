<script lang="ts">
  import type { ConfigItem } from '../../contracts/configuration';
  import type { Profile } from '../../contracts/native';
  import type { SessionSummary } from '../../contracts/sessions';
  import type { ModelsController, ModelsState } from './controller';
  import {
    ENTITLEMENT_NOTE,
    catalogueFailure,
    NATIVE_NOTE,
    SELECTED_NOTE,
    completeness,
    configuredRows,
    entryRow,
    observedRows,
    provenance,
    selectedRows,
  } from './present';

  let {
    models,
    controller,
    configItems,
    profiles,
    sessions,
    visible,
  }: {
    models: ModelsState;
    controller: ModelsController;
    /** The inventory the Configuration view reads; null until it has been. */
    configItems: readonly ConfigItem[] | null;
    profiles: readonly Profile[];
    /** The managed sessions of the newest page; null until the page was read. */
    sessions: readonly SessionSummary[] | null;
    visible: boolean;
  } = $props();

  const catalogue = $derived(models.catalogue);
  const offered = $derived(catalogue ? catalogue.entries.map(entryRow) : []);
  const configured = $derived(configuredRows(configItems));
  const selected = $derived(selectedRows(profiles));
  const observed = $derived(observedRows(sessions));
</script>

<section
  class="native-feature models-view"
  aria-label="Models"
  hidden={!visible}
  inert={!visible}
>
  <p class="native-note">
    Four different things, kept apart: what a runtime <strong>offers</strong>,
    what an agent’s files are <strong>configured</strong> with, what a profile
    <strong>selects</strong> for a session it starts, and what a runtime
    <strong>reported in use</strong>. None is filled from another. A default in
    a list, a value in a file or a name passed at start says what was asked for,
    never what answered.
  </p>

  <h3>Offered by the managed Codex runtime</h3>
  {#if models.phase === 'loading' || models.phase === 'idle'}
    <p class="sessions-status" role="status" aria-busy="true">
      Reading the runtime’s list…
    </p>
  {:else if models.phase === 'failed'}
    <p class="sessions-status failed" role="alert">
      {catalogueFailure(models.failure)}
    </p>
    <button type="button" class="action" onclick={() => controller.refresh()}
      >Try again</button
    >
  {:else if catalogue}
    {#if models.stale}
      <p class="sessions-status failed" role="status">
        The last read failed. What is shown is older.
      </p>
    {/if}
    <p class="native-note" data-testid="provenance">
      {provenance(catalogue)}
    </p>
    {#if catalogue.state === 'listed'}
      <p class="native-note">{ENTITLEMENT_NOTE}</p>
      {#if completeness(catalogue)}
        <p class="native-note">{completeness(catalogue)}</p>
      {/if}
      {#if offered.length === 0}
        <p class="native-note">The runtime listed no model.</p>
      {:else}
        <ul class="events-list" aria-label="Models the runtime offers">
          {#each offered as row (row.id)}
            <li class="models-row">
              <span class="event-title">
                {row.title}
                {#if row.isDefault}
                  <span class="pill compact" data-tone="ok"
                    >default of this runtime</span
                  >
                {/if}
              </span>
              <span class="event-meta">
                {row.provider ? `Provider ${row.provider} · ` : ''}Efforts:
                {row.efforts ?? 'not reported by this authority version'}
              </span>
              {#if row.replacement}
                <span class="event-meta">{row.replacement}</span>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
    <div class="native-plan-actions">
      <button type="button" class="action" onclick={() => controller.refresh()}
        >Read again</button
      >
    </div>
  {/if}

  <h3>Configured in the agents’ own files</h3>
  {#if configItems === null}
    <p class="native-note">
      Not read yet. The Configuration view reads these files.
    </p>
  {:else if configured.length === 0}
    <p class="native-note">No model setting was found in the places read.</p>
  {:else}
    <ul class="events-list" aria-label="Configured models">
      {#each configured as row, index (`${row.provider}:${row.key}:${row.source}:${index}`)}
        <li class="models-row">
          <span class="event-title"
            >{row.providerName} · {row.key}: {row.value}</span
          >
          <span class="event-meta">
            {row.source} · {row.scope}. As the file says; it applies to new
            sessions, and what a running session does with a change is not
            known.
          </span>
        </li>
      {/each}
    </ul>
  {/if}

  <h3>Selected by a profile</h3>
  <p class="native-note">{SELECTED_NOTE}</p>
  {#if selected.length === 0}
    <p class="native-note">No profile names a model or an effort.</p>
  {:else}
    <ul class="events-list" aria-label="Models selected by profiles">
      {#each selected as row (row.profile)}
        <li class="models-row">
          <span class="event-title"
            >{row.profile}{row.template ? ' (template)' : ''}</span
          >
          <span class="event-meta">
            {row.agents} · Model: {row.model ?? 'the agent’s own choice'} · Effort:
            {row.effort ?? 'the agent’s own choice'}{row.inherited.length
              ? ` · inherited: ${row.inherited.join(', ')}`
              : ''}
          </span>
        </li>
      {/each}
    </ul>
  {/if}

  <h3>Reported in use</h3>
  {#if sessions === null}
    <p class="native-note">Managed sessions have not been read yet.</p>
  {:else if observed.length === 0}
    <p class="native-note">No managed session is attached.</p>
  {:else}
    <ul class="events-list" aria-label="Models reported by the runtime">
      {#each observed as row (row.id)}
        <li class="models-row">
          <span class="event-title">{row.label}</span>
          <span class="event-meta">{row.line}</span>
        </li>
      {/each}
    </ul>
  {/if}
  <p class="native-note">{NATIVE_NOTE}</p>
</section>
