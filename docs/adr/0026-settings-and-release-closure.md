# ADR 0026: Settings and release closure

Status: Accepted (the architecture and the code). The physical release is **not** accepted:
local acceptance is pending, see [Cloud and local](#cloud-and-local-validation).
Date: 2026-10-10

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
   `mksquashfs -root-owned`, zstd, with the bundler's own runtime), with the commit time
   as the time of every file.
2. **The menu entry** is named `wyss-console.desktop` in the `.deb`.
3. The desktop's `bundle.linux.deb.files` / `appimage.files` place the authority.

Layout, the same below every prefix:

```text
<prefix>/bin/wyss-console                        directory package: prefix = the folder
<prefix>/libexec/wyss-console/wyss-control       .deb:              prefix = /usr
                                                 AppImage:          prefix = the mounted image's usr
```

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
  menu entry and an icon;
- the AppImage unpacks without FUSE or a display;
- in both: the desktop and the authority are where the desktop looks, executable; the
  authority's digest is the manifest's pin and the desktop contains that pin; there is
  exactly one authority and one desktop;
- no forbidden name (`.env`, `auth.json`, keys, databases, `.git`, caches, fixtures, tests,
  sources, lock files, acceptance state), no credential-shaped content in any file,
  binaries included, neither program names the checkout or the build machine, nothing
  writable by group or others or set-id, no link leaving the package;
- with `--repo`: no credential file is tracked and no tracked file holds a credential-shaped
  string, historical documents included.

Findings name a file and a kind, never the bytes. A probable live credential stops the
release.

### Checksums and signing

`SHA256SUMS` (the `sha256sum` format) and `release-manifest.json`. The artifacts are
**checksummed but unsigned**: there is no release signing key or process, none was
generated, and the configuration carries no signing or updater setting (a test fails if it
does). A checksum next to the file proves a download is the one that was published only as
far as the place it was published is trusted.

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

Two limits of the build host are recorded rather than hidden. Its `bubblewrap` is 0.9.0
and the Local Tools sandbox needs 0.11 or newer (it is looked for at `/usr/bin/bwrap`, which
was not replaced), and it has no user service manager. Forty tests of the Rust suite
fail for that reason; the same forty fail on a clean `origin/main`, and the 569 that pass
include every test this phase added. The packaged desktop was started under Xvfb from
the extracted `.deb` and AppImage with an isolated home, and stayed up; no authority could
be started without a service manager. None of this stands in for the local acceptance.

The cloud did not run Codex: nothing in this phase touches runtime launching or the
provider protocol, so a paid provider run would have repeated 3G's evidence and shown
nothing new.

## Known residuals

- **Dark theme only**; Appearance is informational.
- **Naming**: "Wyss AI" in the window and top bar, "Wyss Console" elsewhere.
- **Unsigned**, and the AppImage is built from a build host's libraries: it was built on
  Ubuntu 24.04 and needs a system at least that recent. The `.deb` needs the web view
  (`libwebkit2gtk-4.1`) and `systemd`; `bubblewrap` is recommended because Local Tools
  needs it. Codex is not packaged.
- **Build-time downloads**: the AppImage bundler fetches `AppRun`, `linuxdeploy` and a
  `continuous` (unpinned) appimage plugin from GitHub when it runs. Reproducing it
  exactly depends on those; the verifier checks what came out, not where it came from.
- **The desktop differs per format**: Tauri patches the executable with the bundle type,
  so the two desktops have different digests. The authority is the same file in both.
- **An AppImage's terminals** inherit the environment its launcher sets (library paths).
- Two desktops of one user write preferences last-writer-wins.
- Graphical execution of either package was not observed.

## Acceptance of this decision

The architecture and code are accepted when every automated gate is green and the
artifacts verify. The release is accepted when every line of
`docs/release/LOCAL_ACCEPTANCE.md` is `PASS`, on a real machine, from the real artifact.
