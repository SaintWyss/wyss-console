<script lang="ts">
  import type { Insight } from '../../contracts/insight';
  import {
    ProjectsCallError,
    type ProjectsClient,
  } from '../../desktop/projects-client';
  import ReadmeEditor from './ReadmeEditor.svelte';
  import UmlDiagram from './UmlDiagram.svelte';

  let {
    client,
    projectId,
    onClose,
  }: {
    client: ProjectsClient;
    projectId: string;
    onClose: () => void;
  } = $props();

  const TABS = [
    { id: 'summary', label: 'Resumen' },
    { id: 'tech', label: 'Tecnologías' },
    { id: 'arch', label: 'Arquitectura' },
    { id: 'uml', label: 'UML' },
    { id: 'readme', label: 'README' },
  ] as const;
  type TabId = (typeof TABS)[number]['id'];
  let tab = $state<TabId>('summary');

  let insight = $state<Insight | null>(null);
  let loading = $state(false);
  let failure = $state<string | null>(null);
  let run = 0;

  async function analyze() {
    const turn = ++run;
    loading = true;
    failure = null;
    try {
      const next = await client.insight(projectId);
      if (turn === run) insight = next;
    } catch (error) {
      if (turn === run) {
        insight = null;
        failure =
          error instanceof ProjectsCallError
            ? error.failure.code
            : 'io_failure';
      }
    } finally {
      if (turn === run) loading = false;
    }
  }

  // Each project (or a new analysis request) starts from the desktop again.
  $effect(() => {
    void projectId;
    void analyze();
  });

  // Languages that are data or prose, not code: they stay out of the language bar.
  const NOT_CODE = ['Markdown', 'JSON', 'YAML'];
  const BAR_COLORS = [
    '--accent',
    '--ok',
    '--pending',
    '--warn',
    '--accent-strong',
    '--muted',
  ];
  const CATEGORY: Record<string, string> = {
    language: 'Lenguajes',
    framework: 'Frameworks',
    library: 'Librerías',
    tooling: 'Herramientas',
    testing: 'Testing',
    database: 'Base de datos',
    infrastructure: 'Infraestructura',
    ci: 'CI',
  };

  function size(n: number): string {
    if (n < 1024) return `${n} B`;
    let value = n / 1024;
    const units = ['KB', 'MB', 'GB'];
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) {
      value /= 1024;
      unit++;
    }
    return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
  }

  const barLanguages = $derived(
    (insight?.languages ?? []).filter((l) => !NOT_CODE.includes(l.name)),
  );
  const barTotal = $derived(barLanguages.reduce((sum, l) => sum + l.bytes, 0));
  const groups = $derived(
    Object.keys(CATEGORY)
      .map((id) => ({
        id,
        label: CATEGORY[id],
        items: (insight?.technologies ?? []).filter((t) => t.category === id),
      }))
      .filter((g) => g.items.length > 0),
  );
</script>

