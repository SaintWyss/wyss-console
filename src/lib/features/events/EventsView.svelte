<script lang="ts">
  import type { ApprovalDetail } from '../../contracts/approvals';
  import type { EventRow } from '../../contracts/events';
  import type { TimeFormat } from '../../contracts/settings';
  import type {
    ManagedApprovalId,
    ManagedSessionId,
  } from '../../contracts/identities';
  import { readApproval } from '../../desktop/approvals-client';
  import { SessionCallError } from '../../desktop/sessions-client';
  import {
    summarizeAuthority,
    type AuthorityObservation,
  } from '../../shell/authority';
  import { APPROVAL_REASON, APPROVAL_STATE } from '../approvals/present';
  import { localTime } from '../sessions/present';
  import {
    EVENT_FILTERS,
    type EventKey,
    type EventsController,
    type EventsState,
  } from './controller';
  import {
    CATEGORY_LABEL,
    DISPOSITION,
    eventDetail,
    eventsReadFailure,
    eventTitle,
    FILTER_LABEL,
    olderFailure,
    short,
    eventClock,
    eventDay,
    shownEvents,
    startMessage,
  } from './present';

  let {
    events,
    controller,
    authority,
    visible,
    timeFormat = 'system',
    onsession,
    onagent,
    onapprovals,
    lookup = (id: ManagedApprovalId) => readApproval(id, null, null),
  }: {
    events: EventsState;
    controller: EventsController;
    /** The shell's one authority observation. Read here for wording only. */
    authority: AuthorityObservation;
    visible: boolean;
    /** How the time of day of an event is written. Only how: never which events. */
    timeFormat?: TimeFormat;
    /** Shows a session in Sessions, where what it is now is read. */
    onsession: (session: ManagedSessionId) => void;
    /** Opens a session in Agents, where its turns are read. */
    onagent: (session: ManagedSessionId) => void;
    onapprovals: () => void;
    /** A canonical read of one approval, as it is now. */
    lookup?: (approval: ManagedApprovalId) => Promise<ApprovalDetail>;
  } = $props();

  const connected = $derived(events.link === 'connected');
  const rows = $derived(
    shownEvents(events.events, events.filter, events.scope),
  );
  const earlier = $derived(
    events.previous
      ? shownEvents(events.previous.events, events.filter, events.scope)
      : [],
  );
  const narrowed = $derived(events.filter !== 'all' || events.scope !== null);
  const selected = $derived.by(() => {
    const key = events.selected;
    if (key === null) return null;
    const from =
      key.stream === events.stream
        ? events.events
        : (events.previous?.events ?? []);
    return from.find((event) => event.sequence === key.sequence) ?? null;
  });

  /** The selected record is of a stream the authority that answers now does not hold. */
  const earlierStream = $derived(
    events.selected !== null && events.selected.stream !== events.stream,
  );

  let list = $state<HTMLElement | null>(null);
  function scrolled() {
    if (list) controller.setAtTop(list.scrollTop <= 4);
  }
  /** Back to the newest end: what is there now is read, and shown from its top. */
  function newest() {
    if (list) list.scrollTop = 0;
    // One read: asked for from where the reader was, then followed from the newest end.
    controller.refresh();
    controller.setAtTop(true);
  }
  function keyOf(stream: string, event: EventRow): EventKey {
    return { stream, sequence: event.sequence };
  }
  function chosen(stream: string, event: EventRow): boolean {
    return (
      events.selected?.stream === stream &&
      events.selected.sequence === event.sequence
    );
  }

  // What an approval is now is the authority's to say, and is read only when asked.
  type Lookup =
    | { status: 'reading' }
    | { status: 'read'; detail: ApprovalDetail }
    | { status: 'gone' }
    | { status: 'failed' };
  let checked = $state<{ approval: ManagedApprovalId; answer: Lookup } | null>(
    null,
  );
  const answer = $derived(
    selected?.approval && checked?.approval === selected.approval
      ? checked.answer
      : null,
  );
  function check(approval: ManagedApprovalId) {
    checked = { approval, answer: { status: 'reading' } };
    const settle = (answer: Lookup) => {
      if (checked?.approval === approval) checked = { approval, answer };
    };
    lookup(approval).then(
      (detail) => settle({ status: 'read', detail }),
      (error) =>
        settle({
          status:
            error instanceof SessionCallError &&
            error.failure.code === 'not_found'
              ? 'gone'
              : 'failed',
        }),
    );
  }
</script>

