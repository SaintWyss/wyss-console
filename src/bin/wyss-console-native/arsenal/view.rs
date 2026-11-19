use std::{
    rc::Rc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use gtk::{gio, glib, prelude::*};
use wyss_console::{
    agentconfig::guide::{self, Guide, GuideBody, GuideEntry},
    arsenal::{
        self,
        store::{Incoming, Selection},
        sync::{self, Status},
    },
    launch::{self, Launch},
};

use super::{
    config,
    library::{self, Decided, Library},
    model::{self, SyncMark, TOOLS},
};
use crate::{
    i18n::{self, Lang, tr, trf},
    personalization::{button, clear, horizontal, label, scroll, single_line, timing, vertical},
    t,
};

type EditLaunch = Rc<dyn Fn(&str)>;

struct Data {
    entries: Option<(Guide, guide::Places)>,
    selections: Result<Vec<Selection>, arsenal::store::Error>,
    launches: Option<Vec<Launch>>,
    sync: Status,
    incoming: Vec<Incoming>,
    library: Option<Library>,
    config: Vec<config::Row>,
}

impl Data {
    fn load() -> Self {
        let started = Instant::now();
        let store = arsenal::open_read_only();
        let library = Library::scan(store.as_ref().ok());
        // A library not yet at the version that holds received changes holds none.
        let incoming = store
            .as_ref()
            .ok()
            .and_then(|store| sync::incoming(store).ok())
            .unwrap_or_default();
        let data = Self {
            library,
            config: config::Row::load(),
            entries: guide::scan_process(),
            selections: store.and_then(|store| store.selections()),
            launches: launch::Places::from_env()
                .and_then(|places| launch::list_for_editor(&places).ok())
                .map(|list| list.launches),
            sync: read_sync(),
            incoming,
        };
        timing("ars:load", started);
        data
    }
}

#[derive(Clone)]
struct Browser {
    data: Rc<Data>,
    right: glib::WeakRef<gtk::Box>,
    root: glib::WeakRef<gtk::Box>,
    busy: Rc<std::cell::Cell<bool>>,
    selected: Rc<glib::WeakRef<gtk::Button>>,
    edit: EditLaunch,
    create: EditLaunch,
    page: Rc<std::cell::Cell<u64>>,
}

impl Browser {
    fn replace(&self) -> Option<gtk::Box> {
        let right = self.right.upgrade()?;
        self.page.set(self.page.get().wrapping_add(1));
        clear(&right);
        Some(right)
    }

    fn show_entries(&self, browse: Browse) {
        let start = Instant::now();
        let Some(right) = self.replace() else { return };
        let Some((guide, _)) = &self.data.entries else {
            right.append(&label(
                &tr("The skills on disk could not be read."),
                "notice",
            ));
            return;
        };
        if guide.truncated {
            right.append(&label(
                &tr("A folder holds more than is read: the lists are incomplete."),
                "notice",
            ));
        }
        let mut entries: Vec<_> = guide
            .entries
            .iter()
            .enumerate()
            .filter(|(_, item)| browse.includes(item))
            .collect();
        entries.sort_by_cached_key(|(_, item)| item.name.to_lowercase());
        heading(&right, &browse.title(), entries.len());
        let table = vertical(0);
        for (index, item) in entries {
            let control = gtk::Button::builder()
                .child(&table_row(
                    &item.name,
                    model::first_sentence(description(item)),
                ))
                .build();
            control.add_css_class("flat");
            control.add_css_class("table-row");
            let (browser, browse) = (self.clone(), browse.clone());
            control.connect_clicked(move |_| browser.detail(index, browse.clone()));
            table.append(&control);
        }
        right.append(&table);
        timing("ars:list", start);
    }

    fn detail(&self, index: usize, browse: Browse) {
        let start = Instant::now();
        let Some(right) = self.replace() else { return };
        let Some((guide, places)) = &self.data.entries else {
            return;
        };
        let item = &guide.entries[index];
        let head = horizontal(8);
        let name = label(&item.name, "title");
        name.set_hexpand(true);
        head.append(&name);
        let back = button(t!("Back"), "action");
        let browser = self.clone();
        back.connect_clicked(move |_| browser.show_entries(browse.clone()));
        head.append(&back);
        right.append(&head);
        right.append(&label(description(item), "hint"));
        if item.description_cut {
            right.append(&label(
                &tr("The description is longer: only its beginning is shown."),
                "hint",
            ));
        }
        let body = vertical(6);
        body.append(&label(&tr("Reading…"), "hint"));
        right.append(&body);
        let location = gtk::Expander::new(Some(&tr("Location")));
        let location_data = vertical(6);
        location_data.append(&label(&item.origin, "source"));
        if let Some(target) = &item.linked_to {
            location_data.append(&label(target, "source"));
        }
        location.set_child(Some(&location_data));
        right.append(&location);
        let (id, places, page, expected) = (
            item.id.clone(),
            places.clone(),
            self.page.clone(),
            self.page.get(),
        );
        let (body, location_data) = (body.downgrade(), location_data.downgrade());
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || guide::body_process(&places, &id))
                .await
                .ok()
                .flatten();
            if page.get() != expected {
                return;
            }
            if let (Some(body), Some(location)) = (body.upgrade(), location_data.upgrade()) {
                show_body(&body, &location, result);
            }
        });
        timing("ars:detail", start);
    }

    fn refresh_presets(
        &self,
        provider: &'static str,
        work: impl FnOnce() -> Result<(), arsenal::store::Error> + Send + 'static,
        failure: &'static str,
    ) {
        if self.busy.replace(true) {
            return;
        }
        let browser = self.clone();
        let expected = self.page.get();
        let content = self.root.upgrade().map(|right| {
            right.set_sensitive(false);
            right.downgrade()
        });
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let error = work().is_err();
                (Data::load(), error)
            })
            .await;
            browser.busy.set(false);
            if let Some(right) = content.and_then(|content| content.upgrade()) {
                right.set_sensitive(true);
            }
            if let Some(root) = browser.root.upgrade() {
                match result {
                    Ok((data, failed)) => {
                        let screen = render(
                            &root,
                            data,
                            browser.edit.clone(),
                            browser.create.clone(),
                            Some(provider),
                        );
                        if failed {
                            screen.prepend(&label(&tr(failure), "notice"));
                        }
                    }
                    Err(_) if browser.page.get() == expected => {
                        if let Some(right) = browser.right.upgrade() {
                            right.prepend(&label(&tr(failure), "notice"));
                        }
                    }
                    Err(_) => {}
                }
            }
        });
    }

    fn presets(&self, provider: &'static str, tool_label: &str) {
        let started = Instant::now();
        let Some(right) = self.replace() else { return };
        right.append(&label(
            &format!("{} · {tool_label}", tr("Presets")),
            "heading",
        ));
        let bar = horizontal(6);
        let status = match &self.data.library {
            Some(lib) => trf(
                "Library: {imported} imported · {new} new on disk",
                &[
                    ("imported", &lib.imported.to_string()),
                    ("new", &lib.eligible.len().to_string()),
                ],
            ),
            None => tr("New capabilities could not be read."),
        };
        let caption = label(&status, "hint");
        caption.set_hexpand(true);
        bar.append(&caption);
        let scan = button(t!("Scan again"), "action");
        let browser = self.clone();
        scan.connect_clicked(move |_| {
            browser.refresh_presets(
                provider,
                || Ok(()),
                t!("New capabilities could not be read."),
            )
        });
        bar.append(&scan);
        if let Some(lib) = self
            .data
            .library
            .as_ref()
            .filter(|lib| !lib.eligible.is_empty())
        {
            let control = gtk::Button::with_label(&trf(
                "Import the {count} new capabilities",
                &[("count", &lib.eligible.len().to_string())],
            ));
            let browser = self.clone();
            control.connect_clicked(move |_| {
                let Some(lib) = &browser.data.library else {
                    return;
                };
                let ids = lib.eligible.clone();
                browser.refresh_presets(
                    provider,
                    move || library::import(&ids),
                    t!("It could not be saved."),
                );
            });
            bar.append(&control);
        }
        right.append(&bar);
        let Ok(selections) = &self.data.selections else {
            right.append(&label(&tr("Presets could not be read."), "notice"));
            return;
        };
        let mut found = false;
        for selection in selections.iter().filter(|s| s.provider == provider) {
            found = true;
            let row = horizontal(8);
            row.add_css_class("preset-row");
            let copy = vertical(4);
            copy.set_hexpand(true);
            let duplicate = model::duplicate(selection, selections);
            let name = if duplicate {
                format!("{} · {}", selection.name, tr("Duplicate name"))
            } else {
                selection.name.clone()
            };
            copy.append(&label(&name, "title"));
            copy.append(&label(
                &trf(
                    model::item_count_key(selection.items.len()),
                    &[("count", &selection.items.len().to_string())],
                ),
                "hint",
            ));
            row.append(&copy);
            let actions = horizontal(6);
            actions.set_valign(gtk::Align::Start);
            let uses = self
                .data
                .launches
                .as_ref()
                .map(|items| model::launches_for(selection, items));
            match &uses {
                Some(launches) if launches.is_empty() => {
                    copy.append(&label(&tr("No launch uses this preset."), "hint"));
                    let create = button(t!("Create launch with it"), "action");
                    create.set_sensitive(!duplicate);
                    if duplicate {
                        create.set_tooltip_text(Some(&tr(
                            "Remove duplicate preset names before creating a launch.",
                        )));
                    }
                    let (open, id) = (self.create.clone(), selection.id.clone());
                    create.connect_clicked(move |_| open(&id));
                    actions.append(&create);
                }
                Some(launches) => {
                    for launch in launches {
                        let row = horizontal(8);
                        row.append(&label(&launch.name, "hint"));
                        let edit = button(t!("Edit"), "compact-action");
                        edit.update_property(&[gtk::accessible::Property::Label(&trf(
                            "Edit launch {name} in Workspace",
                            &[("name", &launch.name)],
                        ))]);
                        let (open, id) = (self.edit.clone(), launch.id.clone());
                        edit.connect_clicked(move |_| open(&id));
                        row.append(&edit);
                        copy.append(&row);
                    }
                }
                None => copy.append(&label(&tr("Workspace could not be read."), "notice")),
            }
            let delete = button(t!("Delete"), "action");
            delete.set_sensitive(uses.is_some());
            let confirmation = match uses {
                Some(used) if !used.is_empty() => trf(
                    "Deleting this preset stops {launches} from starting. The deletion syncs to the other computer; its launches are not visible here.",
                    &[(
                        "launches",
                        &used
                            .iter()
                            .map(|launch| launch.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                    )],
                ),
                _ => tr(
                    "No launch on this computer uses this preset. Deleting it syncs to the other computer; its launches are not visible here.",
                ),
            };
            let (browser, id) = (self.clone(), selection.id.clone());
            delete.connect_clicked(move |button| {
                let parent = button.root().and_downcast::<gtk::Window>();
                let dialog = gtk::AlertDialog::builder()
                    .message(tr("Confirm delete"))
                    .detail(&confirmation)
                    .buttons([tr("Cancel"), tr("Confirm delete")])
                    .cancel_button(0)
                    .default_button(0)
                    .modal(true)
                    .build();
                let (browser, id) = (browser.clone(), id.clone());
                dialog.choose(parent.as_ref(), gio::Cancellable::NONE, move |result| {
                    if matches!(result, Ok(1)) {
                        browser.refresh_presets(
                            provider,
                            move || library::delete(&id),
                            t!("It could not be deleted."),
                        );
                    }
                });
            });
            actions.append(&delete);
            row.append(&actions);
            right.append(&row);
        }
        if !found {
            right.append(&label(&tr("No presets for this tool."), "hint"));
        }
        timing("ars:presets", started);
    }

    fn show_config(&self, title: &str, indices: &[usize]) {
        let started = Instant::now();
        let Some(right) = self.replace() else { return };
        heading(&right, title, indices.len());
        let table = vertical(0);
        for index in indices {
            let item = &self.data.config[*index];
            let row = table_row(&item.name, &item.value);
            row.add_css_class("table-row");
            table.append(&row);
        }
        right.append(&table);
        timing("ars:configuration", started);
    }
}

