use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use wyss_console::project::{
    self, AgentSection, Block, Branch, Git, GitError, Inline, Inventory, Project, Summary,
};

use crate::i18n::{tr, trf};
use crate::layout::SavedPanel;
use crate::personalization::{clear, label, scroll, single_line, timing, vertical};
use crate::t;

pub const CSS: &str = include_str!("project.css");
pub const REQUEST: &str = t!(
    "Explain the flow and architecture of this project (components, how they communicate, input → output) in brief Markdown and save it with: wyss-panel set flujo < file.md"
);
const POLL: Duration = Duration::from_millis(16);
const GIT_INTERVAL: Duration = Duration::from_secs(3);
const TITLES: [&str; 7] = [
    t!("Summary"),
    t!("Git"),
    t!("Technologies"),
    t!("How to run"),
    t!("Structure"),
    t!("CI and quality"),
    t!("Flow"),
];

type GitData = Option<Result<Git, GitError>>;
type Notes = Vec<(AgentSection, Vec<Arc<str>>)>;

enum Update {
    Located(Option<Project>, bool),
    Overview(Summary, GitData),
    Inventory([Rows; 4]),
    Git(GitData),
    Notes(Option<PathBuf>, Notes),
}
enum Job {
    Locate(Option<PathBuf>, Option<Project>),
    Git(Project),
    Notes(Project),
}

struct Section {
    root: gtk::Expander,
    body: gtk::Box,
}

impl Section {
    fn new(title: &str, expanded: bool) -> Self {
        let heading = single_line(title, "lbl");
        let body = vertical(0);
        let root = gtk::Expander::builder()
            .label_widget(&heading)
            .child(&body)
            .expanded(expanded)
            .build();
        root.add_css_class("project-section");
        root.set_focusable(true);
        Self { root, body }
    }

    fn loading(&self) {
        clear(&self.body);
        line(&self.body, &tr("Reading…"), "hint");
    }
}

#[derive(Clone, PartialEq, Eq, Default)]
struct Target {
    pane: Option<String>,
    cwd: Option<PathBuf>,
}

pub struct Panel {
    pub paned: gtk::Paned,
    scroll: gtk::ScrolledWindow,
    name: gtk::Label,
    branch: gtk::Label,
    state: gtk::Label,
    body: gtk::Box,
    sections: Vec<Section>,
    extras: RefCell<Vec<Section>>,
    ask: gtk::Button,
    ask_label: gtk::Label,
    requested_width: Cell<Option<i32>>,
    allocated_width: Cell<i32>,
    changing_width: Cell<bool>,
    target: RefCell<Target>,
    project: RefCell<Option<Project>>,
    displayed: RefCell<Option<Project>>,
    ready: Cell<bool>,
    overview_pending: Cell<bool>,
    generation: Arc<AtomicU64>,
    git_at: Cell<Instant>,
    git_pending: Cell<bool>,
    notes_dirty: Cell<bool>,
    notes_pending: Cell<bool>,
    agent: Cell<bool>,
    rx: mpsc::Receiver<(u64, Update)>,
    tx: mpsc::Sender<(u64, Update)>,
    monitor: RefCell<Option<(PathBuf, gio::FileMonitor)>>,
    head: RefCell<Option<String>>,
    notes: RefCell<Notes>,
    notes_initialized: Cell<bool>,
    last_git: RefCell<Option<GitData>>,
    first_draw: Cell<Option<Instant>>,
    git_paint: Cell<u64>,
    note_paint: Cell<u64>,
    poll_source: RefCell<Option<glib::SourceId>>,
}

