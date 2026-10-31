<script lang="ts">
  import type {
    ArsenalProvider,
    Candidate,
    CandidateKind,
    CandidateScope,
    CandidateStatus,
    Compatibility,
    Composite,
    Dependency,
    Item,
    Preset,
    Selection,
    Summary,
  } from '../../contracts/arsenal';
  import { blockedText } from './messages';
  import {
    filterDiscover,
    filterLibrary,
    libraryCompatibility,
    librarySources,
    type KindFilter,
    type ProviderFilter,
    type ScopeFilter,
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
    { id: 'composites', label: 'Composites' },
    { id: 'presets', label: 'Presets' },
    { id: 'selections', label: 'Selections' },
    { id: 'terminals', label: 'Terminals' },
  ];
  const PROVIDER_LABEL: Record<ArsenalProvider, string> = {
    claude_code: 'Claude Code',
    codex: 'Codex',
  };
  const KIND_LABEL: Record<CandidateKind, string> = {
    skill: 'Skill',
    agent: 'Agent',
    rule: 'Rule',
    hook: 'Hook',
    mcp_server: 'MCP server',
    plugin: 'Plugin',
  };
  const SCOPE_LABEL: Record<CandidateScope, string> = {
    user: 'Account',
    project: 'Project',
  };
  const STATUS_LABEL: Record<CandidateStatus, string> = {
    not_imported: 'Not imported',
    imported: 'Imported',
    source_changed: 'Source changed',
    read_only: 'Read only',
  };
  const COMPATIBILITY_LABEL: Record<Compatibility, string> = {
    supported: 'Supported',
    unsupported: 'Unsupported',
    unverified: 'Unverified',
    requires_qualification: 'Needs provider qualification',
    conflicting: 'Conflicting',
    missing_dependency: 'Missing dependency',
  };
  const DEPENDENCY_KIND: Record<Dependency['kind'], string> = {
    tool: 'Tool',
    mcp_server: 'MCP server',
    file: 'File',
  };
  const DEPENDENCY_STATE: Record<Dependency['state'], string> = {
    declared: 'declared, not checked offline',
    present: 'present',
    missing: 'missing',
    unknown: 'unknown',
  };
  /** Why a read-only kind is listed and never copied. Said once, next to the entry. */
  const READ_ONLY_NOTE: Record<string, string> = {
    hook: 'A hook runs code. Arsenal lists it and never copies or runs it.',
    mcp_server:
      'An MCP definition can hold credentials. Arsenal lists it and never copies it.',
    plugin:
      'A plugin brings code and hooks. Arsenal lists it; importing plugins is not available yet.',
  };
  const SOURCE_LABEL: Record<string, string> = {
    claude_user_skills: 'Claude user skills',
    codex_user_skills: 'Codex user skills',
    codex_home_skills: 'Codex home skills',
    codex_system_skills: 'Codex system skills',
    claude_project_skills: 'Claude project skills',
    codex_project_skills: 'Codex project skills',
    claude_user_agents: 'Claude user agents',
    codex_user_agents: 'Codex user agents',
    claude_project_agents: 'Claude project agents',
    codex_project_agents: 'Codex project agents',
    claude_user_rules: 'Claude user rules',
    codex_user_rules: 'Codex user rules',
    claude_project_rules: 'Claude project rules',
    codex_project_rules: 'Codex project rules',
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
  const CONFLICT_LABEL: Record<string, string> = {
    name_collision: 'Same name',
    duplicate_content: 'Same content',
    instruction_overlap: 'Two rules for one provider',
    methodology_conflict: 'Two methodologies',
    scope_shadow: 'Installed in account and project',
  };

  // Library filters
  let libraryQuery = $state('');
  let libraryProvider = $state<ProviderFilter>('all');
  let librarySource = $state<string | 'all'>('all');
  let libraryCompat = $state<Compatibility | 'all'>('all');
  // Discover filters
  let discoverQuery = $state('');
  let discoverProvider = $state<ProviderFilter>('all');
  let discoverKind = $state<KindFilter>('all');
  let discoverScope = $state<ScopeFilter>('all');
  let discoverStatus = $state<CandidateStatus | 'all'>('all');
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
    composites: [] as string[],
  });
  let startFromPreset = $state<string>('');
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
  const candidates = $derived(
    filterDiscover(arsenal.discover?.candidates ?? [], {
      query: discoverQuery,
      provider: discoverProvider,
      kind: discoverKind,
      scope: discoverScope,
      status: discoverStatus,
    }),
  );
  const chosen = $derived<Candidate | null>(
    arsenal.discover?.candidates.find(
      (candidate) => candidate.id === arsenal.chosen,
    ) ?? null,
  );
  const busy = $derived(arsenal.busy !== null);
  const eligibleCount = $derived(
    arsenal.discover?.candidates.filter(
      (candidate) =>
        candidate.importable &&
        (candidate.status === 'not_imported' ||
          candidate.status === 'source_changed'),
    ).length ?? 0,
  );
  const detail = $derived(
    arsenal.chosenDetail && chosen && arsenal.chosenDetail.id === chosen.id
      ? arsenal.chosenDetail
      : null,
  );

  function nameOfCandidate(id: string): string {
    return (
      arsenal.discover?.candidates.find((candidate) => candidate.id === id)
        ?.name ?? id
    );
  }
  function inItems(items: Item[], id: string): boolean {
    return items.some((item) => item.capability === id);
  }
  function toggle(items: Item[], id: string): Item[] {
    return inItems(items, id)
      ? items.filter((item) => item.capability !== id)
      : [...items, { capability: id, revision: null }];
  }
  /** Pins an item to a revision, or lets it follow the current one. */
  function pin(items: Item[], id: string, value: string): Item[] {
    return items.map((item) =>
      item.capability === id
        ? { ...item, revision: value === 'current' ? null : Number(value) }
        : item,
    );
  }
  function revisionsOf(id: string): number[] {
    const count =
      arsenal.library.find((item) => item.id === id)?.revisions ?? 0;
    return Array.from({ length: count }, (_, index) => count - index);
  }
  function compositeFor(provider: ArsenalProvider): Composite[] {
    return (arsenal.discover?.composites ?? []).filter(
      (composite) =>
        composite.kind === 'gsd_core' &&
        (composite.provider === provider || composite.provider === 'shared'),
    );
  }
  function toggleComposite(key: string) {
    selectionDraft.composites = selectionDraft.composites.includes(key)
      ? selectionDraft.composites.filter((entry) => entry !== key)
      : [...selectionDraft.composites, key];
  }
  function applyPreset(id: string) {
    const preset = arsenal.presets.find((entry) => entry.id === id);
    if (!preset) return;
    const known = new Set(selectionDraft.items.map((item) => item.capability));
    selectionDraft.items = [
      ...selectionDraft.items,
      ...preset.items
        .filter((item) => !known.has(item.capability))
        .map((item) => ({ ...item })),
    ];
    startFromPreset = '';
  }
  function copySelection(selection: Selection) {
    selectionDraft = {
      id: null,
      name: `Copy of ${selection.name}`,
      provider: selection.provider,
      items: selection.items.map((item) => ({ ...item })),
      composites: [...selection.composites],
    };
    selectionError = null;
  }
  function when(seconds: number): string {
    return new Date(seconds * 1000).toLocaleString(undefined, {
      dateStyle: 'short',
      timeStyle: 'short',
    });
  }
  function nameOf(id: string): string {
    return (
      arsenal.library.find((item) => item.id === id)?.name ??
      'Missing capability'
    );
  }
  function kindOf(item: Summary): string {
    return KIND_LABEL[item.kind as CandidateKind] ?? item.kind;
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
  /** Starts a preset or a selection with one capability, named after it, in its own tab. */
  function presetFrom(summary: Summary) {
    presetDraft = {
      id: null,
      name: summary.name,
      description: '',
      items: [{ capability: summary.id, revision: null }],
    };
    presetError = null;
    controller.setTab('presets');
  }
  function selectionFrom(summary: Summary) {
    selectionDraft = {
      id: null,
      name: summary.name,
      provider: summary.provider,
      items: [{ capability: summary.id, revision: null }],
      composites: [],
    };
    selectionError = null;
    controller.setTab('selections');
  }
  function editSelection(selection: Selection) {
    selectionDraft = {
      id: selection.id,
      name: selection.name,
      provider: selection.provider,
      items: selection.items.map((item) => ({ ...item })),
      composites: [...selection.composites],
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
        composites: [],
      };
    }
  }
  function revisionOf(detail: { summary: Summary }): string {
    return `${detail.summary.revision} of ${detail.summary.revisions}`;
  }
  function excludedText(reason: string): string {
    return EXCLUDED_LABEL[reason] ?? reason;
  }
  function importLabel(candidate: Candidate): string {
    if (arsenal.busy === 'import') return 'Importing…';
    if (candidate.status === 'imported') return 'Already imported';
    if (candidate.status === 'source_changed') return 'Import as new revision';
    return 'Import';
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
      <p class="sessions-count">Capabilities installed on this machine</p>
    </div>
    <button
      class="action"
      type="button"
      disabled={arsenal.phase === 'loading' || busy}
      onclick={() => controller.refresh()}>Refresh</button
    >
  </div>
  <p class="configuration-note">
    Installed does not mean imported. A selection reaches a terminal only when
    you start one with it from Workspace.
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
              <option value="supported">Supported</option>
              <option value="unverified">Unverified</option>
              <option value="missing_dependency">Missing dependency</option>
              <option value="unsupported">Unsupported</option>
            </select>
          </label>
        </div>

        {#if arsenal.library.length === 0 && arsenal.phase === 'ready'}
          <p class="arsenal-empty">
            Nothing is imported yet. Open Discover to import a capability you
            already have.
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
                  <span class="pill">{kindOf(item)}</span>
                  <span class="pill">{PROVIDER_LABEL[item.provider]}</span>
                  <span class="pill"
                    >{SOURCE_LABEL[item.source] ?? item.source}</span
                  >
                  <span class="pill">Revision {item.revision}</span>
                  <span class="pill"
                    >{COMPATIBILITY_LABEL[libraryCompatibility(item)]}</span
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
              {KIND_LABEL[opened.summary.kind as CandidateKind] ??
                opened.summary.kind} · {PROVIDER_LABEL[opened.summary.provider]} ·
              {SOURCE_LABEL[opened.summary.source] ?? opened.summary.source} · Revision
              {revisionOf(opened)}
            </p>
            {#if opened.summary.description}<p>
                {opened.summary.description}
              </p>{/if}
            <dl class="arsenal-facts">
              <dt>Imported from</dt>
              <dd><code>{opened.summary.origin}</code></dd>
              <dt>Content</dt>
              <dd><code>{opened.summary.tree_hash.slice(0, 16)}</code></dd>
              <dt>Compatibility</dt>
              <dd>
                <span class="pill"
                  >{COMPATIBILITY_LABEL[opened.compatibility.status]}</span
                >
                {opened.compatibility.reason}
              </dd>
              <dt>Use in a session</dt>
              <dd>
                Only through a terminal plan: Workspace shows, per terminal,
                whether this revision is installed and whether it can be
                applied.
              </dd>
            </dl>
            <div class="arsenal-actions">
              <button
                class="action"
                type="button"
                onclick={() => presetFrom(opened.summary)}
                >Start a preset with this</button
              >
              <button
                class="action"
                type="button"
                onclick={() => selectionFrom(opened.summary)}
                >Start a selection with this</button
              >
            </div>
            <h4>Dependencies</h4>
            {#if opened.dependencies.length === 0}
              <p class="arsenal-muted">
                Its files declare and point at nothing Arsenal can check.
              </p>
            {:else}
              <ul class="arsenal-files" aria-label="Dependencies">
                {#each opened.dependencies as dep (dep.kind + dep.name)}
                  <li>
                    <span
                      >{DEPENDENCY_KIND[dep.kind]}:
                      <code>{dep.name}</code></span
                    >
                    <span
                      class={dep.state === 'missing'
                        ? 'arsenal-warn'
                        : 'arsenal-muted'}>{DEPENDENCY_STATE[dep.state]}</span
                    >
                  </li>
                {/each}
              </ul>
            {/if}

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
            >Kind
            <select bind:value={discoverKind}>
              <option value="all">All</option>
              <option value="skill">Skills</option>
              <option value="agent">Agents</option>
              <option value="rule">Rules</option>
              <option value="mcp_server">MCP servers</option>
              <option value="hook">Hooks</option>
              <option value="plugin">Plugins</option>
            </select>
          </label>
          <label
            >Where
            <select bind:value={discoverScope}>
              <option value="all">Account and project</option>
              <option value="user">Account</option>
              <option value="project">Project</option>
            </select>
          </label>
          <label
            >Status
            <select bind:value={discoverStatus}>
              <option value="all">All</option>
              <option value="not_imported">Not imported</option>
              <option value="imported">Imported</option>
              <option value="source_changed">Source changed</option>
              <option value="read_only">Read only</option>
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
          <button
            class="action"
            type="button"
            disabled={busy || arsenal.discovering || eligibleCount === 0}
            onclick={() => controller.importEligible()}
          >
            {busy && arsenal.bulk && !arsenal.bulk.finished
              ? `Importing ${arsenal.bulk.done} of ${arsenal.bulk.total}…`
              : `Import ${eligibleCount} eligible`}
          </button>
        </div>

        {#if arsenal.bulk && !arsenal.bulk.finished}
          <progress
            max={arsenal.bulk.total}
            value={arsenal.bulk.done}
            aria-label="Bulk import progress"
          ></progress>
        {/if}

        {#if arsenal.discovering && arsenal.discover === null}
          <p role="status">Reading installed capabilities…</p>
        {:else if arsenal.discover}
          <p class="arsenal-muted">
            {#if arsenal.discover.project}
              Project: {arsenal.discover.project}
            {:else}
              No project selected. Project capabilities are not listed; select a
              project in Configuration.
            {/if}
          </p>
          <ul class="arsenal-sources" aria-label="Capability folders">
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
              The folders have more entries than the scan shows. Some are not
              listed.
            </p>
          {/if}

          {#if arsenal.discover.composites.length > 0}
            <p class="arsenal-muted">
              {arsenal.discover.composites.length} composite collection(s) installed.
              <button
                class="action"
                type="button"
                onclick={() => controller.setTab('composites')}
                >Open Composites</button
              >
            </p>
          {/if}

          {#if arsenal.discover.candidates.length === 0}
            <p class="arsenal-empty">
              No capabilities found in your Claude, Codex or project folders.
            </p>
          {:else if candidates.length === 0}
            <p class="arsenal-empty">No capabilities match these filters.</p>
          {/if}

          <ul class="arsenal-list" aria-label="Discovered capabilities">
            {#each candidates as candidate (candidate.id)}
              <li class="arsenal-row-item">
                <button
                  class="arsenal-row"
                  type="button"
                  aria-pressed={arsenal.chosen === candidate.id}
                  onclick={() => controller.choose(candidate.id)}
                >
                  <span class="arsenal-row-name">{candidate.name}</span>
                  <span class="arsenal-badges">
                    <span class="pill">{KIND_LABEL[candidate.kind]}</span>
                    <span class="pill"
                      >{PROVIDER_LABEL[candidate.provider]}</span
                    >
                    <span class="pill">{SCOPE_LABEL[candidate.scope]}</span>
                    <span class="pill">{STATUS_LABEL[candidate.status]}</span>
                    {#if candidate.managed_by}<span class="pill"
                        >{candidate.managed_by.composite}
                        {candidate.managed_by.version}</span
                      >{/if}
                    {#if candidate.blocked && candidate.status !== 'read_only' && !candidate.managed_by}<span
                        class="pill">Blocked</span
                      >{/if}
                    {#if candidate.duplicates.length > 0}<span class="pill"
                        >Duplicate content</span
                      >{/if}
                    {#if candidate.name_collisions.length > 0}<span class="pill"
                        >Name collision</span
                      >{/if}
                  </span>
                </button>
              </li>
            {/each}
          </ul>

          {#if chosen}
            <section class="arsenal-detail" aria-label="Capability to import">
              <h3>{chosen.name}</h3>
              <p class="arsenal-muted">
                {KIND_LABEL[chosen.kind]} · {PROVIDER_LABEL[chosen.provider]} · {SOURCE_LABEL[
                  chosen.source
                ] ?? chosen.source} · {SCOPE_LABEL[chosen.scope]} · {STATUS_LABEL[
                  chosen.status
                ]}
              </p>
              {#if chosen.description}<p>{chosen.description}</p>{/if}
              <dl class="arsenal-facts">
                <dt>Read from</dt>
                <dd><code>{chosen.origin}</code></dd>
                {#if chosen.tree_hash}
                  <dt>Content</dt>
                  <dd><code>{chosen.tree_hash.slice(0, 16)}</code></dd>
                {/if}
                <dt>Compatibility</dt>
                <dd>{COMPATIBILITY_LABEL[chosen.compatibility]}</dd>
              </dl>

              {#if chosen.status === 'read_only'}
                <p class="arsenal-muted">
                  {READ_ONLY_NOTE[chosen.kind] ??
                    'Listed only. Arsenal does not copy it.'}
                </p>
              {:else}
                {#if chosen.executes_code}
                  <p class="arsenal-warn">
                    Contains scripts. Arsenal copies them and never runs them.
                  </p>
                {/if}
                {#if chosen.duplicates.length > 0}
                  <p>
                    Same content as {chosen.duplicates
                      .map(nameOfCandidate)
                      .join(', ')}.
                  </p>
                {/if}
                {#if chosen.name_collisions.length > 0}
                  <p>
                    Another capability here has the same name. Both can be
                    imported; they stay separate.
                  </p>
                {/if}

                <h4>
                  {chosen.importable
                    ? 'Files that will be copied'
                    : 'Files in this capability'}
                </h4>
                <ul class="arsenal-files">
                  {#each detail?.files ?? [] as file (file.path)}
                    <li>
                      <span>{file.path}</span>
                      <span class="arsenal-muted">{file.size} B</span>
                    </li>
                  {/each}
                </ul>
                {#if (detail?.excluded.length ?? 0) > 0}
                  <h4>Not copied</h4>
                  <ul class="arsenal-files">
                    {#each detail?.excluded ?? [] as entry (entry.path)}
                      <li>
                        <span>{entry.path}</span>
                        <span class="arsenal-muted"
                          >{excludedText(entry.reason)}</span
                        >
                      </li>
                    {/each}
                  </ul>
                {/if}
                {#if (detail?.review.length ?? 0) > 0}
                  <h4>Review before importing</h4>
                  <p class="arsenal-muted">
                    These files use a credential word. They are copied; check
                    that none holds a real value.
                  </p>
                  <ul class="arsenal-files">
                    {#each detail?.review ?? [] as entry (entry.path)}
                      <li>
                        <span>{entry.path}</span>
                        <span class="arsenal-muted"
                          >Mentions a credential word</span
                        >
                      </li>
                    {/each}
                  </ul>
                {/if}

                {#if chosen.managed_by}
                  <p role="alert">
                    This is a link into {chosen.managed_by.composite}
                    {chosen.managed_by.version}. It is read only: GSD Core
                    installs it, so Arsenal does not copy it.
                  </p>
                {:else if chosen.blocked}
                  <p role="alert">{blockedText(chosen.blocked)}</p>
                {/if}
                <button
                  class="action"
                  type="button"
                  disabled={busy ||
                    !chosen.importable ||
                    chosen.status === 'imported'}
                  onclick={() => controller.importCandidate(chosen.id)}
                >
                  {importLabel(chosen)}
                </button>
              {/if}
            </section>
          {/if}
        {/if}
      </div>
    {/if}

    {#if arsenal.tab === 'composites'}
      <div
        id="arsenal-panel-composites"
        role="tabpanel"
        aria-labelledby="arsenal-tab-composites"
        class="arsenal-panel"
      >
        <p class="configuration-note">
          A composite is installed by its own tool: GSD Core by its installer, a
          plugin by Claude Code. Arsenal describes it and never copies it.
        </p>
        {#if arsenal.discovering && arsenal.discover === null}
          <p role="status">Reading installed composites…</p>
        {:else if arsenal.discover && arsenal.discover.composites.length === 0}
          <p class="arsenal-empty">
            No composite collection is installed in your Claude or Codex
            folders.
          </p>
        {/if}
        {#each arsenal.discover?.composites ?? [] as composite (composite.key)}
          <section class="arsenal-detail" aria-label={composite.name}>
            <h3>{composite.name}</h3>
            <p class="arsenal-badges">
              <span class="pill"
                >{composite.kind === 'gsd_core'
                  ? 'GSD Core payload'
                  : 'Claude Code plugin'}</span
              >
              {#if composite.version}<span class="pill"
                  >Version {composite.version}</span
                >{/if}
              {#if composite.enabled !== null}<span class="pill"
                  >{composite.enabled ? 'Enabled' : 'Disabled'} in your settings</span
                >{/if}
              {#if composite.methodology}<span class="pill">Methodology</span
                >{/if}
              <span class="pill">Not importable</span>
            </p>
            {#if composite.description}<p>{composite.description}</p>{/if}
            <dl class="arsenal-facts">
              <dt>Source</dt>
              <dd><code>{composite.origin}</code></dd>
              <dt>Members</dt>
              <dd>
                {composite.skills.length} skills · {composite.agents.length} agents
                {#if composite.commands > 0}· {composite.commands} commands{/if}
                {#if composite.artifacts > 0}· {composite.artifacts} artifacts in
                  its manifest{/if}
              </dd>
              <dt>Runs code</dt>
              <dd>
                {composite.hooks
                  ? 'Yes: it registers hooks.'
                  : composite.kind === 'gsd_core'
                    ? 'Its hooks and engine belong to the GSD Core install.'
                    : 'No hooks declared.'}
                {composite.mcp ? ' It declares MCP servers.' : ''}
              </dd>
            </dl>
            {#if composite.categories.length > 0}
              <h4>Manifest</h4>
              <ul class="arsenal-files">
                {#each composite.categories as [category, count] (category)}
                  <li>
                    <span>{category.replaceAll('_', ' ')}</span><span
                      class="arsenal-muted">{count}</span
                    >
                  </li>
                {/each}
              </ul>
            {/if}
            <h4>Limits</h4>
            <ul class="arsenal-files">
              {#each composite.limitations as line (line)}<li>{line}</li>{/each}
            </ul>
            {#if composite.skills.length > 0}
              <details>
                <summary>Skills ({composite.skills.length})</summary>
                <p class="arsenal-member-list">{composite.skills.join(', ')}</p>
              </details>
            {/if}
            {#if composite.agents.length > 0}
              <details>
                <summary>Agents ({composite.agents.length})</summary>
                <p class="arsenal-member-list">{composite.agents.join(', ')}</p>
              </details>
            {/if}
          </section>
        {/each}
      </div>
    {/if}

    {#if arsenal.tab === 'terminals'}
      <div
        id="arsenal-panel-terminals"
        role="tabpanel"
        aria-labelledby="arsenal-tab-terminals"
        class="arsenal-panel"
      >
        <p class="configuration-note">
          What each terminal started with, recorded right after its start from
          the host's own session record. Applied means the host launched it with
          that exact exclusion; whether the provider then used each skill is not
          observed.
        </p>
        <button
          class="action"
          type="button"
          onclick={() => controller.loadBindings()}>Refresh</button
        >
        {#if arsenal.bindings.length === 0}
          <p class="arsenal-empty">
            No terminal has started with a selection yet. Choose one in
            Workspace when starting Claude Code or Codex.
          </p>
        {/if}
        <ul class="arsenal-list" aria-label="Terminal configurations">
          {#each arsenal.bindings as binding (binding.session_id)}
            <li class="arsenal-detail">
              <h3>{binding.selection_name}</h3>
              <p class="arsenal-badges">
                <span class="pill"
                  >{binding.state === 'applied'
                    ? 'APPLIED'
                    : 'NOT APPLIED'}</span
                >
                <span class="pill">Use not observed</span>
                <span class="pill">{PROVIDER_LABEL[binding.provider]}</span>
                {#if binding.provider_version}<span class="pill"
                    >{binding.provider_version}</span
                  >{/if}
              </p>
              <p>{binding.reason}</p>
              <p class="arsenal-muted">
                <code>{binding.project}</code> · {when(binding.created_at)} · session
                <code>{binding.session_id.slice(0, 8)}</code>
              </p>
              {#if binding.plan}
                <p class="arsenal-muted">
                  {binding.plan.items.filter((i) => i.state === 'prepared')
                    .length} prepared · {binding.plan.excluded.length} installed skills
                  {binding.state === 'applied'
                    ? 'switched off'
                    : 'would have been switched off'} · {binding.plan.kept
                    .length}
                  kept
                </p>
              {/if}
            </li>
          {/each}
        </ul>
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
                  Import a capability first; presets are built from the library.
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
                    >{kindOf(item)} · {PROVIDER_LABEL[item.provider]} · revision {item.revision}
                    · {COMPATIBILITY_LABEL[libraryCompatibility(item)]}</span
                  >
                </label>
              {/each}
            </fieldset>
            {#if presetDraft.items.length > 0}
              <ul class="arsenal-files" aria-label="Inspect preset">
                {#each presetDraft.items as item (item.capability)}
                  <li>
                    <span>{nameOf(item.capability)}</span>
                    <label class="arsenal-pin"
                      >Revision
                      <select
                        value={item.revision === null
                          ? 'current'
                          : String(item.revision)}
                        onchange={(event) =>
                          (presetDraft.items = pin(
                            presetDraft.items,
                            item.capability,
                            event.currentTarget.value,
                          ))}
                      >
                        <option value="current">Follow current</option>
                        {#each revisionsOf(item.capability) as number (number)}
                          <option value={String(number)}>Pin {number}</option>
                        {/each}
                      </select>
                    </label>
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
                  composites: [],
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
                  <button
                    class="action"
                    type="button"
                    onclick={() => copySelection(selection)}
                    >Duplicate {selection.name}</button
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
                    >{kindOf(item)} · {PROVIDER_LABEL[
                      item.provider
                    ]}{item.provider !== selectionDraft.provider
                      ? ' · different provider'
                      : ''}</span
                  >
                </label>
              {/each}
            </fieldset>
            {#if arsenal.presets.length > 0}
              <label
                >Add a preset's capabilities
                <select
                  bind:value={startFromPreset}
                  onchange={() => applyPreset(startFromPreset)}
                >
                  <option value="">Choose a preset…</option>
                  {#each arsenal.presets as preset (preset.id)}
                    <option value={preset.id}>{preset.name}</option>
                  {/each}
                </select>
              </label>
            {/if}
            {#if selectionDraft.items.length > 0}
              <ul
                class="arsenal-files"
                aria-label="Revisions in this selection"
              >
                {#each selectionDraft.items as item (item.capability)}
                  <li>
                    <span>{nameOf(item.capability)}</span>
                    <label class="arsenal-pin"
                      >Revision
                      <select
                        value={item.revision === null
                          ? 'current'
                          : String(item.revision)}
                        onchange={(event) =>
                          (selectionDraft.items = pin(
                            selectionDraft.items,
                            item.capability,
                            event.currentTarget.value,
                          ))}
                      >
                        <option value="current">Follow current</option>
                        {#each revisionsOf(item.capability) as number (number)}
                          <option value={String(number)}>Pin {number}</option>
                        {/each}
                      </select>
                    </label>
                  </li>
                {/each}
              </ul>
            {/if}
            <fieldset>
              <legend>Keep installed composites</legend>
              {#if arsenal.discover === null}
                <p class="arsenal-muted">
                  {arsenal.discovering
                    ? 'Reading installed composites…'
                    : 'Installed composites are read from Discover.'}
                </p>
              {:else if compositeFor(selectionDraft.provider).length === 0}
                <p class="arsenal-muted">
                  No composite installed for this provider. Plugins are not
                  listed here: their skills are switched as a whole, outside
                  Arsenal.
                </p>
              {/if}
              {#each compositeFor(selectionDraft.provider) as composite (composite.key)}
                <label class="arsenal-check">
                  <input
                    type="checkbox"
                    checked={selectionDraft.composites.includes(composite.key)}
                    onchange={() => toggleComposite(composite.key)}
                  />
                  {composite.name}
                  <span class="arsenal-muted"
                    >{composite.skills.length} skills stay exposed{composite.methodology
                      ? ' · methodology'
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
                      >{KIND_LABEL[item.kind as CandidateKind] ?? item.kind} · revision
                      {item.revision}{item.pinned
                        ? ' (pinned)'
                        : ' (current)'}</span
                    >
                    <span class="pill"
                      >{COMPATIBILITY_LABEL[item.compatibility]}</span
                    >
                    <span class="arsenal-muted">{item.reason}</span>
                    {#each item.dependencies.filter((dep) => dep.state === 'missing') as dep (dep.kind + dep.name)}
                      <span class="arsenal-warn"
                        >Missing {DEPENDENCY_KIND[dep.kind].toLowerCase()}
                        {dep.name}</span
                      >
                    {/each}
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
                    {CONFLICT_LABEL[conflict.code] ?? conflict.code}: {conflict.capabilities
                      .map(nameOf)
                      .join(', ')}
                  </li>
                {/each}
              </ul>
            {/if}
            <p class="arsenal-muted">
              {preview.dependencies === 'analysed'
                ? 'Dependencies were read from every file. Tools are declared, not checked; MCP servers are checked per terminal.'
                : 'Some items were too large to read here; their dependencies are not listed.'}
            </p>
            <p class="arsenal-muted">
              To see what a terminal would actually start with, choose this
              selection in Workspace when starting Claude Code or Codex.
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
  .arsenal-pin {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }
  .arsenal-member-list {
    font-family: var(--font-mono);
    font-size: 12px;
    overflow-wrap: anywhere;
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
  .arsenal-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
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
      grid-template-columns: 2fr 1fr 1fr 1fr 1fr 1fr auto;
      align-items: end;
    }
    .arsenal-split {
      grid-template-columns: 1fr 1fr;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .arsenal-row,
    .arsenal-tab {
      transition: none;
    }
  }
</style>
