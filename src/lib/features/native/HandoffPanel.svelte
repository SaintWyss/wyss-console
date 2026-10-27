<script lang="ts">
  import { onMount } from 'svelte';
  import type { AgentId, NativeSession, Preview } from '../../contracts/native';
  import type { NativeController, NativeState } from './controller';
  import { AGENT_LABEL, profilesFor } from './present';

  let {
    session,
    native,
    controller,
    onclose,
    onstarted,
  }: {
    session: NativeSession;
    native: NativeState;
    controller: NativeController;
    onclose: () => void;
    /** An agent was started with the brief: the window shows its terminal. */
    onstarted: () => void;
  } = $props();

  let text = $state('');
  let loading = $state(true);
  let screen = $state(false);
  let preview = $state<Preview | null>(null);
  let previewFailed = $state(false);
  // svelte-ignore state_referenced_locally
  let target = $state<AgentId>(
    session.agent_id === 'claude_code' ? 'codex' : 'claude_code',
  );
  let profile = $state<string | null>(null);
  const agents = $derived(
    (native.launcher?.agents ?? []).filter(
      (a) => a.installed && a.agent_id !== 'hermes',
    ),
  );
  const profiles = $derived(profilesFor(native.profiles, target));
  const tooLong = $derived(text.length > 16 * 1024);
  const ready = $derived(
    !loading &&
      !native.starting &&
      !tooLong &&
      text.startsWith('# ') &&
      (profile === null || preview?.can_start === true) &&
      agents.some((a) => a.agent_id === target),
  );
  $effect(() => {
    if (profile && !profiles.some((p) => p.id === profile)) profile = null;
  });
  let previewSeq = 0;
  $effect(() => {
    const selectedAgent = target;
    const selectedProfile = profile;
    const sequence = ++previewSeq;
    preview = null;
    previewFailed = false;
    if (!selectedProfile) return;
    void controller.preview(selectedAgent, selectedProfile).then(
      (next) => {
        if (sequence === previewSeq) preview = next;
      },
      () => {
        if (sequence === previewSeq) previewFailed = true;
      },
    );
  });
  onMount(async () => {
    const brief = await controller.handoff(session.id);
    if (brief) {
      text = brief.text;
      screen = brief.screen_included;
    }
    loading = false;
  });
  async function start() {
    if (
      await controller.startAgent(
        target,
        session.project,
        profile,
        text,
        preview,
      )
    )
      onstarted();
  }
</script>

<section
  class="native-card native-handoff"
  aria-label="Hand off to another agent"
>
  <h3>Hand off {session.agent_name}’s work</h3>
  <p class="native-note">
    This starts a <strong>new</strong> session of the agent you choose, in the
    same folder, with the text below as its first message. It is not a transfer:
    the new agent has only this text and the files, not {session.agent_name}’s
    context, and nothing is moved or ended. Read it and change it before
    starting. Secrets found in it were removed.
  </p>
  <p class="native-warning" role="note">
    Hermes Native remains available as a separate interactive provider. It is
    omitted here because its pinned CLI does not expose a verified reviewed
    first-message interface without entering an effectful one-shot mode.
  </p>
  {#if loading}
    <p class="sessions-status" role="status" aria-busy="true">
      Preparing the brief…
    </p>
  {:else}
    {#if !screen}
      <p class="native-warning">
        The screen of that session is no longer held, so the brief has the
        repository state only.
      </p>
    {/if}
    <div class="native-field">
      <label for="handoff-text">Brief (Markdown, starts with “# ”)</label>
      <textarea id="handoff-text" rows="16" bind:value={text}></textarea>
      {#if tooLong}<p class="native-warning">
          Longer than 16 KiB: shorten it.
        </p>{/if}
    </div>
    <div class="native-field">
      <label for="handoff-target">Start</label>
      <div class="native-row">
        <select id="handoff-target" bind:value={target}>
          {#each agents as a (a.agent_id)}
            <option value={a.agent_id}>{a.name}</option>
          {/each}
        </select>
        <select id="handoff-profile" aria-label="Profile" bind:value={profile}>
          <option value={null}>No profile — as configured</option>
          {#each profiles as p (p.id)}
            <option value={p.id}>{p.body.name}</option>
          {/each}
        </select>
      </div>
    </div>
    {#if profile && previewFailed}
      <p class="native-warning" role="status">
        Could not verify this profile preview. Refresh Native before starting.
      </p>
    {:else if preview}
      <ul class="native-preview" aria-label="What the profile does">
        {#each preview.applied as line (line)}<li>{line}</li>{/each}
        {#each preview.with_cost as line (line)}<li class="cost">
            {line}
          </li>{/each}
        {#each preview.not_applicable as line (line)}<li class="refused">
            {line}
          </li>{/each}
        {#each preview.warnings as line (line)}<li class="evidence">
            {line}
          </li>{/each}
      </ul>
    {/if}
    <div class="native-plan-actions">
      <button
        type="button"
        class="action primary"
        disabled={!ready}
        onclick={start}
      >
        {native.starting
          ? 'Starting…'
          : `Start ${AGENT_LABEL[target]} with this brief`}
      </button>
      <button
        type="button"
        class="action"
        disabled={loading || tooLong}
        onclick={() => controller.saveBrief(text)}
      >
        Save to a file…
      </button>
      <button type="button" class="action" onclick={onclose}>Cancel</button>
    </div>
  {/if}
</section>