impl Panel {
    pub fn new(terminal: &gtk::Stack) -> Rc<Self> {
        let pane = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .start_child(terminal)
            .resize_start_child(true)
            .shrink_start_child(true)
            .resize_end_child(false)
            .shrink_end_child(false)
            .build();
        let content = vertical(8);
        content.add_css_class("project-panel");
        let name = single_line(&tr("Project"), "project-title");
        let branch = single_line("", "project-branch");
        let state = label(&tr("Reading…"), "hint");
        let body = vertical(0);
        for widget in [
            name.upcast_ref::<gtk::Widget>(),
            branch.upcast_ref(),
            state.upcast_ref(),
            body.upcast_ref(),
        ] {
            content.append(widget);
        }
        let sections: Vec<_> = TITLES
            .iter()
            .enumerate()
            .map(|(i, title)| {
                let section = Section::new(&tr(title), matches!(i, 0 | 1 | 6));
                body.append(&section.root);
                section.loading();
                section
            })
            .collect();
        let ask = gtk::Button::new();
        ask.add_css_class("flat");
        let ask_label = label("", "");
        ask.set_child(Some(&ask_label));
        let scroll = scroll(&content);
        scroll.set_size_request(280, -1);
        scroll.set_focusable(true);
        scroll.set_visible(false);
        pane.set_end_child(Some(&scroll));
        let (tx, rx) = mpsc::channel();
        let panel = Rc::new(Self {
            paned: pane,
            scroll,
            name,
            branch,
            state,
            body,
            sections,
            extras: RefCell::default(),
            ask,
            ask_label,
            requested_width: Cell::new(None),
            allocated_width: Cell::new(0),
            changing_width: Cell::new(false),
            target: RefCell::default(),
            project: RefCell::default(),
            displayed: RefCell::default(),
            ready: Cell::new(false),
            overview_pending: Cell::new(false),
            generation: Arc::new(AtomicU64::new(0)),
            git_at: Cell::new(Instant::now()),
            git_pending: Cell::new(false),
            notes_dirty: Cell::new(false),
            notes_pending: Cell::new(false),
            agent: Cell::new(false),
            rx,
            tx,
            monitor: RefCell::default(),
            head: RefCell::default(),
            notes: RefCell::default(),
            notes_initialized: Cell::new(false),
            last_git: RefCell::default(),
            first_draw: Cell::new(None),
            git_paint: Cell::new(0),
            note_paint: Cell::new(0),
            poll_source: RefCell::default(),
        });
        panel.keyboard();
        let weak = Rc::downgrade(&panel);
        panel.paned.connect_position_notify(move |pane| {
            let Some(panel) = weak.upgrade() else { return };
            let total = pane.width();
            if panel.is_open()
                && total >= 560
                && !panel.changing_width.get()
                && total == panel.allocated_width.get()
            {
                let width = (total - pane.position()).clamp(280, panel.max_width(total));
                panel.requested_width.set(Some(width));
                panel.size();
            }
        });
        let weak = Rc::downgrade(&panel);
        panel.paned.connect_max_position_notify(move |_| {
            let weak = weak.clone();
            glib::idle_add_local_once(move || {
                if let Some(panel) = weak.upgrade()
                    && panel.allocated_width.get() != panel.paned.width()
                {
                    panel.size();
                }
            });
        });
        let weak = Rc::downgrade(&panel);
        panel.scroll.connect_map(move |scroll| {
            let weak_size = weak.clone();
            scroll.add_tick_callback(move |_, clock| {
                if let Some(panel) = weak_size.upgrade() {
                    panel.size();
                    if let Some(start) = panel.first_draw.take() {
                        let handler = Rc::new(Cell::new(None));
                        let connection = handler.clone();
                        let id = clock.connect_after_paint(move |clock| {
                            if let Some(id) = connection.take() {
                                clock.disconnect(id);
                            }
                            timing("panel:first-frame", start);
                        });
                        handler.set(Some(id));
                    }
                }
                glib::ControlFlow::Break
            });
            let Some(panel) = weak.upgrade() else {
                return;
            };
            let weak = weak.clone();
            let source = glib::timeout_add_local(POLL, move || {
                let Some(panel) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                panel.poll();
                glib::ControlFlow::Continue
            });
            *panel.poll_source.borrow_mut() = Some(source);
        });
        let weak = Rc::downgrade(&panel);
        panel.scroll.connect_unmap(move |_| {
            if let Some(panel) = weak.upgrade() {
                if let Some(source) = panel.poll_source.borrow_mut().take() {
                    source.remove();
                }
                panel.invalidate();
            }
        });
        panel
    }

    pub fn on_ask(&self, f: impl Fn() + 'static) {
        self.ask.connect_clicked(move |_| f());
    }

    pub fn contains_focus(&self, window: &gtk::ApplicationWindow) -> bool {
        gtk::prelude::GtkWindowExt::focus(window)
            .is_some_and(|focused| focused == self.scroll || focused.is_ancestor(&self.scroll))
    }

    pub fn is_open(&self) -> bool {
        self.scroll.is_visible()
    }

