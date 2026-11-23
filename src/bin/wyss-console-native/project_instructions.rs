use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    path::Path,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

use gtk::{glib, prelude::*};
use wyss_console::{
    arsenal::{
        environment,
        project_instructions::{Change, Omission, Remembered, Snapshot},
        role_instructions, suggestion,
    },
    native::arsenal::{ArsenalLaunch, InstructionReview},
};

use crate::{
    i18n::{tr, trf},
    t,
};

pub struct Review {
    snapshot: Arc<Snapshot>,
    choice: Option<role_instructions::Choice>,
    remembered: Remembered,
    suggestion: Result<suggestion::Request, String>,
    pinned_model: Option<String>,
    answer: mpsc::SyncSender<Decision>,
}

struct Decision {
    selection: BTreeSet<String>,
    review: InstructionReview,
}

/// Runs on a launch worker. Only the dialog and its widget updates run on GTK's thread.
pub fn select(
    launch: &mut ArsenalLaunch,
    provider: &str,
    project: &str,
    restart: bool,
    show: impl FnOnce(Review),
) -> Result<bool, String> {
    let read_error = || tr(t!("The project instruction files could not be inspected."));
    let snapshot = Arc::new(Snapshot::read(Path::new(project)).map_err(|_| read_error())?);
    if snapshot.sources.is_empty() && !(restart && launch.project_instructions_reviewed) {
        return Ok(true);
    }
    let data_error = || {
        tr(t!(
            "The choices for these project instructions could not be read or saved."
        ))
    };
    let directory = environment::directory().ok_or_else(data_error)?;
    let choice = if restart {
        match role_instructions::get(&directory, &launch.selection).map_err(|_| data_error())? {
            role_instructions::Choice::Load => None,
            choice => Some(choice),
        }
    } else {
        launch.launch_instructions.clone()
    };
    if snapshot.sources.is_empty() {
        launch.apply_review(InstructionReview {
            choice,
            reviewed: false,
            model: None,
        });
        return Ok(true);
    }
    let remembered = snapshot
        .remembered(&directory, &launch.selection)
        .map_err(|_| data_error())?;
    let pinned_model = launch.suggestion_model.clone();
    let suggestion = suggestion_for(provider, pinned_model.as_deref());
    let (answer, receive) = mpsc::sync_channel(1);
    show(Review {
        snapshot: snapshot.clone(),
        choice,
        remembered,
        suggestion,
        pinned_model,
        answer,
    });
    let Ok(decision) = receive.recv() else {
        return Ok(false);
    };
    snapshot
        .remember(&directory, &launch.selection, &decision.selection)
        .map_err(|_| data_error())?;
    launch.apply_review(decision.review);
    Ok(true)
}

fn suggestion_for(
    provider: &str,
    pinned_model: Option<&str>,
) -> Result<suggestion::Request, String> {
    if provider == "codex" {
        return Err(tr(t!(
            "Codex CLI 0.170.0 cannot run with all tools disabled."
        )));
    }
    let cli_error = || {
        tr(t!(
            "This Claude Code CLI does not advertise a tool-free query."
        ))
    };
    let home = std::env::var_os("HOME").ok_or_else(cli_error)?;
    let cli = wyss_console::native::catalogue::discover_name(
        "claude",
        std::env::var_os("PATH"),
        Path::new(&home),
    )
    .ok_or_else(cli_error)?;
    let model = pinned_model
        .map(str::to_owned)
        .or_else(|| std::env::var("ANTHROPIC_MODEL").ok());
    if std::env::var_os("ANTHROPIC_BASE_URL").is_some()
        || [
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_USE_VERTEX",
            "CLAUDE_CODE_USE_FOUNDRY",
        ]
        .iter()
        .any(|key| std::env::var_os(key).is_some_and(|value| value == "1"))
    {
        return Err(tr(t!("The model could not be pinned for this launch.")));
    }
    suggestion::Request::prepare(cli, model.as_deref()).map_err(|error| match error {
        suggestion::Unavailable::Model => tr(t!("The model could not be pinned for this launch.")),
        suggestion::Unavailable::Tools => cli_error(),
        suggestion::Unavailable::Auth => tr(t!(
            "The tool-free query needs an Anthropic API key in the environment."
        )),
        suggestion::Unavailable::ReadOnly => {
            tr(t!("The read-only process sandbox is unavailable."))
        }
    })
}

