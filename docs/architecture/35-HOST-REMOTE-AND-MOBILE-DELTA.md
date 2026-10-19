# 35 — Host, hybrid remote access and native Android: design delta

Status: **candidate specification, DN-05 unresolved**.
Owner direction: DN-44=B, DN-45=B, DN-46=B, DN-47=B, DN-48=A and
transparent remote access over local/direct/fallback paths. Read alongside
[16](16-MULTI-DEVICE-AND-REMOTE.md), [32](32-OWNER-PRODUCT-DIRECTIONS-2026-10-13.md)
and [33](33-CROSS-DOMAIN-RECONCILIATION.md).
The current RA3/WireGuard-class overlay recommendation in 16 is a **candidate**, not
an owner decision. None of the paths below was installed or tested for this change.

## 1. Personal-product use cases and constraints

- Host computer stays on at home, possibly without KDE graphical login; ongoing
  native PTYs and finite approved OL3 pipelines continue under the owner-enabled
  user-service lifetime (DN-46).
- From Android or the second computer: discover authorised hosts, see project/session
  status, attach to a live terminal, obtain an exclusive input claim when privileged,
  type, resize, see output, approve managed operations, submit a bounded plan and
  reconnect after networks change.
- The owner should not have to *manually configure or turn on a VPN each time*.
  Automatic background overlay connection, a P2P library with built-in relay, and
  direct LAN with fallback are all viable experience *candidates* until benchmarked.
- Out of scope: Sarmiento screen/video/control integration and public product launch.
  Sarmiento may be researched as *optional* trusted inventory/discovery, but neither
  its RustDesk server nor its MFA grants access to Wyss automatically.
- Remote terminal input is explicitly full host-account power. Least-privilege
  watching/approval profiles must **not** implicitly receive full shell access.

## 2. Component responsibilities

| Component | Holds | Must never do |
| --- | --- | --- |
| `wyss-host` | PTY ownership, child process groups, native session generations, bounded screen-state and scrollback, input ownership/resize contracts, local Unix socket | Hold `wyss-control` leases or remotely authenticate Internet clients itself |
| `wyss-gateway` | Device identity, pairing/grants, anti-replay, fresh presence check, capability enforcement, remote audit and transport-independent protocol | Treat transport membership as authority, supply native dialog on the host, issue arbitrary provider commands |
| `wyss-control` | Managed authority and its existing strict control of turn/tool approvals | Trust gateway claims as more than existing typed client intents; give remote terminals a managed lease |
| Android client | Keystore-backed device identity, explicit input grant requests, local view state, connection retry and offline indicators | Store terminal output durably by default; assume device unlock is host confirmation |
| Network adapter | Reachability and encrypted authenticated channel established on verified endpoint keys | Issue application grants, alter input sequence semantics, expose plaintext via relay |
| `wyss-console` UI | Distinct config and terminal capabilities (DN-22=B), native confirmations when locally present, project workspace | Resurrect dead sessions or treat remote opt-in as native host confirmation |

**Required:** the process owning PTY masters must be separate from
untrusted terminal output parsers. Each terminal's parser runs in
an individually restartable, resource-limited low-privilege helper.
Parser crash/OOM degrades screen fidelity **without killing any
PTY or sibling session**. The IPC, isolation mechanism, parser
restart and fidelity acceptance require a new ADR and actual tests;
this requirement is not itself proof.

### 2.1 Coordinator service is separate from the window

OL3 finite pipelines that outlive the desktop require a **headless**
coordinator/user-service candidate (wyss-orchestrator; see
[34 §2.1](34-OL3-ORCHESTRATION-CONTRACT.md)).
It must not reuse wyss-control as a scheduler or acquire
native PTY byte parsing. Owner-enabled lingering supports both
wyss-host and the coordinator, but no user service can
continue work after actual power-off. A reconnection to the
Console must never create a second coordinator run for the same
pipeline generation. This placement is an unaccepted
architecture choice and needs lifecycle/footprint evidence.

