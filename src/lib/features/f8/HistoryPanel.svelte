<script lang="ts">
  import {
    CATEGORY_LABEL,
    DISPOSITION,
    eventClock,
    eventDay,
    eventTitle,
    short,
  } from '../events/present';
  import type { F8Controller, F8ViewState } from './controller';
  import { failureText } from './present';

  let {
    f8,
    controller,
    connected,
  }: {
    f8: F8ViewState;
    controller: F8Controller;
    connected: boolean;
  } = $props();

  // Narrows what is shown of the window that was read; it asks the authority for nothing.
  let own = $state(true);
  const history = $derived(f8.history);
  const page = $derived(history.page?.page ?? null);
  const rows = $derived(
    (page?.events ?? []).filter((e) => !own || e.session === f8.selected),
  );
</script>

<div class="rt-history">
  <div class="events-filters" role="group" aria-label="History">
    <label class="mode-option rt-own">
      <input type="checkbox" bind:checked={own} />
      <span class="mode-name">This session only</span>
    </label>
    <span class="events-note"
      >{own
        ? 'Narrows the window loaded here, not all of history.'
        : 'Every session.'}</span
    >
    <button
      type="button"
      class="action"
      disabled={!connected || history.status === 'reading'}
      onclick={() => controller.newestHistory()}>Newest</button
    >
  </div>

  {#if history.status === 'failed' && history.failure}
    <div class="sessions-status failed" role="alert">
      <p>{failureText(history.failure.code)}</p>
      <button
        type="button"
        class="action"
        onclick={() => controller.newestHistory()}>Try again</button
      >
    </div>
  {/if}

  {#if !page}
    <div
      class="sessions-empty"
      role="status"
      aria-busy={history.status === 'reading'}
    >
      <p class="empty-title">
        {history.status === 'failed'
          ? 'History is unavailable'
          : 'Reading history…'}
      </p>
    </div>
  {:else if page.events.length === 0}
    <div class="sessions-empty">
      <p class="empty-title">No events yet.</p>
      <p>The authority records every state change it commits.</p>
    </div>
  {:else}
    {#if page.gap}
      <p class="events-note" role="note">
        History before the oldest event shown is not available.
      </p>
    {/if}
    {#if rows.length === 0}
      <p class="events-note">
        No event in this window belongs to this session.
      </p>
    {:else}
      <ul class="events-list" aria-label="Events, newest first">
        {#each rows as event (event.sequence)}
          <li>
            <div class="event-row" role="group">
              <time class="event-time" datetime={event.at}
                >{eventClock(event.at)}</time
              >
              <span class="event-title">{eventTitle(event)}</span>
              {#if event.disposition !== 'normal'}
                <span
                  class="pill compact"
                  data-tone={DISPOSITION[event.disposition].tone}
                >
                  <span class="dot" aria-hidden="true"></span>
                  {DISPOSITION[event.disposition].label}
                </span>
              {/if}
              <span class="event-meta">
                {CATEGORY_LABEL[event.category]}
                {#if event.session && !own}
                  · session <span class="mono">{short(event.session)}</span>
                {/if}
                · #{event.sequence}
                {#if eventDay(event.at)}· {eventDay(event.at)}{/if}
              </span>
            </div>
          </li>
        {/each}
      </ul>
    {/if}
    <div class="sessions-pager">
      <p>
        Newest first{history.depth > 0
          ? ` · ${history.depth} window${history.depth === 1 ? '' : 's'} back`
          : ''}
      </p>
      <button
        type="button"
        class="action"
        disabled={!page.older || history.status === 'reading' || !connected}
        onclick={() => controller.loadHistory(true)}>Older</button
      >
    </div>
  {/if}
</div>
