//! Shared configuration path for the native interface.

use std::path::PathBuf;

/// `$XDG_CONFIG_HOME` (or `~/.config`) `/wyss-console/preferences.json`.
pub fn path() -> Option<PathBuf> {
    path_from(std::env::var_os("XDG_CONFIG_HOME"), std::env::var_os("HOME"))
}

pub fn path_from(xdg_config_home: Option<std::ffi::OsString>, home: Option<std::ffi::OsString>) -> Option<PathBuf> {
    let abs = |v: Option<std::ffi::OsString>| v.map(PathBuf::from).filter(|p| p.is_absolute());
    let config = abs(xdg_config_home).or_else(|| abs(home).map(|h| h.join(".config")))?;
    Some(config.join("wyss-console/preferences.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_is_found_under_xdg_config_home_or_dot_config() {
        assert_eq!(
            path_from(Some("/home/a/custom".into()), Some("/home/a".into())),
            Some(PathBuf::from("/home/a/custom/wyss-console/preferences.json"))
        );
        assert_eq!(
            path_from(Some("relative".into()), Some("/home/a".into())),
            Some(PathBuf::from("/home/a/.config/wyss-console/preferences.json"))
        );
        assert_eq!(path_from(None, None), None);
    }
}