    pub fn snapshot(&self) -> SavedPanel {
        SavedPanel {
            open: self.is_open(),
            width: self.requested_width.get(),
        }
    }

    pub fn restore(&self, layout: SavedPanel) {
        self.requested_width.set(layout.width);
        self.set_open(layout.open, false);
    }

    pub fn set_open(&self, open: bool, focus: bool) {
        if self.is_open() != open {
            if open {
                self.first_draw.set(Some(Instant::now()));
            } else {
                self.invalidate();
            }
            self.scroll.set_visible(open);
        }
        if open {
            self.size();
            if focus {
                if self.body.is_visible() {
                    self.sections[0].root.grab_focus();
                } else {
                    self.scroll.grab_focus();
                }
            }
        }
    }

    fn size(&self) {
        let total = self.paned.width();
        self.allocated_width.set(total);
        if !self.is_open() || total < 560 {
            return;
        }
        let default = if self.window_width(total) < 1000 {
            280
        } else {
            360
        };
        let width = self
            .requested_width
            .get()
            .unwrap_or(default)
            .clamp(280, self.max_width(total));
        self.changing_width.set(true);
        self.paned.set_position(total - width);
        self.changing_width.set(false);
    }

    fn window_width(&self, total: i32) -> i32 {
        self.paned
            .root()
            .and_downcast::<gtk::Window>()
            .map_or(total, |w| w.width())
    }

    fn max_width(&self, total: i32) -> i32 {
        (self.window_width(total) / 2).min(total - 280).max(280)
    }

    fn keyboard(&self) {
        let controller = gtk::EventControllerKey::new();
        // Extras are reached through the same linear widget tree as the fixed sections.
        let body = self.body.downgrade();
        controller.connect_key_pressed(move |_, key, _, _| {
            if !matches!(key, gdk::Key::Up | gdk::Key::Down) {
                return glib::Propagation::Proceed;
            }
            let Some(body) = body.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let focused = body
                .root()
                .and_downcast::<gtk::Window>()
                .and_then(|window| gtk::prelude::GtkWindowExt::focus(&window));
            let mut child = body.first_child().and_downcast::<gtk::Expander>();
            while let Some(widget) = child {
                let on_heading = focused.as_ref().is_some_and(|f| {
                    f == &widget
                        || (f.is_ancestor(&widget)
                            && widget
                                .child()
                                .is_none_or(|content| f != &content && !f.is_ancestor(&content)))
                });
                if on_heading {
                    let next = if key == gdk::Key::Up {
                        widget.prev_sibling()
                    } else {
                        widget.next_sibling()
                    };
                    if let Some(next) = next {
                        next.grab_focus();
                    }
                    return glib::Propagation::Stop;
                }
                child = widget.next_sibling().and_downcast::<gtk::Expander>();
            }
            glib::Propagation::Proceed
        });
        self.body.add_controller(controller);
    }

    pub fn follow(&self, pane: Option<String>, cwd: Option<PathBuf>, agent: bool) {
        let agent_changed = self.agent.replace(agent) != agent;
        let target = Target { pane, cwd };
        if *self.target.borrow() != target {
            *self.target.borrow_mut() = target;
            self.invalidate();
            self.update_ask();
        } else if agent_changed {
            self.update_ask();
        }
    }

    pub fn can_ask(&self) -> bool {
        self.is_open()
            && self.ready.get()
            && !self.overview_pending.get()
            && self.project.borrow().is_some()
            && self.agent.get()
    }

    fn update_ask(&self) {
        let enabled = self.can_ask();
        self.ask.set_sensitive(enabled);
        self.ask_label.set_label(&tr(if enabled {
            t!("Ask the agent")
        } else {
            t!("Open an agent in this folder to generate it")
        }));
    }

    fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.ready.set(false);
        self.overview_pending.set(true);
        self.git_pending.set(false);
        self.notes_pending.set(false);
        self.notes_dirty.set(false);
        self.monitor.borrow_mut().take();
        if self.is_open() {
            self.state.set_label(&tr("Reading…"));
            self.state.set_visible(true);
        }
    }

    fn spawn(&self, job: Job) {
        let tx = self.tx.clone();
        let generation = self.generation.clone();
        let id = generation.load(Ordering::Relaxed);
        std::thread::spawn(move || {
            let cancelled = || generation.load(Ordering::Relaxed) != id;
            let send = |update| {
                if !cancelled() {
                    let _ = tx.send((id, update));
                }
            };
            let notes = |p: &Project| {
                let path = project::cache_path(p);
                let items = path
                    .as_ref()
                    .map(|path| project::sections(p, path))
                    .unwrap_or_default();
                let watch = path.as_deref().and_then(project::watch_path);
                let rendered = items
                    .into_iter()
                    .map(|section| {
                        let markup = render_markdown(&section.markdown);
                        (section, markup)
                    })
                    .collect();
                Update::Notes(watch, rendered)
            };
            match job {
                Job::Locate(cwd, previous) => {
                    let start = Instant::now();
                    let found = cwd.as_deref().and_then(project::detect);
                    let reload = found != previous;
                    send(Update::Located(found.clone(), reload));
                    if let Some(p) = found
                        && !cancelled()
                    {
                        if reload {
                            let (overview, git, remaining) = project::overview(&p);
                            timing("panel:overview-worker", start);
                            send(Update::Overview(overview, git));
                            send(notes(&p));
                            if !cancelled() {
                                let inventory = project::inventory(&p, remaining, cancelled);
                                timing("panel:inventory-worker", start);
                                send(Update::Inventory(inventory_rows(inventory)));
                            }
                        } else {
                            send(notes(&p));
                        }
                    }
                }
                Job::Git(p) => send(Update::Git(p.read_git())),
                Job::Notes(p) => send(notes(&p)),
            }
        });
    }

    fn poll(self: &Rc<Self>) {
        if !self.is_open() || !self.scroll.is_mapped() {
            return;
        }
        let generation = self.generation.load(Ordering::Relaxed);
        // One update per main-loop slice. Stale messages are drained without drawing.
        while let Ok((id, update)) = self.rx.try_recv() {
            if id == generation {
                self.apply(update);
                return;
            }
        }
        if !self.ready.get() {
            if !self.git_pending.replace(true) {
                let previous = self.displayed.borrow().clone();
                self.spawn(Job::Locate(self.target.borrow().cwd.clone(), previous));
            }
            return;
        }
        let Some(project) = self.project.borrow().clone() else {
            return;
        };
        if self.notes_dirty.get() && !self.notes_pending.replace(true) {
            self.notes_dirty.set(false);
            self.spawn(Job::Notes(project.clone()));
        }
        if project.has_git && Instant::now() >= self.git_at.get() && !self.git_pending.replace(true)
        {
            self.git_at.set(Instant::now() + GIT_INTERVAL);
            self.spawn(Job::Git(project));
        }
    }

    fn apply(self: &Rc<Self>, update: Update) {
        let start = Instant::now();
        match update {
            Update::Located(found, changed) => {
                *self.project.borrow_mut() = found.clone();
                self.ready.set(true);
                self.overview_pending.set(changed && found.is_some());
                self.git_at.set(Instant::now() + GIT_INTERVAL);
                self.git_pending.set(changed && found.is_some());
                self.notes_pending.set(found.is_some());
                self.state.set_visible(found.is_none() || changed);
                self.body.set_visible(found.is_some());
                if found.is_none() {
                    self.displayed.borrow_mut().take();
                    self.name.set_label(&tr("Project"));
                    self.branch.set_label("");
                    self.state
                        .set_label(&tr("This folder does not look like a project"));
                    self.update_ask();
                } else if changed {
                    // Keep the previous project's content until the new overview arrives.
                    self.state.set_label(&tr("Reading…"));
                } else {
                    self.update_ask();
                }
            }
            Update::Overview(summary, git) => {
                self.displayed.borrow_mut().take();
                self.overview_pending.set(false);
                self.show_summary(summary);
                for section in &self.sections[2..] {
                    section.loading();
                }
                self.notes.borrow_mut().clear();
                self.note_paint.set(self.note_paint.get().wrapping_add(1));
                self.notes_initialized.set(false);
                self.last_git.borrow_mut().take();
                self.head.borrow_mut().take();
                self.remove_extras();
                self.show_git(git);
                self.state.set_visible(false);
                self.update_ask();
                if self.scroll.has_focus() {
                    self.sections[0].root.grab_focus();
                }
            }
            Update::Git(git) => self.show_git(git),
            Update::Inventory(inv) => self.show_inventory(inv),
            Update::Notes(path, items) => {
                self.notes_pending.set(false);
                self.watch(path);
                let mut previous = self.notes.borrow_mut();
                let changed = !previous
                    .iter()
                    .map(|(s, _)| s)
                    .eq(items.iter().map(|(s, _)| s));
                *previous = items;
                drop(previous);
                let initialized = self.notes_initialized.replace(true);
                if changed || !initialized {
                    self.show_notes();
                }
            }
        }
        timing("panel:gtk-update", start);
    }

    fn show_summary(&self, summary: Summary) {
        self.name.set_label(&summary.name);
        self.name.set_tooltip_text(Some(&summary.name));
        let body = &self.sections[0].body;
        clear(body);
        optional_row(body, &tr("Readme"), summary.readme.as_deref());
        optional_row(body, &tr("License"), summary.license.as_deref());
        optional_row(body, &tr("Origin"), summary.origin.as_deref());
        if body.first_child().is_none() {
            line(body, &tr("No summary available"), "hint");
        }
    }

    fn show_git(self: &Rc<Self>, result: GitData) {
        self.git_pending.set(false);
        if self.last_git.borrow().as_ref() == Some(&result) {
            return;
        }
        *self.last_git.borrow_mut() = Some(result);
        let saved = self.last_git.borrow();
        let body = &self.sections[1].body;
        clear(body);
        let revision = self.git_paint.get().wrapping_add(1);
        self.git_paint.set(revision);
        let value = match saved.as_ref().expect("set above") {
            None => {
                self.branch.set_label("");
                line(body, &tr("This is not a git repository"), "hint");
                return;
            }
            Some(Err(e)) => {
                self.branch.set_label("");
                git_error(body, *e);
                return;
            }
            Some(Ok(git)) => git,
        };
        let branch = match &value.branch {
            Branch::Named(name) => name.clone(),
            Branch::Detached(sha) => trf("Detached HEAD at {sha}", &[("sha", abbrev(sha))]),
        };
        self.branch.set_label(&branch);
        self.branch.set_tooltip_text(Some(&branch));
        let status = if value.modified + value.new + value.deleted == 0 {
            "ok"
        } else {
            "warn"
        };
        let work = [
            trf("{n} modified", &[("n", &value.modified.to_string())]),
            trf("{n} new", &[("n", &value.new.to_string())]),
            trf("{n} deleted", &[("n", &value.deleted.to_string())]),
        ]
        .join(" / ");
        line(body, &work, status);
        line(
            body,
            &match value.upstream {
                Some((a, b)) => format!(
                    "{} / {}",
                    trf("{n} ahead", &[("n", &a.to_string())]),
                    trf("{n} behind", &[("n", &b.to_string())])
                ),
                None => tr("No upstream"),
            },
            "hint",
        );
        let mut rows = Vec::new();
        if value.commits.is_empty() {
            line(body, &tr("No commits yet"), "hint");
        }
        rows.extend(value.commits.iter().map(|c| {
            let detail = format!("{} · {} · {}", c.sha, c.author, relative(c.seconds));
            (c.title.clone(), detail, "commit")
        }));
        for (title, names) in [
            (t!("Local branches"), &value.local),
            (t!("Remote branches"), &value.remote),
            (t!("Tags"), &value.tags),
        ] {
            if !names.is_empty() {
                rows.push((tr(title), String::new(), "title"));
                rows.extend(
                    names
                        .iter()
                        .map(|name| (name.clone(), String::new(), "ref")),
                );
            }
        }
        let parent = body.clone();
        self.paint_slices(
            "panel:gtk-git",
            revision,
            |p| &p.git_paint,
            rows.into_iter(),
            move |(title, detail, kind)| match kind {
                "title" => line(&parent, &title, "lbl"),
                "ref" => row(&parent, &title, &detail, true),
                _ => row_with_detail(&parent, &title, &detail, false, true),
            },
        );
        if *self.head.borrow() != value.head {
            *self.head.borrow_mut() = value.head.clone();
            if !self.notes.borrow().is_empty() {
                self.show_notes();
            }
        }
    }

    fn paint_slices<T: 'static>(
        self: &Rc<Self>,
        tag: &'static str,
        revision: u64,
        clock: fn(&Self) -> &Cell<u64>,
        mut items: impl Iterator<Item = T> + 'static,
        mut paint: impl FnMut(T) + 'static,
    ) {
        let weak = Rc::downgrade(self);
        glib::idle_add_local(move || {
            let Some(panel) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if clock(&panel).get() != revision {
                return glib::ControlFlow::Break;
            }
            let start = Instant::now();
            for _ in 0..8 {
                let Some(item) = items.next() else {
                    timing(tag, start);
                    return glib::ControlFlow::Break;
                };
                paint(item);
                if start.elapsed() >= Duration::from_millis(3) {
                    break;
                }
            }
            timing(tag, start);
            glib::ControlFlow::Continue
        });
    }

    fn show_inventory(self: &Rc<Self>, groups: [Rows; 4]) {
        let generation = self.generation.load(Ordering::Relaxed);
        let weak = Rc::downgrade(self);
        let mut rows = groups.into_iter().enumerate();
        let mut pending = None;
        glib::idle_add_local(move || {
            let Some(panel) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if panel.generation.load(Ordering::Relaxed) != generation || !panel.is_open() {
                return glib::ControlFlow::Break;
            }
            let start = Instant::now();
            if pending.is_none() {
                let Some((i, rows)) = rows.next() else {
                    *panel.displayed.borrow_mut() = panel.project.borrow().clone();
                    return glib::ControlFlow::Break;
                };
                clear(&panel.sections[i + 2].body);
                pending = Some((i + 2, rows.into_iter()));
            }
            if let Some((i, lines)) = &mut pending {
                for (title, detail, mono) in lines.by_ref().take(8) {
                    row(&panel.sections[*i].body, &title, &detail, mono);
                    if start.elapsed() >= Duration::from_millis(3) {
                        return glib::ControlFlow::Continue;
                    }
                }
                if lines.len() == 0 {
                    pending = None;
                }
            }
            timing("panel:gtk-inventory", start);
            glib::ControlFlow::Continue
        });
    }

    fn watch(self: &Rc<Self>, path: Option<PathBuf>) {
        if self.monitor.borrow().as_ref().map(|(path, _)| path) == path.as_ref() {
            return;
        }
        let Some(path) = path else {
            self.monitor.borrow_mut().take();
            return;
        };
        if let Ok(monitor) = gio::File::for_path(&path)
            .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
        {
            let weak = Rc::downgrade(self);
            monitor.connect_changed(move |_, _, _, _| {
                if let Some(panel) = weak.upgrade() {
                    panel.notes_dirty.set(true);
                }
            });
            *self.monitor.borrow_mut() = Some((path, monitor));
            // Close the gap between the scan and installing the watch (including a new cache dir).
            self.notes_dirty.set(true);
        }
    }

    fn remove_extras(&self) {
        for section in self.extras.take() {
            self.body.remove(&section.root);
        }
    }

    fn show_notes(self: &Rc<Self>) {
        let old_expanded: Vec<_> = self
            .extras
            .borrow()
            .iter()
            .map(|s| {
                (
                    s.root
                        .label_widget()
                        .and_downcast::<gtk::Label>()
                        .map(|l| l.label()),
                    s.root.is_expanded(),
                )
            })
            .collect();
        self.remove_extras();
        let flow = &self.sections[6].body;
        clear(flow);
        let notes = self.notes.borrow();
        if !notes.iter().any(|(s, _)| s.name == "flujo") {
            self.update_ask();
            flow.append(&self.ask);
        }
        let revision = self.note_paint.get().wrapping_add(1);
        self.note_paint.set(revision);
        let mut chunks = Vec::new();
        for (note, markup) in notes.iter() {
            let title = note.title.as_deref().unwrap_or(&note.name);
            if note.name == "flujo" {
                if let Some(title) = &note.title {
                    line(flow, title, "lbl");
                }
                show_note_header(flow, note, self.head.borrow().as_deref());
                chunks.extend(markup.iter().map(|text| (flow.clone(), text.clone())));
            } else {
                let expanded = old_expanded
                    .iter()
                    .any(|(old, expanded)| *expanded && old.as_deref() == Some(title));
                let part = Section::new(title, expanded);
                show_note_header(&part.body, note, self.head.borrow().as_deref());
                chunks.extend(markup.iter().map(|text| (part.body.clone(), text.clone())));
                self.body.append(&part.root);
                self.extras.borrow_mut().push(part);
            }
        }
        self.paint_slices(
            "panel:gtk-notes",
            revision,
            |p| &p.note_paint,
            chunks.into_iter(),
            |(body, text)| append_markup(&body, &text),
        );
    }
}