## 3. Reachability comparison: no selection yet (DN-05)

| Candidate | User experience | Operating/maintenance trade-off | Hard questions |
| --- | --- | --- | --- |
| H1 automatic private overlay (e.g. user-configured Tailscale-like) + Wyss gateway | Apps reconnect through background overlay; traffic attempts direct P2P and uses relays | Third-party coordination/control plane and client integration; can be simplest operationally | Does Android VPN-style system integration match owner's UX and privacy requirements? Works with direct, relayed and roaming networks? |
| H2 Rust P2P library such as iroh/QUIC, built-in NAT traversal and fallback relay + Wyss gateway | A single native app connection flow, no separate VPN UI | Library/runtime, relay availability, application discovery and versioned protocol owned by Wyss | Android integration, mobile background constraints, licence/SBOM, relay hosting/fees, idle CPU, security maturity |
| H3 independent LAN-direct + P2P/relay framework + protocol bridge | Optimise LAN separately and traverse remote networks | Most connection modes; highest transport testing burden | Safe path switching, session migration without input replay, LAN discovery spoofing, endpoint pinning |
| H4 SSH tunnel over existing trusted network or direct reachable SSH | Simple between desktops where SSH already available | Android UX and broad full shell rights do not provide a scoped gateway by default | How to avoid an SSH key being an unconditional full-account bypass? |
| H5 integrate Sarmiento inventory with H1/H2 | Central device list for owner's computers | Coupled enrolment/availability; version and threat-boundary mismatch | What exact non-secret discovery facts can be shared; what happens when Sarmiento is down? |
| H6 invent a public relay and signalling protocol from scratch | Tailored semantics but not required for personal product | Highest cost and permanent public service attack surface | Hard to justify against H1/H2; should require documented failure of existing choices |

At this stage **H1/H2 are comparative finalists**, not H3/H6 implementation commitments.
Use a transport-neutral gateway contract to avoid assuming the winning technology.

External source references (2026-10-13 documentation research, **not tested locally**):
- iroh crate docs, direct P2P QUIC with hole punching and relays:
  https://docs.rs/iroh/latest/iroh/
- Tailscale on direct encrypted routes and DERP fallback:
  https://tailscale.com/docs/concepts/traffic-routing-through-tailscale
- Android Keystore per-use authentication:
  https://developer.android.com/privacy-and-security/keystore

Do **not** infer the stated product-specific security, licence compatibility, lower resource
cost or guaranteed connectivity from upstream advertising. Those are evidence gates.

### 3.1 Candidate decision procedure (not a vote)

Comparative measurement must report H1/H2 on the owner's reference desktop and Android
with at least: same-LAN, mobile data, university Wi-Fi, CGNAT/symmetric NAT where possible,
network swap while attached, phone screen lock/unlock, host logout/lingering, relay outage,
revoked device, restart. Report p50/p95 time-to-ready, reconnection time, byte/CPU/RAM
idle footprint, time-to-first-output, input latency, E2E test status, NAT/relay costs,
maintenance burden, diagnostics, privacy and recovery. Separate **measured**,
**documented** and **inferred** fields. If testing is not separately authorised, leave
the decision unresolved; do not fabricate a score.

## 4. Host lifecycle for DN-46

- Preferred candidate is a separately supervised **user** service `wyss-host`
  (HS1): no privileged system-wide daemon, no public listener on host, Unix IPC.
- Closing desktop window simply detaches viewers and never kills active sessions.
- KDE screen lock does not close the login session, unless OS suspend policy intervenes.
- KDE logout normally may stop `systemd --user`; optional explicit
  `loginctl enable-linger` keeps its manager available. Enabling linger is a
  **host administration action**, not silently performed by the Console.
- The host's idle shutdown requires **no live PTY, no active/paused/
  waiting-approval/recovering finite pipeline, and no required
  client**. The headless coordinator issues a bounded renewable
  keepalive while the pipeline has work or waiting approvals.
  Zero PTYs and viewers alone are insufficient. Never use only
  zero UI clients to infer that no work exists.
