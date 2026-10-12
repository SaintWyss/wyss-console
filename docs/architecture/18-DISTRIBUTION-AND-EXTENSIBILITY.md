# 18 — Distribution and extensibility

Domain P of [04](04-TARGET-ARCHITECTURE.md). What the product needs in order to be used by
developers other than its owner. **None of it is authorised**: ADR 0027 stands, the product is
built from this repository and run from its directory package, and "a distribution format, if
one is ever wanted, is a new decision with its own phase. It is not to be rebuilt piece by
piece on this one" (`0027:68-70`) `[R]`. This document states requirements for that future
phase (F13) and the few rules that cost nothing to keep now.

## 1. Today `[R]`

| Fact                       | Value                                                                                                   |
| -------------------------- | ------------------------------------------------------------------------------------------------------- |
| Package                    | A directory: `bin/wyss-console`, `libexec/wyss-console/wyss-control`, `share/wyss-console/package.json` |
| Built by                   | `npm run desktop:package`                                                                               |
| Must be run from           | A place it and every folder above it can be written by the user alone (README `:21-26`)                 |
| Platform                   | Linux with `systemd --user`; Local Tools needs `bwrap` 0.11 or newer                                    |
| Signed, published, updated | No, no, no; a test holds that no signing or updater is configured                                       |
| Repository                 | Private, no licence file `[O]`                                                                          |
| Application menu entry     | None in the package; the owner's machine has one made by hand `[O]`                                     |

## 2. Rules that hold from now on

They keep the door open and are not distribution work.

| Rule                                                                                         | Requirement | Held by                                                                  |
| -------------------------------------------------------------------------------------------- | ----------- | ------------------------------------------------------------------------ |
| No path, user name, host name or provider setting of a particular person in code or fixtures | FR-DIST-01  | Review; a test that scans the tree for the known ones of the owner `[P]` |
| Every location is derived from the user's home and the XDG directories                       | FR-DIST-01  | As the authority and preferences already do `[R]`                        |
| A provider is absent until found: nothing assumes Codex or Claude Code is installed          | FR-DIST-02  | The catalogue (05 §4.1)                                                  |
| The first thing a new user sees changes nothing of his                                       | FR-DIST-02  | F1 is read only by construction                                          |
| Every capability is tied to the provider version it was verified on                          | NFR-MNT-01  | 06 §6                                                                    |
| What ties the design to Linux and systemd is written down                                    | NFR-PORT-02 | §4 below                                                                 |

## 3. Requirements for a public product (F13)

| Area                      | Requirement                                                                                                                                          | Depends on                    |
| ------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------- |
| Licence                   | A licence for Wyss Console; a check of every dependency's licence and of what is bundled                                                             | An owner decision (DN-07)     |
| Platforms                 | A stated list, each with a tested build. Nothing is "probably fine"                                                                                  | §4                            |
| Installation              | A documented, tested way to install without building; where the authority and its unit go; what is asked of the user                                 | ADR 0025's rules carried over |
| Provenance                | The desktop still installs only the authority it was built with, from a place nobody else can write                                                  | SG-11                         |
| Updates                   | How a new version arrives, how the authority is replaced only when none runs (as today), how a store is migrated, how to go back                     | ADR 0025; 14 §5               |
| Uninstallation            | What is removed and what is kept; the user's history and restoration data are his and are listed before anything goes                                | README `:97-100` `[R]`; 14 §8 |
| Onboarding                | Which agents were found, at which versions, what the Console can do with each, what it cannot; no account needed, no agent required for the terminal | 06                            |
| Safe defaults             | Read only until the user makes his first change; no telemetry by default; nothing leaves the machine                                                 | —                             |
| Configuration portability | Session profiles export and import (09 §4.5); restoration records export and import (16 §2.3)                                                        | —                             |
| Diagnostics               | A report the user can read before sending, holding versions, closed error words and counters, never content (FR-OBS-05)                              | 11 §5.5                       |
| Documentation             | For users: what each plane is and is not; for each provider, what is verified. For contributors: the adapter contract                                | 06                            |
| Accessibility             | The rules of 13 §5 verified on the supported platforms                                                                                               | —                             |
| Security maintenance      | A way to report a vulnerability; a policy for fixes; the provider versions supported                                                                 | DN-07                         |
| Support                   | What is promised and what is not                                                                                                                     | DN-07                         |

