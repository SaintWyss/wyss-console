use std::cell::Cell;
use std::io::{self, Read};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::PathBuf;
use std::rc::Rc;

use crate::personalization::vertical;

use gtk::prelude::*;
use gtk::{gio, glib};
use wyss_console::settings::{
    AppearanceTheme, CursorStyle, FONT_SIZE, Language, Preferences, SettingsFailure,
    SettingsService, SettingsView, Storage,
};

use crate::host::Host;
use crate::i18n::{Lang, lang, tr, trf};
use crate::{t, theme::Theme};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Landing {
    #[default]
    Console,
    Workspace,
    Personalization,
}

impl Landing {
    const ALL: [Self; 3] = [Self::Console, Self::Workspace, Self::Personalization];

    pub fn name(self) -> &'static str {
        match self {
            Self::Console => "console",
            Self::Workspace => "workspace",
            Self::Personalization => "personalization",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Console => t!("Console"),
            Self::Workspace => t!("Workspace"),
            Self::Personalization => t!("Personalization"),
        }
    }
}

// The landing page file lives in the `gtk-term` state directory beside the layout.
fn landing_path() -> Option<PathBuf> {
    let path = crate::layout::state_path()?;
    path.is_absolute()
        .then(|| path.with_file_name("landing-page"))
}

fn load_landing() -> Landing {
    let read = || {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(landing_path()?)
            .ok()?;
        if !file.metadata().ok()?.is_file() {
            return None;
        }
        let mut value = String::new();
        file.take(32).read_to_string(&mut value).ok()?;
        Landing::ALL.into_iter().find(|page| page.name() == value)
    };
    read().unwrap_or_default()
}

fn save_landing(page: Landing) -> Result<(), SettingsFailure> {
    let save = || -> io::Result<()> {
        let path = landing_path().ok_or_else(|| io::Error::other("no state path"))?;
        let dir = path.parent().expect("landing path has a parent");
        if page != Landing::Console {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
        }
        let meta = match std::fs::symlink_metadata(dir) {
            Err(error) if error.kind() == io::ErrorKind::NotFound && page == Landing::Console => {
                return Ok(());
            }
            result => result?,
        };
        if !meta.is_dir()
            || meta.uid() != nix::unistd::geteuid().as_raw()
            || meta.mode() & 0o077 != 0
        {
            return Err(io::Error::other("non-private state directory"));
        }
        if page == Landing::Console {
            return match std::fs::remove_file(path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                result => result,
            };
        }
        glib::file_set_contents_full(
            path,
            page.name().as_bytes(),
            glib::FileSetContentsFlags::CONSISTENT | glib::FileSetContentsFlags::DURABLE,
            0o600,
        )
        .map_err(io::Error::other)
    };
    save().map_err(|_| SettingsFailure::NotSaved)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Values {
    shared: Preferences,
    landing: Landing,
}

impl Values {
    fn new(shared: Preferences, storage: Storage) -> Self {
        let landing = if storage == Storage::Ready {
            load_landing()
        } else {
            Landing::Console
        };
        Self { shared, landing }
    }

    fn change(mut self, change: Change) -> Self {
        match change {
            Change::Theme(value) => self.shared.interface.theme = value,
            Change::Language(value) => self.shared.interface.language = value,
            Change::Landing(value) => self.landing = value,
            Change::Font(value) => self.shared.terminal.font_size = value,
            Change::History(value) => self.shared.terminal.scrollback = value,
            Change::Cursor(value) => self.shared.terminal.cursor_style = value,
            Change::Blink(value) => self.shared.terminal.cursor_blink = value,
            Change::Reset => {
                self = Self {
                    shared: Preferences::default(),
                    landing: Landing::Console,
                }
            }
        }
        self
    }
}

pub fn appearance(preferences: Preferences) -> Theme {
    match preferences.interface.theme {
        AppearanceTheme::Dark => Theme::Dark,
        AppearanceTheme::Light => Theme::Light,
        AppearanceTheme::HighContrast => Theme::HighContrast,
    }
}

pub fn language(preferences: Preferences) -> Lang {
    match preferences.interface.language {
        Language::En => Lang::En,
        Language::Es => Lang::Es,
    }
}

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug))]
enum Change {
    Theme(AppearanceTheme),
    Language(Language),
    Landing(Landing),
    Font(u8),
    History(u32),
    Cursor(CursorStyle),
    Blink(bool),
    Reset,
}

