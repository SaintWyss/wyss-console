<script lang="ts">
  import { onMount } from 'svelte';
  import type { SessionView } from '../../contracts/projects';
  import {
    ProjectsCallError,
    type ProjectsClient,
  } from '../../desktop/projects-client';
  import { createRenderer, type TerminalRenderer } from '../terminal/renderer';
  import { InputQueue, inputReportWords } from './input-queue';

  let {
    session,
    client,
    title,
    ondetach,
    onrefresh,
    claimOnOpen = false,
  }: {
    session: SessionView;
    client: ProjectsClient;
    title: string;
    ondetach: () => void;
    onrefresh: () => void;
    /** The window started this session a moment ago: it takes input as the pane opens. */
    claimOnOpen?: boolean;
  } = $props();

  let host = $state<HTMLDivElement>();
  // What reading says, and what taking input says: a good read never hides a refusal.
  let note = $state('');
  let inputNote = $state('');
  let truncated = $state(false);
  let holding = $state(false);
  // How this pane came to show a session that was already running: its screen, or a replay of
  // what the host kept, with the reason. Said, never implied.
  let recovery = $state('');
  const REPLAY: Record<string, string> = {
    host_without_screen: 'this host keeps no screen',
    helper_missing: 'the screen helper is not running',
    too_large: 'the screen was too large to send',
    timeout: 'the screen helper did not answer in time',
    no_answer: 'the host did not answer',
    invalid_screen: 'the screen could not be used',
    not_drawn: 'the screen could not be drawn here',
    commit_refused: 'the host or session changed while it was drawn',
    attach_failed: 'the screen could not be asked for',
  };
  // A terminal is drawn for Native and shell sessions. A Managed session has none: it shows what
  // the authority says, and its input stays with the Managed screens.
  const terminal = $derived(session.kind !== 'managed');
  const live = $derived(session.availability === 'live');
  const WORDS: Record<string, string> = {
    host_changed: 'The terminal host is not the one this session lived in.',
    host_offline: 'No terminal host answers.',
    owner_refused: 'The terminal host refused that.',
    unsupported_capability: 'Not available for this session.',
  };

  const encoder = new TextEncoder();
  const binary = (data: string) =>
    Uint8Array.from(data, (c) => c.charCodeAt(0) & 0xff);

  onMount(() => {
    if (!host || !terminal) return;
    let stopped = false;
    let renderer: TerminalRenderer;
    try {
      renderer = createRenderer(host);
    } catch {
      note = 'The terminal could not be drawn.';
      return;
    }
    renderer.setInput(false);
    let from = 0;
    let unlisten: (() => void) | undefined;
    let size: { rows: number; cols: number } | undefined;

    const words = (error: unknown, fallback: string) => {
      const failure = error instanceof ProjectsCallError ? error.failure : null;
      // A refused claim is its own thing: someone else types, and this view mirrors.
      if (failure?.field === 'input_claim')
        return 'Another window holds input.';
      return WORDS[failure?.code ?? ''] ?? fallback;
    };
    const say = (error: unknown) => {
      note = words(error, 'Reading stopped.');
    };
    const sayInput = (error: unknown) => {
      inputNote = words(error, 'Input was not sent.');
    };

    // The screen is drawn before reading on, through the checked owner. The read position moves
    // only when the backend answers the commit: a screen that was not drawn, or that the backend
    // no longer vouches for, is cleared and what the host kept is read from its start.
    async function recover() {
      const replay = (reason: string) => {
        from = 0;
        recovery = `Replayed from the output the host kept (${REPLAY[reason] ?? reason}). A full-screen program may look incomplete until it redraws.`;
      };
      let attach;
      try {
        attach = await client.attachScreen(session.id);
      } catch {
        // No screen could be asked for (host offline or changed): reading says why, below.
        if (!stopped) replay('attach_failed');
        return;
      }
      if (stopped) return;
      if (attach.mode !== 'screen') {
        await client.commitScreen(session.id, null, false).catch(() => 0);
        if (!stopped) replay(attach.reason);
        return;
      }
      const drawn = await Promise.resolve(
        renderer.restore?.(attach.repaint, attach.size) ?? false,
      ).catch(() => false);
      if (stopped) return;
      try {
        from = await client.commitScreen(session.id, attach.token, drawn);
        if (stopped) return;
        if (drawn) recovery = '';
        else replay('not_drawn');
      } catch {
        if (stopped) return;
        // Drawn, but not committed: reading on from anywhere would repeat or skip output.
        if (drawn)
          await renderer
            .restore?.(new Uint8Array(0), attach.size)
            .catch(() => false);
        replay('commit_refused');
      }
    }

    // One long-poll per pane that is on screen; the host keeps the scrollback.
    async function read() {
      await recover();
      while (!stopped) {
        try {
          const out = await client.readOutput(session.id, from, 1000);
          if (stopped) return;
          note = '';
          truncated = out.truncated;
          if (out.data.length > 0) await renderer.write(out.data);
          from = out.next;
          if (out.eof) {
            note = 'The session ended.';
            return;
          }
        } catch (error) {
          if (stopped) return;
          say(error);
          await new Promise((resolve) => setTimeout(resolve, 3000));
        }
      }
    }

    const fit = () => {
      const next = renderer.fit();
      if (!next) return;
      const changed =
        !size || size.rows !== next.rows || size.cols !== next.cols;
      size = next;
      // Only the window that holds input sets the terminal's size; a mirror just fits itself.
      if (holding && changed)
        void client.resize(session.id, next.rows, next.cols).catch(sayInput);
    };
    const observer = new ResizeObserver(fit);
    observer.observe(host);
    fit();

    // Taking input is explicit, and it is let go of when the window loses the keyboard.
    // Keystrokes reach the terminal in the order they were typed: one call of at most 4 KiB at
    // a time, at most 64 KiB waiting with it, and nothing is ever sent twice (input-queue.ts).
    const queue = new InputQueue(
      (piece) => client.writeInput(session.id, piece),
      (report) => {
        if (stopped) return;
        inputNote = inputReportWords(
          report,
          report.kind === 'not_sent'
            ? words(report.error, 'The terminal host refused the input.')
            : '',
        );
        // A refusal can mean the claim is gone: what the backend says now is read again.
        if (report.kind === 'not_sent') onrefresh();
      },
    );
    const send = (bytes: Uint8Array) => void queue.push(bytes);
    // One claim at a time: a second click or the open-time claim while one is in flight is the
    // same claim, so the keyboard is wired once and a keystroke is sent once.
    let claiming = false;
    holdInput = async () => {
      if (claiming || holding || stopped) return;
      claiming = true;
      try {
        await client.claimInput(session.id);
      } catch (error) {
        claiming = false;
        if (!stopped) {
          sayInput(error);
          onrefresh();
        }
        return;
      }
      claiming = false;
      if (stopped) {
        // The pane went away while the claim was in flight: give it straight back.
        void client.releaseInput(session.id).catch(() => undefined);
        return;
      }
      holding = true;
      inputNote = '';
      renderer.setInput(true);
      unlisten = renderer.input(
        (text) => send(encoder.encode(text)),
        (data) => send(binary(data)),
      );
      // The terminal's size is set by whoever holds input, so send it again on taking it.
      size = undefined;
      fit();
      renderer.focus();
      onrefresh();
    };
    letGo = async () => {
      if (!holding) return;
      holding = false;
      unlisten?.();
      unlisten = undefined;
      renderer.setInput(false);
      // What was typed and not yet sent belongs to this hold: a later one never sends it.
      queue.reset();
      await client.releaseInput(session.id).catch(() => undefined);
      if (!stopped) onrefresh();
    };
    const blur = () => void letGo();
    window.addEventListener('blur', blur);
    void read();
    if (claimOnOpen) void holdInput();
    return () => {
      queue.reset();
      stopped = true;
      window.removeEventListener('blur', blur);
      if (holding) void client.releaseInput(session.id).catch(() => undefined);
      unlisten?.();
      observer.disconnect();
      renderer.dispose();
    };
  });
  let holdInput: () => Promise<void> = async () => undefined;
  let letGo: () => Promise<void> = async () => undefined;
  const KIND: Record<string, string> = {
    native: 'Agent',
    shell: 'Shell',
    managed: 'Managed',
  };
