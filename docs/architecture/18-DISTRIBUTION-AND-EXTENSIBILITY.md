# 18 — Distribution, installation and maintenance

Domain P of [04](04-TARGET-ARCHITECTURE.md).

**Revision of 2026-10-13 (seventh planning pass).** The first version stated requirements for a
future phase. This one is the final-product design of how Wyss Console becomes a distributable,
maintainable Linux desktop application that does not depend on its owner's machine:

- platforms (§3);
- packaging and installation (§4);
- first run (§5);
- updates and compatibility (§6);
- data, backup and removal (§7);
- supply chain and licences (§8);
- security maintenance (§9);
- extension boundaries (§10).

**Status.**

- **None of it is authorised.** ADR 0027 stands: the product is built from this repository and
  run from its directory package, and a distribution format "is a new decision with its own
  phase. It is not to be rebuilt piece by piece on this one" (`0027:68-70`) `[R]`.
- This document is that phase's design (F13), for when the owner decides it (DN-07, DN-49 to
  DN-51).
- **Nothing was built, packaged, installed or run** in this pass. Licences were read from the
  lockfiles and from published registry metadata (§8).
- Everything is `[P]` unless marked. It was written by the author of the set and is
  **unreviewed**.

## 1. Today `[R]`

| Fact                       | Value                                                                                                                                                                                                     |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Package                    | A directory: `bin/wyss-console`, `libexec/wyss-console/wyss-control`, `share/wyss-console/package.json`                                                                                                   |
| Built by                   | `npm run desktop:package` (`scripts/package.mjs`)                                                                                                                                                         |
| Must be run from           | A place it and every folder above it can be written by the user alone, or root (README `:21-26`; ADR 0025)                                                                                                |
| Authority installation     | The desktop copies the pinned `wyss-control` to `$XDG_DATA_HOME/wyss-console/authority/` (0700) and writes `$XDG_CONFIG_HOME/systemd/user/wyss-control.service` (0600), no `[Install]` section (ADR 0025) |
| Platform                   | Linux with `systemd --user`; Local Tools needs `bwrap` 0.11 or newer; crash-loop detection needs systemd 260 (ADR 0027)                                                                                   |
| Signed, published, updated | No, no, no; `tests/package.test.ts:27-43` holds that no installer, updater or signing is configured                                                                                                       |
| Version                    | 0.2.0 in `package.json`, `Cargo.toml` and `tauri.conf.json`; identifier `com.saintwyss.wyss-console`                                                                                                      |
| Repository                 | Private, no licence file `[O]`                                                                                                                                                                            |

## 2. Rules that hold from now on

They keep the door open and are not distribution work.

| Rule                                                                                         | Requirement | Held by                                                                         |
| -------------------------------------------------------------------------------------------- | ----------- | ------------------------------------------------------------------------------- |
| No path, user name, host name or provider setting of a particular person in code or fixtures | FR-DIST-01  | Review; a test that scans the tree for the known ones of the owner (TS-DIST-01) |
| Every location is derived from the user's home and the XDG directories                       | FR-DIST-01  | As the authority and preferences already do `[R]`                               |
| A provider is absent until found: nothing assumes Codex or Claude Code is installed          | FR-DIST-02  | The catalogue (05 §4.1)                                                         |
| The first thing a new user sees changes nothing of theirs                                    | FR-DIST-02  | F1 is read only by construction; §5                                             |
| Every capability is tied to the provider version it was verified on                          | NFR-MNT-01  | 06 §6                                                                           |
| What ties the design to Linux and systemd is written down                                    | NFR-PORT-02 | §3.4                                                                            |
| A dependency is added only for a need a phase has, with a written reason                     | NFR-MNT-02  | §8                                                                              |

## 3. Supported platforms

### 3.1 Tiers

| Tier                         | Meaning                                                                                                           | What it promises                                                                   |
| ---------------------------- | ----------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| **PT0 Reference**            | The platform the product is developed and accepted on                                                             | Every phase's acceptance runs here                                                 |
| **PT1 Officially supported** | A platform with a tested package, its own run of TS-DIST and of the phase acceptance criteria, and security fixes | Bugs on it are product bugs                                                        |
| **PT2 Experimental**         | Expected to work from its components; built and started in CI or by hand, without the full acceptance             | Reported problems are looked at; nothing is promised                               |
| **PT3 Unsupported**          | Known not to meet a requirement                                                                                   | The product says so at start, where it can detect it, and does not pretend to work |

**No platform is "tested" in this document.** A platform enters PT1 only with a recorded run of
TS-DIST-01 to 15 and the acceptance criteria of the phases it ships, on that platform.

### 3.2 The matrix

