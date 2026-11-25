//! The Console window: icon rail, tab bar, a tree of panes (GtkPaned) whose leaves are terminals
//! of the host, a status bar. The logic that does not need a widget lives in `layout`, `keymap`
//! and `procinfo`; this file only wires them to GTK.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};

use crate::activity;
use crate::host::Host;
use crate::i18n::{tr, trf};
use crate::icons::{self, Icon};
use crate::keymap::{self, Action, Focus};
use crate::layout::{self, Dir, Node, SavedLayout, SavedTab};
use crate::procinfo::{self, Git};
use crate::project_panel::{self, Panel};
use crate::term;
use crate::{launches, native, search, settings, t, theme};
use wyss_console::settings::{Preferences, SettingsService};

const THEME: &str = include_str!("theme.css");
/// Set (`WYSS_CONSOLE_DEMO=launch-menu,search:text`) to open the menu or the search
/// by itself, so a screenshot can show those states without a keyboard.
const DEMO_VAR: &str = "WYSS_CONSOLE_DEMO";
const REFRESH_SECS: u32 = 2;

fn can_send_panel_request(cached_running: bool, verify: impl FnOnce() -> bool) -> bool {
    cached_running && verify()
}

#[test]
fn panel_security_ask_rechecks_cached_running_state() {
    let queried = Cell::new(false);
    assert!(!can_send_panel_request(true, || {
        queried.set(true);
        false
    }), "a stale Running state would send terminal input");
    assert!(queried.get());
    assert!(can_send_panel_request(true, || true));
}

fn review_project_instructions(
    launch: &mut wyss_console::native::arsenal::ArsenalLaunch,
    provider: &str,
    directory: &str,
    restart: bool,
) -> Result<bool, String> {
    crate::project_instructions::select(launch, provider, directory, restart, |review| {
        glib::MainContext::default().invoke(move || UI.with(|ui| {
            if let Some(ui) = ui.borrow().clone() { review.show(&ui.win); }
        }));
    })
}

pub struct Config {
    pub host: Option<Host>,
    /// What the command line asked for (tabs, folder, program, no terminal).
    pub request: crate::cli::Request,
    /// Folder the caller was in, for a new tab without `--workdir`.
    pub cwd: Option<String>,
}

/// A later `wyss-console …` while the window is open: a new tab in it, then the window to the front.
pub fn handle(request: &crate::cli::Request, cwd: Option<String>) -> bool {
    let Some(ui) = UI.with(|ui| ui.borrow().clone()) else { return false };
    ui.open_request(request, cwd);
    ui.win.present();
    true
}

/// What the periodic read of /proc learned about a pane.
#[derive(Clone, Default)]
struct Info {
    known: bool,
    running: bool,
    shell: String,
    pid: Option<i32>,
    rss: Option<u64>,
    cwd: Option<std::path::PathBuf>,
    git: Option<Git>,
}

/// The search bar of a pane (Ctrl+Shift+F).
struct FindBar {
    bar: gtk::Box,
    entry: gtk::Entry,
    count: gtk::Label,
}

struct Pane {
    key: String,
    tid: Option<String>,
    id: u64,
    term: term::Terminal,
    root: gtk::Box,
    /// The pane's × (closes just that pane); shown in the top bar while the tab is split.
    actions: gtk::Box,
    find: FindBar,
    info: RefCell<Info>,
    /// Set when the terminal is an agent's: what the host says about its session.
    native: RefCell<Option<native::Tag>>,
    /// What the session's state offers (stop and restart, or start again and forget); `None` until
    /// the first read of `native_sessions`, and for a start that is still being prepared.
    life: Cell<Option<native::Lifecycle>>,
}

struct Tab {
    n: u64,
    tree: Node,
    focus: String,
    body: gtk::Box,
    head: gtk::Box,
    ram: gtk::Label,
    count: gtk::Label,
}

struct Job {
    key: String,
    tid: Option<String>,
    local_pid: Option<u32>,
    known: Option<(i32, String)>,
}

struct Res {
    key: String,
    info: Info,
}

struct Ui {
    win: gtk::ApplicationWindow,
    stack: gtk::Stack,
    project: Rc<Panel>,
    tab_strip: gtk::Box,
    tab_scroll: gtk::ScrolledWindow,
    tab_more: gtk::Button,
    tab_more_pop: gtk::Popover,
    folder: gtk::Label,
    git: gtk::Label,
    kind: gtk::Label,
    pane_actions: gtk::Box,
    launch_btn: gtk::Button,
    launch_panel: gtk::Box,
    banner: gtk::Box,
    banner_text: gtk::Label,
    banner_gen: Cell<u64>,
    opening: Cell<bool>,
    tabs: RefCell<Vec<Tab>>,
    active: Cell<usize>,
    panes: RefCell<HashMap<String, Rc<Pane>>>,
    by_id: RefCell<HashMap<u64, String>>,
    next_id: Cell<u64>,
    next_n: Cell<u64>,
    host: Option<Host>,
    home: Option<String>,
    refreshing: Cell<bool>,
    activity_refreshing: Cell<bool>,
    activity_strip: activity::Strip,
    activity: RefCell<wyss_console::native::service::ActivityView>,
    save_pending: Cell<bool>,
    terminal_preferences: Cell<wyss_console::settings::Terminal>,
}

thread_local! {
    /// The worker thread hands its results back through the main context, which reaches the UI here.
    static UI: RefCell<Option<Rc<Ui>>> = const { RefCell::new(None) };
}

fn dismantle(w: &gtk::Widget) {
    if let Some(p) = w.downcast_ref::<gtk::Paned>() {
        let (a, b) = (p.start_child(), p.end_child());
        p.set_start_child(None::<&gtk::Widget>);
        p.set_end_child(None::<&gtk::Widget>);
        for c in [a, b].into_iter().flatten() {
            dismantle(&c);
        }
    }
}

fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class(class);
    l
}

impl Ui {
    // ---- panes ----------------------------------------------------------------------------

    fn new_pane(self: &Rc<Self>, source: term::Source, tid: Option<String>, tag: Option<native::Tag>) -> Rc<Pane> {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let key = tid.clone().unwrap_or_else(|| format!("local-{id}"));
        let weak = Rc::downgrade(self);
        let t = term::Terminal::new(id, source, self.terminal_preferences.get(), move || {
            if let Some(ui) = weak.upgrade() {
                ui.on_exit(id);
            }
        });

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("pane");
        root.set_overflow(gtk::Overflow::Hidden);
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        header.add_css_class("pane-actions");
        let pane_close = self.close_button(&key);
        header.append(&pane_close);

        // Search bar (Ctrl+Shift+F), closed until asked for.
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        bar.add_css_class("find-bar");
        bar.set_visible(false);
        let entry = gtk::Entry::new();
        entry.set_placeholder_text(Some(&tr("Search…")));
        entry.set_tooltip_text(Some(&tr("Search this terminal")));
        let count = label("", "find-count");
        count.set_halign(gtk::Align::Start);
        let prev = gtk::Button::with_label("↑");
        prev.set_tooltip_text(Some(&tr("Previous match")));
        let next = gtk::Button::with_label("↓");
        next.set_tooltip_text(Some(&tr("Next match")));
        let close = gtk::Button::with_label("×");
        close.set_tooltip_text(Some(&tr("Close search")));
        for b in [&prev, &next, &close] {
            b.set_focusable(false);
        }
        for w in [entry.upcast_ref::<gtk::Widget>(), count.upcast_ref(), prev.upcast_ref(), next.upcast_ref(), close.upcast_ref()] {
            bar.append(w);
        }
        root.append(&bar);
        t.widget.set_margin_start(8);
        t.widget.set_margin_top(6);
        root.append(&t.widget);

        let focus = gtk::EventControllerFocus::new();
        {
            let weak = Rc::downgrade(self);
            let key = key.clone();
            focus.connect_enter(move |_| {
                if let Some(ui) = weak.upgrade() {
                    ui.focus_changed(&key);
                }
            });
        }
        t.widget.add_controller(focus);

        let pane = Rc::new(Pane {
            key: key.clone(),
            tid,
            id,
            term: t,
            root,
            actions: header,
            find: FindBar { bar, entry, count },
            info: RefCell::new(Info { running: true, ..Info::default() }),
            native: RefCell::new(tag),
            life: Cell::new(None),
        });
        // The search bar talks to this pane only; a weak reference keeps the widgets from holding it alive.
        {
            let w = Rc::downgrade(&pane);
            let ui = Rc::downgrade(self);
            let wc = w.clone();
            pane.find.entry.connect_changed(move |e| {
                if let Some(p) = wc.upgrade() {
                    let info = p.term.find_set(e.text().as_str());
                    Ui::paint_count(&p, info);
                }
            });
            for (btn, older) in [(&prev, false), (&next, true)] {
                let w = w.clone();
                btn.connect_clicked(move |_| {
                    if let Some(p) = w.upgrade() {
                        let info = p.term.find_step(older);
                        Ui::paint_count(&p, info);
                    }
                });
            }
            {
                let (w, ui) = (w.clone(), ui.clone());
                close.connect_clicked(move |_| {
                    if let (Some(p), Some(ui)) = (w.upgrade(), ui.upgrade()) {
                        ui.close_search(&p);
                    }
                });
            }
            let keys = gtk::EventControllerKey::new();
            keys.set_propagation_phase(gtk::PropagationPhase::Capture);
            keys.connect_key_pressed(move |_, key, _, mods| {
                let Some(p) = w.upgrade() else { return glib::Propagation::Proceed };
                match key {
                    gdk::Key::Escape => {
                        if let Some(ui) = ui.upgrade() {
                            ui.close_search(&p);
                        }
                        glib::Propagation::Stop
                    }
                    gdk::Key::Return | gdk::Key::KP_Enter => {
                        // Enter goes to the next match (older), Shift+Enter to the previous.
                        let info = p.term.find_step(!mods.contains(gdk::ModifierType::SHIFT_MASK));
                        Ui::paint_count(&p, info);
                        glib::Propagation::Stop
                    }
                    _ => glib::Propagation::Proceed,
                }
            });
            pane.find.entry.add_controller(keys);
        }
        self.panes.borrow_mut().insert(key.clone(), pane.clone());
        self.by_id.borrow_mut().insert(id, key);
        pane
    }

