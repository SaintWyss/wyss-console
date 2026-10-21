# Changelog

## 2026-10-21 — F8: installed wiring, W1 native confirmation, candidate validation (still not activated)

- `wyss-control --protocol 2 …` serves a V8 image of its own on a private socket (no arguments is still
  protocol 1; never falls back); `wyss-console --f8-authority <dir>` is the only way the window becomes
  active; version, schema and socket-directory checks on connect; `main` + `f8` capabilities listed.
- W1: accepting an approval and granting/renewing an executing lease need **Allow** in a native GTK window
  that shows what the authority holds (fail closed, inert for a moment, bidi-safe).
- `wyss-f8-fixture` (feature `f8-fixture`, never packaged) for isolated validation; real-process tests
  for both authorities; the resolved ACL is tested against the framework.
- Package installed over the previous one with `--keep-host`; installed store, authority and sessions untouched.
  Authority in production has no runtime (`NoRuntime`): Codex is not launched.

## 2026-10-21 — F8: protocol-2 window and integrated acceptance (inactive, not activated)

- New **Runtimes** window (sessions, runtime lifecycle, leases, bindings, turns with live
  streaming and cancellation, native command/file approvals with accept/decline/cancel, bounded
  history, reconnect/offline states) over protocol 2. Reachable only through a Rust-owned connector
  (`f8-dev`); the installed `main` capability grants only the state read.
- F8-01..F8-18 and all 29 methods pass through the real application and transport; defects found
  by that integration fixed (status facts, recovery reasons, binding recovery, request latency,
  idle-connection reuse, generation counter, idle CPU). Independent review: no CRITICAL/HIGH;
  bidi-safe approval card, bounded status lookups, rate-limited reconnect.
- Installed app stays on protocol 1 / schema 7. Not proven: authenticated provider, multiprocess
  credential refresh, installed migration. ADR 0051 still Proposed; activation not authorized.

## 2026-10-20 — Per-session managed runtimes, proposed (F8 design only)

- ADR 0051 (Proposed): identity, attestation per generation, bounded concurrency and budgets, cancellation,
  recovery and a V6→V7 migration with its evidence list. No code; the authority and its guarantees are unchanged.

## 2026-10-20 — Remote gateway core, second review (still no transport)

- Five fixes from a second cold review: a stored decision no longer outlives its issuer's revocation,
  an enrolled key cannot be paired again to shed `suspect`, the gateway's time never goes back, rotating
  source names cannot reset the failure limiter, and the journal keeps a digest of a target, not the
  target. One finding declined with its reason (ADR 0049). 36 tests; unreachable, signature scheme open.

## 2026-10-20 — Project files and hook/MCP templates (F5 closes)

- **Choose project…** (native folder window; judged: inside the home, no link, nobody else can write it,
  not a provider or credential folder). Project `CLAUDE.md`, rules, skills and `AGENTS.md` can be listed,
  created, edited, deleted and undone through the same transaction; a project file's plan or undo only
  applies while its project is selected.
- Hook and MCP-server **templates**: sealed export of an entry; applying one only fills the native form,
  after which it is the ordinary reviewed, confirmed plan (ADR 0050).
- Benchmark safety follow-up: `owned.py` also refuses an ignored SIGCHLD; `idle_ab.py` ends its own
  helpers on error.

## 2026-10-20 — Remote gateway core, no transport (F10a foundations)

- Host-side decisions of the remote design as code with tests and no listener: grants and profiles
  (LP4 and `terminal.write` refused), device states, QR-secret pairing, per-request check with one-word
  refusals, step-up binding, limiter, content-free audit, revocation (ADR 0049). Unreviewed and
  unreachable; the signature scheme and the transport are undecided (DN-05).

## 2026-10-20 — Idle cost of the host and the terminal pull (P1 increment, P1 still open)

- A host with one idle terminal woke 243 times a second (0.65 % of a core): its accept loop polled every
  5 ms and its pump every few ms. It waits now (about 14 wake-ups a second), and the page's output pull
  is a one-second long poll instead of three asks a second. Measured before/after in
  `docs/acceptance/p1-2026-10-20.md`, with the backend baseline (throughput, echo latency, memory per
  terminal, runaway output) over three runs. No budget is declared passed.

