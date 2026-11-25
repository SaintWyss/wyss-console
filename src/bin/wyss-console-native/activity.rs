//! Usage strip and per-pane prompt queue. Only the host reads usage sources or writes the PTY.
use crate::{
    host::Host,
    i18n::{tr, trf},
};
use gtk::{gio, glib, prelude::*};
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wyss_console::native::{
    catalogue::AgentId,
    prompt_queue::{self, Arming, Dispatch, Edit, Queue},
    service::{ActivitySession, ActivityView},
    store,
    usage::{self, Level, Plan},
};

pub struct Strip {
    pub root: gtk::Box,
    claude: Column,
    codex: Column,
    closed: Option<ClosedQueues>,
}

struct Column {
    root: gtk::Box,
    context: Meter,
    short: Meter,
    long: Meter,
}

/// One reading: its name and the percentage, which turns amber or red as it fills.
struct Meter {
    cell: gtk::Box,
    value: gtk::Label,
}

impl Strip {
    pub fn new(host: Option<Host>) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("usage-strip");
        let columns = gtk::Box::new(gtk::Orientation::Horizontal, 18);
        let claude = Column::new(&columns, "Claude Code");
        let codex = Column::new(&columns, "Codex");
        root.append(&columns);
        let closed = host.map(|host| ClosedQueues::new(&root, host));
        let result = Self {
            root,
            claude,
            codex,
            closed,
        };
        result.paint(&ActivityView::default(), None);
        result
    }

    pub fn paint(&self, activity: &ActivityView, focused: Option<&str>) {
        let for_provider = |provider| {
            activity
                .sessions
                .iter()
                .find(|s| s.agent == provider && Some(s.id.as_str()) == focused)
                .or_else(|| {
                    activity
                        .sessions
                        .iter()
                        .rev()
                        .filter(|s| s.agent == provider)
                        .reduce(|latest, s| {
                            if s.started_at > latest.started_at {
                                s
                            } else {
                                latest
                            }
                        })
                })
        };
        let now = store::now();
        self.claude.paint(
            activity.plans.claude,
            for_provider(AgentId::ClaudeCode),
            now,
        );
        self.codex
            .paint(activity.plans.codex, for_provider(AgentId::Codex), now);
        if let Some(closed) = &self.closed {
            closed.paint(&activity.sessions);
        }
    }
}

struct ClosedQueues {
    button: gtk::MenuButton,
    list: gtk::Box,
    host: Host,
    panels: RefCell<Vec<Rc<QueuePanel>>>,
}

impl ClosedQueues {
    fn new(root: &gtk::Box, host: Host) -> Self {
        let button = gtk::MenuButton::builder().label(tr("Saved queues")).build();
        button.set_valign(gtk::Align::Center);
        let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let scroll = gtk::ScrolledWindow::builder()
            .min_content_height(115)
            .max_content_height(240)
            .propagate_natural_height(true)
            .child(&list)
            .build();
        let popover = gtk::Popover::new();
        popover.set_child(Some(&scroll));
        button.set_popover(Some(&popover));
        root.append(&button);
        Self {
            button,
            list,
            host,
            panels: RefCell::new(Vec::new()),
        }
    }

    fn paint(&self, sessions: &[ActivitySession]) {
        let closed = sessions
            .iter()
            .rev()
            .filter(|s| s.terminal_id.is_none() && s.queued > 0);
        let mut panels = self.panels.borrow_mut();
        if !panels
            .iter()
            .map(|p| p.id.as_str())
            .eq(closed.clone().map(|s| s.id.as_str()))
        {
            while let Some(child) = self.list.first_child() {
                self.list.remove(&child);
            }
            *panels = closed
                .map(|s| {
                    let panel = QueuePanel::new(
                        self.host.clone(),
                        s.id.clone(),
                        None,
                        s.agent == AgentId::Codex,
                    );
                    panel.button.set_label(&s.name);
                    self.list.append(&panel.button);
                    panel
                })
                .collect();
        }
        self.button.set_visible(!panels.is_empty());
    }
}

fn one_line() -> gtk::Label {
    let label = gtk::Label::new(None);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label
}

impl Column {
    fn new(root: &gtk::Box, title: &str) -> Self {
        let col = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        col.add_css_class("usage-column");
        let heading = gtk::Label::new(Some(title));
        heading.add_css_class("usage-provider");
        col.append(&heading);
        let context = Meter::new(&col, &tr("Context"));
        let short = Meter::new(&col, "5 h");
        let long = Meter::new(&col, &tr("Week"));
        root.append(&col);
        Self {
            root: col,
            context,
            short,
            long,
        }
    }

