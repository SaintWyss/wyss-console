//! `wyss-console --diagnose` (F13): what an installed Console can say about itself without a
//! window. Names, counts, versions and permissions; never a value from a configuration file or a
//! key. Each line starts with `ok`, `note` or `WARN`, so a person or a script can read it.
use crate::{
    agentconfig::{SecretToolStore, Vault},
    host::{client::HostLink, protocol::HostError},
    native::catalogue::{self, AGENTS},
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

fn mode_of(path: &Path) -> Option<u32> {
    fs::symlink_metadata(path)
        .ok()
        .map(|m| m.permissions().mode() & 0o777)
}

fn count(dir: &Path) -> usize {
    fs::read_dir(dir).map_or(0, |d| {
        d.flatten()
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .count()
    })
}

pub fn report() -> Vec<String> {
    let mut out = Vec::new();
    let mut line = |level: &str, text: String| out.push(format!("{level:<4} {text}"));
    line(
        "ok",
        format!(
            "Wyss Console {} (revision {})",
            env!("CARGO_PKG_VERSION"),
            option_env!("WYSS_BUILD_REVISION").unwrap_or("unknown")
        ),
    );
    if let Ok(exe) = std::env::current_exe() {
        line("ok", format!("running from {}", exe.display()));
    }
    let Some(home) = home() else {
        line(
            "WARN",
            "HOME is not set: nothing else can be checked".to_owned(),
        );
        return out;
    };

    // The host.
    match HostLink::hello_only() {
        Ok(hello) => line(
            "ok",
            format!(
                "host running: pid {}, version {}, protocol {}",
                hello["pid"], hello["version"], hello["protocol"]
            ),
        ),
        Err(HostError { code, .. }) if code == "host_unavailable" => line(
            "note",
            "no host is running (the window starts one; it keeps terminals and agents when the window closes)".to_owned(),
        ),
        Err(error) => line("WARN", format!("the host answered with an error: {}", error.code)),
    }

    // Agents.
    let path_var = std::env::var_os("PATH");
    for spec in &AGENTS {
        match catalogue::discover(spec, path_var.clone(), &home) {
            Some(found) => line(
                "ok",
                format!(
                    "{} found at {} ({})",
                    spec.name,
                    found.display(),
                    catalogue::probe_version(&found)
                        .unwrap_or_else(|| "version unknown".to_owned())
                ),
            ),
            None => line("note", format!("{} is not installed", spec.name)),
        }
    }
    match wyss_console::terminal::TerminalService::user_shell() {
        Ok(shell) => line("ok", format!("login shell {}", shell.display())),
        Err(_) => line("WARN", "no usable login shell".to_owned()),
    }

    // The key store for sealed records.
    if Vault::new(Box::new(SecretToolStore::new())).available() {
        line(
            "ok",
            "the Secret Service answers: sealed records and API keys can be kept".to_owned(),
        );
    } else {
        line("WARN", "no Secret Service: a hook cannot be turned off individually if it holds a secret, and no API key can be kept".to_owned());
    }

    // Its own state.
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".local/state"))
        .join("wyss-console");
    if state.is_dir() {
        match mode_of(&state) {
            Some(mode) if mode & 0o077 == 0 => line(
                "ok",
                format!("state directory {} (mode {mode:o})", state.display()),
            ),
            Some(mode) => line(
                "WARN",
                format!(
                    "state directory {} is open to others (mode {mode:o})",
                    state.display()
                ),
            ),
            None => {}
        }
        line(
            "ok",
            format!(
                "{} native session records, {} profiles, {} API keys, layout {}",
                fs::read(state.join("native/registry/registry.json"))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                    .and_then(|v| v["sessions"].as_array().map(Vec::len))
                    .unwrap_or(0),
                count(&state.join("native/profiles")),
                count(&state.join("credentials")),
                if state.join("layout.json").is_file() {
                    "saved"
                } else {
                    "none"
                },
            ),
        );
    } else {
        line(
            "note",
            format!("no state directory yet ({})", state.display()),
        );
    }

    // The agents' own folders: present and who can write them, never what is in them.
    for (name, dir) in [
        ("Claude Code", home.join(".claude")),
        ("Codex", home.join(".codex")),
    ] {
        match mode_of(&dir) {
            Some(mode) if mode & 0o002 != 0 => line(
                "WARN",
                format!("{name}'s folder {} is writable by everyone", dir.display()),
            ),
            Some(mode) => line(
                "ok",
                format!("{name}'s folder {} (mode {mode:o})", dir.display()),
            ),
            None => line("note", format!("{name} has no folder yet")),
        }
    }
    out
}

/// Prints the report; the exit code is 1 when anything is a warning.
pub fn run() -> i32 {
    let lines = report();
    for line in &lines {
        println!("{line}");
    }
    i32::from(lines.iter().any(|l| l.starts_with("WARN")))
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_report_names_the_version_and_holds_no_value_of_the_environment() {
        let lines = super::report();
        assert!(lines[0].starts_with("ok") && lines[0].contains(env!("CARGO_PKG_VERSION")));
        let all = lines.join("\n");
        for secret in ["ANTHROPIC_API_KEY", "TOKEN", "PASSWORD"] {
            assert!(!all.contains(secret), "{secret}");
        }
        assert!(
            lines
                .iter()
                .all(|l| l.starts_with("ok") || l.starts_with("note") || l.starts_with("WARN"))
        );
    }
}
