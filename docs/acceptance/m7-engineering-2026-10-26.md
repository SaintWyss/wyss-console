# M7 engineering evidence — 2026-10-26

## Baseline and result

This separate branch is based on the open M6 engineering head
`328c54e35658fbd2b7ef00dad94676505702079d`. M6 engineering checks passed, but
M6 is not merged, has no independent security/architecture approval and has no
real-provider acceptance. M7 changes do not promote those findings.

M7 adds cryptographic identity and terminal-delivery primitives plus a native
Android client shell. OD01 still blocks selecting a network transport. The
Android app ships with no Internet permission and no gateway connection
implementation; pairing, remote reads and all effectful actions are visibly
unavailable. It does not establish production remote access.

## Implemented

- **Two device identities:** QR pairing now binds distinct P-256 SEC1 connection
  and presence public keys. Invalid/off-curve, malformed, identical or swapped
  keys fail before host confirmation. Both fingerprints are shown to the host
  confirmation interface; both keys remain revoked and non-reenrollable after
  revocation. `ring` 0.17.14 verifies DER ECDSA/SHA-256 per-use presence
  signatures. Private device keys never enter the Rust gateway core.
- **Native Android application:** `android/` is a Java/Android application with
  Hosts, Pairing, Projects, Sessions, Terminal, Pipelines, Approvals, Devices
  and Settings screens. It is offline by construction (no `INTERNET`
  permission), secure-window enabled, uses native accessibility headings,
  horizontally scrollable screen navigation and one-column content scrolling,
  and clears rendered in-memory content on background.
  Android Keystore creates separate non-exportable P-256 connection and
  per-use-authenticated presence keys when explicitly invoked. It labels
  attestation as absent; security level alone is not treated as hardware
  attestation or LP4 eligibility.
- **Terminal input contract:** `RemoteInputSequencer` bounds each request to
  4,096 bytes, permits a one-shot payload read, identifies each request and
  session-generation/connection-epoch/sequence, reports exact accepted bytes,
  and maps partial or impossible acknowledgements distinctly. A lost ACK or
  reconnect consumes the in-flight bytes and requires fresh user input; the
  sequencer has no persistence or retry method.
  Closing and reading the payload now share one synchronized transition: if the
  first-send read wins, reconnect reports `OUTCOME_UNKNOWN`; if reconnect wins,
  the retained handle cannot release bytes. A new request after reconnect cannot
  make the stale handle sendable again; the Java regression exercises that case
  and races first-send against reconnect 250 times.
- **H1/H2 comparison:**
  - H1, an owner-configured private overlay, reuses its own reachability and
    Android VPN integration but adds a third-party client/control-plane
    dependency and its account lifecycle. Tailscale documents Android 8.0+
    support and a VPN-configuration setup; its current reference plans list
    Personal at $0 (up to six users), Standard at $8/user/month and Premium at
    $18/user/month. This does not assert the owner's current plan.
  - H2, iroh/QUIC, now has first-party Kotlin bindings and lists Android as a
    supported platform. The published transport uses end-to-end TLS; its relay
    comparison lists public shared service as free/rate-limited/no-SLA, managed
    cloud from $19/month, and open-source self-hosting with operator-owned
    infrastructure and availability.
  - No reference-device CPU/RAM/battery/idle/latency/roaming data is available
    for either option. These sources establish advertised support and price,
    not Wyss's security, quality or performance. OD01 remains the owner choice.

