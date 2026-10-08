<script lang="ts">
  import { NAV_ITEMS, type ViewId } from './nav';

  let { active, onselect }: { active: ViewId; onselect: (id: ViewId) => void } =
    $props();
</script>

<nav class="sidebar" aria-label="Primary">
  <p class="nav-section">Workspace</p>
  <ul>
    {#each NAV_ITEMS as item (item.id)}
      <li>
        <button
          type="button"
          class="nav-item"
          aria-current={active === item.id ? 'page' : undefined}
          disabled={!item.available}
          title={item.available ? item.label : `${item.label} · coming soon`}
          onclick={() => onselect(item.id)}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true"
            ><path d={item.icon} /></svg
          >
          <span class="nav-label">{item.label}</span>
          {#if !item.available}<span class="nav-tag">Soon</span>{/if}
        </button>
      </li>
    {/each}
  </ul>
</nav>
