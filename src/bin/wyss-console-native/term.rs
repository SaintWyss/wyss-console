//! A terminal widget: libghostty-vt keeps the screen state, GTK's scene graph (GSK) draws it on the
//! GPU. Output flows PTY -> reader thread -> `ghostty::Terminal` (behind a fair
//! lock); the GTK thread snapshots it into a `RenderState` and rebuilds only the dirty rows.

use std::cell::{Cell as StdCell, RefCell};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gdk, glib, graphene, gsk, pango};

use crate::ghostty::{self, Cell, CursorStyle, Dirty, Rgb, Wide};
use crate::host::{self, Host};
use crate::palette;
use crate::pty::Pty;
use crate::keys;
use crate::wheel;
use crate::selection::{self, Mode, Pos, Selection};
use crate::view::TermView;

/// IBM Plex Mono when installed (the Console's font); pango falls back to the system monospace.
const FONT: &str = "IBM Plex Mono, Monospace";
const PAD: f32 = 4.0;
const READ_BLOCK: usize = 256 * 1024;
const WHEEL_LINES: f64 = 3.0;
/// How long a host read waits for new output before answering empty.
const LONG_POLL_MS: u64 = 1000;
/// The host caps the history included in a screen snapshot at 1000 lines.
pub fn attach_scrollback(lines: u32) -> u32 {
    lines.min(1000)
}

thread_local! {
    /// Widgets by id, reachable from the PTY thread through `MainContext::invoke`.
    static VIEWS: RefCell<HashMap<u64, glib::WeakRef<TermView>>> = RefCell::new(HashMap::new());
    static ON_EXIT: RefCell<HashMap<u64, Box<dyn Fn()>>> = RefCell::new(HashMap::new());
}

/// The terminal shared between the reader thread (writes output) and the GTK thread (snapshots it,
/// scrolls, resizes). `next` makes the lock fair: a waiter queues on `next` first, so the reader
/// cannot starve the draw while output floods in.
struct Shared {
    term: Mutex<ghostty::Terminal>,
    next: Mutex<()>,
    /// A redraw is already requested and not yet painted.
    pending: AtomicBool,
    scrollback: AtomicU32,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, ghostty::Terminal> {
        let queue = self.next.lock().unwrap_or_else(|e| e.into_inner());
        let guard = self.term.lock().unwrap_or_else(|e| e.into_inner());
        drop(queue);
        guard
    }
}

/// Requests one redraw per frame: the flag stays set until `draw` clears it, so a flood of output
/// costs a single main-loop dispatch per painted frame.
fn request_redraw(shared: &Shared, id: u64) {
    if !shared.pending.swap(true, Ordering::AcqRel) {
        glib::MainContext::default().invoke(move || {
            VIEWS.with(|v| {
                if let Some(w) = v.borrow().get(&id).and_then(|w| w.upgrade()) {
                    w.queue_draw();
                }
            });
        });
    }
}

pub struct Terminal {
    state: Rc<State>,
    pub widget: TermView,
    /// The shell of a local terminal (host terminals are asked about through the host).
    pub local_pid: Option<u32>,
}

/// Where a terminal's bytes come from.
pub enum Source {
    /// A shell on a PTY of this process.
    Local,
    /// A terminal that lives in the host; closing the tab leaves it running.
    Host { host: Host, tid: String, attach: Attach },
}

/// How a window joins a host terminal.
pub enum Attach {
    /// Just created: read from the start, nothing to restore.
    Fresh,
    /// Was running without this window: draw its screen, read on from where the screen ends.
    Screen(host::Screen),
    /// The host has no screen for it: read what the host kept from the start.
    Replay,
}

/// What the terminal sends to its child: straight to the PTY master, or through a thread that
/// talks to the host (input never blocks the GTK thread on a socket).
#[derive(Clone)]
enum Input {
    Pty(Arc<std::fs::File>),
    Host(mpsc::Sender<Out>),
}

enum Out {
    Data(Vec<u8>),
    Resize(u16, u16),
}

impl Input {
    fn send(&self, bytes: &[u8]) {
        match self {
            Input::Pty(f) => {
                let _ = (&**f).write_all(bytes);
            }
            Input::Host(tx) => {
                let _ = tx.send(Out::Data(bytes.to_vec()));
            }
        }
    }
}

/// Sends input and resizes to the host in order, merging consecutive writes into one call.
fn host_writer(host: Host, tid: String, rx: mpsc::Receiver<Out>) {
    let mut pending: Option<Out> = None;
    loop {
        let first = match pending.take() {
            Some(m) => m,
            None => match rx.recv() {
                Ok(m) => m,
                Err(_) => return,
            },
        };
        match first {
            Out::Resize(rows, cols) => {
                let _ = host.resize(&tid, rows, cols);
            }
            Out::Data(mut buf) => {
                while let Ok(m) = rx.try_recv() {
                    match m {
                        Out::Data(more) if buf.len() < host::WRITE_CHUNK => buf.extend_from_slice(&more),
                        other => {
                            pending = Some(other);
                            break;
                        }
                    }
                }
                let _ = host.write(&tid, &buf);
            }
        }
    }
}

/// Pause before the next read after `failures` transport errors in a row: 100 ms doubling to 2 s.
fn retry_backoff(failures: u32) -> Duration {
    let ms = 100u64.saturating_mul(1u64 << failures.saturating_sub(1).min(5));
    Duration::from_millis(ms.min(2000))
}

fn retry_read_error(error: &host::HostError, failures: u32) -> Option<Duration> {
    match error {
        host::HostError::Remote { code, .. } if code != "busy" => None,
        _ => Some(retry_backoff(failures)),
    }
}

