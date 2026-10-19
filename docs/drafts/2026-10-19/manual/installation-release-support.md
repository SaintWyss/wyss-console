# Release, installation, upgrades and support — F13 operational contract

**Status:** Complete release-process specification, **not an installed/published release**. Architecture [18](../../../architecture/18-DISTRIBUTION-AND-EXTENSIBILITY.md) and accepted ADR 0027 prevail. Public distribution requires owner DN-07, platform/license/security-policy decisions and first-user acceptance.

## 1. Proposed initial distribution scope

Primary platform candidate: Linux with `systemd --user`, tested KDE desktop; distributions/versions outside verified CI and native smoke are unsupported. Current code uses Linux-specific Unix peer identity, PTYs, inotify, GTK/WebKitGTK, `/proc` and managed isolation `bwrap` ≥ version recorded in architecture 18. A macOS or Windows port requires replacing all those trust and lifecycle assumptions; do not claim portability from Tauri alone.

Installed components: GUI package, host service, managed authority binary (with provenance constraints), private IPC endpoints, desktop entry/icon, user-service lifecycle and documented XDG stores. None may require root for ordinary use.

## 2. Supply-chain contract

- Pin reproducible Rust/npm dependencies and record toolchains/lockfile provenance. Ship SBOM/licenses and explicit bundled notices.
- Verify no private repo secret or personal path from tests is bundled.
- Build separate GUI/host/authority artifacts with expected hash and declared paths; no untrusted directories in executable resolution.
- Check package signatures/checksums and release process per owner-approved DN-07. A Git tag alone is not an authenticated distribution.
- Automated release workflows must not access users' auth files, KWallet, terminal data or provider secrets.

## 3. Installer checklist

1. Detect platform, required native libraries and service manager.
2. Explain exactly which files, user units, socket/host processes and local states will be created.
3. Check install-root ownership and ancestor trust; reject shared writable executable paths.
4. Install executables atomically, file modes and unit descriptors; no implicit network listener or provider configuration change.
5. Start GUI and optionally host/authority only with user-aware consent, respecting existing session processes.
6. Perform first-run onboarding: provider detection, terminal-only case, Native vs Managed explanation, opt-in encrypted store, supported versions and no surprise telemetry.
7. Log **nonsecret** install receipt and rollback instructions.

Do not embed a specific `systemctl --user` unit name or shell command until verified in the local package; the actual manifests from F13 become canonical.

## 4. Update and migration protocol

```text
Check release provenance
    ↓
Stage new package in private, verified location
    ↓
Collect versioned store backups + migration plan
    ↓
Drain or explicitly defer active host/authority
    ↓
Atomically switch eligible binaries and units
    ↓
Migrate stores under exclusive ownership
    ↓
Native smoke: terminal → host attach → config read-only → authority status
    ↓
Commit upgrade receipt; retain supported rollback window
```

No updating the running authority behind its back; no swallowing migration errors; no silently deleting old decryption keys or device grants. If migration fails, leave state recoverable and restore last compatible package/schema where proven safe. An incompatible newer store can force older build read-only rather than silently downgrade.

## 5. Uninstall

Present independent choices to retain/remove:

- Program files, icon and user services.
- Profiles, layouts and settings.
- Native session registry/log metadata.
- Encrypted restoration records and wallet key material.
- Remote device grants/certificates.
- Diagnostics, benchmarks and backups.

Stop or safely detach host/native sessions before removing its executable or service. Never kill all providers blindly. Explain when previously stored encrypted data cannot be recovered after key deletion. Do not promise physical secure erase on SSD.

## 6. Release quality gates

| Gate                       | Evidence                                                        |
| -------------------------- | --------------------------------------------------------------- |
| R1: source provenance      | Build commit, artifact checksums, dependency license audit      |
| R2: isolated clean install | Fresh supported non-owner user, no provider installed           |
| R3: real providers         | Claude and Codex detected/launched under actual permissions     |
| R4: host and authority     | Private peer identity, correct process owners, restart          |
| R5: upgrade                | N−1 → N with real-shaped sealed fixtures, no data loss          |
| R6: rollback               | Fail step-injection, restored package and compatible data       |
| R7: uninstall              | Explicit retained/deleted state, wallet handling                |
| R8: privacy and network    | No telemetry/listener/secret exposure by default                |
| R9: performance/a11y       | Accepted P1 budgets on supported reference machines             |
| R10: security maintenance  | Vulnerability intake channel, support window, update ownership  |
| R11: product walkthrough   | Core real GUI workflows, negative states and F10 where released |

Any failed R1–R8 blocks the affected release. F10b physical mobile acceptance must be separate from desktop package acceptance.

## 7. Documentation produced by a release

- Release notes: user-visible behavior, tested versions, known limitations.
- Versioned installation manual with exact **verified** commands, migration and uninstall.
- Screenshot/video acceptance with redacted fixtures and accessibility notes.
- Permissions/data-retention matrix; privacy policy matching actual telemetry and networking.
- Threat model updates and vulnerability reporting.
- Compatibility matrix with explicit unsupported states.
- Signed or otherwise authenticated provenance if selected by owner.

Publication, updating real users or accepting a software license for them cannot be inferred from a design branch. Owner explicitly authorizes these actions.
