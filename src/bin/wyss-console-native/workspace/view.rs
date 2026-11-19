use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    sync::{Arc, Mutex},
    time::Instant,
};

use gtk::{gio, prelude::*};
use wyss_console::{
    arsenal::{
        self, environment,
        library::InstalledScanCache,
        roles,
        store::{Store, Summary},
    },
    distributors,
    launch::{self, Kind, Launch, Places, Variant, VariantsRequest},
    native::{
        instruction_text,
        launch_home::{HookOption, Owner},
        launch_home_codex::CodexOwner,
    },
};

use super::{
    distros::{self, DISTROS},
    model::{self, CLAUDE, CODEX, CommandSection, Draft},
};
use crate::{
    i18n::{tr, trf},
    personalization::{clear, horizontal, scroll, vertical},
    t,
};

type Action = Rc<dyn Fn(&str)>;
type Rows = BTreeMap<String, Row>;

struct Row {
    button: glib::WeakRef<gtk::Button>,
    badge: glib::WeakRef<gtk::Label>,
}

fn label(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.);
    label.set_wrap(true);
    label.add_css_class(class);
    label
}

fn button(text: &str, class: &str) -> gtk::Button {
    let button = gtk::Button::with_label(text);
    button.add_css_class(class);
    button
}

fn entry(text: &str) -> gtk::Entry {
    let entry = gtk::Entry::new();
    entry.set_text(text);
    entry.set_hexpand(true);
    entry
}

fn row(parent: &gtk::Box, title: &str, hint: &str, control: &impl IsA<gtk::Widget>) {
    let row = horizontal(24);
    row.add_css_class("row");
    let copy = vertical(3);
    copy.add_css_class("row-copy");
    for (text, class) in [(title, "lbl"), (hint, "hint")] {
        if !text.is_empty() {
            let caption = label(text, class);
            caption.set_max_width_chars(28);
            caption.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            copy.append(&caption);
        }
    }
    control.set_hexpand(true);
    control.set_valign(gtk::Align::Start);
    row.append(&copy);
    row.append(control);
    parent.append(&row);
}

fn section(parent: &gtk::Box, text: &str) {
    parent.append(&label(text, "sksec"));
}

fn switch(label: &str, on: bool, change: impl Fn(bool) + 'static) -> gtk::Switch {
    let control = gtk::Switch::builder()
        .active(on)
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .build();
    control.add_css_class("sw");
    control.update_property(&[gtk::accessible::Property::Label(label)]);
    control.connect_active_notify(move |control| {
        change(control.is_active());
        refresh_save(control);
        let _ = control.activate_action("workspace-metrics.refresh", None);
    });
    control
}

fn toggle_row(
    parent: &gtk::Box,
    name: &str,
    hint: &str,
    class: &str,
    control: Option<&gtk::Switch>,
) {
    let line = horizontal(12);
    line.add_css_class("tree-row");
    let title = label(name, class);
    title.set_hexpand(true);
    title.set_wrap(false);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_tooltip_text(Some(name));
    line.append(&title);
    if !hint.is_empty() {
        let meta = label(hint, "hint");
        meta.set_wrap(false);
        meta.set_max_width_chars(55);
        meta.set_ellipsize(gtk::pango::EllipsizeMode::End);
        meta.set_tooltip_text(Some(hint));
        line.append(&meta);
    }
    if let Some(control) = control {
        line.append(control);
    } else {
        let spacer = horizontal(0);
        spacer.set_width_request(34);
        line.append(&spacer);
    }
    parent.append(&line);
}

#[derive(Clone)]
struct Select {
    widget: gtk::DropDown,
    ids: Rc<Vec<String>>,
}

impl Select {
    fn selected_id(&self) -> Option<&str> {
        self.ids
            .get(self.widget.selected() as usize)
            .map(String::as_str)
    }

    fn connect_changed(&self, change: impl Fn(&str) + 'static) {
        let ids = self.ids.clone();
        self.widget.connect_selected_notify(move |drop_down| {
            if let Some(id) = ids.get(drop_down.selected() as usize) {
                change(id);
            }
        });
    }
}

fn select(items: &[(&str, &str)], selected: &str) -> Select {
    let labels = items.iter().map(|(_, name)| *name).collect::<Vec<_>>();
    let widget = gtk::DropDown::from_strings(&labels);
    if let Some(index) = items.iter().position(|(id, _)| *id == selected) {
        widget.set_selected(index as u32);
    }
    Select {
        widget,
        ids: Rc::new(items.iter().map(|(id, _)| (*id).into()).collect()),
    }
}

fn text_view(text: &str) -> gtk::TextView {
    let view = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    view.buffer().set_text(text);
    view
}

fn buffer_text(buffer: &gtk::TextBuffer) -> String {
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), false)
        .into()
}

fn foot(
    parent: &gtk::Box,
    delete: Option<impl Fn() + 'static>,
    test: Option<impl Fn() + 'static>,
    save: impl Fn() + 'static,
) -> (gtk::Button, Option<gtk::Button>) {
    let footer = horizontal(10);
    footer.add_css_class("foot");
    if let Some(delete) = delete {
        let remove = button(&tr("Delete launch"), "danger");
        remove.connect_clicked(move |_| delete());
        footer.append(&remove);
    }
    let spacer = horizontal(0);
    spacer.set_hexpand(true);
    footer.append(&spacer);
    let try_button = test.map(|test| {
        let try_button = button(&tr("Try"), "secondary");
        try_button.connect_clicked(move |_| test());
        footer.append(&try_button);
        try_button
    });
    let save_button = button(&tr("Save"), "primary");
    save_button.connect_clicked(move |_| save());
    footer.append(&save_button);
    parent.append(&footer);
    (save_button, try_button)
}

fn confirm(window: &gtk::ApplicationWindow, action: impl Fn() + 'static) {
    let dialog = gtk::AlertDialog::builder()
        .message(tr("Delete this launch?"))
        .build();
    dialog.set_modal(true);
    dialog.set_buttons(&[&tr("Cancel"), &tr("Delete")]);
    dialog.set_cancel_button(0);
    dialog.set_default_button(0);
    dialog.choose(Some(window), gio::Cancellable::NONE, move |result| {
        if result.ok() == Some(1) {
            action();
        }
    });
}

struct Timing {
    stage: &'static str,
    start: Option<Instant>,
}

impl Timing {
    fn new(stage: &'static str) -> Self {
        Self {
            stage,
            start: (std::env::var("WYSS_WS_TIMING").as_deref() == Ok("1")).then(Instant::now),
        }
    }
}

impl Drop for Timing {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            eprintln!(
                "ws-timing {} {:.3}",
                self.stage,
                start.elapsed().as_secs_f64() * 1000.
            );
        }
    }
}

#[cfg(test)]
thread_local! {
    static SKILL_LOADS: Cell<usize> = const { Cell::new(0) };
}

struct Catalog {
    store: Option<Mutex<Store>>,
    library: Vec<Summary>,
    inventory: model::Inventory,
    distributors: Option<distributors::List>,
}

impl Catalog {
    fn load(places: &Places) -> Self {
        let (store, library) = {
            let _timing = Timing::new("editor:disk:arsenal");
            let store = arsenal::open_read_only().ok();
            let library = store
                .as_ref()
                .and_then(|store| store.list().ok())
                .unwrap_or_default();
            (store, library)
        };
        let inventory = {
            let _timing = Timing::new("editor:disk:inventory");
            model::inventory(&places.home)
        };
        let distributors = {
            let _timing = Timing::new("editor:disk:distributors");
            distributors::Places::from_env().and_then(|places| distributors::list(&places).ok())
        };
        Self {
            store: store.map(Mutex::new),
            library,
            inventory,
            distributors,
        }
    }

    fn skills(&self, draft: &Draft, scans: &InstalledScanCache) -> Arc<model::Skills> {
        #[cfg(test)]
        SKILL_LOADS.set(SKILL_LOADS.get() + 1);
        let _timing = Timing::new("editor:disk:skill-availability");
        Arc::new(model::Skills::load(
            draft,
            self.store.as_ref(),
            &self.library,
            scans,
        ))
    }

    fn prepare(&self, places: &Places, launch: Option<&Launch>) -> Result<Draft, String> {
        let _timing = Timing::new("editor:draft-load");
        let store = self.store.as_ref().map(|store| store.lock().unwrap());
        Draft::load(places, launch, CLAUDE, store.as_deref())
    }
}

struct Listing {
    entries: launch::List,
    deletable_commands: BTreeSet<String>,
    missing_directories: BTreeSet<String>,
}

struct Details {
    catalog: Arc<Catalog>,
    badges: BTreeMap<String, String>,
    agents: BTreeMap<String, Result<Draft, String>>,
}

impl Details {
    fn load(places: &Places, entries: &[Launch]) -> Self {
        let catalog = Arc::new(Catalog::load(places));
        let mut badges = BTreeMap::new();
        let mut agents = BTreeMap::new();
        for entry in entries {
            if let Kind::Session { agent, .. } = &entry.kind {
                let prepared = catalog.prepare(places, Some(entry));
                if agent == CLAUDE
                    && let Ok(draft) = &prepared
                {
                    let tokens = model::estimate(draft, &catalog.inventory.0, &catalog.library);
                    badges.insert(entry.id.clone(), format!("~{}", model::k_tokens(tokens)));
                }
                agents.insert(entry.id.clone(), prepared);
            }
        }
        Self {
            catalog,
            badges,
            agents,
        }
    }
}

struct Cache {
    listing: Result<Listing, String>,
    details: Option<Details>,
}

async fn load_cache(
    places: Arc<Places>,
    io: Arc<Mutex<()>>,
    load: impl FnOnce(&Places) -> Cache + Send + 'static,
) -> std::thread::Result<Cache> {
    gio::spawn_blocking(move || {
        let _guard = io.lock().unwrap();
        load(&places)
    })
    .await
}

impl Cache {
    fn load(places: &Places) -> Self {
        let _timing = Timing::new("editor:disk:launches");
        let listing = launch::list_for_editor(places).map(|entries| {
            let mut deletable_commands = BTreeSet::new();
            let mut missing_directories = BTreeSet::new();
            for entry in &entries.launches {
                if matches!(entry.kind, Kind::Commands { .. })
                    && launch::can_delete_commands(places, &entry.id)
                {
                    deletable_commands.insert(entry.id.clone());
                }
                if !std::path::Path::new(&entry.directory).is_dir() {
                    missing_directories.insert(entry.id.clone());
                }
            }
            Listing {
                entries,
                deletable_commands,
                missing_directories,
            }
        });
        Self {
            listing,
            details: None,
        }
    }
}

#[derive(Clone)]
struct Screen {
    window: glib::WeakRef<gtk::ApplicationWindow>,
    editor: glib::WeakRef<gtk::Box>,
    list: glib::WeakRef<gtk::Box>,
    search: glib::WeakRef<gtk::SearchEntry>,
    new: glib::WeakRef<gtk::MenuButton>,
    status: glib::WeakRef<gtk::Label>,
    selected: Rc<RefCell<Option<String>>>,
    rows: Rc<RefCell<Rows>>,
    cache: Rc<RefCell<Option<Cache>>>,
    reload_id: Rc<Cell<u64>>,
    edit_id: Rc<Cell<u64>>,
    preview: Rc<Cell<bool>>,
    scans: Arc<InstalledScanCache>,
    pending: Rc<RefCell<Option<Pending>>>,
    saving: Rc<WriteState>,
    io: Arc<Mutex<()>>,
    places: Arc<Places>,
    recent: Option<crate::host::Host>,
    try_launch: Option<Action>,
}

use gtk::glib;

#[derive(Clone, Copy, Default)]
enum WritePhase {
    #[default]
    Idle,
    Writing,
    ClosingAfterWrite,
}