/// Reads a host terminal into ghostty until it ends. Returns when the terminal is closed or eof,
/// or the host reports it unknown; transport errors are retried.
fn host_reader(host: Host, tid: String, mut from: u64, shared: Arc<Shared>, id: u64, quiet: Arc<AtomicBool>, mut replay: bool) {
    let mut failures = 0u32;
    let (started, mut bytes, mut reads, mut feed_ns, mut resyncs) = (std::time::Instant::now(), 0usize, 0u32, 0u128, 0u32);
    loop {
        match host.read(&tid, from, LONG_POLL_MS) {
            Ok(r) => {
                failures = 0;
                if r.truncated {
                    // The host's ring moved past what was read: draw its screen again and go on from there.
                    if let Ok(Some(s)) = host.screen(&tid, attach_scrollback(shared.scrollback.load(Ordering::Relaxed))) {
                        resyncs += 1;
                        quiet.store(true, Ordering::Release);
                        {
                            let mut t = shared.lock();
                            t.write(b"\x1bc");
                            t.write(&s.repaint);
                        }
                        quiet.store(false, Ordering::Release);
                        request_redraw(&shared, id);
                        from = s.at;
                        continue;
                    }
                }
                if !r.data.is_empty() {
                    let t = std::time::Instant::now();
                    shared.lock().write(&r.data);
                    feed_ns += t.elapsed().as_nanos();
                    bytes += r.data.len();
                    reads += 1;
                    request_redraw(&shared, id);
                } else if replay {
                    // The kept output is replayed: answers to its old queries were not wanted.
                    replay = false;
                    quiet.store(false, Ordering::Release);
                }
                from = r.next;
                if r.eof || r.closed {
                    if std::env::var_os("WYSS_CONSOLE_STATS").is_some() {
                        eprintln!("host_reader: eof={} closed={} at {from}", r.eof, r.closed);
                    }
                    break;
                }
            }
            Err(e) => {
                if std::env::var_os("WYSS_CONSOLE_STATS").is_some() {
                    eprintln!("host_reader: {e}");
                }
                failures = failures.saturating_add(1);
                let Some(delay) = retry_read_error(&e, failures) else { break };
                if Arc::strong_count(&shared) == 1 {
                    break; // the terminal view is gone: nobody is left to read for
                }
                std::thread::sleep(delay);
            }
        }
    }
    if std::env::var_os("WYSS_CONSOLE_STATS").is_some() {
        let ms = |ns: u64| ns / 1_000_000;
        eprintln!(
            "stats: {resyncs} truncations (screen resyncs), {bytes} bytes in {reads} data reads, {:.2}s since attach; host call+frame decode {} ms (parse of bytes {} ms), ghostty feed {} ms",
            started.elapsed().as_secs_f64(),
            ms(host::NET_NS.load(Ordering::Relaxed)),
            ms(host::PARSE_NS.load(Ordering::Relaxed)),
            feed_ns / 1_000_000
        );
    }
    // The shell ended (or the terminal was closed): drop its entry in the host.
    let _ = host.close(&tid);
    glib::MainContext::default().invoke(move || {
        ON_EXIT.with(|m| {
            if let Some(f) = m.borrow_mut().remove(&id) {
                f();
            }
        });
    });
}

/// GTK-thread side of the drawing: the render state, the cells of each viewport row as last painted
/// (selection reads these, so it always copies what is on screen) and the node cached per row.
struct Frame {
    rs: ghostty::RenderState,
    rows: Vec<Option<gsk::RenderNode>>,
    grid: Vec<Vec<Cell>>,
    /// Rows whose content ghostty reported dirty in the frame being built.
    dirty: Vec<bool>,
    /// Selection bounds baked into the cached nodes.
    painted: Option<(Pos, Pos)>,
    size: (u16, u16),
}

struct State {
    preferences: StdCell<wyss_console::settings::Terminal>,
    blink_visible: StdCell<bool>,
    cursor_blinks: StdCell<bool>,
    shared: Arc<Shared>,
    pty: Option<Pty>,
    input: Input,
    input_revision: StdCell<u64>,
    cell_w: StdCell<f64>,
    cell_h: StdCell<f64>,
    size: StdCell<(usize, usize)>,
    pango: RefCell<Option<pango::Context>>,
    frame: RefCell<Frame>,
    sel: RefCell<Option<Selection>>,
    /// The search of the pane (Ctrl+Shift+F) and what it found as of the last frame; `None` while the
    /// bar is closed.
    find: RefCell<Option<ghostty::Search>>,
    found: RefCell<ghostty::Found>,
    /// A left-button press in progress: where it started and how it expands.
    drag: RefCell<Option<(Pos, Mode)>>,
    /// Text of the selection as of the last drag step: a redraw under the pointer may drop the
    /// selection before the button is released, and release still has to copy.
    drag_text: RefCell<Option<String>>,
    /// Row of the cursor as last painted: where the input line is.
    cursor_row: std::cell::Cell<Option<usize>>,
    /// The selection is the whole input line (Ctrl+Shift+A): Backspace/Delete then erase it.
    input_sel: std::cell::Cell<bool>,
    /// Last pointer position over the area, for mouse wheel reports.
    pointer: StdCell<(f64, f64)>,
}

impl State {
    /// Viewport cell under a widget point, clamped to the grid; `None` before the first frame.
    fn cell_at(&self, x: f64, y: f64) -> Option<Pos> {
        let f = self.frame.borrow();
        let cols = f.grid.first().map_or(0, |r| r.len());
        if cols == 0 {
            return None;
        }
        let col = ((x - PAD as f64) / self.cell_w.get()).floor().max(0.0) as usize;
        let row = ((y - PAD as f64) / self.cell_h.get()).floor().max(0.0) as usize;
        Some(Pos { row: row.min(f.grid.len() - 1), col: col.min(cols - 1) })
    }

    /// Text of the current selection, as painted.
    fn selected_text(&self) -> Option<String> {
        let sel = (*self.sel.borrow())?;
        let f = self.frame.borrow();
        let (a, b) = sel.bounds(&f.grid);
        let t = selection::extract(&f.grid, a, b);
        (!t.is_empty()).then_some(t)
    }

    fn clear_selection(&self) {
        *self.sel.borrow_mut() = None;
        *self.drag.borrow_mut() = None;
        self.input_sel.set(false);
    }

    /// User input: snaps the view back to the bottom, then goes to the child.
    fn write(&self, bytes: &[u8]) {
        {
            let mut t = self.shared.lock();
            if !t.viewport_active() {
                t.scroll_bottom();
            }
        }
        self.send_input(bytes);
    }

    fn send_input(&self, bytes: &[u8]) {
        self.input_revision.set(self.input_revision.get().saturating_add(1));
        self.input.send(bytes);
    }

}

/// A line end can submit a command unless DEC 2004 is enabled.
fn paste_needs_confirmation(text: &str, bracketed: bool) -> bool {
    // Any line end would act as Enter, including one at the tail ("curl … | sh⏎").
    !bracketed && text.contains(['\r', '\n'])
}

/// The lines of a paste, split on LF, CR LF or CR, without the line ends at its tail.
fn paste_lines(text: &str) -> Vec<&str> {
    let text = text.trim_end_matches(['\r', '\n']);
    if text.is_empty() {
        return Vec::new();
    }
    text.split("\r\n").flat_map(|l| l.split(['\n', '\r'])).collect()
}

fn prepare_paste(text: &str, read_bracketed: impl FnOnce() -> bool) -> (bool, Vec<u8>) {
    let bracketed = read_bracketed();
    (paste_needs_confirmation(text, bracketed), paste_bytes(text, bracketed))
}

