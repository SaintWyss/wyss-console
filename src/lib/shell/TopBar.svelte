<script lang="ts">
  let {
    title,
    subtitle,
    rail = false,
    ontogglerail,
  }: {
    title: string;
    subtitle: string;
    /** The sidebar is collapsed to icons. */
    rail?: boolean;
    ontogglerail?: () => void;
  } = $props();
</script>

<header class="top-bar">
  <div class="brand">
    <!-- The product's name stays for screen readers; the window's own title bar shows it. -->
    <div class="visually-hidden"><h1>Wyss Console</h1></div>
    <button
      type="button"
      class="sidebar-toggle"
      aria-label={rail ? 'Pin the sidebar open' : 'Collapse the sidebar'}
      aria-pressed={rail}
      title={rail ? 'Pin the sidebar open' : 'Collapse the sidebar'}
      onclick={() => ontogglerail?.()}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true"
        ><path d="M4 5h16v14H4zM9 5v14" /></svg
      >
    </button>
  </div>
  <div class="context">
    <p class="context-title">{title}</p>
    <p class="context-subtitle">{subtitle}</p>
  </div>
</header>