| Configuration                                                    | Tier proposed                                                                               | Evidence                                                                                            | Notes                                                                                                                                  |
| ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| CachyOS (Arch-based), KDE Plasma, systemd 262, x86_64            | **PT0**                                                                                     | Accepted on it (ADR 0025, 0027) `[O]`; two computers of the owner (30 §6) `[O]`                     | Session type (X11 or Wayland) is not recorded yet; P1 records it (15 §5)                                                               |
| KDE Plasma under **Wayland**                                     | PT0 if it is the owner's session, else PT1 candidate                                        | `[N]`                                                                                               | The window's translucency requests KWin's blur through a GDK property (`Cargo.toml` comment; 15 PR-06); its Wayland behaviour is `[N]` |
| KDE Plasma under **X11**                                         | PT0 if it is the owner's session, else PT1 candidate                                        | `[N]`                                                                                               | 15 PR-06 describes the X11 compositor path                                                                                             |
| Arch Linux and other Arch-based distributions with systemd ≥ 260 | PT1 candidate                                                                               | Same package family as PT0 `[N]`                                                                    | Closest to the reference                                                                                                               |
| Fedora, current release                                          | PT2, then PT1 by decision (DN-51)                                                           | `[N]`                                                                                               | Its systemd, WebKitGTK and bubblewrap versions to be read per release (OQ-58)                                                          |
| Debian or Ubuntu                                                 | PT2 only where systemd ≥ 260 is shipped, else PT3 until ADR 0027's lifecycle limit is fixed | ADR 0027: below systemd 260 a crash loop reads as a failed start `[R]`                              | OQ-58 per release                                                                                                                      |
| GNOME, other desktops with a Secret Service                      | PT2                                                                                         | `[N]`                                                                                               | Native dialogs and the keyring are the parts to check (§3.3)                                                                           |
| Desktops or sessions without a Secret Service                    | PT2 with the vault unavailable                                                              | 27 §5: the vault needs a custody (DN-25)                                                            | Everything but the vault works; the vault says why it cannot be used                                                                   |
| No `systemd --user` (other init systems, some containers)        | **PT3** for the managed plane                                                               | ADR 0025: "the desktop says so and starts nothing" `[R]`                                            | The terminal works; the managed plane and the host service (16 §4) do not                                                              |
| `bwrap` older than 0.11 or without unprivileged user namespaces  | PT3 for Local Tools                                                                         | README `[R]`                                                                                        | Inference and everything else work                                                                                                     |
| Root user, shared accounts, homes on network filesystems         | PT3                                                                                         | Path rules refuse writable-by-others directories (ADR 0011, 0025) `[R]`; 28 for network filesystems | Said at start                                                                                                                          |
| WSL, containers without a graphical session                      | PT3                                                                                         | —                                                                                                   | —                                                                                                                                      |
| aarch64 Linux                                                    | PT2 candidate                                                                               | `[N]`                                                                                               | Nothing in the code is architecture-specific that is known; nothing built                                                              |
| macOS, Windows                                                   | **Not a tier.** A separate portability project each (§3.4; DN-07)                           | Other platforms compile the lifecycle as a stub that reports failure (ADR 0025) `[R]`               | Not claimed, not planned                                                                                                               |

### 3.3 Runtime components a supported platform must provide

| Component                                         | Required for                                    | Minimum                                                                                           | If absent                                                               |
| ------------------------------------------------- | ----------------------------------------------- | ------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| GTK 3 and WebKitGTK (4.1 API)                     | The window                                      | The versions Tauri 2.12 and `webkit2gtk` 2.0 link against `[R]` (`Cargo.lock`); exact floor `[N]` | No window: the package declares them as dependencies                    |
| `systemd --user`                                  | The authority (ADR 0025), the host service (16) | 260 for crash-loop detection (ADR 0027)                                                           | PT3 row above                                                           |
| `bubblewrap`                                      | Local Tools                                     | 0.11 with unprivileged overlay                                                                    | Local Tools unavailable, said so                                        |
| A Secret Service implementation                   | The vault's key (DN-25)                         | KWallet 6.30 and GNOME Keyring 51.1 observed `[O]` (30 §6)                                        | Vault unavailable; or a passphrase custody if DN-25 allows              |
| An XDG portal or GTK file chooser                 | Native dialogs (11 §4)                          | `[N]` per desktop (OQ-43)                                                                         | Operations that need a native dialog are refused (`dialog_unavailable`) |
| inotify, `/proc`, Unix sockets with `SO_PEERCRED` | The watcher, observation, every local protocol  | Linux kernel                                                                                      | Not Linux: not a platform                                               |

### 3.4 What ties the design to its platform

| Dependency                            | Used for                                                                                                                                              | Elsewhere it would need                                            |
| ------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `systemd --user`                      | Hosting the authority and the host service, restarting them, telling a crash loop from a failed start (systemd 260 or newer, ADR 0027 `:60-66`) `[R]` | Another service manager per platform                               |
| Unix domain sockets and `SO_PEERCRED` | Every local protocol and its same-user check `[R]`                                                                                                    | A per-platform equivalent with the same guarantee                  |
| `bwrap` with unprivileged overlay     | The Local Tools namespace `[R]`                                                                                                                       | The platform's own sandbox, and a new attestation                  |
| `/proc`                               | Resource observation and the foreground program `[P]`                                                                                                 | Per platform                                                       |
| inotify                               | Outside-change detection `[P]`                                                                                                                        | Per platform                                                       |
| GTK and WebKitGTK                     | The window and the native dialogs `[R]`                                                                                                               | Tauri's other backends; the native-dialog rule must hold there too |
| A Secret Service                      | The vault's key (27 §5)                                                                                                                               | The platform's keychain, and a new custody design                  |
| XDG base directories                  | Every location `[R]`                                                                                                                                  | Per-platform conventions                                           |

A second operating system is therefore a project, not a build flag.

## 4. Packaging and installation

### 4.1 What any format must preserve

A format that cannot hold these is not used. **No packaging decision may weaken them silently:**

| #   | Requirement                                                                                                             | Source            |
| --- | ----------------------------------------------------------------------------------------------------------------------- | ----------------- |
| PK1 | The terminal runs the user's real shell and agents on the host, in real PTYs                                            | 05 §1; 16 §4.1    |
| PK2 | Native agents are the user's installed executables, found as the user would find them                                   | 05 §17            |
| PK3 | The user's real provider configuration is read where the provider reads it                                              | 07                |
| PK4 | The authority and the host service are the user's own `systemd --user` units, started on demand, never enabled at login | ADR 0025; 16 §4.1 |
| PK5 | Sockets in `$XDG_RUNTIME_DIR/wyss-console`, 0600 in 0700, same-UID checked                                              | ADR 0011          |
| PK6 | The desktop installs or uses only the authority it was built with, from a place nobody else can write                   | SG-11; ADR 0025   |
| PK7 | Native dialogs opened by the backend, in the user's desktop session                                                     | 11 §4             |
| PK8 | The vault's key in the user's Secret Service collection, under DN-25's rules                                            | 27 §5             |
| PK9 | No network listener unless the remote gateway (16 §5) is explicitly enabled                                             | 16 §5.2.2         |

### 4.2 Formats

