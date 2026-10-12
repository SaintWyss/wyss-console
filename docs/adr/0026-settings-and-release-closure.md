# ADR 0026: Settings and release closure

Status: Accepted for the architecture. The code meets its acceptance rule
([below](#acceptance-of-this-decision)) except for one gate: 40 Rust tests that need
`bubblewrap` 0.11 and a real `systemd --user` have not been run on this branch's head (they last
passed on `ca86b8c`, in the independent audit). The release is **not** accepted: 0 of 19 local
acceptance controls have been run, see [Cloud and local](#cloud-and-local-validation).
Date: 2026-10-10 (release closure revised 2026-10-11)

## Decision

Wyss Console gets a Settings view for the preferences of the desktop and becomes an
installable product: a `.deb` and an AppImage that carry the desktop and the authority it
was pinned to, with checksums, a release manifest and a verifier.

```text
install  ->  launch  ->  the authority bootstraps (ADR 0025)  ->  use  ->  close, reopen, reboot
                                  |
                  Settings: a typed, bounded, local set of preferences
                  that change how the product looks and where it opens,
                  and never what it is allowed to do
```

Nothing of the core changed: not the control protocol, runtime supervision, approvals,
turn admission, authority state or isolation. The only code outside the new module that
changed is one visibility keyword (`lifecycle::descend`, which the preference store
reuses) and the way `Provision::of_build` finds the executable it starts from, so that the
layouts of every package format could be tested (the rule is the same).

The documents archived in `docs origins/` were used as history and nothing else. When they
disagree with the code, the ADRs or the README, the code wins; no assumption of the
earlier Python, hexagonal or multi-agent systems entered this phase.

## Preference is not authority

```text
preference  ≠  authority          what is shown      ≠  what is allowed
observed    ≠  enforced           capability         ≠  authority
unknown     ≠  supported          events             ≠  canonical state
```

A preference can change convenience. It cannot weaken a boundary, and the type has no
place to put anything that would.

| Is a preference (desktop's)                                    | Is not, ever (authority's, or nobody's)                                                  |
| -------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| The view the window opens on                                   | A default of Local Tools: new sessions are Inference, Local Tools is chosen once         |
| Terminal font size, scrollback, cursor style and blink         | A shell, a program, a directory or an environment for a human terminal                   |
| How the time of an event is written, which filter it opens on  | Event retention, auditing, the stream, what the authority records                        |
| (nothing about approvals)                                      | Always allow, auto approve, remember, trust a project, skip a confirmation               |
| Whether to confirm a reset (not a setting: it is always asked) | A workspace, a root, a sandbox profile, an execution lease, a safety profile             |
|                                                                | The authority's executable, socket, unit, service name, provider command, control method |

Anything with security significance that someone proposes for Settings needs its own
architectural phase. A preference that cannot decide anything may be added by extending
the vocabulary below, in the open, with its test.

### Ownership

The store belongs to the desktop. `wyss-control` does not read, hold or know it, and
nothing cosmetic was added to the authority. Preferences are not in the canonical store,
not in `localStorage`, not in the repository and not at a path the window chooses.

## The schema

`schema_version: 1`, explicit typed fields, no free text anywhere:

```text
interface.landing_page      console | sessions | agents | approvals | events      (default console)
terminal.font_size          integer 10..=24                                       (default 16)
terminal.scrollback         integer 500..=50000                                   (default 5000)
terminal.cursor_style       block | bar | underline                               (default block)
terminal.cursor_blink       boolean                                               (default false)
events.time_format          system | h12 | h24                                    (default system)
events.default_filter       all | sessions | runtime | turns | approvals | security (default all)
```

The defaults are what the product looked like before it had preferences. Settings is not
a landing page: it is reached on purpose. There is no density and no theme: only one of
each is designed, and offering a choice that does nothing is not a preference.

Rust is the judge of every value (`Preferences::validate`). The window parses the same
bounds and refuses to send an invalid set, but nothing relies on it. The three requests
deny unknown fields; the update takes one whole `Preferences`, so a key that is not in
the type (`auto_approve`, `default_mode`, `shell`, `cwd`, `environment`, `workspace`,
`retention_days`, ...) fails before any code runs. The vocabulary is spelled out in a
Rust test and in a TypeScript test, and the two lists have to be edited together.

### Versioning

A file of a **newer** `schema_version` is not interpreted, not overwritten and not
moved: the defaults apply, the Settings page says so, and changes are refused until the
user resets (`read_only`). A file that is damaged, oversized, missing a field, with an
unknown field, out of bounds or of version 0 is **set aside** as `preferences.invalid.json`
(one place, replaced by a later one), the defaults apply and the page says so. Unknown
fields are never ignored inside a known version: the safe reading of a file nobody here
wrote is not to use it.

## Storage

`$XDG_CONFIG_HOME/wyss-console/preferences.json` (default `~/.config`), 0600, in a
directory of this user's, 0700. Nothing else is stored there and no secret ever is.

- **Reading** opens the file without following a link and judges the descriptor it
  reads: a regular file, this user's, mode exactly 0600, one name, at most 4 KiB. A link,
  another owner, readable or writable by others, a second hard link, a directory: the
  file is not read (`unsafe`) and the next change replaces it. A directory that is not
  safely this user's makes everything `unavailable`: defaults, and nothing is written.
- **Writing** uses a name of its own, flushes it, renames it over the old file and flushes
  the directory. The live file is never truncated; a reader and a crash see the old file
  or the new one. A write interrupted before the rename changes nothing and leaves no
  file behind.
- **Leftovers** of a writer that died (`.preferences.<pid>.<n>.tmp`) are removed by their
  exact name pattern, but only the regular files of this user that are over a minute old:
  another desktop of the same user may be writing one right now. A first version of this
  code removed them at once, and a test with a concurrent writer found it.
- **A read that loses a race with a write** opened the old file just before it was renamed
  over, and saw a file with no name left. That is a replaced file, not an unsafe one: the
  read starts over (five times at most) and also checks that the path still names the file
  it read. This too was found by the concurrent-reader test, one run in six.
- **One lock** serializes every read, update and reset in the process, so an update
  racing a reset leaves one of the two outcomes and never a mixture; what is in memory
  is what is on disk. Two desktops of the same user are last-writer-wins, and a change
  made to the file while a desktop runs is not noticed until the next start.
- **Reset** removes `preferences.json` and the file set aside, and nothing else. It does
  not touch sessions, the event history, the canonical store, the authority, its unit,
  the provider login or any workspace. A link in the file's place is removed, never what
  it points at.

## The window's door

Three commands, the whole of the settings capability:

```text
settings_read    -> SettingsView { schema_version, storage, preferences, about }
settings_update  <- { preferences }          (the whole typed set)  -> SettingsView
settings_reset   <- {}                                              -> SettingsView
```

`storage` is one of `ready | recovered | newer | unsafe | unavailable`; failures are
`invalid | read_only | not_saved | busy | internal`. No path, error text or file name
crosses. `about` is build constants: the version, a short source revision when the build
knew one, and the platform. The capability list stays a list of this application's own
commands: no `core:`, `fs:`, `shell:`, `process:`, `dialog:` or `opener:` permission was
added, and a test fails if one is.

Preference changes never touch the authority: they restart nothing, call no lifecycle
intent and read no managed state. The window reads the preferences once and writes only
when the user changes one; nothing polls and nothing is watched.

## What the window does with them

- **Landing page**, applied once, when the preferences arrive, and only if the user has not
  already moved: a late answer never takes someone somewhere they did not go.
- **Terminal**: a small appearance store each renderer follows for its lifetime. A font size
  change refits the terminal through the existing resize path; scrollback and cursor are
  xterm options. Human PTYs remain the desktop's; no preference reaches them.
- **Events**: the clock follows the time format; the first filter is applied to the loaded
  events. It narrows what is shown and nothing the authority keeps.

## The Settings view

Sections: **Appearance**, **Interface**, **Terminal**, **Events**, **Authority**,
**About**, **Reset**. Appearance has no control: the product has one theme, and a light
one would be a redesign, not a preference; the page says so. A short paragraph at the top
states what preferences never change.

**Authority** is read-only and in the product's words: connected or not running, the Codex
runtime (ready, starting, stopped, or not available while offline), who installed it
(managed by Wyss Console, or started outside it), compatibility ("This Wyss authority is not
compatible with this version of the app."), its version and protocol while connected, and
that an update waits for the next start. It shows no socket, unit, path, process,
environment, credential or raw output, and a test walks four authority states to prove it.
**Retry** is shown where the lifecycle already offers it. There is no Stop and no
Restart: the lifecycle (ADR 0025) deliberately has none, and Settings did not add any.

## Product pass

No visual redesign. The interface no longer names the internal service (`wyss-control`)
to a user: it says "the authority". Offline states on Sessions, Approvals and Events say
that the local terminals keep working. The Agents subtitle says what it is for. Sidebar
placeholders (a disabled item, a "Soon" tag) are gone. A test visits every view in four
authority states and refuses development vocabulary, raw enum or error codes and the
internal name. The window and the top bar still say "Wyss AI" while the package, the menu
entry and About say "Wyss Console": which is the product's name was left to its owner.

## Packaging

`npm run release:package` builds the authority, takes its SHA-256, builds the desktop
again with that digest compiled in and bundles it, in the same step, with Tauri 2's
bundler as a `.deb` and an AppImage. Three things are done to what the bundler makes:

1. **The bundler's second copy of the authority is removed.** Tauri puts every binary of the
   crate in `usr/bin`, which would leave an authority on `PATH` that no build pinned and
   nothing installs. The packages are written again (`dpkg-deb --root-owner-group -Zxz`;
   `mksquashfs -root-owned`, zstd, with the pinned runtime below), with the commit time
   as the time of every file.
2. **The menu entry** is named `wyss-console.desktop` in the `.deb`.
3. The desktop's `bundle.linux.deb.files` / `appimage.files` place the authority.

Layout, the same below every prefix:

```text
<prefix>/bin/wyss-console                        directory package: prefix = the folder
<prefix>/libexec/wyss-console/wyss-control       .deb:              prefix = /usr
                                                 AppImage:          prefix = the mounted image's usr
```

### The AppImage build toolchain is pinned

Tauri's AppImage bundler runs executables it did not build, and out of the box it fetches
them when it runs. Found when this was closed: `AppRun` and `linuxdeploy` from release assets
of `tauri-apps/binary-releases`, the AppImage plugin from the **floating `continuous`**
release of `linuxdeploy-plugin-appimage`, and, one level down, `appimagetool` inside that
plugin fetching the AppImage **runtime** from the floating `continuous` release of
`AppImage/type2-runtime`. The runtime is the first thing an AppImage executes, and the
`continuous` one changed between two builds made a day apart. A verifier of the output
cannot make up for running an unpinned build tool, so the tools themselves are pinned.

`scripts/appimage-tools.json` (committed) lists every such executable:

| Tool                          | Release (immutable tag) | Notes                                                                           |
| ----------------------------- | ----------------------- | ------------------------------------------------------------------------------- |
| `AppRun`                      | `apprun-old`            | Tauri's `tauri-apps/binary-releases`                                            |
| `linuxdeploy`                 | `linuxdeploy-07333c6`   | The release Tauri 2.12 expects; run unchanged but for the one byte change below |
| `linuxdeploy-plugin-appimage` | `1-alpha-20250213-1`    | A tagged release; it contains `appimagetool`                                    |
| `type2-runtime`               | `20251108`              | Handed to `appimagetool` as `LDAI_RUNTIME_FILE`; the image has to begin with it |

and, for each: one fixed `https://github.com/<owner>/<repo>/releases/download/<tag>/<asset>`
address, the SHA-256 of what is downloaded, the SHA-256 of what runs, and a size bound. The
two shell plugins Tauri writes itself (`gtk`, `gstreamer`) are pinned by digest too, as
what `@tauri-apps/cli` 2.12.1 produces. The expected digests are in the repository, not
fetched; a table that names `continuous`, `latest`, a branch, plain http, another host, a
query, a missing or malformed digest or an unbounded size is refused before anything runs,
and a test holds the committed table to that.

What `release:package` does with them:

```text
tool directory of the bundler (private, 0700)
  every entry that is not a pinned download is removed (the scripts Tauri writes, strangers)
  each pinned tool: lstat (a link or directory is removed, never followed) -> hash what is there
      exact pinned digest? use it   else remove it and fetch
  fetch: /usr/bin/curl, https only, redirects only to GitHub's release hosts, bounded size and
      time, to a name nothing else uses; no second address
  verify the SHA-256 of what arrived -> apply the one change Tauri would make to linuxdeploy
      (zero the AppImage magic bytes, so it runs without FUSE) -> verify the SHA-256 of what
      will run -> written executable to its own name, flushed, renamed into place -> verified again
the bundler runs with every proxy variable unreachable and Cargo offline
  (not a network namespace: a direct connection is not blocked, the re-hashing below is)
  it prints a download? the release stops.   a tool differs afterwards? the release stops
  every pinned tool, and the two scripts it wrote, are hashed again against the table
the image is checked to begin with the pinned runtime
```

A cache is therefore only a way to avoid downloading: it is hashed on every use, and what
fails the hash is replaced or the build stops. Applying the byte change to linuxdeploy
ourselves, on a file whose digest is pinned, means the bundler's own in-place change leaves
the file as it is, so the file that is hashed before is the file that is run. The runtime:
`appimagetool` writes the MD5 of the image it built into one 16-byte field of it. The image
is written again here, so the pinned runtime is used byte for byte, and the bundler's copy
must differ from it in that one field and nowhere else. The field is the runtime's
`.digest_md5` ELF section, located from the section headers of the pinned runtime itself
(exactly one, of 16 bytes, inside the file); every byte outside it, including the bytes
next to it, has to be the pinned byte, and a pinned runtime whose field cannot be located
pins nothing. A first version accepted any difference that fitted in a window of 16 bytes
anywhere in the file, which would have let a patch of the runtime's code through.

Not pinned, and recorded in the manifest instead: the build host's own `dpkg-deb`,
`mksquashfs`, `patchelf` and `strip` (path, version line, SHA-256), which come from the
distribution's packages, and the shared libraries the AppImage carries from the build host.
They are not downloaded; a build on another host will say so in its manifest.

### The release build environment

The AppImage carries the build host's shared libraries, so where it is built decides what it
contains. Release artifacts are built in one environment, `scripts/build-environment.json`:
**Ubuntu 24.04, x86_64**, the system whose `libgnutls` private-key blocks were reviewed. A build
on CachyOS completed and was refused by the verifier, for a `libgnutls.so.30` with blocks the
list does not have; that is the content-bound exception working, and the answer is not to
widen it but to build where it holds. So:

```text
release:package   another host (os-release, uname -m)?      refused before anything is built
                  manifest.build_environment                 system, architecture, package versions
                  artifacts -> .artifacts/release.staging/   verify -> .artifacts/release/ (accepted)
                                                                    or .artifacts/release.rejected/ + REJECTED
release:verify    a manifest of another environment?         refused
```

`scripts/release.Containerfile` builds that environment from pinned inputs on any host with a
container runtime: the Ubuntu 24.04 image by digest, the distribution's packages from one
snapshot of its archive, Node and npm at the versions `package.json` declares (enforced, not
advisory) and `rustup-init`, each by version and SHA-256, Rust by release.
Only a set that verified is ever in `.artifacts/release/`.

### Provenance is unchanged

The desktop finds its authority from its own real executable (`/proc/self/exe`, links
resolved) and nowhere else: `<exe>/../../libexec/wyss-console/wyss-control`. That is the
rule ADR 0025 accepted, and it already holds for all three layouts, so it did not change.
The authority is read once, through a descriptor opened without following a link, from a
file of this user or root that nobody else can write, and its SHA-256 has to be the one
compiled into the desktop. The managed install is then a copy under `~/.local/share`
that runs under the user's service manager, independent of the package it came from.

Tests prove: the layout resolves in the directory, `.deb` and AppImage shapes; a missing
payload, a payload that is not the pinned one, a link in its place, a writable payload and
a writable directory are refused; the payload is found from the real file behind a link
and never from the link's neighbours; a build that cannot say where it runs is invalid and
not "development"; and the packager's configuration places the authority exactly where
that rule looks (`tests/release.test.ts` reads both).

### Verifier

`npm run release:verify` (run by `release:package`) opens the produced files:

- the manifest, `SHA256SUMS` and each artifact agree (digest, size);
- the `.deb` has the expected control data (name, version, `Depends: systemd`, the web
  view, `Recommends: bubblewrap`), no maintainer script, everything owned by root, a valid
  menu entry and an icon; each of these is read with `dpkg-deb`, and a read that fails is a
  failure, never an empty (and therefore clean) answer;
- the AppImage unpacks without FUSE or a display;
- in both: the desktop and the authority are where the desktop looks, executable; the
  authority's digest is the manifest's pin and the desktop contains that pin; there is
  exactly one authority and one desktop;
- no forbidden name (`.env`, `auth.json`, keys, databases, `.git`, caches, fixtures, tests,
  sources, lock files, acceptance state), no credential-shaped content in any file,
  binaries included, neither program names the checkout or a build machine (the verifier's
  own checkout, and any home directory, `/root`, un-remapped Cargo or rustup directory or Cargo
  target directory, wherever the build ran), nothing writable by group or others or set-id,
  no link leaving the package;
- the manifest names the commit the source is at (`git rev-parse HEAD`), the short revision
  is its start, the desktop was compiled with that revision, and the tree was clean;
- the manifest says it was built in the release build environment
  (`scripts/build-environment.json`, below);
- every build tool in the manifest is the pinned one, by version, source and digest, as it
  ran; the table itself is pinned; the bundler ran with every proxy unreachable and Cargo offline; the AppImage begins with
  the pinned runtime;
- with `--repo` (or `--repo-only`): no credential file is tracked and no tracked file holds a
  credential-shaped string, historical documents included. What is scanned is what Git
  records (every blob of the committed tree and of the index, read from Git's objects) and
  what is on disk for every tracked path; a tracked file missing from the working tree, of
  another kind there, unreadable, unmerged, a submodule, or no commit at all, is a failure.

Findings name a file and a kind, never the bytes. A probable live credential stops the
release.

**No exception stands on a path and a kind.** An earlier version excused one archived
planning document because it plants a made-up `sk-ant-` string in a test that checks a
secret is not indexed. That would also have excused a real key added to the same file. The
string was instead redacted (`sk-ant-<FAKE_TEST_SECRET>`, nothing else in that document
touched) and the exception removed; a test puts a credential of each of five providers in
that file and requires the scan to fail. The one remaining exception is bound to content:
GnuTLS compiles its own published self-test keys into the `libgnutls` the AppImage
carries, and a private-key finding in that library is dropped only if every block in it,
from its BEGIN line to its END line, has a SHA-256 listed in
`scripts/reviewed-library-keys.json`. An unlisted block, or one that does not end, stops the
release.

### Checksums and signing

`SHA256SUMS` (the `sha256sum` format) and `release-manifest.json`. The artifacts are
**checksummed but unsigned**: there is no release signing key or process, none was
generated, and the configuration carries no signing or updater setting (a test fails if it
does). A checksum next to the file proves a download is the one that was published only as
far as the place it was published is trusted.

### The artifacts are of the final commit

Git describes the process; the artifact manifest describes the binary. The order is fixed:

```text
all source and documentation committed -> tree clean -> pushed
  -> release:package from that exact HEAD -> release:verify -> nothing tracked changes afterwards
```

`release:package` refuses a tree with any uncommitted or untracked change before it fetches
or builds anything, compiles `WYSS_BUILD_REVISION` (shown in About) from `HEAD`, and fails if
`HEAD` or the tree moved while it ran. It writes, untracked, `SHA256SUMS` and
`release-manifest.json`: the version, the full commit and its short form, the authority
digest, each artifact's name, size and digest, `signing: unsigned`, the toolchain, the pinned
build tools as they ran, and the host tools. `release:verify` requires the manifest's
commit to be the commit of the checkout it runs in, so an artifact from an earlier commit
is refused, including after a documentation-only commit. For that reason no tracked
document records an artifact's hash or revision: a commit that did would move `HEAD`.

### Upgrade, uninstall, data

Upgrade is ADR 0025's cold-start model, unchanged and not hot-updated: an authority that
runs is never replaced and the window says `update pending`; the next time none runs, the
packaged one is installed and started and canonical state is preserved. The packages
install no service and no maintainer script, so installing, upgrading or removing a
package changes nothing in anyone's home. Removing the package removes the desktop; the
managed authority, its unit and the canonical state remain until removed by hand
(documented in `docs/release/RELEASE.md`). Nothing erases history silently.

### Version

`0.2.0`. The repository has no tag and no earlier release, and `0.1.0` was the scaffold
the work began from. This is the first set of installable artifacts, the product changed
in capabilities (3A to 3H) and compatibility between 0.x versions is not promised, so the
next minor of the 0.x line is the honest number. `1.0.0` would claim an acceptance that
has not happened. No tag was created and history was not touched.

## Cloud and local validation

| Verified in the cloud                                                         | Needs the real machine (`LOCAL_ACCEPTANCE.md`)               |
| ----------------------------------------------------------------------------- | ------------------------------------------------------------ |
| Compilation, formatting, lint, typecheck, unit and process tests              | A real `systemd --user` bootstrapping the packaged authority |
| Preference store behaviour, atomicity, concurrency, hostile files             | The user's Codex login; Inference and Local Tools Ready      |
| The settings view against a scripted desktop, including every refusal         | A managed turn and an approval against real Codex            |
| Packages built, opened and checked; layout, digests, secret and fixture scans | Installing and launching from the application menu           |
| The provenance rule over three layouts                                        | Executing the AppImage (needs FUSE and a display)            |
| That the window is granted no framework permission                            | KDE/KWin native blur; 960×640 and narrow layouts by eye      |
|                                                                               | Upgrading from a running previous authority; uninstalling    |

Two limits of the cloud host are recorded rather than hidden. Its `bubblewrap` is 0.9.0
and the Local Tools sandbox needs 0.11 or newer (it is looked for at `/usr/bin/bwrap`, which
was not replaced), and it was not booted with systemd, so no `systemd --user` can run. Forty
tests of the Rust suite fail there for those two reasons (33 and 7, by test binary in
[`RELEASE.md`](../release/RELEASE.md)); the 570 that pass include every test this phase
added. The independent audit ran the whole suite, 610 tests, on a host that has both, and
they passed: that was the candidate `ca86b8c`, before this closure's fixes, none of which
touches the code those 40 tests exercise. It is not evidence for a later commit until it is
run again there. The previous candidate's packaged desktop was started under Xvfb with an
isolated home, and stayed up; no authority could be started without a service manager. None
of this stands in for the local acceptance.

The cloud did not run Codex: nothing in this phase touches runtime launching or the
provider protocol, so a paid provider run would have repeated 3G's evidence and shown
nothing new.

## Findings of the release build

Building and running the candidate found these, each reproduced first, fixed minimally and
held by a test that fails without the fix:

- **The pinned-runtime check was a window, not a field.** It accepted any difference that
  fitted in 16 bytes anywhere in the runtime: sixteen patched bytes of its code, or a window
  across the end of `.digest_md5` and the start of `.upd_info`. It now allows exactly the
  `.digest_md5` section, found in the pinned runtime's own ELF section headers.
- **The credential scan failed open.** A file larger than its read limit was skipped, and a
  file with more than 50,000 places to look at stopped being read, both reporting clean. Both
  are findings now, in the package scan and in the build-host scan.
- **"No network" was not what the bundler had.** It has no proxy route and Cargo offline,
  which stops clients that honour them, not a direct connection. The manifest says
  `proxies-unreachable`; what covers the rest is the pinned tool set hashed again afterwards.
- **Retry was offered and did nothing.** The authority lifecycle published a failed attempt,
  with Retry on, a few instructions before the attempt's thread let go of its turn. The turn
  is now released in the same step, under the view's lock, that publishes how the attempt
  ended. Two tests that raced the same window, or assumed the view is a log, were made to say
  what they mean; a terminal test that assumed `/bin/sh` is bash (dash leaves by itself when
  its terminal goes away) now uses bash.
- **Settings was drawn under every other view.** Every view stays mounted and is hidden with
  the `hidden` attribute, and each class that sets `display` needs a `[hidden]` rule that
  undoes it; Settings had none. Nothing in jsdom could show it, because jsdom does not apply
  the stylesheet. It was seen in the packaged window under Xvfb. A test now requires the rule
  for every view the window hides.
- **The `.deb`'s root directory was 0700**, the private mode of the build's temporary
  directory. It is 0755 and the verifier checks it.
- **The README said** there was no authority or agent UI, that Codex was unusable from the
  window and that the desktop never starts the authority. It no longer does.

The independent review of that candidate found more, each reproduced here first and held by a
test that fails without its fix:

- **The checkout scan failed open.** It listed tracked paths with `git ls-files` and read each
  one from the working tree, skipping any that was not a regular file there: a committed file
  holding a credential, deleted on disk, let `--repo-only` pass. It now reads what Git records
  from Git's objects, and requires every tracked path on disk.
- **A `.deb` that could not be read passed as clean.** The exit status of `dpkg-deb -e`, `-f`
  and `-c` was ignored, so an unreadable control archive meant "no maintainer script".
- **The build-host check knew one host.** It looked for the verifier's own checkout path only;
  verified anywhere else, a program naming another build machine's paths passed (an existing
  test required that). It now also looks for any build machine's paths.
- **The release was reproducible only by accident.** Nothing said where it had to be built,
  and a build on CachyOS made a complete-looking set of artifacts that verification refused,
  left where an accepted one would be. Now the environment is named, enforced, recorded and
  buildable from pinned inputs, and a refused set is moved aside.
- **A frontend test assumed English.** The 12-hour clock test expected `AM`/`PM`; under
  `es_AR.UTF-8` the clock correctly says `p. m.`, and the test failed. The tests now ask the
  locale for its day period, digits and calendar; production did not change.
- **A development build kept naming an old commit**: `build.rs` watched `.git/HEAD`, which a
  commit on the current branch does not change. It now watches the branch's ref too.
- **At 960×640 the reset confirmation's buttons were below the fold.** It now scrolls into view
  when it opens, and Cancel has the focus.

Its pull request's review found two more, fixed the same way:

- **A tracked link changed on disk was not scanned.** Git stores a link as its target and the
  committed target was scanned; a target changed in the working tree was not. It is read with
  `readlink`, never followed, and scanned.
- **The container did not meet the declared engines.** It ran Node 22.22.0 and npm 10.9.4 while
  `package.json` (and several locked dependencies) declare more; npm only warned. It now pins
  Node 26.11.1 and npm 12.2.0 and makes npm refuse an engine mismatch.

## Known residuals

- **Dark theme only**; Appearance is informational.
- **Naming**: "Wyss AI" in the window and top bar, "Wyss Console" elsewhere.
- **Unsigned**, and the AppImage is built from a build host's libraries: it is built in the
  release build environment, Ubuntu 24.04, and needs a system at least that recent. The
  distribution packages are pinned by a snapshot date in the container, not by the
  verifier: a rebuild with another snapshot is a new candidate, and a `libgnutls` whose key
  blocks changed stops it until they are reviewed. The `.deb` needs the web view
  (`libwebkit2gtk-4.1`) and `systemd`; `bubblewrap` is recommended because Local Tools
  needs it. Codex is not packaged.
- **The pinned tools are GitHub release assets** and are trusted as far as the digest that
  was committed for them: it was computed from the file at the time of pinning, and says
  nothing about how that file was made. A tool that disappears from GitHub stops the
  build until it is provided another way.
- **Host tools and libraries are the distribution's**: recorded in the manifest, not
  pinned. A `.deb` is built by the host's `dpkg-deb`, an AppImage carries the host's
  libraries.
- **Not reproducible bit for bit.** Two builds of one commit in the release build environment
  (on a host and in the container) both verified, with the same inputs recorded, and differ in
  LLVM's internal symbol suffixes; each build is its own candidate.
- **The desktop differs per format**: Tauri patches the executable with the bundle type,
  so the two desktops have different digests. The authority is the same file in both.
- **An AppImage's terminals** inherit the environment its launcher sets (library paths).
- Two desktops of one user write preferences last-writer-wins.
- Neither package has been run on a real desktop; the only graphical run was the previous
  candidate's, under Xvfb, without a service manager.

## Acceptance of this decision

The architecture and code are accepted when every automated gate is green and the
artifacts verify, with the gates run on the commit in question: a gate the cloud cannot run
(the 40 Rust tests above) is green only once it has been run, for that commit, where it can.
The release is accepted when every line of `docs/release/LOCAL_ACCEPTANCE.md` is `PASS`, in
each environment it applies to, from the real artifact.
