use std::{cell::RefCell, path::PathBuf, rc::Rc};

use gtk::{gio, glib, prelude::*};
use wyss_console::{
    distributors::{self, Distributor, List, Places},
    native::catalogue::{AGENTS, discover, cached_provider_version},
};

use super::model::{self, Draft, KNOWN, SUGGESTED};
use crate::{
    i18n::{tr, trf},
    personalization::{button, clear, horizontal, label, scroll, single_line, timing, vertical},
    t,
};

type Run = Rc<dyn Fn(&str, &str, &str)>;
type EditLaunch = Rc<dyn Fn(&str)>;

#[derive(Clone)]
struct Screen {
    cards: glib::WeakRef<gtk::Box>,
    status: glib::WeakRef<gtk::Label>,
    window: glib::WeakRef<gtk::ApplicationWindow>,
    places: Rc<Places>,
    run: Run,
    edit_launch: EditLaunch,
}

fn update_tool() -> &'static str {
    match crate::i18n::lang() {
        crate::i18n::Lang::En => t!("Update tool"),
        crate::i18n::Lang::Es => t!("Update distributor tool"),
    }
}

type KnownTools = Rc<[KnownTool; 3]>;

struct KnownTool {
    installed: bool,
    version: RefCell<Option<Option<String>>>,
    status: glib::WeakRef<gtk::Label>,
}

impl KnownTool {
    fn caption(&self) -> String {
        if !self.installed {
            return tr("Tool not installed");
        }
        match self.version.borrow().as_ref() {
            None => "…".into(),
            Some(None) => tr("Installed"),
            Some(Some(raw)) => model::version(raw).to_owned(),
        }
    }

    fn probe(known: &KnownTools, index: usize, path: PathBuf) {
        let known = known.clone();
        glib::spawn_future_local(async move {
            let version =
                gio::spawn_blocking(move || cached_provider_version(AGENTS[index].id, &path))
                    .await
                    .unwrap_or_default();
            let tool = &known[index];
            tool.version.replace(Some(version));
            if let Some(status) = tool.status.upgrade() {
                status.set_label(&tool.caption());
            }
        });
    }
}

impl Screen {
    fn notice(&self, message: &str) {
        if let Some(status) = self.status.upgrade() {
            status.set_label(message);
            status.set_visible(!message.is_empty());
        }
    }

    fn refresh(&self, known: KnownTools, result: Result<List, String>) {
        let Some(cards) = self.cards.upgrade() else {
            return;
        };
        let start = std::time::Instant::now();
        clear(&cards);
        let home = self.places.home.to_string_lossy();
        for (i, (maker, update)) in KNOWN.into_iter().enumerate() {
            let spec = &AGENTS[i];
            let tool = &known[i];
            let installed = tool.installed;
            let (actions, status) = card(
                &cards,
                &format!("{maker} · {} · ", spec.name),
                &tool.caption(),
                installed,
            );
            tool.status.set(Some(&status));
            self.command(
                &actions,
                t!("Open"),
                spec.name,
                &home,
                Some(spec.executable),
                installed,
            );
            self.command(&actions, update_tool(), spec.name, &home, update, installed);
            for title in [t!("Edit"), t!("Delete")] {
                let locked = button(title, "action");
                locked.set_sensitive(false);
                locked.set_tooltip_text(Some(&tr(
                    "Comes with the Console: it cannot be edited or deleted.",
                )));
                actions.append(&locked);
            }
        }
        match result {
            Ok(list) => {
                for tool in list.distributors {
                    self.added(&cards, known.clone(), Rc::new(tool), &list.home);
                }
                for refusal in list.refused {
                    cards.append(&label(
                        &format!("{}.toml: {}", refusal.id, tr(&refusal.error)),
                        "notice",
                    ));
                }
            }
            Err(_) => self.notice(&tr("The added distributors did not load.")),
        }
        let add = button(t!("Add a tool"), "action");
        add.set_halign(gtk::Align::End);
        add.add_css_class("add-tool");
        let screen = self.clone();
        add.connect_clicked(move |_| screen.editor(known.clone(), Draft::default()));
        cards.append(&add);
        timing("dist:render", start);
    }

