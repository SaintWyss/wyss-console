<script lang="ts">
  import type { TypeDecl } from '../../contracts/insight';
  import {
    RELATED_BY_DEFAULT_ABOVE,
    filterTypes,
    headerText,
    layout,
    relations,
    toMermaid,
    toPlantUml,
  } from './uml';

  let { types }: { types: TypeDecl[] } = $props();

  let folder = $state('');
  let text = $state('');
  /** null: the default for the project size; otherwise the user's choice. */
  let relatedChoice = $state<boolean | null>(null);
  let picked = $state<number | null>(null);
  let notice = $state<string | null>(null);
  let svgEl = $state<SVGSVGElement | null>(null);

  const folders = $derived(
    [
      ...new Set(
        types.map((t) =>
          t.file.includes('/') ? t.file.slice(0, t.file.lastIndexOf('/')) : '',
        ),
      ),
    ]
      .filter((f) => f !== '')
      .sort(),
  );
  const effectiveRelated = $derived(
    relatedChoice ?? types.length > RELATED_BY_DEFAULT_ABOVE,
  );
  const shown = $derived(
    filterTypes(types, {
      folder,
      text,
      onlyRelated: relatedChoice ?? undefined,
    }),
  );
  const rels = $derived(relations(shown));
  const lay = $derived(layout(shown, rels));
  const mermaid = $derived(toMermaid(shown));
  const plantuml = $derived(toPlantUml(shown));
  const selected = $derived(picked !== null ? (shown[picked] ?? null) : null);

  // The view: a window onto the layout. It starts at the full diagram and moves by pan or zoom.
  // It resets to the full diagram whenever the layout changes; pan and zoom then assign it.
  let view = $derived({ x: 0, y: 0, w: lay.width, h: lay.height });
  let drag: { x: number; y: number } | null = null;

  function zoom(factor: number, cx: number, cy: number) {
    const w = Math.min(
      Math.max(view.w * factor, lay.width * 0.1),
      lay.width * 6,
    );
    const scale = w / view.w;
    const h = view.h * scale;
    view = {
      x: cx - (cx - view.x) * scale,
      y: cy - (cy - view.y) * scale,
      w,
      h,
    };
  }

  // A wheel listener that may cancel the page scroll (Svelte's own wheel handler cannot).
  $effect(() => {
    const el = svgEl;
    if (!el) return;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      const rect = el.getBoundingClientRect();
      const cx = view.x + ((event.clientX - rect.left) / rect.width) * view.w;
      const cy = view.y + ((event.clientY - rect.top) / rect.height) * view.h;
      zoom(event.deltaY < 0 ? 0.9 : 1.1, cx, cy);
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  });

  function onPointerDown(event: PointerEvent) {
    drag = { x: event.clientX, y: event.clientY };
    (event.currentTarget as Element).setPointerCapture(event.pointerId);
  }
  function onPointerMove(event: PointerEvent) {
    if (!drag || !svgEl) return;
    const rect = svgEl.getBoundingClientRect();
    const dx = ((event.clientX - drag.x) / rect.width) * view.w;
    const dy = ((event.clientY - drag.y) / rect.height) * view.h;
    drag = { x: event.clientX, y: event.clientY };
    view = { ...view, x: view.x - dx, y: view.y - dy };
  }
  function onPointerUp() {
    drag = null;
  }

  async function copy(content: string, what: string) {
    try {
      await navigator.clipboard.writeText(content);
      notice = `${what} copiado.`;
    } catch {
      notice = `No se pudo copiar ${what}.`;
    }
  }

  /** The whole diagram as an SVG file, with its colours written in (no page styles travel). */
  function downloadSvg() {
    const source = svgEl;
    if (!source) return;
    const clone = source.cloneNode(true) as SVGSVGElement;
    clone.setAttribute('viewBox', `0 0 ${lay.width} ${lay.height}`);
    clone.setAttribute('width', String(lay.width));
    clone.setAttribute('height', String(lay.height));
    const style = document.createElementNS(
      'http://www.w3.org/2000/svg',
      'style',
    );
    style.textContent =
      'rect{fill:#1c1c1c;stroke:#8a8a8a}.uml-line{stroke:#8a8a8a;fill:none}text{fill:#ffffff;font-family:sans-serif;font-size:12px}.uml-muted{fill:#c4c4c4}';
    clone.prepend(style);
    const blob = new Blob([new XMLSerializer().serializeToString(clone)], {
      type: 'image/svg+xml',
    });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = 'uml.svg';
    link.click();
    URL.revokeObjectURL(url);
  }

  function pick(index: number) {
    picked = picked === index ? null : index;
  }