fn line(parent: &gtk::Box, content: &str, class: &str) {
    let widget = label(content, class);
    widget.add_css_class("project-line");
    parent.append(&widget);
}

fn row(parent: &gtk::Box, title: &str, detail: &str, mono: bool) {
    row_with_detail(parent, title, detail, mono, mono);
}

fn row_with_detail(
    parent: &gtk::Box,
    title: &str,
    detail: &str,
    title_mono: bool,
    detail_mono: bool,
) {
    let row = vertical(2);
    row.add_css_class("project-row");
    let heading = single_line(title, if title_mono { "mono" } else { "lbl" });
    heading.set_tooltip_text(Some(title));
    row.append(&heading);
    if !detail.is_empty() {
        let value = single_line(detail, "hint");
        if detail_mono {
            value.add_css_class("mono");
        }
        value.set_tooltip_text(Some(detail));
        row.append(&value);
    }
    parent.append(&row);
}

fn optional_row(parent: &gtk::Box, title: &str, detail: Option<&str>) {
    if let Some(detail) = detail {
        row(parent, title, detail, false);
    }
}

fn git_error(parent: &gtk::Box, error: GitError) {
    line(parent, &git_error_message(error), "error");
}

fn git_error_message(error: GitError) -> String {
    let reason = tr(match error {
        GitError::Command => t!("git query failed"),
        GitError::Timeout => t!("time limit reached"),
        GitError::OutputLimit => t!("output limit reached"),
    });
    trf("Could not read git: {reason}", &[("reason", &reason)])
}

