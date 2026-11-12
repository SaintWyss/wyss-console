<script lang="ts">
  import type { ProjectsController, ProjectsState } from './controller';
  import VirtualList from './VirtualList.svelte';
  import { msg, t } from '../../i18n/index.svelte';

  let {
    projects,
    controller,
    oninsight = null,
  }: {
    projects: ProjectsState;
    controller: ProjectsController;
    /** Opens the details of a project whose folder is available. */
    oninsight?: ((projectId: string) => void) | null;
  } = $props();

  let showArchived = $state(false);
  let folderName = $state('');
  let renaming = $state<string | null>(null);
  let renamed = $state('');
  const live = $derived(
    projects.projects.filter(
      (p) => !p.tombstone && (showArchived || !p.archived),
    ),
  );
  const current = $derived(
    projects.projects.find((p) => p.id === projects.project) ?? null,
  );
  const AVAILABILITY: Record<string, string> = {
    available: '',
    missing: msg('Folder missing'),
    identity_changed: msg('Folder replaced'),
    ineligible: msg('Folder not usable'),
    shared: msg('Another account can write here'),
    unknown: '',
  };
</script>

<nav class="projects-rail" aria-label={t('Projects')}>
  <div class="projects-row">
    <button
      type="button"
      class="action primary"
      disabled={projects.busy}
      onclick={() => void controller.registerFolder()}
    >
      {t('Add a folder…')}
    </button>
  </div>
  <form
    class="projects-row"
    onsubmit={(event) => {
      event.preventDefault();
      const name = folderName.trim();
      if (name)
        void controller.createFolder(name).then(() => (folderName = ''));
    }}
  >
    <input
      type="text"
      maxlength="255"
      placeholder={t('New folder name')}
      aria-label={t('New folder name')}
      bind:value={folderName}
    />
    <button
      type="submit"
      class="action"
      disabled={projects.busy || folderName.trim() === ''}
      >{t('Create…')}</button
    >
  </form>
  <ul class="projects-list" aria-label={t('Scratch')}>
    <li>
      <button
        type="button"
        class="projects-item"
        aria-current={projects.project === null ? 'true' : undefined}
        onclick={() => void controller.selectProject(null)}
      >
        <span>{t('Scratch')}</span>
        <span class="projects-note">{t('no folder')}</span>
      </button>
    </li>
  </ul>
  <VirtualList
    items={live}
    rowHeight={56}
    key={(project) => project.id}
    label={t('Projects')}
  >
    {#snippet row(project)}
      <button
        type="button"
        class="projects-item"
        aria-current={projects.project === project.id ? 'true' : undefined}
        onclick={() => void controller.selectProject(project.id)}
      >
        <span
          >{project.archived
            ? t('{name} (archived)', { name: project.label })
            : project.label}</span
        >
        <span class="projects-note">
          {project.root
            ? AVAILABILITY[project.root.availability]
              ? t(AVAILABILITY[project.root.availability])
              : project.root.label
            : t('no folder')}
        </span>
      </button>
    {/snippet}
  </VirtualList>
  <label class="projects-check">
    <input type="checkbox" bind:checked={showArchived} />
    {t('Show archived')}
  </label>

  {#if current}
    <div
      class="projects-actions"
      aria-label={t('Actions for {name}', { name: current.label })}
    >
      {#if renaming === current.id}
        <input
          type="text"
          maxlength="64"
          aria-label={t('Project name')}
          bind:value={renamed}
        />
        <button
          type="button"
          class="action"
          disabled={projects.busy || renamed.trim() === ''}
          onclick={async () => {
            await controller.rename(current.id, renamed.trim());
            renaming = null;
          }}>{t('Save name')}</button
        >
        <button type="button" class="action" onclick={() => (renaming = null)}
          >{t('Cancel')}</button
        >
      {:else}
        <button
          type="button"
          class="action"
          onclick={() => {
            renaming = current.id;
            renamed = current.label;
          }}>{t('Rename')}</button
        >
        <button
          type="button"
          class="action"
          disabled={projects.busy}
          onclick={() => void controller.archive(current.id, !current.archived)}
        >
          {current.archived ? t('Unarchive') : t('Archive')}
        </button>
        {#if current.root?.availability === 'available' && oninsight}
          <button
            type="button"
            class="action"
            onclick={() => oninsight?.(current.id)}
          >
            Detalles
          </button>
        {/if}
        {#if current.root}
          <button
            type="button"
            class="action"
            disabled={projects.busy}
            onclick={() =>
              void controller.relink(current.id, current.root!.bindingId)}
          >
            {t('Relink folder…')}
          </button>
        {/if}
        <button
          type="button"
          class="action"
          disabled={projects.busy}
          onclick={() =>
            void controller.createWorkspace(
              current.id,
              `Workspace ${current.workspaceIds.length + 1}`,
            )}
        >
          {t('New workspace')}
        </button>
        <button
          type="button"
          class="action"
          disabled={projects.busy}
          onclick={() => void controller.remove(current.id, true)}
        >
          {t('Remove, keep history')}
        </button>
        <button
          type="button"
          class="action"
          disabled={projects.busy}
          onclick={() => void controller.remove(current.id, false)}
        >
          {t('Remove completely')}
        </button>
        <p class="projects-note">
          {t(
            "Removing a project never deletes its folder, a provider's history or a session; it is refused while any of its sessions may still be running.",
          )}
        </p>
      {/if}
    </div>
    {#if current.workspaceIds.length > 1}
      <ul class="projects-list" aria-label={t('Workspaces')}>
        {#each current.workspaceIds as id, n (id)}
          <li>
            <button
              type="button"
              class="projects-item"
              aria-current={projects.workspaceId === id ? 'true' : undefined}
              onclick={() => void controller.selectWorkspace(id)}
            >
              {id === current.defaultWorkspaceId
                ? t('Default')
                : t('Workspace {count}', { count: n + 1 })}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</nav>
