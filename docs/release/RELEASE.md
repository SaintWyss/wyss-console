# Wyss Console 0.2.0 — release candidate

Status: **code complete; built and verified in the cloud, where 40 Rust tests cannot run;
local acceptance not started** (0 of 19 controls, [`LOCAL_ACCEPTANCE.md`](LOCAL_ACCEPTANCE.md)).
This is a candidate, not a released product. Nothing here has been tagged or published; both
are for the owner to do.

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
- The AppImage: built in the release build environment (Ubuntu 24.04, below) with that
  system's libraries inside it; use a system at least that recent. It needs FUSE 2 to be
  double-clicked (`fuse2` on Arch-based systems such as CachyOS, `libfuse2t64` on Ubuntu
  24.04), or `--appimage-extract-and-run`.
- Codex, installed separately, on the `PATH` the service manager gives its units (a desktop
  session imports it at login, which is where `~/.local/bin` is). It is not packaged.
- A display. Terminals and the managed surfaces run in the window.

## Installation

```sh
sha256sum -c SHA256SUMS --ignore-missing          # in the folder you downloaded to
sudo apt install ./wyss-console_0.2.0_amd64.deb   # Debian, Ubuntu
chmod +x wyss-console_0.2.0_amd64.AppImage        # anywhere else (CachyOS, Arch: no apt, no .deb)
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

- `sudo apt remove wyss-console` (Debian, Ubuntu), or deleting the AppImage, removes the
  desktop only.
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

### The release build environment

Release artifacts are built in **one** environment, named in
`scripts/build-environment.json`: **Ubuntu 24.04, x86_64**. It is not a preference. The
AppImage carries the build host's shared libraries, and the one exception of the content scan
(below) lists, by digest, the private-key blocks of _that_ system's `libgnutls`
(`libgnutls30t64 3.8.3-1.1ubuntu3.6`). Built on another system, the AppImage carries other
libraries: on CachyOS the verifier found a `libgnutls.so.30` with private-key blocks that are
not in the list, and refused it, as it should. The list is not widened for other systems.

- `npm run release:package` refuses any other host (by `/etc/os-release` and `uname -m`)
  before it fetches or builds anything. `npm run desktop:package` still works anywhere.
- The manifest records `build_environment`: the system, the architecture, and the versions
  of the distribution packages whose files are in or made the packages.
- `npm run release:verify` refuses a manifest of any other environment.

On any Linux host with Docker or Podman, `scripts/release.Containerfile` is that environment,
from pinned inputs: the Ubuntu 24.04 image by digest, every distribution package it adds from
one moment of the Ubuntu archive (`snapshot.ubuntu.com`, `UBUNTU_SNAPSHOT`), Node and npm at
the versions `package.json` declares (enforced) and `rustup-init`, each by version and SHA-256,
Rust by release. The snapshot is only served over https, so `ca-certificates` is first taken
from the live archive; whatever that installed or upgraded is then put back to the snapshot's
version (downgraded if the archive has moved on), and the image is not built if any package
that is not the base image's differs from the snapshot. The checkout is mounted at
`/build/wyss-console`:

```sh
git status --short                 # must print nothing: a release is a commit
docker build -f scripts/release.Containerfile -t wyss-console-release scripts
docker run --rm -v "$PWD":/build/wyss-console wyss-console-release
ls .artifacts/release/             # only if it verified
```

(`podman` takes the same arguments.) The container needs the network for `npm ci`, Cargo's
crates and, the first time, the pinned AppImage tools; behind a TLS-intercepting proxy, give
the build its CA as the secret `extra-ca` and the run the same file through
`CURL_CA_BUNDLE`, `NODE_EXTRA_CA_CERTS` and `CARGO_HTTP_CAINFO`.

### What `release:package` does

`npm run release:package`, in that environment, with `dpkg-deb`, `mksquashfs` and network
access to GitHub's release downloads for the first fetch of the pinned tools:

1. refuses a working tree that is not exactly the commit, and a host that is not the release
   build environment;
2. builds the authority, takes its SHA-256, builds the desktop pinned to it;
3. makes every AppImage build tool exactly the one pinned in `scripts/appimage-tools.json`
   (`AppRun`, `linuxdeploy`, `linuxdeploy-plugin-appimage` `1-alpha-20250213-1`, the AppImage
   runtime `20251108`), each from an immutable release address and against a committed
   SHA-256, hashed again on every use, then bundles with every proxy variable unreachable and
   Cargo offline (a proxy-aware download fails; a direct connection is not blocked, which is
   why the tools are hashed again afterwards and a printed download stops the release);
4. removes the bundler's second copy of the authority, writes both packages again, writes
   `SHA256SUMS` and `release-manifest.json`, all in `.artifacts/release.staging/`;
5. opens what it made (`release-verify.mjs --repo`). Only a set that verified is moved to
   `.artifacts/release/`. A set that did not is moved to `.artifacts/release.rejected/` with a
   `REJECTED` note, and the command fails: a refused or unfinished build is never in the
   place a candidate is taken from. All three are removed once the checks of step 1 pass,
   before anything is built.

Toolchain: Rust 1.97.0, `@tauri-apps/cli` 2.12.1, target `x86_64-unknown-linux-gnu`. The
container pins Node 26.11.1 and npm 12.2.0, the engines `package.json` declares, and makes npm
refuse, not warn about, any engine a dependency declares and this toolchain does not meet
(`npm_config_engine_strict`). The host builds of section 1 below, the first container build and
the earlier cloud build ran Node 22.22.0 and npm 10.9.4, below those engines (npm warned). The
manifest records the exact versions of the build that made it.

### What the content scan allows

Nothing by name or path. The archived planning document that planted a made-up key in a
test was redacted (`sk-ant-<FAKE_TEST_SECRET>`); a credential in that file now fails
`npm run release:verify`. The only exception is content-bound: the private-key blocks that
GnuTLS compiles into the `libgnutls` the AppImage carries, each listed by digest in
`scripts/reviewed-library-keys.json`.

The checkout (`--repo`, `--repo-only`) is scanned as Git records it: every file of the
committed tree and of the index is read from Git's objects, binary content included, so a
file that was deleted or replaced on disk is still scanned as it was committed. Every tracked
path also has to be on disk as the kind of entry Git records, and is scanned there too. No
commit, an unreadable object, a submodule, an unmerged entry, a tracked file missing from or
of another kind in the working tree, or a file too large to read: each fails the scan.

### What was run, and what could not be

Three kinds of evidence, kept apart. None of them is local acceptance.

#### 1. This branch, in the cloud

Commit `bc1c79e` of `fix/settings-release-audit-closure`, 2026-10-11, on a disposable Ubuntu
24.04 x86_64 container with Rust 1.97.0, Node 22.22.0 and npm 10.9.4. After that commit come
this documentation and one change, to when `release:package` clears earlier output; the
branch's head is built and verified again, in the container, for its own candidate. The packages added to it for the build are the
ones `scripts/release.Containerfile` installs, plus `bubblewrap` (0.9.0, Ubuntu's) and the
`es_AR.UTF-8` and `en_US.UTF-8` locales for the tests.

| Check                                                                   | Result                                                     |
| ----------------------------------------------------------------------- | ---------------------------------------------------------- |
| `format:check`, `lint`, `typecheck`, `build`                            | pass                                                       |
| `npm test` (vitest) with `LANG=es_AR.UTF-8`                             | 32 files, 611 tests, all pass                              |
| `npm test` (vitest) with `LANG=en_US.UTF-8`                             | 32 files, 611 tests, all pass                              |
| `cargo fmt --check`, `cargo check --locked`, `cargo clippy -D warnings` | pass                                                       |
| `cargo test --locked --no-fail-fast`                                    | **570 pass, 40 fail**: the same 40 as before (below)       |
| `npm run test:pty`                                                      | 23 Rust and 10 frontend tests, all pass                    |
| `release:package` on the host, then `release:verify --repo`             | 80 checks, 0 failed; moved to `.artifacts/release/`        |
| Tamper controls on those artifacts (below)                              | 15 of 15 as expected: 1 accepted, 14 refused               |
| `release:package` on Ubuntu 22.04 (a container, clean tree)             | refused before building: not the release build environment |

**The 40 failures are the host, not the code**, and are the same 40 as on the previous
candidate, by test binary: 33 need `bubblewrap` 0.11 or newer (the Local Tools sandbox uses
`--overlay-src`; Ubuntu 24.04 has 0.9.0, and the code requires `/usr/bin/bwrap`): 5 isolation
unit tests and 28 process tests (`approvals_process` 5, `events_process` 2, `runtime_process` 20,
`turns_process` 1); each ends with the runtime unavailable or failed. 7 need a real
`systemd --user` (`lifecycle_process`: "the private manager did not start"). Replacing this
host's `/usr/bin/bwrap` with a 0.11 build was not permitted in this environment, so **these 40
were not run anywhere for this branch**; the last time they passed is in (2).

**Tamper controls.** Each copies the accepted set, changes one thing and, where a forger would,
rewrites `SHA256SUMS` and the manifest to match; `release:verify` has to refuse it. Refused: a
byte of the `.deb`; a `.deb` with another authority, with a maintainer script, with a credential,
with a world-writable desktop; an AppImage with another authority, with an unreviewed private-key
block in `libgnutls.so.30` (the CachyOS case), with a byte of its runtime changed; a manifest
naming another revision, another build environment, another authority, a signature, a build tool
that was not the pinned one; an artifact missing. The set as built is accepted.

The build environment was also built from `scripts/release.Containerfile` as it was at that
commit (image digest, snapshot `20261011T220000Z`, Node 22.22.0), and the same commit packaged in
it from a fresh clone: `release:package`
passed, 80 checks, 0 failed. Its manifest records the same revision, build environment, package
versions, toolchain and host tools as the host build. **The two builds are not bit-for-bit
identical**: the authority has the same size and differs in LLVM's internal symbol suffixes
(`.llvm.<hash>`) and therefore its build ID, so each build is its own candidate with its own
digests. Making the Rust build reproducible bit for bit is not done.

With the container's Node pinned to the declared engines (26.11.1, npm 12.2.0, strict), commit
`df0592d` (this branch's code after the review's findings): `npm ci` with no engine warning;
`format:check`, `lint`, `typecheck`, `build` pass; vitest 32 files, **612 tests, all pass** under
`LANG=es_AR.UTF-8` and under `LANG=en_US.UTF-8`.

Every one of these artifacts was of `bc1c79e`; `release:verify` refuses them at any later commit,
this documentation's included. The candidate of a commit is the set `release:package` makes and
verifies at that commit.

#### 2. Earlier evidence, not repeated here

- The previous cloud build (candidate `ca86b8c`, same toolchain): vitest 599 tests, all passing
  in that container's locale; `cargo test` 570 pass and the same 40 fail; `release:verify` on
  its artifacts reported as 75 checks, 0 failed; run as `release:package` runs it (`--repo`),
  that version of the verifier has 76 checks (counted again here, on this branch's artifacts).
- The independent audit of `ca86b8c`: vitest **598 pass, 1 fail under `es_AR.UTF-8`** (the clock
  test, fixed here) and 599 pass under `en_US.UTF-8`; `cargo test` **610 pass**, the whole suite,
  so on a host that had both `bubblewrap` 0.11 and `systemd --user`; on CachyOS,
  `release:package` completed and `release:verify` gave **75 passed, 1 failed**
  (`libgnutls.so.30`, unreviewed key blocks), which led to the release build environment above.
  `main` (`a8b507e`) had 500 vitest tests, all passing.

These are as reported; they were not re-run for this branch, and the counts of the verifier
differ between versions of it (it now has more checks).

#### 3. Not observed by anyone yet

A real desktop session, `systemd --user` running the packaged authority, Codex, KDE, an install
from the menu, an upgrade, an uninstall: every line of [`LOCAL_ACCEPTANCE.md`](LOCAL_ACCEPTANCE.md),
in both environments, is `NOT RUN`. The previous candidate's window was started under Xvfb from
its packages (no service manager: the window said the authority could not start, with Retry); that
was not repeated here and is not acceptance either.