fn persist(
    service: &SettingsService,
    current: Values,
    change: Change,
) -> Result<SettingsView, SettingsFailure> {
    let next = current.change(change);
    match change {
        Change::Landing(_) => {
            let view = service.read();
            match view.storage {
                Storage::Newer => return Err(SettingsFailure::ReadOnly),
                Storage::Unavailable => return Err(SettingsFailure::NotSaved),
                _ => {}
            }
            save_landing(next.landing)?;
            if view.storage == Storage::Ready {
                return Ok(view);
            }
            service.update(current.shared).map_err(|error| {
                let _ = save_landing(current.landing);
                error.code()
            })
        }
        Change::Reset => {
            let previous = load_landing();
            save_landing(next.landing)?;
            service.reset().map_err(|error| {
                let _ = save_landing(previous);
                error.code()
            })
        }
        _ => service.update(next.shared).map_err(|error| error.code()),
    }
}

#[derive(Clone)]
pub(super) struct Snapshot {
    view: SettingsView,
    values: Values,
    saved: bool,
}

impl Snapshot {
    pub fn load(service: &SettingsService) -> Self {
        let view = service.read();
        let values = Values::new(view.preferences, view.storage);
        let saved = preferences_file_exists();
        Self { view, values, saved }
    }

    pub fn preferences(&self) -> Preferences {
        self.values.shared
    }
}

fn preferences_file_exists() -> bool {
    crate::prefs::path().is_some_and(|path| path.is_file())
}

fn status(storage: Storage, values: Values, saved: bool) -> &'static str {
    if storage == Storage::Ready
        && (values.landing != Landing::Console || saved)
    {
        t!("Saved on this computer")
    } else {
        t!("Defaults in use")
    }
}

fn notice(storage: Storage) -> Option<(&'static str, &'static str)> {
    Some(match storage {
        Storage::Ready => return None,
        Storage::Recovered => (
            t!("Your saved preferences could not be read"),
            t!(
                "They were set aside and the defaults are in use. Change a preference to save a new set, or reset to start clean."
            ),
        ),
        Storage::Newer => (
            t!("Saved by a newer version of Wyss Console"),
            t!(
                "These preferences were left as they are and the defaults are in use. They cannot be changed from this version until they are reset."
            ),
        ),
        Storage::Unsafe => (
            t!("Your saved preferences were not trusted"),
            t!(
                "The file is not private to you, so it was not read and the defaults are in use. Changing a preference replaces it."
            ),
        ),
        Storage::Unavailable => (
            t!("Preferences cannot be saved here"),
            t!(
                "The place they are kept in is not safely yours, so the defaults are in use and changes are not kept."
            ),
        ),
    })
}

fn failure_text(error: SettingsFailure) -> &'static str {
    match error {
        SettingsFailure::Invalid => t!("That value is not allowed."),
        SettingsFailure::ReadOnly => t!(
            "These preferences were saved by a newer version. Reset them to change them from this version."
        ),
        SettingsFailure::Internal | SettingsFailure::NotSaved => {
            t!("The preference could not be saved. Nothing was changed.")
        }
    }
}

fn label(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.add_css_class(class);
    label
}

fn card(grid: &gtk::Grid, column: i32, row: i32, title: &str) -> gtk::Box {
    let card = vertical(10);
    card.add_css_class("settings-card");
    card.set_valign(gtk::Align::Start);
    card.set_hexpand(true);
    card.append(&label(&tr(title), "settings-card-title"));
    grid.attach(&card, column, row, 1, 1);
    card
}

fn fields(card: &gtk::Box) -> gtk::Grid {
    let grid = gtk::Grid::builder()
        .column_spacing(12)
        .row_spacing(8)
        .build();
    card.append(&grid);
    grid
}

fn field(
    grid: &gtk::Grid,
    row: i32,
    name: &str,
    control: &(impl IsA<gtk::Widget> + IsA<gtk::Accessible>),
    hint: Option<&str>,
) {
    let caption = label(&tr(name), "settings-caption");
    caption.set_width_chars(17);
    caption.set_mnemonic_widget(Some(control));
    control.update_relation(&[gtk::accessible::Relation::LabelledBy(&[
        caption.upcast_ref()
    ])]);
    grid.attach(&caption, 0, row, 1, 1);
    control.set_halign(gtk::Align::Start);
    grid.attach(control, 1, row, 1, 1);
    if let Some(hint) = hint {
        let hint = label(hint, "settings-hint");
        hint.set_hexpand(true);
        control.update_relation(&[gtk::accessible::Relation::DescribedBy(&[hint.upcast_ref()])]);
        grid.attach(&hint, 1, row + 1, 1, 1);
    }
}