    fn command(
        &self,
        parent: &gtk::Box,
        title: &str,
        name: &str,
        home: &str,
        command: Option<&str>,
        available: bool,
    ) {
        let control = button(title, "action");
        control.set_sensitive(available && command.is_some() && !home.is_empty());
        if let Some(command) = command {
            control.set_tooltip_text(Some(&trf(
                "Types “{command}” in a new terminal of the Console.",
                &[("command", command)],
            )));
            let (name, home, command) = (name.to_owned(), home.to_owned(), command.to_owned());
            let run = self.run.clone();
            control.connect_clicked(move |_| run(&name, &home, &command));
        } else {
            control.set_tooltip_text(Some(&tr(
                "The Console has no verified update command for this tool.",
            )));
        }
        parent.append(&control);
    }

    fn added(&self, parent: &gtk::Box, known: KnownTools, tool: Rc<Distributor>, home: &str) {
        let maker = tool
            .maker
            .as_ref()
            .map(|maker| format!("{maker} · "))
            .unwrap_or_default();
        let installed = tool.found.is_some();
        let status = if installed {
            tr("Installed")
        } else {
            tr("Tool not installed")
        };
        let (actions, _) = card(
            parent,
            &format!("{maker}{} · ", tool.name),
            &status,
            installed,
        );
        self.command(
            &actions,
            t!("Open"),
            &tool.name,
            home,
            Some(&tool.open),
            true,
        );
        let edit = |title, class| {
            let control = button(title, class);
            let (screen, known, tool) = (self.clone(), known.clone(), tool.clone());
            control
                .connect_clicked(move |_| screen.editor(known.clone(), Draft::from(tool.as_ref())));
            actions.append(&control);
            control
        };
        if tool.update.is_some() {
            self.command(
                &actions,
                update_tool(),
                &tool.name,
                home,
                tool.update.as_deref(),
                true,
            );
        } else {
            edit(update_tool(), "action").set_sensitive(!home.is_empty());
        }
        edit(t!("Edit"), "action");

        let stack = gtk::Stack::new();
        let delete = button(t!("Delete"), "action");
        let confirmation = horizontal(6);
        let confirm = button(t!("Confirm delete"), "danger");
        let cancel = button(t!("Cancel"), "action");
        confirmation.append(&confirm);
        confirmation.append(&cancel);
        stack.add_named(&delete, Some("delete"));
        stack.add_named(&confirmation, Some("confirm"));
        for (button, next) in [(delete, "confirm"), (cancel, "delete")] {
            let stack = stack.downgrade();
            button.connect_clicked(move |_| {
                if let Some(stack) = stack.upgrade() {
                    stack.set_visible_child_name(next);
                }
            });
        }
        let screen = self.clone();
        confirm.connect_clicked(move |confirm| {
            confirm.set_sensitive(false);
            let (screen, known, id, confirm) = (
                screen.clone(),
                known.clone(),
                tool.id.clone(),
                confirm.downgrade(),
            );
            let places = screen.places.as_ref().clone();
            glib::spawn_future_local(async move {
                let result = gio::spawn_blocking(move || {
                    distributors::delete(&places, &id).map(|()| distributors::list(&places))
                })
                .await;
                match result {
                    Ok(Ok(list)) => {
                        screen.notice("");
                        screen.refresh(known, list);
                    }
                    Ok(Err(error)) => screen.notice(&tr(&error)),
                    Err(_) => screen.notice(&tr("It could not be deleted.")),
                }
                if let Some(confirm) = confirm.upgrade() {
                    confirm.set_sensitive(true);
                }
            });
        });
        actions.append(&stack);
    }