{#snippet row(stream: string, event: EventRow)}
  <li>
    <button
      type="button"
      class="event-row"
      aria-pressed={chosen(stream, event)}
      onclick={() =>
        controller.select(chosen(stream, event) ? null : keyOf(stream, event))}
    >
      <time class="event-time" datetime={event.at}
        >{eventClock(event.at, timeFormat)}</time
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
        {#if event.session}
          · session <span class="mono">{short(event.session)}</span>
        {/if}
        {#if eventDay(event.at)}
          · {eventDay(event.at)}
        {/if}
      </span>
    </button>
  </li>
{/snippet}

<section
  class="events-feature"
  aria-label="Events"
  hidden={!visible}
  inert={!visible}
>
  <div class="sessions-toolbar">
    <div class="sessions-heading">
      <h2>Events</h2>
      <p class="sessions-count">
        {#if events.link === 'connecting' && !events.loaded}
          Connecting to the authority
        {:else if !events.loaded}
          {connected ? 'Reading' : 'Authority unavailable'}
        {:else}
          {events.events.length} loaded · newest first
          {#if narrowed}
            · {rows.length} shown
          {/if}
        {/if}
      </p>
    </div>
    <button
      type="button"
      class="action"
      disabled={!connected || events.reading}
      onclick={newest}>{events.detached ? 'Newest events' : 'Refresh'}</button
    >
  </div>

  {#if events.loaded && !connected}
    <p class="sessions-status failed unknown" role="status">
      Authority offline. Showing previously loaded history: it was read before
      the connection was lost and nothing newer is known.
    </p>
  {:else if events.loaded && events.stale}
    <p class="sessions-status" role="status">
      Showing previously loaded history. It is being read again.
    </p>
  {/if}
  {#if events.restarted}
    <div class="sessions-status" role="status">
      <p>
        wyss-control restarted. It kept the same event stream: the history below
        continues, and what it ended on the way is recorded in it.
      </p>
      <button
        type="button"
        class="action"
        onclick={() => controller.acknowledge()}>Dismiss</button
      >
    </div>
  {/if}
  {#if events.loaded && events.readFailure}
    <div class="sessions-status failed" role="alert">
      <p>{eventsReadFailure(events.readFailure.code)}</p>
      <button type="button" class="action" onclick={newest}>Try again</button>
    </div>
  {/if}

  {#if events.link === 'connecting' && !events.loaded}
    <div class="sessions-empty" role="status">
      <p class="empty-title">Connecting to wyss-control…</p>
    </div>
  {:else if !events.loaded && !connected}
    <div class="sessions-empty" role="status">
      <p class="empty-title">wyss-control is unavailable</p>
      <p>{summarizeAuthority(authority).detail}</p>
      <p>
        Events are the authority's own record and are read from it. Nothing is
        shown until it is connected.
      </p>
    </div>
  {:else if !events.loaded && events.readFailure}
    <div class="sessions-empty" role="alert">
      <p class="empty-title">{eventsReadFailure(events.readFailure.code)}</p>
      <button type="button" class="action" onclick={newest}>Try again</button>
    </div>
  {:else if !events.loaded}
    <div class="sessions-empty" role="status" aria-busy="true">
      <p class="empty-title">Reading events…</p>
    </div>
  {:else if events.events.length === 0 && events.previous === null}
    <div class="sessions-empty">
      <p class="empty-title">No events yet.</p>
      <p>
        wyss-control records every session, runtime, turn, approval and lease
        change it commits. Nothing has been recorded in this stream.
      </p>
    </div>
  {:else}
    <div class="events-filters" role="group" aria-label="Filter events">
      {#each EVENT_FILTERS as filter (filter)}
        <button
          type="button"
          class="action chip"
          aria-pressed={events.filter === filter}
          onclick={() => controller.setFilter(filter)}
          >{FILTER_LABEL[filter]}</button
        >
      {/each}
      {#if events.scope}
        <span class="events-scope">
          {events.scope.turn
            ? `Turn ${short(events.scope.turn)}`
            : `Session ${short(events.scope.session)}`}
          <button
            type="button"
            class="action chip"
            onclick={() => controller.setScope(null)}>Clear</button
          >
        </span>
      {/if}
      {#if narrowed}
        <span class="events-note"
          >Filters apply to the events loaded here, not to all of history.</span
        >
      {/if}
    </div>

    <div class="sessions-body" class:has-details={!!selected}>
      <div
        class="sessions-main events-main"
        bind:this={list}
        onscroll={scrolled}
      >
        {#if events.newer}
          <button type="button" class="action events-newer" onclick={newest}
            >New events available</button
          >
        {/if}
        {#if rows.length === 0}
          <p class="events-note">No loaded event matches.</p>
        {:else}
          <ul class="events-list" aria-label="Events, newest first">
            {#each rows as event (event.sequence)}
              {@render row(events.stream ?? '', event)}
            {/each}
          </ul>
        {/if}

        <div class="events-end">
          {#if events.olderFailure}
            <p class="events-note failed" role="alert">
              {olderFailure(events.olderFailure.code)}
            </p>
          {/if}
          {#if events.start === 'more'}
            <button
              type="button"
              class="action"
              disabled={!connected || events.loadingOlder || events.reading}
              aria-busy={events.loadingOlder}
              onclick={() => controller.loadOlder()}
              >{events.loadingOlder ? 'Reading…' : 'Load older'}</button
            >
            <p class="events-note">Older events exist and are not loaded.</p>
          {:else}
            <p
              class="events-note"
              class:failed={events.start === 'unavailable'}
            >
              {startMessage(events.start)}
            </p>
          {/if}
        </div>

        {#if events.previous}
          <div class="events-boundary" role="separator">
            <p class="empty-title">New authority event stream</p>
            <p>
              The events above belong to the stream wyss-control holds now. The
              ones below were read from another stream (<span class="mono"
                >{short(events.previous.stream)}</span
              >) and are not continued by them: positions of the two are
              unrelated.
            </p>
          </div>
          <ul
            class="events-list earlier"
            aria-label="Events of an earlier stream"
          >
            {#each earlier as event (event.sequence)}
              {@render row(events.previous.stream, event)}
            {/each}
          </ul>
        {/if}
      </div>

      {#if selected}
        <aside class="session-details event-details" aria-label="Event details">
          <div class="details-head">
            <h3>{eventTitle(selected)}</h3>
            <button
              type="button"
              class="details-close"
              aria-label="Hide details"
              onclick={() => controller.select(null)}>×</button
            >
          </div>
          <p class="session-hint">{eventDetail(selected)}</p>
          <dl class="session-facts">
            <dt>Time</dt>
            <dd>
              <time datetime={selected.at}>{localTime(selected.at)}</time>
              <span class="fact-note mono">{selected.at}</span>
            </dd>
            <dt>Kind</dt>
            <dd>
              {CATEGORY_LABEL[selected.category]}
              {#if selected.disposition !== 'normal'}
                <span class="fact-note"
                  >{DISPOSITION[selected.disposition].label}</span
                >
              {/if}
            </dd>
            {#if selected.session}
              <dt>Session</dt>
              <dd class="mono">{selected.session}</dd>
            {/if}
            {#if selected.turn}
              <dt>Turn</dt>
              <dd class="mono">{selected.turn}</dd>
            {/if}
            {#if selected.approval}
              <dt>Approval</dt>
              <dd class="mono">{selected.approval}</dd>
            {/if}
            {#if selected.facts.expires_at}
              <dt>Lease until</dt>
              <dd>
                <time datetime={selected.facts.expires_at}
                  >{localTime(selected.facts.expires_at)}</time
                >
              </dd>
            {/if}
            <dt>Position</dt>
            <dd class="mono">
              {selected.sequence}
              <span class="fact-note"
                >in stream {short(events.selected?.stream ?? '')}</span
              >
            </dd>
          </dl>

          {#if earlierStream}
            <!-- What this record is about belongs to another stream's authority
                 state. Nothing here may be read against the one that answers now. -->
            <p class="session-hint" role="note">
              This record was read from a stream wyss-control no longer holds
              here. What it is about cannot be opened or read from this
              authority.
            </p>
          {:else}
            <p class="session-hint">
              This is a record of what happened then. What it is about may have
              changed since: that is read from wyss-control where it lives.
            </p>
          {/if}
          {#if !earlierStream}
            <div class="session-actions">
              {#if selected.session}
                {@const session = selected.session}
                <button
                  type="button"
                  class="action"
                  onclick={() => onsession(session)}>Open session</button
                >
                {#if selected.turn || selected.category === 'turn'}
                  <button
                    type="button"
                    class="action"
                    onclick={() => onagent(session)}>Open in Agents</button
                  >
                {/if}
                <button
                  type="button"
                  class="action"
                  onclick={() =>
                    controller.setScope({ session, turn: selected.turn })}
                  >{selected.turn
                    ? 'Show events of this turn'
                    : 'Show events of this session'}</button
                >
              {/if}
              {#if selected.approval}
                {@const approval = selected.approval}
                <button
                  type="button"
                  class="action"
                  disabled={!connected || answer?.status === 'reading'}
                  onclick={() => check(approval)}
                  >{answer?.status === 'reading'
                    ? 'Reading…'
                    : 'Check what it is now'}</button
                >
                <button type="button" class="action" onclick={onapprovals}
                  >Open Approvals</button
                >
              {/if}
            </div>
          {/if}
          {#if earlierStream}
            <!-- no canonical answer belongs under a record of another stream -->
          {:else if answer?.status === 'read'}
            {@const now = answer.detail.approval}
            <p class="session-hint" role="status">
              Now: {APPROVAL_STATE[now.state].label}.
              {now.reason ? APPROVAL_REASON[now.reason] : ''}
              {answer.detail.command === null && answer.detail.files === null
                ? 'wyss-control no longer holds what it was about.'
                : 'What it is about is shown in Approvals.'}
            </p>
          {:else if answer?.status === 'gone'}
            <p class="session-hint" role="status">
              Approval detail is no longer available. The record above is still
              what happened.
            </p>
          {:else if answer?.status === 'failed'}
            <p class="session-hint failed" role="alert">
              The approval could not be read from wyss-control.
            </p>
          {/if}
        </aside>
      {/if}
    </div>
  {/if}
</section>
