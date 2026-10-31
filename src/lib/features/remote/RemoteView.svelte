<script lang="ts">
  let { visible = false }: { visible?: boolean } = $props();
  const sections = [
    'Hosts',
    'Pairing',
    'Projects',
    'Sessions',
    'Terminal',
    'Pipelines',
    'Approvals',
    'Devices',
    'Settings',
  ] as const;
  let section = $state<(typeof sections)[number]>('Hosts');
</script>

<section class="remote" aria-label="Remote PCs" hidden={!visible}>
  <header>
    <p class="eyebrow">M7 · REMOTE GATEWAY</p>
    <h2>Remote PCs</h2>
    <p class="blocked" role="status">
      Unavailable · OD01 transport choice is open. This view sends no network
      requests and has no paired hosts.
    </p>
  </header>

  <nav class="areas" aria-label="Remote areas">
    {#each sections as item (item)}
      <button
        type="button"
        aria-current={section === item ? 'page' : undefined}
        onclick={() => (section = item)}>{item}</button
      >
    {/each}
  </nav>

  <article aria-live="polite">
    <h3>{section}</h3>
    {#if section === 'Hosts'}
      <p>
        No remote host is enrolled. Authentication, pairing, connection type and
        grant expiry are unavailable until a transport is approved and
        independently reviewed.
      </p>
      <button type="button" disabled>Connect to a host</button>
    {:else if section === 'Pairing'}
      <p>
        Pairing requires two distinct device keys, a pinned host identity, QR
        transcript proof and explicit host confirmation. No key or enrollment is
        created from this view.
      </p>
      <button type="button" disabled>Begin pairing</button>
    {:else if section === 'Projects'}
      <p>
        Remote projects and workspaces will be listed only from an authenticated
        host. No local path or cached project is shown here.
      </p>
    {:else if section === 'Sessions'}
      <p>
        Native and Managed sessions need host identity, plane, generation and
        observation age. No remote session metadata is cached.
      </p>
    {:else if section === 'Terminal'}
      <p>
        Disconnected · no output is available. View and input grants are
        separate; terminal.write grants full host-account control. Locked-host
        full-account input remains blocked.
      </p>
      <p>
        Input must report accepted bytes, partial writes or OUTCOME_UNKNOWN. A
        lost acknowledgement is never replayed; new user input is required after
        reconnect.
      </p>
      <button type="button" disabled>Attach read-only terminal</button>
      <button type="button" disabled>Claim terminal input</button>
    {:else if section === 'Pipelines'}
      <p>
        Pipeline metadata is unavailable offline. M6 keeps prompts and provider
        output out of durable journals; a remote view will fetch through the
        reviewed host protocol.
      </p>
    {:else if section === 'Approvals'}
      <p>
        Managed approval decisions must pass through the existing wyss-control
        authority and its per-operation presence check. This view cannot approve
        while disconnected.
      </p>
      <button type="button" disabled>Approve</button>
      <button type="button" disabled>Deny</button>
    {:else if section === 'Devices'}
      <p>
        No paired-device list exists. Revocation, expiry and host-confirmed
        grant changes will be shown only after authenticated host negotiation.
      </p>
    {:else}
      <h4>Transport candidates</h4>
      <p>
        H1 · owner-configured overlay: Android VPN lifecycle and third-party
        control plane; Tailscale reference pricing is $0 Personal (up to 6
        users), $8/user/month Standard, $18/user/month Premium.
      </p>
      <p>
        H2 · iroh/QUIC: first-party Kotlin/Android bindings and end-to-end TLS;
        public relays are free/rate-limited/no-SLA, hosted relays start at
        $19/month, self-hosting transfers infrastructure and operations to Wyss.
      </p>
      <p>
        No CPU, RAM, battery, idle, latency or roaming measurements exist for
        either candidate. OD01 remains an owner decision. OD02 keeps locked-host
        full-account input blocked.
      </p>
    {/if}
    <p class="no-queue">Disconnected · no action is queued or sent.</p>
  </article>
</section>

<style>
  /* The same grammar as every view: a heading at the view scale, tabs drawn as tabs, a
     column that scrolls. Selection and keyboard focus never look the same. */
  .remote {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 2px 4px 24px 0;
    color: var(--text);
  }

  header {
    display: grid;
    gap: 6px;
  }

  .eyebrow {
    color: var(--faint);
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
  }

  h2 {
    margin: 0;
    font-size: var(--text-xl);
    font-weight: 600;
  }

  .blocked {
    max-width: 72ch;
    padding: 8px 12px;
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--text);
    background: var(--warn-soft);
    border: 1px solid var(--warn-line);
    border-left-width: 3px;
    border-radius: var(--radius-md);
  }

  .areas {
    display: flex;
    gap: 2px;
    margin-top: 12px;
    overflow-x: auto;
    border-bottom: 1px solid var(--line);
  }

  .areas button {
    position: relative;
    flex: 0 0 auto;
    min-height: 38px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
    color: var(--muted);
    background: transparent;
    font-weight: 500;
  }

  .areas button:hover:not(:disabled) {
    color: var(--text);
    background: var(--surface-hover);
  }

  .areas button[aria-current='page'] {
    color: var(--text);
    font-weight: 600;
  }

  .areas button[aria-current='page']::after {
    content: '';
    position: absolute;
    left: 8px;
    right: 8px;
    bottom: -1px;
    height: 2px;
    border-radius: 2px 2px 0 0;
    background: var(--accent);
  }

  .areas button:focus-visible {
    outline-offset: -2px;
  }

  article {
    max-width: 72ch;
    padding: 16px 0;
    line-height: 1.6;
  }

  h3 {
    margin: 0 0 8px;
    font-size: var(--text-lg);
    font-weight: 600;
  }

  .no-queue {
    margin-top: 24px;
    color: var(--muted);
    font-size: var(--text-sm);
  }
</style>
