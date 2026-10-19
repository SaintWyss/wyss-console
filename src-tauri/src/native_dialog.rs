//! The dialogs only the desktop can show (11 §4): GTK windows that the page cannot draw on,
//! read, or answer for. A confirmation names what is about to change and has no "do not ask
//! again"; a passphrase is typed here and never reaches the page; a file is chosen here and
//! its path goes nowhere but to the file system call that uses it.
use crate::agentconfig::changes::{Confirm, ConfirmRequest, Effect};
use crate::agentconfig::{Operation, ProviderId, Risk, Scope};
use std::{
    path::PathBuf,
    sync::{Arc, OnceLock, mpsc},
    time::Duration,
};
use tauri::{AppHandle, Manager};
use zeroize::Zeroizing;

/// Longer than anyone leaves a dialog open; shorter than for ever.
const WAIT: Duration = Duration::from_secs(10 * 60);

fn main_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    app.get_webview_window("main")
}

/// Runs `show` on GTK's thread, with the main window as parent, and waits for its answer.
fn on_gtk<T: Send + 'static>(
    app: &AppHandle,
    show: impl FnOnce(Option<gtk::ApplicationWindow>) -> T + Send + 'static,
) -> Option<T> {
    let window = main_window(app)?;
    let (answer, answered) = mpsc::channel();
    let host = window.clone();
    window
        .run_on_main_thread(move || {
            let _ = answer.send(show(host.gtk_window().ok()));
        })
        .ok()?;
    answered.recv_timeout(WAIT).ok()
}

fn operation_text(operation: Operation) -> &'static str {
    match operation {
        Operation::ParkHook => "Turn off a hook (remove it from the file and keep it here)",
        Operation::RestoreHook => "Turn a hook back on (put it back in the file)",
        Operation::DisableAllHooks => "Turn off every hook",
        Operation::EnableAllHooks => "Allow hooks again",
        Operation::SkillOff => "Turn off a skill",
        Operation::SkillOn => "Turn on a skill",
        Operation::CodexSkillOff => "Turn off a Codex skill",
        Operation::CodexSkillOn => "Turn on a Codex skill",
        Operation::CodexMcpOff => "Turn off a Codex MCP server",
        Operation::CodexMcpOn => "Turn on a Codex MCP server (it can start a program)",
    }
}

fn risk_text(risk: Risk) -> &'static str {
    match risk {
        Risk::Normal => "",
        Risk::WeakensSecurity => "This can switch off something that stops an action.",
        Risk::ExecutesCode => "This puts something back that runs programs.",
    }
}

fn effect_text(effect: Effect) -> &'static str {
    match effect {
        Effect::HooksNewSessionsChecked => {
            "A new Claude Code session follows the file (checked). A session that is already running is documented to pick it up; that has not been checked."
        }
        Effect::CodexMcpChecked => {
            "A new Codex process shows the server as the file says (checked). A Codex session that is already running is not known to follow it."
        }
        Effect::CodexSkillListed => {
            "A new Codex process lists the skill as the file says (checked, through its own skills/list). A Codex session that is already running is not known to follow it."
        }
        Effect::SkillsNewSessionsChecked => {
            "A new Claude Code session refuses to run a skill that is switched off (checked). Skills that come from a plugin are not affected. What a running session does is not known."
        }
    }
}

/// The confirmation dialog. The application handle is given by the first command that
/// arrives, because the service is made before the application exists.
pub struct GtkConfirm {
    app: Arc<OnceLock<AppHandle>>,
}

impl GtkConfirm {
    pub fn new(app: Arc<OnceLock<AppHandle>>) -> Self {
        Self { app }
    }
}

impl Confirm for GtkConfirm {
    fn ask(&self, request: &ConfirmRequest) -> bool {
        use gtk::prelude::*;
        let provider = match request.provider {
            ProviderId::ClaudeCode => "Claude Code",
            ProviderId::Codex => "Codex",
            ProviderId::Opencode => "OpenCode",
        };
        let scope = match request.scope {
            Scope::System => "system",
            Scope::Global => "your account",
            Scope::Project => "this project",
        };
        let mut body = format!(
            "{provider}, {scope}\nEntry: {}\n\n{}\n{}",
            request.name,
            effect_text(request.effect),
            risk_text(request.risk),
        );
        if request.holds_hidden_values {
            body.push_str("\nThe entry holds values that are not shown here.");
        }
        let heading = operation_text(request.operation).to_owned();
        let Some(app) = self.app.get() else {
            return false;
        };
        on_gtk(app, move |parent| {
            let dialog = gtk::MessageDialog::builder()
                .modal(true)
                .message_type(gtk::MessageType::Warning)
                .text(heading)
                .secondary_text(body)
                .build();
            if let Some(parent) = &parent {
                dialog.set_transient_for(Some(parent));
            }
            dialog.add_button("Cancel", gtk::ResponseType::Cancel);
            dialog.add_button("Apply", gtk::ResponseType::Accept);
            dialog.set_default_response(gtk::ResponseType::Cancel);
            let accepted = dialog.run() == gtk::ResponseType::Accept;
            dialog.close();
            accepted
        })
        // A dialog that could not be shown is a refusal.
        .unwrap_or(false)
    }
}

