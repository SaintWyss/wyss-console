<script lang="ts">
  import ConfigurationView from '../src/lib/features/configuration/ConfigurationView.svelte';
  import {
    ConfigurationController,
    type ConfigurationClient,
  } from '../src/lib/features/configuration/controller';

  // A view with a controller of its own, as the application wires them, over a client
  // the test chooses. `visible` is a getter so the test can hide and show it.
  let {
    client,
    visible = true,
  }: { client: ConfigurationClient; visible?: boolean } = $props();
  // The harness is built once, for one client: reading `client` later would be wrong.
  // svelte-ignore state_referenced_locally
  const controller = new ConfigurationController((next) => {
    configuration = next;
  }, client);
  let configuration = $state(controller.state());
  $effect(() => {
    controller.setVisible(visible);
    return () => controller.dispose();
  });
</script>

<ConfigurationView {configuration} {controller} {visible} />
