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
  `systemd`). It recommends `bubblewrap`, which Local Tools needs, **0.11 or newer**: it uses
  `--overlay-src`; without it Local Tools cannot become Ready.
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
  host's libraries and on the pinned, GitHub-hosted build tools (see ADR 0026).
- Unsigned. Verify the checksum against the place you trust to have published it.

## Where the exact artifacts are described

The artifacts are **not** described in this repository by hash or by revision, because a
commit that said so would move the revision it describes. Git holds the process; each build
holds its own record, untracked, in `.artifacts/release/`:

| File                                | What it says                                                                                                                                                                                             |
| ----------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `wyss-console_0.2.0_amd64.deb`      | the package                                                                                                                                                                                              |
| `wyss-console_0.2.0_amd64.AppImage` | the AppImage                                                                                                                                                                                             |
| `SHA256SUMS`                        | `sha256sum` format; `sha256sum -c SHA256SUMS`                                                                                                                                                            |
| `release-manifest.json`             | version, the full source commit and its short form, authority digest, each artifact's size and digest, `signing: "unsigned"`, the toolchain, the pinned AppImage build tools as they ran, the host tools |

The rule that makes them the artifacts of this branch: they are built from a clean tree at
the commit that was pushed, nothing tracked changes afterwards, and `npm run release:verify`
refuses them if the manifest's commit is not the commit of the checkout it runs in. About
(Settings) shows the short revision they were compiled with; it is the start of the
manifest's commit. If they are rebuilt, they are a new candidate with new digests.

**Checksummed, not signed.** There is no release signing key or process.

## How they are built

`npm run release:package`, on Linux with `dpkg-deb`, `mksquashfs` and network access to
GitHub's release downloads for the first fetch of the pinned tools:

1. refuses a working tree that is not exactly the commit;
2. builds the authority, takes its SHA-256, builds the desktop pinned to it;
3. makes every AppImage build tool exactly the one pinned in `scripts/appimage-tools.json`
   (`AppRun`, `linuxdeploy`, `linuxdeploy-plugin-appimage` `1-alpha-20250213-1`, the AppImage
   runtime `20251108`), each from an immutable release address and against a committed
   SHA-256, hashed again on every use, then bundles with every proxy variable unreachable and
   Cargo offline (a proxy-aware download fails; a direct connection is not blocked, which is
   why the tools are hashed again afterwards and a printed download stops the release);
4. removes the bundler's second copy of the authority, writes both packages again, writes
   `SHA256SUMS` and `release-manifest.json`;
5. opens what it made (`npm run release:verify`).

Toolchain of the cloud build: Rust 1.97.0, Node v22.22.0, npm 10.9.4 (`package.json` asks for
Node 26 and npm 12; the engines are advisory and everything ran on the older ones),
`@tauri-apps/cli` 2.12.1, target `x86_64-unknown-linux-gnu`. The manifest records the exact
versions of the build that made it.

### What the content scan allows

Nothing by name or path. The archived planning document that planted a made-up key in a
test was redacted (`sk-ant-<FAKE_TEST_SECRET>`); a credential in that file now fails
`npm run release:verify`. The only exception is content-bound: the private-key blocks that
GnuTLS compiles into the `libgnutls` the AppImage carries, each listed by digest in
`scripts/reviewed-library-keys.json`.

### What was run, and what could not be

The build host was a disposable Ubuntu 24.04 container (x86_64) with Rust 1.97.0, Node 22.22.0
and npm 10.9.4 unchanged; the system packages added for it were `libwebkit2gtk-4.1-dev`,
`libayatana-appindicator3-dev`, `librsvg2-dev`, `libssl-dev`, `squashfs-tools`, `patchelf` and
`desktop-file-utils`. Run on the exact commit that was packaged:

| Check                                                                   | Result                        |
| ----------------------------------------------------------------------- | ----------------------------- |
| `format:check`, `lint`, `typecheck`, `build`                            | pass                          |
| `npm test` (vitest)                                                     | 32 files, 599 tests, all pass |
| `cargo fmt --check`, `cargo check --locked`, `cargo clippy -D warnings` | pass                          |
| `cargo test --locked --no-fail-fast`                                    | **570 pass, 40 fail** (below) |
| `lifecycle::` unit tests, 200 runs, 8 processes at once                 | no failure                    |
| `release:package`, then `release:verify` on the artifacts               | 75 checks, 0 failed           |
| An independent look at the packages (below)                             | as described                  |

**The 40 failures are the host, not the code.** Each one is one of two things, and none is a
regression:

- 33 need `bubblewrap` 0.11 or newer. The Local Tools sandbox uses `--overlay-src`; the
  host's distribution has 0.9.0 (`bwrap: Unknown option --overlay-src`, and a runtime that
  then exits before it can be attested) and the code requires `/usr/bin/bwrap`. They are 5 of
  the isolation unit tests and 28 of the process tests for approvals, events, runtime and turns.
- 7 need a real `systemd --user` manager (`lifecycle_process`). The host was not booted with
  systemd, so `systemd --user` refuses to run (`the system has not been booted with systemd`).

They have to be run on a machine that has both.

**Looked at independently of the verifier**, on the packages that `release:package` made:
`SHA256SUMS` checks; the `.deb` has no maintainer script, installs the desktop in `usr/bin`, the
authority in `usr/libexec/wyss-console`, a menu entry and icons, depends on `systemd`,
`libwebkit2gtk-4.1-0` and `libgtk-3-0` and recommends `bubblewrap`; the AppImage's first bytes
are the pinned runtime, byte for byte; the authority in both packages has the digest the
manifest names, and the desktop of each carries that digest; each package holds one authority.

**Started under Xvfb**, from the extracted `.deb` and from the AppImage
(`--appimage-extract-and-run`: there is no FUSE here), with an isolated home and no service
manager: the window opens, the terminal runs a shell, and the top bar says the authority
could not start, with the notice and Retry the interface promises. Nothing was installed or
written outside the application's own data directory: with no manager, no authority and no
unit are written. This is where the Settings view was seen drawn under every other view (see
[ADR 0026](../adr/0026-settings-and-release-closure.md#findings-of-the-release-build)).

None of this is an observation of a real desktop session, of `systemd --user` running the
authority, of Codex, or of KDE. Those are in [`LOCAL_ACCEPTANCE.md`](LOCAL_ACCEPTANCE.md), all
still `PENDING_LOCAL_ACCEPTANCE`.
