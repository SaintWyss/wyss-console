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
  GitHub-hosted tooling Tauri fetches when it is built (see ADR 0026).
- Unsigned. Verify the checksum against the place you trust to have published it.

## This candidate's build record

Built in the cloud (Ubuntu 24.04, x86_64) from source revision **`818657b38fce`**, the commit
that held all of the code of this release. Commits after it change documentation only; the
artifacts were not rebuilt for them, so the build revision they report in About is the one
above. `npm run release:package` made them and `npm run release:verify` opened them again:
**60 checks passed, 0 failed**.

| Artifact                            | Size       | SHA-256                                                            |
| ----------------------------------- | ---------- | ------------------------------------------------------------------ |
| `wyss-console_0.2.0_amd64.deb`      | 5,682,292  | `2004bbe63c7a9314e938514a00bd19aa668da18b32c2d8a13684a15b9dedaba2` |
| `wyss-console_0.2.0_amd64.AppImage` | 86,063,608 | `3888e7533f201ded5fff87f71501cbd49f210bbfcd36a293e83a1c4a7484fbb3` |

The authority both carry (and the desktop is pinned to):
`7decca439c325aa652479159d52f1aabf9a7efe1be78794bdde9130b08a75537`.

**Checksummed, not signed.** `SHA256SUMS` and `release-manifest.json` are produced by the
build and are not committed: the artifacts are rebuilt from a commit, not stored in it.
They are in `.artifacts/release/` of the machine that built them.

Toolchain: rustc 1.97.0, cargo 1.97.0 (`--locked`, `Cargo.lock` as committed), Node
v22.22.0, npm 10.9.4 (`npm ci`, `package-lock.json` as committed), `@tauri-apps/cli` 2.12.1,
target `x86_64-unknown-linux-gnu`. (`package.json` asks for Node 26 and npm 12; this
environment had 22 and 10, and every command ran on them. The engines are advisory.)

What was checked in the artifacts themselves (see ADR 0026): the manifest, `SHA256SUMS` and
both files agree; the desktop and the authority are where the desktop looks, executable, one
of each; the authority is the pinned one and the desktop carries its digest; no credential,
fixture, source, database or local state by name or content (the checkout's 581 tracked
files included); neither program names the build machine; nothing writable by group or
others; the `.deb` has no maintainer script, is owned by root, and its menu entry
validates.

What was observed running: the packaged desktop (the `.deb`'s `usr/bin/wyss-console`, and the
AppImage's entry point after extraction) was started under Xvfb with an empty home and stayed
up. With no user service manager in the build container its bootstrap could not start an
authority, which is the case the product says it handles; nothing was installed. That is not
an observation of a real desktop session, `systemd --user` or Codex: those are in
[`LOCAL_ACCEPTANCE.md`](LOCAL_ACCEPTANCE.md).

### Reviewed findings of the content scan

- An archived planning document of an earlier system (`docs origins/.planning/phases/03-pipeline-de-ingesta/03-PATTERNS.md`,
  line 573) contains a test that plants a made-up `sk-ant-` string in a file to check it is
  not indexed. It is 43 characters where a real key has about a hundred, and it sits in a
  test named for planting a secret. It was judged a canary and not a credential, and is
  allowed by path and kind so that any second finding stops the release. **Whoever owns that
  document may want to confirm it and redact it.**
- The AppImage carries the build host's `libgnutls`, which compiles GnuTLS's own published
  self-test keys into the library. Allowed by library and kind.

### What the cloud could not run

The Rust suite on this build host has **40 failing tests, the same 40 that fail on a clean
`origin/main`** (checked by running both): 5 isolation tests and 28 process tests (told apart by their errors: an unknown `bwrap` option, or a Local Tools runtime that failed to start) need
`bubblewrap` 0.11 or newer (the Local Tools sandbox uses `--overlay-src`; this host has
0.9.0 and the code requires `/usr/bin/bwrap`), and 7 lifecycle tests need a real
`systemd --user` manager. They have to be run on a machine that has both. 569 tests pass
(42 more than on `origin/main`: the new ones).
