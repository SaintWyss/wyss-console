# Incident 2026-10-20: a benchmark ended the owner's live GUI and terminal host

Severity P0 (test-harness safety). **Irreversible loss:** the owner's original fish terminal and its
in-memory shell state (history of the session, running jobs, scrollback) were destroyed. A new fish
terminal now exists in a relaunched window; **that session is not recovered and is not to be recorded
as recovered.**

## What happened

`benchmarks/idle-ab.sh` obtained "its" application PID with `pgrep -x -n wyss-console` and ended it with
`kill -TERM $APP`. The binaries under test were copies with other names, so the pattern could only match
the owner's real processes: the live GUI (PID 1310334) and the persistent host (PID 1188641) were
terminated, in two rounds, and the measurements of those rounds were of the owner's processes. The
installed application was relaunched afterwards (new GUI and host); the A/B script was rewritten.

## Root cause

Process selection by name ("the newest process called X") instead of by ownership, followed by a signal
to a bare PID. Nothing tied the PID to something the script had started. The first rewrite checked
`/proc/PID/exe` against the binary's path before ending it, which is still check-then-kill on a bare PID
and would still end the owner's process if the path matched (a copy is not the only possible target), and
the abort branch did not re-check at all.

## Audit (2026-10-20)

Every tracked place that signals a process: `benchmarks/idle-ab.sh` (the defect), `scripts/install.sh`
(asks the host to stop through its own `--wyss-host-stop`, never a signal; `--keep-host` skips it),
`scripts/tool.mjs` (forwards to its own child), `scripts/research/f1b/h.py` (own `Popen` handle) and the
Rust code and tests (own `Child` handles, `killpg` on groups the worker created, `prctl` death signals).
None of the others selects a process by name or path.

## Fix

- `benchmarks/owned.py`: a process may be ended only if this program launched it. It is a direct child
  (an unreaped child's PID cannot be reused), a **pidfd** is opened at once, every signal goes through
  that descriptor (`pidfd_send_signal`; a gone process yields an error, never another process), the
  launch is verified (parent is this process, executable is the expected one) and a mismatch ends only
  what was launched. Descendants are adopted by descriptor after a double-read check and only adopted
  ones are signalled; anything else is reported, never touched. `with Owned(...)` ends the process on
  normal exit, exception, timeout (SIGTERM, then SIGKILL on the descriptor), SIGINT and SIGTERM.
- `benchmarks/idle_ab.py` replaces `idle-ab.sh` and uses only `Owned`.
- `benchmarks/test_owned.py` (in `npm run check` as `test:harness`): a decoy process running the _same
  executable path_ as the launch survives normal end, an exception, a process that ignores SIGTERM, a
  launch that is not the expected binary, an already-exited process, a signal through a dead descriptor,
  and interruption; an adopted child is ended with its owner and a same-named non-child cannot be
  adopted. A lint test fails any script under `benchmarks/` or `scripts/` that uses `pkill`, `pgrep`,
  `killall`, `pidof`, `fuser`, `os.kill` or `kill` on a variable, and a test shows it catches the lines
  of the original script.
- Hygiene: `npm run scan:repo` also stops tracked links to the working machine, dependency or build
  directories, and the owner's home directory in code or configuration (a separate cause: a local
  `node_modules` link was committed by mistake the same day; removed, history kept).

## Rules until reviewed

No process-ending benchmark is run again until the fix is independently reviewed. Before any run with the
application: note the owner's live PIDs, and check they are unchanged afterwards.

## Independent review

Two cold passes by Codex (`codex-gsd-check`), given the goal and the code and none of the author's
reasoning. Pass 1: no path signalling a process the program did not launch; findings fixed: a launch that
failed after `Popen` (pidfd error, early interrupt) left the child running; `stop()` could be cut short by
an error or a second interrupt; the lint missed `kill -- "$X"`, `kill -s`, `kill 1234`, `os.killpg`,
`os.system`. Pass 2 (of the fixes): cleanup of a failed launch could itself be interrupted; the
Popen fallback assumes nobody else reaps the child (now refused when a SIGCHLD handler exists); the lint
missed `process.kill` and `command kill`/`/bin/kill`. All fixed, each with a test that fails on the earlier
code. The residual assumption is documented in `owned.py`: this class is the only reaper of its child.

## Follow-up: SIGCHLD ignored, and measurement helpers

- `Owned` now requires SIGCHLD at its default disposition. `SIG_IGN` was accepted before, but on Linux it
  makes the kernel reap children at once, which frees the PID while the class still holds it. Test:
  `test_ignoring_sigchld_is_refused…`; the default path is covered by `test_the_default_sigchld_disposition_still_works`.
- `idle_ab.py`: the `threads.py` helpers are its own unreaped children and are now ended, on error or
  interrupt, only through their own `Popen` handles; output files are closed with `with`. No benchmark was rerun.
