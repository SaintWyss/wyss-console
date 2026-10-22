//! The native confirmation of what lets a managed runtime act (finding W1).
//!
//! With protocol 2 active, a window that has been compromised could ask the authority to
//! accept a command, or to grant the lease that lets one be proposed. These steps are
//! therefore not decided by the page alone: before the command reaches the authority, **this
//! side** reads what is being decided from the authority itself, shows it in a window of the
//! toolkit that the page cannot draw on, read, click or answer for, and sends the decision
//! only if the user pressed Allow there.
//!
//! - **What is shown is what is sent.** For an approval the action (command and directory, or
//!   every path and diff of a change set) is read here, by the desktop, not taken from the
//!   page; the revision decided is the one read here, and the page's must agree with it.
//!   For a lease the scopes and the lifetime shown are the ones sent.
//! - **Fail closed.** No window, an unanswered window, a window already open, a change set
//!   too large to show whole, or any read that fails: not allowed. A change that cannot be
//!   shown whole can only be declined.
//! - **No habit.** Cancel is the default response; Allow is inert for a moment after the
//!   window opens so that a click meant for something else cannot confirm it; there is no
//!   "do not ask again"; one window at a time.
//! - **Honest text.** Control and formatting characters (a right-to-left override, a zero-width
//!   joiner) are shown as `⟨U+XXXX⟩` instead of acting.
//!
//! Declining and cancelling take no confirmation: they only reduce what can happen.
use super::{
    ApprovalDecision, DecisionReceipt, ExecutionScope, F8Error, F8Failure, F8Gui, FenceView,
};
use crate::{
    contracts::identities::{ManagedApprovalId, ManagedSessionId},
    f8::gui::ApprovalDetail,
};
use std::{
    collections::VecDeque,
    fmt::Write as _,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// Characters of text one window shows. Past this the user cannot read it whole, and Allow
/// is not offered.
const SHOWN: usize = 16 * 1024;
/// Files of one change set that are read for the window.
const FILES: u16 = 64;
/// Longest a window waits for the user. An approval expires sooner anyway.
const WAIT: Duration = Duration::from_secs(5 * 60);
/// How long Allow stays inert after the window opens.
const SETTLE: Duration = Duration::from_millis(1200);
/// How long an Allow is remembered for the very same action, so that the page sending it
/// again (the authority was busy, the answer was lost) is not asking the user twice.
const REMEMBER: Duration = Duration::from_secs(120);
const REMEMBERED: usize = 16;

/// What the user is asked, already in plain text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub heading: String,
    pub body: String,
    /// Everything that is being decided is in `body`. When it is not, Allow is not offered.
    pub complete: bool,
}

/// Who answers a `Prompt`. `true` only for an explicit Allow.
pub trait Confirmer: Send + Sync + 'static {
    fn ask(&self, prompt: &Prompt) -> bool;
}

/// The confirmation the desktop manages: one prompt at a time, whoever answers it.
#[derive(Clone)]
pub struct F8Confirm {
    answer: Arc<dyn Confirmer>,
    open: Arc<AtomicBool>,
    /// What the user allowed lately, by the exact action: never a class of actions.
    allowed: Arc<Mutex<VecDeque<(String, Instant)>>>,
}
impl F8Confirm {
    pub fn new(answer: impl Confirmer) -> Self {
        Self {
            answer: Arc::new(answer),
            open: Arc::new(AtomicBool::new(false)),
            allowed: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
    /// `ask`, except that an action the user allowed a moment ago is not asked again.
    /// `action` names it exactly: the intent and everything that was shown.
    fn ask_once(&self, action: &str, prompt: &Prompt) -> Result<bool, F8Error> {
        {
            let mut allowed = self.allowed.lock().unwrap_or_else(PoisonError::into_inner);
            allowed.retain(|(_, at)| at.elapsed() < REMEMBER);
            if allowed.iter().any(|(known, _)| known == action) {
                return Ok(true);
            }
        }
        let answer = self.ask(prompt)?;
        if answer {
            let mut allowed = self.allowed.lock().unwrap_or_else(PoisonError::into_inner);
            if allowed.len() == REMEMBERED {
                allowed.pop_front();
            }
            allowed.push_back((action.to_owned(), Instant::now()));
        }
        Ok(answer)
    }
    /// A refusal when another prompt is open: two cannot be told apart by a user. The
    /// reservation is released however the answer ends, a panic included.
    fn ask(&self, prompt: &Prompt) -> Result<bool, F8Error> {
        struct Open<'a>(&'a AtomicBool);
        impl Drop for Open<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        self.open
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| F8Error::new(F8Failure::Busy))?;
        let _open = Open(&self.open);
        Ok(self.answer.ask(prompt))
    }
}

