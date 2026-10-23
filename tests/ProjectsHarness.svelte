<script lang="ts">
  import ProjectsView from '../src/lib/features/projects/ProjectsView.svelte';
  import { ProjectsController } from '../src/lib/features/projects/controller';
  import type { ProjectsClient } from '../src/lib/desktop/projects-client';

  let {
    client,
    visible: initiallyVisible = true,
    ids = () => crypto.randomUUID(),
  }: {
    client: ProjectsClient;
    visible?: boolean;
    ids?: () => string;
  } = $props();
  // svelte-ignore state_referenced_locally
  let visible = $state(initiallyVisible);
  // Built once for one client, as the application does.
  // svelte-ignore state_referenced_locally
  const controller = new ProjectsController(
    (next) => {
      projects = next;
    },
    client,
    { poll: 60000 },
    ids,
  );
  let projects = $state(controller.state());
  $effect(() => {
    controller.setVisible(visible);
    return () => controller.dispose();
  });
</script>

<button type="button" class="test-toggle" onclick={() => (visible = !visible)}
  >toggle view</button
>
<ProjectsView {projects} {controller} {client} {visible} />