</script>

<div class="insight-uml">
  <div class="insight-uml-bar">
    <label class="projects-note">
      Carpeta
      <select bind:value={folder}>
        <option value="">Todo el proyecto</option>
        {#each folders as f (f)}
          <option value={f}>{f}</option>
        {/each}
      </select>
    </label>
    <label class="projects-note">
      Buscar
      <input type="text" bind:value={text} placeholder="Nombre o archivo" />
    </label>
    <label class="projects-check">
      <input
        type="checkbox"
        checked={effectiveRelated}
        onchange={(event) => (relatedChoice = event.currentTarget.checked)}
      />
      Solo tipos con relaciones
    </label>
  </div>

  <div class="insight-uml-bar">
    <button
      type="button"
      class="action"
      onclick={() => void copy(mermaid, 'Mermaid')}>Copiar Mermaid</button
    >
    <button
      type="button"
      class="action"
      onclick={() => void copy(plantuml, 'PlantUML')}>Copiar PlantUML</button
    >
    <button
      type="button"
      class="action"
      onclick={downloadSvg}
      disabled={shown.length === 0}>Descargar SVG</button
    >
    <button
      type="button"
      class="action"
      onclick={() => (view = { x: 0, y: 0, w: lay.width, h: lay.height })}
    >
      Ajustar
    </button>
    <span class="projects-note" role="status">
      {shown.length} tipos · {rels.length} relaciones{notice
        ? ` · ${notice}`
        : ''}
    </span>
  </div>

  {#if shown.length === 0}
    <p class="projects-note">No hay tipos que mostrar con estos filtros.</p>
  {:else}
    <svg
      bind:this={svgEl}
      class="insight-uml-canvas"
      role="img"
      aria-label="Diagrama de clases"
      viewBox={`${view.x} ${view.y} ${view.w} ${view.h}`}
      onpointerdown={onPointerDown}
      onpointermove={onPointerMove}
      onpointerup={onPointerUp}
      onpointercancel={onPointerUp}
    >
      <defs>
        <marker
          id="uml-extends"
          viewBox="0 0 10 10"
          refX="10"
          refY="5"
          markerWidth="10"
          markerHeight="10"
          orient="auto-start-reverse"
        >
          <path d="M 0 0 L 10 5 L 0 10 z" class="uml-arrow-head" />
        </marker>
        <marker
          id="uml-implements"
          viewBox="0 0 10 10"
          refX="10"
          refY="5"
          markerWidth="10"
          markerHeight="10"
          orient="auto-start-reverse"
        >
          <path d="M 0 0 L 10 5 L 0 10 z" class="uml-arrow-head" />
        </marker>
      </defs>

      {#each rels as r, n (n)}
        {@const child = lay.boxes[r.from]}
        {@const parent = lay.boxes[r.to]}
        <line
          class="uml-line"
          class:uml-dashed={r.kind === 'implements'}
          x1={child.x + child.w / 2}
          y1={child.y}
          x2={parent.x + parent.w / 2}
          y2={parent.y + parent.h}
          marker-end={r.kind === 'extends'
            ? 'url(#uml-extends)'
            : 'url(#uml-implements)'}
        />
      {/each}

      {#each lay.boxes as box (box.index)}
        {@const t = shown[box.index]}
        <g
          class="uml-box"
          class:uml-picked={picked === box.index}
          role="button"
          tabindex="0"
          aria-label={`${headerText(t)}, ${t.file}`}
          onclick={() => pick(box.index)}
          onkeydown={(event) => {
            if (event.key === 'Enter' || event.key === ' ') {
              event.preventDefault();
              pick(box.index);
            }
          }}
        >
          <rect x={box.x} y={box.y} width={box.w} height={box.h} rx="4" />
          <text x={box.x + 8} y={box.y + 16} font-weight="600"
            >{headerText(t)}</text
          >
          <line
            class="uml-line"
            x1={box.x}
            x2={box.x + box.w}
            y1={box.y + 24}
            y2={box.y + 24}
          />
          {#each [...t.fields, ...t.methods] as member, m (m)}
            <text
              class="uml-muted"
              x={box.x + 8}
              y={box.y + 24 + 16 * (m + 1) - 3}>{member}</text
            >
          {/each}
        </g>
      {/each}
    </svg>
  {/if}

  {#if selected}
    <p class="projects-note" role="status">
      <code>{selected.file}</code> · {selected.language}
    </p>
  {/if}
</div>