fn abbrev(sha: &str) -> &str {
    &sha[..sha.floor_char_boundary(7)]
}

fn relative(seconds: i64) -> String {
    let now = glib::DateTime::now_local().map_or(0, |d| d.to_unix());
    let elapsed = now.saturating_sub(seconds).max(0);
    let (key, n) = match elapsed {
        0..=59 => (t!("Just now"), 0),
        60..=3599 => (t!("{n} min ago"), elapsed / 60),
        3600..=86399 => (t!("{n} h ago"), elapsed / 3600),
        _ => (t!("{n} d ago"), elapsed / 86400),
    };
    trf(key, &[("n", &n.to_string())])
}

type Rows = Vec<(String, String, bool)>;

fn inventory_rows(inv: Inventory) -> [Rows; 4] {
    let mut tech = Vec::new();
    let total = inv.languages.iter().map(|(_, size)| size).sum::<u64>();
    let mut other = 0;
    for (i, (name, size)) in inv.languages.into_iter().enumerate() {
        if i < 6 {
            tech.push((
                name.into(),
                format!("{:.1} %", 100.0 * size as f64 / total as f64),
                false,
            ));
        } else {
            other += size;
        }
    }
    if other > 0 {
        tech.push((
            tr("Other"),
            format!("{:.1} %", 100.0 * other as f64 / total as f64),
            false,
        ));
    }
    if !inv.frameworks.is_empty() {
        tech.push((tr("Frameworks"), inv.frameworks.join(", "), false));
    }
    for m in inv.manifests {
        let identity = [m.package, m.version]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
        tech.push((m.file, identity, false));
        tech.extend(
            m.edition
                .into_iter()
                .chain(m.dependencies)
                .map(|(name, value)| (name, value, false)),
        );
    }
    let runs = inv
        .runs
        .into_iter()
        .map(|(source, command)| {
            let source = match source.as_str() {
                "docs" => tr("Documentation"),
                "script" => tr("Script"),
                _ => source,
            };
            (source, command, true)
        })
        .collect::<Vec<_>>();
    let mut dirs = inv
        .directories
        .into_iter()
        .map(|(name, files, lines)| {
            (
                name,
                trf(
                    "{files} files · {lines} lines",
                    &[("files", &files.to_string()), ("lines", &lines.to_string())],
                ),
                false,
            )
        })
        .collect::<Vec<_>>();
    if let Some(e) = inv.listing_error {
        let notice = (git_error_message(e), String::new(), false);
        tech.push(notice.clone());
        dirs.push(notice);
    }
    if inv.limited {
        let notice = (
            tr("Partial counts · file reading limit reached"),
            String::new(),
            false,
        );
        tech.push(notice.clone());
        dirs.push(notice);
    }
    let mut ci: Rows = inv
        .workflows
        .into_iter()
        .map(|(name, triggers)| (name, triggers, false))
        .collect();
    ci.extend(
        inv.quality
            .into_iter()
            .map(|config| (config, String::new(), false)),
    );
    let mut all = [tech, runs, dirs, ci];
    for rows in &mut all {
        if rows.is_empty() {
            rows.push((tr("Nothing detected"), String::new(), false));
        }
    }
    all
}

