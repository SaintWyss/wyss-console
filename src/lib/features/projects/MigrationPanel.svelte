<script lang="ts">
  import type { MigrationSummary } from '../../contracts/projects';
  import type { ProjectsController } from './controller';
  import { msg, t } from '../../i18n/index.svelte';

  let {
    preview,
    controller,
    busy,
    mode,
  }: {
    preview: MigrationSummary | null;
    controller: ProjectsController;
    busy: boolean;
    /** `start` offers the first import; `update` offers what appeared later. */
    mode: 'start' | 'update';
  } = $props();

  let accept = $state(false);
  // A new preview is a new question: an acceptance given to an earlier one does not carry over.
  let shownPreview = '';
  $effect(() => {
    const id = preview?.previewId ?? '';
    if (id !== shownPreview) {
      shownPreview = id;
      accept = false;
    }
  });
  const BLOCKING = [
    'source_unreadable',
    'source_unstable',
    'source_oversized',
    'over_bound',
    'invalid',
  ];
  // Accepting spellings never overrides something that blocks the import.
  const blocked = $derived(
    (preview?.conflicts ?? []).some((c) => BLOCKING.includes(c.kind)),
  );
  const KIND: Record<string, string> = {
    source_unreadable: msg(
      'A file of the older state cannot be read as the schema it claims',
    ),
    source_unstable: msg(
      'A file of the older state kept changing while it was read',
    ),
    source_oversized: msg('A file of the older state is bigger than its bound'),
    alias: msg('A registered folder is another spelling of a registered root'),
    root_unavailable: msg(
      'A registered folder is missing or cannot be used (not imported)',
    ),
    root_shared: msg(
      'Another account can write in a registered folder, or that could not be checked (not imported)',
    ),
    host_offline: msg('No terminal host answered: no live session is imported'),
    over_bound: msg('Something does not fit the limits; nothing is cut to fit'),
    incompatible_id: msg(
      'An identity from the older state cannot be kept (not imported)',
    ),
    invalid: msg('The result would not be valid'),
  };
</script>

<section
  class="projects-migration"
  aria-label={t('Import from the older windows')}
>
  {#if !preview}
    <p class="projects-note">
      {mode === 'start'
        ? t(
            'Projects and workspaces are built from the folders, agents and tabs the older windows already know. The preview reads them and changes nothing; the older windows keep working.',
          )
        : t(
            'The older windows changed since the last import. A new preview adds only what is new; nothing already here is changed.',
          )}
    </p>
    <div class="projects-row">
      <button
        type="button"
        class="action primary"
        disabled={busy}
        onclick={() => void controller.previewMigration()}
      >
        {t('Preview the import')}
      </button>
      {#if mode === 'start'}
        <button
          type="button"
          class="action"
          disabled={busy}
          onclick={() => void controller.initializeEmpty()}
        >
          {t('Start empty instead')}
        </button>
      {/if}
    </div>
  {:else}
    <h3>
      {preview.mode === 'initial' ? t('Import preview') : t('Update preview')}
    </h3>
    <ul class="projects-facts-list">
      <li>{t('{count} projects', { count: preview.counts.projects })}</li>
      <li>{t('{count} workspaces', { count: preview.counts.workspaces })}</li>
      <li>{t('{count} live sessions', { count: preview.counts.sessions })}</li>
      <li>
        {t('{placed} panes with a session, {empty} empty', {
          placed: preview.counts.placedLeaves,
          empty: preview.counts.emptyLeaves,
        })}
      </li>
      {#if preview.counts.tabsSkipped > 0}<li>
          {t('{count} tabs with nothing alive (skipped)', {
            count: preview.counts.tabsSkipped,
          })}
        </li>{/if}
    </ul>
    {#if preview.conflicts.length > 0}
      <ul class="projects-conflicts" aria-label={t('Things to know')}>
        {#each preview.conflicts as conflict (conflict.kind)}
          <li>
            {KIND[conflict.kind] ? t(KIND[conflict.kind]) : conflict.kind} ({conflict.count})
          </li>
        {/each}
      </ul>
    {/if}
    {#if preview.needsAliasAcceptance}
      <label class="projects-check">
        <input type="checkbox" bind:checked={accept} />
        {t('I accept that spellings of the same folder become one project')}
      </label>
    {/if}
    <p class="projects-note">
      {t(
        'The exact older files are kept in a private snapshot first. The older state is never overwritten, and no session is started, stopped or restarted.',
      )}
    </p>
    <div class="projects-row">
      <button
        type="button"
        class="action primary"
        disabled={busy ||
          blocked ||
          !(preview.canCommit || (preview.needsAliasAcceptance && accept))}
        onclick={() => void controller.commitMigration(accept)}
        >{t('Import')}</button
      >
      <button
        type="button"
        class="action"
        onclick={() => controller.cancelPreview()}>{t('Cancel')}</button
      >
    </div>
  {/if}
</section>
