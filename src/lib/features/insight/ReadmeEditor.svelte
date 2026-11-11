<script lang="ts">
  import type { Readme } from '../../contracts/insight';
  import {
    ProjectsCallError,
    type ProjectsClient,
  } from '../../desktop/projects-client';
  import { renderMarkdown } from './markdown';

  let {
    client,
    projectId,
    readme,
    onSaved,
  }: {
    client: ProjectsClient;
    projectId: string;
    readme: Readme | null;
    onSaved: (content: string) => void;
  } = $props();

  type Mode = 'view' | 'edit' | 'split';
  let mode = $state<Mode>('split');
  let text = $state('');
  let saved = $state('');
  let preview = $state('');
  let busy = $state(false);
  let notice = $state<string | null>(null);

  // The README on disk is the baseline; a new project or a re-analysis resets the editor.
  $effect(() => {
    const content = readme?.content ?? '';
    text = content;
    saved = content;
    notice = null;
  });

  // The preview follows each key after 100 ms, so a large README does not lag the typing.
  $effect(() => {
    const current = text;
    const timer = setTimeout(() => (preview = renderMarkdown(current)), 100);
    return () => clearTimeout(timer);
  });

  const name = $derived(readme?.name ?? 'README.md');
  const dirty = $derived(text !== saved);
  const locked = $derived(readme?.truncated ?? false);
  const canSave = $derived(!busy && !locked && (readme === null || dirty));

  async function save() {
    if (!canSave) return;
    busy = true;
    notice = null;
    try {
      const written = await client.saveReadme(projectId, text);
      saved = text;
      notice = `Guardado en ${written}.`;
      onSaved(text);
    } catch (error) {
      const code =
        error instanceof ProjectsCallError ? error.failure.code : 'io_failure';
      notice = `No se pudo guardar: ${code.replace(/_/g, ' ')}.`;
    } finally {
      busy = false;
    }
  }
</script>

<div class="insight-readme">
  <div class="insight-readme-bar">
    <div class="insight-tabs" role="group" aria-label="Vista">
      <button
        type="button"
        class="action"
        aria-pressed={mode === 'view'}
        onclick={() => (mode = 'view')}>Vista</button
      >
      <button
        type="button"
        class="action"
        aria-pressed={mode === 'edit'}
        onclick={() => (mode = 'edit')}>Editar</button
      >
      <button
        type="button"
        class="action"
        aria-pressed={mode === 'split'}
        onclick={() => (mode = 'split')}>Dividido</button
      >
    </div>
    <span class="projects-note" role="status">
      {#if readme === null}
        Este proyecto no tiene README.md.
      {:else if dirty}
        Sin guardar
      {:else}
        {name}
      {/if}
    </span>
    <button
      type="button"
      class="action primary"
      disabled={!canSave}
      onclick={() => void save()}
    >
      {readme === null ? 'Crear README.md' : 'Guardar'}
    </button>
  </div>

  {#if locked}
    <p class="projects-note" role="alert">
      El README es demasiado grande para editarlo aquí; se muestra completo pero
      no se guarda, para no perder contenido.
    </p>
  {/if}
  {#if notice}
    <p class="projects-note" role="status">{notice}</p>
  {/if}

  <div class="insight-readme-panes" data-mode={mode}>
    {#if mode !== 'view'}
      <textarea
        class="insight-readme-source"
        aria-label={`Contenido de ${name}`}
        spellcheck="false"
        readonly={locked}
        bind:value={text}
        onkeydown={(event) => {
          if (
            (event.ctrlKey || event.metaKey) &&
            event.key.toLowerCase() === 's'
          ) {
            event.preventDefault();
            void save();
          }
        }}></textarea>
    {/if}
    {#if mode !== 'edit'}
      <div class="md insight-readme-preview" aria-label="Vista previa">
        <!-- eslint-disable-next-line svelte/no-at-html-tags -- renderMarkdown escapes all source text -->
        {@html preview}
      </div>
    {/if}
  </div>
</div>
