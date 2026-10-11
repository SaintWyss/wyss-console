# Local acceptance — Wyss Console 0.2.0

Status of every line: **PENDING_LOCAL_ACCEPTANCE**.

These checks need a real desktop session, a real `systemd --user`, a real Codex login and
a real display. The cloud build could not observe any of them and did not try to claim
them. Run them on the machine the product is for, with the artifact from `.artifacts/release/`
(check it against `SHA256SUMS` first), and change a line to `PASS` or `FAIL` only after
running it.

What the cloud did observe (a window started under Xvfb from the extracted packages, with an
isolated home and no service manager; see `RELEASE.md`) is not any of these lines and none of
them is marked from it.

Do not use a checkout, `cargo` or `npm` for any of this. If a step needs one, that is a
finding.

| #   | Check                                                                                                                                                                                                                      | Result                   |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------ |
| 1   | `sha256sum -c SHA256SUMS` passes for the artifact you will install.                                                                                                                                                        | PENDING_LOCAL_ACCEPTANCE |
| 2   | Install the real artifact (`sudo apt install ./wyss-console_0.2.0_amd64.deb`, or make the AppImage executable). No terminal is needed after this step.                                                                     | PENDING_LOCAL_ACCEPTANCE |
| 3   | Launch Wyss Console from the application menu.                                                                                                                                                                             | PENDING_LOCAL_ACCEPTANCE |
| 4   | The authority bootstraps through the real `systemd --user`: the top bar goes from Starting to Connected with nothing started by hand. `systemctl --user status wyss-control` shows the one unit, running.                  | PENDING_LOCAL_ACCEPTANCE |
| 5   | Inference is Ready: create a session, activate it, the Codex pill says Ready.                                                                                                                                              | PENDING_LOCAL_ACCEPTANCE |
| 6   | Local Tools is Ready: create a session in Local Tools on a throw-away folder, activate it.                                                                                                                                 | PENDING_LOCAL_ACCEPTANCE |
| 7   | One managed turn: give an objective, the answer streams in and the turn ends.                                                                                                                                              | PENDING_LOCAL_ACCEPTANCE |
| 8   | One approval: in Local Tools, a proposed command or file change waits in Approvals; approve it once; it runs once; deny another and it does not run.                                                                       | PENDING_LOCAL_ACCEPTANCE |
| 9   | Events shows the session, the turn and the approval just made, newest first.                                                                                                                                               | PENDING_LOCAL_ACCEPTANCE |
| 10  | Settings persistence: change the terminal font size, the cursor, the time format and the opening view; close the window; open it again. Every change is still in force and the window opens on the chosen view.            | PENDING_LOCAL_ACCEPTANCE |
| 11  | Settings do not weaken anything: after step 10 a new session is still created in Inference, and the Local Tools approval of step 8 still waits. Reset preferences: the defaults return and the sessions and history stay.  | PENDING_LOCAL_ACCEPTANCE |
| 12  | Close and reopen: the authority survived (same sessions, same history, Connected at once). Log out and in, or reboot: opening the window starts it again.                                                                  | PENDING_LOCAL_ACCEPTANCE |
| 13  | Native KDE blur: the window is translucent and blurred under KWin; with the blur refused or off it falls back to the opaque window and stays readable.                                                                     | PENDING_LOCAL_ACCEPTANCE |
| 14  | 960×640: every view fits, Settings scrolls and nothing is cut off, and no view shows the content of another (Settings in particular is only drawn in Settings).                                                            | PENDING_LOCAL_ACCEPTANCE |
| 15  | Narrow layout (about 360 px wide): the top bar, the sidebar, Settings rows and the approval card stay usable.                                                                                                              | PENDING_LOCAL_ACCEPTANCE |
| 16  | Upgrade from a previous managed authority (an install of the Phase 3G package, or an older 0.2.x): with it **running**, launch the new version — it connects, says `update pending`, and replaces nothing.                 | PENDING_LOCAL_ACCEPTANCE |
| 17  | Then stop the authority (`systemctl --user stop wyss-control`) and launch again: the new authority is installed and started, and the sessions and history of before are still there.                                       | PENDING_LOCAL_ACCEPTANCE |
| 18  | Uninstall the application (`sudo apt remove wyss-console`, or delete the AppImage) **without** deleting canonical data: `~/.local/state/wyss-console/` is still there, and so are the managed authority and its unit.      | PENDING_LOCAL_ACCEPTANCE |
| 19  | AppImage only: it starts from a double click and bootstraps the authority from inside the mounted image (the authority ends up installed under `~/.local/share/wyss-console/authority/` and keeps running after you quit). | PENDING_LOCAL_ACCEPTANCE |

When every line is `PASS`, the release can be called ready. Until then it is a candidate:
code complete and checked in the cloud, not yet accepted on a desktop.
