# ADR 0025: Authority lifecycle and desktop bootstrap

Status: Accepted
Date: 2026-10-10

## Decision

Launching Wyss Console is enough. A packaged desktop makes its authority available:

```text
launch Wyss Console
  -> an authority answers?            yes -> connect to it, whoever started it
                                      no  -> install the packaged wyss-control where it is missing
                                             write its service definition where it is missing
                                             ask the user's service manager to start it
  -> AuthorityService connects        -> the normal window
```

No terminal, no developer command, no `sudo`.

`wyss-control` stays exactly what it was: a separate process that owns canonical state,
the control socket and the runtime, and decides. The desktop does not host it, does not
run it as a child and does not restart or stop it.

```text
systemd --user          owns the process: starts it, restarts it after a crash within a limit,
                        stops it when the session ends
wyss-control            owns its store, its socket, its locks and its runtime
AuthorityService        observes the connection (ADR 0019): connected is its word alone
AuthorityBootstrap      owns two files and one request, start
the window              shows where it stands, and has one intent: Retry
```

Control Protocol v1 is unchanged, and so is the store. Two things outside
`src-tauri/src/lifecycle` changed in the authority, both recorded below: the working
directory its runtime starts in, and its exit status when it will not start.

## Why the authority stays external

The product's invariant is that the console shows and asks, an external authority
decides, and managed runtimes execute. An authority inside the window's process would
die with the window, would have to be trusted with the window's whole surface, and
could not be told apart from it by anything outside. So the lifecycle was built around
the constraint instead of removing it: the authority has to survive a desktop that
crashes, exits or is replaced, and a second desktop has to find the same one.

That rules out `Command::spawn("wyss-control")` from Tauri. A child's lifetime is its
parent's unless it is detached, and a detached child that nobody supervises is an
orphan nobody restarts and nobody can account for.

## What was found

Read from the code and from a running authority before anything was built.

| Fact                | What it is                                                                                                                                                                        |
| ------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Entry point         | `wyss-control`, no arguments. It reads `XDG_RUNTIME_DIR` and `XDG_STATE_HOME` (or `~/.local/state`).                                                                              |
| Single instance     | Two exclusive locks, in the runtime and in the state directory. A second authority fails with `already_running`.                                                                  |
| Socket              | `$XDG_RUNTIME_DIR/wyss-console/control.sock`, 0600. A socket nobody listens on is removed by the authority itself, after proving it with a refused connection and the same inode. |
| Shutdown            | `SIGTERM` and `SIGINT` are handled: the runtime is stopped in order and the last commit is witnessed. Exit status 0.                                                              |
| Store               | `control.sqlite3` and its witness in `$XDG_STATE_HOME/wyss-console`. A restart continues the same stream (ADR 0024).                                                              |
| After a restart     | Desired runtime state is durable; the runtime is started again. A turn that ran is `indeterminate`, a pending approval `invalidated`.                                             |
| Compatibility       | The greeting carries the protocol version, which both sides refuse to mismatch, a service version and a storage schema version.                                                   |
| Packaging           | None. `bundle.active` is false; `desktop:build` produces two binaries in `target/release`.                                                                                        |
| Runtime's directory | **The repository root compiled into the binary** (ADR 0012: "Packaging has to replace that before distribution").                                                                 |
| Exit status         | **1 for every failure**, including the two a retry cannot change.                                                                                                                 |
| Codex               | Found in the authority's own `PATH`.                                                                                                                                              |

The last three shaped the phase. The first two of them were changed; the third is a
limit that is documented.

## Host model: a `systemd --user` service

The authority runs as one user unit, `wyss-control.service`, started on demand.

- It survives the desktop. Closing, crashing or replacing the window does nothing to it.
- One owner of restarts. `Restart=on-failure` with a start limit belongs to the service
  manager, and nothing in the desktop restarts anything on top of it.
- No root. The unit is the user's own, under their own configuration directory.
- It can be looked at with the system's own tools, and its output goes to the journal.
  No logging was built.

