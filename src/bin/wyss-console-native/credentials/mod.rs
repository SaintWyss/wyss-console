mod dialog;
mod model;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

use gtk::{gio, glib, prelude::*};
use wyss_console::{
    agentconfig::{SecretToolStore, Vault},
    credentials::{CredentialError, CredentialStore, MAX_NAME, Meta, Provider},
};

use self::model::Model;
use crate::{
    i18n::{Lang, lang, tr},
    personalization::{button, clear, horizontal, label, scroll, timing, vertical},
    t,
};

pub const CSS: &str = include_str!("credentials.css");

const PROVIDERS: [(Provider, &str); 5] = [
    (Provider::Anthropic, "Anthropic"),
    (Provider::Openai, "OpenAI"),
    (Provider::Google, "Google"),
    (Provider::Openrouter, "OpenRouter"),
    (Provider::Other, t!("Other")),
];

fn store() -> Result<Arc<CredentialStore>, CredentialError> {
    // Share the core's operation lock across page opens, even if a worker is still finishing.
    static STORE: Mutex<Option<Arc<CredentialStore>>> = Mutex::new(None);
    let mut existing = STORE
        .lock()
        .map_err(|_| CredentialError::StoreUnavailable)?;
    if let Some(store) = &*existing {
        return Ok(Arc::clone(store));
    }
    let directory = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .ok_or(CredentialError::StoreUnavailable)?
        .join("wyss-console/credentials");
    let vault = Vault::new(Box::new(SecretToolStore::new()));
    let store = Arc::new(CredentialStore::open(directory, vault)?);
    *existing = Some(Arc::clone(&store));
    Ok(store)
}

fn failure(error: CredentialError) -> &'static str {
    match error {
        CredentialError::NameInvalid => {
            t!("The name must be 1 to 60 characters, not blank, without control characters.")
        }
        CredentialError::SecretInvalid => t!(
            "The key may not be empty, contain whitespace or control characters, or be longer than 4096 bytes."
        ),
        CredentialError::Unknown => t!("That key is not kept any more."),
        CredentialError::TooMany => t!("There are too many keys (100)."),
        CredentialError::KeyStoreUnavailable => t!(
            "No key store (the Secret Service) is available, so nothing was kept. A key is never kept unsealed."
        ),
        CredentialError::KeyLost => {
            t!("The key that sealed this record is gone from the key store. Replace the key.")
        }
        CredentialError::Damaged => t!("The record is damaged. Replace the key."),
        CredentialError::StoreUnavailable => t!("The key store of Wyss Console is not available."),
    }
}

type Notice = Result<Option<&'static str>, CredentialError>;

#[derive(Clone)]
struct Page {
    content: glib::WeakRef<gtk::Box>,
    items: glib::WeakRef<gtk::Box>,
    status: glib::WeakRef<gtk::Label>,
    name: glib::WeakRef<gtk::Entry>,
}

impl Page {
    fn show_status(&self, message: Option<&str>, failed: bool) {
        if let Some(status) = self.status.upgrade() {
            status.set_text(&message.map(tr).unwrap_or_default());
            status.set_visible(message.is_some());
            if failed {
                status.add_css_class("notice");
            } else {
                status.remove_css_class("notice");
            }
        }
    }

    fn ready_content(&self) -> Option<gtk::Box> {
        let content = self.content.upgrade()?;
        if content.is_sensitive() {
            Some(content)
        } else {
            self.show_status(
                Some(match lang() {
                    Lang::En => "Another operation is in progress. Try again when it finishes.",
                    Lang::Es => "Hay otra operación en curso. Volvé a intentar cuando termine.",
                }),
                true,
            );
            None
        }
    }

    fn run(
        &self,
        metric: &'static str,
        reset_name: Option<glib::GString>,
        operation: impl FnOnce(&CredentialStore) -> Notice + Send + 'static,
    ) {
        let Some(content) = self.ready_content() else {
            return;
        };
        content.set_sensitive(false);
        let page = self.clone();
        glib::spawn_future_local(async move {
            let started = Instant::now();
            let result = gio::spawn_blocking(move || {
                let store = store()?;
                let notice = operation(&store);
                Ok::<_, CredentialError>((notice, Model::list(&store)))
            })
            .await;
            timing(metric, started);
            content.set_sensitive(true);
            let (notice, model) = match result {
                Ok(Ok((notice, model))) => (notice, Some(model)),
                Ok(Err(error)) => (Err(error), None),
                Err(_) => (Err(CredentialError::StoreUnavailable), None),
            };
            if let Some(items) = page.items.upgrade() {
                clear(&items);
                if let Some(model) = model {
                    page.render_items(&items, model);
                }
            }
            if matches!(notice, Ok(Some(_)))
                && let Some(original) = reset_name
                && let Some(name) = page.name.upgrade()
                && name.text() == original
            {
                name.set_text("");
            }
            match notice {
                Ok(message) => page.show_status(message, false),
                Err(error) => page.show_status(Some(failure(error)), true),
            }
        });
    }

