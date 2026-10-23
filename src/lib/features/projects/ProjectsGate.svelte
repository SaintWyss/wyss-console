<script lang="ts">
  import type { ProjectsController, ProjectsState } from './controller';

  let {
    projects,
    controller,
  }: {
    projects: ProjectsState;
    controller: ProjectsController;
  } = $props();
  const enabled = $derived(projects.overview?.enabled ?? false);
</script>

<section class="settings-card" aria-label="Projects and workspaces">
  <h3>Projects and workspaces</h3>
  <p class="setting-hint">
    {#if enabled}
      On. Projects and workspaces are shown in their own view. Turning them off
      goes back to the older windows; nothing running is stopped and the project
      data is kept.
    {:else}
      Off. When turned on, a project is a folder with its own workspaces and
      sessions. Your sessions and the older windows are not changed, and nothing
      is started or stopped. Turning it on is confirmed in a window of its own.
    {/if}
  </p>
  <div class="settings-actions">
    <button
      type="button"
      class="action"
      onclick={() => void controller.setEnabled(!enabled)}
      >{enabled ? 'Turn off…' : 'Turn on…'}</button
    >
  </div>
  {#if projects.notice}<p class="setting-hint" role="alert">
      {projects.notice.text}
    </p>{/if}
</section>