| Format                               | PK1–PK3 (PTY, agents, configuration)                                                                                                                                                        | PK4–PK5 (units, sockets)                                                                                     | PK6 (provenance)                                                                              | PK7–PK8 (dialogs, keyring)                                                                       | Updates and signing                                                       | Verdict                                                                                          |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| PF1 Directory package (today)        | Held `[R]`                                                                                                                                                                                  | Held: the desktop writes the user unit (ADR 0025) `[R]`                                                      | Held: pinned digest, path rules `[R]`                                                         | Held                                                                                             | None; the user rebuilds                                                   | Stays the development and owner's form                                                           |
| **PF2 Native Arch package** (pacman) | Held: unsandboxed                                                                                                                                                                           | The user unit can ship as a vendor unit in `/usr/lib/systemd/user/`, not enabled; root-owned binaries (§4.3) | Held: root-owned files in root-owned directories satisfy the path rule as ADR 0025 allows     | Held                                                                                             | The package manager's signed repositories; the owner's own repository key | **Recommended first**, for PT0 and PT1 Arch-based                                                |
| PF3 `.deb` / `.rpm`                  | Held                                                                                                                                                                                        | As PF2                                                                                                       | As PF2                                                                                        | Held                                                                                             | Distribution repositories or the owner's, signed                          | Per PT1 platform decided (DN-51)                                                                 |
| PF4 AppImage                         | Held: unsandboxed                                                                                                                                                                           | Not by itself: the desktop's installer (ADR 0025) would copy the authority out of a mounted image            | The image mount is root-owned only through FUSE's view; the pinned digest still applies `[N]` | Held                                                                                             | Optional delta updates; signatures not checked by default                 | **Experimental** (PT2): carries its own WebKitGTK, which ADR 0027 removed for cost               |
| PF5 Flatpak                          | **Fails.** Inside the sandbox the PTY runs the sandbox's shell, not the host's; host agents and configuration need `flatpak-spawn --host` and `--filesystem=home`, which remove the sandbox | User units on the host and sockets in the host's runtime directory need holes in the sandbox                 | The authority would run outside the sandbox it was shipped in                                 | Portals give dialogs; the Secret portal gives an application secret, not DN-25's collection item | Signed, store-delivered                                                   | **Rejected.** A sandboxed format with every hole open would claim a confinement it does not have |
| PF6 Snap (classic or strict)         | Strict: as Flatpak. Classic: unsandboxed                                                                                                                                                    | Classic only                                                                                                 | Classic only                                                                                  | Classic only                                                                                     | Store-delivered, automatic by default                                     | **Rejected**: automatic updates conflict with §6.3; classic adds nothing over PF2                |
| PF7 Nix                              | Held                                                                                                                                                                                        | Held with a Home Manager module                                                                              | Store paths are root-owned and immutable                                                      | Held                                                                                             | The user's channel                                                        | PT2 if a user contributes it                                                                     |

### 4.3 Installation layout for system packages (PF2, PF3)

| File                                                                          | Owner, mode | Note                                                                                                                         |
| ----------------------------------------------------------------------------- | ----------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `/usr/bin/wyss-console`                                                       | root, 0755  | The desktop                                                                                                                  |
| `/usr/lib/wyss-console/wyss-control`                                          | root, 0755  | The authority                                                                                                                |
| `/usr/lib/wyss-console/wyss-host`                                             | root, 0755  | The host service, from F9 (16 §4)                                                                                            |
| `/usr/lib/wyss-console/wyss-gateway`                                          | root, 0755  | Only if F10a ships; never started unless enabled by the user (16 §5)                                                         |
| `/usr/lib/systemd/user/wyss-control.service`, `wyss-host.service`             | root, 0644  | Vendor user units with ADR 0025's content, `ExecStart` the paths above, **no `[Install]`**: never enabled, started on demand |
| `/usr/share/applications/com.saintwyss.wyss-console.desktop`, icons, metainfo | root, 0644  | The application-menu entry ADR 0027 removed                                                                                  |
| `/usr/share/licenses/wyss-console/`                                           | root, 0644  | The product's licence (DN-07) and the third-party notices of §8                                                              |

**This changes ADR 0025's installation layout.** With a system package, the desktop does not copy
the authority into `$XDG_DATA_HOME` or write a unit into `$XDG_CONFIG_HOME`. It uses the vendor
unit and the root-owned binary, still checking that binary's digest against the one it was built
with.

- A build flag selects the layout: user-installed (PF1) or system-installed (PF2, PF3).
- A user unit of the same name in `$XDG_CONFIG_HOME` left by an earlier PF1 installation would
  override the vendor unit. The desktop detects it by ADR 0025's marker line and offers to remove
  it, never silently.
- This needs its own ADR (§12).

**Installation workflow** (system packages):

- **What the package manager does:** installs the files, declares dependencies (GTK 3, WebKitGTK
  4.1, `bubblewrap` ≥ 0.11, systemd ≥ 260) and runs **no scripts that touch user homes, units or
  services**.
- **Ownership:** binaries root's, data the user's.
- **No service is registered for any user at install time.**
- **First run is detected by the absence** of `$XDG_CONFIG_HOME/wyss-console/preferences.json` and
  of `$XDG_STATE_HOME/wyss-console` (§5).
- **Removal** removes the files above and nothing in any home (§7.2).
- **A running authority or host keeps the deleted inode** until it stops. Its vendor unit is gone,
  so the user manager needs `daemon-reload`, which the desktop never asks for. The next desktop of
  another version, if any, reports `update pending` (§6.5).

## 5. First run and onboarding

A first launch creates no privileged service, enrols no device, starts no agent and changes no
agent's file.