- Power off, hibernate and reboot terminate or freeze local processes;
  do not promise continued execution. On host crash, mark lost process instances
  accurately and offer a fresh authorised restart.
- A daemon restart loses PTY masters unless a validated
  survivor mechanism exists, but PTY loss **does not prove
  child or detached descendant exit**. Native jobs need
  cgroup/scope accounting; escaping descendants can remain
  `ORPHANS_POSSIBLE`. Deny automatic duplicate launches
  while effects could survive the host.
- When not logged in, native host-only confirmation requests wait. Device
  presence on Android does not synthesize a KDE dialog response.
- Host metadata can survive restart under a versioned schema with stable IDs and
  terminal outcomes, but neither secret-bearing output nor pretend-live PTY handles.

## 5. Terminal state: fidelity without unbounded memory (DN-47+B / DN-40+C)

Store separate types of **ephemeral** state:
1. Current screen grid: bounded cells/attributes, active/alternate screen,
   cursor, modes, title and resize generation; one authoritative size.
2. Scrollback history: per-terminal line/byte preference and *global*
   admission budget, with a user-visible memory status and explicit retention
   condition. Choosing DN-40=C means existing history is not quietly truncated
   merely when the budget is crossed; new terminals receive a lower quota.
3. Transport replay/output queue: bounded per-client bytes and time; a slow
   client can fall behind and receive a checkpoint/resync rather than blocking
   a child PTY or forcing unbounded queues.
4. Renderer cache: client-local derived presentation, disposable.

Full-screen mode, line wrapping, ANSI escapes, hyperlinks, alternate-screen
buffers, UTF-8 and combining characters must be tested with hostile and real
program corpora. The **screen checkpoint**, not only raw byte replay, is what
DN-47=B asks for; fidelity for specific programs is a test result, not a promise.
The emulator must not retrieve OS files, follow arbitrary hyperlinks or execute
terminal escape commands. Limit title, hyperlink and OSC/DCS payload lengths
and reject unsupported controls.

**Memory contradiction:** preserving already-retained scrollback under DN-40=C
means a merely advisory budget cannot promise a hard cap. The design must also
declare upper admission bounds on terminal count, initial per-terminal byte limit,
current-screen dimensions, backlog size and max process memory; if aggregate
memory is about to exhaust, reject new work and report resource failure instead
of silently discarding existing held user data. Ephemeral output availability
may degrade explicitly under the existing bounded ring contract; it is not a
protected vault record. Measurements M12/M21/M24 decide numeric thresholds.

## 6. Protocol and reconnect (IF-REMOTE candidate)

Every message has a bounded, versioned envelope:
`protocol_version`, `host_id`, `device_id`, `grant_id`,
`session_id`, `session_generation`, `connection_epoch`,
`stream_id`, `request_id`, optional idempotency key, message type and payload.
All identity fields are validated as typed opaque values, not user-provided
paths or shell fragments. Compression must have bounded decompressed size.

- Authentication proves a device key plus a pinned host endpoint; transport
  encryption is mandatory even on LAN. On relay fallback the relay must see
  only encrypted application payloads and minimal routing metadata.
- Grants are host/device scoped, expiring and revocable for
  future normal gateway actions, but **a compromised LP4
  host-user shell can tamper with same-UID grant files and
  install persistence**. Revocation cannot repair account
  integrity. Independent, differently privileged grant
  storage is an unproved future safety requirement
  (see [42](42-POST-REJECT-CORRECTIVE-PLAN.md)).
- Terminal output uses monotonically numbered frames and ACK watermark. After
  reconnect, offer a screen checkpoint and bounded scrollback tail. Missing
  history is labelled, never silently presented as complete.
- Input requires an exclusive claim and a proven presence-key
  capability. Bind claim to terminal/host generation, unpredictable
  `gateway_instance_id` per boot, claim epoch and sequence.
  A gateway restart invalidates old claims. If the gateway
  could have executed input before crashing without sending
  ACK, return `OUTCOME_UNKNOWN` and **never replay bytes
  automatically**. In-instance duplicates must be refused.
  Managed intents rely on the authority's durable intent
  receipts, never only gateway in-memory deduplication.