/// A passphrase typed in a window of its own. `again` asks twice, for a new one.
pub fn passphrase(app: &AppHandle, again: bool) -> Option<Zeroizing<String>> {
    use gtk::prelude::*;
    on_gtk(app, move |parent| {
        let dialog = gtk::Dialog::builder()
            .modal(true)
            .title("Passphrase")
            .build();
        if let Some(parent) = &parent {
            dialog.set_transient_for(Some(parent));
        }
        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
        dialog.add_button("OK", gtk::ResponseType::Accept);
        let content = dialog.content_area();
        let entry = |placeholder: &str| {
            let entry = gtk::Entry::new();
            entry.set_visibility(false);
            entry.set_placeholder_text(Some(placeholder));
            entry.set_activates_default(true);
            content.add(&entry);
            entry
        };
        let first = entry(if again {
            "New passphrase for this file"
        } else {
            "Passphrase of this file"
        });
        let second = again.then(|| entry("Type it again"));
        dialog.set_default_response(gtk::ResponseType::Accept);
        dialog.show_all();
        let response = dialog.run();
        let chosen = first.text().to_string();
        let same = second.as_ref().is_none_or(|s| s.text() == chosen.as_str());
        dialog.close();
        (response == gtk::ResponseType::Accept && same && !chosen.is_empty())
            .then(|| Zeroizing::new(chosen))
    })
    .flatten()
}

/// A file chosen to write (`save`) or to read, in GTK's own chooser.
pub fn choose_file(app: &AppHandle, save: bool, name: &str) -> Option<PathBuf> {
    choose_file_titled(
        app,
        save,
        name,
        if save {
            "Export record"
        } else {
            "Import record"
        },
    )
}

pub fn choose_file_titled(
    app: &AppHandle,
    save: bool,
    name: &str,
    title: &'static str,
) -> Option<PathBuf> {
    use gtk::prelude::*;
    let name = name.to_owned();
    on_gtk(app, move |parent| {
        let chooser = gtk::FileChooserNative::new(
            Some(title),
            parent.as_ref(),
            if save {
                gtk::FileChooserAction::Save
            } else {
                gtk::FileChooserAction::Open
            },
            Some(if save { "Save" } else { "Open" }),
            Some("Cancel"),
        );
        chooser.set_modal(true);
        if save {
            chooser.set_current_name(&name);
            chooser.set_do_overwrite_confirmation(true);
        }
        let accepted = chooser.run() == gtk::ResponseType::Accept;
        accepted.then(|| chooser.filename()).flatten()
    })
    .flatten()
}

/// A folder chosen in GTK's own chooser: the only way the window gets to name a project.
pub fn choose_folder(app: &AppHandle) -> Option<PathBuf> {
    use gtk::prelude::*;
    on_gtk(app, move |parent| {
        let chooser = gtk::FileChooserNative::new(
            Some("Select project folder"),
            parent.as_ref(),
            gtk::FileChooserAction::SelectFolder,
            Some("Select"),
            Some("Cancel"),
        );
        chooser.set_modal(true);
        let accepted = chooser.run() == gtk::ResponseType::Accept;
        accepted.then(|| chooser.filename()).flatten()
    })
    .flatten()
}

/// A secret typed in a window of its own, masked, asked once. The page never sees it.
pub fn secret_entry(
    app: &AppHandle,
    title: &'static str,
    label: &'static str,
) -> Option<Zeroizing<String>> {
    use gtk::prelude::*;
    on_gtk(app, move |parent| {
        let dialog = gtk::Dialog::builder().modal(true).title(title).build();
        if let Some(parent) = &parent {
            dialog.set_transient_for(Some(parent));
        }
        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
        dialog.add_button("Keep", gtk::ResponseType::Accept);
        let content = dialog.content_area();
        content.add(&gtk::Label::new(Some(label)));
        let entry = gtk::Entry::new();
        entry.set_visibility(false);
        entry.set_activates_default(true);
        content.add(&entry);
        dialog.set_default_response(gtk::ResponseType::Accept);
        dialog.show_all();
        let response = dialog.run();
        let typed = entry.text().to_string();
        entry.set_text("");
        dialog.close();
        (response == gtk::ResponseType::Accept && !typed.is_empty()).then(|| Zeroizing::new(typed))
    })
    .flatten()
}

/// A yes or no in a window of its own. A dialog that could not be shown is a no.
pub fn confirm_text(app: &AppHandle, heading: &str, body: &str) -> bool {
    use gtk::prelude::*;
    let (heading, body) = (heading.to_owned(), body.to_owned());
    on_gtk(app, move |parent| {
        let dialog = gtk::MessageDialog::builder()
            .modal(true)
            .message_type(gtk::MessageType::Warning)
            .text(heading)
            .secondary_text(body)
            .build();
        if let Some(parent) = &parent {
            dialog.set_transient_for(Some(parent));
        }
        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
        dialog.add_button("Delete", gtk::ResponseType::Accept);
        dialog.set_default_response(gtk::ResponseType::Cancel);
        let accepted = dialog.run() == gtk::ResponseType::Accept;
        dialog.close();
        accepted
    })
    .unwrap_or(false)
}
