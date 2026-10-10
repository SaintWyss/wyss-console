# Changelog

## 0.2.0 — release candidate

The first installable Wyss Console. Linux, with a `systemd --user` session. Checksummed,
not signed. Not yet accepted on a real desktop: see `docs/release/LOCAL_ACCEPTANCE.md`.

### What it does

- **Terminal.** Local terminals in tabs, with your own shell. They work whether or not
  anything else does.
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

### Packaging

- A `.deb` and an AppImage, each carrying the authority the desktop was built with; the
  desktop only installs an authority whose SHA-256 it was built with.
- `SHA256SUMS` and a release manifest; a verifier that opens the packages.

### Known limits

- Dark theme only. One Codex runtime, in one mode, at a time. History cannot be searched
  or exported. No light theme, no Settings for approvals, by design.
- The AppImage was built on Ubuntu 24.04 and needs a system at least that recent.
- Codex is not included and has to be installed separately.