- A command typed remotely into a native PTY is not a typed managed action:
  it's a full shell-capability input. Show this at grant issuance and audit
  the fact of claim and expiry, not the command body.
- Managed approval intent is forwarded to `wyss-control` exactly as typed;
  gateway authentication is no replacement for a managed lease, approval or
  local configuration write confirmation.
- On unknown protocol version, revoked device, stale generation, mismatched
  host key, vanished process or ambiguous replay: fail closed for input and
  show actionable recovery, preserving local host work where safe.
- No remote side may write directly into the protected vault via a metadata
  operation or invoke a host-only native confirmation.

## 7. Android client contract (DN-48=A)

Proposed screens: Hosts, Pair host, Sessions, Terminal, Pipeline status, Managed
approvals, Notifications, Devices/grants, Settings, Offline/reconnect.

| Interaction | UX and security rule |
| --- | --- |
| First pair | Local host approval and two-device comparison (not just QR), bound device public key, explicit grants |
| Reopen | Keystore-backed client identity; show actual host and connection type and last observation age |
| Terminal attach | Read and write are separate privileges; show full-account warning before granting LP4 |
| Begin input | Default-deny LP4 when authenticated key attestation unavailable. Verified per-use presence; brief exclusive claim TTL (candidate <=15 min, not a 7-day unattended input privilege); Ctrl/Esc/tab and paste guard; ownership indicator |
| Change network | No raw input automatically retransmitted; checkpoint/resync with "reconnected" and generation |
| Approve managed action | Present exact managed-authority approval summary and per-operation step-up; no native-dialog imitation |
| Prepare config request | Submit a versioned *proposal*, display "waiting for confirmation on the host"; cannot apply remotely |
| Host locked or logged out | Read/watch under valid separate grants; **LP4 terminal.write/create/close denied as temporary safe default** until a distinct owner-approved, independently reviewed absent-host policy exists. An active local login by itself is not a trustworthy human-presence signal under the same UID. This interim restriction does not satisfy the owner's final anywhere-control goal |
| Lost phone | Revoke future gateway grants from a trusted context; do not claim previously executed commands undone or host account clean. Inspect persistence, user services, SSH keys, hooks and accessible credentials |
| Notification | No command, path, prompt, output or recoverable content in push payload; local client fetches details only after authenticated open |
| Offline | Read-only last-known *metadata*, visibly stale; no local queue of arbitrary shell commands |
| App background/suspend | Work continues on host; reconnect and re-attest as Android lifecycle allows; no promise of perpetual Android background socket |

Android TEE/StrongBox attestation varies. LP4 requires verified
per-use authenticated key properties, not just possession. On
failure or software-only evidence, LP4 is denied by default until
an informed owner decision; an emulator cannot validate physical
hardware backing. Device presence is not physical host consent.

## 8. Tests to design and later execute

- **TS-HYB-01** LAN and remote flows produce identical gateway authorization
  decisions for same device/grants/request; overlay membership alone grants none.
- **TS-HYB-02** direct-to-relay and relay-to-direct switch does not change
  authenticated host, session generation, input ownership or scope.
- **TS-HYB-03** network interruption during terminal input: duplicate and
  reordered input is never written twice.
- **TS-HYB-04** revoked phone, stolen device key, replayed pairing code, stale
  device-presence signature, mismatched endpoint are rejected.
- **TS-HYB-05** full-account LP4 is labelled; LP1/LP2/LP3 cannot invoke `terminal.write`.
- **TS-HYB-06** host logged out with lingering: existing PTY remains; without
  lingering: UI reports stopped/unavailable honestly; no host confirmation from Android.
- **TS-HYB-07** 24h and 7d mobile reconnect/host-running with process and memory
  tracking, no hidden output-spool growth or fabricated 'running' state.