A root service, a Tauri-owned child, a detached child, a socket-activated unit and D-Bus
activation were considered. The first needs a privilege the product does not need. The
second and third fail the invariant above. Socket activation would move the creation of
the control socket, which is security-sensitive code of the authority's (ADR 0018), into
a unit file; the authority's own rules for a dead socket would no longer be what runs.

## Platform scope

Linux, with a `systemd --user` session. That is the supported platform of this phase
and the one it was accepted on (CachyOS, systemd 262, KDE).

- Without a user service manager the desktop says so and starts nothing. There is no
  fallback to a child process: failing clearly was preferred to weakening ownership.
- Other platforms compile the bridge as a stub that reports the lifecycle as failed and
  never changes. No lifecycle exists for Windows or macOS.

## Installation layout

Two files, both the user's, under the XDG directories:

| File               | Path                                                 | Mode |
| ------------------ | ---------------------------------------------------- | ---- |
| Managed authority  | `$XDG_DATA_HOME/wyss-console/authority/wyss-control` | 0700 |
| Service definition | `$XDG_CONFIG_HOME/systemd/user/wyss-control.service` | 0600 |

with the specification's defaults (`~/.local/share`, `~/.config`). An installer lock,
`.install.lock`, sits beside the binary. Directories that are missing are created 0700.
Nothing else is written anywhere, and nothing is written outside these two directories.

The path of the binary is fixed because the unit names it. Canonical state is somewhere
else (`$XDG_STATE_HOME/wyss-console`) and is never read, moved or deleted by installing,
repairing or updating: reinstalling the authority is not resetting Wyss.

Uninstalling is removing those two files and the lock. No uninstall flow was built.

## Provenance: which binary is installed

The desktop installs exactly one authority: the one it was built with.

- `npm run desktop:package` builds the authority, takes its SHA-256, and builds the
  desktop again with that digest compiled in (`WYSS_AUTHORITY_SHA256`). The package lays
  the two out as `bin/wyss-console` and `libexec/wyss-console/wyss-control`.
- At run time the payload is found **from the running executable** (`/proc/self/exe`,
  links resolved) and from nowhere else: not `PATH`, not the working directory, not a
  name or a path from the window.
- It is read once, through a descriptor opened without following a link, and its digest
  has to be the pinned one. Those bytes, and no other read of the file, are what is
  installed.
- The payload has to be a regular file of this user's or of root's, writable by its
  owner alone, in directories of this user's or root's that nobody else can write.

A build that was not packaged has no digest and **manages no authority**: it installs
and starts nothing and uses one that is already running. That is `desktop:build`,
`desktop:dev` and every test binary, so a development build never points the product at
`target/release/wyss-control` and never overwrites anything.

The digest identifies a build, not a version: the two binaries are one crate, and any
change to it is a new authority.

## Safe paths

The installer follows the authority's own rules (`control/paths.rs`), which were made
visible to the rest of the crate and not duplicated:

- every directory on the way is this user's or root's, is not a link, and is writable
  by nobody else;
- the managed directory is 0700;
- the managed binary is a regular file of this user's, with one name, that only its
  owner can write; the unit is the same;
- both files are judged before either is written, so a refusal leaves nothing half done.

A link where the binary goes, a second name for it, a directory others can write, a
unit directory that is a link: each is refused and left exactly as it was found. A unit
of that name that does not start with this product's marker line was written by
somebody else and is never replaced; one that does and differs is this product's from
another build, and is rewritten.

## Atomic install and update

Each file is written under a name nothing else uses, flushed, renamed over the old one,
and its directory flushed. A reader, an `exec` and a crash see the old file or the new
file. A binary that is running keeps the file it started from: a file is never written
in place.

One installer runs at a time across every desktop of the user (an advisory lock that
dies with its holder). Under the lock, temporary files an interrupted installer left
are removed, by their exact name pattern, and what is installed is read again. Nothing
is written where it is already current.

## The unit