<section class="insight-panel" aria-label="Detalles del proyecto">
  <header class="insight-head">
    <h3>{insight?.name ?? 'Detalles del proyecto'}</h3>
    <div class="projects-row">
      <button
        type="button"
        class="action"
        disabled={loading}
        onclick={() => void analyze()}
      >
        Volver a analizar
      </button>
      <button type="button" class="action" onclick={onClose}>Cerrar</button>
    </div>
  </header>

  {#if loading && !insight}
    <p class="projects-note" role="status" aria-busy="true">
      Analizando la carpeta…
    </p>
  {:else if failure}
    <div class="projects-banner" data-kind="error" role="alert">
      <span>No se pudo analizar el proyecto: {failure.replace(/_/g, ' ')}.</span
      >
    </div>
  {:else if insight}
    <div class="insight-tabs" role="tablist" aria-label="Secciones">
      {#each TABS as t (t.id)}
        <button
          type="button"
          role="tab"
          class="action"
          aria-selected={tab === t.id}
          onclick={() => (tab = t.id)}
        >
          {t.label}
        </button>
      {/each}
    </div>

    {#if tab === 'summary'}
      {#if insight.truncated}
        <p class="projects-banner" data-kind="info" role="status">
          La carpeta es muy grande: los conteos son un mínimo.
        </p>
      {/if}
      <dl class="insight-cards">
        <div>
          <dt>Archivos</dt>
          <dd>{insight.files}</dd>
        </div>
        <div>
          <dt>Carpetas</dt>
          <dd>{insight.folders}</dd>
        </div>
        <div>
          <dt>Tamaño</dt>
          <dd>{size(insight.bytes)}</dd>
        </div>
        <div>
          <dt>Git</dt>
          <dd>{insight.git ? 'Sí' : 'No'}</dd>
        </div>
        <div>
          <dt>Licencia</dt>
          <dd>{insight.license ?? 'Sin licencia detectada'}</dd>
        </div>
      </dl>
      <h4>Estructura</h4>
      <ul class="insight-list">
        {#each insight.structure as e (e.name)}
          <li>
            {e.dir ? 'Carpeta' : 'Archivo'} · {e.name}{e.dir
              ? ` (${e.files} archivos)`
              : ''}
          </li>
        {:else}
          <li class="projects-note">Vacía.</li>
        {/each}
      </ul>
      <h4>Scripts</h4>
      {#if insight.scripts.length === 0}
        <p class="projects-note">No se declararon scripts.</p>
      {:else}
        <table class="insight-table">
          <thead><tr><th>Fuente</th><th>Nombre</th><th>Comando</th></tr></thead>
          <tbody>
            {#each insight.scripts as s (s.source + s.name)}
              <tr
                ><td>{s.source}</td><td>{s.name}</td><td
                  ><code>{s.command}</code></td
                ></tr
              >
            {/each}
          </tbody>
        </table>
      {/if}
    {:else if tab === 'tech'}
      <h4>Lenguajes</h4>
      {#if barTotal > 0}
        <div class="insight-bar" role="img" aria-label="Lenguajes por tamaño">
          {#each barLanguages as l, n (l.name)}
            <span
              style={`width: ${(l.bytes / barTotal) * 100}%; background: var(${BAR_COLORS[n % BAR_COLORS.length]});`}
              title={`${l.name}: ${l.files} archivos, ${size(l.bytes)}`}
            ></span>
          {/each}
        </div>
        <ul class="insight-legend">
          {#each barLanguages as l (l.name)}
            <li>
              {l.name} · {l.files} archivos · {(
                (l.bytes / barTotal) *
                100
              ).toFixed(1)}%
            </li>
          {/each}
        </ul>
      {:else}
        <p class="projects-note">No hay archivos de código.</p>
      {/if}
      {#each groups as g (g.id)}
        <h4>{g.label}</h4>
        <div class="insight-chips">
          {#each g.items as t (t.name + t.source)}
            <span class="insight-chip" title={t.source}>
              {t.name}{t.version ? ` ${t.version}` : ''}
            </span>
          {/each}
        </div>
      {:else}
        <p class="projects-note">
          No se detectaron tecnologías en los manifiestos.
        </p>
      {/each}
    {:else if tab === 'arch'}
      {#each insight.architecture as p (p.name)}
        <article class="insight-pattern">
          <header>
            <strong>{p.name}</strong>
            <span class="projects-note">{p.confidence}%</span>
          </header>
          <div class="insight-meter" aria-hidden="true">
            <span style={`width: ${p.confidence}%`}></span>
          </div>
          <p>{p.description}</p>
          <ul class="insight-list">
            {#each p.evidence as e (e)}<li>{e}</li>{/each}
          </ul>
        </article>
      {:else}
        <p class="projects-note">No se detectó un patrón claro.</p>
      {/each}
    {:else if tab === 'uml'}
      <UmlDiagram types={insight.types} />
    {:else if tab === 'readme'}
      <ReadmeEditor
        {client}
        {projectId}
        readme={insight.readme}
        onSaved={(content) => {
          if (insight)
            insight = {
              ...insight,
              readme: {
                name: insight.readme?.name ?? 'README.md',
                content,
                truncated: false,
              },
            };
        }}
      />
    {/if}
  {/if}
</section>
