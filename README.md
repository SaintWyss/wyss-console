# Wyss Console

A Linux desktop for local terminals and for working with Codex in managed, approved
sessions. Your terminals work on their own; managed sessions, approvals and history are
kept by a separate authority that Wyss Console installs and starts for you.

**Version 0.2.0.** A native desktop application for CachyOS with KDE Plasma, built and
run from this repository. There is no installer and no published release: no `.deb`, no
AppImage, no signed artifact. Changes: [CHANGELOG](CHANGELOG.md).

## Build and use

You need Linux with a `systemd --user` session (any desktop session has one),
[Codex](https://developers.openai.com/codex) on the `PATH` your session gives its
services, which is where a desktop session puts `~/.local/bin`, and `bubblewrap` 0.11 or
newer for Local Tools (Inference does not need it). To build: stable Rust, Node 26+,
npm 12+ and the [Tauri Linux prerequisites](https://v2.tauri.app/start/prerequisites/#linux).

1. Build the package: `npm ci && npm run desktop:package`. It writes
   `.artifacts/package/wyss-console-<version>-linux-<arch>/`.
2. Copy that whole directory to a place where it and every folder above it can be written
   by you alone (or by root), for example `~/.local/opt/wyss-console/`, and run
   `bin/wyss-console` from there. The desktop refuses an authority that someone else could
   have replaced: run from a folder with a group-writable parent (`drwxrwxr-x`, which is
   what a checkout is under a umask of 002), it says the authority could not be trusted,
   installs nothing, and only the local terminals work.
3. The authority is installed and started by itself; the top bar goes from Starting to
   Connected.

What you can do:

- **Console**: local terminals, in tabs.
- **Sessions**: create a managed session for a project folder and activate it. It starts in
  **Inference** (Codex answers, nothing runs). **Local Tools** is chosen for a session on
  purpose.
- **Agents**: give a session an objective and follow the turn.
- **Approvals**: in Local Tools every command and file change Codex proposes waits here
  until you approve or deny it, one at a time.
- **Events**: what the authority recorded, newest first.
- **Settings**: how Wyss Console looks and where it opens (font size, scrollback and cursor of
  the terminals, how event times are written, the view it opens on), the authority's state,
  the version, and a reset. Settings change convenience only: they cannot turn on Local
  Tools, skip an approval, or change what Codex may do.

Closing the window does not stop the authority: the next time you open it, the same
sessions and history are there.

### The authority

Wyss Console ensures its authority is available automatically:
there is nothing to start by hand, no terminal to keep open and no `sudo`.

```text
launch  ->  an authority answers?  yes -> connected
                                   no  -> installed where missing, started, connected
```

- **The authority is a separate process**, `wyss-control`, run by your login session's
  service manager (`systemd --user`) and not by the window. Closing the window does not
  stop it; opening it again finds the same one, with the same sessions and history.
- **If it crashes it is restarted**, a bounded number of times. If it keeps ending, the
  window says so and offers **Retry**.
- **Stopping it is respected.** An authority that was stopped is not started again
  behind your back: the window says it is not running, and Retry starts it.
- **An authority that is already running is used as it is**, whoever started it, and is
  never replaced, restarted or stopped by the window. A newer authority that came with
  an update is installed the next time none is running; the top bar says
  `update pending` until then.
- **Local terminals never depend on it.** They work while it starts, and when it cannot.

What is installed, once, for your account:

| File                | Where                                                |
| ------------------- | ---------------------------------------------------- |
| The authority       | `~/.local/share/wyss-console/authority/wyss-control` |
| Its service         | `~/.config/systemd/user/wyss-control.service`        |
| Its state (not new) | `~/.local/state/wyss-console/`                       |

Removing the first two uninstalls the authority; the state is yours and nothing removes
it. Codex has to be in the `PATH` your session gives its services, which is where a
desktop session puts `~/.local/bin`. Linux with a `systemd --user` session is the
supported platform. See [ADR 0025](docs/adr/0025-authority-lifecycle-bootstrap.md).

### Your data, upgrades and removal

| What                 | Where                                                      |
| -------------------- | ---------------------------------------------------------- |
| Preferences          | `~/.config/wyss-console/preferences.json` (private, small) |
| Sessions and history | `~/.local/state/wyss-console/` (the authority's, yours)    |
| The authority        | `~/.local/share/wyss-console/authority/wyss-control`       |
| Its service          | `~/.config/systemd/user/wyss-control.service`              |

- **Upgrading** is building the package again and running the new one. A running authority
  is never replaced: the window says `update pending`, and the new authority is installed
  the next time none runs (after you log out, or stop it). Your sessions and history carry
  over.
- **Removing the application** is deleting the package directory, which removes the desktop
  only. The authority, its unit and your data stay. To remove them too:
  `systemctl --user stop wyss-control`, delete the two files above, and, only if you want
  to lose your history, delete `~/.local/state/wyss-console/`.
- **Settings → Reset preferences** restores the defaults and nothing else.

## Development

Everything below is for working on Wyss Console itself.

```sh
npm ci
npm run desktop:dev
```

`npm run dev` runs the frontend alone; application IPC requires the desktop host.
Development and native-run commands keep Cargo/XDG caches inside ignored `.cache/`.

## Validation

```sh
npm run check
npm run check:rust
npm run test:pty
npm run scan:repo
```

Frontend commands: `format:check`, `lint`, `typecheck`, `test`, `build`.
Rust commands: `rust:fmt`, `rust:check`, `rust:test`, `rust:clippy`.
`npm run format` applies frontend/config/document formatting.

The Rust suite runs the real things, in private: the Local Tools tests need `bubblewrap`
0.11 or newer at `/usr/bin/bwrap`, and the lifecycle tests start a `systemd --user` of their
own on a temporary runtime directory and an empty home, so nothing of your session, units
or Wyss state is read or touched.

`scan:repo` scans every tracked file for credentials, as Git records it and as it is on
disk: no credential file by name and no credential-shaped content, with no exception by
path. A tracked file that is missing from the working tree fails it, so it is run on a
committed tree.

## Production build

```sh
npm run desktop:package    # the directory package: bin/, libexec/, share/
```

It builds the authority, pins the desktop to its SHA-256 (`WYSS_AUTHORITY_SHA256`) and
writes the package to `.artifacts/package/wyss-console-<version>-linux-<arch>/`:

```text
bin/wyss-console
libexec/wyss-console/wyss-control
share/wyss-console/package.json
```

The desktop finds its authority from its own real executable, never from `PATH`, and
installs and starts it only if its SHA-256 is the pinned one. That is the only package
format: the Tauri bundler is off (`bundle.active` is false), and nothing is signed or
published. See [ADR 0027](docs/adr/0027-local-product-no-distribution-packaging.md).

```sh
npm run desktop:build    # the two binaries only, in src-tauri/target/release
npm run desktop:run
```

A build that was not packaged manages no authority: it uses one that is already
running, and installs and starts nothing. That is the development workflow below.

## Interactive terminal

The application opens one local account-shell terminal automatically. **New Terminal**
creates an independent session/tab, up to 16 retained sessions (the backend also
limits PTY resources to 16). Each tab keeps its shell, screen and scrollback when hidden;
background output continues to be consumed. Type normally; keyboard input is transported
to that session's shell. The active terminal fits the window; hidden terminals retain
previous dimensions until activated.

`exit` retains the exited tab and final screen without respawning. A tab's close control
releases only its local resource; closing the final tab leaves an empty workspace.
New Terminal starts again explicitly. These are local human-terminal sessions, not
managed-agent sessions. Tabs, identities and scrollback are not persisted; restart
creates fresh shells. Tab arrows/Home/End move tab focus; Enter/Space activate.
No global terminal shortcuts are intercepted.

TERM is scoped to each child as `xterm-256color`. Scrollback is volatile (5,000 lines unless changed in Settings).
Output uses bounded byte batches acknowledged after xterm processing. Input is capped
at 64 KiB queued and 4 KiB per IPC write; an excessive paste fails visibly instead of
buffering indefinitely. Selection/paste use default WebView behavior, without added
clipboard permissions. External terminal hyperlinks are disabled.

Human terminal survival across Console process crash is not guaranteed at this stage.
Graceful application exit closes its local terminals. ADRs 0005–0007 record backend
ownership, renderer flow and local session ownership. The desktop itself runs no
agent and persists nothing; managed agents exist only behind the external authority below.

## Agent adapter contracts

Validated Rust/TypeScript v1 contracts describe agent adapters, capabilities,
version/compatibility evidence, observed managed-session state, models and safe
errors. Shared synthetic JSON fixtures prove classification and semantic round-trip
parity. They do not claim installed-runtime compatibility. See ADR 0008 and
[official runtime-surface research](docs/research/agent-runtime-surfaces-2026-10-06.md).
These contracts are the vocabulary the later layers share (the authority, ADR 0011, and the
window's managed views, ADRs 0019–0024); the adapter layer itself embeds no agent SDK.
Existing local terminal behavior is unchanged; managed-agent execution requires the
external authority.

## Codex protocol boundary

An offline Rust codec validates and normalizes a bounded subset of Codex App Server
messages using schema-derived synthetic fixtures. It includes initialization ordering,
thread/turn/item lifecycle, approvals and model catalogs. Shared common projections
are checked in Rust and TypeScript. The codec itself starts nothing; the window reaches
Codex only through the authority's managed sessions ([Managed sessions](#managed-sessions)).
See ADR 0009 for codec limits.

## Read-only Codex compatibility probe

The explicit developer harness discovers Codex, reads its version, starts a private
app-server, initializes with experimental API disabled, requests one model catalog
page and reaps its child. **This read-only probe executes no managed work.** No threads,
turns, prompts, approval responses, login or configuration changes are sent.

```sh
node scripts/tool.mjs cargo build --locked --manifest-path src-tauri/Cargo.toml --example codex_live_probe
./src-tauri/target/debug/examples/codex_live_probe
```

Build and execution are separate so the probe inherits your normal environment,
without the Cargo helper's project-local XDG overrides. It prints a safe JSON summary;
no raw stderr/protocol traffic is printed. Missing Codex is a reported unavailable
outcome (exit 0); an installed failed/incompatible probe exits 1. Normal tests do not
require or launch Codex. Linux process tests require standard coreutils.

Current local observation (2026-10-06): Codex 0.159.0 completed the read-only probe,
including legal interleaved notification metadata and a seven-model catalog page
with no next cursor. Compatibility is confirmed only for the exercised protocol
surface; listing is not entitlement. The owned app-server exited cleanly and was
reaped. See ADRs 0009–0010 and the research note. Managed execution still requires
external authority.

## External control authority

`wyss-control` independently owns prepared/closed managed-session identities,
canonical control state, durable intent receipts and a minimal EventEnvelope audit.
**Prepared is not execution permission.** Explicit, leased managed turns are
available from the window and through the authority developer client:
inference-only by default, and with authority-mediated commands and file changes
in the isolated local-tools profile.
Human terminal tabs remain Console-local; the read-only Codex probe above remains
a separate developer tool. A packaged desktop installs the authority and asks the
user's service manager to start it ([Using Wyss Console](#using-wyss-console)); it never
runs it as its own child. When one is running the desktop connects to it, follows it
([Desktop authority link](#desktop-authority-link)) and sends it its intents
([Managed sessions](#managed-sessions)).

For development, the authority is started by hand. A development build of the desktop
uses it and installs nothing. Build the independent binary and developer client:

```sh
node scripts/tool.mjs cargo build --locked --manifest-path src-tauri/Cargo.toml --bin wyss-control --example control_client
```

Start independently, with a valid private XDG runtime directory (it exits with status 2
when one already runs: `already_running`):

```sh
./src-tauri/target/debug/wyss-control
```

From another terminal:

```sh
./src-tauri/target/debug/examples/control_client status
./src-tauri/target/debug/examples/control_client prepare
./src-tauri/target/debug/examples/control_client list
./src-tauri/target/debug/examples/control_client audit
./src-tauri/target/debug/examples/control_client audit 0 2
./src-tauri/target/debug/examples/control_client audit/watch 12 50 30000
```

The client also supports `hello`, `get UUID`, `close UUID revision`, and
`prepare intent-UUID` for durable retry. Close commits session/lease control state and cancels any active managed turn.
Prepared sessions create no Codex thread/turn. Stop authority using Ctrl+C or SIGTERM;
a normal control client cannot shut it down.

Socket: `$XDG_RUNTIME_DIR/wyss-console/control.sock` (0700 directory, 0600 socket).
Missing/unsafe runtime directories fail closed. State: `$XDG_STATE_HOME/wyss-console/control.sqlite3`,
fallback `$HOME/.local/state/wyss-console/control.sqlite3`. Product state is private.
SQLite WAL/FULL commits state, audit and intent receipts atomically. Do not delete
WAL/SHM files or change/copy an active canonical database casually. Next to the
database, `control.anchor` witnesses which stream the directory holds and the head it
has reached. A database that is missing or empty is created only where nothing shows
an earlier one (no witness, no WAL, SHM or journal), and a store of another stream or
behind its witnessed head is refused: a lost database, a lost or damaged WAL and an
older copy fail closed instead of opening as empty or earlier state. A refused store
is left exactly as found. Removing the witness is the explicit operator act that
accepts whatever database is there. Client EOF
changes no state. Authority restart preserves committed state/stream/sequence.
Same-UID SO_PEERCRED is a local boundary, not protection from compromised same-user
processes. Maximum eight connected clients, of which at most four may be waiting in
`audit/watch` (a running desktop is one of each); idle connections expire after five seconds. Session/audit pages are
bounded to fifty entries, and an audit page also to the 64 KiB frame. See ADR 0011 and
[Durable event replay](#durable-event-replay).

`npm run rust:test` includes hermetic authority unit tests and actual process tests
using isolated temporary XDG directories. They do not require or launch Codex.
No production state is created by the test suite.

## Desktop authority link

The desktop owns one application-lifetime, read-only link to `wyss-control`, through the
same typed `ControlClient` the developer client uses:

```text
Svelte  ->  Tauri desktop  ->  shared ControlClient  ->  Unix socket  ->  wyss-control
```

`wyss-control` stays the only authority. The desktop makes no authorization or security
decision, never opens the authority's database and, through this link, never starts,
stops or configures the authority or its runtime. (Two separate, narrower doors exist: a
packaged desktop installs the authority and asks the user's service manager to start it,
ADR 0025; and activating a session asks the authority to start a runtime, ADR 0021.) The requests this link sends are `authority/hello`,
`authority/status`, `runtime/status`, `audit/read` and `audit/watch`. Session requests
go over separate short-lived connections ([Managed sessions](#managed-sessions)).

The window cannot speak the control protocol. Its one command for this link,
`authority_state`, returns the desktop's own projection (AuthorityView v1): the link
state, why the last attempt ended, the authority's version, session counts and durable
head, and the managed runtime's desired and actual state, version and model count.
No instance, stream or session identity, runtime diagnostic or workspace path reaches
the window. The top bar renders it as two statuses next to the local terminal's:

| Status    | Shows                                                                                                                                                                                                                                                  |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Terminal  | The local human terminal. It never depends on the authority.                                                                                                                                                                                           |
| Codex     | The managed runtime as the authority reports it; `Agent · Unknown` while the authority is not connected.                                                                                                                                               |
| Authority | `Connected`, or why not: `Offline` (nothing is listening), `Reconnecting` (it stopped answering), `Incompatible` (another protocol or storage version), `Rejected` (it broke the protocol), `Unsafe` (the runtime directory or socket is not private). |

The authority pill's tooltip carries the session summary (prepared and closed counts)
and the event head; the count is also shown inline in windows wider than 1040 px.

Try it with the authority and developer client built [above](#external-control-authority):

```sh
npm run desktop:run                                    # Authority: Offline
./src-tauri/target/debug/wyss-control                  # another terminal: Connected
./src-tauri/target/debug/examples/control_client prepare   # the session count moves
```

Behavior worth knowing:

- An absent authority is retried with a delay that doubles from 250 ms to 5 s, so a
  started authority is found within five seconds. A lost one is noticed at once.
- A restarted authority is a new instance of the same stream: the desktop reads its
  canonical state again and replays the events it missed with `audit/read` (up to 2000;
  a longer gap moves the cursor to the head, which the snapshot already describes).
- What is shown is always a canonical reading, never something derived from an event.
  Events only say when to read again; a change that writes no event is picked up within
  twenty seconds, one second while the runtime is starting or stopping.
- A connected desktop holds one of the authority's eight client slots and one of its
  four `audit/watch` slots. If the watch slots are full it reads once a second instead.
- Nothing shown is stale: when the link is not connected, authority and agent facts
  are cleared instead of kept.
- Closing the desktop closes its connection and tells the authority nothing.

Local terminals are independent of all of it: typing, paste, output, tabs, resize,
scrollback and Ctrl+C work the same with the authority absent, starting, restarting,
killed, incompatible or malformed. See
[ADR 0019](docs/adr/0019-desktop-authority-link.md).

## Managed sessions

The `Sessions` view shows the managed sessions the authority holds, newest first, and
sends six product intents. The desktop requests; `wyss-control` decides.

```text
Svelte  ->  narrow Tauri commands  ->  SessionService  ->  shared ControlClient  ->  wyss-control
```

| In the window                             | What the desktop asks the authority                                                                                                                                                                                                                                                                          |
| ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| The list                                  | `managedSession/list`, then `managedSession/runtime/status` for each session shown. Twenty-five to a page.                                                                                                                                                                                                   |
| **New session**, **Activate**             | One step at a time until the session is ready in the mode chosen: `managedSession/prepare` for a new one, `runtime/stop` if the runtime serves another mode or folder and no turn is running, `runtime/start` if no runtime is on its way, `managedSession/runtime/create`, `managedSession/runtime/resume`. |
| **Enable Local Tools**, **Use Inference** | The same steps for a session that is already bound, after one confirmation, for the workspace the authority holds for it.                                                                                                                                                                                    |
| **Resume**                                | `managedSession/runtime/resume`: attaches a session that is already bound to a workspace to the running runtime.                                                                                                                                                                                             |
| **Discard attempt**                       | `managedSession/runtime/abandon`, after one confirmation, for a binding attempt the authority could not confirm.                                                                                                                                                                                             |
| **Close**                                 | `managedSession/close`, after one confirmation. Closing is final and revokes the session's lease.                                                                                                                                                                                                            |

Each session shows its control state, agent, binding state, mode and workspace, revision,
times and identity. The Codex thread, runtime instance, lease and operation identities
never reach the window, and the window cannot name a control method, an adapter, a
scope, a lease, a lifetime or a safety profile: it sends a session it was shown, the
revision it saw and the identity of its own action. The only path it can send is a
folder the user selected in the native dialog.

### Creating and activating a session

```text
New session  ->  Mode (Inference unless chosen otherwise)  ->  Choose folder (native dialog)  ->  Create & activate
Preparing session…  ->  Starting Codex…  ->  Creating workspace binding…  ->  Attaching…  ->  Ready
```

- **The folder comes from a native dialog.** `Choose folder` opens GTK's folder
  chooser. The window has no filesystem access: it cannot list, read or write, and an
  activation is accepted only for a folder that dialog returned in this run. Selecting
  a folder is a request; whether it may be a workspace is the authority's to say.
- **The authority starts Codex.** If no runtime is running or starting, the desktop
  asks `wyss-control` to start it in the mode that was chosen (the default profile for
  Inference) and waits for the authority to report it ready. The desktop never launches Codex and never asks while a runtime
  is already starting. A runtime the authority contained for safety is not restarted
  from the window.
- **The binding takes a lease for that alone.** The authority admits a binding under a
  lease that grants `session_create`. The desktop asks; if the authority says a lease
  is what is missing, the desktop asks it to grant one for that single scope and
  fifteen seconds, asks again, and asks it to revoke that lease. The attachment that
  follows does the same with `session_resume` and thirty seconds. A lease another
  client holds is used as it is and never touched.
- **It is not a transaction.** Each step can succeed and the next one fail, and the
  window shows what the authority holds: no workspace, binding, detached, ready or
  uncertain. `Ready` is shown when a read says so. An existing session with no
  workspace is activated the same way from its details; one that is bound and detached
  is resumed.
- **Uncertain is not failed.** If the authority cannot confirm whether Codex created the
  binding, the session is shown as uncertain and nothing asks again: not a retry, not a
  new click, not a restart. `Discard attempt` gives up on that attempt explicitly; a
  thread Codex may have created for it is left behind. There is no exactly-once
  guarantee for a binding, and none is claimed.

Behavior worth knowing:

- **One action, one identity.** A click names its action with one identity. Every
  authority mutation the action leads to has an identity of its own derived from it, so
  a step sent again is the same intent with the same payload. A second click while it
  is pending sends nothing. If the authority does not answer, the outcome is unknown
  and Retry continues the same action: a session is never created, bound or closed
  twice by it.
- **Resume is gated** the same way, under `session_resume`. The window does not offer
  Resume while the runtime is not ready.
- **What is shown is what was read.** The list is re-read when the authority's durable
  head moves, using the same observation the top bar follows: no second watcher exists,
  and it is also what ends an activation's wait. Nothing is shown optimistically, and
  when the authority is not connected the sessions are cleared, every action is
  disabled and the view says why.
- **Closing the window during an activation** sends nothing more. What the authority
  holds is what the next launch shows, and a lease the desktop could not give back
  expires by itself within seconds.
- **The Console keeps running.** Both views stay mounted: going to Sessions and back
  ends no terminal and starts none, and an activation goes on whichever view is shown.
- The list is walked forward, because the protocol pages oldest first. A page costs one
  request per fifty sessions held, and past two thousand sessions the newest are not
  reachable from the window.

Try it with the authority and developer client built [above](#external-control-authority):

```sh
./src-tauri/target/debug/wyss-control      # one terminal
npm run desktop:run                        # Sessions -> New session -> Choose folder -> Create & activate
./src-tauri/target/debug/examples/control_client list
./src-tauri/target/debug/examples/control_client runtime/status
```

See [ADR 0020](docs/adr/0020-managed-sessions-ui.md) and
[ADR 0021](docs/adr/0021-session-activation.md).

## Managed turns in the window

The `Agents` view is where a ready session is given something to do. A session is
opened in it from its details in `Sessions` (**Open agent**); up to eight stay open as
tabs, and the session every turn on screen belongs to is always named above them.

```text
Svelte  ->  four narrow Tauri commands  ->  TurnService  ->  ControlClient  ->  wyss-control  ->  managed Codex
```

| In the window          | What the desktop asks the authority                                                                                                                                                              |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| The turns of a session | `audit/read` for the turns the durable stream names, then `managedSession/turn/status` for each of the last twenty.                                                                              |
| **Run** (Enter)        | `managedSession/turn/start` with the session, the revision that was shown and the text. If the gate names a lease as what is missing: `executionLease/grant` for `turn_start` alone, then again. |
| The answer             | `managedSession/turn/events` from the cursor the last reading returned, `managedSession/turn/status`, `managedSession/turn/approval/pending`.                                                    |
| **Cancel turn**        | `managedSession/turn/cancel` at the revision that was shown.                                                                                                                                     |

- **Each submission is one managed turn**, with one identity made when it is submitted.
  A start whose answer was lost is retried with that same identity, never replaced.
- **The lease of a turn is kept while it runs** and given back by the reading that finds
  it ended. It is `turn_start` for thirty minutes, which is the turn's time budget if
  the desktop is not there to give it back; for a session attached in Local Tools it
  also carries `command_execute` and `file_change`, without which the authority accepts
  no approval. A lease somebody else holds is used as it is.
- **What is shown is what the authority has.** The state is one of its own: Starting,
  Running, Completed, Interrupted, Failed, Indeterminate, with the reason it recorded.
  A cancel shows "Cancelling…" until a reading says what became of the turn.
- **Only the answer crosses as text.** Reasoning is shown as "Thinking…" and nothing
  more; a command or a file change is shown as one line saying that it happened, and
  the output of a command does not cross. The provider's thread and turn, the lease and
  the runtime instance never reach the window.
- **Text is shown as text.** Nothing Codex or the user wrote is interpreted as HTML or
  Markdown.
- **A turn waiting on an approval shows it under that turn**, with **Approve** and
  **Deny** (see [below](#local-tools-and-approvals-in-the-window)). The turn stays
  `Running`, because that is what it is.
- **Wyss keeps no conversation text**, so the input of a turn is shown only by the
  window that sent it, while it is open, and output is shown while the authority still
  holds it (the last eight turns, in memory).
- **One reading at a time.** The turn that runs goes on being read whichever view is
  shown; nothing is read while no turn runs and nothing changed at the authority.
- **After a restart** of the desktop the running turn is found and followed again, and
  nothing is sent to start it. After a restart of the authority the turn is
  `indeterminate` and the session has to be resumed.

The header of the view names the mode the session is attached in: `Codex · Inference`,
where Codex is given no tools, or `Codex · Local Tools`.

```sh
./src-tauri/target/debug/wyss-control      # one terminal
npm run desktop:run                        # Sessions -> a Ready session -> Open agent -> write -> Enter
./src-tauri/target/debug/examples/control_client audit        # the lease and the turn's lifecycle, as recorded
```

See [ADR 0022](docs/adr/0022-managed-turns-ui.md).

## Local Tools and approvals in the window

A session is created in one of two modes. **Inference** is preselected: text only.
**Local Tools** lets Codex propose commands and file changes in the session's folder;
nothing it proposes runs until it was approved.

```text
New session  ->  Local Tools  ->  Choose folder  ->  Create & activate
a turn  ->  Codex proposes a command or a file change  ->  wyss-control holds it as a pending approval
the window shows it, under the turn and in Approvals  ->  Approve or Deny  ->  the same turn goes on, or ends
```

| In the window                           | What the desktop asks the authority                                                                                                                          |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Mode **Local Tools**                    | `runtime/start` with the local-tools profile for that folder, after `runtime/stop` when the runtime serves another mode or folder. The window names neither. |
| The approvals                           | `audit/read` for the approvals the durable stream names, then `managedSession/turn/approval/status` for each of the last eight.                              |
| A change longer than a page, more files | `managedSession/turn/approval/status` for one file and one page of its diff.                                                                                 |
| **Approve**, **Deny**                   | `managedSession/turn/approval/respond` with `accept` or `decline`, at the revision that was shown.                                                           |

**One runtime, one mode.** `wyss-control` supervises a single Codex runtime, and a
generation of it runs in one mode: Inference, or Local Tools for exactly one folder.
So:

- **Entering a mode the runtime is not in restarts the runtime**, and every session
  attached to it is detached, whichever session asked. The window says so before it is
  asked for, names its action for it (**Restart Codex & activate**) and, for an existing
  session, asks for one confirmation.
- **It is never asked while a managed turn is running**: the action is refused as "A turn
  is running". `wyss-control` enforces it: a `runtime/stop` while a managed turn is
  active is answered `active_turn_exists` and changes nothing, whichever client sends
  it. The window also reads first, so as not to ask.
- **A session keeps the mode it was created in.** Codex keeps a conversation in the
  home of the profile that created it and refuses to resume it under the other one. An
  Inference session cannot be given tools: Local Tools needs a session of its own.
- **A detached session shows no mode**, because the authority reports one only while a
  session is attached. After `wyss-control` restarts the runtime is back in Inference
  and a Local Tools session is entered again with **Enable Local Tools**.

**Approvals.**

- **What is shown is what the authority holds**: the whole command and its directory, or
  the files of a change set with their diffs, as plain text. Nothing in the desktop runs,
  parses or completes a command. A diff longer than a page says how much of it is on
  screen, and **Approve is offered once every file was read to its end**.
- **Approve and Deny are two commands**, each for one approval at the revision that was
  shown. There is no "always allow", no trusted folder, no rule and no policy editing.
- **Approve is a request.** `wyss-control` checks the approval, the lease of its turn,
  the action and its confinement again before Codex is answered. The same turn goes on;
  no turn is started again.
- **Deny refuses that one action.** What becomes of the turn is read from the authority:
  a denied command ends it `Interrupted`, a denied file change lets it go on.
- **One decision, one identity.** A second click sends nothing, a lost answer is retried
  with the same decision, and Approve never becomes Deny by itself.
- **An approval waits five minutes at most.** When its deadline passes the window reads
  what became of it; an approval that ended has no buttons.
- **Nothing is read on a connection that is gone.** Offline, no approval is shown as
  pending, and nothing is decided on reconnection. After a restart of `wyss-control` a
  pending approval comes back as `No longer applies`.

```sh
./src-tauri/target/debug/wyss-control      # one terminal
npm run desktop:run                        # Sessions -> New session -> Local Tools -> Choose folder -> Create & activate
                                           # Open agent -> write -> Enter -> Approve or Deny
./src-tauri/target/debug/examples/control_client audit        # the lease, the turn and the approval, as recorded
```

See [ADR 0023](docs/adr/0023-local-tools-and-approvals-ui.md).

## Events and history in the window

**Events** shows what `wyss-control` recorded: every session, runtime, turn, approval
and lease change it committed, newest first.

```text
Events  ->  the newest fifty events  ->  Load older, fifty at a time, down to the first event
a row   ->  what the record says  ->  Open session · Open in Agents · Check what an approval is now
```

| In the window        | What the desktop asks the authority                                                               |
| -------------------- | ------------------------------------------------------------------------------------------------- |
| Opening Events       | The stream and its head from the greeting, then `audit/read` for the fifty events below the head. |
| **Load older**       | `audit/read` for the fifty events before the oldest one shown, in the stream it was read from.    |
| A new event arriving | Nothing by itself: the head the top bar already follows moved, and the view reads again.          |
| Filters              | Nothing. They are applied to the events that are loaded.                                          |

- **A record is evidence, not state.** A row saying an approval was requested does not
  say it is pending. What a session, a turn or an approval is now is read where it lives:
  a row's details link there, and for an approval ask the authority what it is now.
- **A row is a closed projection.** Its position, time, kind, the Wyss identities of
  its session, turn and approval, and a few recorded facts (why a turn ended, how the
  runtime exited, what a lease allowed). The event as the authority stores it never
  reaches the window: no payload, no provider thread or turn, no lease identity, no
  action fingerprint, no runtime instance. Events hold no prompt, command, diff or
  output, and none is shown.
- **Nothing here changes anything.** Events has no Approve, Deny, Retry, Cancel, Start,
  Stop or Replay, and no export.
- **It reads only while it is shown**, once for each change of the durable head. At the
  newest end new events are added; anywhere else the view says **New events available**
  and leaves the reader where they are.
- **Bounded.** Fifty events a read, 400 kept; further back, the newest are let go.
- **What is not there is said.** _Older events exist and are not loaded_, _This is the
  beginning of the event stream_, or _History before this point is unavailable_.
- **A restart of `wyss-control` keeps its stream**: history continues and the view says
  the authority restarted. Another state store on the same socket is another stream, and
  is shown below an explicit boundary, never as a continuation; a record of the replaced
  one offers nothing to open, because what it is about lived in that stream's authority.
- **Offline, loaded rows stay** and are said to be from before; nothing is read.
- **Refusals are not events.** `wyss-control` records what it committed. A refused
  request (a stop under a running turn, a stale revision) is answered and not recorded,
  so it does not appear here.

```sh
./src-tauri/target/debug/wyss-control      # one terminal
npm run desktop:run                        # Events
./src-tauri/target/debug/examples/control_client audit        # the same stream, as the authority stores it
```

See [ADR 0024](docs/adr/0024-events-history-ui.md).

## Agent configuration in the window

The Configuration view shows what Claude Code and Codex are set up with, read from their
own files: hooks, skills, MCP servers, rule files and model settings, each with where it was
found, what the file says about it, what cannot be confirmed without running the provider,
and why nothing can be changed (everything is read-only in this phase). A second list says
what became of every place that was looked at, with a closed reason when it could not be
read, and never a path.

- **What is read.** `~/.claude` (or `CLAUDE_CONFIG_DIR`), `~/.claude.json` (only the MCP
  servers in it), `~/.codex` (or `CODEX_HOME`), `/etc/claude-code` and `/etc/codex`. Nothing
  else, and never a credential file such as `auth.json` or `.credentials.json`. A project's
  own files are read for a selected project, and Codex's only if the user trusts it.
- **What is shown.** Commands, flags and names stay; environment values, headers, tokens and
  anything named like a secret are replaced before anything leaves the backend. Rule files
  are shown by name and fingerprint, never their text.
- **How it stays current.** The backend watches the directories and rescans after an edit
  settles, within two seconds; the window asks only whether the revision changed, and only
  while the view is shown. A rescan every 30 seconds covers what no watch can see.
- **Where the code is.** `src-tauri/src/agentconfig/` (reader, adapters, redaction, service)
  and `src-tauri/src/configuration.rs` (the two commands) are in the desktop binary, not in
  the library the authority links; `tests/authority_boundary.rs` searches the built
  authority binary to prove it. The contract with the window is
  `tests/fixtures/configuration-inventory-v1.json`, read by both sides' tests.
- **Trying it on your own machine.** `cargo test -- --ignored real_machine --nocapture`
  prints counts and outcomes of your real configuration, never a value, and writes nothing.

## Terminals that outlive the window (the host)

The first window starts a **host** (`wyss-console --wyss-host`, a separate process) that owns
every terminal and native agent session; windows are its clients over
`$XDG_RUNTIME_DIR/wyss-console/host.sock` (private: 0700 directory, 0600 socket, peer uid
checked). Close the window and everything keeps running; open one and the tabs come back with
their scrollback. A host with nothing to hold exits after ten minutes. `wyss-console
--wyss-host-stop` stops it (closing its terminals). Without a reachable host the window runs one
of its own and says that terminals then end with it. Decision record:
`docs/adr/0031-host-process-for-terminals-and-native-sessions.md`.

## Agents in the window (native sessions and profiles)

The Workspace view starts the installed Claude Code or Codex in a terminal tab of the Console.
They are **native** sessions: your own agents in your own shell, not mediated, contained or
approved by the Console (the managed sessions of the Sessions view are another plane).

- **How it starts.** The terminal runs your interactive shell with a command that replaces it
  by the agent (`exec`), so your start-up files run and the process the terminal owns is the
  agent. Recipes exist for `sh`, `bash`, `zsh` and `fish`; any other shell starts the agent
  directly and the session says so. The window names an agent of a closed list, a project
  folder (home, one chosen in the native dialog, or a known project) and a profile; it never
  sends a command, arguments, an environment or a path.
- **What it keeps.** `~/.local/state/wyss-console/native/` holds `registry.json` (metadata of
  the sessions and projects; no process is kept) and `profiles/`. After the window closes,
  what was running is shown as **Lost**: the Console did not see it end and its terminal went
  with the window.
- **Profiles.** A profile is applied when a session starts and changes no global file. For
  Claude Code it becomes flags and a generated `--settings` overlay under
  `native/sessions/<id>/` (removed when the session ends). For Codex it becomes flags
  (`-m`, `-s`, `-a`). What the agent cannot do for one session (Codex hooks and skills) refuses
  to start instead of being ignored.
- **Code.** `src-tauri/src/native/` (desktop binary only) and `src/lib/features/native/`.
  Decision record: `docs/adr/0030-native-launcher-registry-and-session-profiles.md`.

## Changing Claude Code's configuration

From the Configuration view the user can turn a Claude Code hook off and back on, turn every
hook off, and turn a skill off and on, in the real settings file, with a way back. The full
account is [docs/spec/configuration-changes.md](docs/spec/configuration-changes.md); in short:

- **A plan first.** Choosing a change shows what would change, how the entry would be kept,
  and what "applied" does and does not mean. Nothing is written until it is confirmed.
- **A native window for anything risky.** Turning off a hook that can block an action, or
  putting a hook back, asks in a window the page cannot draw on or answer for.
- **Only the change's bytes change.** The rest of the file is the same bytes. A file that
  changed since the plan is not written. Putting a hook back into an untouched file gives
  the original file back, byte for byte.
- **A hook that was turned off is kept encrypted.** It may hold a secret, so it is sealed
  with authenticated encryption under a key in the system's Secret Service (ADR 0029). If
  that is unavailable, an entry that may hold a secret is not turned off one by one, and the
  app says why. Losing the key means the kept hooks cannot be put back from here.
- **History and undo.** Every change is journaled without its content; an undo is a plan
  like any other. A hook can be exported under a passphrase and imported on another computer.
- **Interrupted writes are repaired at start**, and every step of an apply is tested with an
  injected failure and an injected crash.

## Authority-owned Codex runtime

`wyss-control` can supervise one shared, long-lived `codex app-server`, independently
of Console clients. Durable desired running/stopped is separate from observed
starting/initializing/ready/stopping/failed. Schema v1–v6 upgrades transactionally to
v7, preserving sessions, leases, bindings, operation journal, turns, approvals, receipts,
stream, audit and runtime desired state. Migration creates no runtime desired state,
managed turns or managed approvals; startup reconciles an existing desired-running record. An
older build refuses a newer schema; there is no downgrade. Durable
conversation bindings use CodexThreadId; runtime attachment remains transient.

The developer client exposes the new normalized control methods:

```sh
./src-tauri/target/debug/examples/control_client runtime/status
./src-tauri/target/debug/examples/control_client runtime/start
./src-tauri/target/debug/examples/control_client runtime/model/list
./src-tauri/target/debug/examples/control_client runtime/stop
```

Start/stop also accept an intent UUID for durable retries. Each new intent commits
its desired state/revision/audit before reconciliation; retry returns the original
receipt. Status observes readiness asynchronously. The Rust client supports catalog
pages of at most twenty descriptors with instance-scoped cursors. The catalog is
bounded to one fifty-model page, transient and refreshed for each new process;
listing is not provider entitlement. A rejected catalog fails safely without auth
remediation. No raw Codex frames/stderr/auth/environment are exposed or persisted.

The shared 2C handshake sends only initialize, initialized and model/list. Stdout
continues to be validated/drained after readiness and stderr remains bounded.
The inference profile answers no server request; protocol failure reaps the child. Linux
parent-death SIGKILL plus parent-identity checking prevents an orphan App Server
after authority crash. Graceful authority shutdown reaps the child but preserves
desired running; restart obtains a fresh AgentRuntimeInstanceId and catalog.
Explicit runtime/stop persists stopped and suppresses startup reconciliation. It is
refused with `active_turn_exists` while a managed turn is active: a turn is cancelled
before the runtime it runs on is stopped. A runtime mutation sent again under an intent
that already committed is answered with its receipt and has no effect: nothing is
detached, stopped or launched for it.
Unexpected runtime failure does not create an automatic restart loop; a new start
intent admits one recovery attempt.

**Runtime process readiness is not execution authorization.** Runtime supervision
creates no Codex threads or turns, submits no prompts, and answers no approvals or
permission requests.
The desktop does not launch or supervise a runtime: since ADR 0021 it can ask the
authority to start one as part of activating a session, and nothing else. Human terminal
behavior and the standalone read-only probe remain unchanged. See ADR 0012.

Rust tests include seeded v1 migration, idempotency/audit rollback, deterministic
fake App Server IO/failure/allowlist checks, graceful restart, explicit stop and
Linux SIGKILL containment. Normal tests use isolated XDG directories and a fake
executable, never the user's Codex runtime or production authority state.

### Isolated local-tools runtime profile

A runtime generation has exactly one safety profile for its whole life. The default
`codex_inference_v1` offers the model no tool. `codex_local_tools_v1` is requested
explicitly and names the one workspace it serves:

```sh
./src-tauri/target/debug/examples/control_client runtime/start/local-tools /abs/workspace
```

Changing profile or workspace needs `runtime/stop` first; a start that would change
either on a live generation is refused before anything durable changes. The profile
is not durable state: after an authority restart a desired-running runtime comes
back as inference, the least capable profile.

The local-tools generation runs inside an unprivileged `bwrap` namespace whose argv
is fixed by the authority and built without a shell or any client text:

- `/etc/codex` is an authority-owned policy whose managed requirement
  `unified_exec = false` removes the persistent command tool and `write_stdin`.
  Passing that flag as plain configuration is not enough: the runtime overrides it.
  It is mounted on a private overlay of `/etc`, so no such directory has to exist on
  the host and nothing of a host `/etc/codex` is visible.
- `CODEX_HOME` is a private authority directory. The user's Codex home, and
  `~/.codex`, are hidden behind an empty tmpfs: no user configuration, rules, hooks or
  MCP servers. Only the pinned release directory returns, read-only.
- The authority's own control plane does not exist inside the namespace: its runtime
  and state directories (control socket, locks, canonical database) are hidden. The
  generation gets back its private home and, read-only, its one model catalog file.
- The process starts in an empty directory that exists only in the namespace: a sealed
  private mount at the authority's `runtime-cwd` path, which the launcher is told to
  change to. It is not the host directory, the workspace or the user's home. A thread
  is told its workspace explicitly.
- One credential capability crosses: the user's `auth.json`, bound read-write
  because Codex 0.159.0 refreshes its token in place and has no other
  non-interactive way to share one login. The authority opens it without following a
  link and without read access, requires a private regular file of this user with a
  single name in a home nobody else can write, and hands the launcher that open
  descriptor: what enters the namespace is the inode that was validated. It is never
  read, copied or logged. Before every conversation, turn and Accept the identity is
  checked again; a replaced, widened, re-linked or removed credential ends the
  generation. A token refresh by the isolated runtime writes the user's own file.
- Every existing project policy layer (`.codex`) from the outermost enclosing
  repository down to the workspace is masked read-only, and every thread forces the
  workspace and every directory above it that holds a `.git` marker `untrusted`. The
  authority creates nothing in the workspace.
- The model catalog is the authority's normalized presentation, with direct tools
  and without code mode, sub-agents or experimental tools; it keeps the patch tool.

Readiness is reported only after the runtime itself proves the profile: its private
home in the initialize answer, the managed requirement through
`configRequirements/read`, every reflected clamp with only the command tool
enabled, no MCP server, no hook, and workspace-only writes (`/tmp` and `$TMPDIR`
excluded, no network). Every thread must come back with approval policy
`untrusted` and that sandbox. Anything unproven is `safety_unproven`, and the same
attestation runs again before each conversation and turn; project policy that
appears in an unmasked layer afterwards stops further work. The host must provide
`/usr/bin/bwrap` with unprivileged overlay support (bubblewrap 0.11 or newer on a
kernel that allows overlayfs in a user namespace). Without it the profile is
unavailable; there is no unisolated fallback.

A session attaches as `local_tools_v1` only for the generation's workspace, and
the `command_execute` and `file_change` gate actions are denied under any other
profile. In this profile the authority mediates the provider's command and
file-change approval requests, as described under
[Tool and approval mediation](#tool-and-approval-mediation); nothing runs without
its Accept. A session bound under one profile keeps its Codex thread in that
profile's home.

## Execution leases and internal gate

Execution is denied by default. A prepared ManagedSession has no permission until
an explicit `executionLease/grant` commits a durable lease and audit. Runtime
startup, client reconnection and authority restart never grant or renew a lease.
Lease operations themselves create **zero Codex threads or turns**, submit no
prompts, and answer no approvals or permission requests. A grant alone never executes work.

Schema v3 adds only execution leases and a unique active-lease index; migration
preserves sessions, runtime desired state, receipts and exact audit bytes, and
creates zero leases. Each lease has its own ExecutionLeaseId and revision, explicit
nonempty scopes (`session_create`, `session_resume`, `turn_start`,
`command_execute`, `file_change`), and an authority-time TTL of 1–3600 seconds.
No scope implies another scope.

Schema v6 rebuilds `execution_leases` so its stored scope encoding is bounded by the
exact canonical maximum (80 characters, all five scopes) and adds
`managed_approvals`: control metadata of one command or file-change approval. A row
holds identities, the admitted lease, state, revision, decision, external-effect
marker, deadlines, an authority-computed SHA-256 action fingerprint and bounded
counts. It never holds a command, output, path, diff or file content. At most one
approval is pending, a provider item is mediated at most once, and a schema trigger
refuses any change that is not a legal single-revision transition; only `pending` is
nonterminal. Startup invalidates an undecided approval and marks an accepted one
without a confirmed outcome `indeterminate`; no provider response is ever replayed.

The typed Rust client and developer client support `executionLease/grant`,
`executionLease/get`, `executionLease/active`, `executionLease/renew` and
`executionLease/revoke`. Grant takes session ID, expected session revision, TTL,
comma-separated scopes and intent UUID. Renew takes lease ID, expected lease
revision, TTL and intent UUID; revoke takes lease ID, revision and intent UUID.
Get/active take the corresponding lease/session ID. Renewal preserves scopes and
uses authority NOW + TTL. Original intent retries return their original receipt,
including the original expiry; UUID case and scope order are normalized.

`executionGate/status` takes session ID and one scope, returning an informational
reason only. It is not an authorization token. Execution reevaluates
the internal gate immediately before the authority-owned side effect. It requires
prepared session, active unexpired scoped lease, matching runtime metadata,
desired running, actual ready, live runtime identity, compatible runtime and
healthy canonical storage. A valid lease with stopped runtime still denies;
runtime readiness without a lease also denies.

The single authority writer expires overdue leases at startup and through a
deadline-driven scheduler. Get returns effective state without writing or inventing
a revision; active never returns an overdue lease. Gates compare UTC time even if
expiry persistence is delayed. Storage/audit failure latches execution closed.
Closing a session atomically expires/revokes its lease before closing the session.
Runtime stop leaves leases unchanged. Graceful/hard authority restart preserves
unexpired lease identity, revision and exact deadline, without extending it.
See [ADR 0013](docs/adr/0013-execution-gates-leases-and-revocation.md).

## Managed conversation binding

The authority implementation adds gated conversation create/resume, schema v4,
immutable ManagedSessionId-to-CodexThreadId bindings and an external-effect journal.
Runtime attachment is transient and restart never automatically resumes a thread.
Conversation create/resume itself sends no turn or prompt. Turns require the additional
profiles and turn_start gate described below. Create and resume answer no approval or
permission request. Tauri/human terminal integration is unchanged.

Empty Codex 0.159.0 threads are materialized durably without a turn through an
internal stable start→archive→unarchive persistence barrier. Create leaves the
binding **bound_detached**. Explicit resume requires the separate `session_resume`
lease scope before attachment; authority/runtime restart never resumes implicitly.
No experimental API or history mode is enabled. The compatibility barrier, journal
ambiguity and zero-turn boundary are documented in
[ADR 0014](docs/adr/0014-managed-codex-session-binding.md).

## Inference-only managed turns

Sprint 2H enables the first authority-owned text inference through the typed Rust client
and developer harness. It requires **all three** of an attested `codex_inference_v1`
process, a current `inference_only_v1` attachment, and an allowed internal `turn_start`
execution gate. The compatible runtime is currently exactly Codex 0.159.0 with
experimentalApi=false. No client can supply runtime args, tools, permissions or turn
configuration overrides.

Each generation first runs a discovery-only App Server to read effective MCP names,
then reaps it and starts the final child with every discovered MCP server disabled
before thread creation. Fixed process and thread overrides disable external tool
surfaces. Backend model metadata can register tools independently of those overrides,
so the authority also launches the final child with its own normalized model catalog
(direct tool mode, no experimental, multi-agent or patch tools); with it the provider
receives zero model-visible tools. A catalog that cannot be proven safe keeps the
runtime from starting. config/read and hooks/list must verify the safety preconditions, including
zero enabled executable external hooks. Restoring user/project/plugin/managed hooks
or incompatible configuration makes inference unavailable; the authority never edits them.
The desktop edits Claude Code's own settings only when the user plans and confirms a change
in the Configuration view (see below), never Codex's, and never silently.
Explicit resume verifies the stored thread/workspace, readOnly sandbox and never
approval policy before publishing its transient attachment profile.

Control operations are `managedSession/turn/start`, `managedSession/turn/status`,
`managedSession/turn/events` and `managedSession/turn/cancel`. The developer start
command takes session ID, expected session revision and intent UUID, and reads the
prompt from stdin. Only one exact text input of 1–8192 UTF-8 bytes is accepted.
Wyss persists its authority-computed SHA-256 fingerprint and byte length, never the
prompt, assistant output or reasoning. Codex-owned conversation history may retain
its legitimate content. Start returns a committed starting receipt asynchronously;
status observes later running/terminal revisions. Retrying the same intent returns
its original receipt and never resends the prompt.

At most one managed turn is starting/running globally. ManagedTurnId is distinct
from CodexTurnId. Durable terminal states are completed, interrupted, failed and
indeterminate. Assistant deltas and visible reasoning summaries use a separate
memory-only stream; raw/private reasoning is suppressed. Per-turn buffers retain
at most 256 events/256 KiB, the last eight turns, with UTF-8-safe splitting and
explicit gaps. Event pages contain at most fifty events within control-frame bounds.
Authority restart preserves canonical status but loses transient output.

Cancellation remains available after lease expiry/revocation. The authority interrupts
on explicit cancel, admitted-lease expiry/revocation or session close. Renewal of the
same lease may extend execution; a replacement never transfers a running admission.
Failure to confirm interruption within the bounded deadline contains/reaps the runtime.
Client disconnect has no turn effect. Runtime/authority restart clears both profiles
and attachments, preserves durable bindings, and never replays a prompt. Nonterminal
turns recovered after a crash become indeterminate.

**In the inference profile, tool, command, file, MCP, web, subagent and
approval/permission execution is disabled.** Any observed external action or server
request fails closed and contains the runtime; no request is answered.
The standalone read-only probe, human terminals and Tauri capabilities are unchanged.
See [ADR 0015](docs/adr/0015-first-managed-turn-and-streaming.md).

## Tool and approval mediation

In a `codex_local_tools_v1` generation the model is offered exactly two tools, a
one-shot command and a patch, and neither does anything on its own. The provider
announces the action and asks; the authority turns that into one pending
`ManagedApproval`; the action runs only after the authority committed an Accept and
wrote exactly one answer. There is at most one active managed turn and one pending
approval, and no approval queue.

```sh
./src-tauri/target/debug/examples/control_client runtime/start/local-tools /abs/workspace
./src-tauri/target/debug/examples/control_client executionLease/grant SESSION 1 600 session_create,session_resume,turn_start,command_execute,file_change INTENT
./src-tauri/target/debug/examples/control_client managedTurn/approval/pending TURN
./src-tauri/target/debug/examples/control_client managedTurn/approval/status APPROVAL
./src-tauri/target/debug/examples/control_client managedTurn/approval/status APPROVAL 0 0
./src-tauri/target/debug/examples/control_client managedTurn/approval/respond APPROVAL REVISION accept INTENT
```

`pending` returns the approval a turn is waiting on. `status` returns its canonical
metadata and, only while the authority still holds the provider request, the action
itself: the complete command with its directory, or one file of the change set
(`status APPROVAL FILE_INDEX OFFSET`) with its complete diff in contiguous 8 KiB
pages. `respond` takes the approval, its expected revision, `accept`, `decline` or
`cancel`, and an intent UUID. There is no generic provider response, provider request
identity or raw provider method. The same intent returns its original receipt from any
connection and never a second provider answer; another payload under the same intent
is `intent_conflict`; a client disconnect decides nothing.

An approval exists only for a complete, bounded action that is provably inside the
session workspace:

- Command: 1–16 KiB, working directory up to 4 KiB, up to 64 parsed actions, and a
  presentation that fits one control frame. Codex wraps the model's command as
  `/usr/bin/bash -lc '<command>'`; that wrapped text is what is shown and approved.
- File change: every path, change kind, move destination and diff; up to 64 files,
  128 KiB per diff and 256 KiB in total. Nothing shown for approval is truncated. A
  request without its complete change set cannot be accepted and is refused after ten
  seconds. A persistent `grantRoot` is never honored.
- Confinement: the working directory and every source and destination must extend the
  canonical workspace component by component. No string-prefix comparison, no `..`,
  no symbolic link anywhere, no hard-linked file, no mount point, no `.codex`, `.git`
  or `.agents` component. The proof is repeated immediately before every Accept.

Anything out of bounds or not provably confined never becomes an approval: the
provider is refused and the turn stops with `action_rejected`.

Accept is an intent the authority revalidates in full: the exact runtime generation,
profile, attachment, turn, provider item, held request, fingerprint and confinement;
a fresh attestation of the isolated profile by the runtime; and, in the committing
transaction, the pending state, revision, deadline, session, turn and the ORIGINAL
lease of the turn with the `command_execute` or `file_change` scope. A replacement
lease never transfers authority. The deadline is the stricter of five minutes and the
lease expiry at the time the approval opened. Decision, dispatch marker, audit and
receipt commit before the one provider answer is written.

Decline and cancel need no execution authority. A refused command is answered with
provider `cancel`, which Codex 0.159.0 offers instead of `decline` and which ends the
turn; a declined file change is answered with `decline` and keeps its turn. Session
grants, exec-policy amendments, network-policy amendments and permission grants are
never sent.

Turn cancel, lease revocation or expiry, session close and shutdown terminalize the
approval before any provider frame, refuse the held request and stop the turn. If the
runtime is lost while an approval is pending it is `invalidated`. If an accepted
answer cannot be proven delivered, or the runtime is lost before the provider reported
the accepted item, the approval is `indeterminate` and the runtime is contained.
Nothing is resent and no action is replayed, including after an authority restart.

The per-turn transient stream additionally reports `command_proposed`,
`file_change_proposed`, `approval_required`, `approval_resolved`, `command_started`,
`file_change_started`, `command_output_delta` and the completed/failed events, under
the same bounds as assistant output. Commands, output, paths and diffs never enter
SQLite, its WAL/SHM, the audit stream or intent receipts: storage holds a SHA-256
fingerprint and bounded counts. Codex keeps its own conversation history in the
private runtime home.

Everything else stays an isolation violation in this profile too, never answered and
contained: hooks, MCP, web and network tools, plugins, apps, skills, dynamic tools,
sub-agents, collaboration, request-user-input, permission escalation, write-stdin,
terminal interaction, a second concurrent request, and any command or file effect the
authority did not accept.

Known limits: the user's `auth.json` is shared read-write with the isolated runtime,
as the one validated capability described above, and a command the user accepts can
read it exactly as it can under the user's own Codex; `bwrap` with overlay support is
required; the profile does not survive an authority restart; and compatibility is
pinned to Codex 0.159.0. See
[ADR 0016](docs/adr/0016-tool-and-approval-mediation.md) and, for what an accepted
command was measured to be able to do, the trust boundaries and the accepted residual
risks, [ADR 0018](docs/adr/0018-core-hardening-and-completion.md).

## Durable event replay

Every canonical control mutation commits one or more EventEnvelope v1 events in its own
transaction, through a single append path. The stream is audit, history and
observation. **It is not the authority**: canonical state is the control tables, nothing
is derived from an event, and reading the stream starts, grants, answers and repeats
nothing. Storage is at schema version 7, which records the event vocabulary next to
the stream.

`audit/read` replays committed history from a cursor the client owns:

```sh
./src-tauri/target/debug/examples/control_client audit                # from the beginning
./src-tauri/target/debug/examples/control_client audit 40 10          # ten events after sequence 40
./src-tauri/target/debug/examples/control_client audit/watch 52 50 30000
```

- `after_sequence` returns events with a greater sequence, ascending. `0` or no
  argument in the developer client means from the beginning.
- A page holds at most fifty events and always fits one 64 KiB frame, so it can stop
  before the requested limit. `next` is the sequence of the last event returned and is
  present only while a later committed event exists: follow it until it is absent.
- A cursor past the durable head, or another stream id, is refused. Sequences are
  decimal strings, never JSON numbers. Timestamps are not cursors.
- `audit/watch after_sequence limit wait_ms` is the same read, answered at once when
  history exists after the cursor and otherwise after the next durable commit or
  `wait_ms` (at most 30000), whichever comes first. A waiting watch takes one of four
  watcher slots; when all are taken it fails immediately with `capacity_exceeded`.

The authority keeps no subscriber state, acknowledgement or offset. After a disconnect
or an authority restart, graceful or not, a client sends the last sequence it processed
and receives everything committed after it; the stream id and the head survive, and the
next event is head + 1. A wakeup is only a hint: a woken watch reads SQLite again by
the client's cursor, so a rolled back mutation is never announced and a lost wakeup
cannot lose an event. Shutdown wakes every watcher; a watcher that disconnects changes
nothing.

A durable event is at most 2048 encoded bytes and holds control metadata only:
identifiers, enumerations, revisions, timestamps, counts and SHA-256 fingerprints. An
event that cannot be validated or appended fails its mutation, without a sequence gap.
A valid event of a type this build does not define is replayed as stored and never
interpreted, but only in a store that a build with a larger event vocabulary has
opened; in any other store such an event was written by no build and the store is
refused. On open the log is compared with the tables and a store that disagrees with
its own history is refused, never repaired.

Assistant text, reasoning summaries, command output, diffs, prompts, provider frames
and terminal bytes are not events. `managedSession/turn/events` remains a separate,
memory-only stream: after an authority restart the durable history is intact and the
transient stream of earlier turns is gone. See
[ADR 0017](docs/adr/0017-core-event-runtime-and-replay.md).
