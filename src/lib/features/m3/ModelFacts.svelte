<script lang="ts">
  import type {
    FeatureInfo,
    ModelFactsInfo,
    ProviderReportInfo,
  } from '../../contracts/m3';
  import { effortLine, localTime, reasonText, rerouteLine } from './present';

  let {
    report,
    selection,
  }: {
    report: ProviderReportInfo | null;
    selection: FeatureInfo;
  } = $props();
  const facts = $derived<ModelFactsInfo | null>(report?.model ?? null);
</script>

<div class="m3-facts" data-testid="model-facts">
  {#if !facts}
    <p class="composer-note">
      No Claude runtime is running, so there are no model facts to show. A
      listed model is offered, not entitled, and a request is not proof of use:
      when a runtime runs, this view keeps what is offered, configured,
      requested and reported in use apart.
    </p>
  {:else}
    <div class="m3-four">
      <section aria-labelledby="mf-offered">
        <h5 id="mf-offered">Offered</h5>
        {#if facts.offered}
          <p class="m3-meta" data-testid="offered-source">
            The provider’s own list · revision {facts.offered.revision} ·
            {facts.offered.freshness} · taken {localTime(
              facts.offered.observed_at,
            )}
            {#if !facts.offered.complete}· incomplete{/if}
          </p>
          <ul class="rt-model-list" aria-label="Models offered">
            {#each facts.offered.entries as e (e.model_id)}
              <li class="rt-model">
                <p class="rt-model-name">
                  {e.alias ?? e.model_id}
                  {#if e.is_default}<span class="pill compact" data-tone="ok"
                      ><span class="dot" aria-hidden="true"></span>Default</span
                    >{/if}
                </p>
                <p class="rt-model-id mono">{e.model_id}</p>
                <p class="m3-meta">
                  Efforts it lists: {e.efforts.join(', ') || 'none'}
                </p>
              </li>
            {/each}
          </ul>
          <p class="m3-meta" data-testid="not-entitlement">
            Offered is not entitled: nothing here says this account may use a
            model.
          </p>
        {:else}
          <p class="m3-meta">Not listed yet.</p>
        {/if}
      </section>
      <section aria-labelledby="mf-configured">
        <h5 id="mf-configured">Configured</h5>
        {#if facts.configured}
          <p class="mono">{facts.configured.model}</p>
          <p class="m3-meta">
            Effort: {facts.configured.effort ?? 'the provider default'}
          </p>
        {:else}
          <p class="m3-meta">
            Nothing was pinned when the runtime started: the provider’s default.
          </p>
        {/if}
      </section>
      <section aria-labelledby="mf-requested">
        <h5 id="mf-requested">Requested</h5>
        {#if facts.requested}
          <p class="mono">{facts.requested.resolved_model}</p>
          <p class="m3-meta">
            For turn {facts.requested.turn_id.slice(0, 8)} · generation
            {facts.requested.generation}
          </p>
        {:else}
          <p class="m3-meta">No model was requested for the latest turn.</p>
        {/if}
      </section>
      <section aria-labelledby="mf-reported">
        <h5 id="mf-reported">Reported in use</h5>
        {#if facts.reported_in_use.length > 0}
          <ul class="rt-chips">
            {#each facts.reported_in_use as m (m)}<li class="mono">
                {m}
              </li>{/each}
          </ul>
        {:else}
          <p class="m3-meta">The provider has not named a model yet.</p>
        {/if}
        <p
          class="m3-meta"
          data-testid="reroute"
          data-state={facts.reroute.state}
        >
          {rerouteLine(facts.reroute)}
        </p>
      </section>
    </div>
    <p
      class="m3-meta"
      data-testid="effort-evidence"
      data-state={facts.effort.state}
    >
      {effortLine(facts.effort)}
    </p>
  {/if}

  <div class="m3-select" data-testid="model-selection">
    <label>
      <span class="mode-name">Model for the next turn</span>
      <select disabled={!selection.available}>
        <option>The provider’s default</option>
        {#each facts?.offered?.entries ?? [] as e (e.model_id)}
          <option>{e.alias ?? e.model_id}</option>
        {/each}
      </select>
    </label>
    <label>
      <span class="mode-name">Reasoning effort</span>
      <select disabled>
        <option>Fixed when the runtime starts</option>
      </select>
    </label>
    {#if !selection.available}
      <ul class="m3-reasons" aria-label="Why model selection is off">
        {#each selection.reasons as r (r)}<li>{reasonText(r)}</li>{/each}
      </ul>
    {/if}
    <p class="m3-meta">
      Claude’s SDK can change the model for a turn, but reads the effort once,
      when its runtime starts. A request that cannot be honored is refused; it
      is never replaced by another model.
    </p>
  </div>
</div>