Sources checked 2026-10-26: [Tailscale Android setup](https://tailscale.com/docs/install/android),
[Tailscale plans](https://tailscale.com/pricing), [iroh Kotlin/Android support](https://www.iroh.computer/blog/iroh-language-support),
[iroh relay hosting](https://www.iroh.computer/services/hosting).

## Security boundary and remaining blockers

- **Transport/authentication is BLOCKED:** no TLS/Noise handshake, endpoint pin,
  host key store/rotation, channel binding, encrypted reachability, relay or
  socket exists. `ring` signatures and QR HMAC do not authenticate a deployed
  channel. OD01 and a transport-specific ADR plus independent security review
  are required.
- **Pairing authenticity is NOT_PROVEN:** the host-side confirmation and QR
  comparison are typed/fixture policy only. A same-UID/native process can still
  target same-user APIs; no claim that it cannot impersonate an owner or host
  confirmation is made. The current pairing MAC is a transport-neutral
  foundation, not production enrollment.
- **Android API is not connected:** generated identity methods are not called
  during app open, no QR scanner is wired, no notifications or remote output
  exist, and the user cannot enroll or reach a PC. Build/device/lifecycle and
  KeyStore behavior are NOT_PROVEN on actual Android hardware.
- **OD02 remains BLOCKED:** full-account terminal input while host is locked or
  logged out remains unavailable. No weak owner-presence or remote approval
  path was added.
- Desktop now exposes a nine-area Remote PCs view with explicit disconnected
  states and disabled actions. It has no gateway connection; the existing
  `remote/` core is still unconstructed outside its tests.

## Acceptance status

| Contract                                                          | Status                  | Evidence or exact blocker                                                                                                                                                                                            |
| ----------------------------------------------------------------- | ----------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| TS-REM-01..02 QR and host-confirmation policy                     | PARTIAL, fixture-tested | Dual-key MAC/point validation is added; no live transport, host confirmation boundary or Android enrollment.                                                                                                         |
| TS-REM-03 short code                                              | BLOCKED                 | No approved PAKE implementation or short-code flow.                                                                                                                                                                  |
| TS-REM-04..07 grants, revocation and request policy               | PARTIAL, fixture-tested | Existing remote core tests retained; no live attachment/API bridge.                                                                                                                                                  |
| TS-REM-08 channel/MITM/channel-binding                            | BLOCKED                 | No selected transport or handshake.                                                                                                                                                                                  |
| TS-REM-09 replay and ambiguous action                             | PARTIAL                 | Existing request replay tests plus Android no-replay input tests; no gateway, PTY and process-crash integration.                                                                                                     |
| TS-REM-10..16 managed, durable journal, key rotation and recovery | NOT_PROVEN / BLOCKED    | No connected authority gateway, persistent host-key/device registry or rotation protocol. Existing policy fixture tests do not prove live behavior.                                                                  |
| TS-REM-17 terminal escape/screen fidelity                         | NOT_RUN                 | No Android renderer or live terminal stream.                                                                                                                                                                         |
| TS-REM-18 Android cache/privacy and full listed states            | PARTIAL                 | Empty disconnected states, secure window, horizontal navigation and foreground memory clearing exist; real content cache, notifications, screen lock and five-minute background behavior are not implemented/tested. |
| TS-HYB-01..14                                                     | BLOCKED / NOT_PROVEN    | OD01 transport selection and actual host/phone are dependencies; no real network or device experiment.                                                                                                               |
| Android Keystore P-256 keys                                       | IMPLEMENTED, NOT_PROVEN | API code creates separate keys and mandates device auth for presence key; CI builds/tests the SDK target, but physical device/TEE/attestation behavior remains untested.                                             |

## Verification

- `cargo test --locked --manifest-path src-tauri/Cargo.toml --bin wyss-console remote::`:
  **45 passed, 0 failed**. Includes a delayed host-dialog acceptance at the exact
  120-second expiry boundary, real ring-generated P-256 signature checks,
  malformed/off-curve key refusal, dual-key pairing and revoked/reused-key
  regressions, request/replay policy, and the versioned bounded protocol.
- `npm run check:rust`: **PASS**. Rust formatting, locked dependency check,
  core/integration tests, B1 fixture suite (136 tests) and Clippy with warnings
  denied all completed successfully.
- `npm run check`: **PASS**. Prettier, ESLint, Svelte/TypeScript checks, 68 Vitest
  files / 1,077 tests, owned harness (16 tests), Claude bridge (77 tests), and
  Vite production build. The build reports the repository's existing >500 kB
  chunk advisory.
- `npm run scan:repo`: **PASS**, 5 checks / 0 failures; all 1,620 tracked files
  were scanned from Git and the working tree after the implementation commits.
- Java `RemoteInputSequencer` standalone smoke compilation/execution: **PASS**.
  A separate 5,000-cycle concurrent reconnect/send smoke also passed. Neither
  standalone run substitutes for the Android/JUnit build.
- The final Android JUnit suite additionally covers a stale unsent request handle
  after reconnect followed by a fresh request; the stale handle cannot release
  either payload, while the fresh handle is acknowledged. Android GitHub Actions
  run `40289210209` passed on final source head
  `0a6d5f748dfdd7870f1dcb04c8a38034a94c8a77`, including the Android build and
  JUnit suite. M1–M5 integration run `40289260949` is still in progress: its
  frontend/contracts and scanner jobs passed, while the Rust job remains at
  runner dependency installation. Do not treat that run as a full CI pass.
- `npm audit`: 0 known vulnerabilities after locking the compatible Vite
  8.3.5 and undici security updates.
- Android SDK/Gradle are unavailable locally. GitHub Actions run
  `40276535354` passed on implementation head
  `467b6c6ff816328a513d536375c915d4500bceeb`, including `testDebugUnitTest`
  and `assembleDebug`; this is the native-app build and JUnit race regression.
  The M1-M5 integration run `40276535146` also passed on that head, including
  frontend/contracts, scanner, full Rust/B1 and Clippy. These runs prove the
  committed sources compile and pass deterministic CI, not behavior on a phone.
  No physical Android device or terminal has been exercised, so device/Keystore
  behavior remains **NOT_PROVEN**. The current M1–M5 GitHub run is tracked
  separately above; the evidence update below changes no Android or Rust source.
- Independent Sol High static review at final source head `0a6d5f7` found no
  actionable issue; this is a static review result, not a formal GitHub
  approval, hardware acceptance or production security sign-off. Two resolved
  Codex PR threads identified clipped horizontal navigation and stale-handle
  payload release; source fixes are in `bdf975b` and `09e4c80`, and the final
  stale-versus-fresh regression passed Android CI on that source head. Resolving
  those threads is not merge approval or production security sign-off.

No provider, remote network, existing service, credential or production data
was touched.

## Preservation and readiness

Managed production concurrency remains one. F8 authority mediation and existing
approval paths are unchanged. Native automation trust boundaries are unchanged.
No automatic retry after ambiguous terminal input was added. The PR is for
independent review of transport-neutral foundations and an offline native client
shell. It is neither production-ready nor a claim of complete FP-REM acceptance.
