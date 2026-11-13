<script lang="ts">
  import { t } from '../../i18n/index.svelte';
  import {
    DEFAULT_SPEC,
    hookLabel,
    isDefault,
    toggle,
    type EnvironmentOptions,
    type HomeSpec,
  } from './environment';
  import { arsenalClient } from '../../desktop/arsenal-client';

  const environmentClient = {
    get: (selection: string) => arsenalClient.environment(selection),
    options: () => arsenalClient.environmentOptions(),
    write: (selection: string, spec: HomeSpec) =>
      arsenalClient.writeEnvironment(selection, spec),
  };

  let {
    selection,
    client = environmentClient,
  }: { selection: string; client?: typeof environmentClient } = $props();

  let options = $state<EnvironmentOptions | null>(null);
  let saved = $state<HomeSpec>(DEFAULT_SPEC);
  let spec = $state<HomeSpec>(DEFAULT_SPEC);
  let failure = $state<string | null>(null);
  let saving = $state(false);

  $effect(() => {
    const id = selection;
    failure = null;
    options = null;
    void Promise.all([client.options(), client.get(id)])
      .then(([o, s]) => {
        options = o;
        saved = s;
        spec = s;
      })
      .catch(() => {
        failure = 'The launch environment could not be read.';
      });
  });

  const dirty = $derived(JSON.stringify(spec) !== JSON.stringify(saved));
  const own = $derived(!isDefault(spec));

  async function save() {
    saving = true;
    failure = null;
    try {
      await client.write(selection, spec);
      saved = spec;
    } catch {
      failure = 'The launch environment could not be saved.';
    } finally {
      saving = false;
    }
  }
</script>

<section class="environment" aria-labelledby="environment-title">
  <h4 id="environment-title">{t('Terminal environment')}</h4>
  {#if failure}
    <p class="error" role="alert">{t(failure)}</p>
  {/if}
  {#if options === null}
    {#if !failure}<p class="muted">{t('Loading…')}</p>{/if}
  {:else}
    <p class="help">
      {t(
        'What this preset loads besides its skills. Anything unticked is not loaded in that terminal, and costs no context.',
      )}
    </p>
    <fieldset>
      <legend>{t('Instructions')}</legend>
      <label
        ><input
          type="checkbox"
          checked={spec.instructions}
          onchange={(e) =>
            (spec = { ...spec, instructions: e.currentTarget.checked })}
        />
        CLAUDE.md / AGENTS.md</label
      >
      {#each options.rules as rule (rule)}
        <label class="indent"
          ><input
            type="checkbox"
            disabled={!spec.instructions}
            checked={spec.instructions && !spec.rules_off.includes(rule)}
            onchange={(e) =>
              (spec = {
                ...spec,
                rules_off: toggle(
                  spec.rules_off,
                  rule,
                  !e.currentTarget.checked,
                ),
              })}
          />
          rules/{rule}</label
        >
      {/each}
      <label
        ><input
          type="checkbox"
          checked={spec.memory}
          onchange={(e) =>
            (spec = { ...spec, memory: e.currentTarget.checked })}
        />
        {t('Auto memory')}</label
      >
    </fieldset>
    <fieldset>
      <legend>{t('Hooks')}</legend>
      {#each options.hooks as hook (hook.key)}
        <label title={hook.command}
          ><input
            type="checkbox"
            checked={!spec.hooks_off.includes(hook.key)}
            onchange={(e) =>
              (spec = {
                ...spec,
                hooks_off: toggle(
                  spec.hooks_off,
                  hook.key,
                  !e.currentTarget.checked,
                ),
              })}
          />
          {hookLabel(hook)}</label
        >
      {:else}
        <p class="muted">{t('No hooks in your settings.')}</p>
      {/each}
    </fieldset>
    <fieldset>
      <legend>MCP</legend>
      <label
        ><input
          type="checkbox"
          checked={spec.mcp === null}
          onchange={(e) =>
            (spec = {
              ...spec,
              mcp: e.currentTarget.checked ? null : [...options!.mcp].sort(),
            })}
        />
        {t('All servers, plugins and claude.ai connectors included')}</label
      >
      {#each options.mcp as server (server)}
        <label class="indent"
          ><input
            type="checkbox"
            disabled={spec.mcp === null}
            checked={spec.mcp === null || spec.mcp.includes(server)}
            onchange={(e) =>
              (spec = {
                ...spec,
                mcp: toggle(spec.mcp ?? [], server, e.currentTarget.checked),
              })}
          />
          {server}</label
        >
      {/each}
    </fieldset>
    <fieldset>
      <legend>{t('Plugins')}</legend>
      {#each options.plugins as plugin (plugin)}
        <label
          ><input
            type="checkbox"
            checked={!spec.plugins_off.includes(plugin)}
            onchange={(e) =>
              (spec = {
                ...spec,
                plugins_off: toggle(
                  spec.plugins_off,
                  plugin,
                  !e.currentTarget.checked,
                ),
              })}
          />
          {plugin}</label
        >
      {:else}
        <p class="muted">{t('No plugins enabled.')}</p>
      {/each}
    </fieldset>
    <p class="starts">
      {own
        ? t(
            'This terminal gets its own launch folder and signs in with your stored long-lived token. Your own ~/.claude is not changed.',
          )
        : t('This terminal uses your own configuration as it is.')}
    </p>
    <p class="muted">
      {t(
        "Always loaded: the project's own CLAUDE.md, skills and hooks, and Claude Code's built-in plugins.",
      )}
    </p>
    {#if own && !options.token_stored}
      <p class="error" role="alert">
        {t(
          'No long-lived token is stored, so this terminal will not start. Run claude setup-token in your own terminal and store it with: secret-tool store --label "Claude Code token (Wyss Console)" service wyss-console kind claude-oauth-token',
        )}
      </p>
    {/if}
    <button
      type="button"
      class="action"
      disabled={!dirty || saving}
      onclick={save}>{saving ? t('Saving…') : t('Save environment')}</button
    >
  {/if}
</section>

<style>
  .environment fieldset {
    border: 0;
    margin: 0 0 var(--space-3, 12px);
    padding: 0;
    display: grid;
    gap: var(--space-1, 4px);
  }
  .environment legend {
    font-weight: 600;
    margin-bottom: var(--space-1, 4px);
  }
  .environment label {
    display: flex;
    gap: var(--space-2, 8px);
    align-items: center;
    min-height: 28px;
    overflow-wrap: anywhere;
  }
  .environment .indent {
    padding-inline-start: var(--space-4, 16px);
  }
</style>
