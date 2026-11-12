<script lang="ts">
  import type {
    ArsenalProvider,
    Candidate,
    CandidateKind,
    CandidateScope,
    CandidateStatus,
    Compatibility,
    CompositeKind,
    Dependency,
    Summary,
  } from '../../contracts/arsenal';
  import { PROVIDERS } from '../../contracts/arsenal';
  import { blockedText } from './messages';
  import PresetsTab from './PresetsTab.svelte';
  import SyncLine from './SyncLine.svelte';
  import {
    candidateMark,
    compatibilityMark,
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
    { id: 'presets', label: 'Presets' },
    { id: 'library', label: 'Library' },
    { id: 'discover', label: 'Discover' },
    { id: 'composites', label: 'Composites' },
    { id: 'terminals', label: 'Terminals' },
  ];
  const PROVIDER_OPTIONS = PROVIDERS;
  const PROVIDER_LABEL: Record<ArsenalProvider, string> = {
    claude_code: 'Claude Code',
    codex: 'Codex',
  };
  /** The provider every tab works on. Chosen above the tabs; the presets draft is kept per provider. */
  let provider = $state<ArsenalProvider>('claude_code');
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
  const COMPOSITE_KIND_LABEL: Record<CompositeKind, string> = {
    gsd_core: 'GSD Core payload',
    claude_plugin: 'Claude Code plugin',
    codex_plugin: 'Codex plugin',
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
  /** Pins an item to a revision, or lets it follow the current one. */
  function kindOf(item: Summary): string {
    return KIND_LABEL[item.kind as CandidateKind] ?? item.kind;
  }
  /**
   * Starts a preset or a selection with one capability, named after it, in its own tab. A draft
   * already in the editor is never discarded: the capability is added to it instead.
   */
  function when(seconds: number): string {
    return new Date(seconds * 1000).toLocaleString(undefined, {
      dateStyle: 'short',
      timeStyle: 'short',
    });
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

  {#if arsenal.sync?.enabled}
    <SyncLine sync={arsenal.sync} onsyncnow={() => controller.syncNow()} />
  {/if}

  <div class="arsenal-provider" role="radiogroup" aria-label="Provider">
    {#each PROVIDER_OPTIONS as option (option)}
      <button
        class="action arsenal-provider-option"
        type="button"
        role="radio"
        aria-checked={provider === option}
        tabindex={provider === option ? 0 : -1}
        onclick={() => (provider = option)}>{PROVIDER_LABEL[option]}</button
      >
    {/each}
  </div>
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
        {:else if arsenal.library.length > 0}
          <p class="arsenal-count" aria-live="polite">
            {visibleLibrary.length === arsenal.library.length
              ? `${arsenal.library.length} imported`
              : `${visibleLibrary.length} of ${arsenal.library.length} imported`}
          </p>
        {/if}

        <ul class="arsenal-list" aria-label="Imported capabilities">
          {#each visibleLibrary as item (item.id)}
            {@const mark = compatibilityMark(libraryCompatibility(item))}
            <li class="arsenal-row-item">
              <button
                class="arsenal-row stripe"
                data-tone={mark.tone}
                type="button"
                aria-pressed={arsenal.opened?.summary.id === item.id}
                onclick={() => controller.open(item.id)}
              >
                <span class="arsenal-row-top">
                  <span class="arsenal-row-name">{item.name}</span>
                  <span class="badge" data-tone={mark.tone}
                    ><span aria-hidden="true">{mark.symbol}</span>
                    {COMPATIBILITY_LABEL[libraryCompatibility(item)]}</span
                  >
                </span>
                {#if item.description}
                  <span class="arsenal-row-desc">{item.description}</span>
                {/if}
                <span class="arsenal-row-meta"
                  >{kindOf(item)} · {PROVIDER_LABEL[item.provider]} · {SOURCE_LABEL[
                    item.source
                  ] ?? item.source} · Revision {item.revision}</span
                >
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
                <span
                  class="badge"
                  data-tone={compatibilityMark(opened.compatibility.status)
                    .tone}
                  ><span aria-hidden="true"
                    >{compatibilityMark(opened.compatibility.status)
                      .symbol}</span
                  >
                  {COMPATIBILITY_LABEL[opened.compatibility.status]}</span
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
            <h4>Dependencies</h4>
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
              {@const mark = candidateMark(candidate.status)}
              {@const blocked =
                candidate.blocked &&
                candidate.status !== 'read_only' &&
                !candidate.managed_by}
              <li class="arsenal-row-item">
                <button
                  class="arsenal-row stripe"
                  data-tone={blocked ? 'error' : mark.tone}
                  type="button"
                  aria-pressed={arsenal.chosen === candidate.id}
                  onclick={() => controller.choose(candidate.id)}
                >
                  <span class="arsenal-row-top">
                    <span class="arsenal-row-name">{candidate.name}</span>
                    <span class="arsenal-badges">
                      <span class="badge" data-tone={mark.tone}
                        ><span aria-hidden="true">{mark.symbol}</span>
                        {STATUS_LABEL[candidate.status]}</span
                      >
                      {#if blocked}<span class="badge" data-tone="error"
                          ><span aria-hidden="true">✕</span> Blocked</span
                        >{/if}
                      {#if candidate.duplicates.length > 0}<span
                          class="badge"
                          data-tone="warn"
                          ><span aria-hidden="true">!</span> Duplicate content</span
                        >{/if}
                      {#if candidate.name_collisions.length > 0}<span
                          class="badge"
                          data-tone="warn"
                          ><span aria-hidden="true">!</span> Name collision</span
                        >{/if}
                    </span>
                  </span>
                  {#if candidate.description}
                    <span class="arsenal-row-desc">{candidate.description}</span
                    >
                  {/if}
                  <span class="arsenal-row-meta"
                    >{KIND_LABEL[candidate.kind]} · {PROVIDER_LABEL[
                      candidate.provider
                    ]} · {SCOPE_LABEL[
                      candidate.scope
                    ]}{#if candidate.managed_by}
                      · {candidate.managed_by.composite}
                      {candidate.managed_by.version}{/if}</span
                  >
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
          plugin by Claude Code or Codex. Arsenal describes it and never copies
          it.
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
              <span class="pill">{COMPOSITE_KIND_LABEL[composite.kind]}</span>
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
                <span
                  class="badge"
                  data-tone={binding.state === 'applied' ? 'ok' : 'warn'}
                  ><span aria-hidden="true"
                    >{binding.state === 'applied' ? '✓' : '!'}</span
                  >
                  {binding.state === 'applied'
                    ? 'APPLIED'
                    : 'NOT APPLIED'}</span
                >
                <span class="badge" data-tone="pending"
                  ><span aria-hidden="true">?</span> Use not observed</span
                >
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
        <PresetsTab {provider} {arsenal} {controller} />
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
    scrollbar-color: var(--line-strong) transparent;
    padding-bottom: 16px;
  }
  /* Sections are tabs, drawn as tabs: an underline on the open one, not a row of buttons. */
  .arsenal-tabs {
    display: flex;
    gap: 2px;
    flex-wrap: wrap;
    margin-top: 4px;
    border-bottom: 1px solid var(--line);
  }
  .arsenal-tab {
    position: relative;
    min-height: 38px;
    padding: 0 14px;
    border: 0;
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
    background: transparent;
    color: var(--muted);
    font-weight: 500;
  }
  .arsenal-tab:hover:not(:disabled) {
    color: var(--text);
    background: var(--surface-hover);
  }
  .arsenal-tab[aria-selected='true'] {
    color: var(--text);
    font-weight: 600;
    background: transparent;
  }
  .arsenal-tab[aria-selected='true']::after {
    content: '';
    position: absolute;
    left: 8px;
    right: 8px;
    bottom: -1px;
    height: 2px;
    border-radius: 2px 2px 0 0;
    background: var(--accent);
  }
  .arsenal-tab:focus-visible {
    outline-offset: -2px;
  }
  .arsenal-pin {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: var(--text-xs);
  }
  .arsenal-member-list {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    overflow-wrap: anywhere;
  }
  .arsenal-filters {
    display: grid;
    gap: 8px 10px;
    grid-template-columns: 1fr;
    margin-block: 4px;
  }
  .arsenal-filters label {
    display: grid;
    gap: 4px;
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--muted);
  }
  /* Fields fill their column; a checkbox or radio keeps its own size. */
  .arsenal-filters input:not([type='checkbox'], [type='radio']),
  .arsenal-filters select {
    width: 100%;
    min-width: 0;
    color: var(--text);
    font-weight: 400;
  }
  .arsenal-count {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .arsenal-list,
  .arsenal-files,
  .arsenal-revisions,
  .arsenal-sources {
    list-style: none;
    padding: 0;
    margin: 4px 0;
    display: grid;
    gap: 6px;
  }
  .arsenal-row {
    width: 100%;
    min-height: 44px;
    text-align: left;
    display: grid;
    gap: 3px;
    padding: 9px 12px 9px 16px;
    border-radius: var(--radius-md);
    border: 1px solid var(--line);
    background: var(--surface);
    color: var(--text);
    cursor: pointer;
  }
  .arsenal-row:hover:not(:disabled) {
    background: var(--surface-hover);
  }
  .arsenal-row[aria-pressed='true'] {
    background: var(--accent-soft);
    border-color: var(--accent-line);
  }
  .arsenal-row-top {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 4px 10px;
    min-width: 0;
  }
  .arsenal-row-name {
    font-size: var(--text-md);
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .arsenal-row-desc {
    font-size: var(--text-sm);
    color: var(--muted);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .arsenal-row-meta {
    font-size: var(--text-xs);
    color: var(--faint);
    overflow-wrap: anywhere;
  }
  /* Wide windows: the list and what is open side by side, so opening one of hundreds
     never sends the reader to the end of the list. */
  @media (min-width: 1100px) {
    .arsenal-panel:has(> .arsenal-detail) {
      display: grid;
      grid-template-columns: minmax(0, 1fr) minmax(340px, 44%);
      column-gap: 16px;
      align-items: start;
    }
    .arsenal-panel:has(> .arsenal-detail) > * {
      grid-column: 1;
    }
    .arsenal-panel > .arsenal-detail {
      grid-column: 2;
      grid-row: 1 / span 40;
      position: sticky;
      top: 0;
      margin-top: 0;
      max-height: calc(100dvh - var(--topbar-h) - 140px);
      overflow-y: auto;
    }
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
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--surface);
    display: grid;
    gap: 10px;
    align-content: start;
  }
  .arsenal-detail h3 {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .arsenal-facts {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: 6px 14px;
    font-size: var(--text-sm);
    margin: 0;
  }
  .arsenal-facts dt {
    color: var(--muted);
  }
  .arsenal-facts dd,
  .arsenal-facts dt {
    margin: 0;
  }
  .arsenal-facts code,
  .arsenal-preview,
  .arsenal-detail code {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
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
    font-size: var(--text-xs);
  }
  .arsenal-empty {
    color: var(--muted);
    font-size: var(--text-md);
  }
  .arsenal-warn {
    color: var(--warn);
    font-size: var(--text-md);
  }
  .arsenal-notice {
    font-size: var(--text-md);
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
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-start;
    gap: 4px 8px !important;
    min-height: 36px;
    color: var(--text) !important;
    font-weight: 500 !important;
    text-align: left;
  }
  .arsenal-status {
    font-size: var(--text-md);
  }
  .arsenal-detail h4 {
    margin: 6px 0 0;
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--muted);
  }
  .arsenal-files li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 8px;
    min-width: 0;
    font-size: var(--text-sm);
  }
  .arsenal-files .action {
    min-height: 30px;
    padding: 0 10px;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    overflow-wrap: anywhere;
    text-align: left;
  }
  /* Filters wrap by the room they have, not by the window: the list column can be narrow. */
  @media (min-width: 768px) {
    .arsenal-filters {
      display: flex;
      flex-wrap: wrap;
      align-items: end;
    }
    .arsenal-filters > * {
      flex: 1 1 140px;
    }
    .arsenal-filters > label:first-child {
      flex: 2 1 240px;
    }
    .arsenal-filters > button {
      flex: 0 0 auto;
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