    fn paint(&self, plan: Plan, session: Option<&ActivitySession>, now: u64) {
        // A provider with nothing to show takes no room in the bar.
        self.root.set_visible(session.and_then(|s| s.context).is_some() || plan.five_hours.is_some() || plan.weekly.is_some());
        self.context
            .set(session.and_then(|s| s.context), None, false);
        // ponytail: only the 5 h window shows its reset; the weekly one is too far away to matter.
        // A stale reading shows its age instead: its reset may have passed already.
        let note = |w: Option<usage::Window>, reset: bool| {
            let w = w?;
            if w.stale(now) {
                return Some(trf("{age} ago", &[("age", &age(now.saturating_sub(w.at)))]));
            }
            let mins = w.resets_at.filter(|_| reset)?.saturating_sub(now).div_ceil(60);
            Some(format!("{}h {}m", mins / 60, mins % 60))
        };
        for (meter, w, reset) in [(&self.short, plan.five_hours, true), (&self.long, plan.weekly, false)] {
            meter.set(w.map(|r| r.used), note(w, reset), w.is_some_and(|r| r.stale(now)));
        }
    }
}

/// Age of a reading in its largest whole unit: 7m, 3h, 2d.
fn age(secs: u64) -> String {
    match secs / 60 {
        m if m < 60 => format!("{m}m"),
        m if m < 48 * 60 => format!("{}h", m / 60),
        m => format!("{}d", m / (24 * 60)),
    }
}

impl Meter {
    fn new(row: &gtk::Box, name: &str) -> Self {
        let cell = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let label = gtk::Label::new(Some(name));
        label.add_css_class("usage-name");
        let value = gtk::Label::new(None);
        value.add_css_class("usage-value");
        cell.append(&label);
        cell.append(&value);
        row.append(&cell);
        Self { cell, value }
    }

    fn set(&self, used: Option<f64>, extra: Option<String>, stale: bool) {
        for class in ["usage-warning", "usage-danger", "usage-stale"] {
            self.value.remove_css_class(class);
        }
        self.cell.set_visible(used.is_some());
        let Some(used) = used else { return };
        match usage::context_level(used) {
            Level::Normal => {}
            Level::Warning => self.value.add_css_class("usage-warning"),
            Level::Danger => self.value.add_css_class("usage-danger"),
        }
        let text = extra.map_or_else(|| percent(used), |e| format!("{} · {e}", percent(used)));
        self.value.set_text(&text);
        if stale {
            self.value.add_css_class("usage-stale");
        }
        self.value
            .set_tooltip_text(stale.then(|| tr("Stale reading")).as_deref());
    }
}

fn percent(value: f64) -> String {
    // Whole number, floored, so 74.9 never reads as the 75 that turns the bar red.
    format!("{}%", value.floor())
}

pub fn snapshot(host: Option<&Host>) -> (ActivityView, bool) {
    if let Some(activity) = host
        .and_then(|host| host.call("native_activity", json!({})).ok())
        .and_then(|v| serde_json::from_value(v).ok())
    {
        return (activity, true);
    }
    let plans = wyss_console::native::state_paths()
        .map_or_else(usage::Plans::default, |(base, _)| {
            usage::cached_plans(&base.join("usage.json"))
        });
    (
        ActivityView {
            plans,
            sessions: Vec::new(),
        },
        false,
    )
}

#[derive(Clone, Copy)]
struct Observation {
    input: Option<u64>,
    local_input: u64,
    turn: Option<prompt_queue::Turn>,
    revision: Option<u64>,
    count: usize,
    running: bool,
}

pub struct QueuePanel {
    pub button: gtk::MenuButton,
    host: Host,
    id: String,
    tid: Option<String>,
    armed: Cell<Arming>,
    observed: Cell<Option<Observation>>,
    queue: RefCell<Option<Queue>>,
    busy: Cell<bool>,
    popover: gtk::Popover,
    list: gtk::ListBox,
    editor: gtk::TextView,
    status: gtk::Label,
    arm: gtk::Button,
    pause: gtk::Button,
    edits: Vec<gtk::Button>,
}

