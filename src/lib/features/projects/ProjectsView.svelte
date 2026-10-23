<script lang="ts">
  import type { Profile } from '../../contracts/native';
  import type { ProjectView, SessionView } from '../../contracts/projects';
  import type { ProjectsClient } from '../../desktop/projects-client';
  import type { NativeProfileOption } from './LaunchPanel.svelte';
  import MigrationPanel from './MigrationPanel.svelte';
  import ProjectRail from './ProjectRail.svelte';
  import WorkspaceCanvas from './WorkspaceCanvas.svelte';
  import type { ProjectsController, ProjectsState } from './controller';

  let {
    projects,
    controller,
    client,
    visible,
    nativeProfiles = [],
    standalone = false,
  }: {
    projects: ProjectsState;
    controller: ProjectsController;
    client: ProjectsClient;
    visible: boolean;
    nativeProfiles?: Profile[];
    /** A window of its own, opened from the application. */
    standalone?: boolean;
  } = $props();

  const store = $derived(projects.overview?.store);
  const enabled = $derived(projects.overview?.enabled ?? false);
  const host = $derived(projects.overview?.host);
  const profiles = $derived<NativeProfileOption[]>(
    nativeProfiles
      .filter((p) => !p.template)
      .map((p) => ({ id: p.id, name: p.body.name, version: p.current })),
  );
  // A Managed owner answering is not known from here; the launch says so if it does not.
  const managedAvailable = true;
  const scratch = $derived(projects.project === null);
  const current = $derived(
    projects.projects.find((p) => p.id === projects.project) ?? null,
  );
  const emptyLeaf = $derived(
    projects.workspace?.leaves.find((l) => !l.session) ?? null,
  );
  // Adopting a shell changes which project it belongs to: it is asked once, then done.
  let adopting = $state<{ session: SessionView; project: ProjectView } | null>(
    null,
  );
  const SESSION_KIND: Record<string, string> = {
    native: 'Agent',
    shell: 'Shell',
    managed: 'Managed',
  };
  const RECOVERY: Record<string, string> = {
    unsupported_schema:
      'This store was written by a newer version of the Console. It is shown as it is, read-only, and nothing was changed.',
    invalid_record:
      'The store cannot be read as the format it claims. Its exact bytes are kept untouched and nothing was replaced by an empty one.',
    capacity_exceeded:
      'The store is bigger than the limit. Its exact bytes are kept untouched.',
  };

  function label(session: SessionView): string {
    if (session.kind === 'managed')
      return `Managed ${session.managedSessionId?.slice(0, 8) ?? ''}`;
    return session.nativeSessionId
      ? `Agent ${session.nativeSessionId.slice(0, 8)}`
      : `Shell ${session.terminalId?.slice(0, 8) ?? ''}`;
  }
</script>

<section
  class="projects-feature"
  class:standalone
  aria-label="Projects and workspaces"
  hidden={!visible}
  inert={!visible}
