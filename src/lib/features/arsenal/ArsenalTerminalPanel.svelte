<script lang="ts">
  import { untrack } from 'svelte';
  import type {
    ArsenalProvider,
    Selection,
    TerminalPlan,
  } from '../../contracts/arsenal';
  import type { AgentId, Preview } from '../../contracts/native';
  import type { ArsenalStart } from '../../desktop/native-client';
  import {
    arsenalClient,
    type ArsenalClient,
  } from '../../desktop/arsenal-client';
  import { failureMessage } from './messages';
  import { FAILURE_TEXT } from '../native/present';
  import { msg, t } from '../../i18n/index.svelte';
  import {
    ENFORCEMENT_LABEL,
    ITEM_STATE_LABEL,
    PLAN_CONFLICT_LABEL,
    planCounts,
    type ArsenalChoice,
  } from './terminal';

  let {
    agent,
    project,
    profile,
    profileKey = null,
    refreshKey = 0,
    previewer,
    onchange,
    client = arsenalClient,
    selected = $bindable(''),
    choices = true,
  }: {
    agent: AgentId | null;
    /** The launcher's project path for this terminal. */
    project: string | null;
    profile: string | null;
    /** Changes when the chosen profile's saved revision changes. */
    profileKey?: string | null;
    /** Changes when the plan must be computed again: the view is shown again, a start was refused. */
    refreshKey?: number;
    /** The Native preview of the profile with this selection applied. */
    previewer: (start: ArsenalStart) => Promise<Preview>;
    onchange: (choice: ArsenalChoice | null) => void;
    client?: ArsenalClient;
    /** The selection id; '' is none. The owner of the choice (Workspace's preset card) may bind it. */
    selected?: string;
    /** False when another control chooses the selection: the radio list and its reload are not drawn. */
    choices?: boolean;
  } = $props();

  let selections = $state<Selection[]>([]);
  let loadFailed = $state<string | null>(null);
  let plan = $state<TerminalPlan | null>(null);
  let planFailed = $state<string | null>(null);
  let planning = $state(false);
  let apply = $state(true);
  let recordOnly = $state(false);
  let preview = $state<Preview | null>(null);
  let previewing = $state(false);
  let previewFailed = $state<string | null>(null);
  let reloads = $state(0);

  const provider = $derived<ArsenalProvider | null>(
    agent === 'claude_code' || agent === 'codex' ? agent : null,
  );
  const offered = $derived(
    provider ? selections.filter((s) => s.provider === provider) : [],
  );
  const counts = $derived(plan ? planCounts(plan) : null);
  const applying = $derived(!!plan && plan.can_apply && apply);

  async function readSelections() {
    try {
      selections = await client.selections();
      loadFailed = null;
    } catch (error) {
      loadFailed = failureMessage(error);
    }
  }
  async function load() {
    reloads++;
    await readSelections();
  }
  // Selections are read when the panel appears and each time the view is shown again, so one saved
  // in Arsenal meanwhile is offered without a Reload.
  $effect(() => {
    void refreshKey;
    untrack(() => void readSelections());
  });

  // A project remembers the selection its last terminal of this provider started with.
  let remembered: string | null = null;
  $effect(() => {
    const key = provider && project ? `${provider}\u0000${project}` : null;
    if (key === remembered) return;
    remembered = key;
    selected = '';
    if (!key || !provider || !project) return;
    void client.projectSelection(project, provider).then(
      (id) => {
        if (remembered === key && id && selected === '') selected = id;
      },
      () => {},
    );
  });

  // The plan is computed by the desktop each time the selection, the agent or the project changes.
  let planSeq = 0;
  $effect(() => {
    const id = selected;
    const a = agent;
    const p = project;
    // The installed skills and the provider build can change while the plan is on screen.
    void refreshKey;
    void reloads;
    const seq = ++planSeq;
    plan = null;
    planFailed = null;
    preview = null;
    previewFailed = null;
    recordOnly = false;
    if (!id || !a || !provider) {
      planning = false;
      return;
    }
    planning = true;
    void client.terminalPlan(id, a, p).then(
      (next) => {
        if (seq !== planSeq) return;
        plan = next;
        apply = next.can_apply;
        planning = false;
      },
      (error) => {
        if (seq !== planSeq) return;
        planFailed = failureMessage(error);
        planning = false;
      },
    );
  });

  // Applying needs the Native preview of exactly this profile with this exclusion.
  let previewSeq = 0;
  $effect(() => {
    const current = plan;
    const wanted = applying;
    const key = profileKey;
    const seq = ++previewSeq;
    void key;
    void profile;
    preview = null;
    previewFailed = null;
    if (!current || !wanted || !project) {
      previewing = false;
      return;
    }
    previewing = true;
    void previewer({
      selection: current.selection,
      config_hash: current.config_hash,
      project,
    }).then(
      (next) => {
        if (seq !== previewSeq) return;
        preview = next;
        previewing = false;
      },
      (error: unknown) => {
        if (seq !== previewSeq) return;
        const code =
          (error as { failure?: { code?: string } }).failure?.code ??
          'internal';
        previewFailed = FAILURE_TEXT[code] ?? msg('The launch preview failed.');
        previewing = false;
      },
    );
  });

  $effect(() => {
    if (!plan || !project || !selected) {
      onchange(selected && provider ? { pending: true } : null);
      return;
    }
    const start: ArsenalStart = {
      selection: plan.selection,
      config_hash: plan.config_hash,
      project,
    };
    const ready = applying ? preview?.can_start === true : recordOnly;
    onchange({ pending: false, start, plan, apply: applying, preview, ready });
  });
</script>

<fieldset
  class="native-field arsenal-terminal"
  aria-label="Arsenal for this terminal"