impl QueuePanel {
    pub fn new(host: Host, id: String, tid: Option<String>, codex: bool) -> Rc<Self> {
        // A label child instead of `.label()`: no dropdown arrow, so it matches Stop and Restart.
        let button = gtk::MenuButton::builder().child(&gtk::Label::new(Some(&tr("Queue")))).build();
        button.add_css_class("hdr-btn");
        button.set_tooltip_text(Some(&tr("Prompt queue")));
        let popover = gtk::Popover::new();
        button.set_popover(Some(&popover));
        let body = gtk::Box::new(gtk::Orientation::Vertical, 6);
        body.set_size_request(345, -1);
        popover.set_child(Some(&body));
        let status = gtk::Label::new(None);
        status.set_wrap(true);
        status.set_max_width_chars(46);
        status.set_xalign(0.0);
        body.append(&status);

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        let scroll = gtk::ScrolledWindow::builder()
            .min_content_height(105)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&list)
            .build();
        body.append(&scroll);
        let editor = gtk::TextView::new();
        editor.set_wrap_mode(gtk::WrapMode::WordChar);
        editor.update_property(&[gtk::accessible::Property::Label(&tr("Queued prompt text"))]);
        let edit_scroll = gtk::ScrolledWindow::builder()
            .min_content_height(65)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&editor)
            .build();
        body.append(&edit_scroll);

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let edits: Vec<_> = [
            tr("Add"),
            tr("Save edit"),
            tr("Move up"),
            tr("Move down"),
            tr("Delete"),
        ]
        .iter()
        .map(|text| gtk::Button::with_label(text))
        .collect();
        for button in &edits {
            actions.append(button);
        }
        body.append(&actions);

        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let arm = gtk::Button::with_label(&tr("Arm queue"));
        let pause = gtk::Button::with_label(&tr("Pause queue"));
        controls.append(&arm);
        controls.append(&pause);
        body.append(&controls);
        let help = gtk::Label::new(Some(&if tid.is_none() {
            tr("Session stopped. You can edit saved prompts.")
        } else if codex {
            tr("Codex does not expose whether its terminal is ready to receive a queued prompt.")
        } else {
            tr("Arm during a turn with the prompt empty. Typing pauses the queue.")
        }));
        help.set_xalign(0.0);
        help.set_wrap(true);
        help.set_max_width_chars(46);
        body.append(&help);

        let panel = Rc::new(Self {
            button,
            host,
            id,
            tid,
            armed: Cell::new(Arming::default()),
            observed: Cell::new(None),
            queue: RefCell::new(None),
            busy: Cell::new(false),
            popover,
            list,
            editor,
            status,
            arm,
            pause,
            edits,
        });
        panel.connect();
        panel.paused(&tr("Queue paused"));
        panel
    }

    fn connect(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.popover.connect_show(move |_| {
            if let Some(p) = weak.upgrade() {
                p.request("native_queue", json!({"id":p.id}), false);
            }
        });
        let weak = Rc::downgrade(self);
        self.arm.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                p.arm();
            }
        });
        let weak = Rc::downgrade(self);
        self.pause.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                p.paused(&tr("Queue paused"));
            }
        });
        let weak = Rc::downgrade(self);
        self.list.connect_row_selected(move |_, row| {
            if let Some(p) = weak.upgrade() {
                let queue = p.queue.borrow();
                let value = row.and_then(|r| queue.as_ref()?.entries.get(r.index() as usize));
                p.editor.buffer().set_text(value.map_or("", String::as_str));
                drop(queue);
                p.sensitivity();
            }
        });
        for (i, button) in self.edits.iter().enumerate() {
            let weak = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(p) = weak.upgrade() {
                    p.edit(i);
                }
            });
        }
    }

    fn edit(self: &Rc<Self>, action: usize) {
        let index = self.list.selected_row().map(|r| r.index() as usize);
        let text = || {
            let buffer = self.editor.buffer();
            buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), true)
                .to_string()
        };
        let edit = match (action, index) {
            (0, _) => Edit::Add { text: text() },
            (1, Some(index)) => Edit::Replace {
                index,
                text: text(),
            },
            (2, Some(index)) => Edit::Move { index, up: true },
            (3, Some(index)) => Edit::Move { index, up: false },
            (4, Some(index)) => Edit::Remove { index },
            _ => return,
        };
        let Some(revision) = self.queue.borrow().as_ref().map(|q| q.revision) else {
            return;
        };
        self.paused(&tr("Queue paused"));
        self.request(
            "native_queue_edit",
            json!({"id":self.id, "revision":revision, "edit":edit}),
            false,
        );
    }

    fn request(self: &Rc<Self>, op: &'static str, args: serde_json::Value, send: bool) {
        if self.busy.replace(true) {
            return;
        }
        self.sensitivity();
        let host = self.host.clone();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                use crate::host::HostError;
                let value = host.call(op, args)?;
                serde_json::from_value::<Queue>(value)
                    .map_err(|e| HostError::Protocol(e.to_string()))
            })
            .await;
            if let Some(panel) = weak.upgrade() {
                panel.busy.set(false);
                match result {
                    Ok(Ok(queue)) => panel.display(queue),
                    Ok(Err(crate::host::HostError::Remote { code, .. }))
                        if code.starts_with("queue_") =>
                    {
                        panel.paused(&crate::native::failure_text(&code));
                    }
                    _ if send => panel.paused(&tr("Queue paused. Send could not be confirmed.")),
                    _ => panel.paused(&tr(
                        "Queue change unavailable. Check the item or reopen the panel.",
                    )),
                }
                panel.sensitivity();
            }
        });
    }

    fn display(&self, queue: Queue) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        if queue.uncertain {
            self.paused(&tr(
                "Queue paused. Review and edit the first item before arming.",
            ));
        } else if queue.entries.is_empty() && self.armed.get().active() {
            self.paused(&tr("Queue is empty"));
        }
        for entry in &queue.entries {
            let label = one_line();
            label.set_text(entry.lines().next().unwrap_or_default());
            self.list.append(&label);
        }
        self.queue.replace(Some(queue));
        self.editor.buffer().set_text("");
    }

    fn sensitivity(&self) {
        let queue = self.queue.borrow();
        let ready = !self.busy.get() && queue.is_some();
        let count = queue.as_ref().map_or(0, |q| q.entries.len());
        let selected = self.list.selected_row().map(|row| row.index() as usize);
        self.edits[0].set_sensitive(ready && count < prompt_queue::MAX_ITEMS);
        self.edits[1].set_sensitive(ready && selected.is_some());
        self.edits[2].set_sensitive(ready && selected.is_some_and(|i| i > 0));
        self.edits[3].set_sensitive(ready && selected.is_some_and(|i| i + 1 < count));
        self.edits[4].set_sensitive(ready && selected.is_some());
        self.arm.set_sensitive(false);
        self.arm.set_tooltip_text(Some(&tr("Automatic sending is off until the agent's idle screen can be verified.")));
        self.pause
            .set_sensitive(self.armed.get().active() && !self.busy.get());
    }

    fn paused(&self, why: &str) {
        self.armed.set(Arming::Paused);
        self.status.set_text(why);
        self.sensitivity();
    }

    fn arm(&self) {
        if let Some(obs) = self.observed.get()
            && let (Some(input), Some(turn)) = (obs.input, obs.turn)
            && turn.can_arm()
        {
            self.armed.set(Arming::arm(input, obs.local_input, turn));
            self.status
                .set_text(&tr("Armed. Waiting for the session to become idle."));
            self.sensitivity();
        }
    }

    pub fn observe(
        self: &Rc<Self>,
        activity: Option<&ActivitySession>,
        (bracketed, local_input): (bool, u64),
    ) {
        let observation = activity.map(|s| Observation {
            input: s.input_revision,
            local_input,
            turn: s.turn,
            revision: s.queue_revision,
            count: s.queued,
            running: self.tid.is_some() && s.terminal_id == self.tid,
        });
        self.observed.set(observation);
        self.sensitivity();
        let Arming::Armed {
            input,
            local_input,
            turn: mut target,
            mut sent,
        } = self.armed.get()
        else {
            return;
        };
        if self.busy.get() {
            return;
        }
        let Some(obs) = observation.filter(|o| o.running) else {
            return self.paused(&tr("Queue paused. Session or signal unavailable."));
        };
        if obs.input != Some(input) || obs.local_input != local_input {
            return self.paused(&tr("Queue paused because the terminal received input."));
        }
        if obs.count == 0 {
            return self.paused(&tr("Queue is empty"));
        }
        let revision = self.queue.borrow().as_ref().map(|q| q.revision);
        if revision != obs.revision {
            // A poll already in flight can finish after a send acknowledgment.
            if obs.revision < revision {
                return;
            }
            return self.paused(&tr(
                "Queue change unavailable. Check the item or reopen the panel.",
            ));
        }
        let Some(turn) = obs.turn else {
            return self.paused(&tr("Queue paused. Session or signal unavailable."));
        };
        if turn.id != target {
            if !sent || !turn.can_arm() {
                return self.paused(&tr("Queue paused. Could not verify the next turn."));
            }
            target = turn.id;
            sent = false;
            self.armed.set(Arming::arm(input, local_input, turn));
        }
        match prompt_queue::dispatch(
            obs.running,
            bracketed,
            Some(turn),
            sent.then_some(target),
            store::now(),
        ) {
            Dispatch::Wait => {}
            Dispatch::Pause => self.paused(&tr("Queue paused. The session may need input.")),
            Dispatch::Send => {
                self.armed.set(Arming::Armed {
                    input,
                    local_input,
                    turn: target,
                    sent: true,
                });
                self.request(
                    "native_queue_send",
                    json!({
                        "id":self.id, "terminal_id":self.tid, "turn":turn.id,
                        "revision":obs.revision, "input_revision":input,
                    }),
                    true,
                );
            }
        }
    }
}

#[cfg(test)]
mod age_tests {
    #[test]
    fn age_uses_the_largest_whole_unit() {
        assert_eq!(super::age(59), "0m");
        assert_eq!(super::age(15 * 60), "15m");
        assert_eq!(super::age(3 * 3600 + 59), "3h");
        assert_eq!(super::age(47 * 3600), "47h");
        assert_eq!(super::age(3 * 86400), "3d");
    }
}