fn heading(parent: &gtk::Box, title: &str, count: usize) {
    let row = horizontal(8);
    row.append(&label(title, "heading"));
    row.append(&label(&count.to_string(), "hint"));
    parent.append(&row);
}

fn table_row(name: &str, description: &str) -> gtk::Box {
    let row = horizontal(12);
    let name = single_line(name, "table-name");
    name.set_width_chars(26);
    name.set_max_width_chars(26);
    row.append(&name);
    let description = single_line(description, "hint");
    description.set_hexpand(true);
    row.append(&description);
    row
}

#[derive(Clone)]
enum Browse {
    Search(String),
    Group {
        tool: &'static str,
        set: String,
        category: &'static str,
    },
}

impl Browse {
    fn title(&self) -> String {
        match self {
            Self::Search(_) => tr("Search"),
            Self::Group { category, .. } => tr(category),
        }
    }

    fn includes(&self, entry: &GuideEntry) -> bool {
        match self {
            Self::Search(query) => model::matches(entry, query),
            Self::Group {
                tool,
                set,
                category,
            } => entry.tool == *tool && entry.set == *set && model::category(entry) == *category,
        }
    }
}

fn description(entry: &GuideEntry) -> &str {
    if entry.unreadable.is_some() {
        return i18n::translate(i18n::lang(), t!("Its text could not be read."));
    }
    let text = if i18n::lang() == Lang::Es {
        entry
            .description_es
            .as_deref()
            .unwrap_or(&entry.description)
    } else {
        &entry.description
    };
    if text.is_empty() {
        i18n::translate(i18n::lang(), t!("Its file has no description."))
    } else {
        text
    }
}

