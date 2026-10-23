<script lang="ts">
  import { onMount } from 'svelte';
  import { desktopProjects } from '../../desktop/projects-client';
  import ProjectsView from './ProjectsView.svelte';
  import { ProjectsController } from './controller';

  // A window of its own onto the same store. It carries the projects capability and nothing else.
  const controller = new ProjectsController((next) => {
    projects = next;
  });
  let projects = $state(controller.state());
  onMount(() => {
    controller.setVisible(true);
    return () => controller.dispose();
  });
</script>

<main class="content projects-window">
  <ProjectsView
    {projects}
    {controller}
    client={desktopProjects}
    visible={true}
    standalone={true}
  />
</main>