fn dropdown(texts: &[String]) -> gtk::DropDown {
    let words = texts.iter().map(String::as_str).collect::<Vec<_>>();
    let result = gtk::DropDown::from_strings(&words);
    result.set_size_request(176, -1);
    result
}

const THEMES: [AppearanceTheme; 3] = [
    AppearanceTheme::Dark,
    AppearanceTheme::Light,
    AppearanceTheme::HighContrast,
];
const LANGUAGES: [Language; 2] = [Language::En, Language::Es];
const CURSORS: [CursorStyle; 3] = [CursorStyle::Block, CursorStyle::Bar, CursorStyle::Underline];

struct Controls {
    theme: gtk::DropDown,
    language: gtk::DropDown,
    landing: gtk::DropDown,
    font: gtk::DropDown,
    history: gtk::DropDown,
    cursor: gtk::DropDown,
    blink: gtk::CheckButton,
    history_values: Vec<u32>,
    fonts: Vec<u8>,
}

impl Controls {
    fn show(&self, values: Values) {
        fn select<T: PartialEq>(dropdown: &gtk::DropDown, choices: &[T], value: T) {
            dropdown.set_selected(
                choices
                    .iter()
                    .position(|choice| *choice == value)
                    .expect("offered value") as u32,
            );
        }
        let p = values.shared;
        select(&self.theme, &THEMES, p.interface.theme);
        select(&self.language, &LANGUAGES, p.interface.language);
        select(&self.landing, &Landing::ALL, values.landing);
        select(&self.font, &self.fonts, p.terminal.font_size);
        select(&self.history, &self.history_values, p.terminal.scrollback);
        select(&self.cursor, &CURSORS, p.terminal.cursor_style);
        self.blink.set_active(p.terminal.cursor_blink);
    }
}

fn lines_label(lines: u32) -> String {
    let count = if lines < 1000 {
        lines.to_string()
    } else {
        let separator = if lang() == Lang::Es { "." } else { "," };
        format!("{}{separator}{:03}", lines / 1000, lines % 1000)
    };
    trf("{count} lines", &[("count", &count)])
}

struct Page {
    service: SettingsService,
    values: Cell<Values>,
    storage: Cell<Storage>,
    saved: Cell<bool>,
    saving: Cell<bool>,
    controls: Controls,
    editors: [gtk::Box; 3],
    status: gtk::Label,
    notice: gtk::Label,
    failure: gtk::Label,
    reset: gtk::Button,
    apply: Box<dyn Fn(Preferences)>,
}

impl Page {
    fn display(&self, storage: Storage, saved: bool) {
        self.storage.set(storage);
        self.saved.set(saved);
        self.controls.show(self.values.get());
        self.status
            .set_text(&tr(status(storage, self.values.get(), saved)));
        let message = notice(storage).map(|(title, text)| format!("{}. {}", tr(title), tr(text)));
        self.notice.set_visible(message.is_some());
        self.notice.set_text(message.as_deref().unwrap_or_default());
        if storage == Storage::Unavailable {
            self.notice.add_css_class("settings-error");
        } else {
            self.notice.remove_css_class("settings-error");
        }
        self.editable(storage != Storage::Newer, true);
    }

    fn editable(&self, edit: bool, reset: bool) {
        for card in &self.editors {
            card.set_sensitive(edit);
        }
        self.reset.set_sensitive(reset);
    }

