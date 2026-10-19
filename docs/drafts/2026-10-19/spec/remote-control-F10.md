# F10a/F10b — Remote agent/session control from desktop and phone (proposed specification)

**Status:** Proposal; no remote deployment authorized. Sources: architecture 16 (§§3–8), 19, 11 and 13. Depends on F9 host, owner decision DN-05; F10a precedes F10b.

## Scope and non-goals

F10a connects a Wyss Console client on another computer to one selected host's **agent/session workspace** with authenticated pairing, capability-specific grants, revocation and accurate session states. This is **not remote graphical desktop/screen sharing**; architecture 16 §3 explicitly excludes controlling the host graphical session. F10b adds a phone client after desktop remote validation.

Do **not** forward the raw authority socket, expose its credentials or create an unaudited remote shell. No public listener by default. Replicated configuration and remote control are separate workflows: neither silently syncs/restores local files across devices.

## Proposed topology

```text
Desktop/phone -- mutually authenticated encrypted channel --> Gateway (host)
                                                        |
                                               Capability router
                                                /             \
                                       Host-owned PTYs    Safe host operations
                                                |
                                        Authority boundary unchanged
```

The gateway is a dedicated process, independent of the webview and managed authority. Default state: no routable listener. An owner-selected private overlay is preferred to exposing LAN/public interfaces. External proxy, cloud relay or account is a separate decision.

## Pairing, identity, grants

`Not paired → Pairing requested → Locally approved → Active → Suspended/Expired/Revoked`.

- Host shows a short-lived challenge, requested permissions and device identity. A QR contains only expiring bootstrap data.
- Device identity is bound to an asymmetric key stored using appropriate platform-protected storage. Protect against replay, skew and stale requests.
- Device grants have host/capability/session scope, expiration, revocation and last-access visibility.
- Grant progressively: view_status → view_terminal → send_terminal_input → session_manage → config_mutation. Do not grant all by default.
- Destructive operations, provider writes, credential handling and managed approvals still require dedicated host policy/step-up, or stay unavailable.
- Revocation invalidates new and active connections immediately, including reconnect using stale credentials.

## Protocol and session lifecycle

- Version protocol, host generation, stable session ID and stream sequence. Reject unknown versions and replayed mutations.
- Bound frame size, terminal buffers, rate and per-client/global resources. Slow viewers must not stall a host PTY.
- Resume terminal output by valid retained offset; otherwise display a labelled gap.
- Input ownership and resize arbitration must be deterministic and visible.
- Distinguish host offline, gateway unreachable, device revoked, grant expired, client disconnected, session ended and session still active.
- Idempotent request IDs for mutating operations; reconnect must never duplicate terminal input or reapprove prompts.
- Pairing/auth and failure paths require rate limits and redacted diagnostics.

## Phone-specific behavior

Use secure device storage for identity, not permanent WebView state. On lock/background hide sensitive content. Dangerous actions require supported device authentication. Terminal can begin read-only; typing requires explicit entry. No terminal/prompts/secret text in notifications or crash reports. State OS screenshot/clipboard limitations honestly. Phone platform and physical acceptance need owner approval.

## Threats and defenses

Threats: stolen phone; revoked but connected device; replayed QR; compromised LAN/relay; DNS rebinding; gateway escalation; cross-host confusion; unbounded streaming; terminal command injection; impersonated remote approval; diagnostic leaks.

Controls: authenticated encrypted transport with host identity validation; explicit scoped grants; tight schemas and process isolation; same-user checks on internal IPC; logging only closed outcomes; default-off gateway and visible kill switch.

## Proposed acceptance (map to canonical TS-REM-01–13)

| Case | Required behavior |
| --- | --- |
| REM-A01 | Gateway off by default; local app unchanged |
| REM-A02 | Pair second desktop after local host confirmation |
| REM-A03 | Reconnect PTY with correct scrollback/gap |
| REM-A04 | View-only grant cannot type |
| REM-A05 | Revoke active device: drop stream and reject old credential |
| REM-A06 | Network loss does not end host process; UI says disconnected |
| REM-A07 | Retry/replay never duplicates actions |
| REM-A08 | Wrong host/expired grant/malformed frame fail closed |
| REM-A09 | Remote mutation cannot bypass native confirmation |
| REM-A10 | No secrets in gateway logs/errors |
| REM-A11 | Physical phone lock/pairing/reauth acceptance |
| REM-A12 | Gateway disable/uninstall revokes access and listeners |

## Delivery gates

1. Owner chooses DN-05 and allowed test network. Existing F9 host ownership (reported ADR 0031) is not by itself approval of a remote gateway.
2. Threat-reviewed gateway contract and offline-default local transport.
3. Desktop pairing, scoped grants, revocation, terminal streams and GUI acceptance.
4. Attack and recovery tests; independent security review; gateway-off rollback.
5. Owner selects phone platform; real-device acceptance.

Internet exposure, VPN installation/account, grants to real remote machines and mobile publishing require separate explicit authorization. Documentation is not operational permission.
