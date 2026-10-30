<script lang="ts">
  import type {
    ArsenalProvider,
    Compatibility,
    Item,
    Preset,
    Selection,
    SkillStatus,
    SkillView,
    Summary,
  } from '../../contracts/arsenal';
  import { blockedText } from './messages';
  import {
    filterDiscover,
    filterLibrary,
    librarySources,
    LIBRARY_COMPATIBILITY,
    type ProviderFilter,
  } from './catalogue';
  import type {
    ArsenalController,
    ArsenalState,
    ArsenalTab,
  } from './controller';

  let {
    visible,
    arsenal,
    controller,
  }: {
    visible: boolean;
    arsenal: ArsenalState;
    controller: ArsenalController;
  } = $props();

  const TABS: { id: ArsenalTab; label: string }[] = [
    { id: 'library', label: 'Library' },
    { id: 'discover', label: 'Discover' },
    { id: 'presets', label: 'Presets' },
    { id: 'selections', label: 'Selections' },
  ];
  const PROVIDER_LABEL: Record<ArsenalProvider, string> = {
    claude_code: 'Claude Code',
    codex: 'Codex',
  };
  const SOURCE_LABEL: Record<string, string> = {
    claude_user_skills: 'Claude user skills',
    codex_user_skills: 'Codex user skills',
  };
  const STATUS_LABEL: Record<SkillStatus, string> = {
    not_imported: 'Not imported',
    imported: 'Imported',
    source_changed: 'Source changed',
  };
  const COMPATIBILITY_LABEL: Record<Compatibility, string> = {
    supported: 'Supported',
    partial: 'Partial',
    unsupported: 'Unsupported',
    unverified: 'Unverified',
  };
  const EXCLUDED_LABEL: Record<string, string> = {
    credential_name: 'Credential file, not copied',
    hidden_entry: 'Hidden, not copied',
    symbolic_link: 'Link, not followed',
    binary_file: 'Binary, not copied',
    may_hold_secret: 'May hold a secret',
    special_file: 'Special file, not copied',
    too_deep: 'Nested too deep, not copied',
    non_utf8_name: 'Name is not UTF-8, not copied',
  };
  const SOURCE_STATE_LABEL: Record<string, string> = {
    read: 'read',
    absent: 'not present',
    unreadable: 'could not be read',
  };

  // Library filters
  let libraryQuery = $state('');
  let libraryProvider = $state<ProviderFilter>('all');
  let librarySource = $state<string | 'all'>('all');
  let libraryCompat = $state<Compatibility | 'all'>('all');
  // Discover filters
  let discoverQuery = $state('');
  let discoverProvider = $state<ProviderFilter>('all');
  let discoverStatus = $state<SkillStatus | 'all'>('all');
  // Drafts
  let presetDraft = $state({
    id: null as string | null,
    name: '',
    description: '',
    items: [] as Item[],
  });
  let presetError = $state<string | null>(null);
  let selectionDraft = $state({
    id: null as string | null,
    name: '',
    provider: 'claude_code' as ArsenalProvider,
    items: [] as Item[],
  });
  let selectionError = $state<string | null>(null);
  let confirmDelete = $state<string | null>(null);

  const visibleLibrary = $derived(
    filterLibrary(arsenal.library, {
      query: libraryQuery,
      provider: libraryProvider,
      source: librarySource,
      compatibility: libraryCompat,
    }),
  );
  const sources = $derived(librarySources(arsenal.library));
  const skills = $derived(
    filterDiscover(arsenal.discover?.skills ?? [], {
      query: discoverQuery,
      provider: discoverProvider,
      status: discoverStatus,
    }),
  );
  const chosen = $derived<SkillView | null>(
    arsenal.discover?.skills.find((skill) => skill.id === arsenal.chosen) ??
      null,
  );
  const busy = $derived(arsenal.busy !== null);

  function inItems(items: Item[], id: string): boolean {
    return items.some((item) => item.capability === id);
  }
  function toggle(items: Item[], id: string): Item[] {
    return inItems(items, id)
      ? items.filter((item) => item.capability !== id)
      : [...items, { capability: id, revision: null }];
  }
  function nameOf(id: string): string {
    return (
      arsenal.library.find((item) => item.id === id)?.name ??
      'Missing capability'
    );
  }
  function newPreset() {
    presetDraft = { id: null, name: '', description: '', items: [] };
    presetError = null;
  }
  function editPreset(preset: Preset) {
    presetDraft = {
      id: preset.id,
      name: preset.name,
      description: preset.description,
      items: preset.items.map((item) => ({ ...item })),
    };
    presetError = null;
  }
  function copyPreset(preset: Preset) {
    presetDraft = {
      id: null,
      name: `Copy of ${preset.name}`,
      description: preset.description,
      items: preset.items.map((item) => ({ ...item })),
    };
    presetError = null;
  }
  async function savePreset() {
    if (presetDraft.name.trim() === '') {
      presetError = 'Name the preset before saving.';
      return;
    }
    presetError = null;
    const saved = await controller.savePreset({
      id: presetDraft.id,
      name: presetDraft.name,
      description: presetDraft.description,
      items: presetDraft.items,
    });
    if (saved) newPreset();
  }
  function editSelection(selection: Selection) {
    selectionDraft = {
      id: selection.id,
      name: selection.name,
      provider: selection.provider,
      items: selection.items.map((item) => ({ ...item })),
    };
    selectionError = null;
  }
  async function saveSelection() {
    if (selectionDraft.name.trim() === '') {
      selectionError = 'Name the selection before saving.';
      return;
    }
    selectionError = null;
    const saved = await controller.saveSelection({ ...selectionDraft });
    if (saved) {
      selectionDraft = {
        id: null,
        name: '',
        provider: selectionDraft.provider,
        items: [],
      };
    }
  }
  function revisionOf(detail: { summary: Summary }): string {
    return `${detail.summary.revision} of ${detail.summary.revisions}`;
  }
  function excludedText(reason: string): string {
    return EXCLUDED_LABEL[reason] ?? reason;
  }
  function onTab(event: KeyboardEvent, index: number) {
    const keys = ['ArrowRight', 'ArrowLeft', 'Home', 'End'];
    if (!keys.includes(event.key)) return;
    event.preventDefault();
    const next =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? TABS.length - 1
          : (index + (event.key === 'ArrowRight' ? 1 : -1) + TABS.length) %
            TABS.length;
    controller.setTab(TABS[next].id);
    document.getElementById(`arsenal-tab-${TABS[next].id}`)?.focus();
  }