    fn editor(&self, known: KnownTools, draft: Draft) {
        let started = std::time::Instant::now();
        let Some(parent) = self.window.upgrade() else {
            return;
        };
        let title = if draft.id.is_some() {
            tr("Edit distributor")
        } else {
            tr("Add distributor")
        };
        let window = gtk::Window::builder()
            .title(title)
            .transient_for(&parent)
            .modal(true)
            .default_width(490)
            .build();
        let form = vertical(10);
        form.add_css_class("distributors");
        window.set_child(Some(&form));
        let entries = draft
            .fields
            .map(|value| gtk::Entry::builder().text(value).hexpand(true).build());
        if draft.id.is_none() {
            let suggestions = horizontal(6);
            suggestions.append(&label(&tr("Suggestions:"), "hint"));
            for (name, maker, command) in SUGGESTED {
                let suggest = gtk::Button::with_label(name);
                let [name_field, maker_field, command_field] =
                    std::array::from_fn(|i| entries[i].clone());
                suggest.connect_clicked(move |_| {
                    name_field.set_text(name);
                    maker_field.set_text(maker);
                    command_field.set_text(command);
                });
                suggestions.append(&suggest);
            }
            form.append(&suggestions);
        }
        for (entry, (key, limit)) in entries.iter().zip([
            (t!("Name"), distributors::MAX_TEXT),
            (t!("Made by"), distributors::MAX_TEXT),
            (t!("Command to open"), 0),
            (t!("Command to update"), 0),
            (t!("Note"), distributors::MAX_NOTE),
        ]) {
            entry.set_max_length(limit as i32);
            let caption = label(&tr(key), "hint");
            caption.set_mnemonic_widget(Some(entry));
            entry.update_property(&[gtk::accessible::Property::Label(&tr(key))]);
            form.append(&caption);
            form.append(entry);
        }
        entries[2].set_placeholder_text(Some("kimi"));
        entries[3].set_placeholder_text(Some(&tr("optional")));
        let issue = label("", "notice");
        let error = label("", "notice");
        error.set_visible(false);
        form.append(&issue);
        form.append(&error);
        let actions = horizontal(6);
        let save = button(t!("Save"), "primary");
        let id = draft.id;
        let read = {
            let entries = entries.each_ref().map(ObjectExt::downgrade);
            let id = id.clone();
            move || Draft {
                id: id.clone(),
                fields: entries.each_ref().map(|entry| {
                    entry
                        .upgrade()
                        .map(|entry| entry.text().to_string())
                        .unwrap_or_default()
                }),
            }
        };
        let validate = {
            let (issue, save, read) = (issue.downgrade(), save.downgrade(), read.clone());
            move || {
                let problem = read().problem();
                if let Some(issue) = issue.upgrade() {
                    issue.set_label(&problem.map(tr).unwrap_or_default());
                    issue.set_visible(problem.is_some());
                }
                if let Some(save) = save.upgrade() {
                    save.set_sensitive(problem.is_none());
                }
            }
        };
        let busy = Rc::new(std::cell::Cell::new(false));
        for entry in &entries {
            let (validate, save) = (validate.clone(), save.downgrade());
            entry.connect_changed(move |_| validate());
            entry.connect_activate(move |_| {
                if let Some(save) = save.upgrade().filter(|save| save.is_sensitive()) {
                    save.emit_clicked();
                }
            });
        }
        validate();
        if let Some(id) = &id {
            let make = button(t!("Make launch"), "action");
            let (screen, window, id, error, busy) = (
                self.clone(),
                window.downgrade(),
                id.clone(),
                error.downgrade(),
                busy.clone(),
            );
            make.connect_clicked(move |_| {
                if busy.replace(true) {
                    return;
                }
                let (screen, window, id, error, busy) = (
                    screen.clone(),
                    window.clone(),
                    id.clone(),
                    error.clone(),
                    busy.clone(),
                );
                let places = screen.places.as_ref().clone();
                glib::spawn_future_local(async move {
                    let result =
                        gio::spawn_blocking(move || distributors::make_launch(&places, &id)).await;
                    busy.set(false);
                    match result {
                        Ok(Ok(id)) => {
                            if let Some(window) = window.upgrade() {
                                window.close();
                            }
                            (screen.edit_launch)(&id);
                        }
                        Ok(Err(why)) => show_error(&error, &tr(&why)),
                        Err(_) => show_error(&error, &tr("The launch could not be made.")),
                    }
                });
            });
            actions.append(&make);
        }
        let spacer = horizontal(0);
        spacer.set_hexpand(true);
        actions.append(&spacer);
        let cancel = button(t!("Cancel"), "action");
        let weak = window.downgrade();
        cancel.connect_clicked(move |_| {
            if let Some(window) = weak.upgrade() {
                window.close();
            }
        });
        actions.append(&cancel);
        actions.append(&save);
        form.append(&actions);
        let (screen, window_weak, form_weak) = (self.clone(), window.downgrade(), form.downgrade());
        save.connect_clicked(move |_| {
            let draft = read();
            if draft.problem().is_some() || busy.replace(true) {
                return;
            }
            let request = draft.request();
            let places = screen.places.as_ref().clone();
            let (screen, known, window, form, error, busy) = (
                screen.clone(),
                known.clone(),
                window_weak.clone(),
                form_weak.clone(),
                error.downgrade(),
                busy.clone(),
            );
            if let Some(form) = form.upgrade() {
                form.set_sensitive(false);
            }
            glib::spawn_future_local(async move {
                let result = gio::spawn_blocking(move || {
                    let saved = distributors::save(&places, &request)?;
                    let warning = draft
                        .id
                        .is_none()
                        .then(|| distributors::make_launch(&places, &saved.id))
                        .and_then(Result::err)
                        .filter(|why| why != "a launch with that name exists");
                    Ok::<_, String>((distributors::list(&places), warning))
                })
                .await;
                busy.set(false);
                if let Some(form) = form.upgrade() {
                    form.set_sensitive(true);
                }
                match result {
                    Ok(Ok((list, warning))) => {
                        if let Some(window) = window.upgrade() {
                            window.close();
                        }
                        let message = warning
                            .map(|why| {
                                trf(
                                    "It was added, but its launch could not be made: {why}",
                                    &[("why", &tr(&why))],
                                )
                            })
                            .unwrap_or_default();
                        screen.notice(&message);
                        screen.refresh(known, list);
                    }
                    Ok(Err(why)) => show_error(&error, &tr(&why)),
                    Err(_) => show_error(&error, &tr("It could not be saved.")),
                }
            });
        });
        window.present();
        entries[0].grab_focus();
        timing("dist:edit", started);
    }
}

