<script lang="ts">
  import type { Profile } from '../../contracts/native';
  import type {
    AdoptionPreview,
    ProjectView,
    SessionView,
  } from '../../contracts/projects';
  import type { ProjectsClient } from '../../desktop/projects-client';
  import type { NativeProfileOption } from './LaunchPanel.svelte';
  import MigrationPanel from './MigrationPanel.svelte';
  import ProjectRail from './ProjectRail.svelte';
  import VirtualList from './VirtualList.svelte';
  import WorkspaceCanvas from './WorkspaceCanvas.svelte';
  import type { ProjectsController, ProjectsState } from './controller';
  import { msg, t } from '../../i18n/index.svelte';

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
  // Adopting a shell changes which project it belongs to. The window first asks the desktop where
  // the shell really is, shows that, and only then asks: the answer is good for one adoption.
  let adopting = $state<{
    session: SessionView;
    project: ProjectView;
    shown: AdoptionPreview;
  } | null>(null);
  async function askToAdopt(session: SessionView, project: ProjectView) {
    adopting = null;
    const shown = await controller.previewAdoption(session.id, project.id);
    if (shown) adopting = { session, project, shown };
  }
  const SESSION_KIND: Record<string, string> = {
    native: msg('Agent'),
    shell: msg('Shell'),
    managed: msg('Managed'),
  };
  const RECOVERY: Record<string, string> = {
    unsupported_schema: msg(
      'This store was written by a newer version of the Console. It is shown as it is, read-only, and nothing was changed.',
    ),
    invalid_record: msg(
      'The store cannot be read as the format it claims. Its exact bytes are kept untouched and nothing was replaced by an empty one.',
    ),
    capacity_exceeded: msg(
      'The store is bigger than the limit. Its exact bytes are kept untouched.',
    ),
  };

  function label(session: SessionView): string {
    if (session.kind === 'managed')
      return t('Managed {id}', {
        id: session.managedSessionId?.slice(0, 8) ?? '',
      });
    return session.nativeSessionId
      ? t('Agent {id}', { id: session.nativeSessionId.slice(0, 8) })
      : t('Shell {id}', { id: session.terminalId?.slice(0, 8) ?? '' });
  }
</script>

<section
  class="projects-feature"
  class:standalone
  aria-label={t('Projects and workspaces')}
  hidden={!visible}
  inert={!visible}