/// The characters that would act on the text around them, show nothing, or pass for a space:
/// controls (but not the line break or tab), soft hyphen, Arabic/Syriac format marks, every
/// kind of zero-width, directional and invisible-operator character, Hangul and Braille
/// fillers, combining grapheme joiner, variation selectors, tag characters and the spaces
/// other than U+0020.
fn invisible(c: char) -> bool {
    (c.is_control() && c != '\n' && c != '\t')
        || matches!(
            c as u32,
            0x00A0
                | 0x00AD
                | 0x034F
                | 0x0600..=0x0605
                | 0x061C
                | 0x06DD
                | 0x070F
                | 0x115F
                | 0x1160
                | 0x1680
                | 0x17B4
                | 0x17B5
                | 0x180B..=0x180F
                | 0x2000..=0x200F
                | 0x2028..=0x202F
                | 0x205F..=0x206F
                | 0x2800
                | 0x3000
                | 0x3164
                | 0xFE00..=0xFE0F
                | 0xFEFF
                | 0xFFA0
                | 0xFFF9..=0xFFFB
                | 0xE0001
                | 0xE0020..=0xE007F
                | 0xE0100..=0xE01EF
        )
}
/// `text` with each such character spelled out where it stands.
pub(super) fn visible(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if invisible(c) {
            let _ = write!(out, "⟨U+{:04X}⟩", c as u32);
        } else {
            out.push(c);
        }
    }
    out
}

/// `visible`, for a command or a path: every line break is marked where it is, so that text
/// pushed below the fold by blank lines cannot pass for nothing.
pub(super) fn inline(text: &str) -> String {
    visible(text).replace('\n', "⏎\n")
}

/// What the user is told about one session, which is all the desktop knows of it by name.
fn session_line(session: &ManagedSessionId) -> String {
    format!("Session: {}", session.as_str())
}

/// The prompt for an approval, read whole from the authority. Reads every file of a change
/// set, page by page, up to what a window can show.
fn approval_prompt(gui: &F8Gui, first: &ApprovalDetail) -> Result<Prompt, F8Error> {
    let mut body = String::new();
    let mut complete = true;
    let _ = writeln!(body, "{}", session_line(&first.approval.session));
    if let Some(workspace) = &first.workspace {
        let _ = writeln!(body, "Workspace: {}", inline(workspace));
    }
    let heading;
    if let Some(command) = &first.command {
        heading = "A runtime asks to run a command";
        let _ = write!(
            body,
            "\nDirectory:\n{}\n\nCommand:\n{}\n",
            inline(&command.cwd),
            inline(&command.command)
        );
        complete = body.chars().count() <= SHOWN;
    } else if let Some(count) = first.files {
        heading = "A runtime asks to change files";
        let _ = writeln!(body, "\n{count} file(s) in this change.");
        if count > FILES {
            complete = false;
        }
        'files: for index in 0..count.min(FILES) {
            let mut offset = 0;
            loop {
                let page = gui.approval(first.approval.id.clone(), Some(index), Some(offset))?;
                let Some(ref file) = page.file else {
                    complete = false;
                    break 'files;
                };
                if offset == 0 {
                    let _ = write!(
                        body,
                        "\n── {} of {count}: {:?} {}",
                        index + 1,
                        file.kind,
                        inline(&file.path)
                    );
                    if let Some(to) = &file.move_path {
                        let _ = write!(body, " → {}", inline(to));
                    }
                    body.push('\n');
                }
                body.push_str(&visible(&file.chunk));
                if body.chars().count() > SHOWN {
                    complete = false;
                    break 'files;
                }
                match file.next {
                    Some(next) if next > offset => offset = next,
                    Some(_) => {
                        complete = false;
                        break 'files;
                    }
                    None => break,
                }
            }
        }
    } else {
        // An approval of a kind this window does not know how to show whole.
        heading = "A runtime asks for permission";
        complete = false;
    }
    if !complete {
        body = body.chars().take(SHOWN).collect();
        body.push_str("\n\n[This is more than can be shown here. It can only be declined.]");
    }
    Ok(Prompt {
        heading: heading.to_owned(),
        body,
        complete,
    })
}