#[derive(Default)]
struct WriteState(Cell<WritePhase>);

impl WriteState {
    fn start(&self) -> bool {
        if self.saving() {
            return false;
        }
        self.0.set(WritePhase::Writing);
        true
    }

    fn saving(&self) -> bool {
        !matches!(self.0.get(), WritePhase::Idle)
    }

    fn request_close(&self) -> glib::Propagation {
        if self.saving() {
            self.0.set(WritePhase::ClosingAfterWrite);
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    }

    fn finish(&self) -> bool {
        matches!(
            self.0.replace(WritePhase::Idle),
            WritePhase::ClosingAfterWrite
        )
    }
}

enum Pending {
    Initial,
    Open(String),
    Preset(String),
    Saved(Launch),
}

impl Pending {
    fn queue_reload(current: &mut Option<Self>, next: Option<Self>) {
        if matches!(current, Some(Self::Open(_))) && !matches!(&next, Some(Self::Open(_))) {
            return;
        }
        *current = next;
    }

    fn editor_enabled(pending: &Option<Self>, saving: bool) -> bool {
        !saving && !matches!(pending, Some(Self::Saved(_)))
    }
}

enum Written {
    Saved(Launch),
    Deleted { warning: Option<String> },
}

impl Screen {
    fn message(&self, message: &str) {
        if let Some(status) = self.status.upgrade() {
            status.set_text(message);
            status.set_visible(!message.is_empty());
        }
    }

    fn editor_enabled(&self) -> bool {
        !self.preview.get() && Pending::editor_enabled(&self.pending.borrow(), self.saving.saving())
    }

    fn sync_editor_sensitivity(&self) {
        if let Some(editor) = self.editor.upgrade() {
            editor.set_sensitive(self.editor_enabled());
        }
    }

    fn refresh(&self) {
        let _timing = Timing::new("list-refresh");
        let Some(list) = self.list.upgrade() else {
            return;
        };
        clear(&list);
        let mut rows = self.rows.borrow_mut();
        rows.clear();
        let cache = self.cache.borrow();
        let Some(cache) = cache.as_ref() else { return };
        let listing = match &cache.listing {
            Ok(listing) => listing,
            Err(error) => {
                self.message(error);
                return;
            }
        };
        let query = self
            .search
            .upgrade()
            .map(|search| search.text())
            .unwrap_or_default();
        let (system, agents) = model::groups(&listing.entries.launches, &query);
        for (title, entries) in [(t!("System"), system), (t!("Agents"), agents)] {
            section(&list, &tr(title));
            for entry in entries {
                let selected = self.selected.borrow().as_deref() == Some(&entry.id);
                let item = button(&entry.name, "it");
                let contents = vertical(2);
                contents.set_valign(gtk::Align::Center);
                let line = horizontal(6);
                let name = label(&entry.name, "item-name");
                name.set_wrap(false);
                name.set_ellipsize(gtk::pango::EllipsizeMode::End);
                name.set_hexpand(true);
                line.append(&name);
                let badge = label("", "hint");
                let estimate = cache
                    .details
                    .as_ref()
                    .and_then(|details| details.badges.get(&entry.id));
                if let Some(estimate) = estimate {
                    badge.set_text(estimate);
                }
                badge.set_visible(estimate.is_some());
                line.append(&badge);
                contents.append(&line);
                if listing.missing_directories.contains(&entry.id) {
                    contents.append(&label(&tr("directory not found"), "warning"));
                }
                item.set_child(Some(&contents));
                if selected {
                    item.add_css_class("on");
                }
                let screen = self.clone();
                let id = entry.id.clone();
                item.connect_clicked(move |_| {
                    let _timing = Timing::new("click");
                    screen.open(&id);
                });
                rows.insert(
                    entry.id.clone(),
                    Row {
                        button: item.downgrade(),
                        badge: badge.downgrade(),
                    },
                );
                list.append(&item);
            }
        }
        if !listing.entries.refused.is_empty() {
            let errors = listing
                .entries
                .refused
                .iter()
                .map(|entry| format!("{} · {}", entry.id, entry.error))
                .collect::<Vec<_>>();
            let errors = label(&errors.join("\n"), "hint");
            errors.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            errors.set_max_width_chars(24);
            list.append(&errors);
        }
    }

    fn reload(&self, pending: Option<Pending>) {
        let timing = Timing::new("list-reload");
        let paint_start = timing.start;
        let id = self.reload_id.get().wrapping_add(1);
        self.reload_id.set(id);
        Pending::queue_reload(&mut self.pending.borrow_mut(), pending);
        let (places, io, screen) = (self.places.clone(), self.io.clone(), self.clone());
        glib::spawn_future_local(async move {
            let loaded = load_cache(places, io, Cache::load).await;
            if screen.reload_id.get() != id || screen.list.upgrade().is_none() {
                return;
            }
            let pending = screen.pending.borrow_mut().take();
            let Ok(cache) = loaded else {
                screen.sync_editor_sensitivity();
                screen.message(&tr("Launch configuration is unavailable"));
                return;
            };
            let entries = cache
                .listing
                .as_ref()
                .map(|listing| listing.entries.launches.clone())
                .unwrap_or_default();
            let initial = entries.first().cloned();
            if screen.cache.borrow().is_none() && cache.listing.is_ok() {
                screen.message("");
            }
            *screen.cache.borrow_mut() = Some(cache);
            if let Some(new) = screen.new.upgrade() {
                new.set_sensitive(false);
            }
            screen.refresh();
            match pending {
                Some(Pending::Initial) => {
                    if let Some(entry) = initial {
                        screen.edit(Some(entry), false);
                    }
                }
                Some(Pending::Open(id)) => screen.open(&id),
                Some(Pending::Preset(id)) => screen.create_from_preset(&id),
                Some(Pending::Saved(launch))
                    if screen
                        .cache
                        .borrow()
                        .as_ref()
                        .is_some_and(|cache| cache.listing.is_ok()) =>
                {
                    screen.edit(Some(launch), false);
                    if !screen
                        .status
                        .upgrade()
                        .is_some_and(|status| status.is_visible())
                    {
                        screen.message(&tr("Launch saved"));
                    }
                }
                _ => {}
            }
            screen.sync_editor_sensitivity();
            drop(timing);
            if let Some(list) = screen.list.upgrade() {
                // The first frame only needs launch definitions and the basic editor fields.
                let load = RefCell::new(Some(move || {
                    drop(Timing {
                        stage: "list-first-paint",
                        start: paint_start,
                    });
                    screen.load_details(id, entries);
                }));
                list.add_tick_callback(move |_, clock| {
                    let handler = Rc::new(Cell::new(None));
                    let load = RefCell::new(load.take());
                    handler.set(Some(clock.connect_after_paint({
                        let handler = handler.clone();
                        move |clock| {
                            let load = load.take();
                            if let Some(id) = handler.take() {
                                clock.disconnect(id);
                            }
                            if let Some(load) = load {
                                load();
                            }
                        }
                    })));
                    glib::ControlFlow::Break
                });
            }
        });
    }

    fn load_details(&self, id: u64, entries: Vec<Launch>) {
        if self.reload_id.get() != id {
            return;
        }
        let places = self.places.clone();
        let screen = self.clone();
        glib::spawn_future_local(async move {
            let loaded = gio::spawn_blocking(move || Details::load(&places, &entries)).await;
            if screen.reload_id.get() != id || screen.list.upgrade().is_none() {
                return;
            }
            let Ok(details) = loaded else {
                screen.message(&tr("Launch configuration is unavailable"));
                return;
            };
            let catalog = details.catalog.clone();
            let prepared = if screen.preview.get() {
                let selected = screen.selected.borrow();
                selected
                    .as_ref()
                    .and_then(|id| details.agents.get(id))
                    .cloned()
            } else {
                None
            };
            for (id, estimate) in &details.badges {
                if let Some(badge) = screen
                    .rows
                    .borrow()
                    .get(id)
                    .and_then(|row| row.badge.upgrade())
                {
                    badge.set_text(estimate);
                    badge.set_visible(true);
                }
            }
            if let Some(cache) = screen.cache.borrow_mut().as_mut() {
                cache.details = Some(details);
            }
            if let Some(new) = screen.new.upgrade() {
                new.set_sensitive(true);
            }
            if let Some(prepared) = prepared {
                screen.show_agent(screen.edit_id.get(), catalog, prepared);
            }
            if matches!(*screen.pending.borrow(), Some(Pending::Preset(_))) {
                let pending = screen.pending.borrow_mut().take();
                if let Some(Pending::Preset(id)) = pending {
                    screen.create_from_preset(&id);
                }
            }
        });
    }

    fn open(&self, id: &str) {
        let entry = {
            let cache = self.cache.borrow();
            let Some(cache) = cache.as_ref() else {
                *self.pending.borrow_mut() = Some(Pending::Open(id.to_owned()));
                return;
            };
            cache.listing.as_ref().ok().and_then(|listing| {
                listing
                    .entries
                    .launches
                    .iter()
                    .find(|entry| entry.id == id)
                    .cloned()
            })
        };
        if let Some(entry) = entry {
            self.edit(Some(entry), false);
        }
    }

    fn select(&self, id: Option<&str>) {
        let mut selected = self.selected.borrow_mut();
        if selected.as_deref() == id {
            return;
        }
        let rows = self.rows.borrow();
        let row = |id: &str| rows.get(id).and_then(|row| row.button.upgrade());
        if let Some(previous) = selected.as_deref().and_then(row) {
            previous.remove_css_class("on");
        }
        if let Some(current) = id.and_then(row) {
            current.add_css_class("on");
        }
        *selected = id.map(str::to_owned);
    }

    fn start_edit(&self) -> Option<(gtk::Box, u64)> {
        let editor = self.editor.upgrade()?;
        self.pending.borrow_mut().take();
        self.preview.set(false);
        let id = self.edit_id.get().wrapping_add(1);
        self.edit_id.set(id);
        editor.set_sensitive(self.editor_enabled());
        editor.insert_action_group("workspace-metrics", None::<&gio::SimpleActionGroup>);
        clear(&editor);
        self.message("");
        Some((editor, id))
    }

    fn edit(&self, launch: Option<Launch>, commands: bool) {
        let _timing = Timing::new("editor");
        let Some((editor, edit_id)) = self.start_edit() else {
            return;
        };
        self.select(launch.as_ref().map(|entry| entry.id.as_str()));
        if commands
            || launch
                .as_ref()
                .is_some_and(|entry| matches!(entry.kind, Kind::Commands { .. }))
        {
            command_editor(self, &editor, launch.as_ref());
            return;
        }
        let details = self.cache.borrow().as_ref().and_then(|cache| {
            let details = cache.details.as_ref()?;
            let prepared = launch
                .as_ref()
                .and_then(|launch| details.agents.get(&launch.id).cloned());
            Some((details.catalog.clone(), prepared))
        });
        let Some((catalog, prepared)) = details else {
            if let Some(launch) = &launch {
                self.preview.set(true);
                editor.set_sensitive(false);
                let draft = Draft::basic(&self.places, Some(launch), CLAUDE);
                agent_editor(self, &editor, Rc::new(RefCell::new(draft)), None, edit_id);
            }
            return;
        };
        if let Some(prepared) = prepared {
            self.show_agent(edit_id, catalog, prepared);
        } else {
            let places = self.places.clone();
            self.prepare_agent(edit_id, catalog, move |catalog| {
                catalog.prepare(&places, launch.as_ref())
            });
        }
    }