/// What Ctrl+V does with the clipboard: text pastes as text; an image has no text form, so the
/// terminal forwards Ctrl+V (0x16) and the program in front (Claude Code, Codex) reads the image
/// itself, as it does under Warp.
#[derive(Debug, PartialEq, Eq)]
enum ClipboardPaste {
    Text,
    ForwardImage,
    Nothing,
}

/// `mimes` are the clipboard's real MIME types. GTK reports `String` as readable for any clipboard
/// (an image comes back as "\x89PNG"), so only an actual text type counts as text.
fn clipboard_paste(mimes: &[&str]) -> ClipboardPaste {
    let has_text = mimes.iter().any(|m| m.starts_with("text/plain") || matches!(*m, "UTF8_STRING" | "STRING" | "TEXT"));
    let has_image = mimes.iter().any(|m| m.starts_with("image/"));
    match (has_text, has_image) {
        (true, _) => ClipboardPaste::Text,
        (false, true) => ClipboardPaste::ForwardImage,
        (false, false) => ClipboardPaste::Nothing,
    }
}

fn paste_clipboard(state: &Rc<State>, area: &gtk::Widget) {
    let clipboard = area.clipboard();
    let mimes = clipboard.formats().mime_types();
    let mimes: Vec<&str> = mimes.iter().map(|m| m.as_str()).collect();
    match clipboard_paste(&mimes) {
        ClipboardPaste::Text => {
            let state = state.clone();
            let area_w = area.downgrade();
            clipboard.read_text_async(None::<&gtk::gio::Cancellable>, move |r| {
                if let Ok(Some(t)) = r
                    && let Some(w) = area_w.upgrade()
                {
                    paste_or_confirm(&state, &w, t.to_string());
                }
            });
        }
        ClipboardPaste::ForwardImage => state.write(b"\x16"),
        ClipboardPaste::Nothing => {}
    }
}

/// Pastes `text`, holding the same DEC 2004 snapshot through the confirmation and send.
fn paste_or_confirm(state: &Rc<State>, widget: &gtk::Widget, text: String) {
    let (confirm, bytes) = prepare_paste(&text, || state.shared.lock().bracketed_paste());
    if !confirm {
        state.write(&bytes);
        return;
    }
    let lines = paste_lines(&text);
    let shown: Vec<String> = lines
        .iter()
        .take(5)
        .map(|l| {
            let mut short: String = l.chars().filter(|c| !c.is_control() || *c == '\t').take(80).collect();
            if l.chars().count() > 80 {
                short.push('…');
            }
            short
        })
        .collect();
    let mut detail = shown.join("\n");
    if lines.len() > shown.len() {
        detail.push_str("\n…");
    }
    let count = lines.len().to_string();
    let dialog = gtk::AlertDialog::builder()
        .message(if lines.len() == 1 {
            crate::i18n::tr("This paste ends with a line break, which can run it as a command.")
        } else {
            crate::i18n::trf(
                "You are about to paste {lines} lines; each line break can run a command.",
                &[("lines", &count)],
            )
        })
        .detail(detail)
        .build();
    dialog.set_modal(true);
    dialog.set_buttons(&[&crate::i18n::tr("Paste"), &crate::i18n::tr("Cancel")]);
    dialog.set_cancel_button(1);
    dialog.set_default_button(1);
    let window = widget.root().and_downcast::<gtk::Window>();
    let state = state.clone();
    dialog.choose(window.as_ref(), gtk::gio::Cancellable::NONE, move |result| {
        if result.ok() == Some(0) {
            state.write(&bytes);
        }
    });
}

const PASTE_START: &str = "\x1b[200~";
const PASTE_END: &str = "\x1b[201~";

