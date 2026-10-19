# ADR 0040: Installing, upgrading, rolling back and diagnosing, for one user (F13)

Status: Accepted (owner authorisation of the whole roadmap, 2026-10-19); extends ADR 0027
Date: 2026-10-19

## Decision

1. **One user, no privileges.** `scripts/install.sh` (copied into the package as
   `share/wyss-console/install.sh`) installs a package directory under `~/.local/opt/wyss-console`
   with a launcher link in `~/.local/bin`, a menu entry and an icon. Nothing is written outside
   the user's home; nothing needs root.
2. **Verified first.** The package's own list of SHA-256 digests is checked; one mismatch and
   nothing is installed. (The desktop still refuses an authority that is not the one it was built
   with, ADR 0025.)
3. **Upgrade beside, swap by rename, keep the old.** The new tree is staged next to the install,
   a running host is asked to stop (it closes its terminals; it is never killed), the old tree
   becomes `<prefix>.previous`, and the new one is renamed into place. `rollback` swaps them back.
4. **Removal is bounded.** `uninstall` removes the application, link, entry, icon and unit. The
   Console's own state is removed only with `--purge`. The agents' own configuration
   (`~/.claude`, `~/.codex`) is never touched, and a key in the Secret Service stays until its
   owner deletes it there (said in the output).
5. **The host can be supervised, optionally.** `--host-service` writes a `systemd --user` unit
   for `wyss-console --wyss-host` (restart on failure, not on exit code 2); enabling it is the
   user's command, printed, not run.
6. **Diagnostics without a window.** `wyss-console --diagnose` prints version and revision, the
   host's state, the agents found and their versions, the login shell, whether a Secret Service
   answers, the state directory's mode and counts, and the agents' folders' modes. Names,
   versions, counts and permissions only; never a value of a configuration or a key. Exit code 1
   when anything is a warning.
7. **Not decided here** (DN-07): a licence, public distribution, an in-app updater, sandboxed
   formats. The product is installed from a package the user built or was given.

## Verified

`tests/install-script.test.ts` (install, tampered file refused, upgrade, rollback, host stopped,
uninstall with and without purge, agents' configuration untouched) and a real package built with
`npm run desktop:package`, installed, upgraded over itself, diagnosed, rolled back and removed in a
temporary prefix.
