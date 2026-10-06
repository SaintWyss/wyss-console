<script lang="ts">
  import { runPtySmoke, type SmokeResult } from './smoke';
  let running = $state(false);
  let preview = $state('');
  let result = $state<SmokeResult>();
  let failed = $state(false);
  async function run() {
    running = true;
    failed = false;
    result = undefined;
    preview = '';
    try {
      result = await runPtySmoke((text) => (preview = text));
    } catch {
      failed = true;
    } finally {
      running = false;
    }
  }
</script>

<details>
  <summary>PTY diagnostic — Sprint 1A</summary>
  <p>
    Temporary backend smoke test. No terminal renderer. Preview capped at 8,192
    characters.
  </p>
  <button onclick={run} disabled={running}>Run PTY smoke</button>
  <p>
    {running
      ? 'Running PTY smoke…'
      : failed
        ? 'PTY smoke failed; backend cleanup requested.'
        : result
          ? 'PTY smoke PASS — create, input, output, resize, close.'
          : 'No PTY created.'}
  </p>
  {#if result}
    <p>
      Shell: {result.info.shell}<br />Cwd: {result.info.cwd}<br />PID
      (observed): {result.info.observed_pid}<br />TerminalId: {result.info
        .terminal_id}<br />State: {result.info.state}; bytes received: {result.bytes}
    </p>
  {/if}
  {#if preview}<pre>{preview}</pre>{/if}
</details>