fn show_body(parent: &gtk::Box, location: &gtk::Box, body: Option<GuideBody>) {
    clear(parent);
    let Some(body) = body else {
        parent.append(&label(&tr("The file could not be read."), "notice"));
        return;
    };
    parent.append(&label(&tr("Instructions"), "title"));
    let copy = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    copy.buffer().set_text(&body.text);
    copy.add_css_class("source");
    let language = label(&tr("Original text: what the tool reads."), "hint");
    if let Some(spanish) = body.text_es {
        let toggle = gtk::ToggleButton::with_label(&tr("See translation"));
        let (copy, language) = (copy.downgrade(), language.downgrade());
        let original = body.text;
        toggle.connect_toggled(move |toggle| {
            if let (Some(copy), Some(language)) = (copy.upgrade(), language.upgrade()) {
                let translated = toggle.is_active();
                copy.buffer()
                    .set_text(if translated { &spanish } else { &original });
                toggle.set_label(&if translated {
                    tr("See original")
                } else {
                    tr("See translation")
                });
                language.set_label(&if translated {
                    tr("Translated text; the tool reads the original.")
                } else {
                    tr("Original text: what the tool reads.")
                });
            }
        });
        parent.append(&toggle);
    }
    parent.append(&language);
    let scroll = scroll(&copy);
    scroll.set_min_content_height(260);
    parent.append(&scroll);
    if body.cut {
        parent.append(&label(&tr("Only the first 256 KB are shown."), "hint"));
    }
    if !body.files.is_empty() {
        let files = gtk::Expander::new(Some(&trf(
            "Files ({n})",
            &[("n", &(body.files.len() + body.more_files).to_string())],
        )));
        let items = vertical(4);
        for file in body.files {
            items.append(&label(&file, "source"));
        }
        if body.more_files > 0 {
            items.append(&label(
                &trf("and {n} more", &[("n", &body.more_files.to_string())]),
                "hint",
            ));
        }
        if body.files_cut {
            items.append(&label(
                &tr("The folder holds more files than are counted here."),
                "hint",
            ));
        }
        files.set_child(Some(&items));
        location.append(&files);
    }
}