>
  {#if projects.notice}
    <div class="projects-banner" data-kind={projects.notice.kind} role="alert">
      <span>{projects.notice.text}</span>
      {#if projects.notice.failure?.nextAction === 'refetch'}
        <button
          type="button"
          class="action"
          onclick={() => void controller.refresh()}>Refresh</button
        >
      {/if}
      <button type="button" class="action" onclick={() => controller.dismiss()}
        >Dismiss</button
      >
    </div>
  {/if}

  {#if projects.phase === 'idle' || projects.phase === 'loading'}
    <div class="sessions-empty" role="status" aria-busy="true">
      <p class="empty-title">Reading…</p>
    </div>
  {:else if projects.phase === 'failed' && !projects.overview}
    <div class="sessions-empty">
      <p class="empty-title">Projects could not be read</p>
      <button
        type="button"
        class="action"
        onclick={() => void controller.refresh()}>Try again</button
      >
    </div>
  {:else if !enabled}
    <div class="sessions-empty">
      <p class="empty-title">Projects and workspaces are off</p>
      <p>
        They can be turned on in Settings. Until then nothing about them is read
        or run, and your sessions and the older windows are untouched.
      </p>
    </div>
  {:else if store?.state === 'missing'}
    <MigrationPanel
      preview={projects.preview}
      {controller}
      busy={projects.busy}
      mode="start"
    />
  {:else if store?.state === 'unusable'}
    <div class="sessions-empty" role="alert">
      <p class="empty-title">The projects store is read-only</p>
      <p>{RECOVERY[store.code] ?? RECOVERY['invalid_record']}</p>
      <p class="projects-note">
        Reason: {store.code.replace(/_/g, ' ')} ({store.field}). Your sessions
        and the older windows are not affected.
      </p>
    </div>
  {:else if store?.state === 'unavailable'}
    <div class="sessions-empty">
      <p class="empty-title">No place to keep projects</p>
      <p>The Console has no state directory to keep them in.</p>
    </div>
  {:else if store?.state === 'ready'}
    {#if host && !host.reachable}
      <p class="projects-banner" data-kind="info" role="status">
        No terminal host answers. Sessions are shown as unknown, and nothing is
        started or recreated.
      </p>
    {/if}
    <div class="projects-layout">
      <ProjectRail {projects} {controller} />
      <div class="projects-main">
        <!-- Hidden means unmounted: no pane keeps reading or holding input behind another view. -->
        {#if projects.workspace && visible}
          <WorkspaceCanvas
            workspace={projects.workspace}
            {scratch}
            {controller}
            {client}
            {profiles}
            {managedAvailable}
            busy={projects.busy}
            launched={projects.launched}
          />
        {/if}
        <section class="projects-sessions" aria-label="Sessions">
          <h3>
            {scratch
              ? 'Unassigned sessions'
              : `Sessions of ${current?.label ?? ''}`}
          </h3>
          {#if projects.sessions.length === 0}
            <p class="projects-note">
              None. History that is not shown in a pane stays here.
            </p>
          {:else}
            <ul class="projects-list">
              {#each projects.sessions as session (session.id)}
                <li
                  class="projects-session-row"
                  data-availability={session.availability}
                >
                  <span>{label(session)}</span>
                  <span class="projects-note">
                    {SESSION_KIND[session.kind]} · {session.availability}{session.reason
                      ? ` (${session.reason.replace('_', ' ')})`
                      : ''} · {session.state}
                  </span>
                  {#if session.availability === 'live' && session.state !== 'attached' && emptyLeaf}
                    <button
                      type="button"
                      class="action"
                      disabled={projects.busy}
                      onclick={() =>
                        void controller.attach(emptyLeaf.id, session.id)}
                      >Show in an empty pane</button
                    >
                  {/if}
                  {#if scratch && session.kind === 'shell' && session.originProjectId === null && projects.projects.length > 0}
                    <select
                      aria-label={`Adopt ${label(session)} into a project`}
                      onchange={(event) => {
                        const id = event.currentTarget.value;
                        const target = projects.projects.find(
                          (p) => p.id === id,
                        );
                        event.currentTarget.value = '';
                        if (target?.root)
                          adopting = { session, project: target };
                      }}
                    >
                      <option value="">Adopt into a project…</option>
                      {#each projects.projects.filter((p) => !p.tombstone && p.root) as p (p.id)}
                        <option value={p.id}>{p.label}</option>
                      {/each}
                    </select>
                  {/if}
                  {#if adopting?.session.id === session.id}
                    <span class="projects-note" role="alert">
                      Move this shell into {adopting.project.label}? It keeps
                      running where it is; only the project it is listed under
                      changes.
                    </span>
                    <button
                      type="button"
                      class="action primary"
                      disabled={projects.busy}
                      onclick={() => {
                        const chosen = adopting;
                        adopting = null;
                        if (chosen?.project.root)
                          void controller.adopt(
                            chosen.session.id,
                            chosen.project.id,
                            chosen.project.root.bindingId,
                          );
                      }}>Move</button
                    >
                    <button
                      type="button"
                      class="action"
                      onclick={() => (adopting = null)}>Cancel</button
                    >
                  {/if}
                </li>
              {/each}
            </ul>
          {/if}
        </section>
        {#if store.state === 'ready'}
          <details class="projects-more">
            <summary>Import, receipts and going back</summary>
            <MigrationPanel
              preview={projects.preview}
              {controller}
              busy={projects.busy}
              mode="update"
            />
            <div class="projects-row">
              <button
                type="button"
                class="action"
                onclick={() => void controller.compactReceipts()}
              >
                Compact finished launch receipts
              </button>
              <button
                type="button"
                class="action"
                onclick={() => void controller.readRollback()}
              >
                What going back needs
              </button>
              {#if !standalone}
                <button
                  type="button"
                  class="action"
                  onclick={() => void controller.openWindow()}
                >
                  Open another window
                </button>
              {/if}
            </div>
            {#if projects.rollback}
              <p class="projects-note">
                {projects.rollback.canDisable
                  ? 'Nothing is unaccounted for; projects can be turned off in Settings.'
                  : `${projects.rollback.unfinishedLaunches} launch(es) of unknown outcome must be resolved first.`}
                {projects.rollback.note}
              </p>
            {/if}
          </details>
        {/if}
      </div>
    </div>
  {/if}
</section>