fn render_markdown(text: &str) -> Vec<Arc<str>> {
    let escape = |text: &str| glib::markup_escape_text(text);
    let inline = |parts: Vec<Inline>| -> String {
        parts
            .into_iter()
            .map(|p| match p {
                Inline::Text(text) => escape(&text).to_string(),
                Inline::Code(code) => format!("<tt>{}</tt>", escape(&code)),
                Inline::Strong(text) => format!("<b>{}</b>", escape(&text)),
                Inline::Link { text, url } => format!(
                    "<a href=\"{}\">{} ({})</a>",
                    escape(&url),
                    escape(&text),
                    escape(&url)
                ),
            })
            .collect()
    };
    let mut items = Vec::new();
    for block in project::markdown(text) {
        match block {
            Block::Heading(parts) => items.push(format!("<b>{}</b>", inline(parts))),
            Block::Paragraph(parts) => items.push(inline(parts)),
            Block::Code(code) => {
                let mut tail = code.as_str();
                while !tail.is_empty() {
                    let limit = tail.floor_char_boundary(1024);
                    let end = tail[..limit].rfind('\n').map_or(limit, |i| i + 1);
                    let (chunk, rest) = tail.split_at(end);
                    items.push(format!("<tt>{}</tt>", escape(chunk.trim_end_matches('\n'))));
                    tail = rest;
                }
            }
            Block::List(list) => items.extend(list.into_iter().map(|p| format!("• {}", inline(p)))),
            Block::Table(rows) => items.extend(rows.into_iter().enumerate().map(|(i, cells)| {
                let row = cells
                    .into_iter()
                    .map(&inline)
                    .collect::<Vec<_>>()
                    .join("  │  ");
                if i == 0 { format!("<b>{row}</b>") } else { row }
            })),
        }
    }
    let mut chunks = Vec::new();
    let mut chunk = String::new();
    for block in items {
        if chunk.len() + block.len() > 4096 && !chunk.is_empty() {
            chunks.push(Arc::from(std::mem::take(&mut chunk)));
        }
        if !chunk.is_empty() {
            chunk.push('\n');
        }
        chunk.push_str(&block);
    }
    if !chunk.is_empty() {
        chunks.push(Arc::from(chunk));
    }
    chunks
}

