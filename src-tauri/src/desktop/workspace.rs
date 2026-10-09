//! Choosing a workspace: one native folder dialog, and the record of what it answered.
//!
//! The window is given no filesystem. It can ask for this dialog, which the user answers
//! in a native window the page cannot draw on or read, and it gets back the one folder
//! the user selected or nothing. It cannot list a directory, read or write a file, or
//! name a path of its own: an activation is accepted only for a folder this dialog
//! returned in this run of the application.
//!
//! A folder chosen here is a request. Whether it may be a workspace is decided by
//! `wyss-control` when the binding is asked for.
use super::sessions::{SessionError, SessionFailure};
use crate::contracts::control::validate_workspace_text;
use serde::Serialize;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::State;

/// Folders remembered. An activation in flight needs one; a few more cover a change of mind.
const REMEMBERED: usize = 8;

/// What the native dialog returned during this run, newest last.
#[derive(Default)]
pub struct WorkspaceSelections {
    chosen: Mutex<VecDeque<String>>,
    /// One dialog at a time.
    open: AtomicBool,
}
impl WorkspaceSelections {
    /// Whether the user selected exactly this folder in the dialog.
    pub fn chose(&self, path: &str) -> bool {
        self.chosen().iter().any(|chosen| chosen == path)
    }
    fn chosen(&self) -> std::sync::MutexGuard<'_, VecDeque<String>> {
        self.chosen.lock().unwrap_or_else(PoisonError::into_inner)
    }
    fn remember(&self, path: String) {
        let mut chosen = self.chosen();
        chosen.retain(|known| known != &path);
        if chosen.len() == REMEMBERED {
            chosen.pop_front();
        }
        chosen.push_back(path);
    }
    #[cfg(test)]
    pub fn remember_for_test(&self, path: &str) {
        self.remember(path.to_owned());
    }
    fn begin(&self) -> Option<Open<'_>> {
        self.open
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Open(&self.open))
    }
}
struct Open<'a>(&'a AtomicBool);
impl Drop for Open<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// The folder the user selected, as text. Nothing else about it crosses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceChoice {
    pub path: String,
}

/// What the dialog answered, as text the authority can be asked about. A folder whose
/// path the control protocol cannot carry is refused here instead of being cut to fit.
fn selection(picked: Option<PathBuf>) -> Result<Option<String>, SessionError> {
    let Some(path) = picked else {
        return Ok(None);
    };
    path.into_os_string()
        .into_string()
        .ok()
        .filter(|text| validate_workspace_text(text).is_ok())
        .map(Some)
        .ok_or(SessionError::new(SessionFailure::WorkspaceInvalid))
}

/// Shows the native folder dialog and answers what the user selected, or null when the
/// dialog was dismissed. The request takes nothing: the page cannot suggest a folder.
#[tauri::command]
pub async fn choose_workspace(
    window: tauri::WebviewWindow,
    selections: State<'_, WorkspaceSelections>,
) -> Result<Option<WorkspaceChoice>, SessionError> {
    let _open = selections
        .begin()
        .ok_or(SessionError::new(SessionFailure::Busy))?;
    let picked = pick_folder(window)
        .await
        .map_err(|()| SessionError::new(SessionFailure::Internal))?;
    Ok(selection(picked)?.map(|path| {
        selections.remember(path.clone());
        WorkspaceChoice { path }
    }))
}

#[cfg(target_os = "linux")]
thread_local! {
    /// GTK does not keep a native dialog alive: this does, until it is answered.
    static DIALOG: std::cell::RefCell<Option<gtk::FileChooserNative>> =
        const { std::cell::RefCell::new(None) };
}

/// GTK's own folder chooser, modal to the main window, on GTK's thread. It can select a
/// folder and nothing else.
#[cfg(target_os = "linux")]
async fn pick_folder(window: tauri::WebviewWindow) -> Result<Option<PathBuf>, ()> {
    use gtk::prelude::*;
    let (answer, mut answered) = tauri::async_runtime::channel::<Option<PathBuf>>(1);
    let host = window.clone();
    window
        .run_on_main_thread(move || {
            let parent = host.gtk_window().ok();
            let dialog = gtk::FileChooserNative::new(
                Some("Select project folder"),
                parent.as_ref(),
                gtk::FileChooserAction::SelectFolder,
                Some("Select"),
                Some("Cancel"),
            );
            dialog.set_modal(true);
            dialog.connect_response(move |dialog, response| {
                let chosen = (response == gtk::ResponseType::Accept)
                    .then(|| dialog.filename())
                    .flatten();
                let _ = answer.try_send(chosen);
                // Released once this signal has returned.
                gtk::glib::idle_add_local_once(|| {
                    DIALOG.with(|open| drop(open.borrow_mut().take()));
                });
            });
            dialog.show();
            DIALOG.with(|open| *open.borrow_mut() = Some(dialog));
        })
        .map_err(|_| ())?;
    answered.recv().await.ok_or(())
}
#[cfg(not(target_os = "linux"))]
async fn pick_folder(_: tauri::WebviewWindow) -> Result<Option<PathBuf>, ()> {
    Err(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_path_the_protocol_can_carry_is_a_selection() {
        assert_eq!(selection(None), Ok(None));
        assert_eq!(
            selection(Some("/home/user/project".into())),
            Ok(Some("/home/user/project".to_owned()))
        );
        let refused = Err(SessionError::new(SessionFailure::WorkspaceInvalid));
        for path in [
            PathBuf::from("relative/project"),
            PathBuf::from(""),
            PathBuf::from(format!("/{}", "x".repeat(256))),
            PathBuf::from("/with\nnewline"),
        ] {
            assert_eq!(selection(Some(path.clone())), refused, "{path:?}");
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let not_text = std::ffi::OsString::from_vec(b"/not\xffutf8".to_vec());
            assert_eq!(selection(Some(not_text.into())), refused);
        }
    }

    #[test]
    fn selections_are_exact_bounded_and_one_dialog_is_open_at_a_time() {
        let selections = WorkspaceSelections::default();
        assert!(!selections.chose("/a"));
        selections.remember("/a".into());
        assert!(selections.chose("/a"));
        // Exactly the text that was returned: no prefix, parent, child or other spelling.
        for other in ["/", "/a/", "/a/b", "/A", "a", "/a/../a", ""] {
            assert!(!selections.chose(other), "{other}");
        }
        for n in 0..REMEMBERED {
            selections.remember(format!("/p{n}"));
        }
        // The oldest is forgotten; choosing a folder again keeps it.
        assert!(!selections.chose("/a"));
        assert!(selections.chose("/p0"));
        selections.remember("/p0".into());
        selections.remember("/new".into());
        assert!(selections.chose("/p0") && !selections.chose("/p1"));
        assert_eq!(selections.chosen().len(), REMEMBERED);

        let open = selections.begin();
        assert!(open.is_some());
        assert!(selections.begin().is_none());
        drop(open);
        assert!(selections.begin().is_some());
    }
}
