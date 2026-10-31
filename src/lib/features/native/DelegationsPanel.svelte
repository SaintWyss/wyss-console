<script lang="ts">
  import {
    cancelDelegation,
    readDelegations,
    type Delegation,
  } from '../../desktop/native-client';

  /** Agents that orchestrator sessions delegated work to, read from the terminal host. */
  let {
    visible,
    read = readDelegations,
    cancel = cancelDelegation,
  }: {
    visible: boolean;
    read?: () => Promise<Delegation[]>;
    cancel?: (id: string) => Promise<void>;
  } = $props();

  let delegations = $state<Delegation[]>([]);
  let failed = $state(false);

  const PROVIDER: Record<string, string> = {
    codex: 'Codex',
    claude_code: 'Claude Code',
  };
  const STATUS: Record<string, string> = {
    running: 'Running',
    succeeded: 'Done',
    failed: 'Failed',
    cancelled: 'Cancelled',
    timed_out: 'Timed out',
    interrupted: 'Interrupted',
  };
  const CHECK: Record<string, string> = {
    verified: 'configuration verified',
    mismatch: 'configuration did not match',
    pending: 'configuration not checked yet',
    unavailable: 'configuration not verifiable',
  };

  async function refresh() {
    try {
      delegations = await read();
      failed = false;
    } catch {
      failed = true;
    }
  }

  $effect(() => {
    if (!visible) return;
    void refresh();
    const timer = setInterval(() => void refresh(), 3000);
    return () => clearInterval(timer);
  });

  async function stop(id: string) {
    try {
      await cancel(id);
    } finally {
      await refresh();
    }
  }

  const when = (seconds: number) =>
    new Date(seconds * 1000).toLocaleTimeString(undefined, {
      timeStyle: 'short',
    });
</script>

<section aria-label="Delegated agents" class="delegations">
  <h3 class="native-heading">Delegated agents</h3>
  {#if failed}
    <p class="native-warning" role="alert">
      The terminal host did not list delegated agents.
    </p>
  {:else if delegations.length === 0}
    <p class="event-meta">
      None yet. A Claude Code or Codex session started with delegation on can
      give a task to the other one.
    </p>
  {/if}
  <ul class="delegation-list">
    {#each delegations as d (d.id)}
      <li class="delegation">
        <p class="delegation-line">
          <strong>{d.title}</strong>
          <span class="pill">{PROVIDER[d.provider] ?? d.provider}</span>
          <span class="pill">{d.model} · {d.effort}</span>
          <span class="pill" data-status={d.status}
            >{STATUS[d.status] ?? d.status}</span
          >
          {#if d.status === 'running'}
            <button type="button" class="action" onclick={() => stop(d.id)}
              >Cancel</button
            >
          {/if}
        </p>
        <p class="event-meta">
          From {PROVIDER[d.parent_provider] ?? d.parent_provider} session {d.parent_session.slice(
            0,
            8,
          )} · {when(d.created_at)}{d.ended_at ? ` – ${when(d.ended_at)}` : ''} ·
          {CHECK[d.verification.state] ?? d.verification.state} ·
          {d.steps} completed step(s)
        </p>
        {#if d.verification.model}
          <p class="event-meta">
            Observed: {d.verification.model} · {d.verification.effort ??
              'effort not reported'}
          </p>
        {/if}
        {#if d.preset || d.selection}
          <p class="event-meta">
            {#if d.preset}Preset: {d.preset}{/if}
            {#if d.preset && d.selection}
              ·
            {/if}
            {#if d.selection}Selection: {d.selection}{/if}
          </p>
        {/if}
        <p class="event-meta">
          Skills: {d.applied.skills.length > 0
            ? d.applied.skills
                .map((s) => `${s.name} r${s.revision ?? '?'}`)
                .join(', ')
            : 'none of your own'} · {d.applied.switched_off} installed skill(s) switched
          off
        </p>
        {#if d.verification.problems.length > 0}
          <p class="native-warning">{d.verification.problems.join('; ')}</p>
        {/if}
        {#if d.error}<p class="native-warning">{d.error}</p>{/if}
        {#if d.result}
          <details>
            <summary>Result</summary>
            <pre class="delegation-result">{d.result}</pre>
          </details>
        {/if}
        {#if d.artifacts.length > 0}
          <p class="event-meta">
            Files in <code>{d.workspace}</code>: {d.artifacts
              .map((a) => a.path)
              .join(', ')}
          </p>
        {/if}
      </li>
    {/each}
  </ul>
</section>

<style>
  .delegation-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .delegation {
    padding: 12px;
    border: 1px solid var(--glass-line);
    border-radius: var(--radius-sm);
    background: var(--surface);
  }
  .delegation-line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 0;
  }
  .delegation-result {
    font-family: var(--font-mono);
    font-size: 12px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 240px;
    overflow-y: auto;
  }
</style>
