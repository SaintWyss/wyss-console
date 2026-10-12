# ADR 0027: A local product, with no distribution packaging

Status: Accepted
Date: 2026-10-12

## Decision

Wyss Console is a native desktop application for our own environment, CachyOS with KDE
Plasma. It is built from this repository and run from its directory package. Nothing is
packaged for other distributions and nothing is published.

```text
npm ci  ->  npm run desktop:package  ->  .artifacts/package/<name>/bin/wyss-console
```

The priority is the working application, not multi-distribution packaging or release
certification.

## What was removed

[ADR 0026](0026-settings-and-release-closure.md) first made the product installable through
a `.deb` and an AppImage. Both carry the libraries of the system that builds them, so they
needed one reviewed build system, and with it a pipeline that had grown larger than what it
delivered. All of it is gone:

| Removed                                                                    | Why it existed                                              |
| -------------------------------------------------------------------------- | ----------------------------------------------------------- |
| `release:package`: the `.deb` and the AppImage, `SHA256SUMS`, the manifest | installable artifacts                                       |
| `release:verify`: the verifier that opened the artifacts                   | what the artifacts were checked against                     |
| `scripts/appimage-tools.json`, `scripts/appimage-tools.mjs`                | pinning the executables the AppImage bundler downloads      |
| `scripts/build-environment.json`, `scripts/release.Containerfile`          | the Ubuntu 24.04 build environment and its archive snapshot |
| `scripts/reviewed-library-keys.json`                                       | the key blocks of that system's `libgnutls` in the AppImage |
| `docs/release/RELEASE.md`, `docs/release/LOCAL_ACCEPTANCE.md`              | release instructions and the two-environment acceptance     |
| the bundle targets, the `.deb` and AppImage sections of `tauri.conf.json`  | configuration of those formats                              |
| the tests of all of the above                                              |                                                             |

No Ubuntu or Debian system, container or virtual machine is needed to develop, build,
test or use the product.

## What stays

Everything that protects the product when it runs, and the checks of the repository itself:

- **The directory package** (`npm run desktop:package`), as ADR 0025 defined it: the
  desktop, the authority it was pinned to, and nothing else.
- **Provenance and the pin** (ADR 0025, ADR 0026): the desktop finds its authority from its
  own real executable, refuses a payload or a directory someone else could write, and
  installs only the authority whose SHA-256 it was compiled with.
- **The authority lifecycle, the sandbox, approvals, leases and the window's capability
  list**, untouched.
- **The credential scan of the repository** (`npm run scan:repo`). It was part of release
  verification and is a check of its own now: what it protects is what gets committed and
  pushed, which does not depend on any package format.
- **No signing and no updater** in the configuration, held by a test.

## Consequences

- There is no application-menu entry: the installers provided it. The package is started
  from its `bin/wyss-console`.
- A machine that is not ours has to build the product itself; nothing says it will work
  there.
- The authority lifecycle tells a crash loop from a failed start by the unit's
  `Result=start-limit-hit` (`src-tauri/src/lifecycle/bootstrap.rs`). systemd reports that
  after a crash loop from version 260 on (read in its source; observed on CachyOS with 262).
  On an older systemd the window would say the start failed and would start the unit again
  when it is reopened. No supported system has one; it is recorded here because supporting
  one would need that fixed first.
- A distribution format, if one is ever wanted, is a new decision with its own phase. It is
  not to be rebuilt piece by piece on this one.
