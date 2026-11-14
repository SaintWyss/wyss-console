<script lang="ts">
  import { msg, t } from '../../i18n/index.svelte';
  import type { Item, Selection } from '../../contracts/arsenal';
  import type { Profile } from '../../contracts/native';
  import {
    arsenalClient,
    type ArsenalClient,
    type Roles,
  } from '../../desktop/arsenal-client';
  import {
    deleteLaunch,
    listLaunches,
    saveLaunch,
    LaunchError,
    type Launch,
  } from '../../desktop/launch-client';
  import { DEFAULT_SPEC, hookLabel, toggle } from '../arsenal/environment';
  import { kTokens } from './estimate';
  import {
    loadOrder,
    renameInOrder,
    saveOrder,
  } from '../terminal/launch-order';
  import {
    allItemsOf,
    estimateLaunch,
    estimateOf,
    libraryItems,
    loadEstimateData,
    NO_DATA,
    selectionOf,
    type EstimateData,
  } from './data';
  import {
    editable,
    launchId,
    savePlan,
    type Agent,
    type LaunchDraft,
  } from './plan';

  let {
    visible,
    profiles,
    recent,
    choose,
    onopen,
    client = arsenalClient,
    launches: loadLaunches = listLaunches,
  }: {
    visible: boolean;
    profiles: readonly Profile[];
    /** Folders the Console knows, newest first (the project registry). */
    recent: readonly { path: string; last_used: number }[];
    /** The native folder dialog; `null` when dismissed. */
    choose: () => Promise<string | null>;
    /** Opens a saved launch in the Console. */
    onopen: (id: string) => void;
    client?: ArsenalClient;
    launches?: typeof listLaunches;
  } = $props();

  const BASIC: readonly { id: string; name: string }[] = [
    { id: 'orchestrator', name: msg('Orchestrator') },
    { id: 'worker', name: msg('Worker') },
    { id: 'researcher', name: msg('Researcher') },
    { id: 'reviewer', name: msg('Reviewer') },
  ];
  const AGENTS: readonly { id: Agent; distributor: string }[] = [
    { id: 'claude_code', distributor: 'Anthropic' },
    { id: 'codex', distributor: 'OpenAI' },
  ];

  let phase = $state<'idle' | 'loading' | 'ready' | 'failed'>('idle');
  let launches = $state<Launch[]>([]);
  let roles = $state<Roles>({ custom: [], assigned: {}, damaged: false });
  let data = $state<EstimateData>(NO_DATA);
  let q = $state('');
  let selected = $state<string | null>(null);
  let draft = $state<LaunchDraft | null>(null);
  let baseline = $state('');
  let open = $state<string | null>('skills');
  let busy = $state(false);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let confirming = $state(false);
  let rolesOpen = $state(false);
  let newRole = $state('');
  let foldersOpen = $state(false);

  const allRoles = $derived([
    ...BASIC.map((r) => ({ id: r.id, name: t(r.name), basic: true })),
    ...roles.custom.map((r) => ({ ...r, basic: false })),
  ]);
  const roleOf = (id: string) => roles.assigned[id] ?? 'worker';
  const knownRole = (id: string) =>
    allRoles.some((r) => r.id === id) ? id : 'worker';

  async function load() {
    phase = 'loading';
    error = null;
    try {
      const [list, r, read] = await Promise.all([
        loadLaunches(),
        // Roles that cannot be read leave the basic ones; the editor still opens.
        client
          .roles()
          .catch((): Roles => ({ custom: [], assigned: {}, damaged: true })),
        loadEstimateData(client),
      ]);
      launches = list.launches;
      roles = r;
      data = read;
      phase = 'ready';
      if (!selected && launches[0]) pick(launches[0]);
    } catch (e) {
      phase = 'failed';
      error =
        e instanceof Error ? e.message : t('Workspace could not be read.');
    }
  }
  $effect(() => {
    if (visible && phase === 'idle') void load();
  });

  function draftOf(launch: Launch | null): LaunchDraft {
    if (!launch || launch.kind !== 'session') {
      const agent: Agent = 'claude_code';
      return {
        id: launch?.id ?? null,
        name: launch?.name ?? t('New launch'),
        role: launch ? knownRole(roleOf(launch.id)) : 'worker',
        agent,
        profile: null,
        directory: launch?.directory ?? recent[0]?.path ?? '',
        selection: null,
        items: allItemsOf(data.library, agent),
        composites: [],
        spec: { ...DEFAULT_SPEC },
      };
    }
    const selection = selectionOf(launch, data.selections);
    return {
      id: launch.id,
      name: launch.name,
      role: knownRole(roleOf(launch.id)),
      agent: launch.agent,
      profile: launch.profile,
      directory: launch.directory,
      selection,
      items: selection
        ? selection.items.map((i) => ({ ...i }))
        : allItemsOf(data.library, launch.agent),
      composites: selection ? [...selection.composites] : [],
      spec:
        launch.agent === 'claude_code'
          ? {
              ...((selection && data.environments.get(selection.id)) ??
                DEFAULT_SPEC),
            }
          : null,
    };
  }
  function pick(launch: Launch | null) {
    selected = launch?.id ?? 'new';
    draft = draftOf(launch);
    baseline = JSON.stringify(draft);
    error = null;
    notice = null;
    confirming = false;
  }
  const dirty = $derived(draft !== null && JSON.stringify(draft) !== baseline);
  const current = $derived(launches.find((l) => l.id === selected) ?? null);
  /** A launch that runs commands or `before` lines: opened from here, edited in its file. */
  const readOnly = $derived(current !== null && !editable(current));
  /** Saving writes the file its name gives; another file name is removed after it. */
  const renames = $derived(
    current !== null &&
      !readOnly &&
      draft !== null &&
      launchId(draft.name) !== '' &&
      launchId(draft.name) !== current.id,
  );
  const canSave = $derived(
    draft !== null &&
      (current === null || editable(current)) &&
      draft.name.trim() !== '' &&
      launchId(draft.name) !== '' &&
      draft.directory.startsWith('/'),
  );

  // The list, by role, filtered by the search.
  const groups = $derived(
    allRoles
      .map((role) => ({
        role,
        items: launches.filter(
          (l) =>
            knownRole(roleOf(l.id)) === role.id &&
            l.name.toLowerCase().includes(q.trim().toLowerCase()),
        ),
      }))
      .filter((g) => g.items.length > 0),
  );

  const est = $derived(
    draft && !readOnly
      ? estimateOf(draft.agent, draft.items, draft.spec, data)
      : null,
  );
  const ceiling = $derived(
    draft
      ? estimateOf(
          draft.agent,
          allItemsOf(data.library, draft.agent),
          DEFAULT_SPEC,
          data,
        )
      : null,
  );
  const agentProfiles = $derived(
    draft
      ? profiles.filter((p) => p.body.applies_to.includes(draft!.agent))
      : [],
  );
  // Only folders the Console knows as projects: the desktop reads no other.
  const known = $derived(new Set(recent.map((r) => r.path)));
  const saved = $derived([
    ...new Set(launches.map((l) => l.directory).filter((d) => known.has(d))),
  ]);
  const used = $derived(
    [...recent].sort((a, b) => b.last_used - a.last_used).slice(0, 8),
  );

  function setAgent(agent: Agent) {
    if (!draft || draft.agent === agent) return;
    // Profile, preset and environment belong to an agent: they start over.
    draft.agent = agent;
    draft.profile = null;
    draft.selection = null;
    draft.items = allItemsOf(data.library, agent);
    draft.composites = [];
    draft.spec = agent === 'claude_code' ? { ...DEFAULT_SPEC } : null;
  }
  function setItem(id: string, on: boolean) {
    if (!draft) return;
    const has = draft.items.some((i) => i.capability === id);
    if (on && !has)
      draft.items = [...draft.items, { capability: id, revision: null }];
    if (!on && has)
      draft.items = draft.items.filter((i) => i.capability !== id);
  }

  // The groups of what a session loads, one row each, as the contract lists them.
  interface Row {
    key: string;
    label: string;
    on: boolean;
    set: (on: boolean) => void;
    disabled?: boolean;
  }
  interface Group {
    id: string;
    label: string;
    rows: Row[];
    tokens: number | null;
    note?: string;
  }
  const groupsOf = $derived.by((): Group[] => {
    if (!draft) return [];
    const d = draft;
    const lib = libraryItems(data.library, d.agent);
    const on = new Set(d.items.map((i) => i.capability));
    const libRows = (kind: string): Row[] =>
      lib
        .filter((item) => item.kind === kind)
        .sort((a, b) => a.name.localeCompare(b.name))
        .map((item) => ({
          key: item.id,
          label: item.name,
          on: on.has(item.id),
          set: (v: boolean) => setItem(item.id, v),
        }));
    const out: Group[] = [];
    const options = data.options;
    if (d.agent === 'claude_code' && d.spec && options) {
      const spec = d.spec;
      out.push({
        id: 'instructions',
        label: msg('Instructions'),
        tokens: est?.instructions ?? null,
        rows: [
          ...(options.instructions
            ? [
                {
                  key: 'CLAUDE.md',
                  label: 'CLAUDE.md',
                  on: spec.instructions,
                  set: (v: boolean) => (spec.instructions = v),
                },
              ]
            : []),
          ...options.rules.map((rule) => ({
            key: `rule:${rule}`,
            label: `rules/${rule}`,
            on: spec.instructions && !spec.rules_off.includes(rule),
            disabled: !spec.instructions,
            set: (v: boolean) =>
              (spec.rules_off = toggle(spec.rules_off, rule, !v)),
          })),
        ],
      });
    }
    out.push({
      id: 'skills',
      label: msg('Skills'),
      tokens: est?.skills ?? null,
      rows: libRows('skill'),
    });
    if (d.agent === 'claude_code' && d.spec && options) {
      const spec = d.spec;
      out.push({
        id: 'mcp',
        label: msg('MCP servers'),
        tokens: null,
        note: msg('not measured'),
        rows: options.mcp.map((name) => ({
          key: name,
          label: name,
          on: spec.mcp === null || spec.mcp.includes(name),
          set: (v: boolean) =>
            (spec.mcp = toggle(spec.mcp ?? [...options.mcp], name, v)),
        })),
      });
    }
    out.push({
      id: 'agents',
      label: msg('Agents'),
      tokens: est?.agents ?? null,
      rows: libRows('agent'),
    });
    if (d.agent === 'claude_code' && d.spec && options) {
      const spec = d.spec;
      out.push({
        id: 'account',
        label: msg('Account skills'),
        tokens: est?.account ?? null,
        rows: options.account_skills.map((name) => ({
          key: name,
          label: name,
          on: !spec.account_skills_off.includes(name),
          set: (v: boolean) =>
            (spec.account_skills_off = toggle(
              spec.account_skills_off,
              name,
              !v,
            )),
        })),
      });
      out.push({
        id: 'hooks',
        label: msg('Hooks'),
        tokens: 0,
        rows: options.hooks.map((hook) => ({
          key: hook.key,
          label: hookLabel(hook),
          on: !spec.hooks_off.includes(hook.key),
          set: (v: boolean) =>
            (spec.hooks_off = toggle(spec.hooks_off, hook.key, !v)),
        })),
      });
    }
    return out;
  });
  function setAll(group: Group, v: boolean) {
    for (const row of group.rows) if (!row.disabled) row.set(v);
  }

  async function addRole() {
    const name = newRole.trim();
    if (!name) return;
    try {
      roles = await client.writeRole({ do: 'create', name });
      newRole = '';
    } catch {
      error = t('The role could not be saved.');
    }
  }
  async function removeRole(id: string) {
    try {
      roles = await client.writeRole({ do: 'delete', id });
      if (draft && draft.role === id) draft.role = 'worker';
    } catch {
      error = t('The role could not be deleted.');
    }
  }

  /** One Save: preset → environment → launch → role. Says what was saved if a step fails. */
  async function save(): Promise<string | null> {
    if (!draft || !canSave || busy) return null;
    // A frozen copy: the fields are disabled while saving, and nothing typed meanwhile mixes in.
    const d: LaunchDraft = $state.snapshot(draft) as LaunchDraft;
    busy = true;
    error = null;
    notice = null;
    const done: string[] = [];
    const folderSelection = await client
      .projectSelection(d.directory, d.agent)
      .catch(() => null);
    const plan = savePlan(d, {
      launches,
      selections: data.selections.filter((s) => s.provider === d.agent),
      folderSelection,
      savedSpec: d.selection
        ? (data.environments.get(d.selection.id) ?? null)
        : null,
      allItems: allItemsOf(data.library, d.agent),
    });
    try {
      let target: Selection | null = d.selection;
      const p = plan.preset;
      if (p.do === 'write' || p.do === 'copy' || p.do === 'create') {
        target = await client.saveSelection({
          id: p.do === 'write' ? p.id : null,
          name: p.name,
          provider: d.agent,
          items: d.items as Item[],
          composites: d.composites,
        });
        done.push(
          p.do === 'copy'
            ? t('preset “{name}” (a copy: the other launches keep theirs)', {
                name: p.name,
              })
            : t('preset “{name}”', { name: p.name }),
        );
      } else if (p.do === 'none') target = null;
      if (plan.environment && target && d.spec) {
        await client.writeEnvironment(target.id, d.spec);
        done.push(t('environment'));
      }
      const launch = await saveLaunch({
        name: d.name.trim(),
        directory: d.directory,
        agent: d.agent,
        profile: d.profile,
        preset: target?.name ?? null,
        replace: plan.launch.replace,
      });
      done.push(t('launch'));
      // The role follows the new name before the old file goes: a failure here leaves both
      // files, never a launch without its role.
      if (plan.role.renamedFrom)
        roles = await client.writeRole({
          do: 'rename',
          from: plan.role.renamedFrom,
          to: launch.id,
        });
      roles = await client.writeRole({
        do: 'assign',
        launch: launch.id,
        role: d.role,
      });
      done.push(t('role'));
      if (plan.removeOld) {
        try {
          await deleteLaunch(plan.removeOld);
          // It keeps its place in + Launch.
          saveOrder(renameInOrder(loadOrder(), plan.removeOld, launch.id));
        } catch {
          error = t(
            'Saved as “{name}”. The file under the old name could not be deleted: it is still listed.',
            { name: launch.name },
          );
        }
      }
      const failed = error;
      await load();
      const fresh = launches.find((l) => l.id === launch.id) ?? null;
      pick(fresh);
      error = failed;
      notice = failed ? null : t('Saved.');
      return launch.id;
    } catch (e) {
      const exists = e instanceof LaunchError && e.code === 'exists';
      const what = exists
        ? t('A launch with that name exists.')
        : e instanceof Error
          ? e.message
          : t('It could not be saved.');
      error =
        done.length > 0
          ? t('{what} Saved so far: {done}. Nothing else changed.', {
              what,
              done: done.join(', '),
            })
          : what;
      return null;
    } finally {
      busy = false;
    }
  }
  async function tryIt() {
    if (!draft) return;
    if (!dirty && draft.id) {
      onopen(draft.id);
      return;
    }
    const id = await save();
    if (id) onopen(id);
  }
  async function remove() {
    if (!current || !editable(current)) return;
    busy = true;
    try {
      await deleteLaunch(current.id);
      roles = await client.writeRole({ do: 'forget', launch: current.id });
      selected = null;
      draft = null;
      await load();
    } catch {
      error = t('The launch could not be deleted.');
    } finally {
      busy = false;
      confirming = false;
    }
  }
  async function browse() {
    const path = await choose().catch(() => null);
    if (path && draft) draft.directory = path;
  }