## 2026-10-20 — Models view, catalogue offers, `model/rerouted`, native effort (F6 increment)

- **Models** view: what the managed runtime offers (default, efforts, replacement marking,
  completeness, provenance, "listing is not entitlement"), what the agents' files configure, what
  profiles select and what a runtime reported in use; never merged (ADR 0048).
- `model/rerouted` is accepted for exactly the running turn and exactly its schema; everything else
  stays an isolation violation. Recorded in memory only. An external review's one finding is fixed.
- Profiles carry a reasoning effort, passed per agent (`--effort`; `-c model_reasoning_effort`), and a
  terminal host older than the window is detected instead of failing opaquely.

## 2026-10-20 — Create and delete instruction files and skills (F5 increment)

- **New instruction file…** creates a Claude Code skill/rule/`CLAUDE.md`, a Codex skill
  (`~/.agents/skills`)/`AGENTS.md`/override or an OpenCode `AGENTS.md`, optionally from a sealed
  template; **Delete…** removes one. Absence is a journal state distinct from an empty file; creation
  never replaces anything; deletion re-checks identity; undo restores exact bytes, mode and absence;
  an interrupted create/delete is repaired at start (ADR 0047).
- Codex skills in `~/.agents/skills` are listed and editable (verified through Codex's own
  `skills/list`, which discovers a created skill and stops listing a deleted one).
- Search by name in the Configuration view; history shows file-less digests; clearer wording.

## 2026-10-19 — Native instruction-file editing and templates (F5 increments)

- Passphrase-sealed instruction templates, native import review and separate reversible
  application (ADR 0046); imports grant no authority.
- Existing user skills/rules open in the native editor, with mandatory native confirmation,
  always-sealed exact undo and concurrent-write/crash recovery (ADR 0045).
- GUI-only upgrades can preserve the running host with `install --keep-host`.
- Corrected the pushed-main/Codex-TOML checkpoint and real-environment testing instructions;
  normal development no longer redirects XDG configuration/data to private directories.

## Unreleased

### Writing configuration, models (F5, F6a)

- **Codex MCP servers** can be added (a new table), changed in place and deleted with every table
  of the server, and put back; verified on the real `config.toml` with the real Codex CLI, byte for
  byte. ADR 0044.

- **Add a hook** (Claude Code) and **add, change or delete an OpenCode MCP server** from the
  Configuration view. The command is typed in a window of the desktop's own, never built from page
  text; the native confirmation shows what will run; an edit is refused if the file moved on; every
  authored change can be taken back to the exact bytes. ADR 0042.
- A managed session shows the model its runtime **reported** (or that none was); a native session
  shows what its profile asked for, what its files say and that the agent does not report the one in
  use. ADR 0043.
- `docs/research/inference-isolation-2026-10-19.md`: experiments E1–E8 for managed Inference
  isolation, with a recommendation that is not yet proven.

### OpenCode in the inventory (F4a, read only)

- The Configuration view lists OpenCode's plugins, MCP servers, models, providers (by name only,
  never what is inside) and instructions files, from `opencode.jsonc` with comments and trailing
  commas. Turn an MCP server on or off, take a plugin out of the list and put it back, as edits of
  the file's own bytes (comments and the keys stay exactly as they were), with undo. ADR 0041.

### Install, upgrade, roll back, diagnose (F13)

- **`install.sh`** (also inside the package) installs for the current user only, after checking
  every file's SHA-256; an upgrade is staged beside the old version, asks the host to stop, swaps
  by rename and keeps the old one for **`rollback`**; **`uninstall`** keeps the Console's state
  unless `--purge` and never touches `~/.claude` or `~/.codex`. `--host-service` writes an
  optional `systemd --user` unit for the host.
- **`wyss-console --diagnose`** prints versions, the host, the agents found, the Secret Service,
  the state directory and the agents' folders' permissions: no values. ADR 0040.

### Handoff between agents (F11a)

- **"Hand off…" on a session** prepares a brief to read and edit: the folder, the repository's
  state and the end of the agent's screen, with secrets removed. Starting the other agent opens a
  **new** session with it as the first message; nothing is moved, and the screen says it is not a
  transfer. It can also be saved to a file. ADR 0039.

