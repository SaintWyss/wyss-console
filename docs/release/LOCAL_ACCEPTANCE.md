# Local acceptance — Wyss Console 0.2.0

Status: **0 of 19 controls executed, in either environment. Every result is `NOT RUN`.**
The release is not accepted.

Each line is recorded as one of:

| Result    | Meaning                                                                   |
| --------- | ------------------------------------------------------------------------- |
| `PASS`    | Executed in that column's environment, with the real artifact, and passed |
| `FAIL`    | Executed there and failed (say what was seen)                             |
| `NOT RUN` | Not executed in that environment: the result is unknown                   |
| `n/a`     | The line does not apply to that environment (KDE blur; the AppImage)      |

These checks need a real desktop session, a real `systemd --user`, a real Codex login and
a real display. Run them on the machines the product is for, with an artifact from
`.artifacts/release/` (the directory `npm run release:package` fills only with a set that
verified; a refused set is in `.artifacts/release.rejected/` and is not a candidate), check
it against `SHA256SUMS` first, and change a line to `PASS` or `FAIL` only after running
it, saying where and from which artifact (the `revision` of its `release-manifest.json`).

**What the cloud ran is not any of these lines**, and none is marked from it: a build and
verification in the release build environment (Ubuntu 24.04 in a container, no desktop
session, no service manager, no Codex), unit and process tests, the frontend under jsdom,
and a browser engine at fixed viewport sizes (see `RELEASE.md`). That a similar thing was
tested elsewhere is not a `PASS` here.

### Environments

| Environment       | What is installed                                                                                                                                                                                          |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Debian/Ubuntu** | The `.deb`: `sudo apt install ./wyss-console_0.2.0_amd64.deb`; removed with `sudo apt remove wyss-console`. `bubblewrap` 0.11 or newer for Local Tools (Ubuntu 24.04 ships 0.9.0: lines 6 and 8 need it).  |
| **CachyOS/KDE**   | The AppImage (there is no `apt`, and the `.deb` is not for Arch-based systems): `chmod +x wyss-console_0.2.0_amd64.AppImage`; removed by deleting it. A double click needs FUSE 2: `sudo pacman -S fuse2`. |

`sha256sum` and `systemctl --user` are the same on both. Do not use a checkout, `cargo` or
`npm` for any of this. If a step needs one, that is a finding.

| #   | Check                                                                                                                                                                                                                      | Debian/Ubuntu (.deb) | CachyOS/KDE (AppImage) |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------- | ---------------------- |
| 1   | `sha256sum -c SHA256SUMS` passes for the artifact you will install.                                                                                                                                                        | NOT RUN              | NOT RUN                |
| 2   | Install the real artifact as its environment does (see Environments above). No terminal is needed after this step.                                                                                                         | NOT RUN              | NOT RUN                |
| 3   | Launch Wyss Console from the application menu.                                                                                                                                                                             | NOT RUN              | NOT RUN                |
| 4   | The authority bootstraps through the real `systemd --user`: the top bar goes from Starting to Connected with nothing started by hand. `systemctl --user status wyss-control` shows the one unit, running.                  | NOT RUN              | NOT RUN                |
| 5   | Inference is Ready: create a session, activate it, the Codex pill says Ready.                                                                                                                                              | NOT RUN              | NOT RUN                |
| 6   | Local Tools is Ready: create a session in Local Tools on a throw-away folder, activate it.                                                                                                                                 | NOT RUN              | NOT RUN                |
| 7   | One managed turn: give an objective, the answer streams in and the turn ends.                                                                                                                                              | NOT RUN              | NOT RUN                |
| 8   | One approval: in Local Tools, a proposed command or file change waits in Approvals; approve it once; it runs once; deny another and it does not run.                                                                       | NOT RUN              | NOT RUN                |
| 9   | Events shows the session, the turn and the approval just made, newest first.                                                                                                                                               | NOT RUN              | NOT RUN                |
| 10  | Settings persistence: change the terminal font size, the cursor, the time format and the opening view; close the window; open it again. Every change is still in force and the window opens on the chosen view.            | NOT RUN              | NOT RUN                |
| 11  | Settings do not weaken anything: after step 10 a new session is still created in Inference, and the Local Tools approval of step 8 still waits. Reset preferences: the defaults return and the sessions and history stay.  | NOT RUN              | NOT RUN                |
| 12  | Close and reopen: the authority survived (same sessions, same history, Connected at once). Log out and in, or reboot: opening the window starts it again.                                                                  | NOT RUN              | NOT RUN                |
| 13  | Native KDE blur: the window is translucent and blurred under KWin; with the blur refused or off it falls back to the opaque window and stays readable.                                                                     | n/a                  | NOT RUN                |
| 14  | 960×640: every view fits, Settings scrolls and nothing is cut off, and no view shows the content of another (Settings in particular is only drawn in Settings).                                                            | NOT RUN              | NOT RUN                |
| 15  | Narrow layout (about 360 px wide): the top bar, the sidebar, Settings rows and the approval card stay usable.                                                                                                              | NOT RUN              | NOT RUN                |
| 16  | Upgrade from a previous managed authority (an install of the Phase 3G package, or an older 0.2.x): with it **running**, launch the new version — it connects, says `update pending`, and replaces nothing.                 | NOT RUN              | NOT RUN                |
| 17  | Then stop the authority (`systemctl --user stop wyss-control`) and launch again: the new authority is installed and started, and the sessions and history of before are still there.                                       | NOT RUN              | NOT RUN                |
| 18  | Uninstall the application as its environment does (see Environments above) **without** deleting canonical data: `~/.local/state/wyss-console/` is still there, and so are the managed authority and its unit.              | NOT RUN              | NOT RUN                |
| 19  | AppImage only: it starts from a double click and bootstraps the authority from inside the mounted image (the authority ends up installed under `~/.local/share/wyss-console/authority/` and keeps running after you quit). | n/a                  | NOT RUN                |

When every line is `PASS` in each environment it applies to, the release can be called
ready. Until then it is a candidate: built and verified in the cloud, not accepted on a
desktop.