    /// The × of a pane: closes just that pane.
    fn close_button(self: &Rc<Self>, key: &str) -> gtk::Button {
        let b = gtk::Button::with_label("×");
        b.add_css_class("hdr-btn");
        b.add_css_class("hdr-close");
        b.set_tooltip_text(Some(&tr("Close the pane")));
        b.set_valign(gtk::Align::Center);
        b.set_focusable(false);
        let (weak, key) = (Rc::downgrade(self), key.to_owned());
        b.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.remove_pane(&key, true);
            }
        });
        b
    }

    fn paint_count(p: &Pane, (total, selected): (usize, Option<usize>)) {
        let empty = p.find.entry.text().is_empty();
        let text = match search::counter(empty, total, selected) {
            None if empty => String::new(),
            None => tr("No matches"),
            Some((current, total)) => trf("{current} of {total}{suffix}", &[("current", &current.to_string()), ("total", &total.to_string()), ("suffix", if total >= 5000 { "+" } else { "" })]),
        };
        p.find.count.set_text(&text);
    }

    /// Ctrl+Shift+F: shows the search bar of the pane with the focus and puts the cursor in it.
    fn open_search(&self, key: &str) {
        let Some(p) = self.pane(key) else { return };
        p.find.bar.set_visible(true);
        p.find.entry.grab_focus();
        p.find.entry.select_region(0, -1);
    }

    fn close_search(&self, p: &Pane) {
        p.term.find_close();
        p.find.entry.set_text("");
        p.find.count.set_text("");
        p.find.bar.set_visible(false);
        self.focus_pane(&p.key);
    }

    fn on_exit(self: &Rc<Self>, id: u64) {
        let key = self.by_id.borrow().get(&id).cloned();
        if let Some(key) = key {
            // An agent that ended keeps its pane, so its last output can still be read; it closes with its tab.
            if let Some(p) = self.pane(&key).filter(|p| p.native.borrow().is_some()) {
                p.info.borrow_mut().running = false;
                self.refresh_soon();
                return;
            }
            // The shell ended: the host already dropped it.
            self.remove_pane(&key, false);
        }
    }

    fn source(&self) -> Option<(term::Source, Option<String>)> {
        let Some(host) = &self.host else { return Some((term::Source::Local, None)) };
        match host.create(24, 80) {
            Ok(tid) => Some((term::Source::Host { host: host.clone(), tid: tid.clone(), attach: term::Attach::Fresh }, Some(tid))),
            Err(e) => {
                eprintln!("wyss-console: term_create failed: {e}");
                None
            }
        }
    }

    fn pane(&self, key: &str) -> Option<Rc<Pane>> {
        self.panes.borrow().get(key).cloned()
    }

    fn tab_of(&self, key: &str) -> Option<usize> {
        self.tabs.borrow().iter().position(|t| layout::has(&t.tree, key))
    }

    // ---- tabs -----------------------------------------------------------------------------

    fn add_tab(self: &Rc<Self>, n: u64, tree: Node, focus: String) {
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        head.add_css_class("tab");
        head.set_valign(gtk::Align::Fill);
        let tab_name = format!("{} {n}", tr("Terminal"));
        let name = label(&tab_name, "tab-name");
        let ram = label("", "tab-ram");
        let count = label("", "tab-panes");
        ram.set_visible(false);
        count.set_visible(false);
        let close = gtk::Button::with_label("×");
        close.add_css_class("tab-close");
        close.set_valign(gtk::Align::Center);
        close.set_tooltip_text(Some(&trf("Close {name}", &[("name", &tab_name)])));
        close.set_focusable(false);
        for w in [name.upcast_ref::<gtk::Widget>(), count.upcast_ref(), ram.upcast_ref(), close.upcast_ref()] {
            head.append(w);
        }
        let click = gtk::GestureClick::new();
        {
            let weak = Rc::downgrade(self);
            click.connect_pressed(move |g, _, _, _| {
                if g.current_button() != 1 {
                    return;
                }
                if let Some(ui) = weak.upgrade()
                    && let Some(i) = ui.tabs.borrow().iter().position(|t| t.n == n)
                {
                    let ui = ui.clone();
                    glib::idle_add_local_once(move || ui.select(i));
                }
            });
        }
        head.add_controller(click);
        {
            let weak = Rc::downgrade(self);
            close.connect_clicked(move |_| {
                if let Some(ui) = weak.upgrade() {
                    let i = ui.tabs.borrow().iter().position(|t| t.n == n);
                    if let Some(i) = i {
                        ui.close_tab(i);
                    }
                }
            });
        }
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.add_css_class("body");
        self.stack.add_named(&body, Some(&format!("t{n}")));
        self.tab_strip.append(&head);
        self.tabs.borrow_mut().push(Tab { n, tree, focus, body, head, ram, count });
        let i = self.tabs.borrow().len() - 1;
        self.rebuild(i);
    }

    fn build(self: &Rc<Self>, node: &Node, path: &str, tab_n: u64) -> gtk::Widget {
        match node {
            Node::Leaf(k) => self.pane(k).map(|p| p.root.clone().upcast()).unwrap_or_else(|| gtk::Box::new(gtk::Orientation::Vertical, 0).upcast()),
            Node::Split { dir, ratio, a, b } => {
                let orient = if *dir == Dir::Row { gtk::Orientation::Horizontal } else { gtk::Orientation::Vertical };
                let p = gtk::Paned::new(orient);
                p.set_wide_handle(true);
                p.set_start_child(Some(&self.build(a, &format!("{path}a"), tab_n)));
                p.set_end_child(Some(&self.build(b, &format!("{path}b"), tab_n)));
                p.set_resize_start_child(true);
                p.set_resize_end_child(true);
                p.set_shrink_start_child(false);
                p.set_shrink_end_child(false);
                p.set_hexpand(true);
                p.set_vexpand(true);
                // Once it has a size: start at the saved share, then follow the divider.
                let ready = Rc::new(Cell::new(false));
                {
                    let (ready, ratio) = (ready.clone(), *ratio);
                    p.add_tick_callback(move |p, _| {
                        let size = if p.orientation() == gtk::Orientation::Horizontal { p.width() } else { p.height() };
                        if size <= 0 {
                            return glib::ControlFlow::Continue;
                        }
                        p.set_position((size as f64 * ratio).round() as i32);
                        ready.set(true);
                        glib::ControlFlow::Break
                    });
                }
                let weak = Rc::downgrade(self);
                let path = path.to_owned();
                p.connect_notify_local(Some("position"), move |p, _| {
                    if !ready.get() {
                        return;
                    }
                    let size = if p.orientation() == gtk::Orientation::Horizontal { p.width() } else { p.height() };
                    if size <= 0 {
                        return;
                    }
                    if let Some(ui) = weak.upgrade() {
                        ui.set_ratio(tab_n, &path, p.position() as f64 / size as f64);
                    }
                });
                p.upcast()
            }
        }
    }

    fn set_ratio(self: &Rc<Self>, tab_n: u64, path: &str, ratio: f64) {
        {
            let mut tabs = self.tabs.borrow_mut();
            let Some(t) = tabs.iter_mut().find(|t| t.n == tab_n) else { return };
            t.tree = layout::resize(&t.tree, path, ratio);
        }
        self.save_soon();
    }

    /// Builds the widget tree of a tab again from its layout tree.
    fn rebuild(self: &Rc<Self>, i: usize) {
        let (n, tree, body) = {
            let tabs = self.tabs.borrow();
            let t = &tabs[i];
            (t.n, t.tree.clone(), t.body.clone())
        };
        if let Some(old) = body.first_child() {
            dismantle(&old);
            body.remove(&old);
        }
        body.append(&self.build(&tree, "", n));
        self.refresh_tab_labels(i);
        self.mark_focus(i);
    }

    fn mark_focus(&self, i: usize) {
        let (tree, focus) = {
            let tabs = self.tabs.borrow();
            (tabs[i].tree.clone(), tabs[i].focus.clone())
        };
        let many = layout::leaves(&tree).len() > 1;
        for k in layout::leaves(&tree) {
            if let Some(p) = self.pane(&k) {
                if many && k == focus {
                    p.root.add_css_class("focused");
                } else {
                    p.root.remove_css_class("focused");
                }
            }
        }
    }

    fn refresh_tab_labels(&self, i: usize) {
        let tabs = self.tabs.borrow();
        let t = &tabs[i];
        let keys = layout::leaves(&t.tree);
        t.count.set_text(&format!("▦ {}", keys.len()));
        t.count.set_visible(keys.len() >= 2);
        let rss: Vec<Option<u64>> = keys.iter().map(|k| self.pane(k).and_then(|p| p.info.borrow().rss)).collect();
        if rss.iter().any(Option::is_some) {
            t.ram.set_text(&procinfo::mb(Some(rss.iter().flatten().sum())));
            t.ram.set_visible(true);
        } else {
            t.ram.set_visible(false);
        }
    }

    /// Indices of the tabs that are not fully inside the visible part of the strip.
    fn hidden_tabs(&self) -> Vec<usize> {
        let adj = self.tab_scroll.hadjustment();
        if adj.page_size() <= 0.0 {
            return Vec::new();
        }
        let (lo, hi) = (adj.value(), adj.value() + adj.page_size());
        let tabs = self.tabs.borrow();
        (0..tabs.len())
            .filter(|&i| {
                tabs[i].head.compute_bounds(&self.tab_strip).is_some_and(|b| {
                    f64::from(b.x()) < lo - 1.0 || f64::from(b.x() + b.width()) > hi + 1.0
                })
            })
            .collect()
    }

    fn show_tab_more(self: &Rc<Self>) {
        let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
        for i in self.hidden_tabs() {
            let name = self.tabs.borrow()[i]
                .head
                .first_child()
                .and_downcast::<gtk::Label>()
                .map(|l| l.label().to_string())
                .unwrap_or_default();
            let item = gtk::Button::with_label(&name);
            item.add_css_class("tab-more-item");
            if let Some(l) = item.child().and_downcast::<gtk::Label>() {
                l.set_xalign(0.0);
            }
            let weak = Rc::downgrade(self);
            item.connect_clicked(move |_| {
                if let Some(ui) = weak.upgrade() {
                    ui.tab_more_pop.popdown();
                    let ui = ui.clone();
                    glib::idle_add_local_once(move || ui.select(i));
                }
            });
            list.append(&item);
        }
        self.tab_more_pop.set_child(Some(&list));
        self.tab_more_pop.popup();
    }

    fn select(self: &Rc<Self>, i: usize) {
        let (n, focus) = {
            let tabs = self.tabs.borrow();
            let Some(t) = tabs.get(i) else { return };
            (t.n, t.focus.clone())
        };
        self.active.set(i);
        for (j, t) in self.tabs.borrow().iter().enumerate() {
            if j == i {
                t.head.add_css_class("active");
            } else {
                t.head.remove_css_class("active");
            }
        }
        self.stack.set_visible_child_name(&format!("t{n}"));
        if let Some(t) = self.tabs.borrow().get(i) {
            let (head, scroll, strip) = (t.head.clone(), self.tab_scroll.clone(), self.tab_strip.clone());
            // Low priority: runs after the redraw, so a tab just added already has its allocation.
            glib::idle_add_local_full(glib::Priority::LOW, move || {
                if let Some(b) = head.compute_bounds(&strip) {
                    let adj = scroll.hadjustment();
                    adj.set_value((b.x() + b.width() / 2.0) as f64 - adj.page_size() / 2.0);
                }
                glib::ControlFlow::Break
            });
        }
        self.update_status();
        self.focus_pane(&focus);
        self.save_soon();
    }

    fn focus_pane(&self, key: &str) {
        if let Some(p) = self.pane(key) {
            let w = p.term.widget.clone();
            glib::idle_add_local_once(move || {
                w.grab_focus();
            });
        }
    }

    fn focus_changed(&self, key: &str) {
        let Some(i) = self.tab_of(key) else { return };
        self.tabs.borrow_mut()[i].focus = key.to_owned();
        self.mark_focus(i);
        if i == self.active.get() {
            self.update_status();
        }
    }

    fn close_tab(self: &Rc<Self>, i: usize) {
        let keys = match self.tabs.borrow().get(i) {
            Some(t) => layout::leaves(&t.tree),
            None => return,
        };
        for k in keys {
            self.remove_pane(&k, true);
        }
    }

    fn remove_tab_at(self: &Rc<Self>, i: usize) {
        let t = self.tabs.borrow_mut().remove(i);
        if let Some(old) = t.body.first_child() {
            dismantle(&old);
            t.body.remove(&old);
        }
        self.stack.remove(&t.body);
        self.tab_strip.remove(&t.head);
        let left = self.tabs.borrow().len();
        if left == 0 {
            self.save_now();
            self.win.close();
            return;
        }
        let was = self.active.get();
        let now = if i < was || (i == was && i >= left) { was.saturating_sub(1) } else { was };
        self.select(now.min(left - 1));
    }

    fn remove_pane(self: &Rc<Self>, key: &str, close_in_host: bool) {
        let Some(i) = self.tab_of(key) else { return };
        let old = self.tabs.borrow()[i].tree.clone();
        let next_focus = layout::cycle(&old, key, 1).filter(|k| k != key);
        let pane = self.panes.borrow_mut().remove(key);
        if let Some(p) = &pane {
            self.by_id.borrow_mut().remove(&p.id);
            if close_in_host && let (Some(h), Some(tid)) = (self.host.clone(), p.tid.clone()) {
                std::thread::spawn(move || {
                    let _ = h.close(&tid);
                });
            }
        }
        match layout::remove(&old, key) {
            None => {
                // Detach the pane before its tab goes away.
                self.tabs.borrow_mut()[i].tree = Node::Leaf(String::new());
                self.remove_tab_at(i);
            }
            Some(tree) => {
                let was_focus = self.tabs.borrow()[i].focus == key;
                {
                    let mut tabs = self.tabs.borrow_mut();
                    tabs[i].tree = tree;
                    if was_focus {
                        tabs[i].focus = next_focus.clone().unwrap_or_default();
                    }
                }
                self.rebuild(i);
                if i == self.active.get() {
                    if let Some(k) = next_focus.filter(|_| was_focus) {
                        self.focus_pane(&k);
                    }
                    self.update_status();
                }
                self.save_soon();
            }
        }
    }

    // ---- actions --------------------------------------------------------------------------

    fn new_tab(self: &Rc<Self>) {
        let Some((src, tid)) = self.source() else { return };
        let pane = self.new_pane(src, tid, None);
        let n = self.next_n.get();
        self.next_n.set(n + 1);
        self.add_tab(n, layout::leaf(&pane.key), pane.key.clone());
        let last = self.tabs.borrow().len() - 1;
        self.select(last);
        self.refresh_soon();
    }

    fn split(self: &Rc<Self>, dir: Dir) {
        let i = self.active.get();
        let (tree, focus) = {
            let tabs = self.tabs.borrow();
            let Some(t) = tabs.get(i) else { return };
            (t.tree.clone(), t.focus.clone())
        };
        let Some((src, tid)) = self.source() else { return };
        let pane = self.new_pane(src, tid, None);
        let new_tree = layout::split(&tree, &focus, &pane.key, dir);
        if new_tree == tree {
            // The tab is full: the terminal made for it goes away again.
            self.panes.borrow_mut().remove(&pane.key);
            self.by_id.borrow_mut().remove(&pane.id);
            if let (Some(h), Some(tid)) = (self.host.clone(), pane.tid.clone()) {
                std::thread::spawn(move || {
                    let _ = h.close(&tid);
                });
            }
            return;
        }
        {
            let mut tabs = self.tabs.borrow_mut();
            tabs[i].tree = new_tree;
            tabs[i].focus = pane.key.clone();
        }
        self.rebuild(i);
        self.focus_pane(&pane.key);
        self.save_soon();
        self.refresh_soon();
    }

    fn do_action(self: &Rc<Self>, a: Action) {
        let i = self.active.get();
        match a {
            Action::NewTab => self.new_tab(),
            Action::Project => self.toggle_project(),
            Action::Search => {
                let focus = self.tabs.borrow().get(i).map(|t| t.focus.clone());
                if let Some(k) = focus {
                    self.open_search(&k);
                }
            }
            Action::Split(d) => self.split(d),
            Action::ClosePane => {
                let focus = self.tabs.borrow().get(i).map(|t| t.focus.clone());
                if let Some(k) = focus {
                    self.remove_pane(&k, true);
                }
            }
            Action::Focus(f) => {
                let (tree, focus) = match self.tabs.borrow().get(i) {
                    Some(t) => (t.tree.clone(), t.focus.clone()),
                    None => return,
                };
                let to = match f {
                    Focus::Way(w) => layout::neighbour(&tree, &focus, w),
                    Focus::Next => layout::cycle(&tree, &focus, 1),
                    Focus::Previous => layout::cycle(&tree, &focus, -1),
                };
                if let Some(k) = to {
                    self.focus_pane(&k);
                }
            }
            Action::Tab(step) => {
                let n = self.tabs.borrow().len() as i32;
                if n > 0 {
                    self.select((i as i32 + step).rem_euclid(n) as usize);
                }
            }
            Action::Jump(n) => {
                if n >= 1 && n <= self.tabs.borrow().len() {
                    self.select(n - 1);
                }
            }
        }
    }

    fn focused(&self) -> Option<Rc<Pane>> {
        self.tabs.borrow().get(self.active.get()).and_then(|t| self.pane(&t.focus))
    }

    fn focus_terminal(&self) {
        if let Some(pane) = self.focused() {
            pane.term.widget.grab_focus();
        }
    }

    fn toggle_project(self: &Rc<Self>) {
        let open = !self.project.is_open();
        self.project.set_open(open, open);
        if !open { self.focus_terminal(); }
        self.save_soon();
    }

    // ---- status bar and /proc -------------------------------------------------------------

    fn ask_project_agent(self: &Rc<Self>) {
        let Some(pane) = self.focused() else { return };
        let tag = pane.native.borrow().clone();
        let (Some(host), Some(terminal), Some(tag)) = (self.host.clone(), pane.tid.clone(), tag) else {
            self.project_agent_unavailable(&pane);
            return;
        };
        let Some(generation) = self.project.begin_ask() else { return };
        let key = pane.key.clone();
        std::thread::spawn(move || {
            let pid = native::running_panel_pid(&host, &tag.session_id, &terminal);
            glib::MainContext::default().invoke(move || {
                UI.with(|u| {
                    let Some(ui) = u.borrow().clone() else { return };
                    if !ui.project.finish_ask(generation) {
                        return;
                    }
                    let Some(pane) = ui.focused().filter(|pane| {
                        pane.key == key
                            && pane.tid.as_ref() == Some(&terminal)
                            && pane.native.borrow().as_ref().is_some_and(|t| t.session_id == tag.session_id)
                    }) else { return };
                    if can_send_panel_request(pane.life.get() == Some(native::Lifecycle::Running), || {
                        pid.is_some_and(procinfo::in_foreground)
                    }) {
                        pane.term.type_text(&tr(project_panel::REQUEST));
                    } else {
                        ui.project_agent_unavailable(&pane);
                    }
                });
            });
        });
    }

    fn project_agent_unavailable(self: &Rc<Self>, pane: &Pane) {
        pane.life.set(None);
        self.update_status();
        self.banner(&tr(t!("The agent is no longer running in this terminal's foreground.")), true);
    }

    fn update_status(&self) {
        let pane = self.focused();
        let info = pane.as_ref().map(|p| p.info.borrow().clone());
        let tag = pane.as_ref().and_then(|p| p.native.borrow().clone());
        let agent = pane.as_ref().is_some_and(|p| p.life.get() == Some(native::Lifecycle::Running))
            && tag.as_ref().is_some_and(|t| matches!(t.agent_name.as_str(), "Claude Code" | "Codex"));
        self.project.follow(pane.map(|p| p.key.clone()), info.as_ref().and_then(|i| i.cwd.clone()), agent);
        match info {
            Some(Info { known: true, cwd: Some(cwd), git, .. }) => {
                let cwd = cwd.to_string_lossy();
                self.folder.set_text(&procinfo::home_short(&cwd, self.home.as_deref()));
                self.folder.set_tooltip_text(Some(&cwd));
                self.git.set_text(&match git {
                    Some(Git::Branch(b)) => b,
                    Some(Git::Detached(h)) => format!("{} {h}", tr("detached HEAD")),
                    _ => tr("no git"),
                });
            }
            _ => {
                self.folder.set_text("");
                self.git.set_text("");
            }
        }
        self.paint_activity();
        // The tab already names a plain terminal; an agent shows only the permission it was started with.
        let permission = tag.as_ref().and_then(native::permission_label);
        self.kind.set_visible(permission.is_some());
        if let Some(p) = &permission {
            self.kind.set_text(p);
        }
        while let Some(child) = self.pane_actions.first_child() {
            self.pane_actions.remove(&child);
        }
        // In a split, the focused pane keeps its × to close just that pane.
        let split = self.tabs.borrow().get(self.active.get()).is_some_and(|t| layout::leaves(&t.tree).len() >= 2);
        if let Some(p) = self.focused().filter(|_| split) {
            self.pane_actions.append(&p.actions);
        }
    }

    fn paint_activity(&self) {
        let pane = self.focused();
        let tag = pane.as_ref().and_then(|p| p.native.borrow().clone());
        self.activity_strip.paint(&self.activity.borrow(), tag.as_ref().map(|t| t.session_id.as_str()));
    }

    fn refresh_activity(self: &Rc<Self>) {
        if !self.visible() || self.activity_refreshing.replace(true) {
            return;
        }
        let host = self.host.clone();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = gtk::gio::spawn_blocking(move || activity::snapshot(host.as_ref())).await;
            let Some(ui) = weak.upgrade() else { return };
            ui.activity_refreshing.set(false);
            let (view, _) = result.unwrap_or_default();
            ui.activity.replace(view);
            ui.paint_activity();
        });
    }

    fn visible(&self) -> bool {
        if !self.win.is_visible() || !self.win.is_mapped() {
            return false;
        }
        match self.win.surface().and_then(|s| s.downcast::<gdk::Toplevel>().ok()) {
            Some(t) => !t.state().contains(gdk::ToplevelState::MINIMIZED),
            None => true,
        }
    }

    fn refresh_soon(self: &Rc<Self>) {
        let ui = self.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || ui.refresh());
    }

    /// Reads memory, folder and branch of every pane off the GTK thread. Only while the window shows.
    fn refresh(self: &Rc<Self>) {
        if !self.visible() || self.refreshing.get() {
            return;
        }
        self.refreshing.set(true);
        let jobs: Vec<Job> = self
            .panes
            .borrow()
            .values()
            .map(|p| {
                let info = p.info.borrow();
                Job { key: p.key.clone(), tid: p.tid.clone(), local_pid: p.term.local_pid, known: info.pid.map(|pid| (pid, info.shell.clone())).filter(|_| info.known) }
            })
            .collect();
        let host = self.host.clone();
        // Agents: their sessions are read once for all of them.
        let agents: Vec<(String, String)> = self.panes.borrow().values().filter_map(|p| p.native.borrow().as_ref().map(|t| (p.key.clone(), t.session_id.clone()))).collect();
        std::thread::spawn(move || {
            let results: Vec<Res> = jobs.into_iter().map(|j| probe(host.as_ref(), j)).collect();
            let states: Vec<(String, String)> = match (&host, agents.is_empty()) {
                (Some(h), false) => native::sessions(h)
                    .map(|all| agents.iter().filter_map(|(key, sid)| all.iter().find(|s| &s.id == sid).map(|s| (key.clone(), s.state.clone()))).collect())
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            glib::MainContext::default().invoke(move || {
                UI.with(|u| {
                    if let Some(ui) = u.borrow().clone() {
                        ui.apply(results, states);
                    }
                });
            });
        });
    }

    fn apply(self: &Rc<Self>, results: Vec<Res>, states: Vec<(String, String)>) {
        self.refreshing.set(false);
        for r in results {
            let Some(p) = self.pane(&r.key) else { continue };
            *p.info.borrow_mut() = r.info;
            if let Some((_, state)) = states.iter().find(|(k, _)| *k == r.key) {
                p.life.set(native::lifecycle(state));
            }
            if p.find.bar.is_visible() {
                Ui::paint_count(&p, p.term.find_info());
            }
        }
        for i in 0..self.tabs.borrow().len() {
            self.refresh_tab_labels(i);
        }
        self.update_status();
    }

    // ---- the Launch menu and the agent lifecycle -----------------------------------------------

    /// Said under the tab bar: what a launch or a lifecycle action did not do (an error) or did (a note).
    fn banner(self: &Rc<Self>, text: &str, error: bool) {
        self.banner_text.set_text(text);
        if error {
            self.banner.add_css_class("error");
        } else {
            self.banner.remove_css_class("error");
        }
        self.banner.set_visible(!text.is_empty());
        let generation = self.banner_gen.get() + 1;
        self.banner_gen.set(generation);
        let ui = self.clone();
        glib::timeout_add_seconds_local_once(20, move || {
            if ui.banner_gen.get() == generation {
                ui.banner.set_visible(false);
            }
        });
    }

    fn toggle_launches(self: &Rc<Self>) {
        if self.launch_panel.is_visible() {
            self.hide_launches();
            return;
        }
        self.fill_launches();
        self.launch_panel.set_visible(true);
        self.launch_btn.add_css_class("open");
    }

    fn hide_launches(&self) {
        self.launch_panel.set_visible(false);
        self.launch_btn.remove_css_class("open");
    }

    /// The menu is read from the owner's files each time it opens: name, type, Full access and the
    /// folder the files are in.
    fn fill_launches(self: &Rc<Self>) {
        while let Some(c) = self.launch_panel.first_child() {
            self.launch_panel.remove(&c);
        }
        let folder = launches::folder();
        let listing = folder.as_deref().map(|f| launches::read_folder(f, &launches::System::read(std::path::Path::new("/etc/os-release")))).unwrap_or_default();
        let home = self.home.clone().unwrap_or_default();
        let shown = folder.as_deref().map(|f| launches::display(f, &home)).unwrap_or_default();
        if listing.launches.is_empty() {
            let l = label(&trf("No launches. Add a file in {folder}.", &[("folder", &shown)]), "launch-empty");
            l.set_halign(gtk::Align::Start);
            self.launch_panel.append(&l);
        }
        for launch in &listing.launches {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            let name = label(&launch.name, "launch-name");
            name.set_hexpand(true);
            name.set_halign(gtk::Align::Start);
            name.set_xalign(0.0);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            let kind = label(launch.type_name(), "launch-type");
            row.append(&name);
            row.append(&kind);
            if launch.full_access() {
                row.append(&label(&tr("Full access"), "pill-full"));
            }
            let b = gtk::Button::new();
            b.add_css_class("launch-item");
            b.set_child(Some(&row));
            b.set_focusable(true);
            let (weak, id) = (Rc::downgrade(self), launch.id.clone());
            b.connect_clicked(move |_| {
                if let Some(ui) = weak.upgrade() {
                    ui.hide_launches();
                    ui.open_launch(&id);
                }
            });
            self.launch_panel.append(&b);
        }
        for (id, why) in &listing.refused {
            let l = label(&format!("{id}.toml: {why}"), "launch-refused");
            l.set_halign(gtk::Align::Start);
            l.set_xalign(0.0);
            l.set_wrap(true);
            l.set_max_width_chars(48);
            self.launch_panel.append(&l);
        }
        let foot = label(&trf("Files in {folder}", &[("folder", &shown)]), "launch-foot");
        foot.set_halign(gtk::Align::Fill);
        foot.set_xalign(0.0);
        foot.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        foot.set_max_width_chars(44);
        self.launch_panel.append(&foot);
    }

    /// Opens a launch by its id, read again from its file: the menu's copy is never trusted. The work
    /// (the host's calls and the `before` commands) runs off the GTK thread.
    fn open_launch(self: &Rc<Self>, id: &str) {
        let Some(host) = self.host.clone() else {
            self.banner(&native::failure_text("host_unavailable"), true);
            return;
        };
        if self.opening.replace(true) {
            return;
        }
        self.banner("", false);
        let id = id.to_owned();
        std::thread::spawn(move || {
            let result = (|| {
                let folder = launches::folder().ok_or_else(|| native::Refusal(native::failure_text("internal")))?;
                let (launch, print) = launches::get_with_fingerprint(&folder, &id, &launches::System::read(std::path::Path::new("/etc/os-release"))).map_err(native::Refusal)?;
                let home = native::launcher(&host)?.home.or_else(|| std::env::var("HOME").ok()).unwrap_or_default();
                let approvals = wyss_console::launch::Approvals::from_env();
                native::open_launch_select(&host, &launch, &print, &home, &native::run_before, approvals.as_ref(), &review_project_instructions)
            })();
            glib::MainContext::default().invoke(move || {
                UI.with(|u| {
                    if let Some(ui) = u.borrow().clone() {
                        ui.opened(result);
                    }
                });
            });
        });
    }

    /// A tab in the asked folder; with a program, it replaces the shell (`exec`) so the tab closes
    /// when the program ends.
    fn open_request(self: &Rc<Self>, request: &crate::cli::Request, cwd: Option<String>) {
        let dir = request.workdir.clone().or(cwd);
        let Some(host) = self.host.clone() else {
            self.new_tab();
            return;
        };
        let title = request
            .command
            .as_ref()
            .and_then(|c| c.first())
            .and_then(|p| p.split_whitespace().next())
            .map(|p| p.rsplit('/').next().unwrap_or(p).to_owned())
            .unwrap_or_else(|| tr("Terminal"));
        let command = request.command.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let (rows, cols) = native::FIRST_SIZE;
                let id = host.create_in(rows, cols, dir.as_deref(), Some(&title))?;
                if let Some(command) = &command
                    && let Err(error) = host.write(&id, &crate::cli::typed(command))
                {
                    let _ = host.close(&id);
                    return Err(native::Refusal::from(error));
                }
                Ok(native::Opened::Terminal { terminal_id: id, title })
            })();
            glib::MainContext::default().invoke(move || {
                UI.with(|ui| {
                    if let Some(ui) = ui.borrow().clone() { ui.opened(result); }
                });
            });
        });
    }

    fn run_distributor(self: &Rc<Self>, title: &str, cwd: &str, command: &str) {
        let Some(host) = self.host.clone() else {
            self.banner(&native::failure_text("host_unavailable"), true);
            return;
        };
        if self.opening.replace(true) { return; }
        self.banner("", false);
        let (title, cwd, command) = (title.to_owned(), cwd.to_owned(), command.to_owned());
        std::thread::spawn(move || {
            let result = (|| {
                let (rows, cols) = native::FIRST_SIZE;
                let id = host.create_in(rows, cols, Some(&cwd), Some(&title))?;
                if let Err(error) = host.write(&id, &launches::typed(&[command])) {
                    let _ = host.close(&id);
                    return Err(native::Refusal::from(error));
                }
                Ok(native::Opened::Terminal { terminal_id: id, title })
            })();
            glib::MainContext::default().invoke(move || {
                UI.with(|ui| {
                    if let Some(ui) = ui.borrow().clone() { ui.opened(result); }
                });
            });
        });
    }

    fn opened(self: &Rc<Self>, result: Result<native::Opened, native::Refusal>) {
        self.opening.set(false);
        match result {
            Err(r) => self.banner(&r.0, true),
            Ok(native::Opened::Terminal { terminal_id, .. }) => self.add_host_tab(terminal_id, None),
            Ok(native::Opened::Agent { started, notes }) => {
                self.add_agent_tab(&started);
                self.banner(&notes.join(" "), false);
            }
            Ok(native::Opened::NeedsApproval(needed)) => self.ask_approval(needed),
            Ok(native::Opened::Cancelled) => {}
        }
    }

    /// Shows the folder, the exact commands it types, the `before` commands and the full access of
    /// a launch file whose text and folder the owner has not approved. Accepting approves that very
    /// text in that folder (its fingerprint) and opens the launch again; if the file changed in
    /// between, the new text is asked for again.
    fn ask_approval(self: &Rc<Self>, needed: wyss_console::launch::NeedsApproval) {
        let mut detail = trf(crate::t!("It runs in {directory}"), &[("directory", &needed.directory)]);
        detail.push_str("\n\n");
        if !needed.commands.is_empty() {
            detail.push_str(&tr(crate::t!("It opens a terminal and types these commands, each followed by Enter:")));
            for command in &needed.commands {
                detail.push_str("\n  ");
                detail.push_str(command);
            }
            detail.push_str("\n\n");
        }
        if needed.bypass {
            detail.push_str(&tr(crate::t!("It starts the agent with full access: no permission prompts and no sandbox.")));
            detail.push_str("\n\n");
        }
        if !needed.before.is_empty() {
            detail.push_str(&tr(crate::t!("Before the agent, it runs these commands:")));
            for command in &needed.before {
                detail.push_str("\n  ");
                detail.push_str(command);
            }
            detail.push_str("\n\n");
        }
        detail.push_str(&tr(crate::t!("This launch file was changed outside Workspace, or was never approved. Run it only if you wrote it.")));
        let dialog = gtk::AlertDialog::builder()
            .modal(true)
            .message(tr(crate::t!("Run this launch?")))
            .detail(detail)
            .buttons([tr(crate::t!("Cancel")), tr(crate::t!("Approve and run"))])
            .default_button(0)
            .cancel_button(0)
            .build();
        let ui = Rc::downgrade(self);
        dialog.choose(Some(&self.win), gtk::gio::Cancellable::NONE, move |answer| {
            let Some(ui) = ui.upgrade() else { return };
            if answer != Ok(1) {
                return;
            }
            let approved = wyss_console::launch::Approvals::from_env()
                .is_some_and(|approvals| approvals.approve(&needed.fingerprint).is_ok());
            if approved {
                ui.open_launch(&needed.id);
            } else {
                ui.banner(&native::failure_text("store_unavailable"), true);
            }
        });
    }

    /// A new tab for a terminal the host already has (re-attached from its start).
    fn add_host_tab(self: &Rc<Self>, tid: String, tag: Option<native::Tag>) {
        let Some(host) = self.host.clone() else { return };
        let pane = self.new_pane(term::Source::Host { host, tid: tid.clone(), attach: term::Attach::Fresh }, Some(tid), tag);
        let n = self.next_n.get();
        self.next_n.set(n + 1);
        self.add_tab(n, layout::leaf(&pane.key), pane.key.clone());
        let last = self.tabs.borrow().len() - 1;
        self.select(last);
        self.refresh_soon();
    }

    fn add_agent_tab(self: &Rc<Self>, started: &native::Started) {
        let s = &started.session;
        let tag = native::Tag { session_id: s.id.clone(), agent_name: s.agent_name.clone(), project: s.project.clone(), profile: s.profile.clone() };
        self.add_host_tab(started.terminal_id.clone(), Some(tag));
    }

    // ---- persistence ----------------------------------------------------------------------

    fn snapshot(&self) -> SavedLayout {
        let tabs = self.tabs.borrow();
        SavedLayout { tabs: tabs.iter().map(|t| SavedTab { n: t.n, root: t.tree.clone(), focus: t.focus.clone() }).collect(), active: self.active.get(), project: self.project.snapshot() }
    }

    fn save_now(&self) {
        if self.host.is_none() {
            return;
        }
        if let Some(path) = layout::state_path()
            && let Err(e) = layout::save(&path, &self.snapshot())
        {
            eprintln!("wyss-console: cannot save the layout: {e}");
        }
    }

    fn save_soon(self: &Rc<Self>) {
        if self.host.is_none() || self.save_pending.replace(true) {
            return;
        }
        let ui = self.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
            ui.save_pending.set(false);
            ui.save_now();
        });
    }
}

