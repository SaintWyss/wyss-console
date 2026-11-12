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
    familyFor,
    groupFor,
    knownGroup,
    type Family,
    type Group,
    type Section,
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
    categoriesOf,
    totalsOf,
    viewOnlyEntries,
    type ChooserKind,
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
    rules: msg('Rules'),
    models: msg('Models'),
    integrations: msg('Integrations'),
    automations: msg('Automations'),
    security: msg('Security'),
    debug: msg('Fixing bugs'),
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
  /** Hover text for each group header: what kind of capability lives there. */
  const GROUP_HELP: Record<Group, string> = {
    rules: msg(
      'Rules the agent always follows: standing instructions read when every session starts.',
    ),
    models: msg(
      'Model profiles (astra, luna, sol) that choose which model and effort a task runs with.',
    ),
    integrations: msg(
      'MCP servers: connections to outside tools such as Notion, a browser or library docs.',
    ),
    automations: msg(
      'Hooks that run by themselves at moments of the session and are not about security.',
    ),
    security: msg(
      'Checks for security problems: audits, threat reviews and guards against dangerous commands.',
    ),
    debug: msg(
      'Finding why something fails: method to chase a bug down to its cause.',
    ),
    content: msg(
      'Writing for the web and social media, and search ranking (SEO).',
    ),
    motion: msg('Animation, 3D and video.'),
    design: msg(
      'Designing screens: visual direction, components, accessibility and turning a mockup into code.',
    ),
    study: msg(
      'Learning and notes: tutors, quizzes, second brain and saved links.',
    ),
    decide: msg(
      'Making a decision: arguing against an idea before you commit to it.',
    ),
    research: msg(
      'Looking things up: documentation, the web and mapping a codebase.',
    ),
    review: msg(
      'Checking work: code review, tests, verification and quality checks.',
    ),
    docs: msg(
      'Writing and converting documents: docs, reports, prompts and PDFs.',
    ),
    planning: msg(
      'Planning the work: specs, roadmaps, plans and the method that orders them.',
    ),
    build: msg(
      'Writing the code: executing plans, fixing findings and shipping.',
    ),
    system: msg(
      'This computer and the tools themselves: setup, health checks and upgrades.',
    ),
    other: msg(
      'What no rule placed. Use Move to group… to put it where it belongs.',
    ),
  };
  const SECTION_LABEL: Record<Section, string> = {
    how: msg('How it works'),
    before: msg('Before building'),
    build: msg('Building'),
    after: msg('After'),
    env: msg('Environment'),
  };
  const KIND_LABEL: Record<string, string> = {
    skill: msg('Skill'),
    agent: msg('Agent'),
    rule: msg('Rule'),
    hook: msg('Hook'),
    mcp_server: msg('MCP server'),
    plugin: msg('Plugin'),
    command: msg('Command'),
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
  const KIND_FILTERS: { value: ChooserKind; label: string }[] = [
    { value: 'all', label: msg('All') },
    { value: 'rule', label: msg('Rules') },
    { value: 'skill', label: msg('Skills') },
    { value: 'agent', label: msg('Agents') },
    { value: 'hook', label: msg('Hooks') },
    { value: 'mcp_server', label: msg('MCP') },
    { value: 'plugin', label: msg('Plugins') },
    { value: 'command', label: msg('Commands') },
  ];
  /** Hover text for each type filter: what that type is, in plain words. */
  const KIND_TITLE: Record<ChooserKind, string> = {
    all: msg('Shows everything at once, of every type.'),
    rule: msg(
      'A rule is a file of permanent instructions, such as invariantes.md. The agent reads it when every session starts and follows it the whole time. It does not run anything.',
    ),
    skill: msg(
      'A skill is a how-to manual for one kind of task, such as designing a screen or finding a bug. The agent opens it only when the task you give it needs it.',
    ),
    agent: msg(
      'An agent is a helper with a job of its own, such as reviewing code. The main agent hands it part of the work; it does it on its own and returns the result.',
    ),
    hook: msg(
      'A hook is a script that runs by itself at a moment of the session, such as before a command or when it starts. It can stop a command or add context, without being asked.',
    ),
    mcp_server: msg(
      'An MCP server connects the agent to an outside tool, such as Notion, a browser or library docs, and gives it new actions to use.',
    ),
    plugin: msg(
      'A plugin is a package that brings several pieces together (skills, agents, hooks, MCP servers) and is switched on or off as a whole.',
    ),
    command: msg(
      'A command is a /name you type to run a saved prompt. Here only your own command files are listed; most /commands are skills.',
    ),
  };
  const CELLS = Array.from({ length: METER_CELLS }, (_, index) => index);

  /** The preset on screen: what it is now, and the saved version it came from (`null` for a new one). */
  let draft = $state<Draft>({ id: null, name: '', items: [], composites: [] });
  let saved = $state<Draft | null>(null);
  let query = $state('');
  /** The type the owner picked; `null` until they pick one. */
  let kindChoice = $state<ChooserKind | null>(null);
  let family = $state<Family | 'all'>('all');
  let opened = $state<string[]>([]);
  /** The section a new category is being named in, and the name typed so far. */
  let adding = $state<Section | null>(null);
  let newName = $state('');
  /** The owner's category being renamed or confirmed for deletion. */
  let renaming = $state<string | null>(null);
  let renameText = $state('');
  let deletingCategory = $state<string | null>(null);
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
  const categories = $derived(categoriesOf(arsenal.categories ?? []));
  const viewOnly = $derived(
    viewOnlyEntries(arsenal.discover ?? null, provider, kind, query),
  );
  const allRows = $derived(
    groupRows(library, chosen, arsenal.groups, categories, viewOnly),
  );
  const matching = $derived(
    new Set(
      filterChooser(library, { query, kind, family }).map((item) => item.id),
    ),
  );
  /** Every category stays on screen, empty ones with 0, except while searching by text. */
  const rows = $derived(
    allRows
      .map((row) => ({
        ...row,
        shown: row.items.filter((item) => matching.has(item.id)),
      }))
      .filter(
        (row) =>
          query.trim() === '' || row.shown.length > 0 || row.extras.length > 0,
      ),
  );
  /** A category's name on screen: the owner's own, or the basic one translated. */
  function labelOf(group: string): string {
    const custom = (arsenal.categories ?? []).find(
      (category) => category.id === group,
    );
    if (custom) return custom.name;
    return t(GROUP_LABEL[group as Group] ?? GROUP_LABEL.other);
  }
  function helpOf(group: string): string | undefined {
    const help = GROUP_HELP[group as Group];
    return help ? t(help) : undefined;
  }
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
  async function addCategory(section: Section) {
    const done = await controller.writeCategory({
      action: 'create',
      name: newName,
      section,
    });
    if (done) {
      adding = null;
      newName = '';
    }
  }
  async function renameCategory(id: string) {
    if (
      await controller.writeCategory({ action: 'rename', id, name: renameText })
    )
      renaming = null;
  }
  async function deleteCategory(id: string) {
    if (await controller.writeCategory({ action: 'delete', id }))
      deletingCategory = null;
  }
  /** The last category of each section, where "+ New category" goes. */
  function lastOfSection(index: number): boolean {
    const row = rows[index];
    return row.section !== null && rows[index + 1]?.section !== row.section;
  }
  function toggleOpen(group: string) {
    opened = opened.includes(group)
      ? opened.filter((entry) => entry !== group)
      : [...opened, group];
  }
  function isOpen(group: string): boolean {
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

  function moveTo(item: LibraryItem, group: string | null) {
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

    {#if arsenal.categoryFailure}
      <p class="error" role="alert">{t(arsenal.categoryFailure)}</p>
    {/if}
    {#if rows.every((row) => row.shown.length === 0 && row.extras.length === 0)}
      <p class="muted">
        {kind === 'command' && query.trim() === ''
          ? t('No custom commands. Your /commands are skills here.')
          : t('No capabilities match.')}
      </p>
    {/if}
    {#if rows.length > 0}
      {#each rows as row, index (row.group)}
        {#if row.section !== null && row.section !== rows[index - 1]?.section}
          <h4 class="section-title">{t(SECTION_LABEL[row.section])}</h4>
        {/if}
        <div class="group" data-empty={row.total + row.extras.length === 0}>
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
              aria-label={labelOf(row.group)}
              disabled={row.total === 0}
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
              title={helpOf(row.group)}
              aria-expanded={isOpen(row.group)}
              onclick={() => toggleOpen(row.group)}
            >
              <span class="group-name">{labelOf(row.group)}</span>
              <span class="mono"
                >{row.chosen}/{row.total}{#if row.extras.length > 0}
                  · +{row.extras.length}{/if}</span
              >
              <span aria-hidden="true">{isOpen(row.group) ? '▾' : '▸'}</span>
            </button>
            {#if row.name !== null}
              <button
                type="button"
                class="action quiet"
                onclick={() => {
                  renaming = row.group;
                  renameText = row.name ?? '';
                  deletingCategory = null;
                }}>{t('Rename')}</button
              >
              <button
                type="button"
                class="action quiet"
                onclick={() => {
                  deletingCategory = row.group;
                  renaming = null;
                }}>{t('Delete')}</button
              >
            {/if}
          </div>
          {#if renaming === row.group}
            <form
              class="category-form"
              onsubmit={(event) => {
                event.preventDefault();
                void renameCategory(row.group);
              }}
            >
              <label class="sr-only" for="rename-{row.group}"
                >{t('Category name')}</label
              >
              <input
                id="rename-{row.group}"
                maxlength="40"
                bind:value={renameText}
                use:focusOnMount
              />
              <button type="submit" class="action">{t('Save')}</button>
              <button
                type="button"
                class="action quiet"
                onclick={() => (renaming = null)}>{t('Cancel')}</button
              >
            </form>
          {/if}
          {#if deletingCategory === row.group}
            <div class="category-form" role="alertdialog">
              <p class="confirm">
                {t(
                  'Delete {name}? Its {n} capabilities go back to their default category.',
                  { name: row.name ?? '', n: String(row.total) },
                )}
              </p>
              <button
                type="button"
                class="action danger"
                onclick={() => void deleteCategory(row.group)}
                >{t('Delete')}</button
              >
              <button
                type="button"
                class="action"
                use:focusOnMount
                onclick={() => (deletingCategory = null)}>{t('Cancel')}</button
              >
            </div>
          {/if}
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
                        {#each categories as category (category.id)}
                          <button
                            type="button"
                            role="menuitemradio"
                            aria-checked={knownGroup(
                              groupFor(item, arsenal.groups[item.id]),
                              categories
                                .filter((entry) => entry.name !== null)
                                .map((entry) => entry.id),
                            ) === category.id}
                            onclick={() => moveTo(item, category.id)}
                            >{labelOf(category.id)}</button
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
              {#each row.extras as entry (entry.key)}
                <li class="capability view-only-row">
                  <span class="view-only-mark" aria-hidden="true">·</span>
                  <div class="capability-body">
                    <div class="capability-head">
                      <span class="capability-name">{entry.name}</span>
                      <span class="tags">
                        <span class="tag"
                          >{t(KIND_LABEL[entry.kind] ?? entry.kind)}</span
                        >
                        {#if entry.partOf}
                          <span class="tag"
                            >{t('Part of {name}', { name: entry.partOf })}</span
                          >
                        {:else}
                          <span class="tag">{t('Not selectable yet')}</span>
                        {/if}
                      </span>
                    </div>
                    {#if entry.description}
                      <p class="capability-summary" title={entry.description}>
                        {entry.description}
                      </p>
                    {/if}
                  </div>
                </li>
              {/each}
              {#if row.total + row.extras.length === 0}
                <li class="muted empty-group">{t('Nothing here yet.')}</li>
              {/if}
            </ul>
          {/if}
        </div>
        {#if lastOfSection(index) && row.section !== null && query.trim() === ''}
          {#if adding === row.section}
            <form
              class="category-form"
              onsubmit={(event) => {
                event.preventDefault();
                if (row.section !== null) void addCategory(row.section);
              }}
            >
              <label class="sr-only" for="new-category-{row.section}"
                >{t('Category name')}</label
              >
              <input
                id="new-category-{row.section}"
                maxlength="40"
                placeholder={t('Category name')}
                bind:value={newName}
                use:focusOnMount
              />
              <button type="submit" class="action">{t('Add')}</button>
              <button
                type="button"
                class="action quiet"
                onclick={() => (adding = null)}>{t('Cancel')}</button
              >
            </form>
          {:else}
            <button
              type="button"
              class="action quiet new-category"
              onclick={() => {
                adding = row.section;
                newName = '';
              }}>+ {t('New category')}</button
            >
          {/if}
        {/if}
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
            <span class="meter-name">{labelOf(row.group)}</span>
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
  .category-form {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding: 6px 0 6px 40px;
  }
  .category-form input {
    min-height: 36px;
    flex: 1 1 180px;
    max-width: 280px;
  }
  .new-category {
    margin: 2px 0 0 40px;
  }
  .section-title {
    margin: 16px 0 4px;
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .group[data-empty='true'] .group-name {
    color: var(--muted);
    font-weight: 500;
  }
  .empty-group {
    padding: 4px 12px;
  }
  .view-only-mark {
    width: 18px;
    text-align: center;
    color: var(--muted);
  }
  .view-only {
    border-top: 1px solid var(--line);
    padding-top: 8px;
    display: grid;
    gap: 6px;
  }
  .view-only h4 {
    margin: 0;
  }
  .view-only-rows {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 4px;
  }
  .view-only-row {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 8px;
    min-height: 32px;
    padding: 4px 12px;
    color: var(--muted);
  }
  .view-only-row .capability-name {
    color: var(--text);
  }
  .view-only-row .capability-summary {
    flex: 1 1 240px;
    min-width: 0;
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