fn counted(title: &str, count: usize) -> gtk::Box {
    let row = horizontal(4);
    row.set_hexpand(true);
    let title = single_line(title, "");
    title.set_hexpand(true);
    row.append(&title);
    row.append(&label(&count.to_string(), "hint"));
    row
}

fn branch(parent: &gtk::Box, title: &str, count: usize, open: bool) -> gtk::Box {
    let control = gtk::Expander::builder()
        .label_widget(&counted(title, count))
        .expanded(open)
        .build();
    let children = vertical(0);
    children.add_css_class("tree-children");
    control.set_child(Some(&children));
    parent.append(&control);
    children
}

fn leaf(
    parent: &gtk::Box,
    title: &str,
    count: usize,
    browser: &Browser,
    action: impl Fn(&Browser) + 'static,
) -> gtk::Button {
    let control = gtk::Button::builder().child(&counted(title, count)).build();
    control.add_css_class("flat");
    control.add_css_class("tree-leaf");
    let browser = browser.clone();
    control.connect_clicked(move |control| {
        if let Some(previous) = browser.selected.upgrade() {
            previous.remove_css_class("selected");
        }
        browser.selected.set(Some(control));
        control.add_css_class("selected");
        action(&browser);
    });
    parent.append(&control);
    control
}

fn categories(
    parent: &gtk::Box,
    browser: &Browser,
    counts: &std::collections::BTreeMap<&'static str, usize>,
    tool: &'static str,
    set: &str,
) -> Option<(Browse, gtk::Button)> {
    let mut first = None;
    for category in model::CATEGORIES.into_iter().chain([t!("Other")]) {
        let Some(count) = counts.get(category) else {
            continue;
        };
        let browse = Browse::Group {
            tool,
            set: set.to_owned(),
            category,
        };
        let target = browse.clone();
        let button = leaf(parent, &tr(category), *count, browser, move |browser| {
            browser.show_entries(target.clone());
        });
        first.get_or_insert((browse, button));
    }
    first
}