/// One pane's facts. Runs on the worker thread: a socket call at most once per terminal (shell and
/// process id), then only `/proc` and the files of a git checkout.
fn probe(host: Option<&Host>, j: Job) -> Res {
    let mut info = Info { known: true, running: true, ..Info::default() };
    let mut gone = false;
    match (&j.known, &j.tid, host, j.local_pid) {
        (Some((pid, shell)), _, _, _) => {
            info.pid = Some(*pid);
            info.shell = shell.clone();
        }
        (None, Some(tid), Some(h), _) => match h.status(tid) {
            Ok(st) if st.state == "running" => {
                info.pid = st.pid.map(|p| p as i32);
                info.shell = st.shell;
            }
            Ok(_) | Err(crate::host::HostError::Remote { .. }) => gone = true,
            Err(_) => info.known = false,
        },
        (None, None, _, Some(pid)) => {
            info.pid = Some(pid as i32);
            info.shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".into());
        }
        _ => info.known = false,
    }
    if gone {
        info.running = false;
    } else if let Some(pid) = info.pid {
        match procinfo::tree_rss(pid) {
            Some(rss) => {
                info.rss = Some(rss);
                if let Some(cwd) = procinfo::cwd(pid) {
                    info.git = Some(procinfo::git_state(&cwd));
                    info.cwd = Some(cwd);
                }
            }
            None => info.running = false,
        }
    }
    Res { key: j.key, info }
}

