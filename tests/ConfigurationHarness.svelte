<script lang="ts">
  import ConfigurationView from '../src/lib/features/configuration/ConfigurationView.svelte';
  import {
    ChangesController,
    type ChangesClient,
  } from '../src/lib/features/configuration/changes-controller';
  import {
    ConfigurationController,
    type ConfigurationClient,
  } from '../src/lib/features/configuration/controller';

  // A view with controllers of its own, as the application wires them, over clients the
  // test chooses. `visible` can be changed to hide and show it.
  let {
    client,
    changesClient,
    visible = true,
  }: {
    client: ConfigurationClient;
    changesClient: ChangesClient;
    visible?: boolean;
  } = $props();
  // The harness is built once, for one client: reading `client` later would be wrong.
  // svelte-ignore state_referenced_locally
  const controller = new ConfigurationController((next) => {
    configuration = next;
  }, client);
  let configuration = $state(controller.state());
  // svelte-ignore state_referenced_locally
  const changesController = new ChangesController(
    (next) => {
      changes = next;
    },
    () => controller.refresh(),
    changesClient,
  );
  let changes = $state(changesController.state());
  $effect(() => {
    controller.setVisible(visible);
    return () => {
      controller.dispose();
      changesController.dispose();
    };
  });
</script>

<ConfigurationView
  {configuration}
  {controller}
  {changes}
  {changesController}
  {visible}
/>
