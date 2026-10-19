# Operator, recovery and troubleshooting handbook (draft)

**Purpose:** Procedures for safe local operations and future remote host deployment. This is not a script; do not assume command names/flags until checked against `README.md`, local HEAD and `--help` of the installed build. Owner reports F9 host mode as `wyss-console --wyss-host`; detailed service placement remains governed by ADR 0031.

## 1. Operator invariant

The host service owns native PTYs/session registry and survives GUI close; `wyss-control` remains a separate Managed authority with its own policy. Closing the GUI does not revoke provider processes; stopping a session does not delete a profile. The remote gateway (if ever enabled) is a third independent trust boundary.

**Never** reactivate old custom security-run/model-preflight approval controllers, remove ACLs or launch a privileged shell as a casual fix. The trusted `agent` ACL co-writer exception is owner-configurable; any change requires source and ownership verification.

## 2. Preflight without modifying state

1. Record installed package/source revision and OS, user, desktop session and provider versions.
2. Check whether the GUI connects to its local F9 host or uses window-owned fallback.
3. Observe existing native sessions in the GUI and distinguish Active, Stopped, Failed, Lost or Ended by signal.
4. Inspect configuration adapter read-only status, provider version support and stale indicators.
5. Check whether the OS wallet is unlocked if encrypted restoration/credentials are needed. Do **not** print its contents.
6. Read recent closed error categories/redacted diagnostics, not raw terminal output or full environment.

Do not run tests against real provider settings that create or delete entries without backups and a documented undo path.

## 3. Failure diagnosis matrix

| Symptom | First check | Safe recovery | Avoid |
| --- | --- | --- | --- |
| GUI cannot attach host | Host process, private socket owner/mode, generation | Relaunch GUI/approved host; explain fallback | Reusing stale socket as proof of life |
| GUI closed but agent still active | Expected F9 ownership | Reopen and reattach | Killing host to "close" the UI |
| Provider terminal is Lost | Host generation/process liveness | Confirm actual state, explicit restart | Claiming agent was stopped |
| Output gap on reattach | Host scrollback bound/stream offset | Show gap; resume live stream | Replaying fabricated missing bytes |
| Profile refuses launch | Provider capability and overlay risk | Edit profile; launch supported variant | Silently omitting unsupported controls |
| Config file read-only | Owner, ancestor perms, ACL named writers, symlink | Fix trust only with owner decision | Blind `chmod g-w ~/.claude` |
| Undo conflicts | Provider revision differs from snapshot | Preserve new bytes; review/apply new plan | Force overwrite |
| KWallet unavailable | OS key service state; key identity | Unlock/restore backed-up key | Plaintext fallback |
| Managed execution unavailable | Authority lease/attestation/policy errors | Follow accepted authority runbook | Native silent fallback or zero-hook bypass |
| Device revoked | Gateway grant state | Pair again only by host approval | Restoring old grant from backup |
| High idle CPU | Host/webview/provider process split, X11/render mode | Measure against P1 baseline | Confusing provider CPU with UI |

## 4. Controlled restart and recovery

### GUI only

Close/reopen GUI while host stays running. Verify a real provider process retains identity, scrollback matches and pane tree restoration doesn't duplicate sessions. If no host exists, make a clear nonpersistent fallback.

### Native provider process

Use the Console's Stop/Start again and exact selected session. Signal handling and final state must reflect actual provider exit. No generic `pkill` or PID-only decision.

### Host service

Before planned stop, identify active PTYs and tell the owner their fate. Save metadata where supported, then stop using the installed user-service mechanism. On restart use a new host generation; old processes must be honestly marked unrecoverable unless actual reconnection contract proves otherwise.

### Managed authority

Only the established authority upgrade/restart and migration procedure applies. The desktop must not replace `wyss-control` while live or tamper with event/audit state. Never repair by deleting its database.

## 5. Backups and restore

Treat independent stores separately. Provider global config should be backed up prior to a reversible mutation. Console profiles/layouts/journals encrypted entries use their owners' safe export. KWallet keys are not necessarily in filesystem backup; record secure recovery limitations.

**Restore sequence:** identify version → verify backup/hash and ownership → ensure no concurrent provider writes → dry-run plan → native confirmation if risky → atomic application → verify source and provider effect → log safe receipt.

If source changed outside since backup, restore conflicts and leaves newer data untouched. If key unavailable, sealed content stays unavailable; no guessed decryption.

## 6. Operational constraints for remote F10 (future)

Gateway starts disabled, binds only owner-authorized private interface, and has per-device grants. Before enabling: independent threat review, documented firewall/overlay, test device pair, revocation drill and owner-signed network scope.

Connectivity loss must not kill host-owned work. A stolen phone response: disable gateway if necessary, revoke device credentials/grants, review authorized recent access, rotate any affected access secrets. Do not send terminal contents or credentials via public issue reports.

## 7. Logs and diagnostics

Safe diagnostic output includes build/provider versions, component health, bounded timings, typed errors, aggregate counters and permissions summary without sensitive principals. Exclude stdout/stderr of agents, prompts, file contents, env, auth headers, secrets, free-form commands and project paths unless user explicitly previews a redacted excerpt.

Retain locally with documented rotation/limits; exports are opt-in and previewable. Logging cannot create a new source of truth for process state.

## 8. Release and rollback checks

- Installer/uninstaller and supported platform verified on a fresh user.
- Host+authority binaries, permissions, systemd user units, socket modes and XDG paths checked.
- Backup/migration tested with data from version N−1.
- CLI providers not required for terminal-only functionality.
- No network gateway, updater, telemetry or credentials collection enabled by default.
- Rollback path proven for code and schema, with incompatible schema read-only refusal.
- Vulnerability reporting channel and dependency license policy approved before public release.

## 9. Evidence operators must record

Revision; host generation; provider versions; exact action and timing; test environment; expected/actual process lifecycle; source hashes (not bytes); error code; follow-up; data retention/rollback state. The user should not be asked to disclose API keys, credentials, full transcript or unrelated home files to debug the app.
