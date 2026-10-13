# ADR 0026: Settings and product closure

Status: Accepted. The installers this decision first included (a `.deb`, an AppImage, a
release build environment and a local acceptance matrix) were withdrawn by
[ADR 0027](0027-local-product-no-distribution-packaging.md); this document describes what
stands.
Date: 2026-10-10 (revised 2026-10-12)

## Decision

Wyss Console gets a Settings view for the preferences of the desktop, and the product is
closed around it: a pass over every view, the directory package that carries the desktop
and the authority it was pinned to, and a credential scan of the repository.

```text
build    ->  launch  ->  the authority bootstraps (ADR 0025)  ->  use  ->  close, reopen, reboot
                                  |
                  Settings: a typed, bounded, local set of preferences
                  that change how the product looks and where it opens,
                  and never what it is allowed to do
```

Nothing of the core changed: not the control protocol, runtime supervision, approvals,
turn admission, authority state or isolation. The only code outside the new module that
changed is one visibility keyword (`lifecycle::descend`, which the preference store
reuses) and the way `Provision::of_build` finds the executable it starts from, so that the
layout could be tested below any prefix (the rule is the same).

The documents archived in `wyss-agent-legacy/` were used as history and nothing else. When they
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
internal name. The window and the top bar still said "Wyss AI" while the package, the menu
entry and About said "Wyss Console": which is the product's name was left to its owner, who
settled it on 2026-10-12: Wyss Console, everywhere.

## Packaging

`npm run desktop:package` builds the authority, takes its SHA-256, builds the desktop
again with that digest compiled in and assembles one directory,
`.artifacts/package/wyss-console-<version>-linux-<arch>/`:

```text
<prefix>/bin/wyss-console
<prefix>/libexec/wyss-console/wyss-control
<prefix>/share/wyss-console/package.json     version, platform, authority digest, file digests
```

It is the only package. The Tauri bundler is off (`bundle.active` is false), there is no
installer format and no menu entry is installed: the package is run from where it is, or
from wherever the whole directory is copied to
([ADR 0027](0027-local-product-no-distribution-packaging.md)).

### Provenance is unchanged

The desktop finds its authority from its own real executable (`/proc/self/exe`, links
resolved) and nowhere else: `<exe>/../../libexec/wyss-console/wyss-control`. That is the
rule ADR 0025 accepted, and it holds below any prefix, so it did not change.
The authority is read once, through a descriptor opened without following a link, from a
file of this user or root that nobody else can write, and its SHA-256 has to be the one
compiled into the desktop. The managed install is then a copy under `~/.local/share`
that runs under the user's service manager, independent of the package it came from.

Tests prove: the layout resolves below the package's own folder, a system prefix and a
nested one; a missing payload, a payload that is not the pinned one, a link in its place, a
writable payload and a writable directory are refused; the payload is found from the real
file behind a link and never from the link's neighbours; a build that cannot say where it
runs is invalid and not "development"; and the packager places the authority exactly where
that rule looks (`tests/package.test.ts` reads both).

### No signing, no updater

Nothing is signed and nothing updates itself. The configuration carries no signing or
updater setting and bundles no resource or sidecar, and a test fails if it does.

### Upgrade, removal, data

Upgrade is ADR 0025's cold-start model, unchanged and not hot-updated: an authority that
runs is never replaced and the window says `update pending`; the next time none runs, the
packaged one is installed and started and canonical state is preserved. Building or
deleting a package changes nothing in anyone's home. Deleting the package removes the
desktop; the managed authority, its unit and the canonical state remain until removed by
hand (the README says how). Nothing erases history silently.

### Version

`0.2.0`. The repository has no tag and no release, and `0.1.0` was the scaffold the work
began from. The product changed in capabilities (3A to 3H) and compatibility between 0.x
versions is not promised, so the next minor of the 0.x line is the honest number. `1.0.0`
would claim an acceptance that has not happened. No tag was created and history was not
touched.

## The credential scan of the repository

`npm run scan:repo` (`scripts/repo-scan.mjs`) checks that no credential file is tracked and
that no tracked file holds a credential-shaped string, historical documents included.