>
  {#if projects.notice}
    <div class="projects-banner" data-kind={projects.notice.kind} role="alert">
      <span>{t(projects.notice.text)}</span>
      {#if projects.notice.failure?.nextAction === 'refetch'}
        <button
          type="button"
          class="action"
          onclick={() => void controller.refresh()}>{t('Refresh')}</button
        >
      {/if}
      <button type="button" class="action" onclick={() => controller.dismiss()}
        >{t('Dismiss')}</button
      >
    </div>
  {/if}

  {#if projects.phase === 'idle' || projects.phase === 'loading'}
    <div class="sessions-empty" role="status" aria-busy="true">
      <p class="empty-title">{t('Reading…')}</p>
    </div>
  {:else if projects.phase === 'failed' && !projects.overview}
    <div class="sessions-empty">
      <p class="empty-title">{t('Projects could not be read')}</p>
      <button
        type="button"
        class="action"
        onclick={() => void controller.refresh()}>{t('Try again')}</button
      >
    </div>
  {:else if !enabled}
    <div class="sessions-empty">
      <p class="empty-title">{t('Projects and workspaces are off')}</p>
      <p>
        {t(
          'They can be turned on in Settings. Until then nothing about them is read or run, and your sessions and the older windows are untouched.',
        )}
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
      <p class="empty-title">{t('The projects store is read-only')}</p>
      <p>{t(RECOVERY[store.code] ?? RECOVERY['invalid_record'])}</p>
      <p class="projects-note">
        {t(
          'Reason: {reason} ({field}). Your sessions and the older windows are not affected.',
          {
            reason: store.code.replace(/_/g, ' '),
            field: store.field,
          },
        )}
      </p>
    </div>
  {:else if store?.state === 'unavailable'}
    <div class="sessions-empty">
      <p class="empty-title">{t('No place to keep projects')}</p>
      <p>{t('The Console has no state directory to keep them in.')}</p>
    </div>
  {:else if store?.state === 'ready'}
    {#if host && !host.reachable}
      <p class="projects-banner" data-kind="info" role="status">
        {t(
          'No terminal host answers. Sessions are shown as unknown, and nothing is started or recreated.',
        )}
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
        <section class="projects-sessions" aria-label={t('Sessions')}>
          <h3>
            {scratch
              ? t('Unassigned sessions')
              : t('Sessions of {name}', { name: current?.label ?? '' })}
          </h3>
          {#if adopting}
            <div class="projects-adopt" role="alert">
              <p class="projects-note">
                {t('{name} is in {folder}.', {
                  name: label(adopting.session),
                  folder: adopting.shown.cwd,
                })}
                {adopting.shown.inside
                  ? t('That is inside {project}.', {
                      project: adopting.project.label,
                    })
                  : t('That is outside the folder of {project}.', {
                      project: adopting.project.label,
                    })}
                {t(
                  'Move it into {project}? It keeps running where it is; only the project it is listed under changes.',
                  {
                    project: adopting.project.label,
                  },
                )}
              </p>
              <div class="projects-row">
                <button
                  type="button"
                  class="action primary"
                  disabled={projects.busy}
                  onclick={() => {
                    const chosen = adopting;
                    adopting = null;
                    if (chosen)
                      void controller.adopt(
                        chosen.session.id,
                        chosen.project.id,
                        chosen.shown.bindingId,
                      );
                  }}>{t('Move')}</button
                >
                <button
                  type="button"
                  class="action"
                  onclick={() => (adopting = null)}>{t('Cancel')}</button
                >
              </div>
            </div>
          {/if}
          {#if projects.sessions.length === 0}
            <p class="projects-note">
              {t('None. History that is not shown in a pane stays here.')}
            </p>
          {:else}
            <VirtualList
              items={projects.sessions}
              rowHeight={44}
              key={(session) => session.id}
              label={t('Sessions')}
              attrs={(session) => ({
                class: 'projects-session-row',
                'data-availability': session.availability,
              })}
            >
              {#snippet row(session)}
                <span>{label(session)}</span>
                <span class="projects-note">
                  {t(SESSION_KIND[session.kind])} · {session.availability}{session.reason
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
                    >{t('Show in an empty pane')}</button
                  >
                {/if}
                {#if scratch && session.kind === 'shell' && session.originProjectId === null && projects.projects.length > 0}
                  <select
                    aria-label={t('Adopt {name} into a project', {
                      name: label(session),
                    })}
                    onchange={(event) => {
                      const id = event.currentTarget.value;
                      const target = projects.projects.find((p) => p.id === id);
                      event.currentTarget.value = '';
                      if (target?.root) void askToAdopt(session, target);
                    }}
                  >
                    <option value="">{t('Adopt into a project…')}</option>
                    {#each projects.projects.filter((p) => !p.tombstone && p.root) as p (p.id)}
                      <option value={p.id}>{p.label}</option>
                    {/each}
                  </select>
                {/if}
              {/snippet}
            </VirtualList>
          {/if}
        </section>
        {#if store.state === 'ready'}
          <details class="projects-more">
            <summary>{t('Import, receipts and going back')}</summary>
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
                {t('Compact finished launch receipts')}
              </button>
              <button
                type="button"
                class="action"
                onclick={() => void controller.readRollback()}
              >
                {t('What going back needs')}
              </button>
              {#if !standalone}
                <button
                  type="button"
                  class="action"
                  onclick={() => void controller.openWindow()}
                >
                  {t('Open another window')}
                </button>
              {/if}
            </div>
            {#if projects.rollback}
              <p class="projects-note">
                {projects.rollback.canDisable
                  ? t(
                      'Nothing is unaccounted for; projects can be turned off in Settings.',
                    )
                  : t(
                      '{count} launch(es) of unknown outcome must be resolved first.',
                      {
                        count: projects.rollback.unfinishedLaunches,
                      },
                    )}
                {projects.rollback.note}
              </p>
            {/if}
          </details>
        {/if}
      </div>
    </div>
  {/if}
</section>
