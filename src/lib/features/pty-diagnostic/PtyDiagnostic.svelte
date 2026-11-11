<script lang="ts">
  import { t } from '../../i18n/index.svelte';
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
  <summary>{t('PTY diagnostic — Sprint 1A')}</summary>
  <p>
    {t(
      'Temporary backend smoke test. No terminal renderer. Preview capped at 8,192 characters.',
    )}
  </p>
  <button onclick={run} disabled={running}>{t('Run PTY smoke')}</button>
  <p>
    {running
      ? t('Running PTY smoke…')
      : failed
        ? t('PTY smoke failed; backend cleanup requested.')
        : result
          ? t('PTY smoke PASS — create, input, output, resize, close.')
          : t('No PTY created.')}
  </p>
  {#if result}
    <!-- prettier-ignore -->
    <p>{t('Shell:')} {result.info.shell}<br />{t('Cwd:')} {result.info.cwd}<br />{t('PID (observed):')} {result.info.observed_pid}<br />{t('TerminalId:')} {result.info.terminal_id}<br />{t('State:')} {result.info.state}; {t('bytes received:')} {result.bytes}</p>
  {/if}
  {#if preview}<pre>{preview}</pre>{/if}
</details>