    fn render_items(&self, items: &gtk::Box, model: Model) {
        if model.items.is_empty() {
            let empty = vertical(8);
            empty.add_css_class("credential-empty");
            empty.set_vexpand(true);
            let message = vertical(8);
            message.set_halign(gtk::Align::Center);
            message.set_valign(gtk::Align::Center);
            message.set_vexpand(true);
            let title = label(&tr("No keys yet"), "credential-title");
            title.set_halign(gtk::Align::Center);
            message.append(&title);
            message.append(&label(
                &tr("Add one above. Its value is asked for in a separate window."),
                "credential-hint",
            ));
            empty.append(&message);
            items.append(&empty);
            return;
        }
        for item in model.items {
            items.append(&self.row(item));
        }
    }

    fn row(&self, item: Meta) -> gtk::Box {
        let row = vertical(6);
        row.add_css_class("credential-row");
        let details = horizontal(8);
        let name = label(&item.name, "credential-title");
        name.set_hexpand(true);
        details.append(&name);
        let provider = PROVIDERS
            .iter()
            .find(|(provider, _)| *provider == item.provider);
        if let Some((_, service)) = provider {
            details.append(&label(&tr(service), "credential-service"));
        }
        details.append(&label(
            &format!("{} {}", tr("checksum"), item.fingerprint),
            "credential-hint",
        ));
        row.append(&details);

        let actions = horizontal(6);
        actions.set_halign(gtk::Align::End);
        let replace = button(t!("Replace key…"), "action");
        let (page, id) = (self.clone(), item.id.clone());
        replace.connect_clicked(move |button| {
            if page.ready_content().is_none() {
                return;
            }
            let Some(parent) = button.root().and_downcast::<gtk::Window>() else {
                return;
            };
            let (page, id) = (page.clone(), id.clone());
            dialog::secret_entry(
                &parent,
                &tr("Replace API key"),
                &tr("Paste the new key. The old one is discarded."),
                move |result| {
                    let secret = match result {
                        Ok(secret) => secret,
                        Err(error) => {
                            page.show_status(Some(failure(error)), true);
                            return;
                        }
                    };
                    let id = id.clone();
                    page.run("cred:replace", None, move |store| {
                        Model::replace(store, &id, secret).map(|()| Some(t!("Key replaced.")))
                    });
                },
            );
        });
        actions.append(&replace);

        let rename = button(t!("Rename"), "action");
        let editor = self.rename_editor(&name, &actions, &item);
        let (edit, display, actions_weak) =
            (editor.downgrade(), name.downgrade(), actions.downgrade());
        rename.connect_clicked(move |_| {
            if let (Some(edit), Some(display), Some(actions)) =
                (edit.upgrade(), display.upgrade(), actions_weak.upgrade())
            {
                actions.set_visible(false);
                display.set_visible(false);
                edit.set_visible(true);
                edit.grab_focus();
            }
        });
        actions.append(&rename);
        details.prepend(&editor);

        let verify = button(t!("Check"), "action");
        let (page, id) = (self.clone(), item.id.clone());
        verify.connect_clicked(move |_| {
            let id = id.clone();
            page.run("cred:verify", None, move |store| {
                Model::verify(store, &id).map(|valid| {
                    Some(if valid {
                        t!("The key opens and matches its checksum.")
                    } else {
                        t!("The key could not be verified. Replace the key.")
                    })
                })
            });
        });
        actions.append(&verify);
        let delete = button(t!("Delete…"), "action");
        let page = self.clone();
        delete.connect_clicked(move |button| {
            let Some(parent) = button.root().and_downcast::<gtk::Window>() else {
                return;
            };
            let (page, id) = (page.clone(), item.id.clone());
            dialog::confirm_text(&parent, &item.name, move |entered| {
                let id = id.clone();
                page.run("cred:delete", None, move |store| {
                    if Model::delete(store, &id, &entered)? {
                        Ok(Some(t!("Key deleted.")))
                    } else {
                        Ok(Some(match lang() {
                            Lang::En => "The name changed. Nothing was deleted.",
                            Lang::Es => "El nombre cambió; no se borró nada.",
                        }))
                    }
                });
            });
        });
        actions.append(&delete);
        row.append(&actions);
        row
    }

