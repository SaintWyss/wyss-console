<script lang="ts">
  import type { WorkspaceView } from '../../contracts/projects';
  import type { ProjectsClient } from '../../desktop/projects-client';
  import { arrange, type Divider } from '../terminal/layout';
  import LaunchPanel, { type NativeProfileOption } from './LaunchPanel.svelte';
  import SessionPane from './SessionPane.svelte';
  import type { ProjectsController } from './controller';

  let {
    workspace,
    scratch,
    controller,
    client,
    profiles,
    managedAvailable,
    busy,
  }: {
    workspace: WorkspaceView;
    scratch: boolean;
    controller: ProjectsController;
    client: ProjectsClient;
    profiles: NativeProfileOption[];
    managedAvailable: boolean;
    busy: boolean;
  } = $props();

  let area = $state<HTMLDivElement>();
  let launching = $state<string | null>(null);
  const arrangement = $derived(
    workspace.layout ? arrange(workspace.layout) : undefined,
  );
  const leafOf = (id: string) => workspace.leaves.find((l) => l.id === id);
  const pending = (id: string) =>
    workspace.unfinished.find((u) => u.leafId === id);

  /** Dragging a divider changes a ratio; it is saved once, when the pointer is let go. */
  function drag(event: PointerEvent, divider: Divider) {
    if (!area) return;
    event.preventDefault();
    const box = area.getBoundingClientRect();
    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture(event.pointerId);
    let ratio = divider.ratio;
    const move = (e: PointerEvent) => {
      ratio =
        divider.dir === 'row'
          ? (e.clientX - box.left) / box.width
          : (e.clientY - box.top) / box.height;
      const span = divider.dir === 'row' ? divider.area.w : divider.area.h;
      const origin = divider.dir === 'row' ? divider.area.x : divider.area.y;
      ratio = (ratio - origin) / span;
    };
    const up = () => {
      target.removeEventListener('pointermove', move);
      target.removeEventListener('pointerup', up);
      void controller.resizeSplit(divider.path, ratio);
    };
    target.addEventListener('pointermove', move);
    target.addEventListener('pointerup', up);
  }
</script>

<section class="projects-canvas" aria-label={`Workspace ${workspace.label}`}>
  <div class="projects-toolbar">
    <span class="projects-title">{workspace.label}</span>
    {#if workspace.layout === null}
      <button
        type="button"
        class="action"
        disabled={busy}
        onclick={() => void controller.addPane(null, 'row')}>Add a pane</button
      >
    {:else}
      <button
        type="button"
        class="action"
        disabled={busy || !workspace.focus}
        onclick={() => void controller.addPane(workspace.focus, 'row')}
        >Split side by side</button
      >
      <button
        type="button"
        class="action"
        disabled={busy || !workspace.focus}
        onclick={() => void controller.addPane(workspace.focus, 'col')}
        >Split top and bottom</button
      >
    {/if}
  </div>
  <div class="pane-area" bind:this={area}>
    {#if arrangement}
      {#each [...arrangement.panes] as [id, place] (id)}
        {@const leaf = leafOf(id)}
        {@const unfinished = pending(id)}
        <div
          class="projects-pane"
          class:focused={workspace.focus === id}
          style:left={`${place.x * 100}%`}
          style:top={`${place.y * 100}%`}
          style:width={`${place.w * 100}%`}
          style:height={`${place.h * 100}%`}
        >
          {#if leaf?.session}
            <SessionPane
              session={leaf.session}
              {client}
              title={leaf.title}
              ondetach={() => void controller.detach(id, leaf.session!.id)}
              onrefresh={() => void controller.refresh()}
            />
          {:else if unfinished}
            <div class="projects-unfinished" role="status">
              <p class="empty-title">
                {unfinished.state === 'pending'
                  ? 'A launch is in progress'
                  : 'A launch has an unknown outcome'}
              </p>
              {#if unfinished.state === 'ambiguous'}
                <p class="projects-note">
                  Nothing was started again. If you find the session it started
                  in the list of sessions, attach it; if nothing started, say
                  so. Nothing here retries.
                </p>
                <div class="projects-row">
                  <button
                    type="button"
                    class="action"
                    disabled={busy}
                    onclick={() =>
                      void controller.reconcile(unfinished.operationId, {
                        kind: 'absent',
                      })}>Nothing started</button
                  >
                  <button
                    type="button"
                    class="action"
                    disabled={busy}
                    onclick={() =>
                      void controller.reconcile(unfinished.operationId, {
                        kind: 'retryManaged',
                      })}>Ask the Managed owner again</button
                  >
                </div>
              {/if}
            </div>
          {:else if launching === id}
            <LaunchPanel
              {scratch}
              {profiles}
              {managedAvailable}
              {busy}
              onlaunch={(choice) => {
                launching = null;
                void controller.launch({ leafId: id, ...choice });
              }}
            />
            <button
              type="button"
              class="action"
              onclick={() => (launching = null)}>Cancel</button
            >
          {:else}
            <div class="projects-empty-pane">
              <p class="projects-note">Empty pane. Nothing runs here.</p>
              <div class="projects-row">
                <button
                  type="button"
                  class="action primary"
                  disabled={busy}
                  onclick={() => (launching = id)}>Start…</button
                >
                <button
                  type="button"
                  class="action"
                  disabled={busy}
                  onclick={() => void controller.closePane(id)}
                  >Remove pane</button
                >
              </div>
            </div>
          {/if}
        </div>
      {/each}
      {#each arrangement.dividers as divider (divider.path)}
        <div
          class="pane-divider"
          class:row={divider.dir === 'row'}
          role="separator"
          aria-orientation={divider.dir === 'row' ? 'vertical' : 'horizontal'}
          style:left={divider.dir === 'row'
            ? `${(divider.area.x + divider.area.w * divider.ratio) * 100}%`
            : `${divider.area.x * 100}%`}
          style:top={divider.dir === 'row'
            ? `${divider.area.y * 100}%`
            : `${(divider.area.y + divider.area.h * divider.ratio) * 100}%`}
          style:width={divider.dir === 'row'
            ? undefined
            : `${divider.area.w * 100}%`}
          style:height={divider.dir === 'row'
            ? `${divider.area.h * 100}%`
            : undefined}
          onpointerdown={(event) => drag(event, divider)}
        ></div>
      {/each}
    {:else}
      <div class="terminal-empty">
        This workspace has no panes. Add one to start something in it.
      </div>
    {/if}
  </div>
</section>