    fn request(self: &Rc<Self>, change: Change) {
        if self.saving.replace(true) {
            return;
        }
        let current = self.values.get();
        let previous_storage = self.storage.get();
        let previously_saved = self.saved.get();
        let next = current.change(change);
        self.failure.set_visible(false);
        self.status.set_text(&tr("Saving…"));
        self.editable(false, false);
        let service = self.service.clone();
        let page = self.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                match persist(&service, current, change) {
                    Ok(view) => Ok((view, preferences_file_exists())),
                    Err(error) => Err((error, Snapshot::load(&service))),
                }
            }).await;
            match result {
                Ok(Ok((view, saved))) => {
                    page.values.set(Values {
                        shared: view.preferences,
                        landing: next.landing,
                    });
                    (page.apply)(view.preferences);
                    page.display(view.storage, saved);
                }
                Ok(Err((error, snapshot))) => {
                    page.values.set(snapshot.values);
                    page.display(snapshot.view.storage, snapshot.saved);
                    page.failure.set_text(&tr(failure_text(error)));
                    page.failure.set_visible(true);
                }
                Err(_) => {
                    page.display(previous_storage, previously_saved);
                    page.failure.set_text(&tr(failure_text(SettingsFailure::Internal)));
                    page.failure.set_visible(true);
                }
            }
            page.saving.set(false);
        });
    }
}

fn connect_choice<T: Copy + 'static>(
    control: &gtk::DropDown,
    page: &Rc<Page>,
    options: Vec<T>,
    change: fn(T) -> Change,
) {
    let page = Rc::downgrade(page);
    control.connect_selected_notify(move |control| {
        if let Some(page) = page.upgrade()
            && !page.saving.get()
            && let Some(&value) = options.get(control.selected() as usize)
        {
            page.request(change(value));
        }
    });
}

