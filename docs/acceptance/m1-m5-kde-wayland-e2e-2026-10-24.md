# M1–M5 on the real KDE Wayland desktop — 2026-10-24/25

**Verdict: PARTIAL.** The packaged GUI ran natively on the owner's KDE Plasma Wayland session and connected to a separate test authority (protocol 2, same packaged binary, disposable store). Session creation, persistence across GUI restart, terminal recovery through the host, the provider gate and the lease gate were verified visually. The Codex runtime did not start in either mode, so turns, approvals, capacity-one and V9 recovery were not reached.

## Tested
- `origin/main` `0e9a5341a9df4e6169436a1d52320ce8548c031c`; same package as the previous report (hashes re-verified: all four files "match").
- Desktop: Plasma 6.7.5 / KWin 6.7.5, Wayland (`wayland-0`), scale 1, 1920×1080, Intel UHD (CML GT2) via EGL, `XDG_CURRENT_DESKTOP=KDE`.
- **Native Wayland, not XWayland:** the GUI process holds no X11 socket and `GDK_BACKEND` was not set; the window is a KWin Wayland window with server-side decorations.
- GUI launched without `--gui-only`: `wyss-console --f8-authority <test dir>`, private `HOME`/`XDG_RUNTIME_DIR`/D-Bus, `WAYLAND_DISPLAY` pointing at the real socket.
- Test authority: `wyss-control --protocol 2 --create` with its own runtime dir, SQLite store and Codex home (fixture `auth.json`, no `--allow-credential`), real `codex-cli 0.159.0` binary. No production credentials.
- Production before/after: launcher symlink unchanged (2026-10-23), `wyss-control` (255925, 701187) and both user units active, none touched.
- Input: `ydotool` through a private `ydotoold`, with the owner's consent; windows captured with `spectacle -a`. One accidental capture of the owner's terminal was deleted immediately. The clipboard was restored afterwards.

## Results
| Scenario | Result | Notes / evidence |
| --- | --- | --- |
| Native Wayland vs XWayland | PASS (native) | no X11 socket in the process |
| Window size vs screen (1440×810 problem) | PASS here | the window is 1440 wide, fits 1920×1080 at (240,105); the earlier clipping was the 1400-px test screen. Not a defect on this desktop. |
| Decorations, HiDPI/fractional scaling | scale 1 only; fractional NOT_PROVEN | w01 |
| GUI connects to test authority ("Connected · protocol 2") | PASS | w05 |
| M1/M5: create managed session | PASS | w06 |
| Persistence: close GUI, reopen, session and Terminal 1 restored | PASS | w11, w13 (authority not restarted) |
| M4 terminal recovery via host (same shell, pending input kept) | PASS | w11 |
| M4 typing with default input method | **FAIL** | see Defect 1 |
| M4 typing with `GTK_IM_MODULE=gtk-im-context-simple` | PASS | w12 |
| M4 resize, scrollback, full-screen apps, minimize/restore, shortcuts | NOT_PROVEN | not driven |
| Clipboard paste into a GTK dialog | PASS | w16 |
| M3 Claude Managed disabled | PASS (gate shown) | "Claude Code Managed is not implemented in this authority" |
| M3 offline fixtures | BLOCKED | production package has no `m3-fixture` |
| M5 lease refused without ready runtime | PASS | Grant lease disabled, "needs a ready runtime" (w19/w20) |
| M2 start Codex runtime, Inference | **FAIL** | Defect 2 |
| M2 start Codex runtime, Local Tools | **FAIL** | Defect 3 |
| M2 turns, approvals, error handling of a live turn | BLOCKED | by defects 2–3; no paid inference attempted |
| M5 capacity-one, isolation between sessions | NOT_PROVEN | needs a running runtime |
| V9 restart/replay/no duplicate execution | NOT_PROVEN | authority was not restarted; Rust tests only |
| Orphan processes, cleanup | PASS | authority, host, ydotoold, bwrap all gone; production unchanged |
| Window close button (pointer) | NOT_PROVEN | pointer click on the title-bar X did not close; Alt+F4 closed it cleanly. Probably my pointer calibration. |

## Defects
1. **High (usability) — terminal input corrupted on KDE Wayland with the default input method.** Typing `echo hello wayland` produced `ecchchocho cho echo hecho helcho …` and the shell executed the garbage; single keystrokes show `c` doubled. Same result at 30 ms and 200 ms per key. With `GTK_IM_MODULE=gtk-im-context-simple` the same input is correct. This desktop runs ibus with Wayland IM enabled, so the probable cause is composition/preedit text being re-sent to the terminal by the web view's input handling (packaging/integration; not a build defect). Needs reproduction with another IM and a fix or a documented launcher variable.
2. **Medium — Start runtime in Inference mode returns "The console sent a request it should not have. Nothing changed."** (invalid request), reproducible; the authority log is empty, so the rejecting layer is not identified. The GUI sends no folder in this mode as designed (`service.rs` `runtime_start`).
3. **Medium — Start runtime in Local Tools: generation 1 goes to Failed, "The runtime process could not be started"**, after two `bwrap` processes appeared. Caveats that keep the cause open: the folder picked was `…/ws/.git` (the chooser auto-completed into `.git`), and the Codex home had no real configuration. Not reproduced with a clean setup.
4. **Low — no diagnostic detail:** both failures surface only a generic message and no authority log.

## Not done / limits
Resize, fractional scaling, minimize/restore, scrollback, focus switching, capacity-one, approvals and V9 replay were not exercised. Synthetic provider behaviour was not used at all (no fixture build), so nothing here is a simulated response presented as acceptance.

## Recommendation
Fix or document Defect 1 first (it breaks the terminal for this desktop's default setup), then diagnose Defects 2–3 with authority-side logging in a clean workspace (repo root with `.git` and `.codex`, no `.git` selection) before repeating the M2/M5/V9 pass.