/// Accepts one approval, but only after the user allowed exactly what the authority holds.
pub(super) fn accept(
    gui: &F8Gui,
    confirm: &F8Confirm,
    intent: crate::contracts::WireUuid,
    fence: FenceView,
    approval: ManagedApprovalId,
    expected_revision: u64,
) -> Result<DecisionReceipt, F8Error> {
    let first = gui.approval(approval.clone(), None, None)?;
    // What the page was shown is not what is pending any more: it reads again.
    if first.approval.revision != expected_revision {
        return Err(F8Error::new(F8Failure::Stale));
    }
    if !first.approval.decidable {
        return Err(F8Error::new(F8Failure::NotAllowed));
    }
    let prompt = approval_prompt(gui, &first)?;
    let action = format!(
        "{}|approval|{}|{}",
        intent.as_str(),
        approval.as_str(),
        first.approval.revision
    );
    if !prompt.complete || !confirm.ask_once(&action, &prompt)? {
        return Err(F8Error::new(F8Failure::NotConfirmed));
    }
    gui.approval_respond(
        intent,
        fence,
        approval,
        first.approval.revision,
        ApprovalDecision::Accept,
    )
}

fn scope_text(scope: ExecutionScope) -> &'static str {
    match scope {
        ExecutionScope::SessionCreate => "create sessions",
        ExecutionScope::SessionResume => "resume sessions",
        ExecutionScope::TurnStart => "start turns",
        ExecutionScope::CommandExecute => "run commands (each one still asks)",
        ExecutionScope::FileChange => "change files (each change still asks)",
    }
}
/// Whether a lease with these scopes lets a runtime propose to act on the machine.
pub(super) fn acts(scopes: &[ExecutionScope]) -> bool {
    scopes.iter().any(|s| {
        matches!(
            s,
            ExecutionScope::CommandExecute | ExecutionScope::FileChange
        )
    })
}
/// The prompt for a lease: the scopes and lifetime that will be sent, as sent.
pub(super) fn lease_prompt(
    renew: bool,
    session: &ManagedSessionId,
    workspace: Option<&str>,
    scopes: Option<&[ExecutionScope]>,
    ttl_seconds: u32,
) -> Prompt {
    let mut body = format!("{}\n", session_line(session));
    if let Some(workspace) = workspace {
        let _ = writeln!(body, "Workspace: {}", inline(workspace));
    }
    body.push('\n');
    if let Some(scopes) = scopes {
        body.push_str("The runtime of this session may be allowed to:\n");
        for scope in scopes {
            let _ = writeln!(body, "  • {}", scope_text(*scope));
        }
    } else {
        body.push_str("The permission this session has now stays in force.\n");
    }
    let _ = write!(
        body,
        "\nFor {} minute(s) {} second(s) from now. You can revoke it at any time.",
        ttl_seconds / 60,
        ttl_seconds % 60
    );
    Prompt {
        heading: if renew {
            "Extend what this runtime may do?".to_owned()
        } else {
            "Let this runtime ask to act?".to_owned()
        },
        body,
        complete: true,
    }
}
/// The workspace the authority holds for a session's runtime, read here and not taken from the page.
pub(super) fn workspace_of(gui: &F8Gui, fence: &FenceView) -> Result<Option<String>, F8Error> {
    let detail = gui.session(fence.session.clone())?;
    if detail.session.runtime.fence != *fence {
        return Err(F8Error::new(F8Failure::Stale));
    }
    Ok(detail.session.runtime.workspace)
}
/// Read failures and a different lease never produce an incomplete Allow prompt.
pub(super) fn lease_of(
    gui: &F8Gui,
    fence: &FenceView,
    lease: &crate::contracts::identities::ExecutionLeaseId,
) -> Result<(Option<String>, Vec<ExecutionScope>), F8Error> {
    let detail = gui.session(fence.session.clone())?;
    if detail.session.runtime.fence != *fence {
        return Err(F8Error::new(F8Failure::Stale));
    }
    let held = detail
        .lease
        .filter(|held| held.id == *lease)
        .ok_or_else(|| F8Error::new(F8Failure::Stale))?;
    Ok((detail.session.runtime.workspace, held.scopes))
}
/// Asks for a lease the user must allow. `Ok` only on Allow. `action` is the intent and what
/// the prompt says, so that the same request sent again is the same answer.
pub(super) fn allow(confirm: &F8Confirm, action: &str, prompt: &Prompt) -> Result<(), F8Error> {
    if confirm.ask_once(action, prompt)? {
        Ok(())
    } else {
        Err(F8Error::new(F8Failure::NotConfirmed))
    }
}