>
  <legend id="native-arsenal-label">Arsenal selection</legend>
  {#if !agent}
    <p class="event-meta">Choose an agent first.</p>
  {:else if !provider}
    <p class="event-meta">
      Arsenal selections are for Claude Code and Codex. This agent starts as
      configured.
    </p>
  {:else}
    {#if choices}
      <div class="native-row">
        <!-- Radios, not a drop-down: every choice is visible and each one is a control of its own. -->
        <div
          id="native-arsenal"
          class="arsenal-choices"
          role="radiogroup"
          aria-labelledby="native-arsenal-label"
        >
          <label class="arsenal-check">
            <input
              type="radio"
              name="native-arsenal"
              value=""
              bind:group={selected}
            />
            None — skills as installed
          </label>
          {#each offered as selection (selection.id)}
            <label class="arsenal-check">
              <input
                type="radio"
                name="native-arsenal"
                value={selection.id}
                bind:group={selected}
              />
              {selection.name}
            </label>
          {/each}
        </div>
        <button type="button" class="action" onclick={load}>Reload</button>
      </div>
      {#if loadFailed}
        <p class="native-warning" role="alert">{loadFailed}</p>
      {:else if offered.length === 0}
        <p class="event-meta">
          No saved selection for this provider. Make one in Arsenal ›
          Selections.
        </p>
      {/if}
    {/if}

    {#if planning}
      <p class="event-meta" role="status">Building the terminal plan…</p>
    {:else if planFailed}
      <p class="native-warning" role="alert">{planFailed}</p>
    {:else if plan && counts}
      <div class="arsenal-plan" aria-live="polite">
        <p class="arsenal-plan-line">
          <span class="pill" data-state={plan.enforcement.state}
            >{ENFORCEMENT_LABEL[plan.enforcement.state]}</span
          >
          <span class="pill" data-state={plan.status}
            >{plan.status === 'ready'
              ? 'Plan ready'
              : plan.status === 'partial'
                ? 'Plan partial'
                : 'Plan blocked'}</span
          >
          <span class="event-meta"
            >{plan.provider_version ?? 'unidentified build'}{plan.project
              ? ` · ${plan.project}`
              : ' · no project'}</span
          >
        </p>
        <p class="event-meta">{plan.enforcement.reason}</p>
        <p class="event-meta">
          {counts.prepared} prepared · {counts.rejected} rejected · {counts.unsupported}
          not carried · {plan.kept.length - plan.inherited_off.length} installed skill(s)
          stay{#if plan.inherited_off.length > 0}<span>&nbsp;·&nbsp;</span>{plan
              .inherited_off.length} already off in your settings{/if} ·
          {plan.excluded.length} would be switched off
        </p>
        {#each plan.conflicts as conflict, index (index)}
          <p class={conflict.blocking ? 'native-warning' : 'event-meta'}>
            {PLAN_CONFLICT_LABEL[conflict.code]}: {conflict.names.join(', ')}
          </p>
        {/each}
        {#each plan.warnings as warning (warning)}
          <p class="event-meta">{warning}</p>
        {/each}
        <details>
          <summary>Items ({plan.items.length})</summary>
          <ul class="native-preview">
            {#each plan.items as item (item.capability)}
              <li class={item.state === 'prepared' ? '' : 'refused'}>
                <strong>{item.name || 'Missing capability'}</strong>
                · {ITEM_STATE_LABEL[item.state]}{item.revision !== null
                  ? ` · revision ${item.revision}${item.pinned ? ' (pinned)' : ''}`
                  : ''} — {item.reason}
              </li>
            {/each}
          </ul>
        </details>
        <details>
          <summary>Switched off ({plan.excluded.length})</summary>
          <p class="arsenal-member-list">
            {plan.excluded.join(', ') || 'Nothing.'}
          </p>
        </details>
        <details>
          <summary>Not changed</summary>
          <ul class="native-preview">
            {#each plan.untouched as line (line)}<li>{line}</li>{/each}
          </ul>
        </details>
        {#if plan.overlay}
          <details>
            <summary
              >{plan.provider === 'codex'
                ? 'Session flag it would add (with --no-daemon)'
                : 'Session settings it would write'}</summary
            >
            <pre class="arsenal-overlay">{plan.overlay}</pre>
          </details>
        {/if}

        {#if plan.can_apply}
          <label class="arsenal-check">
            <input type="checkbox" bind:checked={apply} />
            Apply to this terminal (restart required to change it later)
          </label>
        {/if}
        {#if applying}
          {#if previewing}
            <p class="event-meta" role="status">Checking the launch preview…</p>
          {:else if previewFailed}
            <p class="native-warning" role="alert">{t(previewFailed)}</p>
          {:else if preview && !preview.can_start}
            <p class="native-warning" role="alert">
              The launch preview refuses this combination:
              {preview.not_applicable.join(' ')}
            </p>
          {/if}
        {:else}
          <p class="native-warning" role="note">
            NOT APPLIED: this terminal will start with every installed skill.
          </p>
          <label class="arsenal-check">
            <input type="checkbox" bind:checked={recordOnly} />
            Start without applying it, and record the selection with the terminal
          </label>
        {/if}
      </div>
    {/if}
  {/if}
</fieldset>

<style>
  .arsenal-plan {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    background: var(--surface);
  }
  .arsenal-plan-line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .arsenal-choices {
    display: flex;
    flex-direction: column;
    flex: 1;
  }
  .arsenal-check {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
  }
  .arsenal-overlay,
  .arsenal-member-list {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    overflow-wrap: anywhere;
    white-space: pre-wrap;
    max-height: 240px;
    overflow-y: auto;
  }
  @media (max-width: 767px) {
    .arsenal-overlay,
    .arsenal-member-list {
      font-size: var(--text-sm);
    }
  }
</style>