- **TS-HYB-08** terminal fuzz corpus against emulator parser: deterministic
  bounded behaviour, no file access, no command execution, no raw escape action.
- **TS-HYB-09** full screen apps across resize/disconnect/attach; visual fidelity
  compared with reference terminal and alternate screen semantics.
- **TS-HYB-10** Android device-key creation, presence unlock, new-enrolment
  invalidation, keystore denial and recovery across app reinstall.
- **TS-HYB-11** `config.request` remains host-confirmed, never auto-applied
  by LP4 or a mobile approval screen; explicitly report LP4 host-account bypass.
- **TS-HYB-12** relay operator cannot see plaintext terminal contents; packet
  capture and endpoint pinning analysis at the chosen transport.
- **TS-HYB-13** app/background lifecycle plus real Android keyboard, clipboard
  and accessibility; safe handling of paste bounds.
- **TS-HYB-14** offline, host crash, process crash, suspends, phone lock and
  gateway crash are distinct and do not resurrect dead sessions.

No test listed is a test performed. Existing TS-HOST/TS-REM cases remain mandatory.
A remote security review independent of this author and reference-device
measurements are required before adopting a transport.

## 9. Critical outstanding decisions

1. **DN-05:** H1 vs H2 (and alternatives if evidence disproves both).
   No remote transport approved.
2. **OQ-53:** Android supported presence-key enforcement and failure modes.
3. **DN-44 risk acceptance** beyond owner choosing the capability: exact
   operational full-account grants, life/expiry and physical loss story.
4. **DN-47 architecture:** emulator placement, limits and sandbox feasibility.
5. **DN-31:** replicated/snapshotted storage still blocks write path.
6. **Deployment:** local automatic network discovery should never mean
   unauthorised pairing or a broadcast of private project details.

The final product must pass these gates; this document cannot grant permission
to run their experiments or claim they are already satisfied.

## 10. Post-REJECT safety overlay (2026-10-14)

[42](42-POST-REJECT-CORRECTIVE-PLAN.md) records IR-02,
IR-06–IR-10 and their explicit STOP gates. The old 7-day
full-account LP4 option, in-memory-only gateway de-duplication,
host owned terminal-parser failure domain and implied logout
write access are **not accepted final design**. The owner's
functional target (remote full control on chosen devices)
remains; implementing it safely requires a real independently
reviewed security boundary, hardware/Android evidence, process
survival and restart tests. No deployment is authorised.

### 10.1 Interim lock/logout restriction is NOT final away-control acceptance

The owner needs remote terminal control from outside the
house even when the Linux desktop GUI is not open.
A permanently refused LP4-away capability does **not**
satisfy DN-44. Until a technically reviewed solution
exists, default-deny LP4 input on screen lock *and*
logout; a mere "session logged in" check by a
same-UID gateway is not an OS security boundary.
The final design must either securely offer
explicitly consented LP4-away or obtain the owner's
informed acceptance of the concrete restriction.
Device grant, input claim, attested presence,
gateway ownership and account-compromise recovery
are separate required contracts, not interchangeable.
See [the Codex audit](reviews/2026-10-14-post-reject-corrections-external-audit.md)
ER-01 through ER-03. No runtime test or ADR
has accepted this interim policy.

## 11. Docker delegation and LP4-away scope

The [observed CachyOS environment](reviews/2026-10-14-cachyos-native-feasibility.md)
gives the desktop user read/write access to a
rootful Docker daemon socket. An unrestricted
Android/PC LP4 terminal inherits that user's
Docker privileges and therefore may gain
**root-level host control**, not simply access
to files under its own UID. It is *not*
legitimate to label the remote account as
an ordinary nonadministrative sandbox.
No real Docker API call/exploit was run.

See [47](47-CACHYOS-DOCKER-PRIVILEGE-AND-SANDBOX-DECISION.md)
for preservation-of-Docker alternatives and
independent approval-authority constraints.
An isolated automated worker's mount view
can conceal Docker sockets, but does not
limit a separate unrestricted manually
launched native process or LP4 shell.