fn tree(parent: &gtk::Box, browser: &Browser, preset: Option<&str>) -> Browse {
    let mut initial = None;
    let mut preset_button = None;
    for (provider, title, tool, config_provider) in TOOLS {
        let guide = browser.data.entries.as_ref().map(|(guide, _)| guide);
        let entries = || {
            guide
                .into_iter()
                .flat_map(|guide| &guide.entries)
                .filter(|item| item.tool == provider)
        };
        let presets = browser.data.selections.as_ref().map_or(0, |all| {
            all.iter().filter(|item| item.provider == provider).count()
        });
        let config_items = browser
            .data
            .config
            .iter()
            .enumerate()
            .filter(|(_, item)| item.provider == config_provider);
        let mut events: Vec<(&str, Vec<usize>)> = Vec::new();
        let mut global = Vec::new();
        let mut mcp = Vec::new();
        for (index, item) in config_items {
            match (&item.event, item.kind.as_str()) {
                (Some(event), _) => {
                    if let Some((_, rows)) = events.iter_mut().find(|(name, _)| *name == event) {
                        rows.push(index);
                    } else {
                        events.push((event, vec![index]));
                    }
                }
                (_, "mcp_server") => mcp.push(index),
                _ => global.push(index),
            }
        }
        let hook_count = events.iter().map(|(_, rows)| rows.len()).sum::<usize>();
        let personal = model::category_counts(entries(), provider, "mine");
        let agents = model::category_counts(entries(), provider, "agents");
        let mut sets: Vec<_> = guide
            .into_iter()
            .flat_map(|guide| &guide.sets)
            .filter(|set| set.tool == provider)
            .map(|set| (set, model::category_counts(entries(), provider, &set.key)))
            .filter(|(_, counts)| !counts.is_empty())
            .collect();
        sets.sort_by_cached_key(|(set, _)| set.name.to_lowercase());
        let skill_count = personal.values().sum::<usize>()
            + sets
                .iter()
                .flat_map(|(_, counts)| counts.values())
                .sum::<usize>();
        let counts = model::ProviderCounts {
            skills: skill_count,
            agents: agents.values().sum(),
            presets,
            hooks: hook_count,
            configuration: global.len(),
            mcp: mcp.len(),
        };
        let open = preset.map_or(initial.is_none() && !personal.is_empty(), |value| {
            value == provider
        });
        let kinds = branch(parent, title, counts.total(), open);
        if guide.is_some() {
            if counts.skills > 0 {
                let groups = branch(&kinds, &tr("Skills"), counts.skills, open);
                if !personal.is_empty() {
                    let children = branch(&groups, &tr("Personal"), personal.values().sum(), open);
                    let first = categories(&children, browser, &personal, provider, "mine");
                    if initial.is_none() {
                        initial = first;
                    }
                }
                for (set, counts) in sets {
                    let children = branch(&groups, &set.name, counts.values().sum(), false);
                    categories(&children, browser, &counts, provider, &set.key);
                }
            }
            if !agents.is_empty() {
                let children = branch(&kinds, &tr("Agents"), counts.agents, false);
                categories(&children, browser, &agents, provider, "agents");
            }
        }
        if provider != "opencode" {
            let button = leaf(
                &kinds,
                &tr("Presets"),
                counts.presets,
                browser,
                move |browser| {
                    browser.presets(provider, tool);
                },
            );
            if preset == Some(provider) {
                preset_button = Some(button);
            }
            let hooks = branch(&kinds, &tr("Hooks"), counts.hooks, false);
            for (event, rows) in events {
                let title = tr(config::event_title(event));
                leaf(&hooks, &title, rows.len(), browser, {
                    let title = title.clone();
                    move |browser| browser.show_config(&title, &rows)
                });
            }
        }
        for (key, rows, count) in [
            (t!("MCP servers"), mcp, counts.mcp),
            (t!("Configuration"), global, counts.configuration),
        ] {
            if count != 0 || provider == "opencode" {
                leaf(&kinds, &tr(key), count, browser, move |browser| {
                    browser.show_config(&tr(key), &rows)
                });
            }
        }
    }
    if let Some(button) = preset_button {
        button.emit_clicked();
    }
    if let Some((browse, button)) = initial {
        if preset.is_none() {
            button.emit_clicked();
        }
        browse
    } else {
        let browse = Browse::Search(String::new());
        if preset.is_none() {
            browser.show_entries(browse.clone());
        }
        browse
    }
}

fn ago(at: i64, now: i64) -> String {
    let seconds = now.saturating_sub(at);
    match seconds {
        ..=59 => tr("just now"),
        60..=3599 => trf(
            "{minutes} min ago",
            &[("minutes", &(seconds / 60).to_string())],
        ),
        3600..=86399 => trf("{hours} h ago", &[("hours", &(seconds / 3600).to_string())]),
        _ => trf("{days} d ago", &[("days", &(seconds / 86400).to_string())]),
    }
}

