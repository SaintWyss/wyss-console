<script lang="ts">
  import {
    NAV_ITEMS,
    navSections,
    type NavGroup,
    type NavItem,
    type ViewId,
  } from './nav';

  let {
    active,
    onselect,
    waiting = 0,
    items = NAV_ITEMS,
    rail = false,
    closed = [],
    ontogglerail,
    ontogglegroup,
  }: {
    active: ViewId;
    onselect: (id: ViewId) => void;
    /** Approvals waiting for a decision. Shown on their own item. */
    waiting?: number;
    /** What the sidebar offers. Runtimes is in it only while protocol 2 is active. */
    items?: readonly NavItem[];
    /** Collapsed to icons; it opens over the view while the pointer or focus is on it. */
    rail?: boolean;
    /** Groups the owner folded. A folded group opens while hovered and when it holds the view. */
    closed?: readonly NavGroup[];
    ontogglerail?: () => void;
    ontogglegroup?: (group: NavGroup) => void;
  } = $props();
  const sections = $derived(navSections(items));
  const holdsActive = (section: { items: readonly NavItem[] }) =>
    section.items.some((item) => item.id === active);
</script>

<div class="sidebar-slot">
  <nav class="sidebar" aria-label="Primary" data-rail={rail}>
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
    {#each sections as section (section.group)}
      <div
        class="nav-group"
        role="group"
        aria-labelledby={`nav-${section.group}`}
        data-open={!closed.includes(section.group) || holdsActive(section)}
      >
        <button
          type="button"
          class="nav-section"
          id={`nav-${section.group}`}
          aria-expanded={!closed.includes(section.group) ||
            holdsActive(section)}
          onclick={() => ontogglegroup?.(section.group)}
        >
          <span>{section.label}</span>
          <svg viewBox="0 0 24 24" aria-hidden="true"
            ><path d="M6 9l6 6 6-6" /></svg
          >
        </button>
        <div class="nav-items">
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
                    <span
                      class="nav-tag waiting"
                      aria-label={`${waiting} waiting`}>{waiting}</span
                    >
                  {/if}
                </button>
              </li>
            {/each}
          </ul>
        </div>
      </div>
    {/each}
  </nav>
</div>
