# ADR 0036: A host process holds the terminals and the native sessions (F9)

Status: Accepted (owner authorisation of the whole roadmap, 2026-10-19; DN-05 as recommended)
Date: 2026-10-19

## Context

Terminals and native agent sessions belonged to the window (ADR 0005) and ended with it. The
owner wants sessions that survive closing the GUI, and F10 (remote) cannot exist before that
(roadmap §5). Doc 16 §4.1 recommends a host service apart from the authority; DN-05 is decided
as recommended.

## Decision

1. **A separate process, the same executable.** The host is `wyss-console --wyss-host`, started
   by the window when none answers, detached (own process group, no stdio, working directory
   `/`). It is not the authority (`wyss-control`) and shares nothing with it: a fault in
   terminal handling cannot reach leases or approvals. One executable keeps installation to one
   file; the authority boundary test still holds because the authority is another binary.
2. **A private socket.** `$XDG_RUNTIME_DIR/wyss-console/host.sock`: directory 0700, socket 0600,
   the peer's uid must equal the host's. One request and one answer per connection, a 4-byte
   length and JSON (at most 1 MiB), a protocol number checked by `hello`. A request has a closed
   operation name and parameters that reject unknown fields: there is no operation that takes a
   program, an argument list or an environment from a client except the catalogue-bound
   `native_start` of ADR 0035. A second host is refused; a socket nobody answers on is replaced;
   anything that is not a socket is never touched.
3. **What it holds.** The terminal service the window always had, plus, per terminal, a reader
   that keeps a bounded scrollback (512 KiB, absolute offsets). A client reads "from here"; a
   client that falls behind is told the output was truncated; several clients can read one
   terminal. The native service of ADR 0035 (registry, profiles, projects) lives in the host, so
   a session is `running` across window restarts and `lost` only when the host itself is gone.
4. **The window is a client.** The terminal commands keep their names and shapes and answer by
   asking the host; the credit rule of the page (a batch is delivered once the previous one is
   acknowledged) is kept in the window per terminal, so the page did not change. On opening, the
   window lists what the host holds and shows it again, with titles and agent tags; a program
   that drew its screen earlier is asked to draw it again (the size is nudged and put back).
   Closing a tab closes the terminal in the host; leaving the window detaches and closes
   nothing.
5. **A host of its own as the fallback.** When no host can be reached or started, the window runs
   the same code in its own process, as before: terminals then end with it, and the Workspace
   view says so instead of promising persistence.
6. **Lifetime.** A host with no terminal and no running agent for ten minutes exits, as does one
   told to (`--wyss-host-stop`, which closes its terminals). It dies with the user's session; it
   is not a system service. A packaged install may ask `systemd --user` to supervise it (F13).
7. **Environment.** Terminals made by the host inherit its environment minus the closed list of
   per-agent-session variables (ADR 0035 §5), plain terminals included.

## Consequences and limits

- Verified in the real window: a terminal's scrollback and a running Claude Code survived the
  window being killed; the reopened window showed both, and the overview said Active with the
  same process id.
- The host is a long-lived process with PTYs and an agent launcher, reachable by any process of
  the same user: the same trust as the authority's socket (T-12). Remote access is not added
  here; F10 puts a gateway with device identities in front of it.
- A crash of the host loses its terminals; their records become `lost`.
- Scrollback is memory only and bounded; no output is written to disk.