| #    | Step                          | Design                                                                                                                                                                                                                                                                                   |
| ---- | ----------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| OB1  | Onboarding page (13 S18)      | One page: the terminal offered first; which agents were found; what the Console can do with each on its version and what it cannot; "Nothing of yours has been changed." No account, no agent required                                                                                   |
| OB2  | Provider discovery            | The catalogue (05 §4.1) and each adapter's `detect()` (06 §4). States per provider: found and verified; found, version without evidence ("read only"); not found ("Not installed. Wyss Console does not install agents."); found but unreadable home (`source_unreadable`)               |
| OB3  | Directories                   | Created on first need, 0700: `$XDG_CONFIG_HOME/wyss-console` (preferences), `$XDG_STATE_HOME/wyss-console` (authority store, journals, registries, profiles, layout), `$XDG_RUNTIME_DIR/wyss-console` (sockets). Placement of the vault and the transaction directory: DN-31, unresolved |
| OB4  | Permissions asked             | None at first run. The vault's consent (27 §7), device pairing (16 §5.2), configuration changes (07 §5): each asked when the user first uses it                                                                                                                                          |
| OB5  | Keyring                       | Detected, not used: the Secret Service's presence and lock state shown in About (S19). Locked or absent: the vault says it is unavailable and why; nothing else is affected                                                                                                              |
| OB6  | Provider configurations       | Read only. No file of any provider is created, changed or "repaired" at first run or after an update                                                                                                                                                                                     |
| OB7  | Authority                     | Started on demand when the managed plane is first used (ADR 0025), not at first run. A user who never uses it never starts it                                                                                                                                                            |
| OB8  | Host service (F9)             | Off until the user chooses "Keep terminals running when the window closes", with what that means (16 §4.1)                                                                                                                                                                               |
| OB9  | Interrupted installation      | Package: the package manager's own transaction. Directory package: ADR 0025's installer lock and temporary-name clean-up `[R]`. A state directory without a completed first-run marker is treated as a first run again; existing stores are never reset                                  |
| OB10 | Diagnostics and accessibility | Every failure a closed word with its sentence (13 §9); "Create a diagnostic report" (S19; FR-OBS-05) shows the report before saving it; the onboarding page meets 13 §5                                                                                                                  |

## 6. Updates and version compatibility

### 6.1 Versions

| Versioned thing                                                      | Scheme                                                                    | Compatibility rule                                                                                                               |
| -------------------------------------------------------------------- | ------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| The product                                                          | Semantic versioning `MAJOR.MINOR.PATCH`; one version for all its binaries | A patch changes no protocol and no schema                                                                                        |
| Control protocol (authority)                                         | An integer, today 1 `[R]`                                                 | Both sides refuse a mismatch `[R]` (ADR 0011, 0025)                                                                              |
| IF-HOST (16 §4.4)                                                    | An integer                                                                | The host accepts its own version and the previous one for one minor release                                                      |
| Gateway protocol (16 §5)                                             | An integer, negotiated                                                    | The gateway supports the current and the previous version, so a phone app that has not updated keeps working for one release     |
| Authority store                                                      | Schema version (7 today) and event vocabulary (1) `[R]`                   | Forward-only migrations; an older build refuses a newer store `[R]` (14 §5; ADR 0017)                                            |
| Other stores (journals, registries, profiles, layout, vault records) | A `schema` field in each file or record                                   | Read the current and earlier versions; never write an older one; a newer file is left untouched (`layout_newer_version`, 05 §15) |
| Provider evidence                                                    | The manifest of 06 §6.2, by provider version                              | An unknown provider version makes mutations `unverified` (06 §6)                                                                 |
| Phone client                                                         | Its own version, reporting the gateway protocol versions it speaks        | As the gateway protocol                                                                                                          |

### 6.2 Compatibility between components

| Pair                                     | Rule                                                                                                                                     | When they differ                                                                         |
| ---------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| CP-01 desktop ↔ authority                | Same control protocol; the desktop installs or uses only the authority it was built with                                                 | `update pending` until no authority runs; the old one keeps serving `[R]`                |
| CP-02 desktop ↔ host                     | Same IF-HOST version or the previous one                                                                                                 | The host is replaced only when it has no terminal, or after the user confirms (16 §4.11) |
| CP-03 two desktops of different versions | Each speaks its own protocol to the running authority; the one that does not match it says `update pending` and offers only the terminal | Never two authorities                                                                    |
| CP-04 gateway ↔ authority, host          | The gateway is part of the product's version and is replaced with it                                                                     | Remote access is paused while they differ                                                |
| CP-05 gateway ↔ phone                    | Current and previous gateway protocol                                                                                                    | Older: "Update the app to keep using this computer"                                      |
| CP-06 adapters ↔ providers               | Per provider version (06 §6)                                                                                                             | Read only, said so                                                                       |
| CP-07 binaries ↔ stores                  | Forward-only migrations, each in a transaction, with a pre-migration copy (§6.4)                                                         | An older binary refuses a newer store, and says so                                       |
| CP-08 vault ↔ binaries                   | Record formats are read by every later version; a record is never rewritten to migrate it                                                | A record in a newer format is refused and kept, never deleted                            |

### 6.3 Discovery, verification and channels

| Element           | Design                                                                                                                                                                                                                                                                                                                 |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Discovery         | Through the package manager. **No in-app updater and no update check over the network by default** (no telemetry, 18 §11). An optional check that only reads a signed release list is DN-50                                                                                                                            |
| Verification      | The package manager's signature over the repository or package. Releases also carry `SHA256SUMS` signed by the release key, and a build provenance record (source commit, builder, toolchain versions, dependency lockfiles' digests). A signature proves **who** built it, **not that it is free of vulnerabilities** |
| Reproducibility   | Not claimed. Pursued as a later goal; until then, provenance records say how each artifact was built                                                                                                                                                                                                                   |
| Channels          | `stable` and `testing`. Security fixes go to both. No automatic channel switch                                                                                                                                                                                                                                         |
| Evidence manifest | 06 §6.5 R-b, a signed data-only manifest, may invalidate a provider-version capability between releases; it can never add one                                                                                                                                                                                          |
| User control      | The user decides when to update; the product never updates itself; no update is applied while the managed plane has a running turn or the host has terminals, without the user's confirmation                                                                                                                          |

### 6.4 Migrations, rollback and recovery

**SM-UPDATE**, per computer:

