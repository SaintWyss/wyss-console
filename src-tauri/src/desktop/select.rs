//! Which protocol-2 authority, if any, this desktop is started against.
//!
//! **Only the command line of whoever starts the desktop selects one**, with the one flag
//! `--f8-authority <runtime directory>`: the directory in which that authority keeps its
//! private socket. No environment variable, configuration file, window request or discovery
//! does, so a desktop started the way the launcher starts it (no arguments) is inactive and
//! sends nothing. A malformed selection is an error that stops the start; it is never read
//! as "no selection", and it never turns into protocol 1.
use crate::f8::gui::{F8Gui, SocketConnector};
use std::{ffi::OsString, path::PathBuf};

/// The flag that selects an authority.
pub const FLAG: &str = "--f8-authority";
/// Longest runtime directory accepted: the socket path under it must fit a unix address.
const LONGEST: usize = 80;

/// The selection the arguments (without the program name) make: inactive, or active against
/// the named authority's directory.
pub fn from_arguments(args: impl IntoIterator<Item = OsString>) -> Result<F8Gui, String> {
    let mut args = args.into_iter();
    if args.next().is_none_or(|first| first != FLAG) {
        return Ok(F8Gui::inactive());
    }
    let root = match (args.next(), args.next()) {
        (Some(root), None) => PathBuf::from(root),
        _ => return Err(format!("{FLAG} takes exactly one directory")),
    };
    let text = root.to_string_lossy();
    if !root.is_absolute()
        || root.as_os_str().len() > LONGEST
        || text.contains('\0')
        || root
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(format!(
            "{FLAG}: the directory must be an absolute path without `..`"
        ));
    }
    Ok(F8Gui::local(SocketConnector::new(root)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn select(args: &[&str]) -> Result<bool, String> {
        from_arguments(args.iter().map(OsString::from)).map(|gui| gui.is_active())
    }

    #[test]
    fn no_flag_is_inactive_and_other_arguments_select_nothing() {
        assert_eq!(select(&[]), Ok(false));
        assert_eq!(select(&["--unknown"]), Ok(false));
        assert_eq!(select(&["/some/file"]), Ok(false));
    }

    #[test]
    fn a_named_absolute_directory_makes_it_active() {
        assert_eq!(select(&[FLAG, "/run/user/1000/f8"]), Ok(true));
    }

    #[test]
    fn a_malformed_selection_is_an_error_not_an_inactive_desktop() {
        for args in [
            &[FLAG][..],
            &[FLAG, "relative/dir"],
            &[FLAG, "/a/../b"],
            &[FLAG, "/a", "extra"],
            &[FLAG, ""],
        ] {
            assert!(select(args).is_err(), "{args:?}");
        }
        let long = format!("/{}", "d".repeat(100));
        assert!(select(&[FLAG, &long]).is_err());
    }
}