struct ReviewState {
    snapshot: Arc<Snapshot>,
    choice: Option<role_instructions::Choice>,
    checks: Vec<(String, gtk::CheckButton)>,
    preview: gtk::TextBuffer,
    start: gtk::Button,
    status: gtk::Label,
    suggest: gtk::Button,
    hint: gtk::Label,
    request: Option<Arc<suggestion::Request>>,
    pending: RefCell<Option<Arc<AtomicBool>>>,
    used_model: RefCell<Option<String>>,
    updating: Cell<bool>,
}

impl ReviewState {
    fn selected(&self) -> BTreeSet<String> {
        self.checks
            .iter()
            .filter(|(_, check)| check.is_active())
            .map(|(id, _)| id.clone())
            .collect()
    }

    fn refresh(&self) {
        if self.updating.get() {
            return;
        }
        let composition = self
            .snapshot
            .compose(self.choice.as_ref(), &self.selected());
        self.preview.set_text(&composition.text);
        let excess = composition.excess();
        self.start
            .set_sensitive(excess == 0 && self.pending.borrow().is_none());
        self.status.set_label(&if excess > 0 {
            trf(
                t!("Uncheck at least {bytes} bytes to fit the 64 KiB limit."),
                &[("bytes", &excess.to_string())],
            )
        } else if composition.text.is_empty() {
            tr(t!(
                "The session will exclude the project instruction files."
            ))
        } else {
            String::new()
        });
    }

    fn suggestion(self: &Rc<Self>) {
        let Some(request) = self.request.clone() else {
            return;
        };
        if let Some(cancel) = self.pending.borrow().as_ref() {
            cancel.store(true, Ordering::Release);
            self.suggest.set_sensitive(false);
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.pending.replace(Some(cancel.clone()));
        self.suggest.set_label(&tr(t!("Cancel suggestion")));
        self.hint
            .set_label(&tr(t!("Reading sections with Claude Code…")));
        self.refresh();
        let snapshot = self.snapshot.clone();
        let running = cancel.clone();
        let model = request.model().to_owned();
        let (send, receive) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _ = send.send(request.run(&snapshot, &running));
        });
        let state = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(50), move || {
            let Some(state) = state.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let result = match receive.try_recv() {
                Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => Err(suggestion::Failure::Process),
                Ok(result) => result,
            };
            state.pending.replace(None);
            state.suggest.set_label(&tr(t!("Suggest")));
            state.suggest.set_sensitive(true);
            if cancel.load(Ordering::Acquire) {
                state.hint.set_label("");
                state.refresh();
                return glib::ControlFlow::Break;
            }
            match result {
                Ok(picks) => {
                    state.used_model.replace(Some(model.clone()));
                    state.updating.set(true);
                    for (id, check) in &state.checks {
                        let reason = picks.get(id);
                        check.set_tooltip_text(reason.map(String::as_str));
                        if reason.is_some() {
                            check.set_active(true);
                        }
                    }
                    state.updating.set(false);
                    state.hint.set_label(&tr(t!("Review the suggested choices before starting. Hover over a suggested section to see its reason.")));
                }
                Err(error) => state.hint.set_label(&match error {
                    suggestion::Failure::Output => {
                        tr(t!("Claude Code returned an invalid section selection."))
                    }
                    suggestion::Failure::Timeout => tr(t!("The suggestion timed out.")),
                    suggestion::Failure::Cancelled | suggestion::Failure::Process => tr(t!(
                        "Claude Code could not complete the tool-free suggestion."
                    )),
                }),
            }
            state.refresh();
            glib::ControlFlow::Break
        });
    }
}

fn label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .build()
}