```ini
[Unit]
Description=Wyss Control Authority
StartLimitIntervalSec=120
StartLimitBurst=5

[Service]
Type=exec
ExecStart="<the managed binary, by its whole path>"
Restart=on-failure
RestartSec=1
RestartPreventExitStatus=2
KillMode=mixed
TimeoutStopSec=30
UMask=0077
NoNewPrivileges=yes
LockPersonality=yes
RestrictRealtime=yes
SystemCallArchitectures=native
UnsetEnvironment=LD_PRELOAD LD_LIBRARY_PATH LD_AUDIT LD_DEBUG LD_DEBUG_OUTPUT LD_PROFILE LD_PROFILE_OUTPUT GCONV_PATH
```

It depends on the path of the managed binary and on nothing else, so two builds that
install to the same place write the same bytes.

- **A crash is restarted, a crash loop ends.** Five starts in two minutes, then the unit
  is failed and stays so.
- **What cannot succeed is not retried.** `wyss-control` now exits with status 2 when it
  already runs or a path it needs is unsafe, and the unit does not restart on 2. A clean
  exit is not restarted either: stopping the authority is respected.
- **`KillMode=mixed`.** Stopping signals the authority alone, which stops its runtime in
  order; what is left after thirty seconds is killed.
- **No `[Install]` section.** The unit cannot be enabled. It is started when a desktop
  needs it and then lives as long as the session, or until someone stops it. After a
  reboot or a new login, opening the desktop starts it again. Nothing runs at login for
  an application that is not open.
- **Hardening is what was tested against the isolation the authority runs Codex in.**
  `RestrictSUIDSGID` breaks `bwrap` ("Can't open source") and is absent on purpose.
  Namespace and file-system directives are absent because the authority has to reach the
  user's workspaces and create namespaces itself; they would also put it inside a user
  namespace the accepted isolation (ADRs 0016, 0018) was never validated under.
- **Environment.** The unit's environment is the service manager's, the same every user
  service of the session has. The desktop passes nothing of its own. The dynamic
  loader's variables are removed.

## The service manager boundary

`systemctl --user`, found at a fixed system path that has to be root's own file, asked
four fixed things about the one unit: how it is, to read unit files again, to start it,
and to forget a failure. Arguments are fixed lists; there is no shell and no string
built from input; its environment is where the manager is and plain output, and nothing
of the desktop's. It is never left to prompt, every call is bounded, and what it printed
is read without waiting on the pipe.

**There is no stop and no restart**, not as a method and not as an argument: this
product never asks for either. D-Bus was not used: it would have been a new dependency
for four calls.

## Bootstrap

`AuthorityBootstrap` is one per desktop process, started by the shell. Every view shares
it; none starts an attempt.

```text
checking      whether an authority answers, and what is installed
installing    writing the managed authority or its service definition
starting      the service manager was asked to start it
connecting    it is started, or being restarted, and has not answered yet (bounded)
ready         AuthorityService is connected
incompatible  an authority answers that this build cannot work with
failed        the attempt ended; Retry is offered
```

It follows `AuthorityService` and acts in two cases only:

1. **when it comes up and no authority answers**, by itself, once;
2. **when the user presses Retry.**

An authority that answered and then went away is never started by the desktop itself,
whether or not this desktop started it. It crashed, and the unit is the service
manager's to restart; or it was stopped, and that is respected. The desktop watches,
says "starting" while the manager brings it back and "not running" when the manager is
not going to. There is one restart policy, the unit's.

An attempt is bounded: twenty seconds for the connection, during which the unit is
looked at a handful of times so that a start that failed is said when it is known. Once
ready nothing is polled: the follower blocks on the link, and no timer reads the unit
or the files.

### Ready is not a process

Ready is the link connected: a real greeting and a real canonical reading. A unit that
is active, a process that exists and a socket file are none of them ready, and the
window is never shown a connection the link does not have, even when the lifecycle view
says `ready` first.

### An authority that already answers

It is used as it is. Nothing is installed, reloaded or started for it, whoever started
it: a developer's, another desktop's, the previous session's. A unit the manager runs or
is about to restart is left to it and not started a second time.

### Managed and foreign