    fn show_agent(&self, edit_id: u64, catalog: Arc<Catalog>, result: Result<Draft, String>) {
        let Some(editor) = self
            .editor
            .upgrade()
            .filter(|_| self.edit_id.get() == edit_id)
        else {
            return;
        };
        let was_preview = self.preview.replace(false);
        if !was_preview {
            self.message("");
        }
        editor.set_sensitive(self.editor_enabled());
        match result {
            Ok(draft) => {
                editor.insert_action_group("workspace-metrics", None::<&gio::SimpleActionGroup>);
                clear(&editor);
                agent_editor(
                    self,
                    &editor,
                    Rc::new(RefCell::new(draft)),
                    Some(catalog),
                    edit_id,
                );
            }
            Err(error) => {
                if was_preview {
                    clear(&editor);
                }
                self.message(&tr(&error));
            }
        }
    }

    fn prepare_agent(
        &self,
        edit_id: u64,
        catalog: Arc<Catalog>,
        prepare: impl FnOnce(&Catalog) -> Result<Draft, String> + Send + 'static,
    ) {
        self.message(&tr("Reading…"));
        let screen = self.clone();
        let worker = catalog.clone();
        glib::spawn_future_local(async move {
            let prepared = gio::spawn_blocking(move || prepare(&worker))
                .await
                .unwrap_or_else(|_| Err(t!("Launch configuration is unavailable").into()));
            screen.show_agent(edit_id, catalog, prepared);
        });
    }

    fn create_from_preset(&self, id: &str) {
        let (catalog, used) = {
            let cache = self.cache.borrow();
            let Some((cache, details)) = cache
                .as_ref()
                .and_then(|cache| Some((cache, cache.details.as_ref()?)))
            else {
                *self.pending.borrow_mut() = Some(Pending::Preset(id.to_owned()));
                return;
            };
            let used = cache
                .listing
                .as_ref()
                .ok()
                .into_iter()
                .flat_map(|listing| &listing.entries.launches)
                .map(|launch| launch.id.clone())
                .collect::<BTreeSet<_>>();
            (details.catalog.clone(), used)
        };
        let Some((_, edit_id)) = self.start_edit() else {
            return;
        };
        self.select(None);
        let places = self.places.clone();
        let id = id.to_owned();
        self.prepare_agent(edit_id, catalog, move |catalog| {
            let draft = {
                let store = catalog
                    .store
                    .as_ref()
                    .ok_or("Presets could not be read.")?
                    .lock()
                    .unwrap();
                let selections = store
                    .selections()
                    .map_err(|_| "Presets could not be read.")?;
                let selection = selections
                    .iter()
                    .find(|selection| selection.id == id)
                    .ok_or("That preset is no longer available. Refresh and try again.")?;
                let mut draft = Draft::from_preset(&places, selection, &store)?;
                if used.contains(&launch::slug(&draft.name)) {
                    let stem: String = selection.name.chars().take(30).collect();
                    for next in 2..=used.len() + 1 {
                        let name = format!("{stem} {next}");
                        if !used.contains(&launch::slug(&name)) {
                            draft.name = name;
                            break;
                        }
                    }
                }
                draft
            };
            Ok(draft)
        });
    }

    fn write(
        &self,
        operation: impl FnOnce(&Places) -> Result<Written, String> + Send + 'static,
        test: Option<Action>,
    ) {
        if !self.editor_enabled() || !self.saving.start() {
            return;
        }
        let screen = self.clone();
        let places = self.places.clone();
        let edit_id = self.edit_id.get();
        let io = self.io.clone();
        self.message(&tr("Saving…"));
        self.sync_editor_sensitivity();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let _guard = io.lock().unwrap();
                operation(&places)
            })
            .await
            .unwrap_or_else(|_| Err(t!("Launch configuration is unavailable").into()));
            let close_requested = screen.saving.finish();
            let same_editor = screen.edit_id.get() == edit_id;
            match result {
                Ok(Written::Saved(launch)) => {
                    if let Some(test) = test {
                        test(&launch.id);
                    }
                    screen.reload(same_editor.then_some(Pending::Saved(launch)));
                }
                Ok(Written::Deleted { warning }) => {
                    if same_editor && !matches!(*screen.pending.borrow(), Some(Pending::Open(_))) {
                        screen.start_edit();
                        screen.select(None);
                        let deleted = tr("Launch deleted");
                        screen.message(warning.as_deref().unwrap_or(&deleted));
                    }
                    screen.reload(None);
                }
                Err(error) => {
                    if same_editor {
                        screen.message(&tr(&error));
                    }
                }
            }
            screen.sync_editor_sensitivity();
            if close_requested && let Some(window) = screen.window.upgrade() {
                window.close();
            }
        });
    }

    fn delete(&self, id: &str, commands: bool) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let screen = self.clone();
        let id = id.to_owned();
        confirm(&window, move || {
            let id = id.clone();
            screen.write(
                move |places| {
                    if commands {
                        launch::delete_commands(places, &id)?;
                    } else {
                        launch::delete(places, &id)?;
                        if let Some(directory) = environment::directory()
                            && let Err(error) =
                                roles::write(&directory, roles::Change::Forget { launch: id })
                        {
                            return Ok(Written::Deleted {
                                warning: Some(format!("{error:?}")),
                            });
                        }
                    }
                    Ok(Written::Deleted { warning: None })
                },
                None,
            );
        });
    }
}

pub fn build(
    window: &gtk::ApplicationWindow,
    try_launch: Option<Action>,
    recent: Option<crate::host::Host>,
) -> gtk::Box {
    let root = horizontal(0);
    root.add_css_class("workspace");
    let Some(places) = Places::from_env() else {
        root.append(&label(&tr("Launch configuration is unavailable"), "notice"));
        return root;
    };
    let side = vertical(0);
    side.add_css_class("list");
    side.set_hexpand(false);
    let search = gtk::SearchEntry::new();
    search.set_width_chars(1);
    search.set_max_width_chars(1);
    search.set_placeholder_text(Some(&tr("Search")));
    side.append(&search);
    let list = vertical(2);
    side.append(&scroll(&list));
    let new = gtk::MenuButton::builder().label(tr("+ New")).build();
    new.add_css_class("new");
    let popover = gtk::Popover::new();
    let choices = vertical(4);
    popover.set_child(Some(&choices));
    new.set_popover(Some(&popover));
    side.append(&new);

    let right = vertical(0);
    right.set_hexpand(true);
    let status = label("", "notice");
    status.set_visible(false);
    right.append(&status);
    let editor = vertical(0);
    editor.set_vexpand(true);
    right.append(&editor);
    root.append(&side);
    root.append(&right);
    let screen = Screen {
        window: window.downgrade(),
        editor: editor.downgrade(),
        list: list.downgrade(),
        search: search.downgrade(),
        new: new.downgrade(),
        status: status.downgrade(),
        places: Arc::new(places),
        recent,
        selected: Rc::default(),
        rows: Rc::default(),
        cache: Rc::default(),
        reload_id: Rc::default(),
        edit_id: Rc::default(),
        preview: Rc::default(),
        scans: Arc::default(),
        pending: Rc::default(),
        saving: Rc::default(),
        io: Arc::default(),
        try_launch,
    };
    let saving = screen.saving.clone();
    window.connect_close_request(move |_| saving.request_close());
    new.set_sensitive(false);
    for (name, commands) in [(t!("Agent"), false), (t!("System commands"), true)] {
        let item = button(&tr(name), "it");
        let screen = screen.clone();
        let popover = popover.downgrade();
        item.connect_clicked(move |_| {
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
            screen.edit(None, commands);
        });
        choices.append(&item);
    }
    let routes = gio::SimpleActionGroup::new();
    for (name, reload) in [("open", false), ("reload-and-open", true)] {
        let action = gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
        let screen = screen.clone();
        action.connect_activate(move |_, parameter| {
            if let Some(id) = parameter.and_then(glib::Variant::str) {
                if reload {
                    screen.reload(Some(Pending::Open(id.to_owned())));
                } else {
                    screen.open(id);
                }
            }
        });
        routes.add_action(&action);
    }
    let create = gio::SimpleAction::new("create-from-preset", Some(glib::VariantTy::STRING));
    create.connect_activate({
        let screen = screen.clone();
        move |_, parameter| {
            if let Some(id) = parameter.and_then(glib::Variant::str) {
                screen.create_from_preset(id);
            }
        }
    });
    routes.add_action(&create);
    root.insert_action_group("workspace-route", Some(&routes));
    let screen_on_search = screen.clone();
    search.connect_search_changed(move |_| screen_on_search.refresh());
    screen.message(&tr("Reading…"));
    screen.reload(Some(Pending::Initial));
    root
}

fn command_editor(screen: &Screen, editor: &gtk::Box, launch: Option<&Launch>) {
    let _timing = Timing::new("editor:system-update");
    let content = vertical(0);
    content.set_vexpand(true);
    let (variants, active, template) = match launch.map(|entry| &entry.kind) {
        Some(Kind::Commands {
            variants,
            active,
            template,
            ..
        }) => (
            variants.as_slice(),
            active.clone(),
            template.as_deref() == Some("system-update"),
        ),
        _ => (&[][..], None, false),
    };
    let variants: Rc<RefCell<BTreeMap<String, Variant>>> = Rc::new(RefCell::new(
        variants
            .iter()
            .cloned()
            .map(|variant| (variant.distro.clone(), variant))
            .collect(),
    ));
    let name = {
        let _timing = Timing::new("editor:system-update:name");
        entry(launch.map(|entry| entry.name.as_str()).unwrap_or(""))
    };
    name.set_editable(launch.is_none());
    name.set_placeholder_text(Some(&tr("Launch name")));
    name.add_css_class("head");
    content.append(&name);
    let body = vertical(0);
    body.add_css_class("body");
    let mut choices: Vec<_> = DISTROS.iter().map(|d| (d.id, d.label)).collect();
    let custom: Vec<_> = variants
        .borrow()
        .keys()
        .filter(|id| !DISTROS.iter().any(|d| d.id == *id))
        .cloned()
        .collect();
    choices.extend(custom.iter().map(|name| (name.as_str(), name.as_str())));
    let cache = screen.cache.borrow();
    let listing = cache.as_ref().and_then(|cache| cache.listing.as_ref().ok());
    let system = listing
        .map(|listing| listing.entries.system.clone())
        .unwrap_or_default();
    if !system.id.is_empty() && !choices.iter().any(|(id, _)| *id == system.id) && !template {
        choices.push((&system.id, &system.name));
    }
    let selector_timing = Timing::new("editor:system-update:distribution");
    let distro = select(
        &choices,
        active
            .as_deref()
            .or_else(|| {
                choices
                    .iter()
                    .find(|(id, _)| *id == system.id)
                    .map(|(id, _)| *id)
            })
            .unwrap_or(DISTROS[0].id),
    );
    row(
        &body,
        &tr("Distribution"),
        &tr("Commands for this PC."),
        &distro.widget,
    );
    drop(selector_timing);
    let options = gtk::Stack::builder()
        .vhomogeneous(false)
        .hhomogeneous(false)
        .build();
    body.append(&options);
    let directory = launch
        .map(|entry| entry.directory.as_str())
        .unwrap_or(screen.places.home.to_str().unwrap_or("~"));
    let update = {
        let options = options.downgrade();
        let variants = variants.clone();
        let directory = directory.to_owned();
        move |id: &str| {
            if let Some(options) = options.upgrade() {
                if options.child_by_name(id).is_none() {
                    let page = vertical(0);
                    command_options(&page, &variants, id, template, &directory);
                    options.add_named(&page, Some(id));
                }
                options.set_visible_child_name(id);
            }
        }
    };
    if let Some(id) = distro.selected_id() {
        update(id);
    }
    distro.connect_changed(update);
    content.append(&scroll(&body));
    let save = {
        let screen = screen.clone();
        let id = launch.map(|entry| entry.id.clone());
        move |test| {
            let mut saved = variants.borrow().clone();
            if template
                && let Some(id) = distro.selected_id()
                && let Some(recipe) = DISTROS.iter().find(|recipe| recipe.id == id)
            {
                saved.entry(id.into()).or_insert_with(|| recipe.fresh(true));
            }
            let variants = saved.into_values().collect();
            let id = id.clone();
            let name = name.text().to_string();
            screen.write(
                move |places| {
                    match id {
                        Some(id) => {
                            launch::save_variants(places, &VariantsRequest { id, variants })
                        }
                        None => launch::create_commands(places, &name, variants),
                    }
                    .map(Written::Saved)
                },
                test,
            );
        }
    };
    let run = screen.try_launch.as_ref().map(|action| {
        let action = action.clone();
        let save = save.clone();
        move || save(Some(action.clone()))
    });
    let delete = launch
        .filter(|entry| {
            listing.is_some_and(|listing| listing.deletable_commands.contains(&entry.id))
        })
        .map(|entry| {
            let screen = screen.clone();
            let id = entry.id.clone();
            move || screen.delete(&id, true)
        });
    foot(&content, delete, run, move || save(None));
    let _timing = Timing::new("editor:system-update:mount");
    editor.append(&content);
}