fn rail_button(icon: Icon, tip: &str, active: bool) -> (gtk::Box, gtk::Button) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let marker = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    marker.add_css_class("rail-marker");
    marker.set_valign(gtk::Align::Center);
    if active {
        marker.add_css_class("active");
    }
    let b = gtk::Button::new();
    b.set_child(Some(&icons::widget(icon, 16)));
    b.add_css_class("rail-btn");
    if active {
        b.add_css_class("active");
    }
    b.set_tooltip_text(Some(tip));
    row.append(&marker);
    row.append(&b);
    (row, b)
}

fn tool_button(icon: Icon, tip: &str) -> gtk::Button {
    let b = gtk::Button::new();
    b.set_child(Some(&icons::widget(icon, 14)));
    b.add_css_class("tool-btn");
    b.set_tooltip_text(Some(tip));
    b.set_valign(gtk::Align::Center);
    b
}

fn shortcuts_popover(anchor: &gtk::Button) -> gtk::Popover {
    let pop = gtk::Popover::new();
    pop.add_css_class("shortcuts");
    pop.set_parent(anchor);
    let grid = gtk::Grid::new();
    grid.set_row_spacing(6);
    grid.set_column_spacing(24);
    let title = label(&tr("Keyboard shortcuts"), "shortcut-title");
    title.set_halign(gtk::Align::Start);
    grid.attach(&title, 0, 0, 2, 1);
    for (i, (k, what)) in keymap::LEGEND.iter().enumerate() {
        let (kl, wl) = (label(k, "shortcut-key"), label(&tr(what), "shortcut-what"));
        kl.set_halign(gtk::Align::Start);
        wl.set_halign(gtk::Align::Start);
        grid.attach(&kl, 0, i as i32 + 1, 1, 1);
        grid.attach(&wl, 1, i as i32 + 1, 1, 1);
    }
    pop.set_child(Some(&grid));
    pop
}