fn count(count: usize, words: [&str; 2]) -> String {
    trf(
        words[usize::from(count != 1)],
        &[("count", &count.to_string())],
    )
}

fn synchronization(sync: &Status) -> Option<gtk::Box> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let changes = |n| count(n, [t!("{count} change"), t!("{count} changes")]);
    let items = |n| count(n, [t!("{count} item"), t!("{count} items")]);
    let (title, text) = match model::sync_mark(sync, now) {
        SyncMark::Error => (
            t!("Sync error"),
            trf(
                "The last run failed: {error}.",
                &[("error", sync.error.as_deref().unwrap_or_default())],
            ),
        ),
        SyncMark::Receiving => (
            t!("Receiving"),
            trf(
                "{changes} waiting for files still on their way.",
                &[("changes", &changes(sync.pending.len()))],
            ),
        ),
        SyncMark::Attention {
            conflicts,
            refused,
            removed,
        } => {
            let mut parts = Vec::new();
            if conflicts > 0 {
                parts.push(trf(
                    "{conflicts} kept both versions",
                    &[(
                        "conflicts",
                        &count(conflicts, [t!("{count} conflict"), t!("{count} conflicts")]),
                    )],
                ));
            }
            if refused > 0 {
                parts.push(trf("{items} refused", &[("items", &items(refused))]));
            }
            if removed > 0 {
                parts.push(trf(
                    "{items} deleted on another computer, a copy kept here",
                    &[("items", &items(removed))],
                ));
            }
            (
                t!("Needs a look"),
                trf("{parts}. See the details.", &[("parts", &parts.join("; "))]),
            )
        }
        SyncMark::Published => {
            let behind: Vec<_> = sync
                .peers
                .iter()
                .filter(|p| p.applied_mine < p.published_mine)
                .collect();
            let hosts = behind
                .iter()
                .map(|p| p.host.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let count = behind
                .iter()
                .map(|p| p.published_mine - p.applied_mine)
                .max()
                .unwrap_or_default();
            let mut values = vec![("hosts", hosts), ("changes", changes(count as usize))];
            let phrase = match behind.first().and_then(|peer| peer.last_heard) {
                Some(at) => {
                    values.push(("time", ago(at, now)));
                    t!("{hosts} has not applied {changes} from here yet (last heard {time}).")
                }
                None => t!("{hosts} has not applied {changes} from here yet."),
            };
            let values: Vec<_> = values
                .iter()
                .map(|(key, value)| (*key, value.as_str()))
                .collect();
            (t!("Published"), trf(phrase, &values))
        }
        SyncMark::Quiet => return None,
    };
    let status = vertical(6);
    status.add_css_class("sync");
    status.append(&label(&format!("{} · {text}", tr(title)), "notice"));
    let details = gtk::Expander::new(Some(&tr("Synchronization details")));
    let facts = vertical(6);
    for (name, value) in [
        (
            tr("This computer"),
            trf(
                "{host} · {count} change(s) published",
                &[("host", &sync.host), ("count", &sync.published.to_string())],
            ),
        ),
        (
            tr("Shared folder"),
            format!("{} {}", sync.folder, tr("(Syncthing carries it)")),
        ),
        (
            tr("Last check"),
            sync.last_success
                .map(|at| ago(at, now))
                .unwrap_or_else(|| tr("not yet")),
        ),
    ] {
        facts.append(&label(&format!("{name} · {value}"), "hint"));
    }
    for peer in &sync.peers {
        let value = trf(
            "applied {applied} of {published} change(s) from here",
            &[
                ("applied", &peer.applied_mine.to_string()),
                ("published", &peer.published_mine.to_string()),
            ],
        );
        let heard = peer
            .last_heard
            .map(|at| {
                format!(
                    " · {}",
                    trf("last heard {time}", &[("time", &ago(at, now))])
                )
            })
            .unwrap_or_default();
        facts.append(&label(&format!("{} · {value}{heard}", peer.host), "hint"));
    }
    for (title, notes) in model::notes(sync)
        .into_iter()
        .filter(|(_, notes)| !notes.is_empty())
    {
        facts.append(&label(&tr(title), "title"));
        for note in notes {
            facts.append(&label(
                &format!(
                    "{} ({}, {}) — {}",
                    note.name,
                    note.kind,
                    ago(note.at, now),
                    note.detail
                ),
                "hint",
            ));
        }
    }
    details.set_child(Some(&facts));
    status.append(&details);
    Some(status)
}