The authority that answers is this installation's when the manager runs the unit from
this installation's file and the process it names is the one that listens on the socket
(the kernel's record of the listener, read from a connection that sends nothing). Both
are read at one moment about a process that is alive to be connected to; a process
identity is compared, never signalled.

Anything else is external. It is used when it is compatible, and it is never replaced,
restarted or stopped.

### Duplicate launches

Desktops that start at once each ask for the same unit to be started; the manager runs
it once, and the authority's own locks would refuse a second one anyway. Their
installers take turns. Nothing here relies on a mutex in the window.

## Compatibility

The protocol version is the contract, and both sides already refuse a mismatch. That is
`incompatible`: it is said, and nothing is done about it.

- An incompatible authority that is not this installation's is left exactly as it is.
- One that is this installation's, from another build, is replaced the next time it
  starts. It is not restarted, because the desktop cannot ask an authority it cannot
  talk to whether work is running.

## Update model: on a cold start

A running authority is never replaced.

- A desktop that finds its own authority running from an older build connects to it and
  says **update pending**. It installs nothing and restarts nothing, with a turn running
  or without one.
- The next time no authority runs, whichever desktop is opened installs the authority
  it carries and starts that. That is after the session ended, after a stop, or after
  the manager gave up on a crash loop and the user pressed Retry.

Updating under idle work was not built. The core has no operation that says "no work
runs and none will start until I am done", so a check followed by a restart would race
a turn another client starts in between. A cold start has no such race.

## Failure, in the product's words

| What happened                                                      | What is said                                            |
| ------------------------------------------------------------------ | ------------------------------------------------------- |
| No service manager answers                                         | could not be started; names the login session's manager |
| The packaged authority is missing, altered, or in an unsafe folder | could not be trusted; nothing was installed             |
| The installation path is not safely the user's                     | not installed; nothing was changed                      |
| A unit somebody else wrote is in its place                         | left as it is; not started                              |
| The manager did not start it                                       | did not start                                           |
| It ended before answering                                          | started and ended                                       |
| It ended as often as its limit allows                              | ended repeatedly and is no longer being restarted       |
| It was stopped                                                     | not running (not an error)                              |
| It runs and does not answer                                        | did not answer in time                                  |
| The socket is not private                                          | not used                                                |
| A development build, and nothing runs                              | does not install or start one; start `wyss-control`     |

A notice above the views carries the sentence, **Retry** where there is something to
retry, and three facts for troubleshooting: what kind of build this is, whether the
authority is installed, and what the service says. No command line, unit, path, process
or socket is shown, and the terminal works throughout.

## Changes in the authority

- **The runtime's working directory.** It was the repository root compiled into the
  binary. It is now `runtime-cwd` inside the authority's private runtime directory,
  created 0700, and the runtime does not start unless it is the authority's, private and
  empty. It is in no project of the user's, next to no file of the authority's, and under
  no directory that carries project configuration. Which workspace a thread works in was
  never taken from it. The read-only developer probe keeps its own. How a process gets
  there depends on how it is started, and every runtime process is covered: the version
  check, discovery and the thread-capable process.
  - **Inference, and the version check of both profiles.** The process is started
    directly by the authority, in that host directory (`Command::current_dir`), and is
    told the same place in `PWD`, so the directory the authority itself was started from
    does not travel with it.
  - **Local Tools.** The process is started by the sandbox launcher, and the namespace
    hides the authority's runtime directory whole, `runtime-cwd` with it. Where the
    launcher is started therefore decides nothing: a `bwrap` whose own directory is gone
    goes to the user's home. The launcher is told where to go instead. The last mounts
    of the namespace are `--perms 0700 --tmpfs <runtime-cwd> --remount-ro <runtime-cwd>`,
    followed by `--chdir <runtime-cwd>`: an empty private directory that exists only in
    that namespace, at the authority's path, sealed so it stays empty, and the place
    the launcher changes to before it becomes the runtime. It is a new mount: the host
    directory is never the one the process is in, and neither it nor any parent of it
    is bound back, so the control plane stays as hidden as it was. Being last, nothing
    mounted before it can cover it. The launch is refused when the directory is not a
    private directory of this user, is reached through a link, lies in the workspace's
    project or in the user's Codex state, or would cover something the generation is
    given (its workspace, private state, executable or catalog).
- **Exit status 2** for `already_running` and `unsafe_path`.
- The path rules of `control/paths.rs` are visible to the crate. Nothing in them changed.

## Security boundary

- The window gets two commands: one read of the lifecycle view and Retry. Neither takes
  anything but a wait cursor. No service name, command, argument, path or binary can be
  named from the window, and there is no shell plugin and no process permission.
- The view is closed words: a phase, a failure, an origin, three diagnostics.
- What is installed is the digest the build was pinned to; where is fixed; what is asked
  of the manager is fixed.
- No stop, restart, enable or disable exists to be reached.

## Limits and residual risk

- **The unit's `PATH` is the service manager's.** Codex is found there. A desktop
  session imports its `PATH` into the manager at login (verified on KDE: `~/.local/bin`
  is in it); a session that does not, gives units `/usr/local/bin:/usr/bin`, and a Codex
  installed elsewhere is then reported as unavailable. The unit does not set a `PATH` of
  its own, because any fixed one would be wrong for someone.
- **The store is the session's.** The authority resolves `XDG_STATE_HOME` from the
  manager's environment. An authority started by hand with another one is another store.
- **A package in a directory its group can write is not trusted**, private group or not.
  The build tree of this repository is such a place; the package has to be installed.
- **An incompatible authority of this installation needs the session to end**, or
  someone to stop it. The desktop cannot do that safely.
- **An update waits for a cold start**, which for a session that never ends is never.
- **A unit that is active and does not answer is left alone.** The desktop says so and
  offers Retry, which waits again. It does not kill what it cannot prove idle.
- **No package format.** `desktop:package` produces a directory, not a `.deb`, an RPM,
  an AppImage or a Flatpak, and nothing signs it.
- **A local-tools runtime cannot write where it stands.** Its working directory is
  sealed. Codex 0.159.0 started, served a thread and ran an approved command from it;
  a later runtime that expects a writable process directory would not.
- **The directory of a local-tools runtime is named in its namespace.** `runtime-cwd`
  is visible there as an empty directory name next to the generation's catalog. Nothing
  else of the runtime directory is.
- **Stopping the authority through a session's end interrupts running work**, as any
  authority shutdown does.
- As with ADRs 0019 to 0024, no party other than the session that built this reviewed
  it before this record was written. Its tests were checked with mutation controls
  instead, which is not the same thing.

## Deferred

A package format and its signature; updating under idle work, which needs a drain
operation in the core; an uninstall flow; a log viewer; Settings; platforms other than
Linux.

## Acceptance evidence

Accepted on Linux (CachyOS, systemd 262, KWin 6.7.5).

Nothing of the user's own session was used. Every service manager was a private
`systemd --user`, started for the test on a temporary runtime directory and an empty
temporary home, so the unit under test was the one the product writes, unchanged. The
user's own manager was never asked anything and has no `wyss-control.service`; nothing
was installed in the real account and nothing was sent to the user's display. No real
Codex was launched for any of it except the two turns recorded under "The working
directory inside the sandbox" below.

**Phase 3F** was fast-forwarded into `main` first (`c9b71a0` to `f91c549`, eight commits
ahead and none behind, pushed, verified equal to `origin/main`). It was not reviewed or
changed again.

**The real authority under a real service manager** (`src-tauri/tests/lifecycle_process.rs`),
eight tests with the real `wyss-control`, the real `AuthorityService` and the Codex
fixture:

- **A clean account.** An empty home; the desktop is launched and nothing else is done.
  Exactly four entries appear under the data directory and three under the configuration
  directory, all private; the manager runs the unit from that file and the process is
  that binary (`/proc/<pid>/exe`); its environment has no `LD_PRELOAD` although the
  manager's has one; the unit is `static`; the store and the socket are where the
  authority has always put them. A session is activated in Local Tools, so the runtime
  starts under the unit's hardening inside `bwrap`; a command it proposes waits for a
  decision, runs once approved, and the turn completes; and every runtime process
  reported, from inside whatever it was started in, that it stands in `runtime-cwd`: the
  version check in the host directory, discovery and the thread-capable process in an
  empty sealed directory of their own namespace. The desktop closes: the same process goes on answering, with
  its runtime ready. It opens again: the same instance, nothing installed, reloaded or
  started, no restart counted. The authority is killed: the manager restarts it once, the
  desktop starts nothing, the stream is the same and continues, and the session is
  there. The authority ends cleanly on `SIGTERM`: nobody restarts it, the desktop says it
  is not running, and Retry starts it on the same store.
- **Four desktops at once** get one authority: one process runs the managed binary, every
  greeting names the same instance, and the installation is whole.
- **A crash loop ends.** An authority that exits at once runs five times and no more;
  the desktop asked for one start; a second desktop starts nothing; Retry gives one more
  round of the same limit. An authority that exits with status 2 runs once.
- **No service manager**: the failure is said within the bound, twice, and the home is
  still empty.
- **An authority started by hand** is used: nothing is installed beside it and nothing is
  asked of the manager. A second one started by hand under a managed one is refused by
  the authority's own locks with status 2.
- **An incompatible authority** (the real framing, another protocol version) is said, it
  is external, nothing is installed or started, and its socket is the one it made.
- **A newer build** while a turn runs under the old authority: `update pending`, the same
  process, the same file, the turn still running, and still so after the turn is over.
  Once the authority is stopped, the next desktop installs the newer one and starts it;
  the store is the same file, the same stream, with the session and its turn.
- **A dead socket and interrupted installs** (a temporary file left, the binary without
  the unit, a unit the manager had not read, a start already asked for) are each recovered
  on the next launch. The authority replaced the dead socket itself.

**The lifecycle against a scripted link and a scripted manager**
(`src-tauri/src/lifecycle/tests.rs`, 25 tests, with the bridge's and the runtime
directory's own): the digest and the provenance of the payload; the layout; the unit's
exact text; a clean install, an update that never writes the running file, an
interrupted install, eight installers at once; every unsafe place refused and left as it
was; what the manager is asked, word for word, by which program and with which
environment; the state machine with its bounds; an authority that answers at launch,
one that appears as an attempt begins, one that went away; an incompatible one; managed
and foreign told apart; no polling once ready; a view that never contradicts itself.

**Window** (`tests/lifecycle.test.ts`, `tests/lifecycle-view.test.ts`), 11 tests: the
parser refuses any other field and any self-contradiction; Retry carries nothing; one
follower with one read in flight and no loop; connected only when the link is; every
failure said in words with no command, unit, path or process; the terminal working while
the authority starts; one Retry however often it is pressed; no view starting anything.

**Mutation controls.** 89 single-change mutants over the adversarial list of this phase
(19 in the window, 62 in Rust, 8 against the real service manager), run on the final
code: 89 killed by a test, none by the compiler. They include the payload found in the
working directory, an unpinned build that installs, a digest not checked, links
followed, a payload or a directory others can write, `systemctl` looked up in `PATH`,
run through a shell or inheriting the desktop's environment, a start that is a restart,
a foreign unit overwritten, a binary written in place, installers not serialized, a unit
that restarts always, has no start limit, keeps the loader's variables or can be
enabled, ready without a connection, an answering authority installed over, a unit in
hand started again, a recovery that starts the service, files written without a manager,
any listener taken for managed, a retry that names a service, a wider permission, the
runtime starting in the build tree. One more is equivalent and was not run (a guard
behind a button that is already disabled).

The first pass left nine survivors: five were closed with tests, one by sending the
authority its own `SIGTERM` where the test had asked the manager to stop it, two were
process-level copies of unit mutants that the real manager's timing does not reach, and
one is the equivalent above. Reading one of them showed a real case: a desktop that was
connected when it came up could start again an authority somebody had just stopped. The
desktop now never starts by itself an authority that had answered. A later survivor was
a flag that no longer decided anything, and was removed.

**Desktop**, the packaged release (`npm run desktop:package`, copied to a prefix of the
account's own) in a nested KWin X11 display with real pointer and keyboard input, on an
empty home with a private service manager:

| #   | Check                      | Result                                                                                    |
| --- | -------------------------- | ----------------------------------------------------------------------------------------- |
| 1   | 3F merged cleanly          | `main` = `origin/main` = `f91c549`                                                        |
| 2   | No authority at launch     | No process, unit not found, home empty                                                    |
| 3   | The desktop opens          | The window and its terminal, at once                                                      |
| 4   | Bootstrap starts by itself | The binary and the unit appear; the manager runs the unit                                 |
| 5   | No terminal or command     | Nothing but launching the application was done                                            |
| 6   | Connected                  | Top bar: Authority Connected                                                              |
| 7   | Sessions                   | Lists, creates, resumes                                                                   |
| 8   | Inference session          | Created through the native folder dialog; runtime ready in the inference profile          |
| 9   | Local Tools session        | Runtime ready in the local-tools profile (where its process stood: see the closure below) |
| 10  | Agent turn                 | Completed                                                                                 |
| 11  | Approval                   | A command proposed, approved, run; the turn completed                                     |
| 12  | Events                     | 45 recorded events shown                                                                  |
| 13  | Close the desktop          | The desktop process is gone                                                               |
| 14  | The authority remains      | The same process and instance, runtime still ready                                        |
| 15  | Reopen                     | Connected to the same instance; nothing started                                           |
| 16  | Kill the authority         | `SIGKILL` to the unit's process                                                           |
| 17  | The service recovers       | One restart counted, another process                                                      |
| 18  | The desktop reconnects     | "Starting", then Connected, with nobody touching it                                       |
| 19  | State survived             | The same stream continued; the session and its failed turn are there; the session resumes |
| 20  | A second desktop           | Two windows, one managed authority, no restart                                            |
| 21  | No service manager         | "could not start", Retry, details: packaged, installed, no service manager answered       |
| 22  | An incompatible authority  | "An incompatible Wyss authority is running", left as it is, nothing to press              |
| 23  | Terminal independent       | It answered in every one of these states                                                  |
| 24  | 960×640                    | The default size of every capture                                                         |
| 25  | Narrow                     | 700×560: the notice above the terminal, under the collapsed sidebar                       |
| 26  | Native glass               | `_KDE_NET_WM_BLUR_BEHIND_REGION` is set on the window, as before                          |

Also there: an inference turn under a provider that proposes a command was stopped by the
authority as an isolation violation, as it should be; a clean stop showed "not running"
and Retry brought the authority back on the same store.

The acceptance found two defects, fixed before this record. The package built into the
repository's own tree was refused, correctly, because that tree is group-writable, and
the message did not say that a folder others can write to is one of the reasons; it does
now, and the package was installed to a private prefix as a user would. The other is the
restart of a stopped authority described above; the checks that depend on it (first run,
crash, clean exit, Retry) were repeated on the rebuilt package.

**Packaging.** `npm run desktop:package` exited 0 three times. Each time the authority's
digest was the same before and after the second build, the desktop binary contained it,
and the package was `bin/wyss-console`, `libexec/wyss-console/wyss-control` and a
manifest, with nothing writable by anyone but the owner.

**Official validation** on the final commit, each exit 0: `format:check`, `lint`,
`typecheck`, `npm test`, `npm run build`, `rust:fmt`, `rust:check`, `rust:test`,
`rust:clippy`, `test:pty`, `desktop:build`, `git diff --check`,
`git diff --cached --check`.

The interface followed the visual contract the shell already has: its tokens, its status
line and its buttons. No new direction was decided, so no design layer was run for it.

### The working directory inside the sandbox

An independent review of this phase, reported to the session that built it, found one
defect of medium severity: for a local-tools generation the working directory described
above was that of the sandbox launcher only. It was reproduced before anything was
changed. The Codex fixture was made to report the directory it is really started in, and
for a local-tools generation it reported the user's home. The evidence this record gave
for row 9, and for "every child of the authority", had read the launcher's directory
from `/proc`.

The correction is the Local Tools paragraph under "Changes in the authority". Nothing of
the lifecycle, the protocol, the store or the approval model changed.

**Tests.** Three unit tests of the isolation and one process test were added, and two
process tests changed what they look at.

- The whole launch is asserted as one exact argv in two layouts, with and without a
  control plane: the directory's mount is the last of the namespace, `--chdir` follows it
  and precedes `--`, and nothing of the control plane is bound back for it.
- In a real namespace, with the launcher started in the directory itself, in the
  workspace, in what it is told is home and in another directory, the process is in the
  one place it was sent to: empty, 0700, this user's, not the host directory of that
  name, not writable, and next to nothing else of the control plane.
- Twenty launches from a directory that is not a private place apart are refused: missing,
  a file, a link, reached through a link, open to group or others, the workspace, inside
  it, above it, elsewhere in its repository, in the user's Codex state, or holding the
  executable, the private state, a control directory or the catalog.
- With the real `wyss-control` and the fixture, in both profiles, the version check,
  discovery and the thread-capable process each report where they stand: `runtime-cwd`,
  the same in `PWD`, empty and 0700. In inference it is the host directory. In Local
  Tools it is a directory of the namespace's own, sealed, while the host one stays
  empty; of the authority's runtime directory the process sees its catalog and that one
  name, and of its state directory what it saw before. A thread is told its workspace
  and the process stays where it started.
- The clean-account test asserts the same from a unit started by a private
  `systemd --user`, under the unit's hardening, and approves a command there.

**Mutation controls.** 25 single-change mutants, each run against the unit tests and the
process tests together, after an unchanged run that passes: 25 killed by a test, none by
the compiler. They are: `--chdir` removed, to the home, to the workspace, to `/`; the
namespace directory not made; the control plane bound back; the directory made before
the mask that covers it; only discovery placed; only the thread-capable process placed;
the host directory bound instead of a new one; its parent bound back; not sealed; not
private; the supervisor sending the workspace; a direct process not told its place, or
told and not moved; the version check, the final process or discovery not placed; and
each of the six refusals removed. `--chdir` removed is killed by the unit tests, which
start the launcher elsewhere, and not by the process tests: the authority also starts
the launcher in that path, so a launcher told nothing still ends up there. As a control
of the evidence itself, the original defect was put back and both kinds of evidence were
run against it: the unit and process tests above fail, and the former check, which read
the launcher's directory, passes.

**Real Codex 0.159.0**, on 2026-10-10, through the debug build of this commit's
authority and the control protocol, in an isolated authority state and a disposable
workspace, with the user's existing login used as it is. Two turns.

| Profile     | Measured                                                                                                                                                                                                                                                                                                                                                                                                       |
| ----------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Inference   | Runtime ready, `codex_inference_v1`, seven models. The app-server process is in `runtime-cwd`, the host directory itself, and `PWD` says so. One turn, completed. The directory was empty afterwards.                                                                                                                                                                                                          |
| Local Tools | Runtime ready, `codex_local_tools_v1`. The launcher is in the host directory. The app-server inside it is at the same path in a different directory: a read-only `tmpfs` mounted there, 0700, empty. One turn: `/usr/bin/bash -c pwd` proposed in the workspace, pending, approved, run, confirmed; the turn completed and the reply named the workspace. The workspace and the host directory were unchanged. |

A command a tool runs is told the workspace; the process that serves it is not there.
The credential file was the same inode, with the same mode and one name, before and
after; nothing of it was read. The reply of the inference turn was not captured: the
driver asked for more events than one read allows, after the turn had ended, and what
is recorded of it is the durable turn record, read by a restarted authority on the same
store.

**The packaged desktop**, built from this commit and installed to a private prefix, in a
nested display on an empty home with a private service manager: the authority was
installed and started by the first launch; an Inference session was created and its
runtime ready; a Local Tools session restarted Codex in that profile and was ready; a
command was proposed, approved, run once, and the turn completed; Events listed 28
recorded events; the terminal was the same session before and after; the window was
closed and the same authority process went on, not restarted, its runtime ready. The six
runtime processes of that run reported the directory as above, and the installed
authority had the pinned digest.

**Official validation and packaging** were repeated on the commit that carries this
section: the thirteen commands above each exit 0, and `npm run desktop:package` exits 0
with the same authority digest as the package the smoke used.
