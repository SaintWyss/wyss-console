<script lang="ts">
  import { t } from '../../i18n/index.svelte';
  import {
    saveVariants,
    type Launch,
    type SystemInfo,
    type Variant,
  } from '../../desktop/launch-client';
  import {
    compose,
    DISTROS,
    distroIdOk,
    distroOf,
    fresh,
    linesOf,
  } from './distros';

  let {
    launch,
    system,
    onsaved,
    onopen,
    save = saveVariants,
  }: {
    launch: Extract<Launch, { kind: 'commands' }>;
    system: SystemInfo;
    /** The launch as the desktop read it back after saving. */
    onsaved: (launch: Launch) => void;
    onopen: (id: string) => void;
    save?: typeof saveVariants;
  } = $props();

  /** Each variant with the script as the editor holds it. */
  interface Row extends Variant {
    script: string;
  }
  const rowOf = (v: Variant): Row => ({
    ...v,
    // Commands the switches would not make were edited in the file: kept as typed.
    manual:
      v.manual ||
      (launch.template === 'system-update' &&
        !!distroOf(v.distro) &&
        compose(distroOf(v.distro)!, v.options).join('\n') !==
          v.commands.join('\n')),
    options: [...v.options],
    commands: [...v.commands],
    script: v.commands.join('\n'),
  });

  let rows = $state<Row[]>([]);
  let baseline = $state('');
  let picked = $state<string | null>(null);
  let adding = $state(false);
  let newId = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);

  function reset(from: typeof launch) {
    rows = from.variants.map(rowOf);
    baseline = JSON.stringify(rows);
    // The distribution picked stays picked after a save, while it is still there.
    picked = rows.some((r) => r.distro === picked)
      ? picked
      : (from.active ?? rows[0]?.distro ?? null);
    adding = false;
    error = null;
  }
  // A fresh read of the launch (another launch picked, or the saved copy back) starts over.
  let seen: typeof launch | null = null;
  $effect.pre(() => {
    if (launch !== seen) {
      seen = launch;
      reset(launch);
    }
  });

  const isUpdate = $derived(launch.template === 'system-update');
  const dirty = $derived(JSON.stringify(rows) !== baseline);
  const row = $derived(rows.find((r) => r.distro === picked) ?? null);
  const distro = $derived(
    row && isUpdate && !row.manual ? distroOf(row.distro) : undefined,
  );
  const labelOf = (id: string) =>
    (isUpdate ? distroOf(id)?.label : undefined) ?? id;
  /** The distributions that can still be added from the shipped ones. */
  const missing = $derived(
    isUpdate ? DISTROS.filter((d) => !rows.some((r) => r.distro === d.id)) : [],
  );
  const valid = $derived(
    rows.every((r) => linesOf(r.script).length > 0) &&
      (rows.length > 0 || launch.commands.length > 0),
  );

  function setSwitch(id: string, on: boolean) {
    if (!row || !distro) return;
    row.options = on
      ? [...row.options.filter((o) => o !== id), id]
      : row.options.filter((o) => o !== id);
    // Kept in the switches' order, so the file does not change with the order they were pressed.
    row.options = distro.switches
      .map((s) => s.id)
      .filter((s) => row.options.includes(s));
    row.commands = compose(distro, row.options);
    row.script = row.commands.join('\n');
  }
  function editScript(text: string) {
    if (!row) return;
    row.script = text;
    row.commands = linesOf(text);
    // A script typed by hand is kept as typed: the switches would write over it.
    if (isUpdate && distroOf(row.distro)) row.manual = true;
  }
  function backToSwitches() {
    if (!row) return;
    const d = distroOf(row.distro);
    if (!d) return;
    row.manual = false;
    row.commands = compose(d, row.options);
    row.script = row.commands.join('\n');
  }
  function add(id: string) {
    const clean = id.trim().toLowerCase();
    if (!distroIdOk(clean) || rows.some((r) => r.distro === clean)) return;
    const shipped = isUpdate ? distroOf(clean) : undefined;
    rows = [
      ...rows,
      rowOf(
        shipped
          ? fresh(shipped)
          : { distro: clean, commands: [], options: [], manual: true },
      ),
    ];
    picked = clean;
    adding = false;
    newId = '';
  }
  function remove(id: string) {
    if (id === launch.active) return;
    rows = rows.filter((r) => r.distro !== id);
    if (picked === id) picked = launch.active ?? rows[0]?.distro ?? null;
  }

  async function store(): Promise<boolean> {
    if (busy || !valid) return false;
    busy = true;
    error = null;
    notice = null;
    try {
      const fresh = await save(
        launch.id,
        rows.map((r) => ({
          distro: r.distro,
          commands: linesOf(r.script),
          options: [...r.options],
          manual: r.manual,
        })),
      );
      onsaved(fresh);
      notice = t('Saved.');
      return true;
    } catch (e) {
      error = e instanceof Error ? e.message : t('It could not be saved.');
      return false;
    } finally {
      busy = false;
    }
  }
  async function tryIt() {
    if (dirty && !(await store())) return;
    onopen(launch.id);
  }
</script>

