<script lang="ts">
  import { tick } from 'svelte';
  import { t } from '../i18n/index.svelte';
  import {
    NAV_ITEMS,
    RAIL_ITEMS,
    moreItems,
    railOf,
    type NavItem,
    type RailItem,
    type ShownView,
    type ViewId,
  } from './nav';

  let {
    active,
    onselect,
    waiting = 0,
    items = NAV_ITEMS,
  }: {
    active: ShownView;
    onselect: (id: ShownView) => void;
    /** Approvals waiting for a decision. Counted on Activity and on Approvals under More. */
    waiting?: number;
    /** Every view this window offers (Runtimes and Projects only when they exist). */
    items?: readonly NavItem[];
  } = $props();

  let moreOpen = $state(false);
  const lit = $derived(railOf(active));
  const more = $derived(moreItems(items));

  function choose(item: RailItem) {
    if (item.view === null) {
      moreOpen = !moreOpen;
      // Opened: the keyboard lands on its first view.
      if (moreOpen)
        void tick().then(() =>
          document
            .querySelector<HTMLElement>('#rail-more-list button')
            ?.focus(),
        );
      return;
    }
    moreOpen = false;
    onselect(item.view);
  }
  function pick(id: ViewId) {
    moreOpen = false;
    onselect(id);
  }
  function onWindowClick(event: MouseEvent) {
    if (
      moreOpen &&
      !(event.target as Element | null)?.closest('.rail-more, .rail-menu')
    )
      moreOpen = false;
  }
  function onWindowKey(event: KeyboardEvent) {
    if (moreOpen && event.key === 'Escape') {
      moreOpen = false;
      document.querySelector<HTMLElement>('.rail-more')?.focus();
    }
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={onWindowKey} />

<nav class="sidebar" aria-label={t('Primary')}>
  <ul class="rail-list">
    {#each RAIL_ITEMS as item (item.id)}
      <li class:rail-foot={item.id === 'more'}>
        <button
          type="button"
          class="nav-item"
          class:rail-more={item.id === 'more'}
          aria-current={lit === item.id ? 'page' : undefined}
          title={t(item.label)}
          aria-controls={item.id === 'more' ? 'rail-more-list' : undefined}
          aria-expanded={item.id === 'more' ? moreOpen : undefined}
          onclick={() => choose(item)}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true"
            ><path d={item.icon} /></svg
          >
          <span class="nav-label visually-hidden">{t(item.label)}</span>
          {#if item.id === 'activity' && waiting > 0}
            <span
              class="nav-tag waiting"
              aria-label={t('{count} waiting', { count: waiting })}
              >{waiting}</span
            >
          {/if}
        </button>
      </li>
    {/each}
  </ul>
  <!-- A disclosure, not a menu: a list of buttons in the tab order, kept in the page while
       closed (hidden) so its views stay addressable by name. -->
  <div
    id="rail-more-list"
    class="rail-menu"
    aria-label={t('More')}
    hidden={!moreOpen}
  >
    {#each more as item (item.id)}
      <button
        type="button"
        class="nav-item rail-menu-item"
        aria-current={active === item.id ? 'page' : undefined}
        onclick={() => pick(item.id)}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d={item.icon} /></svg>
        <span class="nav-label">{t(item.label)}</span>
        {#if item.id === 'approvals' && waiting > 0}
          <span
            class="nav-tag waiting"
            aria-label={t('{count} waiting', { count: waiting })}
            >{waiting}</span
          >
        {/if}
      </button>
    {/each}
  </div>
</nav>