/// "Received from another computer": each change another computer sent, what it does, its
/// files with scripts marked, and Accept / Refuse. Nothing of it is usable until accepted.
fn received(
    root: &gtk::Box,
    incoming: &[Incoming],
    this_device: &str,
    edit: &EditLaunch,
    create: &EditLaunch,
) -> Option<gtk::Box> {
    if incoming.is_empty() {
        return None;
    }
    let section = vertical(6);
    section.add_css_class("sync");
    section.append(&label(&tr("Received from another computer"), "title"));
    section.append(&label(
        &tr("Nothing here is used or applied to Claude Code or Codex until you accept it."),
        "hint",
    ));
    let cards = vertical(8);
    for change in incoming {
        let card = vertical(4);
        card.append(&label(
            &trf(
                "{name} · from {host} · {change}",
                &[
                    ("name", &change.name),
                    ("host", &change.host),
                    ("change", &tr(model::incoming_change(change))),
                ],
            ),
            "notice",
        ));
        if !this_device.is_empty() && change.device == this_device {
            card.append(&label(
                &tr("Says it comes from this computer, but this computer did not publish it."),
                "notice",
            ));
        }
        if change.scripts {
            card.append(&label(&tr("Brings scripts."), "notice"));
        }
        if change.kind == "capability" && change.records.len() > 1 {
            card.append(&label(
                &trf(
                    "Also brings {count} older revision(s).",
                    &[("count", &(change.records.len() - 1).to_string())],
                ),
                "hint",
            ));
        }
        if !change.files.is_empty() {
            let files = vertical(2);
            for file in &change.files {
                let (script, line) = model::incoming_file(file);
                let text = if script {
                    trf("{file} (script)", &[("file", &line)])
                } else {
                    line
                };
                files.append(&label(&text, "hint"));
            }
            let expander = gtk::Expander::new(Some(&trf(
                "{count} file(s)",
                &[("count", &change.files.len().to_string())],
            )));
            expander.set_child(Some(&files));
            card.append(&expander);
        }
        let actions = horizontal(8);
        for (key, accept) in [(t!("Accept"), true), (t!("Refuse"), false)] {
            let action = button(key, "compact-action");
            action.update_property(&[gtk::accessible::Property::Label(&trf(
                if accept {
                    t!("Accept {name} from {host}")
                } else {
                    t!("Refuse {name} from {host}")
                },
                &[("name", &change.name), ("host", &change.host)],
            ))]);
            let (root, edit, create) = (root.downgrade(), edit.clone(), create.clone());
            let (offer, section) = (change.clone(), section.downgrade());
            action.connect_clicked(move |_| {
                if let Some(section) = section.upgrade() {
                    section.set_sensitive(false);
                }
                let (root, edit, create, offer) =
                    (root.clone(), edit.clone(), create.clone(), offer.clone());
                glib::spawn_future_local(async move {
                    let result = gio::spawn_blocking(move || {
                        let decided = library::decide(&offer, accept);
                        (Data::load(), decided)
                    })
                    .await;
                    let (Some(root), Ok((data, decided))) = (root.upgrade(), result) else {
                        return;
                    };
                    let screen = render(&root, data, edit, create, None);
                    let problem = match decided {
                        Decided::Done => None,
                        Decided::Kept => Some(t!(
                            "The decision is kept; applying it failed and is retried on the next sync."
                        )),
                        Decided::Failed => Some(t!(
                            "Nothing was decided: the change was replaced or the library could not be written. Look at it again."
                        )),
                    };
                    if let Some(problem) = problem {
                        screen.prepend(&label(&tr(problem), "notice"));
                    }
                });
            });
            actions.append(&action);
        }
        card.append(&actions);
        cards.append(&card);
    }
    // Many changes arrive at once the first time two computers meet: they scroll, the rest of
    // the Arsenal stays in view.
    let list = gtk::ScrolledWindow::builder()
        .child(&cards)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(320)
        .build();
    section.append(&list);
    Some(section)
}

fn read_sync() -> Status {
    let Some(paths) = sync::Paths::from_env() else {
        return Status::default();
    };
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
    sync::read_status(&paths, host.trim())
}

