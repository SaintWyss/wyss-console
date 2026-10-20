<script lang="ts">
  import type { ConfigItem } from '../src/lib/contracts/configuration';
  import type { Profile } from '../src/lib/contracts/native';
  import type { SessionSummary } from '../src/lib/contracts/sessions';
  import {
    ModelsController,
    type ModelsClient,
  } from '../src/lib/features/models/controller';
  import ModelsView from '../src/lib/features/models/ModelsView.svelte';

  // A view with a controller of its own, as the application wires it, over a client the test
  // chooses and the other facts the application would hand it.
  let {
    client,
    configItems = null,
    profiles = [],
    sessions = null,
    visible = true,
  }: {
    client: ModelsClient;
    configItems?: readonly ConfigItem[] | null;
    profiles?: readonly Profile[];
    sessions?: readonly SessionSummary[] | null;
    visible?: boolean;
  } = $props();
  // svelte-ignore state_referenced_locally
  const controller = new ModelsController((next) => {
    models = next;
  }, client);
  let models = $state(controller.state());
  $effect(() => {
    controller.setVisible(visible);
    return () => controller.dispose();
  });
</script>

<ModelsView
  {models}
  {controller}
  {configItems}
  {profiles}
  {sessions}
  {visible}
/>