fn command_options(
    parent: &gtk::Box,
    variants: &Rc<RefCell<BTreeMap<String, Variant>>>,
    id: &str,
    template: bool,
    directory: &str,
) {
    let distro = DISTROS.iter().find(|d| d.id == id);
    let variant = variants.borrow().get(id).cloned().unwrap_or_else(|| {
        distro.map_or_else(
            || Variant {
                distro: id.into(),
                commands: vec![],
                options: vec![],
                manual: true,
            },
            |distro| distro.fresh(template),
        )
    });
    let text_timing = Timing::new("editor:system-update:command-text");
    let commands = text_view(&variant.commands.join("\n"));
    commands.set_monospace(true);
    commands.set_wrap_mode(gtk::WrapMode::None);
    let command_editor = gtk::ScrolledWindow::builder()
        .child(&commands)
        .min_content_height(135)
        .build();
    command_editor.add_css_class("command-editor");
    let controls = vertical(6);
    let manual = horizontal(8);
    manual.append(&label(
        &tr("Edited by hand: the options no longer change them."),
        "hint",
    ));
    let reset = button(&tr("Rebuild from the options"), "flat");
    manual.add_css_class("notice");
    manual.append(&reset);
    manual.set_visible(template && variant.manual && distro.is_some());
    controls.append(&command_editor);
    controls.append(&manual);
    drop(text_timing);
    let _timing = Timing::new("editor:system-update:options");

    // Only user edits mark a generated variant as manual.
    let composing = Rc::new(std::cell::Cell::new(false));
    let current = Rc::new(RefCell::new(variant));
    let options = distro.filter(|_| template).map(|distro| {
        let grid = gtk::FlowBox::builder()
            .column_spacing(24)
            .row_spacing(8)
            .homogeneous(true)
            .min_children_per_line(1)
            .max_children_per_line(2)
            .selection_mode(gtk::SelectionMode::None)
            .build();
        grid.add_css_class("options");
        let regenerate = {
            let manual = manual.downgrade();
            let buffer = commands.buffer().downgrade();
            let current = current.clone();
            let composing = composing.clone();
            let variants = variants.clone();
            move |force: bool| {
                let mut variant = current.borrow_mut();
                if force || !variant.manual {
                    distro.compose(&mut variant);
                    composing.set(true);
                    if let Some(buffer) = buffer.upgrade() {
                        buffer.set_text(&variant.commands.join("\n"));
                    }
                    composing.set(false);
                    if let Some(manual) = manual.upgrade() {
                        manual.set_visible(false);
                    }
                }
                variants
                    .borrow_mut()
                    .insert(variant.distro.clone(), variant.clone());
            }
        };
        for option in distro.options {
            let title = tr(option.label);
            let check = gtk::CheckButton::new();
            check.update_property(&[gtk::accessible::Property::Label(&title)]);
            check.set_tooltip_text(Some(&tr(option.hint)));
            check.set_active(current.borrow().options.iter().any(|id| id == option.id));
            let copy = horizontal(6);
            let caption = label(&title, "option-label");
            caption.set_wrap(false);
            copy.append(&caption);
            if !option.flag.is_empty() {
                let flag = label(option.flag, "flag");
                flag.set_wrap(false);
                copy.append(&flag);
            }
            check.set_child(Some(&copy));
            let cell = gtk::FlowBoxChild::new();
            cell.set_focusable(false);
            cell.set_child(Some(&check));
            grid.append(&cell);
            let current = current.clone();
            let regenerate = regenerate.clone();
            check.connect_toggled(move |check| {
                model::set_off(
                    &mut current.borrow_mut().options,
                    option.id,
                    !check.is_active(),
                );
                regenerate(false);
            });
        }
        reset.connect_clicked(move |_| regenerate(true));
        grid
    });
    commands.buffer().connect_changed({
        let manual = manual.downgrade();
        let variants = variants.clone();
        let has_recipe = template && distro.is_some();
        move |buffer| {
            if composing.get() {
                return;
            }
            let mut variant = current.borrow_mut();
            variant.commands = distros::command_lines(&buffer_text(buffer));
            variant.manual = true;
            variants
                .borrow_mut()
                .insert(variant.distro.clone(), variant.clone());
            if let Some(manual) = manual.upgrade() {
                manual.set_visible(has_recipe);
            }
        }
    });
    for section in model::command_sections(options.is_some()) {
        match section {
            CommandSection::Options => {
                if let Some(grid) = &options {
                    row(
                        parent,
                        &tr("Options"),
                        &tr("Each option changes the commands below."),
                        grid,
                    );
                }
            }
            CommandSection::Commands => row(
                parent,
                &tr("Commands"),
                &trf(
                    "Editable. One per line, in a new terminal in {folder}.",
                    &[("folder", directory)],
                ),
                &controls,
            ),
        }
    }
}

type Editing = Rc<RefCell<Draft>>;

fn current_skill_result<T>(
    current_edit: u64,
    requested_edit: u64,
    current_request: u64,
    requested: u64,
    result: T,
) -> Option<T> {
    (current_edit == requested_edit && current_request == requested).then_some(result)
}