| From      | To        | Trigger                                               | Guard                                 | Side effect                                                                                                                                             | On failure                                                                       |
| --------- | --------- | ----------------------------------------------------- | ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| current   | installed | The package manager replaced the files                | —                                     | Old processes keep their files                                                                                                                          | The package manager's transaction                                                |
| installed | pending   | A new desktop meets an old authority or host          | Running processes                     | `update pending` shown                                                                                                                                  | —                                                                                |
| pending   | migrating | No old process runs; the new authority or host starts | Free space for the pre-migration copy | A copy of each store about to migrate, through SQLite's backup API for databases, into `$XDG_STATE_HOME/wyss-console/pre-migration/<from>-<to>/` (0700) | Not enough space: nothing migrates; the new binary refuses to start and says why |
| migrating | migrated  | Each migration committed in its transaction           | —                                     | Schema version raised; a migration record in the store's own audit                                                                                      | → failed                                                                         |
| migrating | failed    | A migration error                                     | —                                     | The transaction rolled back; the store unchanged; the new binary refuses to start (`migration_failed`)                                                  | The previous version still opens the unchanged store                             |
| migrated  | verified  | The new version's start-up checks pass                | —                                     | Pre-migration copies kept for the bound of §7                                                                                                           | —                                                                                |

**Rollback of binaries is not rollback of data.**

- **Before a migration commits,** installing the previous version works with the unchanged store.
- **After a migration commits,** an older binary refuses the newer store `[R]`. Going back means
  restoring the pre-migration copy. That **discards every event, decision and audit record made
  since**, so it is never automatic. It is an explicit action, with a confirmation that lists what
  will be lost.
- **When a copy is restored, the authority records it.** The first event after the restore is a
  restore record carrying the version, the time and the copy's identity. The audit then shows the
  gap, never hides it.
- **Vault records are never migrated in place (CP-08)**, so they need no restore. A vault record
  made by the newer version stays unreadable to the older one, and is kept.
- **Recovery after a failed update:**
  - the package manager's own recovery for the files;
  - this table for the stores;
  - the diagnostic report (FR-OBS-05) for what happened.

## 7. Data ownership, backup, uninstall and deletion

### 7.1 What exists, and what removal does to it

| #     | Data                                    | Location                                                          | Sensitivity                | Removed by uninstall                                        | Removed by "Delete history" | Removed by "Delete kept entries" | Removed by "Delete all Wyss Console data"                               | Backup                                                             |
| ----- | --------------------------------------- | ----------------------------------------------------------------- | -------------------------- | ----------------------------------------------------------- | --------------------------- | -------------------------------- | ----------------------------------------------------------------------- | ------------------------------------------------------------------ |
| DR-01 | Preferences                             | `$XDG_CONFIG_HOME/wyss-console/preferences.json` `[R]`            | Low                        | No                                                          | No                          | No                               | Yes                                                                     | The user's own                                                     |
| DR-02 | Layout                                  | `$XDG_STATE_HOME/wyss-console/layout.json` (05 §15)               | Folder paths               | No                                                          | No                          | No                               | Yes                                                                     | Not needed                                                         |
| DR-03 | Profiles and their versions             | State directory (09 §4)                                           | References, no secrets     | No                                                          | No                          | No                               | Yes                                                                     | Export (09 §4.5)                                                   |
| DR-04 | Managed authority state and audit       | `$XDG_STATE_HOME/wyss-console/control.sqlite3` `[R]`              | Identifiers, decisions     | No                                                          | No                          | No                               | Yes, after stopping the authority                                       | Pre-migration copies (§6.4)                                        |
| DR-05 | Host session registry                   | State directory (16 §4.10)                                        | Session records            | No                                                          | No                          | No                               | Yes                                                                     | Not needed                                                         |
| DR-06 | Device pairing records and host key     | Gateway state; host key in the keyring or a 0600 file (16 §5.2.1) | High: grants access        | No                                                          | No                          | No                               | Yes, **after revoking every device**                                    | Never backed up: a restored key would re-admit revoked devices     |
| DR-07 | Configuration and gateway journals      | State directory (14 §3; 16 §5.2.6)                                | Identifiers, keyed digests | No                                                          | Yes (14 §8)                 | No                               | Yes                                                                     | Not needed                                                         |
| DR-08 | Vault records, index, consent           | Vault directory (27; placement DN-31)                             | **High: may hold secrets** | **No**                                                      | No                          | Yes (one or all)                 | **Only after "Delete kept entries"** has run, with its own confirmation | 27 §9.3 export, deferred (DN-29)                                   |
| DR-09 | Vault key and recovery metadata         | Secret Service item (DN-25); a recovery code if DN-26 allows      | **High**                   | **No**                                                      | No                          | Yes, when all records go (14 §8) | As DR-08                                                                | The recovery code, if any, is the user's to keep                   |
| DR-10 | Pre-migration copies                    | `$XDG_STATE_HOME/wyss-console/pre-migration/`                     | As DR-04                   | No                                                          | No                          | No                               | Yes                                                                     | Kept for two successful start-ups of the new version, then removed |
| DR-11 | Installed authority and unit (PF1 only) | ADR 0025's two files `[R]`                                        | —                          | Yes, by the user's "Remove" in Settings or by hand (README) | —                           | —                                | Yes                                                                     | —                                                                  |

### 7.2 Four separate operations

1. **Uninstall the software:** the package manager, or deleting the directory package. **Nothing
   of the user's is removed.** The vault and its key stay, and a later installation finds them
   (14 §8; README `:97-100`) `[R]`.
2. **Remove the services (PF1):** stop the authority, then delete ADR 0025's two files. Offered in
   Settings from F13, with a list of what it does.
3. **Delete history** and **Delete kept entries:** as 14 §8, each its own action and confirmation.
4. **Delete all Wyss Console data:**
   - It lists every row of §7.1 with its size.
   - It is **refused while the vault holds a record**: the user must first delete the kept entries
     with that action's own confirmation, which says that each is an entry that can no longer be
     restored. The only recoverable copy of a disabled entry is never destroyed by a broader action.
   - It is refused while a device is paired, until the user revokes every device.
   - It refuses while an authority, host or gateway runs.

**What deletion cannot reach** is said in every dialog that deletes sensitive data (27 §10; T-36):

- filesystem snapshots;
- backups;
- the replication tool's versions;
- freed blocks of an SSD;
- other computers.

Deleting ciphertext does not remove it from a snapshot. The only stronger measure is destroying
the key, and that cannot reach a backup of the keyring.

## 8. Supply chain and licences

**Licence facts below** were read on 2026-10-13:

