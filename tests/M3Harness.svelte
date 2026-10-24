<script lang="ts">
  import type { M3Client } from '../src/lib/desktop/m3-client';
  import { M3Controller } from '../src/lib/features/m3/controller';
  import ProvidersView from '../src/lib/features/m3/ProvidersView.svelte';

  // A view with the controller the application wires, over a client the test chooses.
  let { client, visible = true }: { client: M3Client; visible?: boolean } =
    $props();
  // svelte-ignore state_referenced_locally
  const controller = new M3Controller(client, (next) => {
    view = next;
  });
  let view = $state(controller.state());
  $effect(() => {
    controller.setVisible(visible);
    return () => controller.dispose();
  });
</script>

<ProvidersView {visible} {view} {controller} />
