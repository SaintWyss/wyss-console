# Changelog

## Unreleased

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