    fn rename_editor(&self, display: &gtk::Label, actions: &gtk::Box, item: &Meta) -> gtk::Box {
        let edit = horizontal(6);
        edit.set_visible(false);
        edit.set_hexpand(true);
        let entry = name_entry(&tr("New name"));
        entry.set_text(&item.name);
        edit.append(&entry);
        let save = button(t!("Save name"), "action");
        enable_with_name(&entry, &save);
        let (page, id, field) = (self.clone(), item.id.clone(), entry.downgrade());
        save.connect_clicked(move |_| {
            if let Some(entry) = field.upgrade() {
                let (id, name) = (id.clone(), entry.text());
                page.run("cred:rename", None, move |store| {
                    Model::rename(store, &id, name.as_str()).map(|()| Some(t!("Renamed.")))
                });
            }
        });
        edit.append(&save);
        let cancel = button(t!("Cancel"), "action");
        let (display, actions, editor, original) = (
            display.downgrade(),
            actions.downgrade(),
            edit.downgrade(),
            item.name.clone(),
        );
        cancel.connect_clicked(move |_| {
            if let (Some(display), Some(actions), Some(editor)) =
                (display.upgrade(), actions.upgrade(), editor.upgrade())
            {
                entry.set_text(&original);
                editor.set_visible(false);
                display.set_visible(true);
                actions.set_visible(true);
            }
        });
        edit.append(&cancel);
        edit
    }
}

fn name_entry(caption: &str) -> gtk::Entry {
    let entry = gtk::Entry::builder()
        .hexpand(true)
        .max_length(MAX_NAME as i32)
        .build();
    entry.update_property(&[gtk::accessible::Property::Label(caption)]);
    entry
}

fn enable_with_name(entry: &gtk::Entry, control: &gtk::Button) {
    control.set_sensitive(!entry.text().trim().is_empty());
    let control = control.downgrade();
    entry.connect_changed(move |entry| {
        if let Some(control) = control.upgrade() {
            control.set_sensitive(!entry.text().trim().is_empty());
        }
    });
}

pub fn build() -> gtk::ScrolledWindow {
    let root = vertical(10);
    root.add_css_class("credentials");
    root.set_vexpand(true);
    root.update_property(&[gtk::accessible::Property::Label(&tr("API credentials"))]);
    root.append(&label(&tr(
        "API keys for the AI services, kept sealed on this computer (the key that seals them is in your Secret Service). A key is typed in a window of its own and is never shown again, never sent to this page and never written unsealed. Only a name, the service and a short checksum are listed. Nothing here starts an agent with a key.",
    ), "credential-hint"));
    let status = label("", "");
    status.set_visible(false);
    root.append(&status);
    let form = vertical(8);
    form.add_css_class("credential-card");
    form.append(&label(&tr("Add a key"), "credential-title"));
    form.append(&label(&tr("Name"), "credential-hint"));
    let name = name_entry(&tr("Name"));
    form.append(&name);
    form.append(&label(&tr("Service"), "credential-hint"));
    let services = PROVIDERS.map(|(_, service)| tr(service));
    let provider = gtk::DropDown::from_strings(&services.each_ref().map(String::as_str));
    provider.update_property(&[gtk::accessible::Property::Label(&tr("Service"))]);
    form.append(&provider);
    let add = button(t!("Enter the key…"), "credential-primary");
    add.set_halign(gtk::Align::Start);
    enable_with_name(&name, &add);
    form.append(&add);
    root.append(&form);
    let items = vertical(0);
    items.set_vexpand(true);
    items.update_property(&[gtk::accessible::Property::Label(&tr("Keys"))]);
    items.append(&label(&tr("Reading…"), "credential-hint"));
    root.append(&items);

    let page = Page {
        content: root.downgrade(),
        items: items.downgrade(),
        status: status.downgrade(),
        name: name.downgrade(),
    };
    let field = name.downgrade();
    let add_page = page.clone();
    add.connect_clicked(move |button| {
        if add_page.ready_content().is_none() {
            return;
        }
        let (Some(parent), Some(field), Some(&(provider, _))) = (
            button.root().and_downcast::<gtk::Window>(),
            field.upgrade(),
            PROVIDERS.get(provider.selected() as usize),
        ) else {
            return;
        };
        let name = field.text();
        if name.trim().is_empty() {
            return;
        }
        let page = add_page.clone();
        dialog::secret_entry(
            &parent,
            &tr("New API key"),
            &tr("Paste the key. It is never shown again."),
            move |result| {
                let secret = match result {
                    Ok(secret) => secret,
                    Err(error) => {
                        page.show_status(Some(failure(error)), true);
                        return;
                    }
                };
                let name = name.clone();
                page.run("cred:add", Some(name.clone()), move |store| {
                    Model::add(store, name.as_str(), provider, secret)
                        .map(|()| Some(t!("Key kept, sealed.")))
                });
            },
        );
    });
    let screen = scroll(&root);
    screen.connect_map(move |_| page.run("cred:list", None, |_| Ok(None)));
    screen
}