fn agent_editor(
    screen: &Screen,
    editor: &gtk::Box,
    draft: Editing,
    catalog: Option<Arc<Catalog>>,
    edit_id: u64,
) {
    let name = entry(&draft.borrow().name);
    name.set_placeholder_text(Some(&tr("Launch name")));
    name.set_tooltip_text(Some(&tr("Rename launch")));
    name.add_css_class("head");
    name.connect_changed({
        let draft = draft.clone();
        move |entry| {
            draft.borrow_mut().name = entry.text().into();
            refresh_save(entry);
        }
    });
    let header = horizontal(12);
    header.append(&name);
    let context = horizontal(8);
    context.add_css_class("context");
    let context_label = label("", "hint");
    context_label.set_wrap(false);
    let meter = gtk::ProgressBar::new();
    meter.set_size_request(110, -1);
    meter.set_valign(gtk::Align::Center);
    context.append(&context_label);
    context.append(&meter);
    header.append(&context);
    editor.append(&header);

    let Some(catalog) = catalog else {
        let tabs = horizontal(0);
        tabs.add_css_class("tabs");
        for (id, title) in model::tabs(&draft.borrow().agent) {
            let tab = button(&tr(title), "tab");
            if id == "general" {
                tab.add_css_class("on");
            }
            tabs.append(&tab);
        }
        editor.append(&tabs);
        let page = vertical(0);
        page.add_css_class("body");
        page.add_css_class("agent-body");
        general(screen, &page, &draft, None);
        editor.append(&scroll(&page));
        return;
    };

    let refresh = gio::SimpleAction::new("refresh", None);
    let actions = gio::SimpleActionGroup::new();
    actions.add_action(&refresh);
    editor.insert_action_group("workspace-metrics", Some(&actions));

    let tabs = horizontal(0);
    tabs.add_css_class("tabs");
    let stack = gtk::Stack::builder()
        .hhomogeneous(false)
        .vhomogeneous(false)
        .build();
    let data = Rc::new(RefCell::new(None::<Arc<model::Skills>>));
    let catalog = Rc::new(RefCell::new(catalog));
    let skills_directory = Rc::new(RefCell::new(None::<String>));
    let skills_request = Rc::new(Cell::new(0u64));
    let mut counters = Vec::new();
    let mut tab_buttons = Vec::new();
    for (id, title) in model::tabs(&draft.borrow().agent) {
        let button = button(&tr(title), "tab");
        if matches!(id, "skills" | "mcp" | "hooks") {
            counters.push((id, title, button.downgrade()));
        }
        tab_buttons.push((id, button.downgrade()));
        let page = vertical(0);
        page.add_css_class("body");
        page.add_css_class("agent-body");
        stack.add_named(&page, Some(id));
        let stack = stack.downgrade();
        button.connect_clicked(move |_| {
            if let Some(stack) = stack.upgrade() {
                stack.set_visible_child_name(id);
            }
        });
        tabs.append(&button);
    }
    let show_page = {
        let screen = screen.clone();
        let draft = draft.clone();
        let data = data.clone();
        let catalog = catalog.clone();
        let skills_directory = skills_directory.clone();
        let skills_request = skills_request.clone();
        Rc::new(move |stack: &gtk::Stack| {
            let Some(id) = stack.visible_child_name() else {
                return;
            };
            for (name, button) in &tab_buttons {
                if let Some(button) = button.upgrade() {
                    if id == *name {
                        button.add_css_class("on");
                    } else {
                        button.remove_css_class("on");
                    }
                }
            }
            if !stack.is_sensitive() {
                return;
            }
            let catalog = catalog.borrow().clone();
            if skills_directory.borrow().as_deref() != Some(draft.borrow().directory.as_str()) {
                let current = draft.borrow().clone();
                let directory = current.directory.clone();
                skills_directory.replace(Some(directory.clone()));
                data.take();
                for tab in ["skills", "summary"] {
                    if let Some(page) = tab_content(stack, tab) {
                        clear(&page);
                    }
                }
                let request_id = skills_request.get().wrapping_add(1);
                skills_request.set(request_id);
                let (stack, screen, draft, data, skills_directory, skills_request, catalog) = (
                    stack.downgrade(),
                    screen.clone(),
                    draft.clone(),
                    data.clone(),
                    skills_directory.clone(),
                    skills_request.clone(),
                    catalog.clone(),
                );
                glib::spawn_future_local(async move {
                    if screen.edit_id.get() != edit_id
                        || skills_request.get() != request_id
                        || stack.upgrade().is_none()
                    {
                        return;
                    }
                    let worker = catalog.clone();
                    let scans = screen.scans.clone();
                    let result = gio::spawn_blocking(move || worker.skills(&current, &scans)).await;
                    let Some(result) = current_skill_result(
                        screen.edit_id.get(),
                        edit_id,
                        skills_request.get(),
                        request_id,
                        result,
                    ) else {
                        return;
                    };
                    let Some(stack) = stack.upgrade() else { return };
                    if draft.borrow().directory != directory {
                        skills_directory.take();
                        return;
                    }
                    let skills = result.unwrap_or_else(|_| {
                        Arc::new(model::Skills::load(
                            &draft.borrow(),
                            None,
                            &catalog.library,
                            &screen.scans,
                        ))
                    });
                    data.replace(Some(skills));
                    if let Some(id) = stack.visible_child_name()
                        && matches!(id.as_str(), "skills" | "summary")
                        && let Some(page) = tab_content(&stack, &id)
                    {
                        clear(&page);
                        agent_tab(&screen, &page, &draft, &id, data.borrow().clone(), &catalog);
                    }
                    let _ = stack.activate_action("workspace-metrics.refresh", None);
                });
            }
            if let Some(page) = tab_content(stack, &id)
                && page.first_child().is_none()
            {
                agent_tab(&screen, &page, &draft, &id, data.borrow().clone(), &catalog);
            }
            let _ = stack.activate_action("workspace-metrics.refresh", None);
        })
    };
    refresh.connect_activate({
        let draft = draft.clone();
        let catalog = catalog.clone();
        let data = data.clone();
        move |_, _| {
            let catalog = catalog.borrow();
            let (claude, codex) = &catalog.inventory;
            let data = data.borrow();
            let draft = draft.borrow();
            let is_claude = draft.agent == CLAUDE;
            meter.set_visible(is_claude && data.is_some());
            if is_claude && let Some(data) = data.as_ref() {
                let tokens = data.estimate(&draft, claude);
                context_label.set_text(&format!(
                    "{} {}",
                    tr("Initial context estimated"),
                    model::k_tokens(tokens)
                ));
                let ceiling = model::estimate_ceiling(claude, &data.library);
                let percentage = (100. * tokens as f64 / ceiling as f64).round();
                meter.set_fraction(percentage.min(100.) / 100.);
            } else if is_claude {
                context_label.set_text("");
            } else {
                context_label.set_text(&tr("Initial context: no estimate for Codex"));
            }
            for (id, title, button) in &counters {
                if let Some(button) = button.upgrade() {
                    let text = match *id {
                        "skills" => data
                            .as_ref()
                            .map(|data| model::skill_counts(&draft, claude, data)),
                        "mcp" => Some(model::mcp_counts(&draft.claude, claude)),
                        "hooks" => Some(model::hook_tab_counts(&draft, claude, codex)),
                        _ => None,
                    };
                    let title = tr(title);
                    if let Some((on, total)) = text {
                        button.set_label(&format!("{title} {on}/{total}"));
                    } else {
                        button.set_label(&title);
                    }
                }
            }
        }
    });
    editor.append(&tabs);
    editor.append(&scroll(&stack));
    show_page(&stack);
    stack.connect_visible_child_notify({
        let show_page = show_page.clone();
        move |stack| show_page(stack)
    });

    let original = draft.borrow().id.clone();
    let delete = original
        .filter(|_| draft.borrow().before.is_empty())
        .map(|id| {
            let screen = screen.clone();
            move || screen.delete(&id, false)
        });
    let can_test = screen.try_launch.is_some();
    let test = {
        let screen = screen.clone();
        let draft = draft.clone();
        let action = screen.try_launch.clone();
        move || {
            let Some(action) = &action else { return };
            if !draft.borrow().uses_global() {
                screen.message(&tr("This preset can only be tried in the Console app."));
                return;
            }
            let current = draft.borrow().clone();
            if let Some(id) = current.id.as_deref().filter(|_| !current.dirty()) {
                action(id);
            } else if current.can_save() {
                screen.write(
                    move |places| model::save(places, &current).map(Written::Saved),
                    Some(action.clone()),
                );
            }
        }
    };
    let (save, try_button) = foot(editor, delete, Some(test), {
        let screen = screen.clone();
        let draft = draft.clone();
        move || {
            let current = draft.borrow().clone();
            screen.write(
                move |places| model::save(places, &current).map(Written::Saved),
                None,
            );
        }
    });
    let draft_for_reload = draft.clone();
    let screen_for_reload = screen.clone();
    let weak_stack = stack.downgrade();
    let summary = tab_content(&stack, "summary").map(|page| page.downgrade());
    let check = gio::SimpleAction::new("check-save", None);
    check.connect_activate(move |_, _| {
        // Summary is a snapshot of the draft. Refresh it on the next visit after an edit.
        if let Some(page) = summary.as_ref().and_then(glib::WeakRef::upgrade) {
            clear(&page);
        }
        let current = draft.borrow();
        save.set_sensitive(current.dirty() && current.can_save());
        if let Some(try_button) = &try_button {
            try_button.set_sensitive(
                can_test && (current.can_save() || (current.id.is_some() && !current.dirty())),
            );
        }
    });
    actions.add_action(&check);
    check.activate(None);

    let reload = gio::SimpleAction::new("reload", Some(gtk::glib::VariantTy::STRING));
    reload.connect_activate(move |_, parameter| {
        let Some(stack) = weak_stack.upgrade() else {
            return;
        };
        let Some(id) = parameter.and_then(glib::Variant::str) else {
            return;
        };
        let Some(page) = tab_content(&stack, id) else {
            return;
        };
        if !stack.is_sensitive() {
            return;
        }
        stack.set_sensitive(false);
        skills_request.set(skills_request.get().wrapping_add(1));
        let (stack, page, places, screen, draft, skills_directory, data, catalog, show_page) = (
            stack.downgrade(),
            page.downgrade(),
            screen_for_reload.places.clone(),
            screen_for_reload.clone(),
            draft_for_reload.clone(),
            skills_directory.clone(),
            data.clone(),
            catalog.clone(),
            show_page.clone(),
        );
        let current = draft.borrow().clone();
        glib::spawn_future_local(async move {
            let scans = screen.scans.clone();
            let loaded = gio::spawn_blocking(move || {
                let mut current = current;
                let fresh = Arc::new(Catalog::load(&places));
                let store = fresh.store.as_ref().map(|store| store.lock().unwrap());
                current.include_new_skills(store.as_deref());
                drop(store);
                scans.invalidate();
                (current, fresh)
            })
            .await;
            if screen.edit_id.get() != edit_id {
                return;
            }
            let (Some(stack), Some(page)) = (stack.upgrade(), page.upgrade()) else {
                return;
            };
            stack.set_sensitive(true);
            skills_directory.take();
            data.take();
            if let Ok((current, fresh)) = loaded {
                {
                    let mut draft = draft.borrow_mut();
                    draft.items = current.items;
                    draft.new_skills = current.new_skills;
                }
                catalog.replace(fresh);
                for (id, _) in model::tabs(&current.agent) {
                    if let Some(content) = tab_content(&stack, id) {
                        clear(&content);
                    }
                }
                refresh_save(&page);
            }
            show_page(&stack);
        });
    });
    actions.add_action(&reload);
}

fn tab_content(stack: &gtk::Stack, id: &str) -> Option<gtk::Box> {
    stack.child_by_name(id)?.downcast().ok()
}

fn refresh_save(widget: &impl IsA<gtk::Widget>) {
    let _ = widget.activate_action("workspace-metrics.check-save", None);
}

fn agent_tab(
    screen: &Screen,
    parent: &gtk::Box,
    draft: &Editing,
    id: &str,
    data: Option<Arc<model::Skills>>,
    catalog: &Arc<Catalog>,
) {
    let _timing = Timing::new(match id {
        "general" => "editor:general",
        "skills" => "editor:skills",
        "mcp" => "editor:mcp",
        "hooks" => "editor:hooks",
        "summary" => "editor:summary",
        _ => return,
    });
    if matches!(id, "skills" | "summary") && data.is_none() {
        parent.append(&label(&tr(t!("Checking what can load…")), "hint"));
        return;
    }
    match id {
        "general" => general(screen, parent, draft, Some(catalog)),
        "skills" => skills(parent, draft, data.unwrap(), catalog),
        _ => {
            let (claude, codex) = &catalog.inventory;
            let is_claude = draft.borrow().agent == CLAUDE;
            match id {
                "mcp" => mcp(parent, draft, claude),
                "hooks" => {
                    let entries = if is_claude {
                        &claude.hooks
                    } else {
                        &codex.hooks
                    };
                    let fixed = if is_claude {
                        vec![]
                    } else {
                        model::fixed_hooks(codex)
                    };
                    hooks(parent, draft, entries, &fixed);
                }
                "summary" => summary(parent, &draft.borrow(), claude, codex, &data.unwrap()),
                _ => {}
            }
        }
    }
}

fn general(screen: &Screen, parent: &gtk::Box, draft: &Editing, catalog: Option<&Arc<Catalog>>) {
    let current = draft.borrow();
    let mut providers = vec![(CLAUDE, "Claude Code"), (CODEX, "Codex")];
    // Distributor definitions supply their display names when they describe the supported agents.
    let installed = catalog.and_then(|catalog| catalog.distributors.as_ref());
    for (id, label) in &mut providers {
        if let Some(found) =
            installed.and_then(|list| list.distributors.iter().find(|item| item.id == *id))
        {
            *label = &found.name;
        }
    }
    let application = select(&providers, &current.agent);
    row(
        parent,
        &tr(model::GENERAL_FIELDS[0]),
        &tr("The program that starts in the terminal."),
        &application.widget,
    );

    let combo = horizontal(0);
    combo.add_css_class("linked");
    combo.set_hexpand(true);
    let directory = entry(&current.directory);
    directory.add_css_class("mono");
    combo.append(&directory);
    let saved = gtk::MenuButton::builder()
        .icon_name("pan-down-symbolic")
        .tooltip_text(tr("Saved directories"))
        .build();
    let directory_popover = gtk::Popover::new();
    let directories = vertical(2);
    let home = screen.places.home.to_string_lossy().into_owned();
    directory_choices(
        &directories,
        &directory,
        &directory_popover,
        vec![home.clone()],
        &[],
    );
    if let Some(host) = screen.recent.clone() {
        let host = Cell::new(Some(host));
        let screen = screen.clone();
        let directories = directories.downgrade();
        let directory = directory.downgrade();
        directory_popover.connect_map(move |popover| {
            let Some(host) = host.take() else { return };
            let (screen, directories, directory, popover, home) = (
                screen.clone(),
                directories.clone(),
                directory.clone(),
                popover.downgrade(),
                home.clone(),
            );
            glib::spawn_future_local(async move {
                let Ok(Some(mut recent)) = gio::spawn_blocking(move || {
                    crate::native::launcher(&host)
                        .ok()
                        .map(|launcher| launcher.projects)
                })
                .await
                else {
                    return;
                };
                let (Some(directories), Some(directory), Some(popover)) = (
                    directories.upgrade(),
                    directory.upgrade(),
                    popover.upgrade(),
                ) else {
                    return;
                };
                recent.sort_by_key(|(_, last_used)| std::cmp::Reverse(*last_used));
                let recent: Vec<_> = recent.into_iter().map(|(path, _)| path).collect();
                let mut paths = vec![home];
                let cache = screen.cache.borrow();
                if let Some(listing) = cache.as_ref().and_then(|cache| cache.listing.as_ref().ok())
                {
                    paths.extend(
                        listing
                            .entries
                            .launches
                            .iter()
                            .filter(|launch| recent.contains(&launch.directory))
                            .map(|launch| launch.directory.clone()),
                    );
                }
                clear(&directories);
                directory_choices(
                    &directories,
                    &directory,
                    &popover,
                    paths,
                    &recent[..recent.len().min(8)],
                );
            });
        });
    }
    directory_popover.set_child(Some(&scroll(&directories)));
    saved.set_popover(Some(&directory_popover));
    combo.append(&saved);
    let browse = button(&tr("Select"), "secondary");
    let folder = horizontal(8);
    folder.append(&combo);
    folder.append(&browse);
    row(
        parent,
        &tr(model::GENERAL_FIELDS[1]),
        &tr("The folder the session opens in."),
        &folder,
    );
    directory.connect_changed({
        let draft = draft.clone();
        move |entry| {
            draft.borrow_mut().directory = entry.text().into();
            refresh_save(entry);
        }
    });
    browse.connect_clicked({
        let window = screen.window.clone();
        let directory = directory.downgrade();
        move |_| {
            let Some(window) = window.upgrade() else {
                return;
            };
            let chooser = gtk::FileDialog::builder()
                .title(tr("Working directory"))
                .modal(true)
                .accept_label(tr("Select"))
                .build();
            if let Some(entry) = directory.upgrade() {
                chooser.set_initial_folder(Some(&gio::File::for_path(entry.text().as_str())));
            }
            chooser.select_folder(Some(&window), gio::Cancellable::NONE, {
                let directory = directory.clone();
                move |result| {
                    if let Some(path) = result.ok().and_then(|file| file.path())
                        && let Some(entry) = directory.upgrade()
                    {
                        entry.set_text(&path.to_string_lossy());
                    }
                }
            });
        }
    });

    let items: Vec<_> = std::iter::once("")
        .chain(model::permissions(&current.agent))
        .map(|id| (id, tr(model::permission_label(id))))
        .collect();
    let items: Vec<_> = items
        .iter()
        .map(|(id, text)| (*id, text.as_str()))
        .collect();
    let permissions = select(&items, current.permission.as_deref().unwrap_or(""));
    let controls = vertical(6);
    controls.append(&permissions.widget);
    let advice = label("", "hint");
    controls.append(&advice);
    permission_help(&advice, permissions.selected_id().unwrap_or(""));
    row(
        parent,
        &tr(model::GENERAL_FIELDS[2]),
        &tr("Which actions need your approval."),
        &controls,
    );
    permissions.connect_changed({
        let draft = draft.clone();
        let widget = permissions.widget.downgrade();
        move |id| {
            draft.borrow_mut().permission = (!id.is_empty()).then(|| id.into());
            permission_help(&advice, id);
            if let Some(widget) = widget.upgrade() {
                refresh_save(&widget);
            }
        }
    });

    let Some(catalog) = catalog else { return };
    application.connect_changed({
        let screen = screen.clone();
        let draft = draft.clone();
        let catalog = catalog.clone();
        move |application| {
            let Some(editor) = screen
                .editor
                .upgrade()
                .filter(|editor| editor.is_sensitive())
            else {
                return;
            };
            editor.set_sensitive(false);
            let edit_id = screen.edit_id.get();
            let mut current = draft.borrow().clone();
            let application = application.to_owned();
            screen.prepare_agent(edit_id, catalog.clone(), move |catalog| {
                let store = catalog.store.as_ref().map(|store| store.lock().unwrap());
                model::change_agent(&mut current, store.as_deref(), &application)?;
                Ok(current)
            });
        }
    });
    drop(current);
    let (claude, codex) = &catalog.inventory;
    instructions(screen, parent, draft, claude, codex.instructions);
}