- from `package-lock.json` (lockfile version 3, which records each package's declared licence);
- from crates.io's published metadata for each crate and version pinned in `src-tauri/Cargo.lock`.

They are **declared** licences, not a legal review. No dependency was installed, upgraded or scanned
for vulnerabilities. **The licence of Wyss Console itself is DN-07**, the owner's.

### 8.1 Direct dependencies

**Rust** (`src-tauri/Cargo.toml`; versions resolved in `Cargo.lock`; licence as published on crates.io for that version):

| Crate          | Version | Declared licence  |
| -------------- | ------- | ----------------- |
| `tauri`        | 2.12.1  | Apache-2.0 OR MIT |
| `tauri-build`  | 2.7.1   | Apache-2.0 OR MIT |
| `gtk`          | 0.18.2  | MIT               |
| `sha2`         | 0.10.9  | MIT OR Apache-2.0 |
| `serde`        | 1.0.229 | MIT OR Apache-2.0 |
| `serde_json`   | 1.0.151 | MIT OR Apache-2.0 |
| `rusqlite`     | 0.40.2  | MIT               |
| `uuid`         | 1.27.0  | Apache-2.0 OR MIT |
| `time`         | 0.3.55  | MIT OR Apache-2.0 |
| `portable-pty` | 0.9.0   | MIT               |
| `nix`          | 0.28.0  | MIT               |

**npm** (`package.json`; versions and declared licences from `package-lock.json`):

| Package                        | Version | Kind        | Declared licence  |
| ------------------------------ | ------- | ----------- | ----------------- |
| `@tauri-apps/api`              | 2.12.1  | runtime     | Apache-2.0 OR MIT |
| `@xterm/addon-fit`             | 0.11.0  | runtime     | MIT               |
| `@xterm/xterm`                 | 6.0.0   | runtime     | MIT               |
| `svelte`                       | 5.57.1  | runtime     | MIT               |
| `@eslint/js`                   | 10.0.1  | development | MIT               |
| `@sveltejs/vite-plugin-svelte` | 7.3.1   | development | MIT               |
| `@tauri-apps/cli`              | 2.12.1  | development | Apache-2.0 OR MIT |
| `@types/node`                  | 26.6.4  | development | MIT               |
| `eslint`                       | 10.12.0 | development | MIT               |
| `eslint-plugin-svelte`         | 3.23.0  | development | MIT               |
| `globals`                      | 17.13.0 | development | MIT               |
| `jsdom`                        | 30.1.2  | development | MIT               |
| `prettier`                     | 3.9.9   | development | MIT               |
| `prettier-plugin-svelte`       | 4.1.1   | development | MIT               |
| `svelte-check`                 | 4.7.6   | development | MIT               |
| `typescript`                   | 6.0.3   | development | Apache-2.0        |
| `typescript-eslint`            | 8.71.1  | development | MIT               |
| `vite`                         | 8.3.2   | development | MIT               |
| `vitest`                       | 5.0.3   | development | MIT               |

### 8.2 What is shipped and what is not

| Part                          | Shipped in a package                                                                                   | Licence facts                                                                                                         |
| ----------------------------- | ------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------- |
| The two binaries              | Yes: Rust code statically linked, including SQLite compiled in by `rusqlite`'s `bundled` feature `[R]` | Every crate in `Cargo.lock` (§8.3)                                                                                    |
| The front end (`dist/`)       | Yes: what Vite bundles from the four runtime npm packages and their dependencies                       | The non-development entries of `package-lock.json` (§8.3)                                                             |
| Development tools             | No: TypeScript, ESLint, Vite, Vitest, Prettier, `lightningcss` (MPL-2.0) and others                    | Recorded, not shipped                                                                                                 |
| GTK, WebKitGTK, glib, libsoup | **No**: system libraries linked dynamically, provided by the distribution (PF2, PF3)                   | LGPL, as the distributions declare; an AppImage (PF4) would ship them and carry their obligations                     |
| Icons                         | Yes                                                                                                    | Made in this repository (`src-tauri/icons/icon.svg`, commit `1d6ab45`) `[R]`; under the product's own licence (DN-07) |
| Fonts                         | No: the system's (`system-ui`, `ui-monospace`) `[R]` (`src/app.css:25-28`)                             | —                                                                                                                     |
| `wyss-agent-legacy/`          | **No**: archived documents of the owner's other product                                                | The owner's own; excluded from any package                                                                            |

### 8.3 Transitive dependencies

**Rust, all 432 crates of `Cargo.lock`** (1 of them this workspace): declared licence expressions, most frequent first:

| Expression                                            | Crates |
| ----------------------------------------------------- | ------ |
| `MIT OR Apache-2.0`                                   | 199    |
| `MIT`                                                 | 105    |
| `Apache-2.0 OR MIT`                                   | 29     |
| `Unicode-3.0`                                         | 18     |
| `MIT/Apache-2.0`                                      | 17     |
| `Zlib OR Apache-2.0 OR MIT`                           | 17     |
| `Unlicense OR MIT`                                    | 9      |
| `MPL-2.0`                                             | 5      |
| `Apache-2.0/MIT`                                      | 4      |
| `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | 3      |
| `BSD-3-Clause`                                        | 2      |
| `Zlib`                                                | 2      |
| `MIT OR Zlib OR Apache-2.0`                           | 2      |
| `BSD-3-Clause OR MIT OR Apache-2.0`                   | 2      |
| `MIT OR Apache-2.0 OR LGPL-2.1-or-later`              | 2      |
| `Unlicense/MIT`                                       | 2      |
| `Apache-2.0`                                          | 2      |
| `0BSD OR MIT OR Apache-2.0`                           | 1      |
| `BSD-3-Clause AND MIT`                                | 1      |
| `BSD-3-Clause/MIT`                                    | 1      |
| `Apache-2.0 AND MIT`                                  | 1      |
| `CC0-1.0 OR MIT-0 OR Apache-2.0`                      | 1      |
| `Apache-2.0 / MIT`                                    | 1      |
| `ISC`                                                 | 1      |
| `MIT OR Apache-2.0 OR Zlib`                           | 1      |
| `BSD-2-Clause OR Apache-2.0`                          | 1      |
| `Apache-2.0 WITH LLVM-exception`                      | 1      |
| `(MIT OR Apache-2.0) AND Unicode-3.0`                 | 1      |

Expressions outside the permissive set (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode, 0BSD, CC0, Unlicense, BSL-1.0), each to be read in its files (OQ-60):

| Crate                    | Expression                               |
| ------------------------ | ---------------------------------------- |
| `cssparser-macros@0.7.1` | `MPL-2.0`                                |
| `cssparser@0.37.0`       | `MPL-2.0`                                |
| `dtoa-short@0.3.5`       | `MPL-2.0`                                |
| `option-ext@0.2.0`       | `MPL-2.0`                                |
| `r-efi@5.3.0`            | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` |
| `r-efi@6.0.0`            | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` |
| `selectors@0.38.0`       | `MPL-2.0`                                |

Crates with no declared licence in the registry metadata: none.

**npm, all 244 packages of `package-lock.json`**: MIT 174, Apache-2.0 19, Apache-2.0 OR MIT 13, MPL-2.0 12, BSD-2-Clause 10, ISC 8, BSD-3-Clause 3, MIT-0 2, BlueOak-1.0.0 2, CC0-1.0 1. Of these, 23 are not development-only, under MIT 20, Apache-2.0 2, Apache-2.0 OR MIT 1. MPL-2.0 (`lightningcss` and its platform builds), BlueOak-1.0.0, CC0-1.0 and MIT-0 appear only in development dependencies.

What these facts imply, without a legal conclusion:

- **MPL-2.0 crates.** The five (`cssparser`, `cssparser-macros`, `selectors`, `dtoa-short`,
  `option-ext`) are in the lockfile. Whether any of them is compiled into the shipped binaries, or
  used only at build time, is to be established with `cargo tree` (not run in this pass; OQ-60).
  MPL-2.0 is a file-level copyleft licence: if they ship, their notices and the availability of
  their source are obligations to check.
- **`r-efi` offers MIT** among its alternatives.
- **The SQLite compiled in through `libsqlite3-sys`** (crate licence `MIT`) is itself dedicated to
  the public domain by its authors; that statement is to be read and recorded (OQ-60).
- **Several expressions use the older `/` separator** instead of SPDX's `OR`. They are recorded as
  declared.
- **A legal reading of these facts is not part of this design.** DN-07's licence choice and any
  redistribution need one.

### 8.4 Trust risks of building, installing and updating

| Risk                                                                                                        | Control                                                                                                                                                                                                                |
| ----------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A dependency release is compromised                                                                         | Exact versions locked (`Cargo.lock`, `package-lock.json`, `=` pins for key crates) `[R]`; builds with `--locked` and `npm ci` `[R]`; an update of a dependency is a reviewed change with a written reason (NFR-MNT-02) |
| A build machine is compromised                                                                              | Builds from a clean, recorded environment; the provenance record of §6.3; a second build compared where reproducibility allows                                                                                         |
| The release key leaks                                                                                       | §9.3                                                                                                                                                                                                                   |
| An installer script runs as root on users' machines                                                         | No install scripts touch homes, units or services (§4.3)                                                                                                                                                               |
| An updater fetches and runs code                                                                            | No in-app updater (§6.3)                                                                                                                                                                                               |
| A package carries files not from the repository                                                             | The package's file list is fixed and tested (TS-DIST-12)                                                                                                                                                               |
| The tests that hold "no updater, no signing" (`tests/package.test.ts`) are removed when a format is adopted | Replaced by tests of the adopted format's actual signing and update path (TS-DIST-09, 12)                                                                                                                              |

## 9. Security maintenance and incident response

### 9.1 Roles

The product starts with one maintainer, so every role below is the owner's until delegated. The
roles are kept apart so that each can be handed to someone else.

| Role               | Responsible for                                                                 |
| ------------------ | ------------------------------------------------------------------------------- |
| Security contact   | Receiving reports; acknowledging within the published time; the private channel |
| Release manager    | Building, signing and publishing releases; the release key                      |
| Maintainer on call | Triaging, fixing, deciding an emergency release                                 |
| Evidence keeper    | The provider evidence manifest (06 §6) and its invalidations                    |

### 9.2 Policy

| Element                | Design                                                                                                                                                                                                                     |
| ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Reporting              | A `SECURITY.md` naming a private channel (FR-DIST-06); no report needs an account on a third-party service                                                                                                                 |
| Disclosure             | Coordinated: acknowledgement within 7 days, a fix or a mitigation published as soon as it exists, public disclosure with the fix or at 90 days, whichever is first, unless the reporter and the maintainer agree otherwise |
| Advisories             | Each fixed vulnerability gets an advisory: affected versions, fixed version, severity, what a user must do                                                                                                                 |
| Supported versions     | The latest minor release of `stable` gets security fixes; the previous minor for 3 months after a new one                                                                                                                  |
| Provider compatibility | When a provider version is found to break a verified capability, its evidence is invalidated (06 §6.4; R-b) and an advisory says what is now read only                                                                     |

### 9.3 Incidents

| Incident                                  | Response                                                                                                                                                                                                                                         |
| ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| IN1 A vulnerability is reported           | Triage; a private fix; an emergency release on both channels; the advisory                                                                                                                                                                       |
| IN2 An unsafe release is published        | Withdraw it from the repositories; publish a fix as a new version. **Never** re-publish an older version under the new number: its stores may already be migrated (§6.4). Users who must go back get the restore procedure, with its cost stated |
| IN3 The release key is compromised        | Revoke it in the repositories and in `SECURITY.md`; a new key, announced through a channel independent of the compromised one; re-sign current releases; list what was signed with the old key since the compromise may have started             |
| IN4 A distributed artifact is compromised | Withdraw it; publish digests of the good artifacts; an advisory telling users to compare and reinstall; the provenance records show which builds are affected                                                                                    |
| IN5 A provider update breaks a capability | Invalidate its evidence (R-b) without a release; users see the capability read only until a release adds evidence                                                                                                                                |
| IN6 A dependency vulnerability            | Assess whether the vulnerable code is reachable; fix or mitigate; an advisory either way                                                                                                                                                         |

**Every incident leaves a record:**

- what happened;
- which versions;
- what was done;
- when.

It is kept with the release history and is not part of any user's data.

## 10. Extension boundaries

| Class                                                    | Examples                                                                                                                         | What runs                                | Trust                                                                                                                                                                                    | Upgrade compatibility                        |
| -------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------- |
| EC1 Data only                                            | Session profile templates (09 §4.5); themes and key maps (preferences); the signed evidence manifest (06 §6.5)                   | Nothing. Parsed with bounds and a schema | An imported file grants nothing (CTL-19); the manifest cannot add a capability                                                                                                           | A `schema` field; newer files left untouched |
| EC2 Declarative, compiled in                             | Provider adapters (06 §4); start recipes (05 §17)                                                                                | Our code, in our binaries                | Reviewed like the rest of the product                                                                                                                                                    | Evidence per provider version (06 §6)        |
| EC3 Subprocess, the provider's                           | Agents themselves; their MCP servers, hooks and skills, which the Console **administers in the provider's files** but never runs | The provider runs them, as the user      | The provider's; the Console shows them and can switch them through the provider's file (blocked now: L1, L2, L5; DN-21). Nothing marks them as sandboxed                                 | The provider's                               |
| EC4 Separate products behind a contract                  | A future Wyss Agent (17 §7); the remote gateway and phone client (16 §5)                                                         | Their own processes                      | A provider is untrusted output; the gateway is part of the product's release                                                                                                             | Versioned protocols (§6.1)                   |
| EC5 Third-party executable plugins loaded by the Console | **None.** Not a goal                                                                                                             | —                                        | If ever wanted, its ADR must answer first: what code runs, with which rights, signed by whom, verified how, updated under whose control. Using an interface does not make code sandboxed | —                                            |

A plugin's interface is not a sandbox. An extension of class EC3 or EC4 runs with the user's
rights unless its own design proves otherwise.

## 11. Data a public user must be able to trust

| Promise                                                           | Mechanism                                                                                                                                                                                                                                                                                  |
| ----------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| The Console reads your agents' configuration and sends it nowhere | No network use by the configuration engine; held by a test that the module links no network client (TS-DIST-13)                                                                                                                                                                            |
| No telemetry                                                      | No network use at all unless the user enables the remote gateway (16) or an update check (DN-50)                                                                                                                                                                                           |
| It changes nothing without a plan you confirmed                   | 07 §5                                                                                                                                                                                                                                                                                      |
| It keeps no secret of yours in clear, and none without asking     | History and indexes hold identifiers, hashes and times (11 §5.5). The content of a hook you disable may be kept, encrypted, on your computer, after a consent that says what, where and for how long (27 §7). What that protects against, and what it does not, is said in the same dialog |
| A disabled entry does not always travel with its file             | On rungs 1 and 2 of ADR 0029 (proposed) it stays in the provider's own file and travels with it. **On rung 3 it is held in this computer's vault and does not travel**: the other computer sees it removed (16 §2.2; 27 §9.2). Export is designed and deferred (DN-29). This corrects C8   |
| You can remove everything it keeps                                | §7.2; 14 §8, with what deletion cannot reach                                                                                                                                                                                                                                               |
| It tells you what it could not verify                             | The evidence marks, carried into the interface (13 §1.1)                                                                                                                                                                                                                                   |

## 12. Decisions, questions and the ADRs this phase needs

| Item                                  | Owner      | State                                                                                                                                                    |
| ------------------------------------- | ---------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| DN-07                                 | The owner  | Whether the product is offered to others, and under which licence and support terms                                                                      |
| DN-49                                 | The owner  | The first package format and repository: PF2 recommended                                                                                                 |
| DN-50                                 | The owner  | Whether the application may check a signed release list over the network: off by default recommended                                                     |
| DN-51                                 | The owner  | Which platforms become PT1 after PT0                                                                                                                     |
| OQ-58, OQ-59, OQ-60                   | Evidence   | Candidate distributions' systemd, WebKitGTK and bubblewrap versions; Wayland and X11 behaviour on PT0; crate licences that need a reading of their files |
| ADR "Distribution layout and updates" | Before F13 | §4.3 (it changes ADR 0025's layout for system packages), §6, §7.2; replaces ADR 0027 only for the formats it adopts                                      |
| ADR "Security maintenance"            | Before F13 | §9                                                                                                                                                       |

F13 starts only by an explicit decision, after the product is accepted by its first user. It may
overlap technical phases and must not displace them.

## 13. Teams

A team sharing one methodology (a common set of rules and hooks, kept in step) was a main theme
of the research report and is not a stated requirement of the owner. It is noted as a later
study. It would build on profiles and on export and import, and it is where a "canonical
configuration" idea, set aside for a single user by AD-03, would have to be examined again.

## 14. Personal final-product scope override (DN-07=A, 2026-10-13)

The owner's final-product target is **personal local deployment** to his
own Linux computers and separately selected native Android client.
No public distribution, external support/SLA, repository publication,
release servers or user-facing packaging ecosystem is authorised.
ADR 0027 remains in force. All earlier public delivery designs in
§3–§11 remain *conditional research for a different future scope*,
not current required implementation. DN-49/51 become not-applicable
under current personal scope; DN-50 defaults to no network update check.

Personal delivery still needs a secure, maintainable lifecycle:
repeatable local install without damaging existing state; a supported
update path, versioned migrations and verified rollback; private
backups and key-loss handling; user-visible version, provenance
and supported reference hardware; dependency/licence checks for any
software actually shipped *even privately*. The scope cannot be used
to skip security maintenance or protecting user secrets.
The target is specified by FR-DIST-07 in
[02 §4](02-REQUIREMENTS.md), TS-FINAL-21 and its acceptance in
[20 §14](20-VERIFICATION-AND-ACCEPTANCE.md).

Changing back to public distribution requires a separate owner
scope decision, independent technical/legal/security review
and reopening the conditional DN-49/51 choices.