</script>

<div
  class="projects-session"
  data-kind={session.kind}
  data-availability={session.availability}
>
  <div class="projects-session-bar">
    <span class="projects-session-title">{title}</span>
    <span class="pill compact" data-tone={live ? 'ok' : 'idle'}>
      <span class="dot" aria-hidden="true"></span>{KIND[session.kind]} ·
      {session.availability}{session.reason
        ? ` (${session.reason.replace('_', ' ')})`
        : ''}
    </span>
    {#if terminal && live}
      {#if holding}
        <button type="button" class="action" onclick={() => void letGo()}>
          Release input
        </button>
      {:else if session.input === 'other'}
        <span class="projects-note">Another window has input</span>
      {:else}
        <button type="button" class="action" onclick={() => void holdInput()}>
          Take input
        </button>
      {/if}
    {/if}
    <button type="button" class="action" onclick={ondetach}>Detach</button>
  </div>
  {#if terminal}
    <div
      class="projects-terminal"
      bind:this={host}
      aria-label={`${title} terminal`}
    ></div>
    {#if recovery}<p class="projects-note" role="status">{recovery}</p>{/if}
    {#if truncated}<p class="projects-note">
        Earlier output is no longer held.
      </p>{/if}
    {#if !holding}<p class="projects-note">
        {live
          ? 'Read-only view. Input is exclusive; take it to type.'
          : 'Read-only.'}
      </p>{/if}
  {:else}
    <dl class="projects-facts">
      <dt>Authority</dt>
      <dd>
        Protocol {session.protocol} · stream
        <span class="mono">{session.storeStreamId?.slice(0, 8)}</span>
      </dd>
      <dt>Session</dt>
      <dd class="mono">{session.managedSessionId?.slice(0, 8)}</dd>
      <dt>Requested profile</dt>
      <dd>{session.requestedSafetyProfile ?? 'not recorded'}</dd>
      <dt>Attached profile</dt>
      <dd>{session.effectiveSafetyProfile ?? 'not attached or unknown'}</dd>
    </dl>
    {#if session.profileMismatch}
      <p class="projects-warn" role="alert">
        The authority attached a different profile than the one requested. The
        request grants nothing; open the Managed screens to decide.
      </p>
    {/if}
    <p class="projects-note">
      A Managed session is opened and run in Sessions, Agents and Runtimes.
      Showing it here starts nothing and grants nothing.
    </p>
  {/if}
  {#if note}<p class="projects-note" role="status">{note}</p>{/if}
  {#if inputNote}<p class="projects-note" role="alert">{inputNote}</p>{/if}
</div>
