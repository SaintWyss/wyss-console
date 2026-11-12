<script lang="ts">
  import type {
    ArsenalProvider,
    Compatibility,
    Item,
    LibraryItem,
    Selection,
  } from '../../contracts/arsenal';
  import { t } from '../../i18n/index.svelte';
  import { msg } from '../../i18n/msg';
  import { compatibilityMark } from './catalogue';
  import type { ArsenalController, ArsenalState } from './controller';
  import {
    GROUPS,
    familyFor,
    groupFor,
    type Family,
    type Group,
  } from './groups';
  import {
    METER_CELLS,
    familiesOf,
    filterChooser,
    groupRows,
    isDirty,
    keptComposites,
    meterFill,
    planKey,
    summaryOf,
    totalsOf,
    type Draft,
    type GroupRow,
  } from './presets';

  let {
    provider,
    arsenal,
    controller,
  }: {
    provider: ArsenalProvider;
    arsenal: ArsenalState;
    controller: ArsenalController;
  } = $props();

  const PROVIDER_LABEL: Record<ArsenalProvider, string> = {
    claude_code: 'Claude Code',
    codex: 'Codex',
  };
  const GROUP_LABEL: Record<Group, string> = {
    models: msg('Models and rules'),
    security: msg('Security'),
    debug: msg('Debugging'),
    content: msg('Content and SEO'),
    motion: msg('Animation and video'),
    design: msg('Design and interface'),
    study: msg('Study and knowledge'),
    decide: msg('Decisions'),
    research: msg('Research'),
    review: msg('Review and testing'),
    docs: msg('Docs and writing'),
    planning: msg('Planning and method'),
    build: msg('Building'),
    system: msg('System and setup'),
    other: msg('Other'),
  };
  const KIND_LABEL: Record<string, string> = {
    skill: msg('Skill'),
    agent: msg('Agent'),
    rule: msg('Rule'),
    hook: msg('Hook'),
    mcp_server: msg('MCP server'),
    plugin: msg('Plugin'),
  };
  const COMPATIBILITY_LABEL: Record<Compatibility, string> = {
    supported: msg('Supported'),
    unverified: msg('Unverified'),
    requires_qualification: msg('Needs provider qualification'),
    missing_dependency: msg('Missing dependency'),
    conflicting: msg('Conflicting'),
    unsupported: msg('Unsupported'),
  };
  const CONFLICT_LABEL: Record<string, string> = {
    name_collision: msg('Same name'),
    duplicate_content: msg('Same content'),
    instruction_overlap: msg('Two rules for one provider'),
    methodology_conflict: msg('Two methodologies'),
    scope_shadow: msg('Installed in account and project'),
  };
  const KIND_FILTERS: {
    value: 'all' | 'skill' | 'agent' | 'rule';
    label: string;
  }[] = [
    { value: 'all', label: msg('All') },
    { value: 'rule', label: msg('Rules') },
    { value: 'skill', label: msg('Skills') },
    { value: 'agent', label: msg('Agents') },
  ];
  /** What each type does for the agent, always listed under the type filter. */
  const KIND_HELP: {
    value: 'skill' | 'agent' | 'rule';
    label: string;
    text: string;
  }[] = [
    {
      value: 'rule',
      label: msg('Rules'),
      text: msg(
        'Always on. Standing instructions the agent reads when the session starts, such as invariantes.md. They set how it works; nothing runs.',
      ),
    },
    {
      value: 'skill',
      label: msg('Skills'),
      text: msg(
        'On demand. A guide for one kind of task; the agent loads it only when the task calls for it.',
      ),
    },
    {
      value: 'agent',
      label: msg('Agents'),
      text: msg(
        'Delegated. A helper with its own role; the main agent hands it a subtask and gets the result back.',
      ),
    },
  ];
  /** Hover text for each type filter: what that type does. */
  const KIND_TITLE: Record<'all' | 'skill' | 'agent' | 'rule', string> = {
    all: msg(
      'Everything installed for this provider: rules, skills and agents.',
    ),
    rule: KIND_HELP[0].text,
    skill: KIND_HELP[1].text,
    agent: KIND_HELP[2].text,
  };
  const CELLS = Array.from({ length: METER_CELLS }, (_, index) => index);

  /** The preset on screen: what it is now, and the saved version it came from (`null` for a new one). */
  let draft = $state<Draft>({ id: null, name: '', items: [], composites: [] });
  let saved = $state<Draft | null>(null);
  let query = $state('');
  /** The type the owner picked; `null` until they pick one. */
  let kindChoice = $state<'all' | 'skill' | 'agent' | 'rule' | null>(null);
  let family = $state<Family | 'all'>('all');
  let opened = $state<Group[]>([]);
  let menuFor = $state<string | null>(null);
  /** The switch the owner asked for while the draft had changes; it runs only after Discard changes. */
  let pending = $state<(() => void) | null>(null);
  let confirmingDelete = $state(false);
  let deleting = $state(false);

  const library = $derived(
    arsenal.library.filter((item) => item.provider === provider),
  );
  /** Starts on Rules when the provider has any, so the first thing on screen is the rules. */
  const kind = $derived(
    kindChoice ??
      (library.some((item) => item.kind === 'rule') ? 'rule' : 'all'),
  );
  const chosen = $derived(new Set(draft.items.map((item) => item.capability)));
  const allRows = $derived(groupRows(library, chosen, arsenal.groups));
  const matching = $derived(
    new Set(
      filterChooser(library, { query, kind, family }).map((item) => item.id),
    ),
  );
  const rows = $derived(
    allRows
      .map((row) => ({
        ...row,
        shown: row.items.filter((item) => matching.has(item.id)),
      }))
      .filter((row) => row.shown.length > 0),
  );
  const filtering = $derived(
    query.trim() !== '' || kind !== 'all' || family !== 'all',
  );
  const families = $derived(familiesOf(library));
  const presets = $derived(
    arsenal.selections.filter((selection) => selection.provider === provider),
  );
  const current = $derived(
    presets.find((selection) => selection.id === draft.id) ?? null,
  );
  const dirty = $derived(isDirty(draft, saved));
  const nameMissing = $derived(draft.name.trim() === '');
  const saving = $derived(arsenal.busy === 'save');
  const key = $derived(planKey(provider, draft.items, draft.composites));
  const plan = $derived(
    arsenal.plan !== null && arsenal.plan.key === key ? arsenal.plan : null,
  );
  const checking = $derived(
    draft.items.length > 0 &&
      (plan === null || (plan.preview === null && !plan.failed)),
  );
  const failed = $derived(plan !== null && plan.failed);
  const totals = $derived(
    plan?.preview ? totalsOf(plan.preview, library.length) : null,
  );
  const conflicts = $derived(plan?.preview?.conflicts ?? []);
  const kept = $derived(
    keptComposites(arsenal.discover?.composites ?? [], provider),
  );
  const meters = $derived(allRows.filter((row) => row.chosen > 0));

  // The draft is checked on the desktop every time it changes, never saved by that.
  $effect(() => {
    void controller.checkDraft(
      provider,
      draft.items.map((item) => ({ ...item })),
      [...draft.composites],
    );
  });

  function clone(value: Draft): Draft {
    return {
      id: value.id,
      name: value.name,
      items: value.items.map((item) => ({ ...item })),
      composites: [...value.composites],
    };
  }
  function fromSelection(selection: Selection): Draft {
    return {
      id: selection.id,
      name: selection.name,
      items: selection.items,
      composites: selection.composites,
    };
  }
  function startNew() {
    draft = { id: null, name: '', items: [], composites: [] };
    saved = null;
    confirmingDelete = false;
  }
  function open(selection: Selection) {
    draft = clone(fromSelection(selection));
    saved = clone(fromSelection(selection));
    confirmingDelete = false;
  }
  function duplicate(selection: Selection) {
    draft = {
      id: null,
      name: t('Copy of {name}', { name: selection.name }),
      items: selection.items.map((item) => ({ ...item })),
      composites: [...selection.composites],
    };
    saved = null;
    confirmingDelete = false;
  }
  /** Runs a switch now, or asks first when the draft has changes that would be lost. */
  function guarded(action: () => void) {
    if (dirty) pending = action;
    else action();
  }
  /** Runs an action that takes this draft away (a provider switch): asks first when it has changes. */
  export function requestLeave(action: () => void) {
    guarded(action);
  }
  function confirmLeave() {
    const action = pending;
    pending = null;
    action?.();
  }
  function discard() {
    if (saved) draft = clone(saved);
    else startNew();
  }

  function selectable(item: LibraryItem): boolean {
    return item.compatibility.status !== 'unsupported';
  }
  function toggleItem(item: LibraryItem) {
    if (!selectable(item)) return;
    draft.items = chosen.has(item.id)
      ? draft.items.filter((entry) => entry.capability !== item.id)
      : [...draft.items, { capability: item.id, revision: null }];
  }
  /** Chooses every selectable capability of a group, or clears them when they are all chosen. */
  function toggleGroup(row: GroupRow) {
    const pick = row.items.filter(selectable);
    const allPicked =
      pick.length > 0 && pick.every((item) => chosen.has(item.id));
    if (allPicked) {
      const ids = new Set(pick.map((item) => item.id));
      draft.items = draft.items.filter((entry) => !ids.has(entry.capability));
    } else {
      const adding: Item[] = pick
        .filter((item) => !chosen.has(item.id))
        .map((item) => ({ capability: item.id, revision: null }));
      draft.items = [...draft.items, ...adding];
    }
  }
  function toggleComposite(entryKey: string) {
    draft.composites = draft.composites.includes(entryKey)
      ? draft.composites.filter((entry) => entry !== entryKey)
      : [...draft.composites, entryKey];
  }
  function toggleOpen(group: Group) {
    opened = opened.includes(group)
      ? opened.filter((entry) => entry !== group)
      : [...opened, group];
  }
  function isOpen(group: Group): boolean {
    return filtering || opened.includes(group);
  }
  function nameOf(id: string): string {
    return arsenal.library.find((item) => item.id === id)?.name ?? id;
  }
  function summary(item: LibraryItem): string {
    return summaryOf(item.description) || t('No description in its file');
  }
  function retry() {
    void controller.checkDraft(
      provider,
      draft.items.map((item) => ({ ...item })),
      [...draft.composites],
      true,
    );
  }

  async function save() {
    const result = await controller.saveSelection({
      id: draft.id,
      name: draft.name,
      provider,
      items: draft.items.map((item) => ({ ...item })),
      composites: [...draft.composites],
    });
    if (result) open(result);
  }

  async function remove(selection: Selection) {
    deleting = true;
    const done = await controller.deleteSelection(selection.id);
    deleting = false;
    if (done) startNew();
    else confirmingDelete = false;
  }

  function moveTo(item: LibraryItem, group: Group | null) {
    menuFor = null;
    void controller.moveGroup(item.id, group);
  }

  function focusOnMount(node: HTMLElement) {
    node.focus();
  }
