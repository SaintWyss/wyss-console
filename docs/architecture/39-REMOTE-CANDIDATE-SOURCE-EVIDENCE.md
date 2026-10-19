# 39 — DN-05 transport candidates: source-verified facts and unverified gaps

Date: 2026-10-13. **Static documentation / source review only.**
The owner authorised bounded isolated technical verification after paper
consolidation. The following is verified only as _upstream source/documentation
statements_, not as observed operation, performance, security or integration
on the owner's CachyOS desktop or native Android device. No package
installed, binaries run, network endpoint contacted or device setting changed.

Reference contracts and goals:
[32](32-OWNER-PRODUCT-DIRECTIONS-2026-10-13.md) (DN-05 open),
[35](35-HOST-REMOTE-AND-MOBILE-DELTA.md) (H1/H2 candidates),
[36](36-PLANNING-CLOSURE-AND-VERIFICATION-GATES.md) (gates).
The requirement is **transparent use from anywhere without manually toggling a VPN
per connection**, not necessarily "no VPN at all".

## 1. Source evidence with stable revision

| Evidence                                                                                                     | Claim that the source actually supports                                                                                                                                                                  | Level                                                              |
| ------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `n0-computer/iroh-ffi` at commit `3103bf5295be6d50c5272ff7a426e9b539f3f587`, `support-matrix.yaml`           | A declared Kotlin binding covers endpoints, protocols/ALPN, multipath, custom relays, services                                                                                                           | Source read, NOT Android runtime observation                       |
| Same commit, `README.kotlin.md`                                                                              | Android cross-compilation is documented through Android NDK and targets including aarch64/armv7/x86; examples target Android API 29                                                                      | Source read, NOT tested on device                                  |
| Same commit, `Cargo.toml`                                                                                    | FFI version `1.1.0`; Rust core dependency listed as `iroh = 1.0.0`; Android JNI/NDK-specific dependencies; license MIT OR Apache-2.0                                                                     | Source read; check actual installed/runtime versions before use    |
| Same commit, `README.md`                                                                                     | Kotlin/JVM Maven coordinates documented: `computer.iroh:iroh`; project expressly includes Kotlin/Swift/Python/Node bindings                                                                              | Source read                                                        |
| `iroh` Rust 1.3.0 API docs: https://docs.rs/iroh/latest/iroh/                                                | An endpoint connects over QUIC, using relay discovery and direct NAT traversal where possible, with authenticated endpoint IDs; the application's authorisation remains the application's responsibility | Provider documentation, NOT actual LAN/P2P result                  |
| iroh security/privacy docs: https://docs.iroh.computer/deployment/security-privacy (historical URL may move) | Relay data is end-to-end encrypted, but relays see metadata; public shared relays are **not recommended by maintainers for sensitive/confidential data**                                                 | Cached provider docs; confirm latest canonical URL before adoption |
| https://tailscale.com/docs/features/tsnet                                                                    | `tsnet` embeds Tailscale into **Go** programs, not a turnkey Kotlin/Rust Android SDK                                                                                                                     | Provider docs validated July 2026                                  |
| https://tailscale.com/docs/install/android                                                                   | Tailscale has a separately installed Android app that enables a device-level VPN configuration                                                                                                           | Provider docs validated January 2026                               |
| https://tailscale.com/docs/concepts/traffic-routing-through-tailscale                                        | Tailscale normally uses direct encrypted links and relays through DERP when direct routing is impossible                                                                                                 | Provider docs validated January 2026                               |

Source URLs:

- https://github.com/n0-computer/iroh-ffi/blob/3103bf5295be6d50c5272ff7a426e9b539f3f587/support-matrix.yaml
- https://github.com/n0-computer/iroh-ffi/blob/3103bf5295be6d50c5272ff7a426e9b539f3f587/README.kotlin.md
- https://github.com/n0-computer/iroh-ffi/blob/3103bf5295be6d50c5272ff7a426e9b539f3f587/Cargo.toml
- https://github.com/n0-computer/iroh-ffi/blob/3103bf5295be6d50c5272ff7a426e9b539f3f587/README.md
- https://github.com/n0-computer/iroh-ffi
- https://docs.rs/iroh/1.3.0/iroh/

## 2. What the evidence narrows

**H1 — automatic external private overlay (Tailscale client)** remains
a feasible UX candidate. The user needs a one-time client installation and
VPN configuration; after that, per-connection manual intervention is not
necessarily required. It can serve a Rust host gateway over a private
device IP without embedding tsnet in Android. How background Android
reconnection, VPN permission and battery restrictions affect Wyss has
**not been tested**.

