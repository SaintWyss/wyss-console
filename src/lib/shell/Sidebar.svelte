<script lang="ts">
  import { NAV_ITEMS, navSections, type NavItem, type ViewId } from './nav';

  let {
    active,
    onselect,
    waiting = 0,
    items = NAV_ITEMS,
  }: {
    active: ViewId;
    onselect: (id: ViewId) => void;
    /** Approvals waiting for a decision. Shown on their own item. */
    waiting?: number;
    /** What the sidebar offers. Runtimes is in it only while protocol 2 is active. */
    items?: readonly NavItem[];
  } = $props();
  const sections = $derived(navSections(items));
</script>

<nav class="sidebar" aria-label="Primary">
  {#each sections as section (section.group)}
    <div
      class="nav-group"
      role="group"
      aria-labelledby={`nav-${section.group}`}
    >
      <p class="nav-section" id={`nav-${section.group}`}>{section.label}</p>
      <ul>
        {#each section.items as item (item.id)}
          <li>
            <button
              type="button"
              class="nav-item"
              aria-current={active === item.id ? 'page' : undefined}
              title={`${item.label} · ${item.subtitle}`}
              onclick={() => onselect(item.id)}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true"
                ><path d={item.icon} /></svg
              >
              <span class="nav-label">{item.label}</span>
              {#if item.id === 'approvals' && waiting > 0}
                <span class="nav-tag waiting" aria-label={`${waiting} waiting`}
                  >{waiting}</span
                >
              {/if}
            </button>
          </li>
        {/each}
      </ul>
    </div>
  {/each}
</nav>
