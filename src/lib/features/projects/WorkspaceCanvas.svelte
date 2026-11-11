<script lang="ts">
  import type { Orphan, WorkspaceView } from '../../contracts/projects';
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
    launched = null,
    oninsight = null,
  }: {
    workspace: WorkspaceView;
    scratch: boolean;
    controller: ProjectsController;
    client: ProjectsClient;
    profiles: NativeProfileOption[];
    managedAvailable: boolean;
    busy: boolean;
    launched?: string | null;
    /** Opens the project's details; absent when the workspace belongs to no folder. */
    oninsight?: (() => void) | null;
  } = $props();

  let area = $state<HTMLDivElement>();
  let launching = $state<string | null>(null);
  // What an unknown Native launch could have started, by launch: read when the user looks.
  let candidates = $state<Record<string, Orphan[] | undefined>>({});
  async function look(operationId: string) {
    const found = await controller.orphans(operationId);
    if (found) candidates[operationId] = found;
  }
  const arrangement = $derived(
    workspace.layout ? arrange(workspace.layout) : undefined,
  );
  // Splitting needs a pane to split: the focused one, else the first.
  const splitTarget = $derived(
    workspace.focus ?? workspace.leaves[0]?.id ?? null,
  );
  const leafOf = (id: string) => workspace.leaves.find((l) => l.id === id);
  const pending = (id: string) =>
    workspace.unfinished.find((u) => u.leafId === id);

  /** The same change from the keyboard: arrows along the divider's axis, one step at a time. */
  function nudge(event: KeyboardEvent, divider: Divider) {
    const keys: Record<string, number> =
      divider.dir === 'row'
        ? { ArrowLeft: -1, ArrowRight: 1 }
        : { ArrowUp: -1, ArrowDown: 1 };
    const step = keys[event.key];
    if (step === undefined) return;
    event.preventDefault();
    const ratio = Math.min(0.9, Math.max(0.1, divider.ratio + step * 0.05));
    void controller.resizeSplit(divider.path, ratio);
  }

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
    {#if !scratch && oninsight}
      <button type="button" class="action" onclick={() => oninsight?.()}
        >Detalles del proyecto</button
      >
    {/if}
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
        disabled={busy || !splitTarget}
        onclick={() => void controller.addPane(splitTarget, 'row')}
        >Split side by side</button
      >
      <button
        type="button"
        class="action"
        disabled={busy || !splitTarget}
        onclick={() => void controller.addPane(splitTarget, 'col')}
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
              claimOnOpen={launched === leaf.session.id}
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
                  Nothing was started again. If it started something, pick it
                  from what is running and not shown anywhere; if nothing
                  started, say so. Nothing here retries.
                </p>
                {@const found = candidates[unfinished.operationId]}
                {#if unfinished.mode === 'native'}
                  {#if found === undefined}
                    <button
                      type="button"
                      class="action"
                      disabled={busy}
                      onclick={() => void look(unfinished.operationId)}
                      >Look for what it started</button
                    >
                  {:else if found.length === 0}
                    <p class="projects-note">
                      Nothing is running that is not already shown.
                    </p>
                  {:else}
                    <ul class="projects-list">
                      {#each found as orphan (orphan.terminalId)}
                        <li class="projects-session-row">
                          <span
                            >{orphan.nativeSessionId ? 'Agent' : 'Shell'}
                            {orphan.terminalId.slice(0, 8)}</span
                          >
                          <span class="projects-note"
                            >{orphan.folder ?? 'unknown folder'}</span
                          >
                          <button
                            type="button"
                            class="action"
                            disabled={busy}
                            onclick={() =>
                              void controller.reconcile(
                                unfinished.operationId,
                                {
                                  kind: 'identified',
                                  terminalId: orphan.terminalId,
                                  sessionId: orphan.nativeSessionId,
                                },
                              )}>This one</button
                          >
                        </li>
                      {/each}
                    </ul>
                  {/if}
                {/if}
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
                  {#if unfinished.mode === 'managed'}
                    <button
                      type="button"
                      class="action"
                      disabled={busy}
                      onclick={() =>
                        void controller.reconcile(unfinished.operationId, {
                          kind: 'retryManaged',
                        })}>Ask the Managed owner again</button
                    >
                  {/if}
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
          role="slider"
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
          tabindex="0"
          aria-label="Resize panes"
          aria-valuenow={Math.round(divider.ratio * 100)}
          aria-valuemin="10"
          aria-valuemax="90"
          onpointerdown={(event) => drag(event, divider)}
          onkeydown={(event) => nudge(event, divider)}
        ></div>
      {/each}
    {:else}
      <div class="terminal-empty">
        This workspace has no panes. Add one to start something in it.
      </div>
    {/if}
  </div>
</section>
