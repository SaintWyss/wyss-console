<script lang="ts">
  import type { MigrationSummary } from '../../contracts/projects';
  import type { ProjectsController } from './controller';

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
    source_unreadable:
      'A file of the older state cannot be read as the schema it claims',
    source_unstable:
      'A file of the older state kept changing while it was read',
    source_oversized: 'A file of the older state is bigger than its bound',
    alias: 'A registered folder is another spelling of a registered root',
    root_unavailable:
      'A registered folder is missing or cannot be used (not imported)',
    host_offline: 'No terminal host answered: no live session is imported',
    over_bound: 'Something does not fit the limits; nothing is cut to fit',
    incompatible_id:
      'An identity from the older state cannot be kept (not imported)',
    invalid: 'The result would not be valid',
  };
</script>

<section class="projects-migration" aria-label="Import from the older windows">
  {#if !preview}
    <p class="projects-note">
      {mode === 'start'
        ? 'Projects and workspaces are built from the folders, agents and tabs the older windows already know. The preview reads them and changes nothing; the older windows keep working.'
        : 'The older windows changed since the last import. A new preview adds only what is new; nothing already here is changed.'}
    </p>
    <div class="projects-row">
      <button
        type="button"
        class="action primary"
        disabled={busy}
        onclick={() => void controller.previewMigration()}
      >
        Preview the import
      </button>
      {#if mode === 'start'}
        <button
          type="button"
          class="action"
          disabled={busy}
          onclick={() => void controller.initializeEmpty()}
        >
          Start empty instead
        </button>
      {/if}
    </div>
  {:else}
    <h3>{preview.mode === 'initial' ? 'Import preview' : 'Update preview'}</h3>
    <ul class="projects-facts-list">
      <li>{preview.counts.projects} projects</li>
      <li>{preview.counts.workspaces} workspaces</li>
      <li>{preview.counts.sessions} live sessions</li>
      <li>
        {preview.counts.placedLeaves} panes with a session, {preview.counts
          .emptyLeaves} empty
      </li>
      {#if preview.counts.tabsSkipped > 0}<li>
          {preview.counts.tabsSkipped} tabs with nothing alive (skipped)
        </li>{/if}
    </ul>
    {#if preview.conflicts.length > 0}
      <ul class="projects-conflicts" aria-label="Things to know">
        {#each preview.conflicts as conflict (conflict.kind)}
          <li>{KIND[conflict.kind] ?? conflict.kind} ({conflict.count})</li>
        {/each}
      </ul>
    {/if}
    {#if preview.needsAliasAcceptance}
      <label class="projects-check">
        <input type="checkbox" bind:checked={accept} /> I accept that spellings of
        the same folder become one project
      </label>
    {/if}
    <p class="projects-note">
      The exact older files are kept in a private snapshot first. The older
      state is never overwritten, and no session is started, stopped or
      restarted.
    </p>
    <div class="projects-row">
      <button
        type="button"
        class="action primary"
        disabled={busy ||
          blocked ||
          !(preview.canCommit || (preview.needsAliasAcceptance && accept))}
        onclick={() => void controller.commitMigration(accept)}>Import</button
      >
      <button
        type="button"
        class="action"
        onclick={() => controller.cancelPreview()}>Cancel</button
      >
    </div>
  {/if}
</section>