fn directory_choices(
    parent: &gtk::Box,
    entry: &gtk::Entry,
    popover: &gtk::Popover,
    mut saved: Vec<String>,
    recent: &[String],
) {
    saved.sort();
    saved.dedup();
    for (title, paths) in [
        (t!("Saved folders"), saved.as_slice()),
        (t!("Recent"), recent),
    ] {
        if paths.is_empty() {
            continue;
        }
        section(parent, &tr(title));
        for path in paths {
            let choice = button(path, "flat");
            let (path, entry, popover) = (path.clone(), entry.downgrade(), popover.downgrade());
            choice.connect_clicked(move |_| {
                if let Some(entry) = entry.upgrade() {
                    entry.set_text(&path);
                }
                if let Some(popover) = popover.upgrade() {
                    popover.popdown();
                }
            });
            parent.append(&choice);
        }
    }
}

fn instructions(
    screen: &Screen,
    parent: &gtk::Box,
    draft: &Editing,
    options: &wyss_console::native::launch_home::Options,
    codex_instructions: bool,
) {
    let _timing = Timing::new("editor:instructions");
    use arsenal::role_instructions::Choice;
    let (selected, contents) = match &draft.borrow().project {
        Choice::Load => ("0", String::new()),
        Choice::Exclude { role } => ("1", role.clone()),
        Choice::Role { role } => ("2", role.clone()),
    };
    let modes = [
        ("0", tr("Load")),
        ("1", tr("Skip")),
        ("2", tr("Custom text")),
    ];
    let mode = select(
        &modes.each_ref().map(|(id, label)| (*id, label.as_str())),
        selected,
    );
    let mode_row = vertical(6);
    mode.widget.set_valign(gtk::Align::Start);
    mode_row.append(&mode.widget);
    let description = label(&project_help(selected), "hint");
    mode_row.append(&description);
    row(
        parent,
        &tr(model::GENERAL_FIELDS[3]),
        &tr("The CLAUDE.md or AGENTS.md of the working directory."),
        &mode_row,
    );
    let custom = vertical(0);
    let body = text_view(&contents);
    body.set_size_request(-1, 110);
    row(
        &custom,
        &tr("Custom text"),
        &tr("Read instead of the project’s instructions."),
        &body,
    );
    custom.set_visible(selected == "2");
    parent.append(&custom);
    mode.connect_changed({
        let draft = draft.clone();
        let body = body.buffer().downgrade();
        let widget = mode.widget.downgrade();
        move |mode| {
            description.set_text(&project_help(mode));
            let index = mode.parse().unwrap_or(0);
            custom.set_visible(index == 2);
            let text = body
                .upgrade()
                .map(|buffer| buffer_text(&buffer))
                .unwrap_or_default();
            draft.borrow_mut().project = model::project_choice(index, text);
            if let Some(widget) = widget.upgrade() {
                refresh_save(&widget);
            }
        }
    });
    body.buffer().connect_changed({
        let draft = draft.clone();
        let mode = mode.widget.downgrade();
        move |buffer| {
            if let Some(mode) = mode.upgrade().filter(|mode| mode.selected() == 2) {
                draft.borrow_mut().project = model::project_choice(2, buffer_text(buffer));
                refresh_save(&mode);
            }
        }
    });

    let global = vertical(0);
    let is_claude = draft.borrow().agent == CLAUDE;
    let claude_dir = Owner::from_home(&screen.places.home).dir;
    let codex_dir = CodexOwner::from_process(&screen.places.home).dir;
    let mut files = Vec::new();
    if is_claude && options.instructions {
        files.push("CLAUDE.md".to_owned());
    }
    if is_claude {
        files.extend(options.rules.iter().map(|rule| format!("rules/{rule}")));
    }
    if !is_claude && codex_instructions {
        files.push("codex/AGENTS.md".to_owned());
    }
    let rules = vertical(0);
    let mut rule_switches = Vec::<(gtk::glib::WeakRef<gtk::Switch>, String)>::new();
    let updating = Rc::new(std::cell::Cell::new(false));
    let have_files = !files.is_empty();
    for file in files.into_iter().rev() {
        let rule = file.strip_prefix("rules/").map(str::to_owned);
        let display = file.trim_start_matches("codex/");
        let on = {
            let draft = draft.borrow();
            if is_claude {
                draft.claude.instructions
                    && rule
                        .as_ref()
                        .is_none_or(|rule| !draft.claude.rules_off.contains(rule))
            } else {
                draft.codex.instructions
            }
        };
        let control = switch(display, on, {
            let draft = draft.clone();
            let rule = rule.clone();
            let updating = updating.clone();
            let rule_switches = rule_switches.clone();
            move |on| {
                if updating.get() {
                    return;
                }
                if let Some(rule) = &rule {
                    model::set_off(&mut draft.borrow_mut().claude.rules_off, rule, on);
                } else if is_claude {
                    draft.borrow_mut().claude.instructions = on;
                    updating.set(true);
                    for (switch, rule) in &rule_switches {
                        if let Some(switch) = switch.upgrade() {
                            switch.set_sensitive(on);
                            switch
                                .set_active(on && !draft.borrow().claude.rules_off.contains(rule));
                        }
                    }
                    updating.set(false);
                } else {
                    draft.borrow_mut().codex.instructions = on;
                }
            }
        });
        let target = if let Some(rule) = &rule {
            control.set_sensitive(draft.borrow().claude.instructions);
            rule_switches.push((control.downgrade(), rule.clone()));
            &rules
        } else {
            &global
        };
        let link = button(display, "instruction-link");
        link.set_halign(gtk::Align::Start);
        link.set_tooltip_text(Some(&tr("Show its text")));
        link.connect_clicked({
            let key = file.clone();
            let claude = claude_dir.clone();
            let codex = codex_dir.clone();
            let window = screen.window.clone();
            move |link| {
                let Some(window) = window.upgrade() else {
                    return;
                };
                let dialog = gtk::Window::builder()
                    .title(link.label().unwrap_or_default())
                    .transient_for(&window)
                    .modal(true)
                    .default_width(800)
                    .default_height(540)
                    .build();
                dialog.set_application(window.application().as_ref());
                dialog.set_destroy_with_parent(true);
                let content = vertical(8);
                dialog.set_child(Some(&content));
                let waiting = label(&tr("Reading…"), "hint");
                content.append(&waiting);
                dialog.present();
                let (content, key, claude, codex) = (
                    content.downgrade(),
                    key.clone(),
                    claude.clone(),
                    codex.clone(),
                );
                glib::spawn_future_local(async move {
                    let result = gio::spawn_blocking(move || {
                        let _timing = Timing::new("editor:disk:instruction-text");
                        instruction_text::read(&key, &claude, &codex)
                    })
                    .await;
                    let Some(content) = content.upgrade() else {
                        return;
                    };
                    clear(&content);
                    match result {
                        Ok(Ok(text)) => {
                            let view = text_view(&text);
                            view.set_editable(false);
                            view.set_cursor_visible(false);
                            view.add_css_class("mono");
                            content.append(&scroll(&view));
                        }
                        _ => content.append(&label(&tr("The file could not be read."), "hint")),
                    }
                });
            }
        });
        let line = horizontal(12);
        line.add_css_class("instruction-row");
        line.append(&link);
        let size = if is_claude {
            let key = rule
                .as_ref()
                .map_or_else(|| "instructions".into(), |rule| format!("rule:{rule}"));
            options
                .sizes
                .get(&key)
                .map(|bytes| format!("~{}", model::k_tokens(bytes.div_ceil(4))))
                .unwrap_or_default()
        } else {
            String::new()
        };
        let estimate = label(&size, "hint");
        estimate.set_hexpand(true);
        estimate.set_xalign(1.);
        line.append(&estimate);
        line.append(&control);
        target.prepend(&line);
    }
    global.append(&rules);
    if !have_files {
        global.append(&label(&tr("Nothing of this kind."), "hint"));
    }
    row(
        parent,
        &tr(model::GENERAL_FIELDS[4]),
        &tr("Your own instruction files. Open one to read it."),
        &global,
    );
}

fn read_again(id: &'static str) -> gtk::Button {
    let button = button(&tr("Read again"), "secondary");
    button.connect_clicked(move |button| {
        let _ = button.activate_action("workspace-metrics.reload", Some(&id.to_variant()));
    });
    button
}

fn fold(parent: &gtk::Box, name: &str, open: bool, nested: bool) -> (gtk::Box, gtk::Box) {
    let header = horizontal(8);
    header.set_valign(gtk::Align::Start);
    header.add_css_class(if nested {
        "tree-category"
    } else {
        "tree-section"
    });
    let trigger = button("", "fold-trigger");
    trigger.set_hexpand(true);
    let content = horizontal(12);
    let arrow = gtk::Image::from_icon_name("pan-end-symbolic");
    arrow.set_pixel_size(12);
    content.append(&arrow);
    let title = label(name, if nested { "lbl" } else { "value" });
    title.set_wrap(false);
    content.append(&title);
    trigger.set_child(Some(&content));
    trigger.update_property(&[gtk::accessible::Property::Label(name)]);
    header.append(&trigger);
    parent.append(&header);

    let rows = vertical(0);
    rows.set_valign(gtk::Align::Start);
    rows.set_visible(open);
    rows.add_css_class(if nested { "tree-entries" } else { "tree-body" });
    parent.append(&rows);
    let update = move |button: &gtk::Button, open| {
        arrow.set_icon_name(Some(if open {
            "pan-down-symbolic"
        } else {
            "pan-end-symbolic"
        }));
        button.update_state(&[gtk::accessible::State::Expanded(Some(open))]);
    };
    update(&trigger, open);
    trigger.connect_clicked({
        let rows = rows.downgrade();
        move |button| {
            if let Some(rows) = rows.upgrade() {
                let open = !rows.is_visible();
                rows.set_valign(gtk::Align::Start);
                rows.set_visible(open);
                update(button, open);
            }
        }
    });
    (header, rows)
}