impl Review {
    pub fn show(self, parent: &impl IsA<gtk::Window>) -> gtk::Window {
        let window = gtk::Window::builder()
            .title(tr(t!("Project instructions")))
            .transient_for(parent)
            .modal(true)
            .destroy_with_parent(true)
            .default_width(920)
            .default_height(600)
            .build();
        let cancel_button = gtk::Button::with_label(&tr(t!("Cancel")));
        let start = gtk::Button::with_label(&tr(t!("Start")));
        start.add_css_class("suggested-action");

        let sources = gtk::Box::new(gtk::Orientation::Vertical, 8);
        sources.set_margin_end(8);
        let mut checks = Vec::new();
        let mut groups = Vec::new();
        for source in &self.snapshot.sources {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            let file = label(&source.file);
            file.add_css_class("heading");
            file.set_hexpand(true);
            row.append(&file);
            let all = gtk::Button::with_label(&tr(t!("Select all")));
            let none = gtk::Button::with_label(&tr(t!("Select none")));
            all.set_sensitive(!source.sections.is_empty());
            none.set_sensitive(!source.sections.is_empty());
            row.append(&all);
            row.append(&none);
            sources.append(&row);
            let first = checks.len();
            for section in &source.sections {
                let heading = if section.heading_path.is_empty() {
                    tr(t!("Before the first heading"))
                } else {
                    section.heading_path.join(" › ")
                };
                let heading = if section.file == source.file {
                    heading
                } else {
                    format!("@{} · {heading}", section.file)
                };
                let (selected, change) = self.remembered.status(section);
                let title = match change {
                    Change::New => trf(t!("{section} · New"), &[("section", &heading)]),
                    Change::Changed => trf(t!("{section} · Changed"), &[("section", &heading)]),
                    Change::Unchanged => heading,
                };
                let check = gtk::CheckButton::new();
                check.set_child(Some(&label(&title)));
                check.set_active(selected);
                sources.append(&check);
                checks.push((section.id.clone(), check));
            }
            groups.push((all, none, first..checks.len()));
            if source.sections.is_empty() && source.omitted.is_empty() {
                sources.append(&label(&tr(t!("No sections."))));
            }
            for (path, reason) in &source.omitted {
                let omitted = match reason {
                    Omission::UnsafeImport => trf(
                        t!("{file} · Not included: outside the project."),
                        &[("file", path)],
                    ),
                    Omission::Unreadable => trf(
                        t!("{file} · Not included: missing, unreadable or not a text file."),
                        &[("file", path)],
                    ),
                    Omission::TooLarge => trf(
                        t!("{file} · Not included: file or total content exceeds the size limit."),
                        &[("file", path)],
                    ),
                    Omission::Limit => trf(
                        t!("{file} · Remaining sections omitted due to the section limit."),
                        &[("file", path)],
                    ),
                };
                sources.append(&label(&omitted));
            }
        }
        let preview = gtk::TextView::builder()
            .monospace(true)
            .editable(false)
            .wrap_mode(gtk::WrapMode::WordChar)
            .build();
        let left = gtk::ScrolledWindow::builder()
            .child(&sources)
            .min_content_width(325)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let right = gtk::ScrolledWindow::builder()
            .child(&preview)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let preview_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        preview_box.append(&label(&tr(t!(
            "Exact preset instructions for this session"
        ))));
        preview_box.append(&right);
        let panes = gtk::Paned::builder()
            .start_child(&left)
            .end_child(&preview_box)
            .resize_start_child(false)
            .position(365)
            .vexpand(true)
            .build();

        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let suggest = gtk::Button::with_label(&tr(t!("Suggest")));
        let can_suggest = self.suggestion.is_ok() && !checks.is_empty();
        suggest.set_sensitive(can_suggest);
        let hint = label(&match &self.suggestion {
            Err(reason) => reason.clone(),
            Ok(_) if !can_suggest => tr(t!("There are no sections to suggest.")),
            Ok(request) => trf(
                t!("Send the sections to {model} for a suggestion."),
                &[("model", request.model())],
            ),
        });
        row.append(&suggest);
        row.append(&hint);
        let status = label("");
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        actions.set_halign(gtk::Align::End);
        actions.append(&cancel_button);
        actions.append(&start);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
        content.set_margin_top(12);
        content.set_margin_bottom(12);
        content.set_margin_start(12);
        content.set_margin_end(12);
        content.append(&panes);
        content.append(&row);
        content.append(&status);
        content.append(&actions);
        window.set_child(Some(&content));

        let state = Rc::new(ReviewState {
            snapshot: self.snapshot,
            choice: self.choice,
            checks,
            preview: preview.buffer(),
            start,
            status,
            suggest,
            hint,
            request: self.suggestion.ok().map(Arc::new),
            pending: RefCell::new(None),
            used_model: RefCell::new(self.pinned_model),
            updating: Cell::new(false),
        });
        for (_, check) in &state.checks {
            let weak = Rc::downgrade(&state);
            check.connect_toggled(move |_| {
                if let Some(state) = weak.upgrade() {
                    state.refresh();
                }
            });
        }
        for (all, none, range) in groups {
            for (button, select) in [(all, true), (none, false)] {
                let weak = Rc::downgrade(&state);
                let range = range.clone();
                button.connect_clicked(move |_| {
                    let Some(state) = weak.upgrade() else { return };
                    state.updating.set(true);
                    for (_, check) in &state.checks[range.clone()] {
                        check.set_active(select);
                    }
                    state.updating.set(false);
                    state.refresh();
                });
            }
        }
        let weak = Rc::downgrade(&state);
        state.suggest.connect_clicked(move |_| {
            if let Some(state) = weak.upgrade() {
                state.suggestion();
            }
        });
        state.refresh();
        let answer = Rc::new(RefCell::new(Some(self.answer)));
        let target = window.downgrade();
        let decision = answer.clone();
        let selected = Rc::downgrade(&state);
        state.start.connect_clicked(move |_| {
            let Some(state) = selected.upgrade() else {
                return;
            };
            if state.pending.borrow().is_some() {
                return;
            }
            let selection = state.selected();
            let Ok(choice) = state
                .snapshot
                .compose(state.choice.as_ref(), &selection)
                .into_choice()
            else {
                return;
            };
            let review = InstructionReview {
                choice: Some(choice),
                reviewed: true,
                model: state.used_model.borrow().clone(),
            };
            if let Some(answer) = decision.take() {
                let _ = answer.send(Decision { selection, review });
            }
            if let Some(window) = target.upgrade() {
                window.close();
            }
        });
        let target = window.downgrade();
        cancel_button.connect_clicked(move |_| {
            if let Some(window) = target.upgrade() {
                window.close();
            }
        });
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(|keys, key, _, _| {
            if key == gtk::gdk::Key::Escape
                && let Some(window) = keys.widget().and_downcast::<gtk::Window>()
            {
                window.close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        window.add_controller(keys);
        let abandon = move || {
            if let Some(cancel) = state.pending.take() {
                cancel.store(true, Ordering::Release);
            }
            answer.take();
        };
        let closing = abandon.clone();
        window.connect_close_request(move |_| {
            closing();
            glib::Propagation::Proceed
        });
        window.connect_destroy(move |_| abandon());
        window.present();
        window
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find<T: IsA<gtk::Widget>>(root: &gtk::Widget, predicate: &impl Fn(&T) -> bool) -> Option<T> {
        if let Ok(widget) = root.clone().downcast::<T>()
            && predicate(&widget)
        {
            return Some(widget);
        }
        let mut child = root.first_child();
        while let Some(widget) = child {
            if let Some(found) = find(&widget, predicate) {
                return Some(found);
            }
            child = widget.next_sibling();
        }
        None
    }

    #[test]
    #[ignore = "requires a dedicated Xvfb display"]
    fn selector_preview_start_cancel_and_role_limit() {
        gtk::init().unwrap();
        let project = std::env::temp_dir().join(format!("selector-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("CLAUDE.md"), "# Review\nSelected text.\n").unwrap();
        let snapshot = Arc::new(Snapshot::read(&project).unwrap());
        let parent = gtk::Window::new();
        let show = |role: String| {
            let (answer, receive) = mpsc::sync_channel(1);
            let window = Review {
                snapshot: snapshot.clone(),
                choice: Some(role_instructions::Choice::Role { role }),
                remembered: snapshot.remembered(&project, "preset").unwrap(),
                suggestion: Err("Codex".to_owned()),
                pinned_model: None,
                answer,
            }
            .show(&parent);
            (window, receive)
        };
        let button = |window: &gtk::Window, text: &str| {
            find(window.upcast_ref(), &|button: &gtk::Button| {
                button.label().as_deref() == Some(text)
            })
            .unwrap()
        };
        let (window, answer) = show("Preset".into());
        let check: gtk::CheckButton =
            find(window.upcast_ref(), &|_: &gtk::CheckButton| true).unwrap();
        assert!(!check.is_active());
        assert!(!button(&window, &tr(t!("Suggest"))).is_sensitive());
        button(&window, &tr(t!("Select all"))).emit_clicked();
        assert!(check.is_active());
        let preview: gtk::TextView = find(window.upcast_ref(), &|_: &gtk::TextView| true).unwrap();
        let buffer = preview.buffer();
        let displayed = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
        assert_eq!(displayed, "Preset\n\n# Review\nSelected text.\n");
        button(&window, &tr(t!("Start"))).emit_clicked();
        let decision = answer.try_recv().unwrap();
        assert_eq!(decision.selection.len(), 1);
        assert_eq!(
            decision.review.choice,
            Some(role_instructions::Choice::Role {
                role: displayed.to_string()
            })
        );

        let (window, answer) = show("x".repeat(64 * 1024));
        button(&window, &tr(t!("Select all"))).emit_clicked();
        assert!(!button(&window, &tr(t!("Start"))).is_sensitive());
        button(&window, &tr(t!("Select none"))).emit_clicked();
        assert!(button(&window, &tr(t!("Start"))).is_sensitive());
        button(&window, &tr(t!("Cancel"))).emit_clicked();
        assert!(matches!(
            answer.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        std::fs::remove_dir_all(project).unwrap();
    }
}