fn show_note_header(body: &gtk::Box, note: &AgentSection, head: Option<&str>) {
    let time = glib::DateTime::from_unix_local(note.written_at as i64)
        .ok()
        .and_then(|date| date.format("%H:%M").ok())
        .unwrap_or_default();
    line(
        body,
        &trf("Written by the agent · {time}", &[("time", &time)]),
        "hint",
    );
    if note.head.as_deref() != head {
        let old = note.head.as_deref().map(abbrev).unwrap_or("—");
        line(
            body,
            &trf("May be out of date (written at {sha})", &[("sha", old)]),
            "warn",
        );
    }
    if note.truncated {
        line(body, &tr("Content clipped at 64 KB"), "warn");
    }
}

fn append_markup(body: &gtk::Box, markup: &str) {
    let label = label("", "project-line");
    label.set_markup(markup);
    label.connect_activate_link(|label, uri| {
        let Some(parent) = label.root().and_downcast::<gtk::Window>() else {
            return glib::Propagation::Stop;
        };
        let dialog = gtk::AlertDialog::builder()
            .modal(true)
            .message(tr("Open external link?"))
            .detail(uri)
            .buttons([tr("Cancel"), tr("Open link")])
            .cancel_button(0)
            .default_button(0)
            .build();
        let url = uri.to_owned();
        let window = parent.downgrade();
        dialog.choose(Some(&parent), gio::Cancellable::NONE, move |response| {
            if response == Ok(1)
                && let Some(parent) = window.upgrade()
            {
                gtk::UriLauncher::new(&url).launch(Some(&parent), gio::Cancellable::NONE, |_| {});
            }
        });
        glib::Propagation::Stop
    });
    body.append(&label);
}
