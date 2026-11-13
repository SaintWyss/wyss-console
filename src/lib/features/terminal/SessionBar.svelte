<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../../i18n/index.svelte';
  import type { SessionGit } from '../../contracts/terminal';
  import type { Profile } from '../../contracts/native';
  import type { TerminalId } from '../../contracts/identities';
  import type { TerminalSession } from './session-manager';
  import { agentPart, gitParts } from './session-bar';

  let {
    session,
    visible = true,
    read,
    profiles: loadProfiles,
    every = 5000,
    slowEvery = 60000,
  }: {
    /** The focused terminal, or none. */
    session: TerminalSession | undefined;
    visible?: boolean;
    /** Asks the desktop for the git state of the folder it holds for this terminal. */
    read: (id: TerminalId) => Promise<SessionGit>;
    profiles: () => Promise<Profile[]>;
    /** How often the git state is read again while the Console is shown. */
    every?: number;
    /** After git ran past its limit, how long until it is asked again. */
    slowEvery?: number;
  } = $props();

  let git = $state<SessionGit | null>(null);
  let profiles = $state<Profile[]>([]);
  const terminal = $derived(session?.view.info?.terminal_id);
  let asked: TerminalId | undefined;
  let busy = false;
  /** Until when this folder is not asked again: git took too long there. */
  let quietUntil = 0;

  async function refresh() {
    const id = terminal;
    if (!id || busy || !visible || Date.now() < quietUntil) return;
    busy = true;
    asked = id;
    try {
      const answer = await read(id);
      // The focus moved while git ran: this answer is about another terminal.
      if (asked === id && terminal === id) {
        git = answer;
        quietUntil =
          answer.kind === 'unknown' && answer.slow ? Date.now() + slowEvery : 0;
      }
    } catch {
      if (terminal === id) git = { kind: 'unknown', dir: null };
    } finally {
      busy = false;
    }
    if (terminal && terminal !== id) void refresh();
  }
  $effect(() => {
    // A new terminal in focus starts from nothing, so no other folder's state shows on it.
    void terminal;
    git = null;
    quietUntil = 0;
    void refresh();
  });
  $effect(() => {
    if (!visible) return;
    const timer = setInterval(() => void refresh(), every);
    return () => clearInterval(timer);
  });
  onMount(() => {
    loadProfiles().then(
      (list) => (profiles = list),
      () => undefined,
    );
  });
  const parts = $derived(gitParts(git, session?.view.info?.cwd ?? ''));
  const agent = $derived(agentPart(session?.native, profiles));
</script>

{#if session}
  <div class="session-bar" role="status" aria-label={t('Session')}>
    {#each parts as part (part.key)}
      {#if part.key === 'folder'}
        <span class="bar-part" title={part.raw}>
          <svg viewBox="0 0 24 24" aria-hidden="true"
            ><path d="M3 6h6l2 2h10v11H3z" /></svg
          ><span class="bar-folder"
            >{part.phrase.values?.folder ?? part.raw}</span
          >
        </span>
      {:else if part.key === 'branch'}
        <span class="bar-part" title={t('Branch')}>
          <svg viewBox="0 0 24 24" aria-hidden="true"
            ><path
              d="M6 3v12M6 15a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM18 3a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM18 9c0 4-6 3-12 6"
            /></svg
          >{part.phrase.values?.branch ?? t(part.phrase.text)}
        </span>
      {:else}
        <span
          class="bar-part"
          class:bar-nogit={part.key === 'nogit'}
          data-tone={part.tone}
          >{t(
            part.phrase.text,
            part.phrase.values as Record<string, string>,
          )}</span
        >
      {/if}
    {/each}
    <span class="bar-part bar-agent"
      >{t(
        agent.kind.text,
        agent.kind.values as Record<string, string>,
      )}{#if agent.detail}&#32;·
        {agent.detail}{/if}</span
    >
  </div>
{/if}