fn watch_sync(root: &gtk::Box, stripe: &gtk::Box, mut previous: Status) {
    let (root, stripe) = (root.downgrade(), stripe.downgrade());
    glib::spawn_future_local(async move {
        loop {
            glib::timeout_future_seconds(4).await;
            if stripe.upgrade().is_none() {
                break;
            }
            let Some(visible) = root.upgrade().map(|root| root.is_mapped()) else {
                break;
            };
            if !visible {
                continue;
            }
            let Ok(status) = gio::spawn_blocking(read_sync).await else {
                continue;
            };
            if status == previous {
                continue;
            }
            let Some(stripe) = stripe.upgrade() else {
                break;
            };
            let expanded = stripe
                .first_child()
                .and_then(|stripe| stripe.last_child())
                .and_downcast::<gtk::Expander>()
                .is_some_and(|detail| detail.is_expanded());
            clear(&stripe);
            if let Some(status) = synchronization(&status) {
                if let Some(detail) = status.last_child().and_downcast::<gtk::Expander>() {
                    detail.set_expanded(expanded);
                }
                stripe.append(&status);
            }
            previous = status;
        }
    });
}

fn render(
    root: &gtk::Box,
    data: Data,
    edit: EditLaunch,
    create: EditLaunch,
    preset: Option<&str>,
) -> gtk::Box {
    clear(root);
    let stripe = vertical(0);
    if let Some(status) = synchronization(&data.sync) {
        stripe.append(&status);
    }
    root.append(&stripe);
    if let Some(section) = received(root, &data.incoming, &data.sync.device, &edit, &create) {
        root.append(&section);
    }
    watch_sync(root, &stripe, data.sync.clone());
    let split = horizontal(0);
    let side = vertical(8);
    side.add_css_class("tree");
    let header = horizontal(6);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some(&tr("Search")));
    search.update_property(&[gtk::accessible::Property::Label(&tr(
        "Search skills and agents",
    ))]);
    header.append(&search);
    let fold = gtk::ToggleButton::with_label("«");
    fold.set_tooltip_text(Some(&tr("Hide the tree")));
    header.append(&fold);
    side.append(&header);
    search.set_hexpand(true);
    let nodes = vertical(0);
    let tree_scroll = scroll(&nodes);
    side.append(&tree_scroll);
    let right = vertical(8);
    right.add_css_class("content");
    let browser = Browser {
        data: Rc::new(data),
        right: right.downgrade(),
        root: root.downgrade(),
        busy: Rc::default(),
        selected: Rc::default(),
        edit,
        create,
        page: Rc::default(),
    };
    let started = Instant::now();
    let initial = tree(&nodes, &browser, preset);
    timing("ars:tree", started);
    search.connect_search_changed(move |search| {
        let query = search.text().trim().to_owned();
        browser.show_entries(if query.is_empty() {
            initial.clone()
        } else {
            Browse::Search(query)
        });
    });
    // A scrolled viewport keeps child requisitions (search and deep nodes) from
    // changing the width allocated to the tree when the right pane changes.
    let sidebar = gtk::ScrolledWindow::builder()
        .child(&side)
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .min_content_width(280)
        .vexpand(true)
        .build();
    let (search, tree_scroll, side_weak, sidebar_weak) = (
        search.downgrade(),
        tree_scroll.downgrade(),
        side.downgrade(),
        sidebar.downgrade(),
    );
    fold.connect_toggled(move |fold| {
        let open = !fold.is_active();
        if let Some(search) = search.upgrade() {
            search.set_visible(open);
        }
        if let Some(tree) = tree_scroll.upgrade() {
            tree.set_visible(open);
        }
        if let Some(side) = side_weak.upgrade() {
            if open {
                side.add_css_class("tree");
            } else {
                side.remove_css_class("tree");
            }
        }
        if let Some(sidebar) = sidebar_weak.upgrade() {
            sidebar.set_min_content_width(if open { 280 } else { 50 });
        }
        fold.set_label(if open { "«" } else { "»" });
        fold.set_tooltip_text(Some(&if open {
            tr("Hide the tree")
        } else {
            tr("Show the tree")
        }));
    });
    split.append(&sidebar);
    let main = scroll(&right);
    main.set_hexpand(true);
    split.append(&main);
    root.append(&split);
    right
}

pub fn build(edit: EditLaunch, create: EditLaunch) -> gtk::Box {
    let root = vertical(0);
    root.add_css_class("arsenal");
    root.append(&label(&tr("Reading…"), "hint"));
    let weak = root.downgrade();
    glib::spawn_future_local(async move {
        match gio::spawn_blocking(Data::load).await {
            Ok(data) => {
                if let Some(root) = weak.upgrade() {
                    render(&root, data, edit, create, None);
                }
            }
            Err(_) => {
                if let Some(root) = weak.upgrade() {
                    clear(&root);
                    root.append(&label(
                        &tr("The skills on disk could not be read."),
                        "notice",
                    ));
                }
            }
        }
    });
    root
}