</script>

<section
  class="launch-editor"
  aria-label={t('Launches')}
  hidden={!visible}
  inert={!visible}
>
  <aside class="le-list">
    <div class="le-search">
      <input
        class="le-input"
        type="search"
        placeholder={t('Search…')}
        aria-label={t('Search launches')}
        bind:value={q}
      />
      <button
        type="button"
        class="top-action icon"
        aria-label={t('Roles')}
        title={t('Roles')}
        aria-expanded={rolesOpen}
        onclick={() => (rolesOpen = !rolesOpen)}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"
          ><path
            d="M9 7a3 3 0 1 0 0 .01M3 20c0-3 2.7-5 6-5s6 2 6 5M17 8v6M14 11h6"
          /></svg
        >
      </button>
    </div>
    {#if rolesOpen}
      <div class="le-roles">
        {#each allRoles as role (role.id)}
          <div class="le-role-row">
            <span>{role.name}</span>
            {#if role.basic}
              <span class="le-meta">{t('basic')}</span>
            {:else}
              <button
                type="button"
                class="action quiet"
                aria-label={t('Delete role {name}', { name: role.name })}
                onclick={() => void removeRole(role.id)}>×</button
              >
            {/if}
          </div>
        {/each}
        <form
          class="le-role-row"
          onsubmit={(e) => {
            e.preventDefault();
            void addRole();
          }}
        >
          <input
            class="le-input"
            placeholder={t('New role…')}
            aria-label={t('New role name')}
            maxlength="40"
            bind:value={newRole}
          />
          <button type="submit" class="action">{t('Add')}</button>
        </form>
        <p class="le-meta">
          {t('Launches of a deleted role become Worker.')}
        </p>
      </div>
    {/if}
    <div class="le-groups">
      {#if phase === 'loading' && launches.length === 0}
        <p class="le-meta">{t('Reading launches…')}</p>
      {/if}
      {#each groups as group (group.role.id)}
        <p class="le-role">{group.role.name}</p>
        {#each group.items as launch (launch.id)}
          {@const e = estimateLaunch(launch, data)}
          <button
            type="button"
            class="le-item"
            aria-current={selected === launch.id ? 'true' : undefined}
            onclick={() => pick(launch)}
          >
            <span class="le-name">{launch.name}</span>
            {#if e}<span class="le-meta">~{kTokens(e.total)}</span>{/if}
          </button>
        {/each}
      {/each}
      {#if phase === 'ready' && launches.length === 0}
        <p class="le-meta">{t('No saved launches yet.')}</p>
      {/if}
    </div>
    <button type="button" class="le-new" onclick={() => pick(null)}
      >+ {t('New')}</button
    >
  </aside>

  {#if draft}
    {@const d = draft}
    <div class="le-editor">
      <header class="le-head">
        <input
          class="le-title"
          disabled={busy}
          aria-label={t('Launch name')}
          maxlength="60"
          bind:value={d.name}
        />
        <span class="le-total" aria-live="polite">
          {#if est && ceiling}
            ~{kTokens(est.total)}
            <span class="le-bar" aria-hidden="true"
              ><span
                style:width={`${Math.min(100, Math.round((est.total / ceiling.total) * 100))}%`}
              ></span></span
            >
            <span class="le-meta"
              >{t('of {total} · estimated, MCP not measured', {
                total: kTokens(ceiling.total),
              })}</span
            >
          {:else if d.agent === 'codex' && !readOnly}
            <span class="le-meta">{t('No estimate for Codex yet.')}</span>
          {/if}
        </span>
        {#if current && editable(current)}
          {#if confirming}
            <span class="le-meta"
              >{t('Delete {name}?', { name: current.name })}</span
            >
            <button
              type="button"
              class="action danger"
              onclick={() => void remove()}>{t('Delete')}</button
            >
            <button
              type="button"
              class="action"
              onclick={() => (confirming = false)}>{t('Cancel')}</button
            >
          {:else}
            <button
              type="button"
              class="top-action icon le-delete"
              aria-label={t('Delete launch')}
              title={t('Delete launch')}
              onclick={() => (confirming = true)}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true"
                ><path
                  d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 11v6M14 11v6"
                /></svg
              >
            </button>
          {/if}
        {/if}
        <button
          type="button"
          class="action"
          disabled={busy || (!d.id && !canSave) || (dirty && !canSave)}
          onclick={() => void tryIt()}
          >{dirty || !d.id ? t('Save and try') : t('Try')}</button
        >
        <button
          type="button"
          class="action primary"
          disabled={busy || !dirty || !canSave}
          onclick={() => void save()}>{busy ? t('Saving…') : t('Save')}</button
        >
      </header>
      {#if current && !editable(current)}
        <p class="notice" data-tone="warn">
          {t(
            'This launch runs commands from its file. It opens from here; edit it in its file.',
          )}
        </p>
      {/if}
      {#if roles.damaged}
        <p class="notice" data-tone="warn">
          {t(
            'roles.json could not be read. Roles are shown as the basic ones and are not saved until the file is fixed or removed.',
          )}
        </p>
      {/if}
      {#if error}<p class="notice" data-tone="error" role="alert">
          {error}
        </p>{/if}
      {#if notice}<p class="notice" data-tone="ok" role="status">
          {notice}
        </p>{/if}

      {#if renames && current && draft}
        <p class="notice" data-tone="warn">
          {t(
            'This launch is saved as {file}.toml. Saving writes {next}.toml and removes {file}.toml.',
            { file: current.id, next: launchId(draft.name) },
          )}
        </p>
      {/if}
      {#if !readOnly}
        <fieldset class="le-fieldset" disabled={busy}>
          <div class="le-fields">
            <label class="le-field"
              ><span>{t('Role')}</span>
              <select class="le-input" bind:value={d.role}>
                {#each allRoles as role (role.id)}
                  <option value={role.id}>{role.name}</option>
                {/each}
              </select>
            </label>
            <label class="le-field"
              ><span>{t('Distributor')}</span>
              <select
                class="le-input"
                value={d.agent}
                onchange={(e) => setAgent(e.currentTarget.value as Agent)}
              >
                {#each AGENTS as a (a.id)}
                  <option value={a.id}
                    >{a.distributor} · {a.id === 'claude_code'
                      ? 'Claude Code'
                      : 'Codex'}</option
                  >
                {/each}
              </select>
            </label>
            <label class="le-field"
              ><span>{t('Model')}</span>
              <select
                class="le-input"
                value={d.profile ?? ''}
                onchange={(e) => (d.profile = e.currentTarget.value || null)}
              >
                <option value="">{t('Default (no profile)')}</option>
                {#each agentProfiles as p (p.id)}
                  <option value={p.body.name}
                    >{p.body.model ?? t('default model')} · {p.body
                      .name}</option
                  >
                {/each}
              </select>
            </label>
            <label class="le-field"
              ><span>{t('Mode')}</span>
              <select
                class="le-input"
                disabled
                title={t(
                  'Launches open Native sessions. Managed sessions start from Sessions.',
                )}
              >
                <option>Native</option>
              </select>
            </label>
            <div class="le-field le-wide">
              <span>{t('Folder')}</span>
              <div class="le-folder">
                <!-- Read only: a folder is chosen in the dialog or among the known projects. The
                     desktop reads only those, so a typed path could never start. -->
                <input
                  class="le-input"
                  aria-label={t('Folder')}
                  readonly
                  title={t('Choose it with Browse… or from the known folders.')}
                  value={d.directory}
                />
                <button
                  type="button"
                  class="top-action icon"
                  aria-label={t('Saved and recent folders')}
                  title={t('Saved and recent folders')}
                  aria-expanded={foldersOpen}
                  onclick={() => (foldersOpen = !foldersOpen)}
                >
                  <svg viewBox="0 0 24 24" aria-hidden="true"
                    ><path
                      d="M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1-4.4-4.3 6.1-.9z"
                    /></svg
                  >
                </button>
                <button
                  type="button"
                  class="action"
                  onclick={() => void browse()}>{t('Browse…')}</button
                >
                {#if foldersOpen}
                  <div class="le-folders">
                    {#if saved.length > 0}<p class="le-role">
                        {t('Saved')}
                      </p>{/if}
                    {#each saved as folder (folder)}
                      <button
                        type="button"
                        class="le-item"
                        onclick={() => {
                          d.directory = folder;
                          foldersOpen = false;
                        }}><span class="le-name">{folder}</span></button
                      >
                    {/each}
                    {#if used.length > 0}<p class="le-role">
                        {t('Recently used')}
                      </p>{/if}
                    {#each used as entry (entry.path)}
                      <button
                        type="button"
                        class="le-item"
                        onclick={() => {
                          d.directory = entry.path;
                          foldersOpen = false;
                        }}><span class="le-name">{entry.path}</span></button
                      >
                    {/each}
                  </div>
                {/if}
              </div>
            </div>
          </div>
        </fieldset>
        <div class="le-loads" inert={busy}>
          {#if d.agent === 'codex'}
            <p class="le-meta le-pad">
              {t(
                'For Codex a launch chooses skills and agents; the rest is Claude Code only.',
              )}
            </p>
          {/if}
          {#each groupsOf as group (group.id)}
            {@const n = group.rows.filter((r) => r.on).length}
            <div class="le-group">
              <div class="le-ghead">
                <input
                  type="checkbox"
                  aria-label={t(group.label)}
                  checked={n === group.rows.length && n > 0}
                  indeterminate={n > 0 && n < group.rows.length}
                  disabled={group.rows.length === 0}
                  onchange={(e) => setAll(group, e.currentTarget.checked)}
                />
                <button
                  type="button"
                  class="le-gtoggle"
                  aria-expanded={open === group.id}
                  onclick={() => (open = open === group.id ? null : group.id)}
                >
                  <span class="le-chev" class:open={open === group.id}>›</span>
                  {t(group.label)}
                  <span class="le-meta"
                    >{t('{on} of {all}', {
                      on: n,
                      all: group.rows.length,
                    })}</span
                  >
                </button>
                <span class="le-cost"
                  >{group.tokens === null
                    ? group.note
                      ? t(group.note)
                      : ''
                    : group.tokens === 0
                      ? '0'
                      : group.tokens < 1000
                        ? kTokens(group.tokens)
                        : `~${kTokens(group.tokens)}`}</span
                >
              </div>
              {#if open === group.id}
                <div class="le-rows">
                  {#each group.rows as row (row.key)}
                    <label class="le-row" class:off={!row.on}>
                      <input
                        type="checkbox"
                        checked={row.on}
                        disabled={row.disabled}
                        onchange={(e) => row.set(e.currentTarget.checked)}
                      />
                      <span title={row.label}>{row.label}</span>
                    </label>
                  {/each}
                  {#if group.rows.length === 0}
                    <p class="le-meta">{t('Nothing of this kind.')}</p>
                  {/if}
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {:else if phase === 'failed'}
    <div class="le-editor">
      <p class="notice" data-tone="error" role="alert">{error}</p>
      <button type="button" class="action" onclick={() => void load()}
        >{t('Try again')}</button
      >
    </div>
  {/if}
</section>
