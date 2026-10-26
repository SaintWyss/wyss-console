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
  .remote {
    max-width: 72rem;
    margin: 0 auto;
    padding: 1.5rem clamp(1rem, 3vw, 2rem) 3rem;
    color: var(--text-primary, #20292d);
  }

  header {
    border-bottom: 1px solid var(--border, #c7c5be);
    padding-bottom: 1rem;
  }

  .eyebrow {
    color: var(--text-muted, #59666a);
    font-size: 0.75rem;
    font-weight: 700;
    letter-spacing: 0.12em;
  }

  h2 {
    margin: 0.25rem 0 0.75rem;
    font-size: clamp(1.6rem, 4vw, 2.4rem);
  }

  .blocked {
    border-left: 4px solid #9a4e1e;
    padding: 0.6rem 0.8rem;
    background: color-mix(in srgb, #d18a5b 15%, transparent);
    font-weight: 650;
  }

  .areas {
    display: flex;
    gap: 0.5rem;
    overflow-x: auto;
    padding: 0.8rem 0;
  }

  button {
    min-height: 2.75rem;
    flex: 0 0 auto;
    border: 1px solid var(--border, #9da7a6);
    border-radius: 0.35rem;
    padding: 0.4rem 0.75rem;
    color: inherit;
    background: var(--surface, #faf9f6);
    font: inherit;
    cursor: pointer;
  }

  button[aria-current='page'] {
    border-color: #315a78;
    outline: 2px solid #315a78;
    outline-offset: 1px;
  }

  button:disabled {
    color: var(--text-muted, #59666a);
    cursor: not-allowed;
    opacity: 0.72;
  }

  article {
    max-width: 54rem;
    padding: 1rem 0;
    line-height: 1.6;
  }

  h3 {
    font-size: 1.35rem;
  }

  .no-queue {
    margin-top: 2rem;
    color: var(--text-muted, #59666a);
    font-size: 0.9rem;
  }

  @media (prefers-reduced-motion: reduce) {
    *,
    *::before,
    *::after {
      scroll-behavior: auto !important;
      animation-duration: 0.01ms !important;
      transition-duration: 0.01ms !important;
    }
  }
</style>