## 4. What ties the design to its platform

| Dependency                            | Used for                                                                                                                                      | Elsewhere it would need                                            |
| ------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `systemd --user`                      | Hosting the authority, restarting it, telling a crash loop from a failed start (systemd 260 or newer for the latter, ADR 0027 `:60-66`) `[R]` | Another service manager per platform                               |
| Unix domain sockets and `SO_PEERCRED` | The control channel and its same-user check `[R]`                                                                                             | A per-platform equivalent with the same guarantee                  |
| `bwrap` with unprivileged overlay     | The Local Tools namespace `[R]`                                                                                                               | The platform's own sandbox, and a new attestation                  |
| `/proc`                               | Resource observation `[P]`                                                                                                                    | Per platform                                                       |
| inotify                               | Outside-change detection `[P]`                                                                                                                | Per platform                                                       |
| GTK and WebKitGTK                     | The window and the native dialogs `[R]`                                                                                                       | Tauri's other backends; the native-dialog rule must hold there too |
| XDG base directories                  | Every location `[R]`                                                                                                                          | Per platform conventions                                           |

A second platform is therefore a project, not a build flag. The first candidates in order of
effort would be another Linux distribution with systemd (smallest), then macOS, then Windows;
no order is decided (DN-07).

## 5. Extensibility

| Extension point             | Today                                  | Target                                                                                                                                                                                                                                     |
| --------------------------- | -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| A new configuration adapter | None exists                            | Compiled in, behind `IF-CONFIG-ADAPTER` (06 §4); F4 proves the contract on two more providers                                                                                                                                              |
| A new execution adapter     | One, Codex, inside the authority `[R]` | Compiled in, with the safety contract of 06 §5                                                                                                                                                                                             |
| Adapters loaded at run time | No                                     | Not before the compiled-in contract has survived several providers and a trust model for third-party code exists. The research report found that extension supply chains are the weakest link of this category of product (its finding 10) |
| Themes, key bindings        | One dark theme `[R]`; no bindings      | Preferences, when wanted                                                                                                                                                                                                                   |
| Session profile templates   | —                                      | Files a user can share (09 §4.5)                                                                                                                                                                                                           |

A plugin system for third parties is not a goal of this set. If it becomes one, its ADR must
answer first: what code runs, with which rights, signed by whom, verified how, and updated
under whose control.

## 6. Data a public user must be able to trust

| Promise                                                           | Mechanism                                                                                                |
| ----------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| The Console reads your agents' configuration and sends it nowhere | No network use by the configuration engine; held by a test that the module links no network client `[P]` |
| It changes nothing without a plan you confirmed                   | 07 §5                                                                                                    |
| It keeps no secret of yours                                       | 11 §5, once DN-01 is decided                                                                             |
| You can remove everything it keeps                                | 14 §8                                                                                                    |
| It tells you what it could not verify                             | The evidence marks, carried into the interface (13 §1.1)                                                 |

## 7. Order, when the time comes

F13 starts only by an explicit decision and after the product is accepted by its first user:
licence and platform decisions; installation and removal on the first platform; updates;
onboarding; documentation; a security policy. It may overlap technical phases, and must not
displace them.

## 8. Teams

A team sharing one methodology (a common set of rules and hooks, kept in step) was a main
theme of the research report and is not a stated requirement of the owner. It is noted as a
later study: it would build on profiles and on export and import, and it is where a "canonical
configuration" idea, set aside for a single user by AD-03, would have to be examined again.
