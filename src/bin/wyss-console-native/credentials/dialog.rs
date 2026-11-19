use std::ffi::CStr;

use gtk::{gdk, glib, prelude::*};
use wyss_console::credentials::CredentialError;
use zeroize::Zeroizing;

use super::model::matches_name;
use crate::{
    i18n::{tr, trf},
    personalization::{button, horizontal, label, vertical},
    t,
};

struct Prompt {
    window: gtk::Window,
    content: gtk::Box,
    actions: gtk::Box,
    accept: gtk::Button,
}

impl Prompt {
    fn new(parent: &gtk::Window, title: &str, message: &str, accept_key: &str) -> Self {
        let content = vertical(12);
        content.add_css_class("credential-prompt-content");
        content.append(&label(message, ""));
        let window = gtk::Window::builder()
            .title(title)
            .transient_for(parent)
            .modal(true)
            .destroy_with_parent(true)
            .default_width(420)
            .resizable(false)
            .child(&content)
            .build();
        window.add_css_class("console");
        let actions = horizontal(6);
        actions.set_halign(gtk::Align::End);
        let cancel = button(t!("Cancel"), "action");
        let weak = window.downgrade();
        cancel.connect_clicked(move |_| {
            if let Some(window) = weak.upgrade() {
                window.close();
            }
        });
        actions.append(&cancel);
        let accept = button(accept_key, "action");
        accept.set_sensitive(false);
        actions.append(&accept);
        let keys = gtk::EventControllerKey::new();
        let weak = window.downgrade();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if let Some(window) = weak.upgrade() {
                    window.close();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        window.add_controller(keys);
        Self {
            window,
            content,
            actions,
            accept,
        }
    }

    fn present(self, entry: &impl IsA<gtk::Widget>) {
        self.content.append(entry);
        self.content.append(&self.actions);
        self.window.present();
        entry.grab_focus();
    }
}

type SecretResult = Result<Zeroizing<String>, CredentialError>;

fn read_secret(entry: &gtk::PasswordEntry) -> SecretResult {
    let editable = entry.upcast_ref::<gtk::Editable>();
    // GTK owns this NUL-terminated UTF-8 buffer. Borrow it only until the copy into
    // Zeroizing completes, with no GTK calls or main-loop reentry during that borrow.
    // EditableExt::text would make an additional unprotected GString copy.
    let text = unsafe { CStr::from_ptr(gtk::ffi::gtk_editable_get_text(editable.as_ptr())) };
    text.to_str()
        .map(|text| Zeroizing::new(text.to_owned()))
        .map_err(|_| CredentialError::SecretInvalid)
}

pub(super) fn secret_entry(
    parent: &gtk::Window,
    title: &str,
    message: &str,
    submit: impl Fn(SecretResult) + 'static,
) {
    let (prompt, entry) = secret_prompt(parent, title, message, submit);
    prompt.present(&entry);
}

fn secret_prompt(
    parent: &gtk::Window,
    title: &str,
    message: &str,
    submit: impl Fn(SecretResult) + 'static,
) -> (Prompt, gtk::PasswordEntry) {
    let prompt = Prompt::new(parent, title, message, t!("Keep"));
    let entry = gtk::PasswordEntry::builder().show_peek_icon(false).build();
    entry.update_property(&[gtk::accessible::Property::Label(title)]);
    // PasswordEntry's non-visible GtkText rejects copy/cut itself. Disable its
    // clipboard actions as well, including action activation from a context menu.
    if let Some(text) = entry.delegate().and_downcast::<gtk::Text>() {
        text.action_set_enabled("clipboard.copy", false);
        text.action_set_enabled("clipboard.cut", false);
    }
    let accept = prompt.accept.downgrade();
    entry.connect_changed(move |entry| {
        if let Some(accept) = accept.upgrade() {
            accept.set_sensitive(read_secret_len(entry) != 0);
        }
    });
    let accept = prompt.accept.downgrade();
    entry.connect_activate(move |_| {
        if let Some(accept) = accept
            .upgrade()
            .filter(gtk::prelude::WidgetExt::is_sensitive)
        {
            accept.emit_clicked();
        }
    });
    let clear = entry.downgrade();
    prompt.window.connect_close_request(move |_| {
        if let Some(entry) = clear.upgrade() {
            entry.set_text("");
        }
        glib::Propagation::Proceed
    });
    let clear = entry.downgrade();
    // GTK can unmap a destroyed window before emitting destroy, if references remain.
    prompt.window.connect_unmap(move |_| {
        if let Some(entry) = clear.upgrade() {
            entry.set_text("");
        }
    });
    let clear = entry.downgrade();
    prompt.window.connect_destroy(move |_| {
        if let Some(entry) = clear.upgrade() {
            entry.set_text("");
        }
    });
    let (field, window) = (entry.downgrade(), prompt.window.downgrade());
    prompt.accept.connect_clicked(move |_| {
        if let (Some(entry), Some(window)) = (field.upgrade(), window.upgrade()) {
            let secret = read_secret(&entry);
            entry.set_text("");
            window.close();
            submit(secret.and_then(|secret| {
                if secret.is_empty() {
                    Err(CredentialError::SecretInvalid)
                } else {
                    Ok(secret)
                }
            }));
        }
    });
    (prompt, entry)
}

fn read_secret_len(entry: &gtk::PasswordEntry) -> i32 {
    // GtkEditable positions are character offsets. Asking the delegate for its
    // buffer length avoids copying the password while updating button state.
    entry
        .delegate()
        .and_downcast::<gtk::Text>()
        .map_or(0, |text| i32::from(text.text_length()))
}

pub(super) fn confirm_text(parent: &gtk::Window, name: &str, submit: impl Fn(String) + 'static) {
    let prompt = Prompt::new(
        parent,
        &tr("Delete this API key?"),
        &trf(
            "“{name}” is removed from this computer. It cannot be brought back. Type its exact name to continue.",
            &[("name", name)],
        ),
        t!("Delete"),
    );
    let entry = gtk::Entry::new();
    entry.update_property(&[gtk::accessible::Property::Label(&tr("Name"))]);
    let (expected, accept) = (name.to_owned(), prompt.accept.downgrade());
    entry.connect_changed(move |entry| {
        if let Some(accept) = accept.upgrade() {
            accept.set_sensitive(matches_name(&expected, entry.text().as_str()));
        }
    });
    let (expected, field, window) = (
        name.to_owned(),
        entry.downgrade(),
        prompt.window.downgrade(),
    );
    prompt.accept.connect_clicked(move |_| {
        if let (Some(entry), Some(window)) = (field.upgrade(), window.upgrade()) {
            let entered = entry.text();
            if matches_name(&expected, entered.as_str()) {
                window.close();
                submit(entered.into());
            }
        }
    });
    prompt.present(&entry);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn password_entry_is_cleared_on_every_close_path() {
        if let Err(error) = gtk::init() {
            eprintln!("Skipping GTK password-entry test: {error}");
            return;
        }

        let parent = gtk::Window::new();
        for path in ["Cancel", "Escape", "close-request", "Keep", "destroy"] {
            let submitted = Rc::new(Cell::new(false));
            let received = Rc::clone(&submitted);
            let (prompt, entry) = secret_prompt(&parent, "Fixture", "Fixture", move |secret| {
                assert!(secret.is_ok_and(|secret| secret.as_str() == "1"));
                received.set(true);
            });
            let window = prompt.window.clone();
            let accept = prompt.accept.clone();
            let cancel = prompt
                .actions
                .first_child()
                .and_downcast::<gtk::Button>()
                .unwrap();
            prompt.present(&entry);
            entry.set_text("1");
            assert_eq!(read_secret_len(&entry), 1, "{path}");

            match path {
                "Cancel" => cancel.emit_clicked(),
                "Escape" => {
                    let controllers = window.observe_controllers();
                    let keys = (0..controllers.n_items())
                        .find_map(|index| {
                            controllers
                                .item(index)
                                .and_downcast::<gtk::EventControllerKey>()
                        })
                        .unwrap();
                    assert!(keys.emit_by_name::<bool>(
                        "key-pressed",
                        &[&gdk::Key::Escape, &0u32, &gdk::ModifierType::empty()],
                    ));
                }
                "close-request" => window.close(),
                "Keep" => {
                    assert!(accept.is_sensitive());
                    accept.emit_clicked();
                }
                "destroy" => window.destroy(),
                _ => unreachable!(),
            }

            assert_eq!(read_secret_len(&entry), 0, "{path}");
            assert_eq!(submitted.get(), path == "Keep", "{path}");
        }
        parent.destroy();
    }
}
