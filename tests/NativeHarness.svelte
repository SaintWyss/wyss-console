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
  // svelte-ignore state_referenced_locally
  let active = $state(which);
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

<nav aria-label="Native test navigation">
  <button type="button" onclick={() => (active = 'workspace')}>Workspace</button
  >
  <button type="button" onclick={() => (active = 'profiles')}>Profiles</button>
</nav>
<WorkspaceView
  {native}
  {controller}
  visible={visible && active === 'workspace'}
  {onconsole}
/>
<ProfilesView
  {native}
  {controller}
  visible={visible && active === 'profiles'}
/>