fn switch_count(controls: &[gtk::Switch], total: usize) -> gtk::Label {
    let active = controls.iter().filter(|item| item.is_active()).count();
    let badge = label(&format!("{active}/{total}"), "hint");
    badge.set_wrap(false);
    let weak = controls
        .iter()
        .map(ObjectExt::downgrade)
        .collect::<Vec<_>>();
    for control in controls {
        let weak = weak.clone();
        let badge = badge.downgrade();
        control.connect_active_notify(move |_| {
            let active = weak
                .iter()
                .filter_map(gtk::glib::WeakRef::upgrade)
                .filter(gtk::Switch::is_active)
                .count();
            if let Some(badge) = badge.upgrade() {
                badge.set_text(&format!("{active}/{total}"));
            }
        });
    }
    badge
}

fn category_switch(name: &str, children: &[gtk::Switch]) -> gtk::Switch {
    let on = !children.is_empty() && children.iter().all(gtk::Switch::is_active);
    let control = gtk::Switch::builder()
        .active(on)
        .valign(gtk::Align::Center)
        .build();
    control.add_css_class("sw");
    control.update_property(&[gtk::accessible::Property::Label(name)]);
    control.set_sensitive(!children.is_empty());
    let updating = Rc::new(std::cell::Cell::new(false));
    let children_weak = children
        .iter()
        .map(ObjectExt::downgrade)
        .collect::<Vec<_>>();
    control.connect_active_notify({
        let updating = updating.clone();
        let children = children_weak.clone();
        move |control| {
            if updating.get() {
                return;
            }
            updating.set(true);
            for child in children.iter().filter_map(gtk::glib::WeakRef::upgrade) {
                child.set_active(control.is_active());
            }
            updating.set(false);
        }
    });
    for child in children {
        let category = control.downgrade();
        let children = children_weak.clone();
        let updating = updating.clone();
        child.connect_active_notify(move |_| {
            if !updating.get()
                && let Some(category) = category.upgrade()
            {
                updating.set(true);
                category.set_active(
                    children
                        .iter()
                        .filter_map(gtk::glib::WeakRef::upgrade)
                        .all(|child| child.is_active()),
                );
                updating.set(false);
            }
        });
    }
    control
}

fn tools(
    parent: &gtk::Box,
    id: &'static str,
    lead: &impl IsA<gtk::Widget>,
    count: Option<&gtk::Label>,
) {
    let bar = horizontal(12);
    bar.add_css_class("tree-tools");
    lead.set_hexpand(true);
    bar.append(lead);
    if let Some(count) = count {
        count.set_wrap(false);
        bar.append(count);
    }
    bar.append(&read_again(id));
    parent.append(&bar);
}

fn skills(parent: &gtk::Box, draft: &Editing, data: Arc<model::Skills>, catalog: &Arc<Catalog>) {
    let query = gtk::SearchEntry::new();
    query.set_placeholder_text(Some(&tr("Filter skills")));
    query.update_property(&[gtk::accessible::Property::Label(&tr("Filter skills"))]);
    let count = label("", "hint");
    tools(parent, "skills", &query, Some(&count));
    let list = vertical(0);
    parent.append(&list);
    let catalog = catalog.clone();
    let render = {
        let list = list.downgrade();
        let count = count.downgrade();
        let draft = draft.clone();
        move |search: &gtk::SearchEntry| {
            let (Some(list), Some(count)) = (list.upgrade(), count.upgrade()) else {
                return;
            };
            clear(&list);
            let needle = search.text().trim().to_lowercase();
            let update = Rc::new({
                let draft = draft.clone();
                let count = count.downgrade();
                let catalog = catalog.clone();
                let data = data.clone();
                move || {
                    let Some(count) = count.upgrade() else { return };
                    let draft = draft.borrow();
                    let (on, total) = model::skill_counts(&draft, &catalog.inventory.0, &data);
                    let mut text = count_text(on, total);
                    if draft.agent == CLAUDE {
                        text.push_str(&format!(" · ~{}", model::k_tokens(data.listings(&draft))));
                    }
                    count.set_text(&text);
                }
            });
            update();
            let current = draft.borrow();
            for group in data.groups(&current.agent, &catalog.inventory.0.account_skills, &needle) {
                let (heading, rows) = fold(&list, &tr(group.label), group.open, false);
                let mut all = Vec::new();
                let mut group_total = 0;
                for category in group.categories {
                    let (header, children) = fold(&rows, &tr(category.label), category.open, true);
                    let entries = category.entries;
                    let total = entries.len();
                    group_total += total;
                    let mut available = Vec::new();
                    for (name, id, account) in entries {
                        if let Some(reason) = (!account).then(|| data.reasons.get(id)).flatten() {
                            toggle_row(
                                &children,
                                name,
                                &crate::i18n::incoming(reason),
                                "hint",
                                None,
                            );
                            continue;
                        }
                        let on = if account {
                            !current
                                .claude
                                .account_skills_off
                                .iter()
                                .any(|key| key == id)
                        } else {
                            data.active(&current, id)
                        };
                        let control = switch(name, on, {
                            let draft = draft.clone();
                            let id = id.to_owned();
                            let update = update.clone();
                            move |on| {
                                let mut current = draft.borrow_mut();
                                if account {
                                    model::set_off(&mut current.claude.account_skills_off, &id, on);
                                } else {
                                    current.set_skill(&id, on);
                                }
                                drop(current);
                                update();
                            }
                        });
                        toggle_row(&children, name, "", "value", Some(&control));
                        available.push(control);
                    }
                    let category = category_switch(&tr(category.label), &available);
                    let badge = switch_count(&available, total);
                    header.append(&badge);
                    header.append(&category);
                    all.extend(available);
                }
                let badge = switch_count(&all, group_total);
                heading.append(&badge);
            }
        }
    };
    render(&query);
    query.connect_search_changed(render);
}

fn mcp(parent: &gtk::Box, draft: &Editing, options: &wyss_console::native::launch_home::Options) {
    tools(
        parent,
        "mcp",
        &label(&tr("To add one, ask Claude."), "hint"),
        None,
    );
    let servers = Rc::new(options.mcp.clone());
    let (header, rows) = fold(parent, &tr("Your servers"), false, false);
    let (_, connectors) = fold(parent, &tr("Your claude.ai account"), false, false);
    let control = gtk::Switch::builder()
        .active(draft.borrow().claude.mcp.is_none())
        .valign(gtk::Align::Center)
        .build();
    control.add_css_class("sw");
    control.update_property(&[gtk::accessible::Property::Label(&tr("Account connectors"))]);
    let updating = Rc::new(std::cell::Cell::new(false));
    let mut switches = Vec::new();
    for name in servers.iter() {
        let server = switch(name, draft.borrow().claude.mcp_enabled(name), {
            let draft = draft.clone();
            let servers = servers.clone();
            let name = name.clone();
            let account = control.downgrade();
            let updating = updating.clone();
            move |on| {
                if updating.get() {
                    return;
                }
                model::set_mcp(
                    &mut draft.borrow_mut().claude,
                    &servers,
                    std::slice::from_ref(&name),
                    on,
                );
                if let Some(account) = account.upgrade() {
                    updating.set(true);
                    account.set_active(false);
                    updating.set(false);
                }
            }
        });
        toggle_row(&rows, name, "", "value", Some(&server));
        switches.push(server);
    }
    if servers.is_empty() {
        rows.append(&label(&tr("Nothing of this kind."), "hint"));
    }
    let badge = switch_count(&switches, switches.len());
    header.append(&badge);
    control.connect_active_notify({
        let draft = draft.clone();
        let switches = switches
            .iter()
            .map(ObjectExt::downgrade)
            .collect::<Vec<_>>();
        move |control| {
            if updating.get() {
                return;
            }
            let on = control.is_active();
            let mut current = draft.borrow_mut();
            current.claude.mcp = if on {
                None
            } else {
                Some(servers.as_ref().clone())
            };
            current.claude.mcp_seen = if on {
                None
            } else {
                Some(servers.as_ref().clone())
            };
            drop(current);
            updating.set(true);
            for server in switches.iter().filter_map(gtk::glib::WeakRef::upgrade) {
                server.set_active(true);
            }
            updating.set(false);
            refresh_save(control);
            let _ = control.activate_action("workspace-metrics.refresh", None);
        }
    });
    toggle_row(
        &connectors,
        &tr("Account connectors"),
        &tr("Only with every server above on: Claude loads them all or none."),
        "value",
        Some(&control),
    );
    if !options.plugins.is_empty() {
        let (header, rows) = fold(parent, &tr("Plugins"), false, false);
        let mut children = Vec::new();
        for name in &options.plugins {
            let control = switch(name, !draft.borrow().claude.plugins_off.contains(name), {
                let draft = draft.clone();
                let name = name.clone();
                move |on| model::set_off(&mut draft.borrow_mut().claude.plugins_off, &name, on)
            });
            toggle_row(&rows, name, "", "value", Some(&control));
            children.push(control);
        }
        let badge = switch_count(&children, children.len());
        header.append(&badge);
    }
}

fn hook_matchers(entries: &[HookOption]) -> String {
    let mut words = Vec::new();
    for tool in entries
        .iter()
        .filter(|hook| !hook.matcher.is_empty())
        .flat_map(|hook| hook.matcher.split('|'))
        .map(str::trim)
    {
        let text = tr(match tool {
            "" => t!("Always"),
            "Bash" => t!("Commands"),
            "Write" | "Edit" | "MultiEdit" => t!("Writing/editing"),
            "Read" => t!("Reading files"),
            "Agent" | "Task" => t!("Agents"),
            other => other,
        });
        if !words.contains(&text) {
            words.push(text);
        }
    }
    if words.is_empty() {
        tr("Always")
    } else {
        words.join(", ")
    }
}

fn hooks(parent: &gtk::Box, draft: &Editing, entries: &[HookOption], fixed: &[HookOption]) {
    let groups = Rc::new(model::hooks(entries));
    let (on, total) = model::hook_counts(&draft.borrow(), &groups);
    let count = label(&count_text(on, total), "hint");
    tools(
        parent,
        "hooks",
        &label(&tr("No context at start. To add one, ask Claude."), "hint"),
        Some(&count),
    );
    if groups.is_empty() {
        parent.append(&label(&tr("Nothing of this kind."), "hint"));
    }
    for (event, entries) in groups.iter() {
        let (header, rows) = fold(parent, &tr(model::event_label(event)), false, false);
        let mut children = Vec::new();
        for (script, hooks) in entries {
            let on = hooks
                .iter()
                .all(|hook| !draft.borrow().hook_exclusions().contains(&hook.key));
            let control = switch(script, on, {
                let draft = draft.clone();
                let hooks = hooks.clone();
                let groups = groups.clone();
                let count = count.downgrade();
                move |on| {
                    draft.borrow_mut().set_hooks(&hooks, on);
                    if let Some(count) = count.upgrade() {
                        let (on, total) = model::hook_counts(&draft.borrow(), &groups);
                        count.set_text(&count_text(on, total));
                    }
                }
            });
            toggle_row(&rows, script, &hook_matchers(hooks), "mono", Some(&control));
            children.push(control);
        }
        let badge = switch_count(&children, children.len());
        header.append(&badge);
    }
    if !fixed.is_empty() {
        let (_, rows) = fold(parent, &tr("Always on (config.toml)"), false, false);
        for (_, entries) in model::hooks(fixed) {
            for (script, hooks) in entries {
                toggle_row(&rows, &script, &hook_matchers(&hooks), "mono", None);
            }
        }
    }
}

