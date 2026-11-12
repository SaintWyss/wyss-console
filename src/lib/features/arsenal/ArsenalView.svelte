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
  import { t } from '../../i18n/index.svelte';
  import { msg } from '../../i18n/msg';
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
    { id: 'presets', label: msg('Presets') },
    { id: 'library', label: msg('Library') },
    { id: 'activity', label: msg('Activity') },
  ];
  /** The sections of the Library tab. Discover and Composites are also controller tabs. */
  const LIBRARY_SECTIONS: { id: ArsenalTab; label: string }[] = [
    { id: 'library', label: msg('Imported') },
    { id: 'discover', label: msg('Discover') },
    { id: 'composites', label: msg('Composites') },
  ];
  const PROVIDER_OPTIONS = PROVIDERS;
  const PROVIDER_LABEL: Record<ArsenalProvider, string> = {
    claude_code: 'Claude Code',
    codex: 'Codex',
  };
  /** The provider every tab works on. Chosen above the tabs; the presets draft belongs to one provider. */
  let provider = $state<ArsenalProvider>('claude_code');
  /** The presets tab, when it is shown: a provider switch asks it first, since it holds the draft. */
  let presetsTab = $state<{ requestLeave: (action: () => void) => void }>();
  const KIND_LABEL: Record<CandidateKind, string> = {
    skill: msg('Skill'),
    agent: msg('Agent'),
    rule: msg('Rule'),
    hook: msg('Hook'),
    mcp_server: msg('MCP server'),
    plugin: msg('Plugin'),
  };
  const SCOPE_LABEL: Record<CandidateScope, string> = {
    user: msg('Account'),
    project: msg('Project'),
  };
  const STATUS_LABEL: Record<CandidateStatus, string> = {
    not_imported: msg('Not imported'),
    imported: msg('Imported'),
    source_changed: msg('Source changed'),
    read_only: msg('Read only'),
  };
  const COMPATIBILITY_LABEL: Record<Compatibility, string> = {
    supported: msg('Supported'),
    unsupported: msg('Unsupported'),
    unverified: msg('Unverified'),
    requires_qualification: msg('Needs provider qualification'),
    conflicting: msg('Conflicting'),
    missing_dependency: msg('Missing dependency'),
  };
  const DEPENDENCY_KIND: Record<Dependency['kind'], string> = {
    tool: msg('Tool'),
    mcp_server: msg('MCP server'),
    file: msg('File'),
  };
  const DEPENDENCY_STATE: Record<Dependency['state'], string> = {
    declared: msg('declared, not checked offline'),
    present: msg('present'),
    missing: msg('missing'),
    unknown: msg('unknown'),
  };
  /** Why a read-only kind is listed and never copied. Said once, next to the entry. */
  const READ_ONLY_NOTE: Record<string, string> = {
    hook: msg(
      'A hook runs code. Arsenal lists it and never copies or runs it.',
    ),
    mcp_server: msg(
      'An MCP definition can hold credentials. Arsenal lists it and never copies it.',
    ),
    plugin: msg(
      'A plugin brings code and hooks. Arsenal lists it; importing plugins is not available yet.',
    ),
  };
  const COMPOSITE_KIND_LABEL: Record<CompositeKind, string> = {
    gsd_core: msg('GSD Core payload'),
    claude_plugin: msg('Claude Code plugin'),
    codex_plugin: msg('Codex plugin'),
  };
  const SOURCE_LABEL: Record<string, string> = {
    claude_user_skills: msg('Claude user skills'),
    codex_user_skills: msg('Codex user skills'),
    codex_home_skills: msg('Codex home skills'),
    codex_system_skills: msg('Codex system skills'),
    claude_project_skills: msg('Claude project skills'),
    codex_project_skills: msg('Codex project skills'),
    claude_user_agents: msg('Claude user agents'),
    codex_user_agents: msg('Codex user agents'),
    claude_project_agents: msg('Claude project agents'),
    codex_project_agents: msg('Codex project agents'),
    claude_user_rules: msg('Claude user rules'),
    codex_user_rules: msg('Codex user rules'),
    claude_project_rules: msg('Claude project rules'),
    codex_project_rules: msg('Codex project rules'),
  };
  const EXCLUDED_LABEL: Record<string, string> = {
    credential_name: msg('Credential file, not copied'),
    hidden_entry: msg('Hidden, not copied'),
    symbolic_link: msg('Link, not followed'),
    binary_file: msg('Binary, not copied'),
    may_hold_secret: msg('May hold a secret'),
    special_file: msg('Special file, not copied'),
    too_deep: msg('Nested too deep, not copied'),
    non_utf8_name: msg('Name is not UTF-8, not copied'),
  };
  const SOURCE_STATE_LABEL: Record<string, string> = {
    read: msg('read'),
    absent: msg('not present'),
    unreadable: msg('could not be read'),
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
  const inLibrary = $derived(
    arsenal.tab === 'library' ||
      arsenal.tab === 'discover' ||
      arsenal.tab === 'composites',
  );
  /** The tab the top bar shows as selected: Discover and Composites belong to Library. */
  const activeTop = $derived<ArsenalTab>(inLibrary ? 'library' : arsenal.tab);
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

  /**
   * Whether the last scan read a source of this provider. `null` until a scan has answered, so
   * the switch stays enabled rather than claim anything the window has not seen.
   */
  function installed(option: ArsenalProvider): boolean | null {
    const sources = arsenal.discover?.sources;
    if (!sources) return null;
    const prefix = option === 'claude_code' ? 'claude_' : 'codex_';
    return sources.some(
      (entry) => entry.source.startsWith(prefix) && entry.state === 'read',
    );
  }
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
  function excludedText(reason: string): string {
    return EXCLUDED_LABEL[reason] ?? reason;
  }
  function importLabel(candidate: Candidate): string {
    if (arsenal.busy === 'import') return msg('Importing…');
    if (candidate.status === 'imported') return msg('Already imported');
    if (candidate.status === 'source_changed')
      return msg('Import as new revision');
    return msg('Import');
  }
  /** Switches the provider. With unsaved changes in the presets draft, the switch waits for Discard changes. */
  function chooseProvider(option: ArsenalProvider) {
    if (option === provider) return;
    const change = () => {
      provider = option;
    };
    if (presetsTab) presetsTab.requestLeave(change);
    else change();
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
  <div class="arsenal-head">
    <h2>Arsenal</h2>
    <div class="arsenal-provider" role="radiogroup" aria-label={t('Provider')}>
      {#each PROVIDER_OPTIONS as option (option)}
        <button
          class="action arsenal-provider-option"
          type="button"
          role="radio"
          aria-checked={provider === option}
          tabindex={provider === option ? 0 : -1}
          disabled={installed(option) === false}
          onclick={() => chooseProvider(option)}
          >{PROVIDER_LABEL[option]}{#if installed(option) === false}
            <span class="arsenal-provider-note">{t('Not installed')}</span
            >{/if}</button
        >
      {/each}
    </div>
    <button
      class="action arsenal-refresh"
      type="button"
      disabled={arsenal.phase === 'loading' || busy}
      onclick={() => controller.refresh()}>{t('Refresh')}</button
    >
  </div>
  <div class="arsenal-tabs" role="tablist" aria-label={t('Arsenal sections')}>
    {#each TABS as tab, index (tab.id)}
      <button
        id="arsenal-tab-{tab.id}"
        class="action arsenal-tab"
        type="button"
        role="tab"
        aria-selected={activeTop === tab.id}
        aria-controls="arsenal-panel-{tab.id}"
        tabindex={activeTop === tab.id ? 0 : -1}
        onclick={() => controller.setTab(tab.id)}
        onkeydown={(event) => onTab(event, index)}>{t(tab.label)}</button
      >
    {/each}
    <p class="arsenal-scope-note">
      {t(
        'Installed does not mean imported. A selection reaches a terminal only when you start one with it from Workspace.',
      )}
    </p>
  </div>

  <div class="arsenal-body">
    {#if arsenal.notice}
      <p
        class="arsenal-notice {arsenal.notice.tone}"
        role={arsenal.notice.tone === 'error' ? 'alert' : 'status'}
        aria-live={arsenal.notice.tone === 'error' ? 'assertive' : 'polite'}
      >
        {t(arsenal.notice.text)}
      </p>
    {/if}

    {#if arsenal.phase === 'loading' || arsenal.phase === 'idle'}
      <p role="status">{t('Reading the Arsenal library…')}</p>
    {:else if arsenal.phase === 'unavailable'}
      <p role="alert">
        {t(
          'The Arsenal library could not be opened. Refresh after the Wyss data folder is writable.',
        )}
      </p>
    {:else if arsenal.phase === 'failed'}
      <p role="alert">
        {t('The library did not load completely. Try Refresh.')}
      </p>
    {/if}

    {#if inLibrary}
      <div
        class="arsenal-subtabs"
        role="group"
        aria-label={t('Library sections')}
      >
        {#each LIBRARY_SECTIONS as section (section.id)}
          <button
            id="arsenal-section-{section.id}"
            class="action"
            type="button"
            aria-current={arsenal.tab === section.id ? 'true' : undefined}
            onclick={() => controller.setTab(section.id)}
            >{t(section.label)}</button
          >
        {/each}
      </div>
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
            >{t('Search')}
            <input
              type="search"
              bind:value={libraryQuery}
              placeholder={t('Name, description or origin')}
            /></label
          >
          <label
            >{t('Provider')}
            <select bind:value={libraryProvider}>
              <option value="all">{t('All')}</option>
              <option value="claude_code">Claude Code</option>
              <option value="codex">Codex</option>
            </select>
          </label>
          <label
            >{t('Source')}
            <select bind:value={librarySource}>
              <option value="all">{t('All')}</option>
              {#each sources as source (source)}
                <option value={source}>{SOURCE_LABEL[source] ?? source}</option>
              {/each}
            </select>
          </label>
          <label
            >{t('Compatibility')}
            <select bind:value={libraryCompat}>
              <option value="all">{t('All')}</option>
              <option value="supported">{t('Supported')}</option>
              <option value="unverified">{t('Unverified')}</option>
              <option value="missing_dependency"
                >{t('Missing dependency')}</option
              >
              <option value="unsupported">{t('Unsupported')}</option>
            </select>
          </label>
        </div>

        {#if arsenal.library.length === 0 && arsenal.phase === 'ready'}
          <p class="arsenal-empty">
            {t(
              'Nothing is imported yet. Open Discover to import a capability you already have.',
            )}
          </p>
        {:else if visibleLibrary.length === 0 && arsenal.library.length > 0}
          <p class="arsenal-empty">
            {t('No capabilities match these filters.')}
          </p>
        {:else if arsenal.library.length > 0}
          <p class="arsenal-count" aria-live="polite">
            {visibleLibrary.length === arsenal.library.length
              ? t('{count} imported', { count: arsenal.library.length })
              : t('{shown} of {total} imported', {
                  shown: visibleLibrary.length,
                  total: arsenal.library.length,
                })}
          </p>
        {/if}

        <ul class="arsenal-list" aria-label={t('Imported capabilities')}>
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
                    {t(COMPATIBILITY_LABEL[libraryCompatibility(item)])}</span
                  >
                </span>
                {#if item.description}
                  <span class="arsenal-row-desc">{item.description}</span>
                {/if}
                <span class="arsenal-row-meta"
                  >{t(kindOf(item))} · {PROVIDER_LABEL[item.provider]} · {t(
                    SOURCE_LABEL[item.source] ?? item.source,
                  )} · {t('Revision {revision}', {
                    revision: item.revision,
                  })}</span
                >
              </button>
            </li>
          {/each}
        </ul>

        {#if arsenal.opened}
          {@const opened = arsenal.opened}
          <section class="arsenal-detail" aria-label={t('Capability details')}>
            <h3>{opened.summary.name}</h3>
            <p class="arsenal-muted">
              {t(
                KIND_LABEL[opened.summary.kind as CandidateKind] ??
                  opened.summary.kind,
              )} · {PROVIDER_LABEL[opened.summary.provider]} ·
              {t(SOURCE_LABEL[opened.summary.source] ?? opened.summary.source)} ·
              {t('Revision {current} of {total}', {
                current: opened.summary.revision,
                total: opened.summary.revisions,
              })}
            </p>
            {#if opened.summary.description}<p>
                {opened.summary.description}
              </p>{/if}
            <dl class="arsenal-facts">
              <dt>{t('Imported from')}</dt>
              <dd><code>{opened.summary.origin}</code></dd>
              <dt>{t('Content')}</dt>
              <dd><code>{opened.summary.tree_hash.slice(0, 16)}</code></dd>
              <dt>{t('Compatibility')}</dt>
              <dd>
                <span
                  class="badge"
                  data-tone={compatibilityMark(opened.compatibility.status)
                    .tone}
                  ><span aria-hidden="true"
                    >{compatibilityMark(opened.compatibility.status)
                      .symbol}</span
                  >
                  {t(COMPATIBILITY_LABEL[opened.compatibility.status])}</span
                >
                {opened.compatibility.reason}
              </dd>
              <dt>{t('Use in a session')}</dt>
              <dd>
                {t(
                  'Only through a terminal plan: Workspace shows, per terminal, whether this revision is installed and whether it can be applied.',
                )}
              </dd>
            </dl>
            <h4>{t('Dependencies')}</h4>
            {#if opened.dependencies.length === 0}
              <p class="arsenal-muted">
                {t('Its files declare and point at nothing Arsenal can check.')}
              </p>
            {:else}
              <ul class="arsenal-files" aria-label={t('Dependencies')}>
                {#each opened.dependencies as dep (dep.kind + dep.name)}
                  <li>
                    <span
                      >{t(DEPENDENCY_KIND[dep.kind])}:
                      <code>{dep.name}</code></span
                    >
                    <span
                      class={dep.state === 'missing'
                        ? 'arsenal-warn'
                        : 'arsenal-muted'}
                      >{t(DEPENDENCY_STATE[dep.state])}</span
                    >
                  </li>
                {/each}
              </ul>
            {/if}

            <h4>
              {t('Files in revision {revision}', {
                revision: opened.revision,
              })}
            </h4>
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

            <h4>{t('Revisions')}</h4>
            <ul class="arsenal-revisions">
              {#each opened.revisions as revision (revision.number)}
                <li>
                  <span
                    >{t('Revision {revision}', {
                      revision: revision.number,
                    })}</span
                  >
                  <span class="arsenal-muted"
                    >{t('{files} files · {bytes} B', {
                      files: revision.files,
                      bytes: revision.bytes,
                    })}</span
                  >
                  {#if revision.current}
                    <span class="pill">{t('Current')}</span>
                  {:else}
                    <button
                      class="action"
                      type="button"
                      disabled={busy}
                      onclick={() => controller.restore(revision.number)}
                    >
                      {arsenal.busy === 'restore'
                        ? t('Restoring…')
                        : t('Restore revision {revision}', {
                            revision: revision.number,
                          })}
                    </button>
                  {/if}
                  <button
                    class="action"
                    type="button"
                    onclick={() =>
                      controller.open(opened.summary.id, revision.number)}
                  >
                    {t('Show files of revision {revision}', {
                      revision: revision.number,
                    })}
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
        aria-labelledby="arsenal-section-discover"
        class="arsenal-panel"
      >
        <div class="arsenal-filters">
          <label
            >{t('Search')}
            <input
              type="search"
              bind:value={discoverQuery}
              placeholder={t('Name, description or origin')}
            /></label
          >
          <label
            >{t('Provider')}
            <select bind:value={discoverProvider}>
              <option value="all">{t('All')}</option>
              <option value="claude_code">Claude Code</option>
              <option value="codex">Codex</option>
            </select>
          </label>
          <label
            >{t('Kind')}
            <select bind:value={discoverKind}>
              <option value="all">{t('All')}</option>
              <option value="skill">{t('Skills')}</option>
              <option value="agent">{t('Agents')}</option>
              <option value="rule">{t('Rules')}</option>
              <option value="mcp_server">{t('MCP servers')}</option>
              <option value="hook">{t('Hooks')}</option>
              <option value="plugin">{t('Plugins')}</option>
            </select>
          </label>
          <label
            >{t('Where')}
            <select bind:value={discoverScope}>
              <option value="all">{t('Account and project')}</option>
              <option value="user">{t('Account')}</option>
              <option value="project">{t('Project')}</option>
            </select>
          </label>
          <label
            >{t('Status')}
            <select bind:value={discoverStatus}>
              <option value="all">{t('All')}</option>
              <option value="not_imported">{t('Not imported')}</option>
              <option value="imported">{t('Imported')}</option>
              <option value="source_changed">{t('Source changed')}</option>
              <option value="read_only">{t('Read only')}</option>
            </select>
          </label>
          <button
            class="action"
            type="button"
            disabled={arsenal.discovering || busy}
            onclick={() => controller.discover()}
          >
            {arsenal.discovering ? t('Reading…') : t('Scan again')}
          </button>
          <button
            class="action"
            type="button"
            disabled={busy || arsenal.discovering || eligibleCount === 0}
            onclick={() => controller.importEligible()}
          >
            {busy && arsenal.bulk && !arsenal.bulk.finished
              ? t('Importing {done} of {total}…', {
                  done: arsenal.bulk.done,
                  total: arsenal.bulk.total,
                })
              : t('Import {count} eligible', { count: eligibleCount })}
          </button>
        </div>

        {#if arsenal.bulk && !arsenal.bulk.finished}
          <progress
            max={arsenal.bulk.total}
            value={arsenal.bulk.done}
            aria-label={t('Bulk import progress')}
          ></progress>
        {/if}

        {#if arsenal.discovering && arsenal.discover === null}
          <p role="status">{t('Reading installed capabilities…')}</p>
        {:else if arsenal.discover}
          <p class="arsenal-muted">
            {#if arsenal.discover.project}
              {t('Project: {project}', { project: arsenal.discover.project })}
            {:else}
              {t(
                'No project selected. Project capabilities are not listed; select a project in Configuration.',
              )}
            {/if}
          </p>
          <ul class="arsenal-sources" aria-label={t('Capability folders')}>
            {#each arsenal.discover.sources as source (source.source)}
              <li>
                {t(SOURCE_LABEL[source.source] ?? source.source)}: {t(
                  SOURCE_STATE_LABEL[source.state] ?? source.state,
                )}
              </li>
            {/each}
          </ul>
          {#if arsenal.discover.truncated}
            <p role="alert">
              {t(
                'The folders have more entries than the scan shows. Some are not listed.',
              )}
            </p>
          {/if}

          {#if arsenal.discover.composites.length > 0}
            <p class="arsenal-muted">
              {t('{count} composite collection(s) installed.', {
                count: arsenal.discover.composites.length,
              })}
              <button
                class="action"
                type="button"
                onclick={() => controller.setTab('composites')}
                >{t('Open Composites')}</button
              >
            </p>
          {/if}

          {#if arsenal.discover.candidates.length === 0}
            <p class="arsenal-empty">
              {t(
                'No capabilities found in your Claude, Codex or project folders.',
              )}
            </p>
          {:else if candidates.length === 0}
            <p class="arsenal-empty">
              {t('No capabilities match these filters.')}
            </p>
          {/if}

          <ul class="arsenal-list" aria-label={t('Discovered capabilities')}>
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
                        {t(STATUS_LABEL[candidate.status])}</span
                      >
                      {#if blocked}<span class="badge" data-tone="error"
                          ><span aria-hidden="true">✕</span>
                          {t('Blocked')}</span
                        >{/if}
                      {#if candidate.duplicates.length > 0}<span
                          class="badge"
                          data-tone="warn"
                          ><span aria-hidden="true">!</span>
                          {t('Duplicate content')}</span
                        >{/if}
                      {#if candidate.name_collisions.length > 0}<span
                          class="badge"
                          data-tone="warn"
                          ><span aria-hidden="true">!</span>
                          {t('Name collision')}</span
                        >{/if}
                    </span>
                  </span>
                  {#if candidate.description}
                    <span class="arsenal-row-desc">{candidate.description}</span
                    >
                  {/if}
                  <span class="arsenal-row-meta"
                    >{t(KIND_LABEL[candidate.kind])} · {PROVIDER_LABEL[
                      candidate.provider
                    ]} · {t(
                      SCOPE_LABEL[candidate.scope],
                    )}{#if candidate.managed_by}
                      · {candidate.managed_by.composite}
                      {candidate.managed_by.version}{/if}</span
                  >
                </button>
              </li>
            {/each}
          </ul>

          {#if chosen}
            <section
              class="arsenal-detail"
              aria-label={t('Capability to import')}
            >
              <h3>{chosen.name}</h3>
              <p class="arsenal-muted">
                {t(KIND_LABEL[chosen.kind])} · {PROVIDER_LABEL[chosen.provider]} ·
                {t(SOURCE_LABEL[chosen.source] ?? chosen.source)} · {t(
                  SCOPE_LABEL[chosen.scope],
                )} · {t(STATUS_LABEL[chosen.status])}
              </p>
              {#if chosen.description}<p>{chosen.description}</p>{/if}
              <dl class="arsenal-facts">
                <dt>{t('Read from')}</dt>
                <dd><code>{chosen.origin}</code></dd>
                {#if chosen.tree_hash}
                  <dt>{t('Content')}</dt>
                  <dd><code>{chosen.tree_hash.slice(0, 16)}</code></dd>
                {/if}
                <dt>{t('Compatibility')}</dt>
                <dd>{t(COMPATIBILITY_LABEL[chosen.compatibility])}</dd>
              </dl>

              {#if chosen.status === 'read_only'}
                <p class="arsenal-muted">
                  {t(
                    READ_ONLY_NOTE[chosen.kind] ??
                      'Listed only. Arsenal does not copy it.',
                  )}
                </p>
              {:else}
                {#if chosen.executes_code}
                  <p class="arsenal-warn">
                    {t(
                      'Contains scripts. Arsenal copies them and never runs them.',
                    )}
                  </p>
                {/if}
                {#if chosen.duplicates.length > 0}
                  <p>
                    {t('Same content as {names}.', {
                      names: chosen.duplicates.map(nameOfCandidate).join(', '),
                    })}
                  </p>
                {/if}
                {#if chosen.name_collisions.length > 0}
                  <p>
                    {t(
                      'Another capability here has the same name. Both can be imported; they stay separate.',
                    )}
                  </p>
                {/if}

                <h4>
                  {chosen.importable
                    ? t('Files that will be copied')
                    : t('Files in this capability')}
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
                  <h4>{t('Not copied')}</h4>
                  <ul class="arsenal-files">
                    {#each detail?.excluded ?? [] as entry (entry.path)}
                      <li>
                        <span>{entry.path}</span>
                        <span class="arsenal-muted"
                          >{t(excludedText(entry.reason))}</span
                        >
                      </li>
                    {/each}
                  </ul>
                {/if}
                {#if (detail?.review.length ?? 0) > 0}
                  <h4>{t('Review before importing')}</h4>
                  <p class="arsenal-muted">
                    {t(
                      'These files use a credential word. They are copied; check that none holds a real value.',
                    )}
                  </p>
                  <ul class="arsenal-files">
                    {#each detail?.review ?? [] as entry (entry.path)}
                      <li>
                        <span>{entry.path}</span>
                        <span class="arsenal-muted"
                          >{t('Mentions a credential word')}</span
                        >
                      </li>
                    {/each}
                  </ul>
                {/if}

                {#if chosen.managed_by}
                  <p role="alert">
                    {t(
                      'This is a link into {composite} {version}. It is read only: GSD Core installs it, so Arsenal does not copy it.',
                      {
                        composite: chosen.managed_by.composite,
                        version: chosen.managed_by.version,
                      },
                    )}
                  </p>
                {:else if chosen.blocked}
                  <p role="alert">{t(blockedText(chosen.blocked))}</p>
                {/if}
                <button
                  class="action"
                  type="button"
                  disabled={busy ||
                    !chosen.importable ||
                    chosen.status === 'imported'}
                  onclick={() => controller.importCandidate(chosen.id)}
                >
                  {t(importLabel(chosen))}
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
        aria-labelledby="arsenal-section-composites"
        class="arsenal-panel"
      >
        <p class="configuration-note">
          {t(
            'A composite is installed by its own tool: GSD Core by its installer, a plugin by Claude Code or Codex. Arsenal describes it and never copies it.',
          )}
        </p>
        {#if arsenal.discovering && arsenal.discover === null}
          <p role="status">{t('Reading installed composites…')}</p>
        {:else if arsenal.discover && arsenal.discover.composites.length === 0}
          <p class="arsenal-empty">
            {t(
              'No composite collection is installed in your Claude or Codex folders.',
            )}
          </p>
        {/if}
        {#each arsenal.discover?.composites ?? [] as composite (composite.key)}
          <section class="arsenal-detail" aria-label={composite.name}>
            <h3>{composite.name}</h3>
            <p class="arsenal-badges">
              <span class="pill">{t(COMPOSITE_KIND_LABEL[composite.kind])}</span
              >
              {#if composite.version}<span class="pill"
                  >{t('Version {version}', {
                    version: composite.version,
                  })}</span
                >{/if}
              {#if composite.enabled !== null}<span class="pill"
                  >{composite.enabled
                    ? t('Enabled in your settings')
                    : t('Disabled in your settings')}</span
                >{/if}
              {#if composite.methodology}<span class="pill"
                  >{t('Methodology')}</span
                >{/if}
              <span class="pill">{t('Not importable')}</span>
            </p>
            {#if composite.description}<p>{composite.description}</p>{/if}
            <dl class="arsenal-facts">
              <dt>{t('Source')}</dt>
              <dd><code>{composite.origin}</code></dd>
              <dt>{t('Members')}</dt>
              <dd>
                {t('{skills} skills · {agents} agents', {
                  skills: composite.skills.length,
                  agents: composite.agents.length,
                })}
                {#if composite.commands > 0}· {t('{count} commands', {
                    count: composite.commands,
                  })}{/if}
                {#if composite.artifacts > 0}· {t(
                    '{count} artifacts in its manifest',
                    { count: composite.artifacts },
                  )}{/if}
              </dd>
              <dt>{t('Runs code')}</dt>
              <dd>
                {composite.hooks
                  ? t('Yes: it registers hooks.')
                  : composite.kind === 'gsd_core'
                    ? t('Its hooks and engine belong to the GSD Core install.')
                    : t('No hooks declared.')}
                {composite.mcp ? ` ${t('It declares MCP servers.')}` : ''}
              </dd>
            </dl>
            {#if composite.categories.length > 0}
              <h4>{t('Manifest')}</h4>
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
            <h4>{t('Limits')}</h4>
            <ul class="arsenal-files">
              {#each composite.limitations as line (line)}<li>{line}</li>{/each}
            </ul>
            {#if composite.skills.length > 0}
              <details>
                <summary
                  >{t('Skills ({count})', {
                    count: composite.skills.length,
                  })}</summary
                >
                <p class="arsenal-member-list">{composite.skills.join(', ')}</p>
              </details>
            {/if}
            {#if composite.agents.length > 0}
              <details>
                <summary
                  >{t('Agents ({count})', {
                    count: composite.agents.length,
                  })}</summary
                >
                <p class="arsenal-member-list">{composite.agents.join(', ')}</p>
              </details>
            {/if}
          </section>
        {/each}
      </div>
    {/if}

    {#if arsenal.tab === 'activity'}
      <div
        id="arsenal-panel-activity"
        role="tabpanel"
        aria-labelledby="arsenal-tab-activity"
        class="arsenal-panel"
      >
        {#if arsenal.sync?.enabled}
          <SyncLine
            sync={arsenal.sync}
            onsyncnow={() => controller.syncNow()}
          />
        {/if}
        <p class="configuration-note">
          {t(
            "What each terminal started with, recorded right after its start from the host's own session record. Applied means the host launched it with that exact exclusion; whether the provider then used each skill is not observed.",
          )}
        </p>
        <button
          class="action"
          type="button"
          onclick={() => controller.loadBindings()}>{t('Refresh')}</button
        >
        {#if arsenal.bindings.length === 0}
          <p class="arsenal-empty">
            {t(
              'No terminal has started with a selection yet. Choose one in Workspace when starting Claude Code or Codex.',
            )}
          </p>
        {/if}
        <ul class="arsenal-list" aria-label={t('Terminal configurations')}>
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
                    ? t('APPLIED')
                    : t('NOT APPLIED')}</span
                >
                <span class="badge" data-tone="pending"
                  ><span aria-hidden="true">?</span>
                  {t('Use not observed')}</span
                >
                <span class="pill">{PROVIDER_LABEL[binding.provider]}</span>
                {#if binding.provider_version}<span class="pill"
                    >{binding.provider_version}</span
                  >{/if}
              </p>
              <p>{binding.reason}</p>
              <p class="arsenal-muted">
                <code>{binding.project}</code> · {when(binding.created_at)} · {t(
                  'session',
                )}
                <code>{binding.session_id.slice(0, 8)}</code>
              </p>
              {#if binding.plan}
                <p class="arsenal-muted">
                  {binding.state === 'applied'
                    ? t(
                        '{prepared} prepared · {excluded} installed skills switched off · {kept} kept',
                        {
                          prepared: binding.plan.items.filter(
                            (i) => i.state === 'prepared',
                          ).length,
                          excluded: binding.plan.excluded.length,
                          kept: binding.plan.kept.length,
                        },
                      )
                    : t(
                        '{prepared} prepared · {excluded} installed skills would have been switched off · {kept} kept',
                        {
                          prepared: binding.plan.items.filter(
                            (i) => i.state === 'prepared',
                          ).length,
                          excluded: binding.plan.excluded.length,
                          kept: binding.plan.kept.length,
                        },
                      )}
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
        {#key provider}
          <PresetsTab
            bind:this={presetsTab}
            {provider}
            {arsenal}
            {controller}
          />
        {/key}
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
  /* One header row: title, provider, Refresh. The page starts working at the second row. */
  .arsenal-head {
    display: flex;
    align-items: center;
    gap: 12px 16px;
    flex-wrap: wrap;
  }
  .arsenal-head h2 {
    margin: 0;
  }
  .arsenal-refresh {
    margin-left: auto;
  }
  .arsenal-tabs {
    display: flex;
    align-items: flex-end;
    gap: 2px;
    flex-wrap: wrap;
    margin-top: 8px;
    border-bottom: 1px solid var(--line);
  }
  .arsenal-scope-note {
    flex: 1 1 280px;
    margin: 0 0 8px 12px;
    color: var(--muted);
    font-size: var(--text-xs);
    text-align: right;
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
  .arsenal-provider,
  .arsenal-subtabs {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .arsenal-subtabs {
    margin: 8px 0;
  }
  .arsenal-provider-option[aria-checked='true'],
  .arsenal-subtabs [aria-current='true'] {
    background: var(--accent-soft);
    font-weight: 600;
  }
  .arsenal-provider-option:disabled {
    color: var(--muted);
  }
  .arsenal-provider-note {
    display: block;
    font-size: var(--text-xs);
    font-weight: 400;
    color: var(--muted);
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