fn apply_theme(provider: &gtk::CssProvider, next: theme::Theme) {
    theme::set(next);
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(theme::prefers_dark(next));
    }
    provider.load_from_string(&format!("{}{THEME}{}{}{}{}{}{}", theme::defines(next), crate::workspace::CSS, crate::distributors::CSS, crate::arsenal::CSS, crate::credentials::CSS, settings::CSS, project_panel::CSS));
}

pub fn build(
    app: &gtk::Application,
    cfg: Config,
    service: SettingsService,
    settings_snapshot: settings::Snapshot,
) -> gtk::ApplicationWindow {
    let preferences = settings_snapshot.preferences();
    crate::i18n::set(settings::language(preferences));
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_theme_name(Some("Default"));
    }
    let provider = gtk::CssProvider::new();
    apply_theme(&provider, settings::appearance(preferences));
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_USER + 1);
    }
    let win = gtk::ApplicationWindow::builder().application(app).title("Wyss Console").default_width(1200).default_height(800).build();
    win.add_css_class("console");
    // An empty title bar of our own keeps KWin from drawing its bar over the traffic lights.
    let no_titlebar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    no_titlebar.add_css_class("no-titlebar");
    win.set_titlebar(Some(&no_titlebar));

    // Rail.
    let rail = gtk::Box::new(gtk::Orientation::Vertical, 4);
    rail.add_css_class("rail");
    let (console_row, console_button) = rail_button(Icon::Terminal, &tr("Console"), true);
    let (workspace_row, workspace_button) = rail_button(Icon::Grid, &tr("Workspace"), false);
    rail.append(&console_row);
    rail.append(&workspace_row);
    let (personal_row, personal_button) = rail_button(Icon::Palette, &tr("Personalization"), false);
    rail.append(&personal_row);
    let (settings_row, settings_button) = rail_button(Icon::Settings, &tr("Settings"), false);
    // Settings sits alone at the bottom of the rail.
    let rail_gap = gtk::Box::new(gtk::Orientation::Vertical, 0);
    rail_gap.set_vexpand(true);
    rail.append(&rail_gap);
    // The shortcut list sits right above Settings.
    let (keys_row, keys) = rail_button(Icon::Keyboard, &tr("Keyboard shortcuts"), false);
    rail.append(&keys_row);
    rail.append(&settings_row);

    // Tab bar.
    let tab_strip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    // The strip scrolls instead of widening the window: with many tabs the sum of their minimum widths
    // would otherwise become the window's minimum width and push the bar off screen.
    let tab_scroll = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_width(false)
        .child(&tab_strip)
        .build();
    let tools = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    tools.add_css_class("tools");
    let project_btn = tool_button(Icon::Folder, &tr("Project (Ctrl+Shift+P)"));
    project_btn.update_property(&[gtk::accessible::Property::Label(&tr("Project"))]);
    tools.append(&project_btn);
    // The agent's buttons (Queue, Stop, Restart) and the pane's × go between Project and Launch.
    let pane_actions = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    pane_actions.set_valign(gtk::Align::Center);
    tools.append(&pane_actions);
    let launch_btn = gtk::Button::with_label(&format!("+ {}", tr("Launch")));
    launch_btn.add_css_class("launch-btn");
    launch_btn.set_valign(gtk::Align::Center);
    launch_btn.set_tooltip_text(Some(&tr("Your saved launches")));
    launch_btn.set_sensitive(cfg.host.is_some());
    tools.append(&launch_btn);
    // Tabs that do not fit are listed behind this button, at the end of the strip.
    let tab_more = gtk::Button::with_label("»");
    tab_more.add_css_class("tool-btn");
    tab_more.set_valign(gtk::Align::Center);
    tab_more.set_visible(false);
    tab_more.set_tooltip_text(Some(&tr("More tabs")));
    let tab_more_pop = gtk::Popover::new();
    tab_more_pop.add_css_class("tab-more");
    tab_more_pop.set_parent(&tab_more);
    let tabbar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    tabbar.add_css_class("tabbar");
    tabbar.append(&tab_scroll);
    tabbar.append(&tab_more);
    tabbar.append(&tools);
    // Minimize, maximize and close with the icons of the system's icon theme, at the end of the bar.
    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    controls.add_css_class("window-buttons");
    controls.set_valign(gtk::Align::Center);
    for (icon, tip) in [("window-minimize-symbolic", tr("Minimize")), ("window-maximize-symbolic", tr("Maximize")), ("window-close-symbolic", tr("Close"))] {
        let b = gtk::Button::from_icon_name(icon);
        if let Some(image) = b.child().and_downcast::<gtk::Image>() {
            image.set_pixel_size(12);
        }
        b.set_tooltip_text(Some(&tip));
        b.set_focusable(false);
        if icon == "window-close-symbolic" {
            b.add_css_class("close");
        }
        let win = win.clone();
        b.connect_clicked(move |_| match icon {
            "window-minimize-symbolic" => win.minimize(),
            "window-maximize-symbolic" => win.set_maximized(!win.is_maximized()),
            _ => win.close(),
        });
        controls.append(&b);
    }
    tabbar.append(&controls);
    // Dragging the tab bar moves the window (there is no system title bar).
    let tabbar_handle = gtk::WindowHandle::new();
    tabbar_handle.set_child(Some(&tabbar));

    let stack = gtk::Stack::new();
    stack.set_vexpand(true);
    stack.set_hexpand(true);

    // Status bar.
    let status = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    status.add_css_class("statusbar");
    let folder_icon = icons::widget(Icon::Folder, 14);
    let folder = label("", "status-folder");
    folder.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    let git = label("", "status-git");
    let kind = label(&tr("Terminal"), "status-kind");
    // One bottom bar: folder and branch on the left, the Claude / Codex readings and the pane on the right.
    let activity_strip = activity::Strip::new();
    activity_strip.root.set_hexpand(true);
    activity_strip.root.set_halign(gtk::Align::End);
    for w in [folder_icon.upcast_ref::<gtk::Widget>(), folder.upcast_ref(), git.upcast_ref(), activity_strip.root.upcast_ref(), kind.upcast_ref()] {
        status.append(w);
    }

    // A line under the tab bar for what a launch or an agent action did not do.
    let banner = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    banner.add_css_class("banner");
    banner.set_visible(false);
    let banner_text = gtk::Label::new(None);
    banner_text.set_hexpand(true);
    banner_text.set_xalign(0.0);
    banner_text.set_wrap(true);
    banner_text.set_selectable(true);
    let banner_close = gtk::Button::with_label("×");
    banner_close.set_valign(gtk::Align::Start);
    banner_close.set_focusable(false);
    banner.append(&banner_text);
    banner.append(&banner_close);

    // The Launch menu floats over the window (a popover would be a surface of its own).
    let launch_panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
    launch_panel.add_css_class("launch-panel");
    launch_panel.set_halign(gtk::Align::End);
    launch_panel.set_valign(gtk::Align::Start);
    launch_panel.set_visible(false);

    let main = gtk::Box::new(gtk::Orientation::Vertical, 0);
    main.set_hexpand(true);
    main.append(&tabbar_handle);
    main.append(&banner);
    let project = Panel::new(&stack);
    project.paned.set_vexpand(true);
    main.append(&project.paned);
    main.append(&status);
    let outer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    rail.set_vexpand(true);
    outer.append(&rail);
    let screens = gtk::Stack::new();
    screens.set_hexpand(true);
    screens.set_hhomogeneous(false);
    screens.set_vhomogeneous(false);
    screens.add_named(&main, Some("console"));
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.set_hexpand(true);
    screens.set_vexpand(true);
    body.append(&screens);
    outer.append(&body);
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&outer));
    overlay.add_overlay(&launch_panel);
    win.set_child(Some(&overlay));

    let ui = Rc::new(Ui {
        win: win.clone(),
        stack,
        project,
        tab_strip,
        tab_scroll,
        tab_more: tab_more.clone(),
        tab_more_pop,
        folder,
        git,
        kind,
        pane_actions,
        launch_btn: launch_btn.clone(),
        launch_panel: launch_panel.clone(),
        banner: banner.clone(),
        banner_text,
        banner_gen: Cell::new(0),
        opening: Cell::new(false),
        tabs: RefCell::new(Vec::new()),
        active: Cell::new(0),
        panes: RefCell::new(HashMap::new()),
        by_id: RefCell::new(HashMap::new()),
        next_id: Cell::new(1),
        next_n: Cell::new(1),
        host: cfg.host.clone(),
        home: std::env::var("HOME").ok(),
        refreshing: Cell::new(false),
        activity_refreshing: Cell::new(false),
        activity_strip,
        activity: RefCell::new(Default::default()),
        save_pending: Cell::new(false),
        terminal_preferences: Cell::new(preferences.terminal),
    });
    UI.with(|u| *u.borrow_mut() = Some(ui.clone()));
    {
        let adj = ui.tab_scroll.hadjustment();
        let refresh = {
            let weak = Rc::downgrade(&ui);
            move || {
                let weak = weak.clone();
                // After the layout, so every tab already has its allocation.
                glib::idle_add_local_full(glib::Priority::LOW, move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.tab_more.set_visible(!ui.hidden_tabs().is_empty());
                    }
                    glib::ControlFlow::Break
                });
            }
        };
        let r = refresh.clone();
        adj.connect_changed(move |_| r());
        adj.connect_value_changed(move |_| refresh());
        let weak = Rc::downgrade(&ui);
        tab_more.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() { ui.show_tab_more(); }
        });
    }

    {
        let weak = Rc::downgrade(&ui);
        project_btn.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() { ui.toggle_project(); }
        });
    }
    {
        let weak = Rc::downgrade(&ui);
        ui.project.on_ask(move || {
            if let Some(ui) = weak.upgrade() {
                ui.ask_project_agent();
            }
        });
    }
    {
        let weak = Rc::downgrade(&ui);
        ui.project.paned.connect_position_notify(move |_| {
            if let Some(ui) = weak.upgrade() { ui.save_soon(); }
        });
    }

    {
        let pop = shortcuts_popover(&keys);
        keys.connect_clicked(move |_| pop.popup());
    }
    {
        let ui = ui.clone();
        launch_btn.connect_clicked(move |_| ui.toggle_launches());
    }
    {
        let banner = banner.clone();
        banner_close.connect_clicked(move |_| banner.set_visible(false));
    }
    // A click outside the menu closes it.
    {
        let (ui, overlay_w, panel, btn) = (ui.clone(), overlay.clone(), launch_panel.clone(), launch_btn.clone());
        let click = gtk::GestureClick::new();
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_pressed(move |_, _, x, y| {
            if !panel.is_visible() {
                return;
            }
            let inside = |w: &gtk::Widget| w.is_ancestor(&panel) || *w == *panel.upcast_ref::<gtk::Widget>() || w.is_ancestor(&btn) || *w == *btn.upcast_ref::<gtk::Widget>();
            if !overlay_w.pick(x, y, gtk::PickFlags::DEFAULT).is_some_and(|w| inside(&w)) {
                ui.hide_launches();
            }
        });
        overlay.add_controller(click);
    }

    let show_screen = {
        let screens = screens.downgrade();
        let console_row = console_row.downgrade();
        let workspace_row = workspace_row.downgrade();
        let personal_row = personal_row.downgrade();
        let settings_row = settings_row.downgrade();
        move |name: &str| {
            if let Some(screens) = screens.upgrade() {
                screens.set_visible_child_name(name);
            }
            for (row, active) in [
                (console_row.upgrade(), name == "console"),
                (workspace_row.upgrade(), name == "workspace"),
                (personal_row.upgrade(), name == "personalization"),
                (settings_row.upgrade(), name == "settings"),
            ] {
                if let Some(row) = row {
                    for child in [row.first_child(), row.last_child()].into_iter().flatten() {
                        if active { child.add_css_class("active"); } else { child.remove_css_class("active"); }
                    }
                }
            }
        }
    };
    console_button.connect_clicked({
        let show_screen = show_screen.clone();
        move |_| {
            // Opened with --no-terminal: the console gets its first tab when it is shown.
            if let Some(ui) = UI.with(|ui| ui.borrow().clone())
                && ui.tabs.borrow().is_empty()
            {
                ui.new_tab();
            }
            show_screen("console")
        }
    });
    let (settings_page, landing) = settings::build(&win, service, settings_snapshot, cfg.host.clone(), {
        let ui = Rc::downgrade(&ui);
        move |preferences: Preferences| {
            let Some(ui) = ui.upgrade() else { return };
            let next = settings::appearance(preferences);
            let theme_changed = next != theme::current();
            if theme_changed {
                apply_theme(&provider, next);
            }
            ui.terminal_preferences.set(preferences.terminal);
            for pane in ui.panes.borrow().values() {
                pane.term.apply_preferences(preferences.terminal, theme_changed);
            }
        }
    });
    screens.add_named(&settings_page, Some("settings"));
    settings_button.connect_clicked({
        let show_screen = show_screen.clone();
        let ui = Rc::downgrade(&ui);
        move |_| {
            if let Some(ui) = ui.upgrade() {
                ui.hide_launches();
                show_screen("settings");
            }
        }
    });
    let open_workspace = {
        let ui = Rc::downgrade(&ui);
        let screens = screens.downgrade();
        let window = win.downgrade();
        let show_screen = show_screen.clone();
        move |target: Option<(&str, &str)>| {
            let (Some(ui), Some(screens), Some(window)) = (ui.upgrade(), screens.upgrade(), window.upgrade()) else { return };
            ui.hide_launches();
            if screens.child_by_name("workspace").is_none() {
                let try_launch = ui.host.as_ref().map(|_| {
                    let show_screen = show_screen.clone();
                    let ui = Rc::downgrade(&ui);
                    Rc::new(move |id: &str| {
                        if let Some(ui) = ui.upgrade() {
                            show_screen("console");
                            ui.open_launch(id);
                        }
                    }) as Rc<dyn Fn(&str)>
                });
                screens.add_named(
                    &crate::workspace::build(&window, try_launch, ui.host.clone()),
                    Some("workspace"),
                );
                if let Some((id, action)) = target
                    && let Some(workspace) = screens.child_by_name("workspace")
                {
                    let action = if action == "workspace-route.reload-and-open" { "workspace-route.open" } else { action };
                    let _ = workspace.activate_action(action, Some(&id.to_variant()));
                }
            } else if let Some((id, action)) = target
                && let Some(workspace) = screens.child_by_name("workspace")
            {
                let _ = workspace.activate_action(action, Some(&id.to_variant()));
            }
            show_screen("workspace");
        }
    };
    workspace_button.connect_clicked({
        let open_workspace = open_workspace.clone();
        move |_| open_workspace(None)
    });
    let personal = crate::personalization::horizontal(0);
    let rail = crate::personalization::vertical(2);
    rail.add_css_class("personal-rail");
    let pages = gtk::Stack::builder().hexpand(true).vexpand(true).build();
    let distributors_tab = gtk::ToggleButton::new();
    let arsenal_tab = gtk::ToggleButton::new();
    let credentials = gtk::ToggleButton::new();
    credentials.set_group(Some(&distributors_tab));
    arsenal_tab.set_group(Some(&distributors_tab));
    let captions = [
        crate::personalization::rail_item(&distributors_tab, "package-x-generic-symbolic", t!("Distributors")),
        crate::personalization::rail_item(&arsenal_tab, "view-list-tree-symbolic", t!("Arsenal")),
        crate::personalization::rail_item(&credentials, "dialog-password-symbolic", t!("Credentials")),
    ];
    arsenal_tab.connect_toggled(move |tab| {
        for caption in &captions {
            caption.set_visible(!tab.is_active());
        }
    });
    rail.append(&distributors_tab);
    rail.append(&arsenal_tab);
    rail.append(&credentials);
    personal.append(&rail);
    personal.append(&pages);
    screens.add_named(&personal, Some("personalization"));
    let run = {
        let ui = Rc::downgrade(&ui);
        let show_screen = show_screen.clone();
        Rc::new(move |title: &str, cwd: &str, command: &str| {
            if let Some(ui) = ui.upgrade() {
                show_screen("console");
                ui.run_distributor(title, cwd, command);
            }
        })
    };
    distributors_tab.connect_toggled({
        let window = win.downgrade();
        let pages = pages.downgrade();
        let open_workspace = open_workspace.clone();
        move |tab| {
            if !tab.is_active() { return; }
            let started = std::time::Instant::now();
            let (Some(window), Some(pages)) = (window.upgrade(), pages.upgrade()) else { return };
            let open_workspace = open_workspace.clone();
            let edit = Rc::new(move |id: &str| open_workspace(Some((id, "workspace-route.reload-and-open"))));
            let page = crate::distributors::build(&window, run.clone(), edit);
            if let Some(previous) = pages.child_by_name("distributors") { pages.remove(&previous); }
            pages.add_named(&page, Some("distributors"));
            pages.set_visible_child(&page);
            crate::personalization::timing("dist:open", started);
        }
    });
    arsenal_tab.connect_toggled({
        let open_workspace = open_workspace.clone();
        let pages = pages.downgrade();
        move |tab| {
            if !tab.is_active() { return; }
            let started = std::time::Instant::now();
            let Some(pages) = pages.upgrade() else { return };
            let open_workspace = open_workspace.clone();
            let create = open_workspace.clone();
            let page = crate::arsenal::build(
                Rc::new(move |id| open_workspace(Some((id, "workspace-route.open")))),
                Rc::new(move |id| create(Some((id, "workspace-route.create-from-preset")))),
            );
            if let Some(previous) = pages.child_by_name("arsenal") { pages.remove(&previous); }
            pages.add_named(&page, Some("arsenal"));
            pages.set_visible_child(&page);
            crate::personalization::timing("ars:open", started);
        }
    });
    credentials.connect_toggled({
        let pages = pages.downgrade();
        move |tab| {
            if !tab.is_active() {
                return;
            }
            let Some(pages) = pages.upgrade() else { return };
            if pages.child_by_name("credentials").is_none() {
                pages.add_named(&crate::credentials::build(), Some("credentials"));
            }
            pages.set_visible_child_name("credentials");
        }
    });
    personal_button.connect_clicked({
        let personal = personal.downgrade();
        let show_screen = show_screen.clone();
        move |_| {
            if let Some(ui) = UI.with(|ui| ui.borrow().clone()) { ui.hide_launches(); }
            if let Some(personal) = personal.upgrade()
                && !personal.is_mapped()
            {
                let selected = if arsenal_tab.is_active() {
                    &arsenal_tab
                } else if credentials.is_active() {
                    &credentials
                } else {
                    &distributors_tab
                };
                if selected.is_active() { selected.emit_by_name::<()>("toggled", &[]); }
                else { selected.set_active(true); }
            }
            show_screen("personalization");
        }
    });

    // Restore from the host: the terminals still alive, in the saved layout.
    let mut plan = SavedLayout { tabs: Vec::new(), active: 0, project: Default::default() };
    let mut attach: HashMap<String, term::Attach> = HashMap::new();
    let mut tags: HashMap<String, native::Tag> = HashMap::new();
    if let Some(h) = &cfg.host {
        match h.hello().and_then(|_| h.list()) {
            Ok(list) => {
                let alive: Vec<String> = list.iter().filter(|e| e.state == "running").map(|e| e.id.clone()).collect();
                tags = native::tags(&list).into_iter().collect();
                let saved = layout::state_path().and_then(|p| layout::load(&p));
                let (restored, next) = layout::restore(saved.as_ref(), &alive);
                ui.next_n.set(next);
                for id in alive {
                    let a = match h.screen(&id, term::attach_scrollback(preferences.terminal.scrollback)) {
                        Ok(Some(s)) => term::Attach::Screen(s),
                        _ => term::Attach::Replay,
                    };
                    attach.insert(id, a);
                }
                plan = restored;
            }
            Err(e) => eprintln!("wyss-console: term_list failed: {e}"),
        }
    }
    ui.project.restore(plan.project);
    for t in &plan.tabs {
        for k in layout::leaves(&t.root) {
            if let (Some(h), Some(a)) = (&cfg.host, attach.remove(&k)) {
                let tag = tags.remove(&k);
                ui.new_pane(term::Source::Host { host: h.clone(), tid: k.clone(), attach: a }, Some(k), tag);
            }
        }
        ui.add_tab(t.n, t.root.clone(), t.focus.clone());
    }
    let request = &cfg.request;
    if plan.tabs.is_empty() {
        if !request.wants_tab() && !request.no_terminal {
            for _ in 0..request.tabs.unwrap_or(1).max(1) {
                ui.new_tab();
            }
        }
    } else {
        ui.select(plan.active);
    }
    if request.wants_tab() {
        ui.open_request(request, cfg.cwd.clone());
    } else if request.no_terminal && ui.tabs.borrow().is_empty() {
        show_screen("workspace");
    }
    if ui.tabs.borrow().is_empty() && !request.wants_tab() && !request.no_terminal {
        eprintln!("wyss-console: no terminal to show");
        win.close();
    }

    // Console shortcuts, before the terminal sees the key.
    let kc = gtk::EventControllerKey::new();
    kc.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let ui = ui.clone();
        let screens = screens.downgrade();
        kc.connect_key_pressed(move |_, key, code, mods| {
            if screens.upgrade().and_then(|screens| screens.visible_child_name()).as_deref() != Some("console") {
                return glib::Propagation::Proceed;
            }
            if key == gdk::Key::Escape && ui.launch_panel.is_visible() {
                ui.hide_launches();
                return glib::Propagation::Stop;
            }
            if key == gdk::Key::Escape && ui.project.contains_focus(&ui.win) {
                ui.focus_terminal();
                return glib::Propagation::Stop;
            }
            match keymap::from_gtk(code, mods) {
                Some(a) => {
                    let ui = ui.clone();
                    glib::idle_add_local_once(move || ui.do_action(a));
                    glib::Propagation::Stop
                }
                None => glib::Propagation::Proceed,
            }
        });
    }
    win.add_controller(kc);

    {
        let ui = ui.clone();
        win.connect_close_request(move |_| {
            // Only disconnects: the terminals stay alive in the host.
            ui.save_now();
            glib::Propagation::Proceed
        });
    }
    {
        let ui = ui.clone();
        win.connect_map(move |_| {
            ui.refresh_soon();
            ui.refresh_activity();
        });
    }
    {
        let ui = ui.clone();
        glib::timeout_add_seconds_local(REFRESH_SECS, move || {
            ui.refresh();
            ui.refresh_activity();
            glib::ControlFlow::Continue
        });
    }

    // Test aid: WYSS_CONSOLE_LAYOUT_DEMO=split splits the first tab and opens a second one.
    if std::env::var("WYSS_CONSOLE_LAYOUT_DEMO").as_deref() == Ok("split") {
        ui.select(0);
        ui.split(Dir::Row);
        ui.new_tab();
        ui.select(0);
    }
    match landing {
        settings::Landing::Console => {}
        settings::Landing::Workspace => open_workspace(None),
        settings::Landing::Personalization => personal_button.emit_clicked(),
    }
    win.present();
    ui.refresh_soon();
    run_demo(&ui);
    win
}

/// Test aid for screenshots: `WYSS_CONSOLE_DEMO` is a comma-separated list of `launch-menu` (opens the Launch
/// menu), `open:<launch id>` (opens that launch as its menu entry does), and `search:<text>` (opens the search bar of the focused pane with that text).
fn run_demo(ui: &Rc<Ui>) {
    let Ok(demo) = std::env::var(DEMO_VAR) else { return };
    for step in demo.split(',') {
        let ui = ui.clone();
        let step = step.to_owned();
        glib::timeout_add_local_once(std::time::Duration::from_millis(900), move || {
            let focus = ui.tabs.borrow().get(ui.active.get()).map(|t| t.focus.clone());
            if step == "launch-menu" {
                ui.toggle_launches();
            } else if let (Some(text), Some(k)) = (step.strip_prefix("search:"), focus.as_ref()) {
                ui.open_search(k);
                if let Some(p) = ui.pane(k) {
                    p.find.entry.set_text(text);
                }
            } else if let Some(id) = step.strip_prefix("open:") {
                ui.open_launch(id);
            }
        });
    }
}