</script>

<div class="presets">
  <section class="presets-list" aria-labelledby="presets-list-title">
    <div class="presets-head">
      <h3 id="presets-list-title">{t('Presets')}</h3>
      <button type="button" class="action" onclick={() => guarded(startNew)}
        ><span aria-hidden="true">+</span> {t('New preset')}</button
      >
    </div>
    {#if arsenal.selectionsFailure}
      <div class="notice" data-tone="error" role="alert">
        <p>{t(arsenal.selectionsFailure)}</p>
        <button
          type="button"
          class="action"
          onclick={() => void controller.loadSelections()}
          >{t('Try again')}</button
        >
      </div>
    {:else if presets.length === 0}
      {#if arsenal.phase === 'loading'}
        <p class="muted">{t('Reading presets…')}</p>
      {:else}
        <p class="muted">
          {t('No presets for {provider} yet.', {
            provider: PROVIDER_LABEL[provider],
          })}
        </p>
      {/if}
    {:else}
      <ul class="preset-rows">
        {#each presets as preset (preset.id)}
          <li>
            <button
              type="button"
              class="preset-row"
              aria-current={draft.id === preset.id ? 'true' : undefined}
              onclick={() => guarded(() => open(preset))}
            >
              <span class="preset-name">{preset.name}</span>
              <span class="mono">{preset.items.length}</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
    {#if current}
      <div class="preset-actions">
        <button
          type="button"
          class="action quiet"
          onclick={() => guarded(() => duplicate(current))}
          >{t('Duplicate')}</button
        >
        {#if confirmingDelete}
          <p class="confirm">{t('Delete {name}?', { name: current.name })}</p>
          <button
            type="button"
            class="action danger"
            disabled={deleting}
            onclick={() => remove(current)}
            >{deleting ? t('Deleting…') : t('Delete')}</button
          >
          <button
            type="button"
            class="action"
            use:focusOnMount
            onclick={() => (confirmingDelete = false)}>{t('Cancel')}</button
          >
        {:else}
          <button
            type="button"
            class="action quiet"
            onclick={() => (confirmingDelete = true)}>{t('Delete')}</button
          >
        {/if}
      </div>
    {/if}
  </section>

  <section class="presets-chooser" aria-labelledby="presets-choose-title">
    <h3 id="presets-choose-title">{t('Choose')}</h3>
    <div class="filters">
      <label class="field search">
        <span class="sr-only">{t('Search capabilities')}</span>
        <input type="search" placeholder={t('Search…')} bind:value={query} />
      </label>
      <div class="chips">
        {#each KIND_FILTERS as option (option.value)}
          <button
            type="button"
            class="chip"
            aria-pressed={kind === option.value}
            title={t(KIND_TITLE[option.value])}
            onclick={() => (kindChoice = option.value)}
            >{t(option.label)}</button
          >
        {/each}
      </div>
      <dl class="kind-help">
        {#each KIND_HELP as entry (entry.value)}
          <div
            class="kind-help-row"
            data-active={kind === 'all' || kind === entry.value}
          >
            <dt>{t(entry.label)}</dt>
            <dd>{t(entry.text)}</dd>
          </div>
        {/each}
      </dl>
      {#if families.length > 0}
        <div class="chips">
          <span class="chip-label">{t('Family')}</span>
          <button
            type="button"
            class="chip"
            aria-pressed={family === 'all'}
            onclick={() => (family = 'all')}>{t('All')}</button
          >
          {#each families as name (name)}
            <button
              type="button"
              class="chip"
              aria-pressed={family === name}
              onclick={() => (family = name)}>{name}</button
            >
          {/each}
        </div>
      {/if}
    </div>

    {#if rows.length === 0}
      <p class="muted">{t('No capabilities match.')}</p>
    {:else}
      {#each rows as row (row.group)}
        <div class="group">
          <div class="group-head">
            <button
              type="button"
              class="group-check"
              role="checkbox"
              aria-checked={row.state === 'all'
                ? 'true'
                : row.state === 'partial'
                  ? 'mixed'
                  : 'false'}
              aria-label={t(GROUP_LABEL[row.group])}
              onclick={() => toggleGroup(row)}
              >{row.state === 'all'
                ? '✓'
                : row.state === 'partial'
                  ? '–'
                  : ''}</button
            >
            <button
              type="button"
              class="group-toggle"
              aria-expanded={isOpen(row.group)}
              onclick={() => toggleOpen(row.group)}
            >
              <span class="group-name">{t(GROUP_LABEL[row.group])}</span>
              <span class="mono">{row.chosen}/{row.total}</span>
              <span aria-hidden="true">{isOpen(row.group) ? '▾' : '▸'}</span>
            </button>
          </div>
          {#if isOpen(row.group)}
            <ul class="capabilities">
              {#each row.shown as item (item.id)}
                <li
                  class="capability"
                  data-chosen={chosen.has(item.id) ? 'true' : undefined}
                >
                  <input
                    type="checkbox"
                    class="capability-check"
                    checked={chosen.has(item.id)}
                    disabled={!selectable(item)}
                    aria-label={item.name}
                    onchange={() => toggleItem(item)}
                  />
                  <div class="capability-body">
                    <div class="capability-head">
                      <span class="capability-name">{item.name}</span>
                      <span class="tags">
                        <span class="tag"
                          >{t(KIND_LABEL[item.kind] ?? item.kind)}</span
                        >
                        {#if familyFor(item.name)}
                          <span class="tag">{familyFor(item.name)}</span>
                        {/if}
                        <span
                          class="badge"
                          data-tone={compatibilityMark(
                            item.compatibility.status,
                          ).tone}
                          >{compatibilityMark(item.compatibility.status).symbol}
                          {t(
                            COMPATIBILITY_LABEL[item.compatibility.status],
                          )}</span
                        >
                      </span>
                    </div>
                    <p
                      class="capability-summary"
                      title={item.description || undefined}
                    >
                      {summary(item)}
                    </p>
                    {#if !selectable(item)}
                      <p class="capability-why">{item.compatibility.reason}</p>
                    {/if}
                    <div class="capability-tools">
                      {#if arsenal.movingGroup === item.id}
                        <span class="muted">{t('Saving…')}</span>
                      {/if}
                      <button
                        type="button"
                        class="action quiet"
                        aria-haspopup="menu"
                        aria-expanded={menuFor === item.id}
                        onclick={() =>
                          (menuFor = menuFor === item.id ? null : item.id)}
                        >{t('Move to group…')}</button
                      >
                    </div>
                    {#if arsenal.groupFailure && arsenal.groupFailure.identity === item.id}
                      <p class="error" role="alert">
                        {t(arsenal.groupFailure.text)}
                      </p>
                    {/if}
                    {#if menuFor === item.id}
                      <p class="help">
                        {t(
                          'Only changes where it is listed on this screen, not what it does. Saved on this computer.',
                        )}
                      </p>
                      <div class="menu" role="menu">
                        {#each GROUPS as group (group)}
                          <button
                            type="button"
                            role="menuitemradio"
                            aria-checked={groupFor(
                              item,
                              arsenal.groups[item.id],
                            ) === group}
                            onclick={() => moveTo(item, group)}
                            >{t(GROUP_LABEL[group])}</button
                          >
                        {/each}
                        <button
                          type="button"
                          role="menuitem"
                          onclick={() => moveTo(item, null)}
                          >{t('Back to automatic group')}</button
                        >
                      </div>
                    {/if}
                  </div>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      {/each}
    {/if}
  </section>

  <aside class="presets-composition" aria-labelledby="composition-title">
    <h3 id="composition-title">{t('How it is put together')}</h3>

    {#if pending}
      <div class="notice" data-tone="warn" role="status">
        <p>{t('Unsaved changes')}</p>
        <button type="button" class="action primary" onclick={confirmLeave}
          >{t('Discard changes')}</button
        >
        <button type="button" class="action" onclick={() => (pending = null)}
          >{t('Keep editing')}</button
        >
      </div>
    {/if}

    <label class="field">
      <span>{t('Name')}</span>
      <input
        type="text"
        bind:value={draft.name}
        aria-describedby={nameMissing ? 'preset-name-help' : undefined}
      />
      {#if nameMissing}
        <span id="preset-name-help" class="help">{t('Name the preset')}</span>
      {/if}
    </label>
    <p class="mono provider">{PROVIDER_LABEL[provider]}</p>

    {#if meters.length > 0}
      <ul class="meters">
        {#each meters as row (row.group)}
          <li class="meter-row">
            <span class="cells" aria-hidden="true">
              {#each CELLS as index (index)}
                <span
                  class="cell"
                  data-filled={index < meterFill(row.chosen)
                    ? 'true'
                    : undefined}
                ></span>
              {/each}
            </span>
            <span class="meter-name">{t(GROUP_LABEL[row.group])}</span>
            <span class="mono">{row.chosen}</span>
          </li>
        {/each}
      </ul>
    {/if}

    <div class="totals" aria-live="polite">
      {#if draft.items.length === 0}
        <p class="muted">
          {t('Nothing chosen yet. Tick a group or a capability.')}
        </p>
      {:else if failed}
        <p class="error">{t('Could not be checked.')}</p>
        <button type="button" class="action" onclick={retry}
          >{t('Try again')}</button
        >
      {:else if checking}
        <p class="muted">{t('Checking…')}</p>
      {:else if totals}
        <p class="mono">
          {t('{on} on · {off} off', { on: totals.on, off: totals.off })}
        </p>
      {/if}
    </div>

    {#if kept.length > 0}
      <div class="kept">
        {#each kept as composite (composite.key)}
          <label class="check">
            <input
              type="checkbox"
              checked={draft.composites.includes(composite.key)}
              onchange={() => toggleComposite(composite.key)}
            />
            <span
              >{t('Keep {name} ({count} skills)', {
                name: composite.name,
                count: composite.skills.length,
              })}</span
            >
          </label>
        {/each}
      </div>
    {/if}

    {#if conflicts.length > 0}
      <div class="conflicts">
        <p class="badge" data-tone="warn">
          ! {conflicts.length === 1
            ? t('{count} conflict', { count: 1 })
            : t('{count} conflicts', { count: conflicts.length })}
        </p>
        <ul>
          {#each conflicts as conflict, index (index)}
            <li>
              {t(CONFLICT_LABEL[conflict.code] ?? conflict.code)}: {conflict.capabilities
                .map(nameOf)
                .join(', ')}
            </li>
          {/each}
        </ul>
      </div>
    {/if}

    {#if totals && draft.items.length > 0}
      <h4>{t('How it starts')}</h4>
      <p class="starts">
        {t(
          '{agent} starts with {on} capabilities from this preset. The other {off} installed skills are switched off in that terminal.',
          {
            agent: PROVIDER_LABEL[provider],
            on: totals.on,
            off: totals.off,
          },
        )}
      </p>
    {/if}

    <div class="composition-actions">
      <button
        type="button"
        class="action primary"
        disabled={!dirty || nameMissing || saving}
        onclick={save}>{saving ? t('Saving…') : t('Save preset')}</button
      >
      <button
        type="button"
        class="action"
        disabled={!dirty || saving}
        onclick={discard}>{t('Discard changes')}</button
      >
    </div>
    {#if dirty}
      <p class="muted">{t('Unsaved changes')}</p>
    {/if}
    {#if arsenal.notice && arsenal.notice.tone === 'error'}
      <p class="error" role="alert">{t(arsenal.notice.text)}</p>
    {/if}
  </aside>
</div>

<style>
  /* Presets run as one strip on top; below, the chooser takes the width and
     the composition panel stays beside it. */
  .presets {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 340px);
    grid-template-areas:
      'list list'
      'chooser composition';
    gap: 12px 16px;
    align-items: start;
  }
  .presets-list {
    grid-area: list;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 12px;
    padding: 8px 12px;
  }
  .presets-list h3 {
    margin: 0;
  }
  .presets-list .preset-rows {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .presets-list .preset-row {
    width: auto;
    border-left: 0;
    border-bottom: 2px solid transparent;
  }
  .presets-list .preset-row[aria-current='true'] {
    border-bottom-color: var(--accent);
  }
  .presets-list .muted,
  .presets-list .preset-actions {
    margin: 0;
  }
  .presets-chooser {
    grid-area: chooser;
    border: 0;
    background: transparent;
    padding: 0;
  }
  .presets-composition {
    grid-area: composition;
  }
  @media (max-width: 900px) {
    .presets {
      grid-template-columns: minmax(0, 1fr);
      grid-template-areas: 'list' 'composition' 'chooser';
    }
    .presets-composition {
      position: static;
    }
  }
  section,
  aside {
    border: 1px solid var(--line);
    background: var(--surface);
    padding: 16px;
    box-sizing: border-box;
    min-width: 0;
  }
  .presets-composition {
    position: sticky;
    top: 16px;
    display: grid;
    gap: 8px;
    align-content: start;
  }
  h3 {
    font-size: var(--text-lg);
    font-weight: 600;
    margin: 0 0 8px;
  }
  h4 {
    font-size: var(--text-md);
    font-weight: 600;
    margin: 8px 0 0;
  }
  .presets-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
  }
  .muted {
    color: var(--muted);
    font-size: var(--text-sm);
    margin: 4px 0;
  }
  .error {
    color: var(--error);
    font-size: var(--text-sm);
    margin: 4px 0;
    overflow-wrap: anywhere;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }
  .preset-rows {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }
  .preset-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: 44px;
    padding: 8px 12px;
    border: 0;
    border-left: 3px solid transparent;
    background: transparent;
    color: var(--text);
    text-align: left;
    font: inherit;
    cursor: pointer;
  }
  .preset-row:hover {
    background: var(--surface-hover);
  }
  .preset-row[aria-current='true'] {
    background: var(--accent-soft);
    border-left-color: var(--accent);
  }
  .preset-name {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .preset-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 12px;
    align-items: center;
  }
  .confirm {
    width: 100%;
    margin: 0;
    font-size: var(--text-sm);
  }
  .filters {
    display: grid;
    gap: 8px;
    margin-bottom: 12px;
  }
  .search input {
    width: 100%;
    min-height: 44px;
    box-sizing: border-box;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
  }
  .chip-label {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .chip {
    min-height: 44px;
    padding: 0 12px;
    border: 1px solid var(--line-strong);
    background: var(--surface-raised);
    color: var(--text);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .chip:hover {
    background: var(--surface-hover);
  }
  .chip[aria-pressed='true'] {
    background: var(--accent-soft);
    border-color: var(--accent);
    font-weight: 600;
  }
  .group {
    border-top: 1px solid var(--line);
  }
  .group-head {
    display: flex;
    align-items: stretch;
    min-height: 40px;
  }
  .group-check {
    width: 40px;
    min-height: 40px;
    flex: none;
    border: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
  .group-check[aria-checked='mixed'] {
    color: var(--muted);
  }
  .group-toggle {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 40px;
    padding: 0 8px;
    border: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .group-toggle:hover {
    background: var(--surface-hover);
  }
  .group-name {
    flex: 1;
    font-weight: 600;
  }
  .capabilities {
    list-style: none;
    margin: 0 0 8px;
    padding: 0 0 0 40px;
    display: grid;
    gap: 6px;
  }
  .capability {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 8px 12px;
    min-height: 44px;
    padding: 8px 12px;
    border-left: 3px solid transparent;
  }
  .capability[data-chosen='true'] {
    border-left-color: var(--accent);
    background: var(--accent-soft);
  }
  .capability-check {
    width: 18px;
    height: 18px;
    margin-top: 2px;
  }
  .capability-body {
    display: grid;
    gap: 4px;
    min-width: 0;
  }
  .capability-head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 8px;
  }
  .capability-name {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .capability-summary {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-sm);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .capability-why {
    margin: 0;
    color: var(--warn);
    font-size: var(--text-sm);
  }
  .capability-tools {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
  }
  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
  }
  .tag {
    font-size: var(--text-xs);
    color: var(--muted);
    border: 1px solid var(--line);
    padding: 0 6px;
  }
  .menu {
    display: grid;
    gap: 2px;
    padding: 4px;
    background: var(--surface-raised);
    border: 1px solid var(--line-strong);
    box-shadow: var(--shadow-pop);
    max-width: 320px;
  }
  .menu button {
    min-height: 44px;
    padding: 0 12px;
    border: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .menu button:hover {
    background: var(--surface-hover);
  }
  .menu button[aria-checked='true'] {
    font-weight: 600;
    color: var(--accent);
  }
  .provider {
    margin: 0;
  }
  .meters {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }
  .meter-row {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    gap: 8px;
    align-items: center;
    font-size: var(--text-sm);
  }
  .cells {
    display: inline-flex;
    gap: 2px;
  }
  .cell {
    width: 8px;
    height: 8px;
    box-sizing: border-box;
    border: 1px solid var(--line-strong);
    transition: background-color 120ms;
  }
  .cell[data-filled='true'] {
    background: var(--accent);
    border-color: var(--accent);
  }
  .kept,
  .conflicts {
    display: grid;
    gap: 4px;
    font-size: var(--text-sm);
  }
  .check {
    display: flex;
    gap: 8px;
    align-items: center;
    min-height: 44px;
  }
  .conflicts ul {
    margin: 0;
    padding-left: 16px;
  }
  .starts {
    margin: 0;
    font-size: var(--text-sm);
    line-height: 1.45;
  }
  .composition-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 8px;
  }
  .notice {
    display: grid;
    gap: 8px;
    padding: 8px;
    border: 1px solid var(--line-strong);
  }
  .help {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .help {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .kind-help {
    display: grid;
    gap: 2px;
    margin: 0;
    font-size: var(--text-xs);
  }
  .kind-help-row {
    display: grid;
    grid-template-columns: 64px minmax(0, 1fr);
    gap: 8px;
    color: var(--muted);
  }
  .kind-help-row[data-active='true'] {
    color: var(--text);
  }
  .kind-help dt {
    font-weight: 600;
  }
  .kind-help dd {
    margin: 0;
  }
  .notice[data-tone='error'] {
    background: var(--error-soft);
    color: var(--error-text);
  }
  @media (max-width: 1279px) {
    .presets {
      grid-template-columns: minmax(0, 1fr) minmax(0, 300px);
    }
    .presets-list {
      grid-column: 1 / -1;
    }
  }
  @media (max-width: 767px) {
    .presets {
      grid-template-columns: minmax(0, 1fr);
    }
    .presets-composition {
      position: static;
    }
    .capabilities {
      padding-left: 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .cell {
      transition: none;
    }
  }
</style>
