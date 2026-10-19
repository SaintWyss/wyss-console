<script lang="ts">
  import ProfilesView from '../src/lib/features/native/ProfilesView.svelte';
  import WorkspaceView from '../src/lib/features/native/WorkspaceView.svelte';
  import {
    NativeController,
    type NativeClient,
  } from '../src/lib/features/native/controller';
  import type { StartReply } from '../src/lib/contracts/native';

  let {
    client,
    which = 'workspace',
    visible = true,
    started = () => undefined,
    onconsole = () => undefined,
  }: {
    client: NativeClient;
    which?: 'workspace' | 'profiles';
    visible?: boolean;
    started?: (reply: StartReply) => void;
    onconsole?: () => void;
  } = $props();
  // Built once for one client, as the application does.
  // svelte-ignore state_referenced_locally
  const controller = new NativeController(
    (next) => {
      native = next;
    },
    (reply) => started(reply),
    client,
    { overview: 20, background: 40 },
  );
  let native = $state(controller.state());
  $effect(() => {
    controller.setVisible(visible);
    return () => controller.dispose();
  });
</script>

{#if which === 'workspace'}
  <WorkspaceView {native} {controller} {visible} {onconsole} />
{:else}
  <ProfilesView {native} {controller} {visible} />
{/if}