</script>

<section
  class="configuration-feature arsenal-feature"
  aria-label="Arsenal"
  hidden={!visible}
  inert={!visible}
>
  <div class="sessions-toolbar arsenal-head">
    <div class="sessions-heading">
      <h2>Arsenal</h2>
      <p class="sessions-count">Capabilities you own · offline</p>
    </div>
    <button
      class="action"
      type="button"
      disabled={arsenal.phase === 'loading' || busy}
      onclick={() => controller.refresh()}>Refresh</button
    >
  </div>
  <p class="configuration-note">
    Installed does not mean imported. Nothing here changes a session.
  </p>

  <div class="arsenal-tabs" role="tablist" aria-label="Arsenal sections">
    {#each TABS as tab, index (tab.id)}
      <button
        id="arsenal-tab-{tab.id}"
        class="action arsenal-tab"
        type="button"
        role="tab"
        aria-selected={arsenal.tab === tab.id}
        aria-controls="arsenal-panel-{tab.id}"
        tabindex={arsenal.tab === tab.id ? 0 : -1}
        onclick={() => controller.setTab(tab.id)}
        onkeydown={(event) => onTab(event, index)}>{tab.label}</button
      >
    {/each}
  </div>

  <div class="arsenal-body">
    {#if arsenal.notice}
      <p
        class="arsenal-notice {arsenal.notice.tone}"
        role={arsenal.notice.tone === 'error' ? 'alert' : 'status'}
        aria-live={arsenal.notice.tone === 'error' ? 'assertive' : 'polite'}
      >
        {arsenal.notice.text}
      </p>
    {/if}

    {#if arsenal.phase === 'loading' || arsenal.phase === 'idle'}
      <p role="status">Reading the Arsenal library…</p>
    {:else if arsenal.phase === 'unavailable'}
      <p role="alert">
        The Arsenal library could not be opened. Refresh after the Wyss data
        folder is writable.
      </p>
    {:else if arsenal.phase === 'failed'}
      <p role="alert">The library did not load completely. Try Refresh.</p>
    {/if}

    {#if arsenal.tab === 'library'}
      <div
        id="arsenal-panel-library"
        role="tabpanel"
        aria-labelledby="arsenal-tab-library"
        class="arsenal-panel"
      >
        <div class="arsenal-filters">
          <label
            >Search <input
              type="search"
              bind:value={libraryQuery}
              placeholder="Name, description or origin"
            /></label
          >
          <label
            >Provider
            <select bind:value={libraryProvider}>
              <option value="all">All</option>
              <option value="claude_code">Claude Code</option>
              <option value="codex">Codex</option>
            </select>
          </label>
          <label
            >Source
            <select bind:value={librarySource}>
              <option value="all">All</option>
              {#each sources as source (source)}
                <option value={source}>{SOURCE_LABEL[source] ?? source}</option>
              {/each}
            </select>
          </label>
          <label
            >Compatibility
            <select bind:value={libraryCompat}>
              <option value="all">All</option>
              <option value="unverified">Unverified</option>
              <option value="supported">Supported</option>
              <option value="partial">Partial</option>
              <option value="unsupported">Unsupported</option>
            </select>
          </label>
        </div>

        {#if arsenal.library.length === 0 && arsenal.phase === 'ready'}
          <p class="arsenal-empty">
            Nothing is imported yet. Open Discover to import a skill you already
            have.
          </p>
        {:else if visibleLibrary.length === 0 && arsenal.library.length > 0}
          <p class="arsenal-empty">No capabilities match these filters.</p>
        {/if}

        <ul class="arsenal-list" aria-label="Imported capabilities">
          {#each visibleLibrary as item (item.id)}
            <li class="arsenal-row-item">
              <button
                class="arsenal-row"
                type="button"
                aria-pressed={arsenal.opened?.summary.id === item.id}
                onclick={() => controller.open(item.id)}
              >
                <span class="arsenal-row-name">{item.name}</span>
                <span class="arsenal-badges">
                  <span class="pill">{PROVIDER_LABEL[item.provider]}</span>
                  <span class="pill"
                    >{SOURCE_LABEL[item.source] ?? item.source}</span
                  >
                  <span class="pill">Revision {item.revision}</span>
                  <span class="pill"
                    >{COMPATIBILITY_LABEL[LIBRARY_COMPATIBILITY]}</span
                  >
                </span>
              </button>
            </li>
          {/each}
        </ul>

        {#if arsenal.opened}
          {@const opened = arsenal.opened}
          <section class="arsenal-detail" aria-label="Capability details">
            <h3>{opened.summary.name}</h3>
            <p class="arsenal-muted">
              {PROVIDER_LABEL[opened.summary.provider]} · {SOURCE_LABEL[
                opened.summary.source
              ] ?? opened.summary.source} · Revision {revisionOf(opened)}
            </p>
            {#if opened.summary.description}<p>
                {opened.summary.description}
              </p>{/if}
            <dl class="arsenal-facts">
              <dt>Imported from</dt>
              <dd><code>{opened.summary.origin}</code></dd>
              <dt>Content</dt>
              <dd><code>{opened.summary.tree_hash.slice(0, 16)}</code></dd>
              <dt>Use in a session</dt>
              <dd>
                Not applied. Use in a launched session is not verified yet.
              </dd>
            </dl>

            <h4>Files in revision {opened.revision}</h4>
            <ul class="arsenal-files">
              {#each opened.files as file (file.path)}
                <li>
                  <button
                    class="action"
                    type="button"
                    onclick={() => controller.showFile(file.path)}
                    >{file.path}</button
                  >
                  <span class="arsenal-muted">{file.size} B</span>
                </li>
              {/each}
            </ul>
            {#if arsenal.file}
              <h4>{arsenal.file.path}</h4>
              <pre class="arsenal-preview">{arsenal.file.text}</pre>
            {/if}

            <h4>Revisions</h4>
            <ul class="arsenal-revisions">
              {#each opened.revisions as revision (revision.number)}
                <li>
                  <span>Revision {revision.number}</span>
                  <span class="arsenal-muted"
                    >{revision.files} files · {revision.bytes} B</span
                  >
                  {#if revision.current}
                    <span class="pill">Current</span>
                  {:else}
                    <button
                      class="action"
                      type="button"
                      disabled={busy}
                      onclick={() => controller.restore(revision.number)}
                    >
                      {arsenal.busy === 'restore'
                        ? 'Restoring…'
                        : `Restore revision ${revision.number}`}
                    </button>
                  {/if}
                  <button
                    class="action"
                    type="button"
                    onclick={() =>
                      controller.open(opened.summary.id, revision.number)}
                  >
                    Show files of revision {revision.number}
                  </button>
                </li>
              {/each}
            </ul>
          </section>
        {/if}
      </div>
    {/if}

    {#if arsenal.tab === 'discover'}
      <div
        id="arsenal-panel-discover"
        role="tabpanel"
        aria-labelledby="arsenal-tab-discover"
        class="arsenal-panel"
      >
        <div class="arsenal-filters">
          <label
            >Search <input
              type="search"
              bind:value={discoverQuery}
              placeholder="Name, description or origin"
            /></label
          >
          <label
            >Provider
            <select bind:value={discoverProvider}>
              <option value="all">All</option>
              <option value="claude_code">Claude Code</option>
              <option value="codex">Codex</option>
            </select>
          </label>
          <label
            >Status
            <select bind:value={discoverStatus}>
              <option value="all">All</option>
              <option value="not_imported">Not imported</option>
              <option value="imported">Imported</option>
              <option value="source_changed">Source changed</option>
            </select>
          </label>
          <button
            class="action"
            type="button"
            disabled={arsenal.discovering || busy}
            onclick={() => controller.discover()}
          >
            {arsenal.discovering ? 'Reading…' : 'Scan again'}
          </button>
        </div>

        {#if arsenal.discovering && arsenal.discover === null}
          <p role="status">Reading installed skills…</p>
        {:else if arsenal.discover}
          <ul class="arsenal-sources" aria-label="Skill folders">
            {#each arsenal.discover.sources as source (source.source)}
              <li>
                {SOURCE_LABEL[source.source] ?? source.source}: {SOURCE_STATE_LABEL[
                  source.state
                ] ?? source.state}
              </li>
            {/each}
          </ul>
          {#if arsenal.discover.truncated}
            <p role="alert">
              The folder has more skills than the scan shows. Some are not
              listed.
            </p>
          {/if}
          {#if arsenal.discover.skills.length === 0}
            <p class="arsenal-empty">
              No skills found in your Claude or Codex folders.
            </p>
          {:else if skills.length === 0}
            <p class="arsenal-empty">No skills match these filters.</p>
          {/if}
          <ul class="arsenal-list" aria-label="Installed skills">
            {#each skills as skill (skill.id)}
              <li class="arsenal-row-item">
                <button
                  class="arsenal-row"
                  type="button"
                  aria-pressed={arsenal.chosen === skill.id}
                  onclick={() => controller.choose(skill.id)}
                >
                  <span class="arsenal-row-name">{skill.name}</span>
                  <span class="arsenal-badges">
                    <span class="pill">{PROVIDER_LABEL[skill.provider]}</span>
                    <span class="pill">{STATUS_LABEL[skill.status]}</span>
                    {#if skill.blocked}<span class="pill">Blocked</span>{/if}
                    {#if skill.duplicates.length > 0}<span class="pill"
                        >Duplicate content</span
                      >{/if}
                    {#if skill.name_collisions.length > 0}<span class="pill"
                        >Name collision</span
                      >{/if}
                  </span>
                </button>
              </li>
            {/each}
          </ul>

          {#if chosen}
            <section class="arsenal-detail" aria-label="Skill to import">
              <h3>{chosen.name}</h3>
              <p class="arsenal-muted">
                {PROVIDER_LABEL[chosen.provider]} · {SOURCE_LABEL[
                  chosen.source
                ] ?? chosen.source} · {STATUS_LABEL[chosen.status]}
              </p>
              {#if chosen.description}<p>{chosen.description}</p>{/if}
              <dl class="arsenal-facts">
                <dt>Read from</dt>
                <dd><code>{chosen.origin}</code></dd>
                <dt>Content</dt>
                <dd><code>{chosen.tree_hash.slice(0, 16)}</code></dd>
              </dl>
              {#if chosen.executes_code}
                <p class="arsenal-warn">
                  Contains scripts. Arsenal copies them and never runs them.
                </p>
              {/if}
              {#if chosen.duplicates.length > 0}
                <p>
                  Same content as {chosen.duplicates
                    .map(
                      (id) =>
                        arsenal.discover?.skills.find((s) => s.id === id)
                          ?.name ?? id,
                    )
                    .join(', ')}.
                </p>
              {/if}
              {#if chosen.name_collisions.length > 0}
                <p>
                  Another skill here has the same name. Both can be imported;
                  they stay separate.
                </p>
              {/if}

              <h4>Files that will be copied</h4>
              <ul class="arsenal-files">
                {#each chosen.files as file (file.path)}
                  <li>
                    <span>{file.path}</span>
                    <span class="arsenal-muted">{file.size} B</span>
                  </li>
                {/each}
              </ul>
              {#if chosen.excluded.length > 0}
                <h4>Not copied</h4>
                <ul class="arsenal-files">
                  {#each chosen.excluded as entry (entry.path)}
                    <li>
                      <span>{entry.path}</span>
                      <span class="arsenal-muted"
                        >{excludedText(entry.reason)}</span
                      >
                    </li>
                  {/each}
                </ul>
              {/if}

              {#if chosen.review.length > 0}
                <h4>Review before importing</h4>
                <p class="arsenal-muted">
                  These files use a credential word. They are copied; check that
                  none holds a real value.
                </p>
                <ul class="arsenal-files">
                  {#each chosen.review as entry (entry.path)}
                    <li>
                      <span>{entry.path}</span>
                      <span class="arsenal-muted"
                        >Mentions a credential word</span
                      >
                    </li>
                  {/each}
                </ul>
              {/if}

              {#if chosen.blocked}
                <p role="alert">{blockedText(chosen.blocked)}</p>
              {/if}
              <button
                class="action"
                type="button"
                disabled={busy ||
                  chosen.blocked !== null ||
                  chosen.status === 'imported'}
                onclick={() => controller.importSkill(chosen.id)}
              >
                {#if arsenal.busy === 'import'}
                  Importing…
                {:else if chosen.status === 'imported'}
                  Already imported
                {:else if chosen.status === 'source_changed'}
                  Import as new revision
                {:else}
                  Import
                {/if}
              </button>
            </section>
          {/if}
        {/if}
      </div>
    {/if}

    {#if arsenal.tab === 'presets'}
      <div
        id="arsenal-panel-presets"
        role="tabpanel"
        aria-labelledby="arsenal-tab-presets"
        class="arsenal-panel"
      >
        <div class="arsenal-split">
          <div>
            <button class="action" type="button" onclick={newPreset}
              >New preset</button
            >
            {#if arsenal.presets.length === 0}
              <p class="arsenal-empty">
                No presets yet. Create one from a capability.
              </p>
            {/if}
            <ul class="arsenal-list" aria-label="Presets">
              {#each arsenal.presets as preset (preset.id)}
                <li class="arsenal-row-item">
                  <button
                    class="arsenal-row"
                    type="button"
                    aria-pressed={presetDraft.id === preset.id}
                    onclick={() => editPreset(preset)}
                  >
                    <span class="arsenal-row-name">{preset.name}</span>
                    <span class="arsenal-badges"
                      ><span class="pill"
                        >{preset.items.length} capabilities</span
                      ></span
                    >
                  </button>
                  <button
                    class="action"
                    type="button"
                    onclick={() => copyPreset(preset)}
                    >Duplicate {preset.name}</button
                  >
                  {#if confirmDelete === preset.id}
                    <button
                      class="action"
                      type="button"
                      onclick={() => {
                        confirmDelete = null;
                        controller.deletePreset(preset.id);
                      }}>Confirm delete {preset.name}</button
                    >
                  {:else}
                    <button
                      class="action"
                      type="button"
                      onclick={() => (confirmDelete = preset.id)}
                      >Delete {preset.name}</button
                    >
                  {/if}
                </li>
              {/each}
            </ul>
          </div>

          <form
            class="arsenal-editor"
            aria-label="Preset editor"
            onsubmit={(event) => {
              event.preventDefault();
              void savePreset();
            }}
          >
            <h3>{presetDraft.id ? 'Edit preset' : 'New preset'}</h3>
            <label
              >Name <input
                type="text"
                maxlength="80"
                bind:value={presetDraft.name}
                aria-invalid={presetError !== null}
                aria-describedby={presetError ? 'preset-error' : undefined}
              /></label
            >
            <label
              >Description <textarea
                rows="2"
                maxlength="500"
                bind:value={presetDraft.description}></textarea></label
            >
            <fieldset>
              <legend>Capabilities</legend>
              {#if arsenal.library.length === 0}
                <p class="arsenal-muted">
                  Import a skill first; presets are built from the library.
                </p>
              {/if}
              {#each arsenal.library as item (item.id)}
                <label class="arsenal-check">
                  <input
                    type="checkbox"
                    checked={inItems(presetDraft.items, item.id)}
                    onchange={() =>
                      (presetDraft.items = toggle(presetDraft.items, item.id))}
                  />
                  {item.name}
                  <span class="arsenal-muted"
                    >{PROVIDER_LABEL[item.provider]} · revision {item.revision}</span
                  >
                </label>
              {/each}
            </fieldset>
            {#if presetDraft.items.length > 0}
              <ul class="arsenal-files" aria-label="Inspect preset">
                {#each presetDraft.items as item (item.capability)}
                  <li>
                    {nameOf(item.capability)} · {item.revision === null
                      ? 'follows current revision'
                      : `pinned to revision ${item.revision}`}
                  </li>
                {/each}
              </ul>
            {/if}
            {#if presetError}<p
                id="preset-error"
                role="alert"
                class="arsenal-warn"
              >
                {presetError}
              </p>{/if}
            <button class="action" type="submit" disabled={busy}
              >{arsenal.busy === 'save' ? 'Saving…' : 'Save preset'}</button
            >
          </form>
        </div>
      </div>
    {/if}

    {#if arsenal.tab === 'selections'}
      <div
        id="arsenal-panel-selections"
        role="tabpanel"
        aria-labelledby="arsenal-tab-selections"
        class="arsenal-panel"
      >
        <div class="arsenal-split">
          <div>
            <button
              class="action"
              type="button"
              onclick={() => {
                selectionDraft = {
                  id: null,
                  name: '',
                  provider: 'claude_code',
                  items: [],
                };
                controller.clearNotice();
              }}>New selection</button
            >
            {#if arsenal.selections.length === 0}
              <p class="arsenal-empty">
                No selections yet. A selection is a saved list for one provider.
              </p>
            {/if}
            <ul class="arsenal-list" aria-label="Selections">
              {#each arsenal.selections as selection (selection.id)}
                <li class="arsenal-row-item">
                  <button
                    class="arsenal-row"
                    type="button"
                    onclick={() => editSelection(selection)}
                  >
                    <span class="arsenal-row-name">{selection.name}</span>
                    <span class="arsenal-badges"
                      ><span class="pill"
                        >{PROVIDER_LABEL[selection.provider]}</span
                      ><span class="pill"
                        >{selection.items.length} capabilities</span
                      ></span
                    >
                  </button>
                  <button
                    class="action"
                    type="button"
                    onclick={() => controller.loadPreview(selection.id)}
                    >Preview {selection.name}</button
                  >
                  {#if confirmDelete === selection.id}
                    <button
                      class="action"
                      type="button"
                      onclick={() => {
                        confirmDelete = null;
                        controller.deleteSelection(selection.id);
                      }}>Confirm delete {selection.name}</button
                    >
                  {:else}
                    <button
                      class="action"
                      type="button"
                      onclick={() => (confirmDelete = selection.id)}
                      >Delete {selection.name}</button
                    >
                  {/if}
                </li>
              {/each}
            </ul>
          </div>

          <form
            class="arsenal-editor"
            aria-label="Selection editor"
            onsubmit={(event) => {
              event.preventDefault();
              void saveSelection();
            }}
          >
            <h3>{selectionDraft.id ? 'Edit selection' : 'New selection'}</h3>
            <label
              >Name <input
                type="text"
                maxlength="80"
                bind:value={selectionDraft.name}
                aria-invalid={selectionError !== null}
                aria-describedby={selectionError
                  ? 'selection-error'
                  : undefined}
              /></label
            >
            <label
              >Provider
              <select bind:value={selectionDraft.provider}>
                <option value="claude_code">Claude Code</option>
                <option value="codex">Codex</option>
              </select>
            </label>
            <fieldset>
              <legend>Capabilities</legend>
              {#each arsenal.library as item (item.id)}
                <label class="arsenal-check">
                  <input
                    type="checkbox"
                    checked={inItems(selectionDraft.items, item.id)}
                    onchange={() =>
                      (selectionDraft.items = toggle(
                        selectionDraft.items,
                        item.id,
                      ))}
                  />
                  {item.name}
                  <span class="arsenal-muted"
                    >{PROVIDER_LABEL[item.provider]}{item.provider !==
                    selectionDraft.provider
                      ? ' · different provider'
                      : ''}</span
                  >
                </label>
              {/each}
            </fieldset>
            {#if selectionError}<p
                id="selection-error"
                role="alert"
                class="arsenal-warn"
              >
                {selectionError}
              </p>{/if}
            <button class="action" type="submit" disabled={busy}
              >{arsenal.busy === 'save' ? 'Saving…' : 'Save selection'}</button
            >
          </form>
        </div>

        {#if arsenal.preview}
          {@const preview = arsenal.preview}
          <section class="arsenal-detail" aria-label="Configuration preview">
            <h3>Configuration preview: {preview.selection.name}</h3>
            <p class="arsenal-status">
              <span class="pill">NOT APPLIED</span> Saved for later. No provider session
              uses it.
            </p>
            <ul class="arsenal-files">
              {#each preview.items as item (item.capability)}
                <li>
                  <span>{item.name || 'Missing capability'}</span>
                  {#if item.state === 'resolved'}
                    <span class="arsenal-muted"
                      >revision {item.revision}{item.pinned
                        ? ' (pinned)'
                        : ' (current)'}</span
                    >
                    <span class="pill"
                      >{COMPATIBILITY_LABEL[item.compatibility]}</span
                    >
                    <span class="arsenal-muted">{item.reason}</span>
                  {:else if item.state === 'revision_missing'}
                    <span class="arsenal-warn">Pinned revision is gone.</span>
                  {:else}
                    <span class="arsenal-warn"
                      >Not in the library any more.</span
                    >
                  {/if}
                </li>
              {/each}
            </ul>
            {#if preview.conflicts.length > 0}
              <h4>Conflicts</h4>
              <ul class="arsenal-files">
                {#each preview.conflicts as conflict, index (index)}
                  <li>
                    {conflict.code === 'name_collision'
                      ? 'Same name'
                      : 'Same content'}: {conflict.capabilities
                      .map(nameOf)
                      .join(', ')}
                  </li>
                {/each}
              </ul>
            {/if}
            <p class="arsenal-muted">
              Dependencies: not analysed in this version.
            </p>
          </section>
        {/if}
      </div>
    {/if}
  </div>
</section>

<style>
  .arsenal-body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--glass-line-strong) transparent;
    padding-bottom: 16px;
  }
  .arsenal-tabs {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin-block: 8px;
  }
  .arsenal-tab {
    min-height: 44px;
  }
  .arsenal-tab[aria-selected='true'] {
    background: var(--accent-soft);
    border-color: var(--accent-line);
  }
  .arsenal-tab:focus-visible,
  .arsenal-feature button:focus-visible,
  .arsenal-feature input:focus-visible,
  .arsenal-feature select:focus-visible,
  .arsenal-feature textarea:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .arsenal-filters {
    display: grid;
    gap: 8px;
    grid-template-columns: 1fr;
    margin-block: 8px;
  }
  .arsenal-filters label,
  .arsenal-editor label {
    display: grid;
    gap: 4px;
    font-size: 14px;
  }
  .arsenal-filters input,
  .arsenal-filters select,
  .arsenal-editor input,
  .arsenal-editor select,
  .arsenal-editor textarea {
    min-height: 44px;
    box-sizing: border-box;
    border-radius: var(--radius-sm);
    border: 1px solid var(--glass-line);
    background: var(--surface-control);
    color: var(--text);
    padding: 8px 12px;
    font: inherit;
  }
  .arsenal-list,
  .arsenal-files,
  .arsenal-revisions,
  .arsenal-sources {
    list-style: none;
    padding: 0;
    margin: 8px 0;
    display: grid;
    gap: 8px;
  }
  .arsenal-row {
    width: 100%;
    min-height: 44px;
    text-align: left;
    display: grid;
    gap: 4px;
    padding: 8px 12px;
    border-radius: var(--radius-sm);
    border: 1px solid transparent;
    background: var(--surface);
    color: var(--text);
    cursor: pointer;
  }
  .arsenal-row:hover {
    background: var(--surface-hover);
  }
  .arsenal-row[aria-pressed='true'] {
    background: var(--accent-soft);
    border-color: var(--accent-line);
  }
  .arsenal-row-name {
    font-size: 14px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .arsenal-badges {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .arsenal-detail,
  .arsenal-editor {
    margin-top: 16px;
    padding: 16px;
    border-radius: var(--radius-md);
    background: var(--surface-raised);
    display: grid;
    gap: 8px;
  }
  .arsenal-facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 4px 12px;
    font-size: 14px;
    margin: 0;
  }
  .arsenal-facts dd,
  .arsenal-facts dt {
    margin: 0;
  }
  .arsenal-facts code,
  .arsenal-preview,
  .arsenal-detail code {
    font-family: var(--font-mono);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .arsenal-preview {
    white-space: pre-wrap;
    padding: 12px;
    border-radius: var(--radius-sm);
    background: var(--surface-control);
    margin: 0;
  }
  .arsenal-muted {
    color: var(--muted);
    font-size: 12px;
  }
  .arsenal-empty {
    color: var(--muted);
    font-size: 14px;
  }
  .arsenal-warn {
    color: var(--warn);
    font-size: 14px;
  }
  .arsenal-notice {
    font-size: 14px;
    padding: 8px 12px;
    border-radius: var(--radius-sm);
  }
  .arsenal-notice.ok {
    background: var(--ok-soft);
    color: var(--ok);
  }
  .arsenal-notice.error {
    background: var(--error-soft);
    color: var(--error-text);
  }
  .arsenal-split {
    display: grid;
    gap: 16px;
    grid-template-columns: 1fr;
  }
  .arsenal-check {
    display: flex !important;
    align-items: center;
    gap: 8px !important;
    min-height: 44px;
  }
  .arsenal-status {
    font-size: 14px;
  }
  @media (min-width: 768px) {
    .arsenal-filters {
      grid-template-columns: 2fr 1fr 1fr 1fr auto;
      align-items: end;
    }
    .arsenal-split {
      grid-template-columns: 1fr 1fr;
    }
  }
  @media (max-width: 767px) {
    .arsenal-panel {
      font-size: 16px;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .arsenal-row,
    .arsenal-tab {
      transition: none;
    }
  }
</style>