fn show_error(label: &glib::WeakRef<gtk::Label>, text: &str) {
    if let Some(label) = label.upgrade() {
        label.set_label(text);
        label.set_visible(true);
    }
}

fn card(parent: &gtk::Box, title: &str, status: &str, installed: bool) -> (gtk::Box, gtk::Label) {
    let row = horizontal(12);
    row.add_css_class("distributor-row");
    let identity = horizontal(0);
    identity.set_hexpand(true);
    identity.append(&single_line(title, "title"));
    let status = label(status, if installed { "installed" } else { "hint" });
    status.set_wrap(false);
    identity.append(&status);
    row.append(&identity);
    let actions = horizontal(6);
    row.append(&actions);
    parent.append(&row);
    (actions, status)
}

pub fn build(
    window: &gtk::ApplicationWindow,
    run: Run,
    edit_launch: EditLaunch,
) -> gtk::ScrolledWindow {
    let root = vertical(12);
    root.add_css_class("distributors");
    let status = label("", "notice");
    status.set_visible(false);
    root.append(&status);
    let cards = vertical(0);
    cards.add_css_class("distributor-list");
    root.append(&cards);
    if let Some(places) = Places::from_env() {
        let screen = Screen {
            cards: cards.downgrade(),
            status: status.downgrade(),
            window: window.downgrade(),
            places: Rc::new(places),
            run,
            edit_launch,
        };
        screen.notice(&tr("Reading…"));
        let places = screen.places.as_ref().clone();
        glib::spawn_future_local(async move {
            match gio::spawn_blocking(move || {
                let started = std::time::Instant::now();
                let paths: [Option<PathBuf>; 3] = std::array::from_fn(|i| {
                    discover(&AGENTS[i], places.path_var.clone(), &places.home)
                });
                let result = (paths, distributors::list(&places));
                timing("dist:load", started);
                result
            })
            .await
            {
                Ok((paths, list)) => {
                    let known = Rc::new(std::array::from_fn(|i| KnownTool {
                        installed: paths[i].is_some(),
                        version: RefCell::new(None),
                        status: glib::WeakRef::new(),
                    }));
                    screen.notice("");
                    screen.refresh(known.clone(), list);
                    for (i, path) in paths.into_iter().enumerate() {
                        if let Some(path) = path {
                            KnownTool::probe(&known, i, path);
                        }
                    }
                }
                Err(_) => screen.notice(&tr("The distributors could not be reached.")),
            }
        });
    } else {
        status.set_label(&tr("The distributors could not be reached."));
        status.set_visible(true);
    }
    scroll(&root)
}
