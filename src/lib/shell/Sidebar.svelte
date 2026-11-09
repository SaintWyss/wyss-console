<script lang="ts">
  import StatusList from './StatusList.svelte';
  import type { AuthorityObservation } from './authority';
  import type { LifecycleObservation } from './lifecycle';
  import type { RuntimeSummary, RuntimeTone } from './runtime';
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
    ontogglegroup,
    runtime,
    authority,
    lifecycle,
    f8,
    ontogglerail,
  }: {
    active: ViewId;
    onselect: (id: ViewId) => void;
    /** Approvals waiting for a decision. Shown on their own item. */
    waiting?: number;
    /** What the sidebar offers. Runtimes is in it only while protocol 2 is active. */
    items?: readonly NavItem[];
    /** Collapsed to icons; it widens only when the owner clicks the button at its foot. */
    rail?: boolean;
    /** Groups the owner folded. A folded group opens while hovered and when it holds the view. */
    closed?: readonly NavGroup[];
    ontogglegroup?: (group: NavGroup) => void;
    runtime: RuntimeSummary;
    authority: AuthorityObservation;
    lifecycle: LifecycleObservation;
    f8: { label: string; tone: RuntimeTone; note: string | null } | null;
    ontogglerail?: () => void;
  } = $props();
  /** A group folded by a click stays folded under the pointer until the pointer leaves it. */
  let folded = $state<NavGroup | null>(null);
  function toggle(group: NavGroup) {
    const closing = !closed.includes(group);
    ontogglegroup?.(group);
    folded = closing ? group : null;
  }
  function choose(item: NavItem) {
    if (closed.includes(item.group)) ontogglegroup?.(item.group);
    onselect(item.id);
  }
  const sections = $derived(navSections(items));
</script>

<div class="sidebar-slot">
  <nav class="sidebar" aria-label="Primary" data-rail={rail}>
    <StatusList {runtime} {authority} {lifecycle} {f8} />
    <div class="nav-scroll">
      {#each sections as section (section.group)}
        <div
          class="nav-group"
          role="group"
          aria-labelledby={`nav-${section.group}`}
          data-open={!closed.includes(section.group)}
          data-folded={folded === section.group}
          onmouseleave={() => {
            if (folded === section.group) folded = null;
          }}
        >
          <button
            type="button"
            class="nav-section"
            id={`nav-${section.group}`}
            aria-expanded={!closed.includes(section.group)}
            onclick={() => toggle(section.group)}
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
                    onclick={() => choose(item)}
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
    </div>
    <div class="sidebar-foot">
      <button
        type="button"
        class="nav-item"
        aria-pressed={rail}
        title={rail ? 'Expand the sidebar' : 'Collapse the sidebar'}
        onclick={() => ontogglerail?.()}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"
          ><path d="M4 5h16v14H4zM9 5v14" /></svg
        >
        <span class="nav-label"
          >{rail ? 'Expand the sidebar' : 'Collapse the sidebar'}</span
        >
      </button>
    </div>
  </nav>
</div>