pub(super) fn build(
    window: &gtk::ApplicationWindow,
    service: SettingsService,
    snapshot: Snapshot,
    host: Option<Host>,
    apply: impl Fn(Preferences) + 'static,
) -> (gtk::Box, Landing) {
    let Snapshot { view, values, saved } = snapshot;
    let root = vertical(0);
    root.add_css_class("settings-view");
    let heading = vertical(3);
    heading.add_css_class("settings-heading");
    heading.append(&label(&tr("Settings"), "settings-title"));
    let status = label("", "settings-hint");
    heading.append(&status);
    root.append(&heading);

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    let body = vertical(14);
    body.add_css_class("settings-body");
    scroller.set_child(Some(&body));
    root.append(&scroller);

    let notice = label("", "settings-notice");
    body.append(&notice);
    let failure = label("", "settings-notice");
    failure.add_css_class("settings-error");
    failure.set_visible(false);
    body.append(&failure);
    let introduction = label(
        &tr(
            "Preferences change how Wyss Console looks and behaves. They never change what Codex may do: sessions start in Inference, Local Tools is chosen for each session, and every command or file change it proposes waits for your approval.",
        ),
        "settings-hint",
    );
    introduction.set_max_width_chars(83);
    introduction.set_halign(gtk::Align::Start);
    body.append(&introduction);

    let grid = gtk::Grid::builder()
        .column_homogeneous(true)
        .column_spacing(14)
        .row_spacing(14)
        .build();
    body.append(&grid);
    let appearance = card(&grid, 0, 0, t!("Appearance"));
    let interface = card(&grid, 1, 0, t!("Interface"));
    let terminal = card(&grid, 0, 1, t!("Terminal"));
    let about = card(&grid, 1, 1, t!("About"));
    let reset_card = card(&grid, 0, 2, t!("Reset"));
    let lifecycle = card(&grid, 1, 2, t!("Terminals after the window closes"));

    let theme = dropdown(&[tr("Dark"), tr("Light"), tr("High contrast")]);
    field(
        &fields(&appearance),
        0,
        t!("Color theme"),
        &theme,
        Some(&tr(
            "Changes appearance only. It does not change provider or approval permissions.",
        )),
    );

    let language = dropdown(&["English".into(), "Español".into()]);
    let landing = dropdown(&Landing::ALL.map(|page| tr(page.label())));
    let interface_fields = fields(&interface);
    let language_note = format!(
        "{} {}",
        tr(
            "The language of Wyss Console itself. What an agent, a provider or the system writes is shown as it arrives."
        ),
        tr("Reopen Wyss Console to apply the new language."),
    );
    field(
        &interface_fields,
        0,
        t!("Language"),
        &language,
        Some(&language_note),
    );
    field(
        &interface_fields,
        2,
        t!("Open on"),
        &landing,
        Some(&tr("The view shown when Wyss Console starts.")),
    );

    let fonts = FONT_SIZE.collect::<Vec<_>>();
    let font = dropdown(
        &fonts
            .iter()
            .map(|size| format!("{size} px"))
            .collect::<Vec<_>>(),
    );
    let mut history_values = vec![
        1000,
        2000,
        5000,
        10_000,
        20_000,
        50_000,
        values.shared.terminal.scrollback,
    ];
    history_values.sort_unstable();
    history_values.dedup();
    let history = dropdown(
        &history_values
            .iter()
            .copied()
            .map(lines_label)
            .collect::<Vec<_>>(),
    );
    let cursor = dropdown(&[tr("Block"), tr("Bar"), tr("Underline")]);
    let blink = gtk::CheckButton::new();
    let terminal_fields = fields(&terminal);
    field(&terminal_fields, 0, t!("Font size"), &font, None);
    field(
        &terminal_fields,
        1,
        t!("History kept"),
        &history,
        Some(&tr(
            "How far back each terminal can scroll. Larger values use more memory.",
        )),
    );
    field(&terminal_fields, 3, t!("Cursor"), &cursor, None);
    field(&terminal_fields, 4, t!("Blinking cursor"), &blink, None);
    terminal.append(&label(
        &tr(
            "These apply to every terminal, open or new. What a terminal runs is not a preference.",
        ),
        "settings-hint",
    ));

    let facts = fields(&about);
    let platform = format!("{} ({})", tr("Linux"), std::env::consts::ARCH);
    for (row, (name, value)) in [
        (
            "Wyss Console".into(),
            trf("Version {version}", &[("version", view.about.version)]),
        ),
        (
            tr("Build"),
            view.about
                .revision
                .map_or_else(|| tr("Not available"), str::to_owned),
        ),
        (tr("Platform"), platform),
    ]
    .into_iter()
    .enumerate()
    {
        facts.attach(&label(&name, "settings-hint"), 0, row as i32, 1, 1);
        let value = label(&value, "settings-value");
        value.set_selectable(true);
        facts.attach(&value, 1, row as i32, 1, 1);
    }
    let reset = gtk::Button::with_label(&tr("Reset preferences…"));
    reset.set_halign(gtk::Align::Start);
    reset_card.append(&reset);
    reset_card.append(&label(
        &tr("Restores the defaults on this screen. Nothing else is deleted."),
        "settings-hint",
    ));
    let lifecycle_input = Cell::new(Some(host));
    lifecycle.connect_map(move |card| {
        if let Some(host) = lifecycle_input.take() {
            build_lifecycle(card, host);
        }
    });

    let page = Rc::new(Page {
        service,
        values: Cell::new(values),
        storage: Cell::new(view.storage),
        saved: Cell::new(saved),
        saving: Cell::new(false),
        controls: Controls {
            theme,
            language,
            landing,
            font,
            history,
            cursor,
            blink,
            history_values,
            fonts,
        },
        editors: [appearance, interface, terminal],
        status,
        notice,
        failure,
        reset: reset.clone(),
        apply: Box::new(apply),
    });
    page.display(view.storage, saved);
    let c = &page.controls;
    connect_choice(&c.theme, &page, THEMES.to_vec(), Change::Theme);
    connect_choice(&c.language, &page, LANGUAGES.to_vec(), Change::Language);
    connect_choice(&c.landing, &page, Landing::ALL.to_vec(), Change::Landing);
    connect_choice(&c.font, &page, c.fonts.clone(), Change::Font);
    connect_choice(&c.history, &page, c.history_values.clone(), Change::History);
    connect_choice(&c.cursor, &page, CURSORS.to_vec(), Change::Cursor);
    c.blink.connect_toggled({
        let page = Rc::downgrade(&page);
        move |button| {
            if let Some(page) = page.upgrade()
                && !page.saving.get()
            {
                page.request(Change::Blink(button.is_active()));
            }
        }
    });
    // Keep the page alive at the root; control handlers hold only weak references.
    reset.connect_clicked({
        let page = Rc::downgrade(&page);
        let window = window.downgrade();
        move |_| {
            let (Some(window), Some(page)) = (window.upgrade(), page.upgrade()) else { return };
            let dialog = gtk::AlertDialog::builder()
                .modal(true)
                .message(tr("Reset all preferences to their defaults?"))
                .detail(tr("Your sessions, history, approvals, workspaces and Codex login are not touched."))
                .buttons([tr("Cancel"), tr("Reset preferences")])
                .default_button(0)
                .cancel_button(0)
                .build();
            dialog.choose(Some(&window), gio::Cancellable::NONE, move |answer| {
                if answer == Ok(1) {
                    page.request(Change::Reset);
                }
            });
        }
    });
    root.connect_destroy(move |_| {
        let _ = &page;
    });
    (root, values.landing)
}