/// What a paste sends: embedded bracket markers and control characters (C0 except tab and line
/// ends, DEL, C1) are dropped so pasted text cannot end the bracket or drive the terminal; then the
/// text is wrapped when bracketed paste is on, or its line ends become CR like typed Enter.
fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let mut t = text.to_owned();
    while t.contains(PASTE_START) || t.contains(PASTE_END) {
        t = t.replace(PASTE_START, "").replace(PASTE_END, "");
    }
    let clean: String = t
        .chars()
        .filter(|&c| matches!(c, '\t' | '\n' | '\r') || !(c < ' ' || ('\u{7f}'..='\u{9f}').contains(&c)))
        .collect();
    if bracketed {
        format!("{PASTE_START}{clean}{PASTE_END}").into_bytes()
    } else {
        clean.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

impl Terminal {
    pub fn new(id: u64, source: Source, preferences: wyss_console::settings::Terminal, on_exit: impl Fn() + 'static) -> Terminal {
        let (mut cols, mut lines) = (80usize, 24usize);
        let quiet = Arc::new(AtomicBool::new(false));
        let (pty, input, reader, host_part) = match source {
            Source::Local => {
                // WYSS_CONSOLE_CMD runs one command instead of the login shell (used by the benchmarks).
                let cmd = std::env::var("WYSS_CONSOLE_CMD").ok();
                let (pty, reader) = Pty::spawn(cols as u16, lines as u16, 8, 16, cmd.as_deref()).expect("spawn shell");
                let input = Input::Pty(pty.writer());
                (Some(pty), input, Some(reader), None)
            }
            Source::Host { host, tid, attach } => {
                if let Attach::Screen(s) = &attach {
                    (cols, lines) = (s.cols as usize, s.rows as usize);
                }
                let (tx, rx) = mpsc::channel();
                let (h, t) = (host.clone(), tid.clone());
                std::thread::spawn(move || host_writer(h, t, rx));
                (None, Input::Host(tx), None, Some((host, tid, attach)))
            }
        };

        // What the terminal answers to queries (DA, cursor position, mode reports) goes to the child,
        // except while a screen or a replay is being drawn: those answers belong to old queries.
        let reply_to = input.clone();
        let reply_quiet = quiet.clone();
        let mut term = ghostty::Terminal::new(
            cols as u16,
            lines as u16,
            preferences.scrollback as usize,
            Box::new(move |bytes| {
                if !reply_quiet.load(Ordering::Acquire) {
                    reply_to.send(bytes);
                }
            }),
        );
        let c = palette::colors();
        term.set_colors(c.fg, c.bg, &c.table);
        term.set_cursor(preferences.cursor_style, preferences.cursor_blink);
        let shared = Arc::new(Shared {
            term: Mutex::new(term),
            next: Mutex::new(()),
            pending: AtomicBool::new(false),
            scrollback: AtomicU32::new(preferences.scrollback),
        });

        // Reader thread: big blocks from the PTY straight into the interpreter.
        if let Some(reader) = reader {
            let shared = shared.clone();
            std::thread::spawn(move || {
                let mut reader = reader;
                let mut buf = vec![0u8; READ_BLOCK];
                loop {
                    match reader.file.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            shared.lock().write(&buf[..n]);
                            request_redraw(&shared, id);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break, // EIO: the child closed the slave side
                    }
                }
                reader.reap();
                glib::MainContext::default().invoke(move || {
                    ON_EXIT.with(|m| {
                        if let Some(f) = m.borrow_mut().remove(&id) {
                            f();
                        }
                    });
                });
            });
        }
        // Host terminal: draw the screen it already has (if any), then long-poll its output.
        if let Some((host, tid, attach)) = host_part {
            let (from, replay) = match attach {
                Attach::Fresh => (0, false),
                Attach::Screen(s) => {
                    quiet.store(true, Ordering::Release);
                    shared.lock().write(&s.repaint);
                    quiet.store(false, Ordering::Release);
                    (s.at, false)
                }
                Attach::Replay => {
                    quiet.store(true, Ordering::Release);
                    (0, true)
                }
            };
            let (shared, quiet) = (shared.clone(), quiet.clone());
            std::thread::spawn(move || host_reader(host, tid, from, shared, id, quiet, replay));
        }

        let local_pid = pty.as_ref().map(|p| p.pid() as u32);
        let state = Rc::new(State {
            shared: shared.clone(),
            preferences: StdCell::new(preferences),
            blink_visible: StdCell::new(true),
            cursor_blinks: StdCell::new(false),
            pty,
            input,
            input_revision: StdCell::new(0),
            cell_w: StdCell::new(8.0),
            pointer: StdCell::new((0.0, 0.0)),
            cell_h: StdCell::new(16.0),
            size: StdCell::new((cols, lines)),
            pango: RefCell::new(None),
            frame: RefCell::new(Frame {
                rs: ghostty::RenderState::new(),
                rows: Vec::new(),
                grid: Vec::new(),
                dirty: Vec::new(),
                painted: None,
                size: (0, 0),
            }),
            sel: RefCell::new(None),
            find: RefCell::new(None),
            found: RefCell::new(ghostty::Found::default()),
            drag: RefCell::new(None),
            drag_text: RefCell::new(None),
            cursor_row: std::cell::Cell::new(None),
            input_sel: std::cell::Cell::new(false),
        });

        let area = {
            let (d, r) = (state.clone(), state.clone());
            TermView::new(
                Box::new(move |snap, w, h| draw(snap, &d, w, h)),
                Box::new(move |w, h| resize(&r, w, h)),
            )
        };
        measure_font(&area, &state);
        {
            let state = Rc::downgrade(&state);
            let area = area.downgrade();
            glib::timeout_add_local(Duration::from_millis(500), move || {
                let (Some(state), Some(area)) = (state.upgrade(), area.upgrade()) else {
                    return glib::ControlFlow::Break;
                };
                if area.is_mapped() && state.cursor_blinks.get() {
                    state.blink_visible.set(!state.blink_visible.get());
                    area.queue_draw();
                } else {
                    state.blink_visible.set(true);
                }
                glib::ControlFlow::Continue
            });
        }
        *state.pango.borrow_mut() = Some(area.pango_context());

        VIEWS.with(|v| v.borrow_mut().insert(id, area.downgrade()));
        ON_EXIT.with(|m| m.borrow_mut().insert(id, Box::new(on_exit)));

        // Input: IM context first (dead keys, IBus), then control/navigation keys.
        let im = gtk::IMMulticontext::new();
        im.set_client_widget(Some(&area));
        {
            let state = state.clone();
            im.connect_commit(move |_, text| state.write(text.as_bytes()));
        }
        let keyc = gtk::EventControllerKey::new();
        keyc.set_im_context(Some(&im));
        {
            let state = state.clone();
            keyc.connect_key_pressed(move |_, key, _, mods| {
                // With the input line selected (Ctrl+Shift+A), Backspace/Delete erase it: End, then
                // kill-to-start (readline, zsh and Claude Code's box all take Ctrl+E / Ctrl+U).
                if state.input_sel.replace(false) && state.sel.borrow().is_some() && matches!(key, gtk::gdk::Key::BackSpace | gtk::gdk::Key::Delete) {
                    state.clear_selection();
                    state.write(&[0x05, 0x15]);
                    return glib::Propagation::Stop;
                }
                let app_cursor = state.shared.lock().app_cursor();
                if let Some(bytes) = keys::encode(key, mods, app_cursor) {
                    state.write(&bytes);
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
        }
        area.add_controller(keyc);
        let focus = gtk::EventControllerFocus::new();
        {
            let im = im.clone();
            focus.connect_enter(move |_| im.focus_in());
        }
        focus.connect_leave(move |_| im.focus_out());
        area.add_controller(focus);
        // Any button gives focus; left starts a selection (1/2/3 clicks = cells/word/line), middle
        // pastes PRIMARY.
        let click = gtk::GestureClick::new();
        click.set_button(0);
        {
            let state = state.clone();
            click.connect_pressed(move |g, n, x, y| {
                let Some(w) = g.widget() else { return };
                w.grab_focus();
                state.input_sel.set(false);
                match g.current_button() {
                    1 => {
                        let Some(pos) = state.cell_at(x, y) else { return };
                        let mode = match (n - 1) % 3 {
                            0 => Mode::Cell,
                            1 => Mode::Word,
                            _ => Mode::Line,
                        };
                        *state.drag.borrow_mut() = Some((pos, mode));
                        // A plain click only clears; a cell selection starts when the pointer moves.
                        *state.sel.borrow_mut() = (mode != Mode::Cell).then_some(Selection { anchor: pos, head: pos, mode });
                        w.queue_draw();
                    }
                    2 => {
                        let state = state.clone();
                        let wk = w.downgrade();
                        w.display().primary_clipboard().read_text_async(None::<&gtk::gio::Cancellable>, move |r| {
                            if let Ok(Some(t)) = r
                                && let Some(w) = wk.upgrade()
                            {
                                paste_or_confirm(&state, &w, t.to_string());
                            }
                        });
                    }
                    _ => {}
                }
            });
        }
        area.add_controller(click);
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        {
            let state = state.clone();
            drag.connect_drag_update(move |g, ox, oy| {
                let Some((sx, sy)) = g.start_point() else { return };
                let Some(&(anchor, mode)) = state.drag.borrow().as_ref() else { return };
                let Some(head) = state.cell_at(sx + ox, sy + oy) else { return };
                let mut sel = state.sel.borrow_mut();
                if sel.is_none() && head == anchor {
                    return;
                }
                let new = Selection { anchor, head, mode };
                if *sel != Some(new) {
                    *sel = Some(new);
                    drop(sel);
                    *state.drag_text.borrow_mut() = state.selected_text();
                    if let Some(w) = g.widget() {
                        w.queue_draw();
                    }
                }
            });
        }
        {
            let state = state.clone();
            drag.connect_drag_end(move |g, _, _| {
                let was_dragging = state.drag.borrow_mut().take().is_some();
                let cached = state.drag_text.borrow_mut().take();
                if !was_dragging && cached.is_none() {
                    return;
                }
                if let (Some(text), Some(w)) = (state.selected_text().or(cached), g.widget()) {
                    // Like Warp, selecting also copies: Ctrl+V pastes the last thing selected or copied.
                    w.display().primary_clipboard().set_text(&text);
                    w.clipboard().set_text(&text);
                }
            });
        }
        area.add_controller(drag);

        // Mouse wheel scrolls the scrollback.
        let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        {
            let state = state.clone();
            let area_w = area.downgrade();
            wheel.connect_scroll(move |c, _dx, dy| {
                let mut rows = (dy * WHEEL_LINES).round() as isize;
                if rows == 0 && dy != 0.0 {
                    rows = dy.signum() as isize;
                }
                let shift = c.current_event_state().contains(gtk::gdk::ModifierType::SHIFT_MASK);
                let (x, y) = state.pointer.get();
                let pos = state.cell_at(x, y).unwrap_or(Pos { row: 0, col: 0 });
                let modes = state.shared.lock().wheel_modes();
                match wheel::action(modes, shift, rows, pos.col, pos.row) {
                    wheel::Action::Scrollback(r) => state.shared.lock().scroll_delta(r),
                    wheel::Action::Send(bytes) => state.send_input(&bytes),
                }
                state.clear_selection();
                if let Some(a) = area_w.upgrade() {
                    a.queue_draw();
                }
                glib::Propagation::Stop
            });
        }
        area.add_controller(wheel);
        let motion = gtk::EventControllerMotion::new();
        {
            let state = state.clone();
            motion.connect_motion(move |_, x, y| state.pointer.set((x, y)));
        }
        area.add_controller(motion);

        // Ctrl+C copies, Ctrl+V and Ctrl+Shift+V paste, and Ctrl+Shift+C sends ETX (0x03).
        // Capture phase so they run before the key controller that feeds the PTY.
        let paste = gtk::ShortcutController::new();
        paste.set_propagation_phase(gtk::PropagationPhase::Capture);
        for trigger in ["<Control>v", "<Control><Shift>v"] {
            let state = state.clone();
            let area_w = area.downgrade();
            paste.add_shortcut(gtk::Shortcut::new(
                gtk::ShortcutTrigger::parse_string(trigger),
                Some(gtk::CallbackAction::new(move |_, _| {
                    if let Some(a) = area_w.upgrade() {
                        paste_clipboard(&state, a.upcast_ref());
                    }
                    glib::Propagation::Stop
                })),
            ));
        }
        {
            let state = state.clone();
            let area_w = area.downgrade();
            paste.add_shortcut(gtk::Shortcut::new(
                gtk::ShortcutTrigger::parse_string("<Control>c"),
                Some(gtk::CallbackAction::new(move |_, _| {
                    // Without a selection Ctrl+C does nothing: it must never reach the PTY.
                    if let (Some(a), Some(t)) = (area_w.upgrade(), state.selected_text()) {
                        a.clipboard().set_text(&t);
                    }
                    glib::Propagation::Stop
                })),
            ));
        }
        {
            // Ctrl+A stays "start of line" for the program; Ctrl+Shift+A selects the input line.
            let state = state.clone();
            let area_w = area.downgrade();
            paste.add_shortcut(gtk::Shortcut::new(
                gtk::ShortcutTrigger::parse_string("<Control><Shift>a"),
                Some(gtk::CallbackAction::new(move |_, _| {
                    if let (Some(a), Some(row)) = (area_w.upgrade(), state.cursor_row.get()) {
                        let cols = state.size.get().0;
                        let (c0, c1) = selection::line_span(cols);
                        *state.sel.borrow_mut() = Some(Selection { anchor: Pos { row, col: c0 }, head: Pos { row, col: c1 }, mode: Mode::Line });
                        state.input_sel.set(true);
                        a.queue_draw();
                    }
                    glib::Propagation::Stop
                })),
            ));
        }
        {
            let state = state.clone();
            paste.add_shortcut(gtk::Shortcut::new(
                gtk::ShortcutTrigger::parse_string("<Control><Shift>c"),
                Some(gtk::CallbackAction::new(move |_, _| {
                    state.write(&[0x03]);
                    glib::Propagation::Stop
                })),
            ));
        }
        area.add_controller(paste);

        Terminal { widget: area, local_pid, state }
    }

    pub fn queue_input_state(&self) -> (bool, u64) {
        (self.state.shared.lock().bracketed_paste(), self.state.input_revision.get())
    }

    pub fn type_text(&self, text: &str) {
        // This entry point neither submits input nor accepts terminal control sequences.
        if !text.chars().any(char::is_control) {
            self.state.write(text.as_bytes());
            self.widget.grab_focus();
        }
    }

    pub fn apply_preferences(&self, preferences: wyss_console::settings::Terminal, theme_changed: bool) {
        let state = &self.state;
        let previous = state.preferences.replace(preferences);
        if previous == preferences && !theme_changed {
            return;
        }
        {
            let mut term = state.shared.lock();
            if previous.scrollback != preferences.scrollback {
                term.set_scrollback(preferences.scrollback as usize);
                state.shared.scrollback.store(preferences.scrollback, Ordering::Relaxed);
            }
            if previous.cursor_style != preferences.cursor_style || previous.cursor_blink != preferences.cursor_blink {
                term.set_cursor(preferences.cursor_style, preferences.cursor_blink);
                state.blink_visible.set(true);
            }
            if theme_changed {
                let c = palette::colors();
                term.set_colors(c.fg, c.bg, &c.table);
            }
        }
        if previous.font_size != preferences.font_size {
            measure_font(&self.widget, state);
            if self.widget.width() > 0 && self.widget.height() > 0 {
                resize(state, self.widget.width(), self.widget.height());
            }
        }
        if theme_changed || previous.font_size != preferences.font_size {
            // Force a complete capture and rebuild of the cached GSK rows.
            state.frame.borrow_mut().size = (0, 0);
        }
        self.widget.queue_draw();
    }

    /// Sets what the pane searches for (an empty text clears the search), jumps to the newest match
    /// and returns `(matches, selected)`; `selected` counts from the newest, which is 0.
    pub fn find_set(&self, needle: &str) -> (usize, Option<usize>) {
        let s = &self.state;
        {
            let term = s.shared.lock();
            let mut find = s.find.borrow_mut();
            let search = find.get_or_insert_with(|| ghostty::Search::new(&term));
            search.set_needle(needle);
            if !needle.is_empty() {
                search.run(&term);
                search.select(true);
            }
            *s.found.borrow_mut() = search.run(&term);
        }
        self.widget.queue_draw();
        self.find_info()
    }

    /// Selects the next match toward older output (`older`) or toward newer, scrolling to it.
    pub fn find_step(&self, older: bool) -> (usize, Option<usize>) {
        let s = &self.state;
        {
            let term = s.shared.lock();
            if let Some(search) = s.find.borrow_mut().as_mut() {
                search.select(older);
                *s.found.borrow_mut() = search.run(&term);
            }
        }
        self.widget.queue_draw();
        self.find_info()
    }

    /// `(matches, selected)` as of the last frame.
    pub fn find_info(&self) -> (usize, Option<usize>) {
        let f = self.state.found.borrow();
        (f.total, f.selected)
    }

    pub fn find_close(&self) {
        {
            let _term = self.state.shared.lock(); // releases what the search tracks inside the terminal
            *self.state.find.borrow_mut() = None;
        }
        *self.state.found.borrow_mut() = ghostty::Found::default();
        self.widget.queue_draw();
    }
}

fn font_desc(size: u8) -> pango::FontDescription {
    let mut font = pango::FontDescription::from_string(FONT);
    font.set_absolute_size(f64::from(size) * f64::from(pango::SCALE));
    font
}

fn measure_font(area: &TermView, state: &State) {
    let ctx = area.pango_context();
    let metrics = ctx.metrics(Some(&font_desc(state.preferences.get().font_size)), None);
    let scale = pango::SCALE as f64;
    // The advance of a glyph, exact: `approximate_digit_width` is rounded to whole pixels (9 for 8.8), and
    // a grid wider than the text drifts away from it (selection, cursor and search highlights land
    // up to a cell off at the end of a line).
    let probe = pango::Layout::new(&ctx);
    probe.set_font_description(Some(&font_desc(state.preferences.get().font_size)));
    probe.set_text(&"0".repeat(100));
    let advance = probe.size().0 as f64 / scale / 100.0;
    state.cell_w.set(if advance > 1.0 { advance } else { metrics.approximate_digit_width() as f64 / scale });
    state.cell_h.set(((metrics.ascent() + metrics.descent()) as f64 / scale).ceil());
}

fn resize(state: &State, w: i32, h: i32) {
    let cols = (((w as f64) - 2.0 * PAD as f64) / state.cell_w.get()).floor().max(2.0) as usize;
    let lines = (((h as f64) - 2.0 * PAD as f64) / state.cell_h.get()).floor().max(1.0) as usize;
    if state.size.get() == (cols, lines) {
        return;
    }
    state.size.set((cols, lines));
    let (cw, ch) = (state.cell_w.get() as u16, state.cell_h.get() as u16);
    // A search is set aside around the resize (see `Terminal::resize_with_search`).
    state.shared.lock().resize_with_search(&mut state.find.borrow_mut(), cols as u16, lines as u16, cw as u32, ch as u32);
    match (&state.pty, &state.input) {
        (Some(p), _) => p.resize(cols as u16, lines as u16, cw, ch),
        (None, Input::Host(tx)) => {
            let _ = tx.send(Out::Resize(lines as u16, cols as u16));
        }
        _ => {}
    }
}

fn rgba(c: Rgb) -> gdk::RGBA {
    gdk::RGBA::new(c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, 1.0)
}

fn mix(a: Rgb, b: Rgb) -> Rgb {
    let m = |x: u8, y: u8| ((x as u16 + y as u16) / 2) as u8;
    Rgb { r: m(a.r, b.r), g: m(a.g, b.g), b: m(a.b, b.b) }
}

/// Text accumulated for one pango layout: narrow cells in a row share a run; each wide cell gets its
/// own, so a fallback-font glyph can never push the following cells off the grid.
struct Run {
    col: usize,
    text: String,
    attrs: pango::AttrList,
    ink: bool,
}

impl Run {
    fn new() -> Run {
        Run { col: 0, text: String::new(), attrs: pango::AttrList::new(), ink: false }
    }
}

/// Inserts `attr` into `list`, limited to the byte range `start..end`.
macro_rules! add {
    ($list:expr, $attr:expr, $start:expr, $end:expr) => {{
        let mut a = $attr;
        a.set_start_index($start as u32);
        a.set_end_index($end as u32);
        $list.insert(a);
    }};
}

/// Builds the render node of one viewport row (coordinates relative to the row's top-left corner).
/// `sel` is the inclusive column range of this row covered by the selection. Order inside the node:
/// cell backgrounds, selection, text.
fn build_row(ctx: &pango::Context, font: &pango::FontDescription, cells: &[Cell], cw: f32, ch: f32, sel: Option<(usize, usize)>) -> Option<gsk::RenderNode> {
    let snap = gtk::Snapshot::new();
    let pal = palette::colors();
    let mut run = Run::new();
    let mut bg_run: Option<(usize, Rgb)> = None; // (first column, color)

    // Text layouts wait until the backgrounds and the selection are in the node, so they draw on top.
    let mut layouts: Vec<(usize, pango::Layout)> = Vec::new();
    let flush_run = |layouts: &mut Vec<(usize, pango::Layout)>, run: &mut Run| {
        if run.ink {
            let layout = pango::Layout::new(ctx);
            layout.set_font_description(Some(font));
            layout.set_text(&run.text);
            layout.set_attributes(Some(&run.attrs));
            layouts.push((run.col, layout));
        }
        *run = Run::new();
    };
    let flush_bg = |snap: &gtk::Snapshot, bg: &mut Option<(usize, Rgb)>, end: usize| {
        if let Some((start, c)) = bg.take() {
            snap.append_color(&rgba(c), &graphene::Rect::new(PAD + start as f32 * cw, 0.0, (end - start) as f32 * cw, ch));
        }
    };

    for (col, cell) in cells.iter().enumerate() {
        let mut fg = cell.fg.unwrap_or(pal.fg);
        let mut bg = cell.bg;
        if cell.inverse {
            let f = fg;
            fg = bg.unwrap_or(pal.bg);
            bg = Some(f);
        }
        if cell.faint {
            fg = mix(fg, bg.unwrap_or(pal.bg));
        }
        // Backgrounds: merge neighbours of the same color into one rectangle.
        let bg = bg.filter(|b| *b != pal.bg);
        match (&bg_run, bg) {
            (Some((_, c)), Some(b)) if *c == b => {}
            _ => {
                flush_bg(&snap, &mut bg_run, col);
                if let Some(b) = bg {
                    bg_run = Some((col, b));
                }
            }
        }
        if cell.wide == Wide::Spacer {
            continue;
        }
        let wide = cell.wide == Wide::Double;
        if wide {
            flush_run(&mut layouts, &mut run);
        }
        if run.text.is_empty() {
            run.col = col;
        }
        let t = if cell.text().is_empty() || cell.invisible { " " } else { cell.text() };
        let start = run.text.len();
        run.text.push_str(t);
        let end = run.text.len();
        let blank = t == " " && !cell.underline && !cell.strike;
        if !blank {
            run.ink = true;
        }
        if fg != pal.fg {
            add!(run.attrs, pango::AttrColor::new_foreground(fg.r as u16 * 257, fg.g as u16 * 257, fg.b as u16 * 257), start, end);
        }
        if cell.bold {
            add!(run.attrs, pango::AttrInt::new_weight(pango::Weight::Bold), start, end);
        }
        if cell.italic {
            add!(run.attrs, pango::AttrInt::new_style(pango::Style::Italic), start, end);
        }
        if cell.underline {
            add!(run.attrs, pango::AttrInt::new_underline(pango::Underline::Single), start, end);
        }
        if cell.strike {
            add!(run.attrs, pango::AttrInt::new_strikethrough(true), start, end);
        }
        if wide {
            flush_run(&mut layouts, &mut run);
        }
    }
    flush_run(&mut layouts, &mut run);
    flush_bg(&snap, &mut bg_run, cells.len());
    if let Some((c0, c1)) = sel {
        snap.append_color(&rgba(pal.sel_bg), &graphene::Rect::new(PAD + c0 as f32 * cw, 0.0, (c1 + 1 - c0) as f32 * cw, ch));
    }
    for (col, layout) in &layouts {
        snap.save();
        snap.translate(&graphene::Point::new(PAD + *col as f32 * cw, 0.0));
        snap.append_layout(layout, &rgba(pal.fg));
        snap.restore();
    }
    snap.to_node()
}

fn draw(snap: &gtk::Snapshot, state: &State, w: f32, h: f32) {
    if std::env::var_os("WYSS_CONSOLE_NODRAW").is_some() {
        return;
    }
    let pal = palette::colors();
    snap.append_color(&rgba(pal.bg), &graphene::Rect::new(0.0, 0.0, w, h));
    let cw = state.cell_w.get() as f32;
    let ch = state.cell_h.get() as f32;

    // Clear the request flag before capturing, so output arriving from here on asks for a new frame.
    state.shared.pending.store(false, Ordering::Release);
    let mut frame = state.frame.borrow_mut();
    let Frame { rs, rows, grid, dirty, painted, size } = &mut *frame;
    {
        let term = state.shared.lock();
        // A search follows the terminal: caught up here, under the same lock that captures the
        // frame, because its matches are only valid until the terminal changes again.
        if let Some(search) = state.find.borrow_mut().as_mut() {
            *state.found.borrow_mut() = search.run(&term);
        }
        rs.update(&term);
    }
    let new_size = rs.size();
    let all = *size != new_size;
    if all {
        *size = new_size;
        rows.clear();
        rows.resize(new_size.1 as usize, None);
        grid.clear();
        grid.resize_with(new_size.1 as usize, Vec::new);
        dirty.resize(new_size.1 as usize, false);
        *painted = None;
    }

    // Copy the rows ghostty marks dirty (every row after a resize) into the grid, then mark clean.
    dirty.fill(false);
    if all || rs.dirty() != Dirty::Clean {
        rs.for_rows(all, |y, cells| {
            if let Some(g) = grid.get_mut(y as usize) {
                g.clear();
                g.extend_from_slice(cells);
                dirty[y as usize] = true;
            }
        });
        rs.clean();
    }

    // A selection whose rows changed no longer describes what is on screen: drop it.
    let new_sel = {
        let mut sel = state.sel.borrow_mut();
        if let Some(s) = *sel {
            let (a, b) = s.bounds(grid);
            if all || dirty.iter().enumerate().any(|(r, d)| *d && r >= a.row && r <= b.row) {
                *sel = None;
                *state.drag.borrow_mut() = None;
                state.input_sel.set(false);
            }
        }
        sel.map(|s| s.bounds(grid))
    };

    // Rebuild the nodes of changed rows and of rows whose selection highlight changed.
    let sel_changed = *painted != new_sel;
    let covers = |b: &Option<(Pos, Pos)>, r: usize| b.is_some_and(|(a, z)| r >= a.row && r <= z.row);
    if let Some(ctx) = state.pango.borrow().clone() {
        let font = font_desc(state.preferences.get().font_size);
        for y in 0..rows.len() {
            if dirty[y] || (sel_changed && (covers(painted, y) || covers(&new_sel, y))) {
                let cols = new_sel.and_then(|(a, z)| selection::row_range(a, z, y, grid[y].len()));
                rows[y] = build_row(&ctx, &font, &grid[y], cw, ch, cols);
            }
        }
        *painted = new_sel;
    }

    for (y, node) in rows.iter().enumerate() {
        if let Some(n) = node {
            snap.save();
            snap.translate(&graphene::Point::new(0.0, PAD + y as f32 * ch));
            snap.append_node(n);
            snap.restore();
        }
    }

    // Matches of the search: a translucent overlay, brighter on the selected one.
    {
        let found = state.found.borrow();
        let (cols, nrows) = (grid.first().map_or(0, |r| r.len()), grid.len());
        let paint = |m: ghostty::Span, color: Rgb, alpha: f32| {
            let c = gdk::RGBA::new(color.r as f32 / 255.0, color.g as f32 / 255.0, color.b as f32 / 255.0, alpha);
            for (row, c0, c1) in crate::search::row_spans(m, nrows, cols) {
                snap.append_color(&c, &graphene::Rect::new(PAD + c0 as f32 * cw, PAD + row as f32 * ch, (c1 + 1 - c0) as f32 * cw, ch));
            }
        };
        for m in &found.visible {
            if Some(*m) != found.current {
                paint(*m, pal.find, 0.35);
            }
        }
        if let Some(m) = found.current {
            paint(m, pal.find_current, 0.85);
        }
    }

    // Cursor, in the style the application asked for.
    let cur = rs.cursor();
    state.cursor_blinks.set(cur.visible && cur.pos.is_some() && cur.blinking);
    state.cursor_row.set(cur.pos.map(|(_, y)| y as usize));
    if let (true, Some((cx, cy))) = (cur.visible && (!cur.blinking || state.blink_visible.get()), cur.pos) {
        let (cx, span) = if cur.wide_tail { (cx.saturating_sub(1), 2.0) } else { (cx, 1.0) };
        let (x, y, cwid) = (PAD + cx as f32 * cw, PAD + cy as f32 * ch, cw * span);
        let col = gdk::RGBA::new(pal.cursor[0], pal.cursor[1], pal.cursor[2], pal.cursor[3]);
        match cur.style {
            CursorStyle::Block => snap.append_color(&col, &graphene::Rect::new(x, y, cwid, ch)),
            CursorStyle::Bar => snap.append_color(&col, &graphene::Rect::new(x, y, 2.0, ch)),
            CursorStyle::Underline => snap.append_color(&col, &graphene::Rect::new(x, y + ch - 2.0, cwid, 2.0)),
            CursorStyle::BlockHollow => {
                snap.append_color(&col, &graphene::Rect::new(x, y, cwid, 1.0));
                snap.append_color(&col, &graphene::Rect::new(x, y + ch - 1.0, cwid, 1.0));
                snap.append_color(&col, &graphene::Rect::new(x, y, 1.0, ch));
                snap.append_color(&col, &graphene::Rect::new(x + cwid - 1.0, y, 1.0, ch));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn busy_reads_share_the_transport_backoff() {
        let busy = host::HostError::Remote { code: "busy".into(), reasons: vec![] };
        let closed = host::HostError::Remote { code: "terminal_unknown".into(), reasons: vec![] };
        let transport = host::HostError::Io(std::io::ErrorKind::BrokenPipe.into());
        for (failures, ms) in [(1, 100), (2, 200), (8, 2000)] {
            assert_eq!(retry_read_error(&busy, failures), Some(Duration::from_millis(ms)));
            assert_eq!(retry_read_error(&busy, failures), retry_read_error(&transport, failures));
            assert_eq!(retry_read_error(&closed, failures), None);
        }
    }

    #[test]
    fn ctrl_v_pastes_text_or_forwards_an_image() {
        assert_eq!(clipboard_paste(&["text/plain;charset=utf-8", "text/plain"]), ClipboardPaste::Text);
        assert_eq!(clipboard_paste(&["text/plain", "image/png"]), ClipboardPaste::Text, "text wins when both are there");
        assert_eq!(clipboard_paste(&["image/png", "application/x-kde-suggestedfilename"]), ClipboardPaste::ForwardImage);
        assert_eq!(clipboard_paste(&["UTF8_STRING"]), ClipboardPaste::Text);
        assert_eq!(clipboard_paste(&["text/html", "image/png"]), ClipboardPaste::ForwardImage, "an image copied from a page");
        assert_eq!(clipboard_paste(&[]), ClipboardPaste::Nothing);
    }

    #[test]
    fn clipboard_uses_one_dec_2004_snapshot() {
        let reads = std::cell::Cell::new(0);
        let (confirm, bytes) = prepare_paste("line\n", || {
            reads.set(reads.get() + 1);
            reads.get() == 1
        });
        assert_eq!(reads.get(), 1);
        assert!(!confirm);
        assert_eq!(bytes, b"\x1b[200~line\n\x1b[201~");
        let (confirm, bytes) = prepare_paste("line\n", || false);
        assert!(confirm);
        assert_eq!(bytes, b"line\r");
    }

    use super::*;

    #[test]
    fn font_size_is_applied_as_absolute_pixels() {
        for size in [10, 16, 24] {
            let font = font_desc(size);
            assert!(font.is_size_absolute());
            assert_eq!(font.size(), i32::from(size) * pango::SCALE);
        }
    }

    #[test]
    fn a_paste_cannot_close_the_bracket_or_smuggle_controls() {
        let evil = "a\x1b[201~rm -rf ~\n";
        let b = paste_bytes(evil, true);
        let s = String::from_utf8(b.clone()).unwrap();
        assert_eq!(s.matches(PASTE_END).count(), 1, "{s:?}");
        assert!(s.ends_with(PASTE_END) && s.starts_with(PASTE_START), "{s:?}");
        assert_eq!(s, "\x1b[200~arm -rf ~\n\x1b[201~");
        let plain = paste_bytes(evil, false);
        assert!(!plain.contains(&0x1b), "{plain:?}");
        assert_eq!(plain, b"arm -rf ~\r");
        // A marker split by another marker does not reassemble.
        let nested = paste_bytes("\x1b[20\x1b[201~1~x", true);
        assert_eq!(String::from_utf8(nested).unwrap().matches(PASTE_END).count(), 1);
        assert_eq!(paste_bytes("x\u{9b}201~\x07\x7fy", false), b"x201~y");
    }

    #[test]
    fn a_paste_with_any_line_end_asks_unless_bracketed() {
        assert!(paste_needs_confirmation("ls\nrm -rf x", false));
        assert!(paste_needs_confirmation("a\rb", false));
        assert!(paste_needs_confirmation("a\r\nb\r\n", false));
        assert!(!paste_needs_confirmation("a\nb", true));
        assert!(!paste_needs_confirmation("one line", false));
        assert!(paste_needs_confirmation("curl x | sh\n", false));
        assert!(!paste_needs_confirmation("curl x | sh\n", true));
        assert!(!paste_needs_confirmation("", false));
        assert_eq!(paste_lines("a\r\nb\nc\rd\n\n"), ["a", "b", "c", "d"]);
    }

    #[test]
    fn the_reader_retries_transport_errors_with_a_backoff_that_stops_at_two_seconds() {
        let waits: Vec<u64> = (1..=8).map(|n| retry_backoff(n).as_millis() as u64).collect();
        assert_eq!(waits, [100, 200, 400, 800, 1600, 2000, 2000, 2000]);
        assert_eq!(retry_backoff(u32::MAX), Duration::from_millis(2000));
    }

    #[test]
    fn a_normal_paste_is_untouched_apart_from_line_ends() {
        let text = "café ñandú 🦀\tfin";
        assert_eq!(paste_bytes(text, false), text.as_bytes());
        assert_eq!(paste_bytes(text, true), format!("{PASTE_START}{text}{PASTE_END}").into_bytes());
        assert_eq!(paste_bytes("uno\r\ndos\ntres", false), b"uno\rdos\rtres");
        assert_eq!(paste_bytes("uno\ndos", true), b"\x1b[200~uno\ndos\x1b[201~");
    }
}
