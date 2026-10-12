# Changelog

## 0.2.0

A native desktop application for CachyOS with KDE Plasma, built and run from this
repository. Linux, with a `systemd --user` session. No installer and no published release.

### What it does

- **Terminal.** Local terminals in tabs, with your own shell. They work whether or not
  anything else does. The window opens at 1440×810, which gives the terminal about 120
  columns at the default font: enough for a shell greeting as wide as CachyOS's fastfetch.
- **Sessions.** Create a managed Codex session for a project folder, activate it, resume
  it, close it. Sessions start in Inference: Codex answers and nothing runs.
- **Agent chat.** Give a session an objective and follow the answer as it arrives; cancel a
  turn that is running.
- **Local Tools.** Choose it for a session, on purpose: Codex may propose commands and
  file changes inside the project folder, in a sandbox.
- **Approvals.** Every command and file change waits for you. Approve or deny each one;
  nothing you decide is remembered and nothing is approved automatically.
- **Events.** What the authority recorded, newest first, back to the first event.
- **Automatic authority.** The first launch installs and starts the authority under your
  login session's service manager. Closing the window does not stop it; it is restarted
  if it crashes; a newer one is installed the next time none is running.
- **Settings.** Font size, scrollback and cursor of the terminals, how event times are
  written, the view Wyss Console opens on, a read-only look at the authority and the
  version, and a reset. Preferences change how Wyss Console looks, never what Codex may do.
- **One name and every status, at every size.** The window and the top bar say Wyss
  Console, as everything else does. The Terminal, Codex and Authority statuses stay in
  the top bar down to the narrowest window: a narrow one drops the state's word, then the
  status's name, and keeps the coloured dot and the tooltip. Settings uses two columns in
  a wide window. The project folder dialog opens in your home folder.

### Packaging

- One directory package (`npm run desktop:package`): the desktop and the authority it was
  built with. The desktop only installs an authority whose SHA-256 it was built with.
- No `.deb`, no AppImage, no release build environment and no acceptance matrix: they were
  removed, with their scripts, tests and documents
  ([ADR 0027](docs/adr/0027-local-product-no-distribution-packaging.md)).

### Checks

- `npm run check` (formatting, lint, typecheck, the frontend tests, the build) and
  `npm run check:rust` (formatting, check, the Rust suite, Clippy). The frontend tests do not
  depend on the runtime locale or on the system they run on.
- `npm run scan:repo`: every tracked file is scanned for credentials, as Git records it and
  as it is on disk; it fails closed, and takes no replace ref's stand-in for an object.
- Found and fixed while closing it: the credential scan could pass a file it had not read, or
  a committed file missing from the working tree; Retry could be ignored right after a
  failure; Settings was drawn under the other views; the reset confirmation's buttons were
  below the fold at 960×640; a frontend test assumed an English clock; a development build
  could show a stale revision.

### Known limits

- Dark theme only. One Codex runtime, in one mode, at a time. History cannot be searched
  or exported. No light theme, no Settings for approvals, by design.
- Codex is not included and has to be installed separately.