/// The toolkit's window. It holds the application handle once there is one.
#[derive(Default)]
pub struct GtkConfirmer {
    app: Mutex<Option<tauri::AppHandle>>,
}
impl GtkConfirmer {
    /// Given by the application once it exists; until then every prompt is refused.
    pub fn attach(&self, app: tauri::AppHandle) {
        *self.app.lock().unwrap_or_else(PoisonError::into_inner) = Some(app);
    }
}
#[cfg(target_os = "linux")]
impl Confirmer for Arc<GtkConfirmer> {
    fn ask(&self, prompt: &Prompt) -> bool {
        use tauri::Manager;
        let Some(app) = self
            .app
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        else {
            return false;
        };
        let Some(window) = app.get_webview_window("main") else {
            return false;
        };
        let (answer, answered) = std::sync::mpsc::channel();
        let host = window.clone();
        let prompt = prompt.clone();
        // Set when nobody answered in time: the window then closes itself, so that a late
        // Allow is not given to nothing and a second window is never stacked on the first.
        let expired = Arc::new(AtomicBool::new(false));
        let watch = expired.clone();
        if window
            .run_on_main_thread(move || {
                let _ = answer.send(show(host.gtk_window().ok(), &prompt, watch));
            })
            .is_err()
        {
            return false;
        }
        match answered.recv_timeout(WAIT) {
            Ok(allowed) => allowed,
            Err(_) => {
                expired.store(true, Ordering::Release);
                // Wait for the window to go, so that the next prompt starts from none.
                answered.recv_timeout(Duration::from_secs(5)).ok();
                false
            }
        }
    }
}
#[cfg(not(target_os = "linux"))]
impl Confirmer for Arc<GtkConfirmer> {
    fn ask(&self, _: &Prompt) -> bool {
        false
    }
}

/// The window itself, on GTK's thread. A modal dialog of the main window whose default is
/// not to allow. Allow is offered when what is shown is whole, a moment after the window
/// opened and once the text has been scrolled to its end: nothing below the fold is allowed
/// unseen.
#[cfg(target_os = "linux")]
fn show(parent: Option<gtk::ApplicationWindow>, prompt: &Prompt, expired: Arc<AtomicBool>) -> bool {
    use gtk::prelude::*;
    use std::{cell::Cell, rc::Rc};
    let dialog = gtk::Dialog::builder()
        .modal(true)
        .title(prompt.heading.as_str())
        .build();
    if let Some(parent) = &parent {
        dialog.set_transient_for(Some(parent));
    }
    dialog.set_default_size(680, 420);
    let deny = dialog.add_button("Don't allow", gtk::ResponseType::Cancel);
    let allow = dialog.add_button("Allow", gtk::ResponseType::Accept);
    allow.set_sensitive(false);
    let heading = gtk::Label::new(Some(prompt.heading.as_str()));
    heading.set_xalign(0.0);
    heading.set_margin(8);
    let view = gtk::TextView::new();
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_monospace(true);
    view.set_wrap_mode(gtk::WrapMode::WordChar);
    view.set_left_margin(8);
    view.set_right_margin(8);
    if let Some(buffer) = view.buffer() {
        buffer.set_text(&prompt.body);
    }
    let scroll = gtk::ScrolledWindow::builder()
        .min_content_width(640)
        .min_content_height(300)
        .expand(true)
        .build();
    scroll.add(&view);
    let content = dialog.content_area();
    content.add(&heading);
    content.add(&scroll);
    dialog.set_default_response(gtk::ResponseType::Cancel);
    dialog.show_all();
    deny.grab_focus();
    if prompt.complete {
        let settled = Rc::new(Cell::new(false));
        let adjustment = scroll.vadjustment();
        let update = {
            let (allow, settled, adjustment) =
                (allow.clone(), settled.clone(), adjustment.downgrade());
            move || {
                let Some(adjustment) = adjustment.upgrade() else {
                    return;
                };
                let at_end =
                    adjustment.value() + adjustment.page_size() >= adjustment.upper() - 1.0;
                allow.set_sensitive(settled.get() && at_end);
            }
        };
        adjustment.connect_value_changed({
            let update = update.clone();
            move |_| update()
        });
        adjustment.connect_changed({
            let update = update.clone();
            move |_| update()
        });
        // Inert for a moment: a click on what was under this window is not an answer to it.
        gtk::glib::timeout_add_local_once(SETTLE, move || {
            settled.set(true);
            update();
        });
    }
    let expiry_source = {
        // Nobody answered in time: answer for them, with no.
        let dialog = dialog.clone();
        gtk::glib::timeout_add_local(Duration::from_millis(500), move || {
            if expired.load(Ordering::Acquire) {
                dialog.response(gtk::ResponseType::Cancel);
            }
            gtk::glib::ControlFlow::Continue
        })
    };
    let accepted = dialog.run() == gtk::ResponseType::Accept;
    expiry_source.remove();
    dialog.close();
    accepted
}

#[cfg(test)]
mod tests;