fn build_lifecycle(card: &gtk::Box, host: Option<Host>) {
    use wyss_console::host::persistence::{Facts, Placement, Sources, read_facts, to_json};

    let note = label(&tr("Reading…"), "settings-hint");
    card.append(&note);
    let table = gtk::Grid::builder()
        .column_spacing(10)
        .row_spacing(0)
        .build();
    card.append(&table);
    let uses_host = host.is_some();
    glib::spawn_future_local(async move {
        let result = gio::spawn_blocking(move || {
            if let Some(host) = host {
                return host
                    .call_timeout(
                        "host_persistence",
                        serde_json::json!({}),
                        std::time::Duration::from_secs(2),
                    )
                    .ok();
            }
            let facts = Sources::system().as_ref().map_or(
                Facts {
                    linger: None,
                    kill_user_processes: false,
                    placement: Placement::Unknown,
                },
                read_facts,
            );
            Some(to_json(&facts))
        })
        .await
        .ok()
        .flatten();
        let Some(mut result) = result else {
            note.set_text(&tr("What survives what could not be read from the host."));
            return;
        };
        let Some(events) = result["events"].as_array_mut() else {
            note.set_text(&tr("What survives what could not be read from the host."));
            return;
        };
        if uses_host {
            note.set_text(&tr("Your terminals belong to a host process, not to this window. Closing the window does not stop them; this is what happens in the other cases on this machine."));
        } else {
            note.set_text(&tr(
                "This window owns the local terminals. Closing it closes their PTYs.",
            ));
            for row in events.iter_mut() {
                match row["id"].as_str().unwrap_or_default() {
                    "window_close" => {
                        row["outcome"] = "ends".into();
                        row["basis"] = t!(
                            "This window owns the local terminals. Closing it closes their PTYs."
                        )
                        .into();
                    }
                    "window_detach" | "host_crash" => row["id"] = serde_json::Value::Null,
                    _ => {}
                }
            }
        };
        for (column, title) in [tr("When"), tr("Terminals"), tr("Why")]
            .into_iter()
            .enumerate()
        {
            table.attach(
                &label(&title, "settings-table-heading"),
                column as i32,
                0,
                1,
                1,
            );
        }
        for (index, event) in events
            .iter()
            .filter(|event| event["id"].is_string())
            .enumerate()
        {
            let title = match event["id"].as_str().unwrap_or_default() {
                "window_close" => t!("Closing this window"),
                "window_detach" => t!("Leaving a terminal (detach)"),
                "screen_lock" => t!("Locking the screen"),
                "logout_with_lingering" => t!("Logging out, lingering on"),
                "logout_without_lingering" => t!("Logging out, lingering off"),
                "suspend_resume" => t!("Suspend and resume"),
                "host_crash" => t!("The host stopping or crashing"),
                "reboot_or_power_off" => t!("Reboot or power-off"),
                _ => continue,
            };
            let outcome = match event["outcome"].as_str() {
                Some("survives") => t!("Keeps running"),
                Some("ends") => t!("Ends"),
                _ => t!("Not certain here"),
            };
            let basis = event["basis"].as_str().unwrap_or_default();
            for (column, text) in [title, outcome, basis].into_iter().enumerate() {
                let cell = label(&tr(text), "settings-table-cell");
                cell.set_yalign(0.0);
                cell.set_hexpand(column == 2);
                cell.set_width_chars(if column == 2 { 15 } else { 11 });
                table.attach(&cell, column as i32, index as i32 + 1, 1, 1);
            }
        }
    });
}

