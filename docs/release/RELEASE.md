# Wyss Console 0.2.0 — release candidate

Status: **code complete and validated in the cloud; local acceptance pending**
([`LOCAL_ACCEPTANCE.md`](LOCAL_ACCEPTANCE.md)). This is a candidate, not a released
product. Nothing here has been tagged or published; both are for the owner to do.

## What this is

|                 |                                                                                                                   |
| --------------- | ----------------------------------------------------------------------------------------------------------------- |
| Version         | 0.2.0 (the first installable build; the repository had no tag and `0.1.0` was the scaffold)                       |
| Platform        | Linux x86_64 with a `systemd --user` session. No other platform is packaged or supported                          |
| Artifacts       | `wyss-console_0.2.0_amd64.deb`, `wyss-console_0.2.0_amd64.AppImage`, `SHA256SUMS`, `release-manifest.json`        |
| Integrity       | **Checksummed, not signed.** There is no release signing key or process                                           |
| Decision record | [ADR 0026](../adr/0026-settings-and-release-closure.md), [ADR 0025](../adr/0025-authority-lifecycle-bootstrap.md) |

## Environment it needs

- Linux with a login session that has a user service manager (`systemd --user`).
- The `.deb`: Debian 12 / Ubuntu 22.04 or newer (needs `libwebkit2gtk-4.1`, `libgtk-3`,
  `systemd`). It recommends `bubblewrap`, which Local Tools needs.
- The AppImage: built on Ubuntu 24.04 with that system's libraries inside it; use a system
  at least that recent. It needs FUSE 2 to be double-clicked, or `--appimage-extract-and-run`.
- Codex, installed separately, on the `PATH` the service manager gives its units (a desktop
  session imports it at login, which is where `~/.local/bin` is). It is not packaged.
- A display. Terminals and the managed surfaces run in the window.

## Installation

```sh
sha256sum -c SHA256SUMS --ignore-missing          # in the folder you downloaded to
sudo apt install ./wyss-console_0.2.0_amd64.deb   # or: chmod +x the .AppImage
```

The packages install no service and run no script. Installing, upgrading or removing one
writes nothing in anyone's home directory.

## First launch

Open **Wyss Console** from the application menu (or run the AppImage). No terminal,
repository, Cargo or npm is involved from here on.

```text
launch  ->  an authority answers?  yes -> connected (whoever started it)
                                   no  -> installed from the package, started, connected
```

1. The desktop finds the authority it carries, from its own executable: `usr/libexec/wyss-console/
wyss-control` below its prefix, and nowhere else. It checks that file's SHA-256 against
   the one it was built with and refuses it if it differs or if anyone else could have
   written it.
2. It installs that file once under `~/.local/share/wyss-console/authority/`, writes one
   user unit, `~/.config/systemd/user/wyss-control.service`, and asks the service manager
   to start it. Closing the window never stops it.
3. The top bar shows **Starting**, then **Connected**. If it cannot start, a notice above the
   views says why in plain words and offers **Retry**. The terminal works throughout.

New sessions start in **Inference**. **Local Tools** is chosen for a session on purpose and
every command and file change waits for your approval.

## Upgrade

Install the new package over the old one.

- If the previous authority is **running**, the window connects to it, says `update
pending`, and replaces nothing. A turn that is running is never interrupted.
- The next time no authority runs (after you log out and in, after `systemctl --user stop
wyss-control`, or after Retry following a crash loop), the authority carried by the new
  package is installed and started. Sessions, history and approvals' records are in the
  state directory and carry over.
- There is no hot update.

## Removal

- `sudo apt remove wyss-console` (or deleting the AppImage) removes the desktop only.
- The managed authority, its unit and your data are yours and are **not** removed. To
  uninstall the authority as well:

  ```sh
  systemctl --user stop wyss-control
  rm ~/.local/share/wyss-console/authority/wyss-control
  rm ~/.config/systemd/user/wyss-control.service
  systemctl --user daemon-reload
  ```

- Your preferences are one small file: `~/.config/wyss-console/preferences.json`.
- Your sessions, approvals' records and event history are in
  `~/.local/state/wyss-console/`. Delete that directory **only** if you want to lose them.
  Nothing in the product deletes it for you.

## Data preservation

| Action                                | Sessions, history | Authority, unit             | Preferences |
| ------------------------------------- | ----------------- | --------------------------- | ----------- |
| Close the window, reopen              | kept              | kept, running               | kept        |
| Upgrade the package                   | kept              | kept; new one on cold start | kept        |
| Remove the package                    | kept              | kept                        | kept        |
| Settings → Reset preferences          | kept              | kept                        | defaults    |
| Delete `~/.local/state/wyss-console/` | **lost**          | kept                        | kept        |

## Known limitations

- One authority runs one Codex runtime, in one mode and for one folder at a time.
- Nothing an approval decided is remembered; history cannot be searched or exported.
- Dark theme only; Settings has nothing to choose for appearance.
- The window and top bar say "Wyss AI"; the package and menu entry say "Wyss Console".
- An authority that is running is never updated in place; an incompatible one from this
  installation is replaced only on its next start.
- No `systemd --user`, no managed authority: the window says so and starts nothing.
- AppImage terminals inherit the library path its launcher sets. The AppImage depends on the
  GitHub-hosted tooling Tauri fetches when it is built (see ADR 0026).
- Unsigned. Verify the checksum against the place you trust to have published it.

## This candidate's build record

See the section below, written from the files that were produced.
