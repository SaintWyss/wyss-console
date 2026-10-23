<script lang="ts" generics="T">
  import type { Snippet } from 'svelte';

  /**
   * A list that keeps in the page only the rows that can be seen. A short list is drawn whole;
   * past `threshold` rows only the ones in view (and a few beside them) exist, in a box of the
   * height of all of them, so a store at its ceilings costs the page what a screenful does.
   * Rows are one fixed height: whatever grows a row is shown outside the list.
   */
  let {
    items,
    rowHeight,
    key,
    row,
    label,
    attrs = () => ({}),
    threshold = 60,
    overscan = 6,
    maxHeight = 360,
  }: {
    items: T[];
    rowHeight: number;
    key: (item: T) => string;
    row: Snippet<[T]>;
    label: string;
    /** Attributes of each row's element. */
    attrs?: (item: T) => Record<string, string>;
    threshold?: number;
    overscan?: number;
    maxHeight?: number;
  } = $props();

  let top = $state(0);
  let box = $state(0);
  const windowed = $derived(items.length > threshold);
  // Without a measured box (before layout) the box is taken at its largest.
  const viewport = $derived(box > 0 ? box : maxHeight);
  const first = $derived(
    windowed ? Math.max(0, Math.floor(top / rowHeight) - overscan) : 0,
  );
  const last = $derived(
    windowed
      ? Math.min(
          items.length,
          Math.ceil((top + viewport) / rowHeight) + overscan,
        )
      : items.length,
  );
  const shown = $derived(items.slice(first, last));
</script>

{#if windowed}
  <div
    class="projects-virtual"
    style:max-height={`${maxHeight}px`}
    bind:clientHeight={box}
    onscroll={(event) => (top = event.currentTarget.scrollTop)}
    data-rows={items.length}
    data-drawn={shown.length}
  >
    <ul
      class="projects-list"
      aria-label={label}
      style:height={`${items.length * rowHeight}px`}
      style:position="relative"
    >
      {#each shown as item, n (key(item))}
        <li
          {...attrs(item)}
          data-row={first + n + 1}
          style:position="absolute"
          style:left="0"
          style:right="0"
          style:top={`${(first + n) * rowHeight}px`}
          style:height={`${rowHeight - 4}px`}
        >
          {@render row(item)}
        </li>
      {/each}
    </ul>
  </div>
{:else}
  <ul class="projects-list" aria-label={label}>
    {#each items as item (key(item))}
      <li {...attrs(item)}>
        {@render row(item)}
      </li>
    {/each}
  </ul>
{/if}