**H2 — embedded iroh transport** is also a feasible UX candidate:
Rust on Linux plus the official Kotlin/Android FFI is documented
upstream and provides encrypted P2P with relay fallback. Its
`EndpointId` authentication is network-peer authentication, **not**
permission to operate a Wyss terminal; the latter needs the independent
Wyss gateway device grant, host pinning and LP4 safeguards.
Building and packaging native NDK binaries, JNI compatibility, app
background lifecycle and version skew are new support obligations.
Neither Kotlin FFI documentation nor Rust QUIC docs demonstrate
production Android reliability.

**H2 privacy constraint**: do not presume public shared relays are
acceptable for shell access simply because they cannot decrypt payloads.
They still see connection metadata, and maintainer documentation warns
against their use for highly sensitive data. Study a trusted/self-hosted
relay or owner-approved operator and its operational and recovery cost.
No relay provider/vendor choice has been made.

**No H3/H6 transport framework should be invented** before H1 and H2
are exhausted by actual constraints. Sarmiento remains optional device
inventory only; no source proves RustDesk hbbr is a generic QUIC
relay for Wyss.

## 3. Current comparison without fabricated metrics

| Factor                                | H1 external automatic overlay                              | H2 iroh embedded                                                        |
| ------------------------------------- | ---------------------------------------------------------- | ----------------------------------------------------------------------- |
| Android client                        | Separate Tailscale VPN app; Wyss app talks through tailnet | Native Kotlin FFI documented; Wyss owns integration                     |
| Network path                          | P2P direct where possible, DERP relays otherwise           | P2P direct where possible, iroh relays otherwise                        |
| Additional infrastructure             | Tailnet identity/control plane and client daemon           | Endpoint discovery/relay configuration, key lifecycle, Android JNI      |
| Initial user setup                    | Device VPN approval and tailnet enrolment                  | Wyss pairing plus endpoint enrolment, maybe trusted relay configuration |
| Gateway permissions                   | Required independent LP4 grants                            | Required independent LP4 grants                                         |
| Operator data exposure                | Relay/routing metadata; must review Tailscale docs         | Relay/routing metadata; upstream cautions on shared public relays       |
| p50/p95 latency, RAM, battery, loss   | **UNMEASURED**                                             | **UNMEASURED**                                                          |
| Host logout / crash recovery          | **UNTESTED**                                               | **UNTESTED**                                                            |
| Total ownership cost                  | **UNKNOWN**                                                | **UNKNOWN**                                                             |
| Android network migration correctness | **UNTESTED**                                               | **UNTESTED**                                                            |

## 4. Precise future isolated experiment plan (not executed)

With the owner's already-granted permission for disposable
environments, these tests can be prepared but need a _reachable_
isolated Linux target and Android test device/emulator with no
personal credentials/configuration. Do **not** run on user's actual
host session or phone without naming the target and agreement on
side effects.

- E-REM-01: pin Rust core + Kotlin binding versions; build test
  app on Android emulator and desktop throwaway container,
  verify QUIC endpoint identity and zero added shell privileges.
- E-REM-02: prove only legitimate paired Wyss device gets
  application-level session grants; reject raw iroh endpoint ID
  with no application pairing.
- E-REM-03: compare direct same-LAN, remote/NAT-simulated and
  relay-only cases; record p50/p95 connect/reconnect, CPU/RAM,
  bytes sent, errors and FFI crash behaviour on app background.
- E-REM-04: flip from direct to relay mid-input; replayed input
  batch may never execute twice. Test stolen phone key and
  host-key pinning mismatch.
- E-REM-05: denial/recovery when relay metadata exposure is
  unacceptable; measure self-hosting feasibility and costs.
- E-REM-06: contrast H1 Tailscale one-time VPN setup and automatic
  Android reconnect, plus authenticated gateway access and
  revocation under the same fixtures.

Each must run on a disposable host with separate keys and no
provider account, no private files and no existing Wyss service;
collect a reproducible pass/fail record and cleanup. If an
Android emulator/device is unavailable, explicitly mark the
Android-specific questions **BLOCKED**, not PASS.

## 5. Gate and recommendation status

**DN-05 remains OPEN.** The upstream evidence is sufficient
to rank H1/H2 as comparative final candidates, and to correct
the assumption that Android requires inventing Kotlin bindings
for iroh. It is **not** sufficient to select a final transport,
recommend a particular hosted relay, claim low latency or
accept production security. No ADR has been accepted.