fn fact(parent: &gtk::Box, title: &str, text: &str, class: &str) {
    let line = horizontal(0);
    line.add_css_class("summary-fact");
    let title = label(title, "fact-title");
    title.set_width_request(260);
    let value = label(if text.is_empty() { "—" } else { text }, class);
    value.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    value.set_selectable(true);
    value.set_hexpand(true);
    line.append(&title);
    line.append(&value);
    parent.append(&line);
}

fn summary(
    parent: &gtk::Box,
    draft: &Draft,
    claude: &wyss_console::native::launch_home::Options,
    codex: &wyss_console::native::launch_home_codex::CodexOptions,
    data: &model::Skills,
) {
    let is_claude = draft.agent == CLAUDE;
    if if is_claude {
        draft.claude.is_default()
    } else {
        draft.codex.is_default()
    } {
        parent.append(&label(
            &tr(if is_claude {
                t!("This launch changes nothing: it uses your full ~/.claude.")
            } else {
                t!("This launch changes nothing: it uses your full ~/.codex.")
            }),
            "summary-note",
        ));
    }
    let (_, start) = fold(parent, &tr("At start"), true, false);
    fact(
        &start,
        &tr("Application"),
        if is_claude { "Claude Code" } else { "Codex" },
        "value",
    );
    fact(&start, &tr("Working directory"), &draft.directory, "mono");
    fact(
        &start,
        &tr("Permissions"),
        &tr(model::permission_label(
            draft.permission.as_deref().unwrap_or(""),
        )),
        "value",
    );
    let estimate = if is_claude {
        format!(
            "~{} {}",
            model::k_tokens(data.estimate(draft, claude)),
            tr("tokens")
        )
    } else {
        tr("Not estimated")
    };
    fact(&start, &tr("Initial context"), &estimate, "value");

    let (_, first) = fold(parent, &tr("Reads first"), false, false);
    let mut files = Vec::new();
    if is_claude && draft.claude.instructions {
        if claude.instructions {
            files.push("CLAUDE.md".into());
        }
        files.extend(
            claude
                .rules
                .iter()
                .filter(|name| !draft.claude.rules_off.contains(name))
                .map(|name| format!("rules/{name}")),
        );
    } else if !is_claude && draft.codex.instructions && codex.instructions {
        files.push("AGENTS.md".into());
    }
    for (i, file) in files.iter().enumerate() {
        fact(&first, &(i + 1).to_string(), file, "mono");
    }
    fact(
        &first,
        &(files.len() + 1).to_string(),
        &tr(match draft.project {
            arsenal::role_instructions::Choice::Load => t!("Project instructions: loaded"),
            arsenal::role_instructions::Choice::Exclude { .. } => {
                t!("Project instructions: skipped")
            }
            arsenal::role_instructions::Choice::Role { .. } => {
                t!("Project instructions: replaced by custom text")
            }
        }),
        "value",
    );

    let (_, skills) = fold(parent, &tr("Skills"), false, false);
    let mut groups = BTreeMap::<_, Vec<_>>::new();
    for item in data
        .library
        .iter()
        .filter(|item| item.provider == draft.agent && item.kind == "skill")
    {
        let (group, category) = super::categories::category(item);
        let names = groups.entry(group).or_default();
        if data.active(draft, &item.id) {
            names.push((category, item.name.as_str()));
        }
    }
    if is_claude && !claude.account_skills.is_empty() {
        let account = groups.entry(t!("Environment")).or_default();
        account.extend(
            claude
                .account_skills
                .iter()
                .filter(|name| !draft.claude.account_skills_off.contains(name))
                .map(|name| (t!("Claude account"), name.as_str())),
        );
    }
    for (group, mut names) in model::ordered_skill_groups(groups) {
        names.sort_by_key(|(category, name)| {
            (model::skill_category_rank(category), name.to_lowercase())
        });
        fact(
            &skills,
            &tr(group),
            &names
                .into_iter()
                .map(|(_, name)| name)
                .collect::<Vec<_>>()
                .join(", "),
            "value",
        );
    }

    if is_claude {
        let (_, mcp) = fold(parent, "MCP", false, false);
        let servers = claude
            .mcp
            .iter()
            .filter(|name| draft.claude.mcp_enabled(name))
            .map(String::as_str)
            .collect::<Vec<_>>();
        fact(&mcp, &tr("Your servers"), &servers.join(", "), "value");
        fact(
            &mcp,
            &tr("Account connectors"),
            &tr(if draft.claude.mcp.is_none() {
                t!("On")
            } else {
                t!("Off")
            }),
            "value",
        );
    }
    let (_, hooks) = fold(parent, &tr("Hooks"), false, false);
    let mut entries = if is_claude {
        &claude.hooks
    } else {
        &codex.hooks
    }
    .iter()
    .filter(|hook| !draft.hook_exclusions().contains(&hook.key))
    .cloned()
    .collect::<Vec<_>>();
    if !is_claude {
        entries.extend(model::fixed_hooks(codex));
    }
    if entries.is_empty() {
        fact(&hooks, "—", &tr("None"), "mono");
    }
    for (event, rows) in model::hooks(&entries) {
        let names = rows
            .iter()
            .map(|(script, hooks)| format!("{script} ({})", hook_matchers(hooks)))
            .collect::<Vec<_>>()
            .join(", ");
        fact(&hooks, &tr(model::event_label(&event)), &names, "mono");
    }
}

fn permission_help(advice: &gtk::Label, id: &str) {
    let key = match id {
        "ask" => t!("Asks for approval before every action."),
        "read_only" => t!("Does not change files or run commands."),
        "edits" => t!("Edits without asking; asks before running commands."),
        "bypass" => t!("No approvals and no sandbox, with your account’s permissions."),
        _ => t!("Uses what the application has configured."),
    };
    advice.set_text(&tr(key));
    if id == "bypass" {
        advice.add_css_class("warning");
    } else {
        advice.remove_css_class("warning");
    }
}

fn count_text(on: usize, total: usize) -> String {
    trf(
        "{on} of {all}",
        &[("on", &on.to_string()), ("all", &total.to_string())],
    )
}

fn project_help(mode: &str) -> String {
    tr(match mode {
        "1" => t!(
            "The agent does not read the project’s instructions or rules; yours follow their switches."
        ),
        "2" => t!("The agent reads the text below instead of the project’s instructions."),
        _ => t!("The agent reads the project’s instructions as well as yours."),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};

    fn saved_launch() -> Launch {
        Launch {
            id: "a".into(),
            name: "Launch A".into(),
            directory: "/fixture".into(),
            kind: Kind::Commands {
                commands: vec![],
                variants: vec![],
                active: None,
                template: None,
            },
        }
    }

    #[test]
    fn final_review_close_is_deferred_until_write_completes() {
        let state = WriteState::default();
        assert_eq!(state.request_close(), glib::Propagation::Proceed);
        assert!(state.start());
        assert_eq!(state.request_close(), glib::Propagation::Stop);
        assert!(!state.start());
        assert_eq!(state.request_close(), glib::Propagation::Stop);
        assert!(state.finish(), "the pending close must be replayed");
        assert_eq!(state.request_close(), glib::Propagation::Proceed);
        assert!(state.start());
        assert!(!state.finish(), "a save alone must not close the window");
    }

    #[test]
    fn final_review_editor_stays_disabled_during_post_save_reload() {
        assert!(Pending::editor_enabled(&None, false));
        assert!(!Pending::editor_enabled(&None, true));
        let mut pending = Some(Pending::Saved(saved_launch()));
        assert!(!Pending::editor_enabled(&pending, false));
        pending.take();
        assert!(Pending::editor_enabled(&pending, false));
    }

    #[test]
    fn final_review_reload_preserves_queued_open() {
        for reload in [None, Some(Pending::Saved(saved_launch()))] {
            let mut pending = Some(Pending::Open("b".into()));
            Pending::queue_reload(&mut pending, reload);
            assert!(matches!(&pending, Some(Pending::Open(id)) if id == "b"));
            Pending::queue_reload(&mut pending, Some(Pending::Open("c".into())));
            assert!(matches!(&pending, Some(Pending::Open(id)) if id == "c"));
        }
    }

    #[test]
    fn workspace_listing_does_not_load_skill_availability() {
        let root = crate::personalization_scratch()
            .join(format!("workspace-cache-{}", uuid::Uuid::new_v4()));
        let fixture = crate::PersonalizationFixture(root);
        let root = &fixture.0;
        let home = root.join("home");
        let bin = root.join("bin");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir(&bin).unwrap();
        for agent in wyss_console::native::catalogue::AGENTS {
            let program = bin.join(agent.executable);
            fs::write(
                &program,
                "#!/bin/sh\nprintf x >> \"$HOME/probe-count\"\nprintf '1.0\\n'\n",
            )
            .unwrap();
            fs::set_permissions(program, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let release = root.join("os-release");
        fs::write(&release, "ID=cachyos\nNAME=CachyOS\n").unwrap();
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "workspace::view::tests::cache_worker_child",
                "--nocapture",
            ])
            .env("WYSS_WORKSPACE_CACHE_CHILD", root)
            .env("HOME", &home)
            .env("PATH", &bin)
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("XDG_STATE_HOME", root.join("state"))
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_HOME")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&result.stdout);
        assert!(
            result.status.success() && stdout.contains("1 passed"),
            "{stdout}\n{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            !home.join("probe-count").exists(),
            "loading the Workspace catalog probed a provider"
        );
    }

    #[test]
    fn skills_results_from_previous_editor_are_discarded() {
        assert_eq!(current_skill_result(4, 4, 1, 1, "current"), Some("current"));
        assert_eq!(current_skill_result(4, 4, 2, 1, "superseded"), None);
        assert_eq!(current_skill_result(5, 4, 1, 1, "previous editor"), None);
    }

    #[test]
    fn cache_worker_child() {
        let Some(root) = std::env::var_os("WYSS_WORKSPACE_CACHE_CHILD").map(PathBuf::from) else {
            return;
        };
        let mut places = Places::from_env().unwrap();
        assert_eq!(places.home, root.join("home"));
        places.os_release = root.join("os-release");
        launch::list_for_editor(&places).unwrap();
        fs::write(
            places.folder.join("fixture.toml"),
            "name = \"Fixture Agent\"\ndirectory = \"~\"\n[session]\nagent = \"codex\"\n",
        )
        .unwrap();
        let caller = std::thread::current().id();
        let cache = glib::MainContext::new()
            .block_on(load_cache(
                Arc::new(places),
                Arc::default(),
                move |places| {
                    assert_ne!(
                        std::thread::current().id(),
                        caller,
                        "loaded on the caller's thread"
                    );
                    let cache = Cache::load(places);
                    assert_eq!(SKILL_LOADS.get(), 0, "list reload requested availability");
                    assert!(
                        cache.details.is_none(),
                        "list reload waited for the catalog"
                    );
                    let entries = &cache.listing.as_ref().unwrap().entries.launches;
                    let details = Details::load(places, entries);
                    assert!(details.agents["fixture"].is_ok());
                    assert_eq!(
                        SKILL_LOADS.get(),
                        0,
                        "draft preparation requested availability"
                    );
                    cache
                },
            ))
            .unwrap();
        let listing = cache.listing.unwrap();
        assert_eq!(listing.entries.system.id, "cachyos");
        assert_eq!(listing.entries.launches.len(), 4);
        assert!(
            listing
                .entries
                .launches
                .iter()
                .any(|launch| launch.id == "fixture")
        );
    }
}