// The host supplies these sentences. Mark them so the generator includes their dictionary entries.
const _: &[&str] = &[
    t!(
        "The host is a separate process with a group of its own; closing the window signals nothing in it."
    ),
    t!("A window that leaves detaches its terminals; the host closes none of them."),
    t!("Locking the screen keeps the session and changes no process."),
    t!("With lingering the user's manager keeps running after the last session ends."),
    t!(
        "Without lingering the user's manager stops with the last session, and what runs inside it stops too."
    ),
    t!(
        "Suspend freezes processes and resume continues them; programs that watch a clock or a network see the gap."
    ),
    t!(
        "The host owns every terminal's PTY: when it stops, they close. Nothing relaunches a command."
    ),
    t!(
        "No process survives a reboot or a loss of power, and nothing is started again by itself afterwards."
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use wyss_console::settings::{EventFilter, LandingPage};

    #[test]
    fn settings_round_trip_preserves_saved_landing_and_event_preferences() {
        let mut shared = Preferences::default();
        shared.interface.landing_page = LandingPage::Sessions;
        shared.events.default_filter = EventFilter::Security;
        let initial = Values {
            shared,
            landing: Landing::Console,
        };
        let updated = [
            Change::Theme(AppearanceTheme::HighContrast),
            Change::Language(Language::Es),
            Change::Landing(Landing::Workspace),
            Change::Font(24),
            Change::History(2000),
            Change::Cursor(CursorStyle::Underline),
            Change::Blink(true),
        ]
        .into_iter()
        .fold(initial, Values::change);
        let decoded: Preferences =
            serde_json::from_slice(&serde_json::to_vec(&updated.shared).unwrap()).unwrap();
        assert_eq!(decoded, updated.shared);
        assert_eq!(decoded.interface.landing_page, LandingPage::Sessions);
        assert_eq!(decoded.events, shared.events);
        assert_eq!(appearance(decoded), Theme::HighContrast);
        assert_eq!(language(decoded), Lang::Es);
        assert_eq!(updated.landing.name(), "workspace");
        assert_eq!(
            decoded.terminal,
            wyss_console::settings::Terminal {
                font_size: 24,
                scrollback: 2000,
                cursor_style: CursorStyle::Underline,
                cursor_blink: true,
            }
        );
        assert_eq!(
            updated.change(Change::Reset),
            Values {
                shared: Preferences::default(),
                landing: Landing::Console
            }
        );
    }

    #[test]
    fn round_trip_and_reset() {
        crate::personalization_round_trip("settings", "settings::tests::isolated_store_child");
    }

    #[test]
    fn isolated_store_child() {
        let Some(root) = crate::personalization_child_root("settings") else {
            return;
        };
        let config = root.join("config");
        assert_eq!(
            std::env::var_os("XDG_CONFIG_HOME").as_deref(),
            Some(config.as_os_str())
        );
        let service = SettingsService::from_environment();
        let view = service.read();
        let mut current = Values::new(view.preferences, view.storage);
        for change in [
            Change::Theme(AppearanceTheme::Light),
            Change::Language(Language::Es),
            Change::Landing(Landing::Personalization),
            Change::Font(22),
            Change::History(2000),
            Change::Cursor(CursorStyle::Bar),
            Change::Blink(true),
        ] {
            persist(&service, current, change).unwrap_or_else(|error| {
                let owners = config
                    .ancestors()
                    .filter_map(|path| {
                        std::fs::symlink_metadata(path)
                            .ok()
                            .map(|meta| (path, meta.uid()))
                    })
                    .collect::<Vec<_>>();
                panic!("{change:?}: {error:?}; config ancestry owners: {owners:?}");
            });
            current = current.change(change);
        }
        let file = config.join("wyss-console/preferences.json");
        let json: Preferences = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(json, current.shared);
        let reopened = SettingsService::from_environment().read();
        assert_eq!(Values::new(reopened.preferences, reopened.storage), current);
        let reset = persist(&service, current, Change::Reset).unwrap();
        assert_eq!(reset.preferences, Preferences::default());
        assert!(!file.exists());
        assert!(!landing_path().unwrap().exists());
        assert_eq!(
            SettingsService::from_environment().read().preferences,
            reset.preferences
        );
    }

    #[test]
    fn unavailable_storage_refuses_changes() {
        let service = SettingsService::open(None);
        let view = service.read();
        let current = Values {
            shared: view.preferences,
            landing: Landing::Console,
        };
        assert_eq!(view.storage, Storage::Unavailable);
        assert_eq!(
            persist(&service, current, Change::Font(20)),
            Err(SettingsFailure::NotSaved)
        );
        assert_eq!(
            persist(&service, current, Change::Landing(Landing::Workspace)),
            Err(SettingsFailure::NotSaved)
        );
        assert_eq!(service.read().preferences, current.shared);
    }
}
