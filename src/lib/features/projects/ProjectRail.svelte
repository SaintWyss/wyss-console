<script lang="ts">
  import type { ProjectsController, ProjectsState } from './controller';

  let {
    projects,
    controller,
  }: {
    projects: ProjectsState;
    controller: ProjectsController;
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
    missing: 'Folder missing',
    identity_changed: 'Folder replaced',
    ineligible: 'Folder not usable',
    unknown: '',
  };
</script>

<nav class="projects-rail" aria-label="Projects">
  <div class="projects-row">
    <button
      type="button"
      class="action primary"
      disabled={projects.busy}
      onclick={() => void controller.registerFolder()}
    >
      Add a folder…
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
      placeholder="New folder name"
      aria-label="New folder name"
      bind:value={folderName}
    />
    <button
      type="submit"
      class="action"
      disabled={projects.busy || folderName.trim() === ''}>Create…</button
    >
  </form>
  <ul class="projects-list" aria-label="Projects">
    <li>
      <button
        type="button"
        class="projects-item"
        aria-current={projects.project === null ? 'true' : undefined}
        onclick={() => void controller.selectProject(null)}
      >
        <span>Scratch</span>
        <span class="projects-note">no folder</span>
      </button>
    </li>
    {#each live as project (project.id)}
      <li>
        <button
          type="button"
          class="projects-item"
          aria-current={projects.project === project.id ? 'true' : undefined}
          onclick={() => void controller.selectProject(project.id)}
        >
          <span>{project.label}{project.archived ? ' (archived)' : ''}</span>
          <span class="projects-note">
            {project.root
              ? AVAILABILITY[project.root.availability] || project.root.label
              : 'no folder'}
          </span>
        </button>
      </li>
    {/each}
  </ul>
  <label class="projects-check">
    <input type="checkbox" bind:checked={showArchived} /> Show archived
  </label>

  {#if current}
    <div class="projects-actions" aria-label={`Actions for ${current.label}`}>
      {#if renaming === current.id}
        <input
          type="text"
          maxlength="64"
          aria-label="Project name"
          bind:value={renamed}
        />
        <button
          type="button"
          class="action"
          disabled={projects.busy || renamed.trim() === ''}
          onclick={async () => {
            await controller.rename(current.id, renamed.trim());
            renaming = null;
          }}>Save name</button
        >
        <button type="button" class="action" onclick={() => (renaming = null)}
          >Cancel</button
        >
      {:else}
        <button
          type="button"
          class="action"
          onclick={() => {
            renaming = current.id;
            renamed = current.label;
          }}>Rename</button
        >
        <button
          type="button"
          class="action"
          disabled={projects.busy}
          onclick={() => void controller.archive(current.id, !current.archived)}
        >
          {current.archived ? 'Unarchive' : 'Archive'}
        </button>
        {#if current.root}
          <button
            type="button"
            class="action"
            disabled={projects.busy}
            onclick={() =>
              void controller.relink(current.id, current.root!.bindingId)}
          >
            Relink folder…
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
          New workspace
        </button>
        <button
          type="button"
          class="action"
          disabled={projects.busy}
          onclick={() => void controller.remove(current.id, true)}
        >
          Remove, keep history
        </button>
        <button
          type="button"
          class="action"
          disabled={projects.busy}
          onclick={() => void controller.remove(current.id, false)}
        >
          Remove completely
        </button>
        <p class="projects-note">
          Removing a project never deletes its folder, a provider's history or a
          session; it is refused while any of its sessions may still be running.
        </p>
      {/if}
    </div>
    {#if current.workspaceIds.length > 1}
      <ul class="projects-list" aria-label="Workspaces">
        {#each current.workspaceIds as id, n (id)}
          <li>
            <button
              type="button"
              class="projects-item"
              aria-current={projects.workspaceId === id ? 'true' : undefined}
              onclick={() => void controller.selectWorkspace(id)}
            >
              {id === current.defaultWorkspaceId
                ? 'Default'
                : `Workspace ${n + 1}`}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</nav>