### API credentials (F14)

- **A Credentials view keeps provider API keys sealed** (vault of ADR 0034): add, replace,
  rename, delete (confirmed in a native window) and check. A key is typed in a native masked
  window and is never shown, returned or written unsealed; only a name, the service and a short
  checksum are listed. With no key store, nothing is kept. Using a key to start an agent is not
  part of this. ADR 0038.

### Panes, search, shortcuts and a saved layout (T1, T2)

- **Split a tab into panes** (side by side or stacked, up to 16, drag the dividers); each pane
  is its own terminal. **Search the scrollback** with Ctrl+Shift+F (case option, next and
  previous). **Console shortcuts** are Ctrl+Shift+T/D/E/W/F, arrows, `.` `,`, PageUp/PageDown,
  1 to 9; every other key goes to the terminal (legend behind the keyboard button).
- **The layout comes back**: tabs, splits, ratios and focus are saved and laid over the host's
  live terminals when the window opens; a shell that is gone is made again with its title and
  folder, an agent never is. ADR 0037.

### Sessions that outlive the window (F9)

- **Terminals and native agent sessions are held by a host process** (`wyss-console
--wyss-host`, started by the window when none answers). Close or kill the window: the
  terminals and the agents keep running; open it again and every tab is back, with its
  scrollback (512 KiB per terminal) and, for agents, its tag; the overview says Active with the
  same process. A host that holds nothing exits after ten minutes; `--wyss-host-stop` stops it.
- A private Unix socket (directory 0700, socket 0600, peer uid checked), one request per
  connection, a closed list of operations. If no host can be started, the window runs one of its
  own, as before, and says so. ADR 0036.
- Plain terminals also lose the per-session variables of an agent session the Console was
  started from.

### Native agent workspace (F3a, F3b, F3c)

- **Start Claude Code and Codex from the Console** (new Workspace view). Pick an agent, a
  project folder (home, a folder chosen in the native dialog, or a known project) and a
  profile; the installed CLI starts in a terminal tab through your own shell, so the
  environment is the one you get when you type it. Words reach the agent as words: tested for
  `sh`, `bash`, `zsh` and `fish`. A shell without a recipe falls back to a direct start, and
  the session says so.
- **A registry of native sessions** with provider, version, project, profile, process,
  processor and memory (sampled only while the overview is shown), and the states Active,
  Stopped, Failed, Restarting and Lost. Stop, restart (a new session), remove. After the
  window is closed what was running is shown as **Lost**, never as exited. Sessions of one
  folder are grouped and warned about.
