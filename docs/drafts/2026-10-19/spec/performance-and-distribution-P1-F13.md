# P1 and F13 — Performance baseline and distribution readiness (proposed)

**Status:** Proposal expanding architecture 15, 18 and 19. No publication, installer, updater, signing key or new network service is authorized by this document. The reported F9/T1/T2 implementation requires normal-hardware measurements before performance acceptance.

## 1. P1: what to measure

Measure the actual Console, host-owned PTYs, rendering and provider startup on each supported reference machine. Before optimization, collect a reproducible baseline and a comparable reference terminal (selected with owner consent).

At minimum:

- Cold application startup, first interactive terminal and first painted frame.
- Time from Start Agent click to actual provider process and first usable terminal.
- Input-to-render latency with a calibrated method (OQ-27).
- Terminal output throughput, scrollback/search behavior and long-output responsiveness.
- Split/resize/reconnect costs, including F9 host attach.
- CPU idle with hidden and visible workspace; CPU under agent/output load.
- Memory for desktop webview, backend, host, PTYs and each provider process **separately**.
- Multiple-session resource growth, host crash/reconnect recovery.
- Time to inventory/configuration changes and watcher notification.
- Cold/steady-state storage I/O, update and import/export impact where relevant.

Use canonical metric identifiers and definitions from architecture 15; this supplement is not a replacement for them.

## 2. Measurement protocol

For each measurement record hardware, kernel, GPU/render mode, compositor, monitor scale, provider versions, power state, foreground/background, run sequence, tooling revision and sample count.

Run warm-up trials then enough repeated samples for median and p95; record raw numbers, failure counts, variance and test fixture. Distinguish cold from warm runs. Never compare a software-rendered nested-display screenshot benchmark with a hardware-accelerated reference without a warning.

P1 questions:

- OQ-26: whether a timestamp probe may ship. Candidate: debug-only instrumented builds, absent in release.
- OQ-27: external input latency method. Candidate: controlled camera/tester vs internal instrumentation, both labelled.
- The reference terminal must be selected/installed with consent.

Propose revised budgets based on measured distributions; only the owner accepts them. Optimize confirmed bottlenecks instead of changing renderer or adding native dependencies by intuition.

## 3. F13: distribute a trustworthy Linux product first

Dependency: product accepted by its first user; DN-07 (license/platform/format/channel/update/security support). Existing local build artifacts are not public releases.

Delivery should include:

1. A reproducible, pinned build and license/dependency review (including bundled code).
2. Installable package and verified per-user systemd units, host and authority lifecycle. No writable-by-group executable lookup paths.
3. Supported platform/distro list with actual tests. macOS/Windows require separate equivalents to Unix sockets, peer credentials, systemd, namespaces, inotify and GTK/WebKitGTK.
4. Fresh-user onboarding that does not require an AI provider or change provider config; terminal remains usable when agents are absent.
5. Versioned schema migrations with rollback/backup for profiles, host sessions, encrypted restoration and credentials.
6. Updater strategy with authenticated provenance, rollback and no updating an active authority unsafely. No updater enabled before DN-07.
7. Uninstall preserving/deleting user state based on an explicit decision; show key-store entries and retained data.
8. User-previewable diagnostics free of prompts, tokens, transcripts, env values, raw paths outside approved roots and credential bytes.
9. Accessibility checks, support promise, vulnerability reporting and security maintenance policy.

No telemetry or Internet listener by default.

## 4. Proposed distribution acceptance (map to canonical TS-DIST-01–06)

| Case     | Assertion                                                                |
| -------- | ------------------------------------------------------------------------ |
| DIST-A01 | Fresh install without Claude/Codex: terminal works, files unchanged      |
| DIST-A02 | Provider detection accurately reports absent/unsupported versions        |
| DIST-A03 | Correct packaged binaries, services and authenticated IPC                |
| DIST-A04 | Upgrade preserves/restores profiles, journal, host and encrypted entries |
| DIST-A05 | Unsupported schema fails safely, no silent rewrite                       |
| DIST-A06 | Uninstall explains and follows data/key retention choice                 |
| DIST-A07 | Exported diagnostics secret-free, previewable                            |
| DIST-A08 | Package provenance and non-writable executable paths verified            |
| DIST-A09 | Keyboard, font scaling, dialogs and accessibility pass                   |
| DIST-A10 | No unsolicited telemetry, listener or auto-update                        |

F13 needs a genuinely fresh supported user environment, a repeatable install/update/uninstall demonstration and owner acceptance — passing `npm run desktop:package` alone is insufficient.

## 5. Decisions and gates

P1: reference terminal, OQ-26/27 and performance budgets. F13/DN-07: license, first platforms, packaging formats, signing, update channel, support contact and publication. Anything public-facing (download URLs, endpoints, accounts, store submission) requires separate approval.

Continue local build/test tooling without publishing while these decisions remain open.
