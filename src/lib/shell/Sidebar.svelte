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
    version,
    canSplit = false,
    onsplit,
    legend = false,
    ontogglelegend,
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
    ontogglegroup?: (group: NavGroup) => void;
    runtime: RuntimeSummary;
    authority: AuthorityObservation;
    lifecycle: LifecycleObservation;
    f8: { label: string; tone: RuntimeTone; note: string | null } | null;
    version: string;
    /** A terminal is open, so it can be split. */
    canSplit?: boolean;
    onsplit?: (dir: 'row' | 'col') => void;
    /** The shortcuts list is shown. */
    legend?: boolean;
    ontogglelegend?: () => void;
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
    <StatusList {runtime} {authority} {lifecycle} {f8} {version} />
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
    <div class="sidebar-tools" role="group" aria-label="Terminal tools">
      <button
        type="button"
        class="nav-item"
        disabled={!canSplit}
        title="Split side by side (Ctrl+Shift+D)"
        aria-label="Split side by side"
        onclick={() => onsplit?.('row')}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"
          ><path d="M4 5h16v14H4zM12 5v14" /></svg
        >
        <span class="nav-label">Split side by side</span>
      </button>
      <button
        type="button"
        class="nav-item"
        disabled={!canSplit}
        title="Split, one above the other (Ctrl+Shift+E)"
        aria-label="Split one above the other"
        onclick={() => onsplit?.('col')}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"
          ><path d="M4 5h16v14H4zM4 12h16" /></svg
        >
        <span class="nav-label">Split above and below</span>
      </button>
      <button
        type="button"
        class="nav-item"
        aria-expanded={legend}
        aria-label="Keyboard shortcuts"
        title="Keyboard shortcuts"
        onclick={() => ontogglelegend?.()}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"
          ><path d="M3 7h18v10H3zM7 11h.01M11 11h.01M15 11h.01M7 14h10" /></svg
        >
        <span class="nav-label">Keyboard shortcuts</span>
      </button>
    </div>
  </nav>
</div>