- **Session profiles** (new Profiles view): model, permissions, hooks off, skills off, notes;
  inheritance, 20 versions each, four read-only templates, export and import in native
  windows. A profile is applied when the session starts and changes no global file: shown
  with the installed Claude Code (an overlay `--settings` file per session; hooks off for
  that session only; the user's `settings.json` has the same SHA-256). A profile the agent
  cannot apply (Codex hooks or skills) refuses to start and says why.
- Seventeen new window commands; none takes a command, an argument list, an environment or a
  path. ADR 0035.

### Review of F2b and the ACL rule

- **Codex skills checked against the installed Codex** through its own `skills/list`: switched
  on and back on the real config, same bytes after the undo. The plan's wording says what is
  checked and what (a running session) is not.
- **One external review, defects fixed**: the named-accounts file is opened without following
  links and read from the descriptor judged; a private group is confirmed through the name
  service and refused when accounts are not local; an undo no longer overwrites an entry that
  was changed after the switch.

### Claude Code's folder with an ACL (F1/F2a trust)

- **Changes to `~/.claude` work again on a machine where the owner gave another account
  write access by ACL.** The Console now reads the folder's ACL: the owner's private group
  does not count as a second writer, and an account the owner names in
  `~/.local/state/wyss-console/configuration/trusted-writers` is accepted (the page says it
  can also write the folder). World-writable folders, other ACL users and shared groups stay
  read-only. No `chmod`, no ACL change; delete the file to go back to the strict rule.

### Codex skills and MCP servers, with a way back (F2b)

- **Codex's skills and MCP servers can be switched from the Configuration view**, in the
  real `~/.codex/config.toml`: one `enabled` line is added or replaced and every comment,
  blank line and key stays. Turning an MCP server on asks in the native window; the rest asks
  in the page.
- **Exact undo from one closed word.** The journal keeps what the key held (absent, enabled,
  disabled); the undo puts the file back byte for byte. Shown on the real configuration with
  an exact backup: the installed Codex listed the server as disabled, then enabled again, and
  the file had the same SHA-256 and mode as before.
- **A prediction, labelled as one**, of what the change does to managed Inference, held equal
  to the authority's own rule by an exhaustive test. Codex hooks are not changeable: their
  switch is not verified and enabling one is blocked.
- The same writer, journal and recovery as F2a: every step of an apply to Codex's file is
  tested failed and crashed, and the undo is exact after each.

### Changing Claude Code's configuration, with a way back (F2a)

- **Decision DN-01 is made (ADR 0034).** Something the Console keeps to put an entry back may
  hold a secret if it is encrypted at rest: authenticated encryption, with a key held by the
  system's Secret Service and nowhere else. Without that, an entry that may hold a secret is
  not turned off one by one; there is no silent plaintext. ADR 0003 and 0028 are reconciled,
  and a protected API Credentials section is recorded as a future requirement.
- **Hooks and skills can be turned off and on from the Configuration view.** A plan shows
  what would change and how it would be kept; risky changes ask in a native window; the file
  changes by exactly the bytes of the change, is checked against the plan and read back; a
  file changed meanwhile is not written. Turning a hook off and back on gives the file back
  byte for byte.
- **A journal, a history and undo.** Every apply writes its intent before and its outcome after;
  an interrupted one is repaired at start. Kept hooks can be exported under a passphrase,
  imported, and forgotten.
- **Failure injection at every step**, and tests that no store, journal, plan or dialog holds a
  seeded secret, or the key.

### Reading the agents' configuration, first part (F1-PR1)

- **The foundation of the configuration inventory exists in the desktop backend, and nothing
  uses it yet.** `src-tauri/src/agentconfig/` holds the typed model, the closed list of
  approved roots, a bounded read-only reader and redaction. It is part of the desktop
  binary only: the authority binary does not contain it, and a test searches the built
  binary to say so. Nothing in the window, the commands or the capability list changed, and
  nothing is written anywhere.
- **Reads stay inside what an adapter approved.** A path, or a link target, outside the
  approved roots is refused without being opened; credential files and `.ssh`, `.gnupg`,
  `.aws` and `secrets` folders under a root are never read; each directory is walked by
  descriptor, so a swap for a link in the middle of a read is not followed. Group-writable
  configuration (the owner's `~/.claude` is `0775`) is read and never marked writable.
- **Secrets stay out, configuration stays in.** Quoted, escaped and multi-line secrets,
  cookies, name/value pairs and numeric PINs are hidden; counts such as `max_tokens` and
  helper commands such as `apiKeyHelper` are shown.
- **Bounded.** File size, items, keys and values, nesting, link hops, listing length and
  re-reads each have a limit and a hard ceiling; a hostile document is refused before it
  is parsed.

### Reading the agents' configuration, second part (F1-PR2)

- **The Claude Code and Codex adapters read the real configuration into one inventory.**
  Hooks, skills, MCP servers, rule files and model preferences of both providers, from
  their user, project and system places, with the state each file declares (enabled,
  disabled) and what cannot be verified said as such: nothing reads a running provider.
  Still no window command uses it, and nothing is written anywhere.
- **Honest about what a file cannot say.** Codex is static only: its items are
  `not_verifiable`, the administrator's `features.hooks` pin beats the user's, a project's
  own Codex configuration is read only for a project the user trusts, and the unproven
  `[hooks.state]` table is skipped. Entries an adapter cannot place are counted, not guessed.
- **Nothing private in the result.** Sources are named by a closed key, items by a hash of
  where they were found, rule files by name and digest only; server environments, headers,
  tokens in commands and the rest of `~/.claude.json` never appear. TOML is parsed with the
  same bounds as JSON (the crate is the one tauri already builds).

### Reading the agents' configuration, third part: service, commands and view (F1-PR3)

- **The inventory is kept current and the window can ask for it.** A service holds the
  latest reading with a revision and a bounded log of which sources changed. A watcher on
  the providers' directories rescans after a burst of edits settles (200 ms quiet, 1 s at
  most), after a lost event, and every 30 s for what no watch can see, so a file edited in
  an editor shows within two seconds. The watcher only wakes the rescan; every read still
  goes through the approved roots.
- **Two new read commands, `configuration_inventory` and `configuration_changes`.** They
  return what the service already holds, take no path and write nothing. The desktop
  library only routes them: their code stays in the desktop binary, and the test that
  searches the built authority binary still finds nothing of the engine in it.
- **A Configuration view.** It lists hooks, skills, MCP servers, rule files and model
  settings of both agents with filters by agent and kind, opens an item to say where it
  was found, what the file says, what cannot be confirmed and why it cannot be changed,
  and says what became of every place that was read. It has no control that changes
  anything. It reads only while shown, asking whether the revision moved every 1.5 s.

### Sessions that can be told apart

- **A session is named so that no other session shown with it has the same name.** Until
  now every session was named for its project folder, and the product itself asks for a
  second session in a folder (a Local Tools session next to an Inference one). The rows in
  Sessions, the Agent tabs, the close, discard and mode prompts, the session an approval
  belongs to and the scope in Events then read as the same name twice. Now two folders that
  end alike are told apart by their parents (`a/app`, `b/app`), and several sessions in one
  folder by when they were created, or by their short identity when the time does not do it.
  A name that is already unique is left as it was.
- **A tab opened from Events is named for its folder** once the Agent view has read it,
  instead of staying under the session's identity. Opening a session again from Sessions
  gives its tab the name Sessions shows.
- **An approval always says which session it belongs to.** Every approval card, under its
  turn and in Approvals, and every ended approval in the list shows the session's canonical
  identity as the authority names it on the approval, next to the folder name. Two sessions in
  one directory can no longer be mistaken for each other when the Sessions page was never
  opened, or does not list the session (it lists one page of 25).
- Names are for reading. Nothing is looked up, sent or decided by one, and no preference,
  approval or authority behavior changed.

### Older turns of a session

- **Load older turns.** The Agent view showed only the newest 20 turns of a session and said
  older ones were not listed, with no way to reach them. It now has **Load older turns**:
  one page of up to 20 at a time, newest first, above the ones already shown, with a
  loading state, a clear message when a stretch of history held no turn or when the start of
  the session's history is reached, and **Try again** when the authority did not answer.
  The place in the list does not move when turns are inserted above it.
- **Nothing is made up.** An old turn is shown as the authority holds it now: its state and
  reason are real, its output is almost always gone (the authority keeps a few turns' output
  in memory and none across its own restart) and says so, and what the user wrote is never
  shown, because Wyss keeps no conversation text.
- **Bounded.** The authority has no list of turns; the desktop finds them in the durable
  event stream, and finds older ones the same way, going back. A page reads at most 40
  pages of that stream and remembers nothing, so history is never loaded whole, and the
  window keeps at most 200 older turns of a session before it asks to start over. Following
  a running turn never waits for it: it has a read slot of its own, and output still held
  for an old turn is read only while no turn runs.
- **Correct.** Each page goes on exactly from where the last one stopped, so no turn is
  shown twice and none is skipped, across a restart of the desktop or of the authority. A
  position from another history, or beyond the end of this one, is refused as history that
  changed, and the view offers to reload from the newest. Every turn must be the session's
  own, and a hole in the stream fails the page instead of being read as empty.
- **Interface changes.** `TurnList` is now version 3 (adds `older`, the position older turns
  are looked for from), and the window is granted one more read, `managed_turns_older`,
  which takes a session and a position it was handed and nothing else. No control-protocol
  method was added or changed; it uses the `audit/read` the desktop already used.
- **Found and fixed on the way:** a reader who scrolled up in the Agent view was pulled back
  to the end each time new output arrived; and two desktop tests that shared a process-wide
  counter failed about half of their runs.

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