- What is scanned is what Git records (every blob of the committed tree and of the index,
  read from Git's objects, binary content included) and what is on disk for every tracked
  path: a regular file as it is there, a link as its target, never followed.
- It reads the objects Git recorded, never a replacement for them: a replace ref
  (`git replace`) does not stand in for a commit, a tree or a file.
- It fails closed. No commit, an unreadable object, a submodule, an unmerged entry, a
  tracked file missing from or of another kind in the working tree, a file too large to
  read, or more places to look at in one file than its limit: each is a failure, never a
  file that was skipped.
- Findings name a file and a kind, never the bytes.

**No exception stands on a path and a kind.** An earlier version excused one archived
planning document because it plants a made-up `sk-ant-` string in a test that checks a
secret is not indexed. That would also have excused a real key added to the same file. The
string was instead redacted (`sk-ant-<FAKE_TEST_SECRET>`, nothing else in that document
touched) and the exception removed; a test puts a credential of each of five providers in
that file and requires the scan to fail.

## Validation

The gates are the repository's own commands, run on the commit in question:

| Gate                                                                    | Needs                                                     |
| ----------------------------------------------------------------------- | --------------------------------------------------------- |
| `format:check`, `lint`, `typecheck`, `npm test` (vitest), `build`       | Node 26+, npm 12+                                         |
| `cargo fmt --check`, `cargo check --locked`, `cargo clippy -D warnings` | stable Rust, the Tauri Linux prerequisites                |
| `cargo test --locked`                                                   | `bubblewrap` 0.11 or newer at `/usr/bin/bwrap`, `systemd` |
| `npm run scan:repo`                                                     | a committed tree                                          |
| `npm run desktop:package`                                               | the same as the Rust gates                                |

The Local Tools tests run the real sandbox (`--overlay-src` needs `bubblewrap` 0.11), and the
lifecycle tests start a private `systemd --user` on a temporary runtime directory and an
empty home: a host without either cannot run them, and they are not green until they have
been run where they can.

The frontend tests do not depend on the runtime locale: the clock tests ask the locale for
its day period, digits and calendar.

## Findings of the closure

Building and running the product found these, each reproduced first, fixed minimally and
held by a test that fails without the fix:

- **Retry was offered and did nothing.** The authority lifecycle published a failed attempt,
  with Retry on, a few instructions before the attempt's thread let go of its turn. The turn
  is now released in the same step, under the view's lock, that publishes how the attempt
  ended. Two tests that raced the same window, or assumed the view is a log, were made to say
  what they mean; a terminal test that assumed `/bin/sh` is bash (dash leaves by itself when
  its terminal goes away) now uses bash.
- **Settings was drawn under every other view.** Every view stays mounted and is hidden with
  the `hidden` attribute, and each class that sets `display` needs a `[hidden]` rule that
  undoes it; Settings had none. Nothing in jsdom could show it, because jsdom does not apply
  the stylesheet. It was seen in the packaged window. A test now requires the rule for every
  view the window hides.
- **At 960×640 the reset confirmation's buttons were below the fold.** It now scrolls into view
  when it opens, and Cancel has the focus.
- **The README said** there was no authority or agent UI, that Codex was unusable from the
  window and that the desktop never starts the authority. It no longer does.
- **The credential scan failed open.** A file larger than its read limit was skipped, and a
  file with more than its limit of places to look at stopped being read, both reporting
  clean. Both are findings now. It listed tracked paths with `git ls-files` and read each one
  from the working tree, skipping any that was not a regular file there: a committed file
  holding a credential, deleted on disk, passed. It now reads what Git records from Git's
  objects, requires every tracked path on disk, scans the on-disk target of a tracked link,
  and takes no replace ref's stand-in for an object.
- **A frontend test assumed English.** The 12-hour clock test expected `AM`/`PM`; under
  `es_AR.UTF-8` the clock correctly says `p. m.`, and the test failed. The tests now ask the
  locale for its day period, digits and calendar; production did not change.
- **A development build kept naming an old commit**: `build.rs` watched `.git/HEAD`, which a
  commit on the current branch does not change. It now watches the branch's ref too, loose
  or packed, without running again on every build while the branch is only in `packed-refs`.

## Known residuals

- **Dark theme only**; Appearance is informational.
- **No installer and nothing signed**: the product is built from this repository and run
  from its directory package. Codex is not packaged.
- Two desktops of one user write preferences last-writer-wins.

## Acceptance of this decision

The architecture and code are accepted when every gate above is green on the commit in
question, each run where it can run. There is no release to accept: nothing is published
([ADR 0027](0027-local-product-no-distribution-packaging.md)).
