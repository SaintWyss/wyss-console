# 0006 — Terminal renderer and transient flow

Status: Accepted

Date: 2026-10-06

## Decision

Sprint 1B uses the official stable `@xterm/xterm 6.0.0` DOM renderer with only
`@xterm/addon-fit 0.11.0`. Package versions were checked against the npm registry;
no deprecated unscoped packages, WebGL, search, web-links or other addons are used.
The application does not implement ANSI parsing, a screen model or cursor semantics.

- [Official renderer and package ecosystem](https://github.com/xtermjs/xterm.js)
- [Byte encoding and onBinary](https://xtermjs.org/docs/guides/encoding/)
- [Renderer processing callback and flow control](https://xtermjs.org/docs/guides/flowcontrol/)

The UI displays one local terminal, a small application header and terminal-local
status/close control. Renderer, controller/lifecycle, IPC bridge and DTOs are separate.
The backend remains capable of multiple PTYs; this is not a singleton restriction
on TerminalService. There is no tab/session manager or automatic respawn.

## TERM and display state

The Rust service sets only `TERM=xterm-256color` for each directly spawned shell,
replacing Sprint 1A's `dumb`. The parent/global environment and shell configuration
are unchanged. Fish's real capability queries are answered by xterm. Backend-only
account-shell tests validate spawn/cleanup without pretending to be a renderer;
real GUI acceptance validates interaction and handshake.

Renderer scrollback is explicitly **5,000 lines**, plus the bounded screen dimensions
(maximum 512 rows/columns). It is volatile frontend visual state, not persistence,
control state or audit history. The system monospace font stack uses 16 px text.
Foreground/background follow the existing light/dark application variables; xterm's
built-in ANSI palette is used with a minimum contrast ratio of 4.5. There is no theme
or font settings UI, bundled font, animation or additional design framework.

## Output credit and input bounds

Flow: PTY → 64 KiB backend ring → at most 16 KiB IPC batch → xterm
`write(Uint8Array, callback)` → callback reports parser processing → next pull
acknowledges that batch. The bridge remembers a receipt when received, but sends it
only on the next pull; the controller never makes that pull before the renderer
callback. At most one output pull/write is in flight. The application never decodes
PTY bytes to text before passing them to xterm. Split UTF-8 and ANSI sequences are
the renderer's responsibility.

Empty pulls back off from 20 ms to **320 ms**; data resets the delay, and input wakes
an idle wait. There is one cancellable timer, not a recurring interval or producer
queue. Closing/disposal cancels the wait and pending renderer callback. Output with
`eof` can still contain buffered bytes: drain successive batches until an empty
EOF batch with the backend's final exit/failure snapshot, then stop polling.
PTY EOF can precede child reaping: input stops immediately, while bounded pulls
continue briefly to obtain the observed exit status. The final screen remains viewable.

Input is independent of output. `onData` text is UTF-8 encoded; `onBinary` bytes
are copied as binary values, without text re-encoding. A FIFO has a **64 KiB logical
byte limit**, including the in-flight chunk, with one <=4 KiB backend write at a time.
Individual text/binary submissions are also bounded. Oversized/saturated input fails
visibly and closes the local resource rather than silently dropping input or growing
an unlimited queue. Large paste polish is deferred. Commands and keyboard control
characters are transported without interpretation. Writes are never automatically
retried after partial/ambiguous failure. Likewise, an ambiguous IPC output failure
stops the terminal rather than guessing whether a receipt was consumed.

## Resize and lifecycle

Fit uses the official addon's measured proposed dimensions only for measurable
containers. Dimensions are clamped to the backend's existing range. ResizeObserver
notifications coalesce over **60 ms**; identical dimensions are skipped. There is one
resize IPC operation in flight plus at most the latest desired size.

Initialization is idempotent per controller. The single view lease waits for prior
owner cleanup on remount; late creation after disposal is immediately closed. HMR
requests disposal without claiming atomic lifecycle across a full module reload.
Input, output and resize stop on disposal. Listeners, observer, timers, theme listener,
xterm and its fit addon are disposed. Repeated explicit close/disposal sends only
one close command. Creation or renderer failure becomes a concise local error.
Natural exit stops IO, does not respawn and retains the final screen until close or
unmount; normal app exit retains Sprint 1A's off-UI-thread backend shutdown.

**Human terminal survival across Console process crash is not guaranteed at this stage.**

## Security and CSP integration

No Tauri capabilities or commands are added. The frontend still cannot choose an
executable, argv, environment, PID or arbitrary signal; no shell/filesystem/network
plugin is introduced. Built-in OSC hyperlinks cannot open an external destination;
no link addon or clipboard permission is enabled. Default selection/paste behavior
is left to xterm and the WebView; clipboard polish is deferred.

Inspection of xterm 6's DOM renderer shows runtime-generated style elements for
geometry and theme (`DomRenderer.ts`). Production permits these CSS elements with
`style-src-elem 'self' 'unsafe-inline'`; `style-src 'self'` remains;
`style-src-attr 'unsafe-inline'` is required as well. GUI acceptance proved that
`style-src-attr 'none'` blocks xterm's `setAttribute('style', ...)` color overrides
(minimum contrast and RGB foreground/background in `DomRendererRowFactory.ts`). This is a CSS-only CSP relaxation, not permission
for inline JavaScript, eval, remote styles, remote scripts or network access.
Script/connect/img/frame/base/form restrictions remain unchanged. Application code
never injects terminal content as HTML; parsing/rendering belongs to xterm.

## Deferred

EventEnvelope lifecycle producers, Event Bus, durable terminal content, authority,
agents, tabs/panes, persistent sessions, settings, clipboard enhancements and every
other control-plane integration remain deferred. ADR 0002's managed-agent lifecycle
invariant is unchanged: these are local human terminals.