<div class="dv" aria-label={t('Distributions')}>
  <p class="dv-system">
    {#if system.id}
      {t('Detected on this computer: {name} ({id})', {
        name: system.name || system.id,
        id: system.id,
      })}
    {:else}
      {t('This computer’s distribution could not be read (/etc/os-release).')}
    {/if}
  </p>
  {#if error}<p class="notice" data-tone="error" role="alert">{error}</p>{/if}
  {#if notice && !dirty}<p class="notice" data-tone="ok" role="status">
      {notice}
    </p>{/if}
  {#if !launch.active}
    <p class="notice" data-tone="warn">
      {launch.commands.length > 0
        ? t(
            'No distribution here matches this computer: the file’s own commands run.',
          )
        : t(
            'No distribution here matches this computer: add {id} to start it here.',
            {
              id: system.id || '?',
            },
          )}
    </p>
  {/if}
  <div class="dv-body">
    <aside class="dv-list">
      <p class="le-role">{t('Distributions')}</p>
      {#each rows as r (r.distro)}
        <button
          type="button"
          class="le-item"
          aria-current={picked === r.distro ? 'true' : undefined}
          onclick={() => (picked = r.distro)}
        >
          <span class="dv-dot" class:on={r.distro === launch.active}></span>
          <span class="le-name">{labelOf(r.distro)}</span>
          {#if r.distro === launch.active}<span
              class="le-meta"
              title={t('This computer’s distribution: it cannot be deleted.')}
              >{t('active')}</span
            >{/if}
        </button>
      {/each}
      {#if adding}
        <div class="dv-add">
          {#each missing as d (d.id)}
            <button type="button" class="le-item" onclick={() => add(d.id)}
              ><span class="le-name">{d.label}</span></button
            >
          {/each}
          <form
            class="le-folder"
            onsubmit={(e) => {
              e.preventDefault();
              add(newId);
            }}
          >
            <input
              class="le-input"
              placeholder={t('os-release ID, e.g. void')}
              aria-label={t('Distribution ID')}
              maxlength="64"
              bind:value={newId}
            />
            <button
              type="submit"
              class="action"
              disabled={!distroIdOk(newId.trim().toLowerCase())}
              >{t('Add')}</button
            >
          </form>
        </div>
      {:else}
        <button type="button" class="le-new" onclick={() => (adding = true)}
          >+ {t('Add distribution')}</button
        >
      {/if}
    </aside>

    {#if row}
      {@const r = row}
      <section class="dv-detail" aria-label={labelOf(r.distro)}>
        <header class="dv-head">
          <h3>{labelOf(r.distro)}</h3>
          <span class="le-meta">{r.distro}</span>
          {#if r.distro === launch.active}<span class="dv-badge"
              >{t('runs on this computer')}</span
            >{/if}
        </header>

        {#if isUpdate && distroOf(r.distro)}
          {@const d = distroOf(r.distro)!}
          <fieldset class="dv-switches" disabled={busy || r.manual}>
            <legend class="le-role">{t('Options')}</legend>
            {#each d.switches as s (s.id)}
              <label class="le-row" class:off={!r.options.includes(s.id)}>
                <input
                  type="checkbox"
                  checked={r.options.includes(s.id)}
                  onchange={(e) => setSwitch(s.id, e.currentTarget.checked)}
                />
                <span>{t(s.label)}</span>
              </label>
            {/each}
          </fieldset>
          {#if r.manual}
            <p class="notice" data-tone="warn">
              {t('The script was written by hand, so the options are off.')}
              <button
                type="button"
                class="action quiet"
                onclick={backToSwitches}>{t('Back to options')}</button
              >
            </p>
          {/if}
        {/if}

        <label class="dv-script">
          <span class="le-role">{t('Script it runs')}</span>
          <textarea
            class="le-input"
            spellcheck="false"
            rows={Math.max(3, r.script.split('\n').length + 1)}
            disabled={busy}
            value={r.script}
            oninput={(e) => editScript(e.currentTarget.value)}></textarea>
          <span class="le-meta"
            >{t('One command per line, typed in a new terminal in {folder}.', {
              folder: launch.directory,
            })}</span
          >
        </label>

        <div class="dv-actions">
          <button
            type="button"
            class="action danger"
            disabled={busy || r.distro === launch.active}
            title={r.distro === launch.active
              ? t('This computer’s distribution: it cannot be deleted.')
              : undefined}
            onclick={() => remove(r.distro)}>{t('Delete')}</button
          >
          <span class="dv-spacer"></span>
          {#if dirty}
            <button
              type="button"
              class="action"
              disabled={busy}
              onclick={() => reset(launch)}>{t('Discard')}</button
            >
          {/if}
          <button
            type="button"
            class="action"
            disabled={busy ||
              !valid ||
              (launch.commands.length === 0 && !dirty)}
            onclick={() => void tryIt()}
            >{dirty ? t('Save and try') : t('Try')}</button
          >
          <button
            type="button"
            class="action primary"
            disabled={busy || !dirty || !valid}
            onclick={() => void store()}
            >{busy ? t('Saving…') : t('Save')}</button
          >
        </div>
      </section>
    {:else}
      <section class="dv-detail">
        {#if launch.commands.length > 0}
          <div class="dv-script">
            <span class="le-role">{t('Script it runs')}</span>
            <pre class="le-input dv-pre">{launch.commands.join('\n')}</pre>
            <span class="le-meta"
              >{t(
                'The file’s own commands, for every distribution. Add one to give it its own.',
              )}</span
            >
          </div>
        {:else}
          <p class="le-meta">{t('No distributions yet.')}</p>
        {/if}
        <div class="dv-actions">
          <span class="dv-spacer"></span>
          {#if launch.commands.length > 0 && !dirty}
            <button
              type="button"
              class="action"
              onclick={() => onopen(launch.id)}>{t('Try')}</button
            >
          {/if}
          <button
            type="button"
            class="action primary"
            disabled={busy || !dirty || !valid}
            onclick={() => void store()}>{t('Save')}</button
          >
        </div>
      </section>
    {/if}
  </div>
</div>
