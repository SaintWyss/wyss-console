//! The Claude launch overlay and its local, allowlisted callback output.
use super::{prompt_queue::Turn, store, usage};
use nix::{
    fcntl::{Flock, FlockArg},
    libc,
    unistd::geteuid,
};
use serde::{Deserialize, Deserializer, de::IgnoredAny};
use serde_json::{Value, json};
use std::{
    ffi::{OsStr, OsString},
    fs::{self, OpenOptions},
    io::{self, BufReader, Read},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::Command,
};
use uuid::Uuid;

pub const LAUNCH: &str = "--claude-activity-launch";
pub const RECEIVE: &str = "--claude-activity-receive";
const MAX_INPUT: u64 = 256 * 1024;
const SETTINGS: [&str; 2] = ["settings.json", "settings.local.json"];

fn private(dir: &Path) -> bool {
    dir.is_absolute()
        && fs::symlink_metadata(dir)
            .is_ok_and(|m| m.is_dir() && m.uid() == geteuid().as_raw() && m.mode() & 0o777 == 0o700)
}

fn session_id(dir: &Path) -> Option<Uuid> {
    dir.file_name()?.to_str()?.parse().ok()
}

fn command(exe: &Path, dir: &Path, channel: &str) -> Option<String> {
    let quote = |arg: &OsStr| {
        arg.to_str()
            .map(|s| format!("'{}'", s.replace('\'', "'\\''")))
    };
    Some(format!(
        "/usr/bin/env -i {} {RECEIVE} {} {}",
        quote(exe.as_os_str())?,
        quote(dir.as_os_str())?,
        quote(OsStr::new(channel))?
    ))
}

#[derive(Deserialize)]
struct StatusKey {
    #[serde(rename = "statusLine", default, deserialize_with = "present")]
    status: bool,
}

fn present<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    IgnoredAny::deserialize(d).map(|_| true)
}

fn status_key(path: &Path) -> Option<bool> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.uid() != geteuid().as_raw() || meta.len() > MAX_INPUT {
        return None;
    }
    // Only check for a top-level field. Deserialization skips values, including environment
    // settings, and never retains or emits them.
    serde_json::from_reader::<_, StatusKey>(BufReader::new(file.take(MAX_INPUT + 1)))
        .ok()
        .map(|v| v.status)
}

fn owner_status(config: &Path, project: &Path) -> bool {
    [config.to_owned()]
        .into_iter()
        .chain(project.ancestors().map(|p| p.join(".claude")))
        .any(|root| {
            SETTINGS.iter().any(|name| {
                let p = root.join(name);
                match fs::symlink_metadata(&p) {
                    Err(e) if e.kind() == io::ErrorKind::NotFound => false,
                    // Ambiguous files count as occupied. Managed keys retain CLI precedence.
                    _ => status_key(&p).unwrap_or(true),
                }
            })
        })
}

fn compose(mut overlay: Value, status: Option<&str>, events: &str) -> Option<Value> {
    let map = overlay.as_object_mut()?;
    if let Some(command) = status {
        map.entry("statusLine")
            .or_insert_with(|| json!({ "type": "command", "command": command }));
    }
    let hooks = map
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()?;
    for (event, matcher) in [
        ("UserPromptSubmit", ""),
        ("Stop", ""),
        ("StopFailure", ""),
        ("SessionEnd", ""),
        ("PermissionRequest", ""),
        ("PreToolUse", "AskUserQuestion|ExitPlanMode"),
        ("Elicitation", ""),
        (
            "Notification",
            "idle_prompt|permission_prompt|elicitation_dialog|elicitation_url_dialog|agent_needs_input",
        ),
    ] {
        hooks
            .entry(event)
            .or_insert_with(|| json!([]))
            .as_array_mut()?
            .push(json!({
                "matcher": matcher,
                "hooks": [{"type": "command", "command": events, "timeout": 2}]
            }));
    }
    Some(overlay)
}

/// Runs after the shell's startup files so the status-line decision sees its effective config
/// directory. A failed overlay falls back to the original arguments and gives no dispatch signal.
pub fn launch(args: &[OsString]) -> io::Error {
    let [dir, program, original @ ..] = args else {
        return io::Error::new(
            io::ErrorKind::InvalidInput,
            "missing Claude launch arguments",
        );
    };
    let prepared = (|| {
        let exe = std::env::current_exe().ok()?;
        let cwd = std::env::current_dir().ok()?;
        let config = std::env::var_os("CLAUDE_CONFIG_DIR")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude")))?;
        prepare(Path::new(dir), &exe, &config, &cwd, original)
    })();
    Command::new(program)
        .args(prepared.as_deref().unwrap_or(original))
        .exec()
}

pub(super) fn prepare(
    dir: &Path,
    exe: &Path,
    config: &Path,
    cwd: &Path,
    original: &[OsString],
) -> Option<Vec<OsString>> {
    if !private(dir) {
        return None;
    }
    let session_id = session_id(dir)?;
    let owned = owner_status(config, cwd);
    let position = original.iter().position(|arg| arg == "--settings");
    let settings = if let Some(i) = position {
        let path = dir.join("settings.json");
        if original.get(i + 1)? != path.as_os_str() {
            return None;
        }
        serde_json::from_slice(&store::read_bounded(&path, MAX_INPUT)?).ok()?
    } else {
        json!({})
    };
    // Sin barra de estado propia: Claude Code le reserva un renglón vacío bajo el prompt.
    let _ = owned;
    let overlay = compose(settings, None, &command(exe, dir, "events")?)?;
    let bytes = serde_json::to_vec(&overlay).ok()?;
    let path = dir.join("activity-settings.json");
    store::write_atomic(&path, &bytes).ok()?;
    let mut words = original.to_vec();
    if let Some(i) = position {
        words[i + 1] = path.into_os_string();
    } else {
        words.splice(..0, ["--settings".into(), path.into_os_string()]);
    }
    words.splice(..0, ["--session-id".into(), session_id.to_string().into()]);
    Some(words)
}

#[derive(Deserialize)]
struct HookInput {
    session_id: String,
    hook_event_name: String,
    notification_type: Option<String>,
    tool_name: Option<String>,
}

fn hook(raw: &[u8], id: Uuid) -> Option<String> {
    let data: HookInput = serde_json::from_slice(raw).ok()?;
    if data.session_id.parse::<Uuid>().ok()? != id {
        return None;
    }
    let name = data.hook_event_name.as_str();
    let event = if name == "Notification" {
        data.notification_type.as_deref()?
    } else {
        name
    };
    match event {
        "UserPromptSubmit"
        | "Stop"
        | "StopFailure"
        | "SessionEnd"
        | "PermissionRequest"
        | "Elicitation"
        | "idle_prompt"
        | "permission_prompt"
        | "elicitation_dialog"
        | "elicitation_url_dialog"
        | "agent_needs_input" => Some(event.to_owned()),
        "PreToolUse"
            if matches!(
                data.tool_name.as_deref(),
                Some("AskUserQuestion" | "ExitPlanMode")
            ) =>
        {
            Some(event.to_owned())
        }
        _ => None,
    }
}

pub(super) fn turn(dir: &Path) -> Option<Turn> {
    store::read_bounded(&dir.join("turn.json"), 1024).and_then(|b| serde_json::from_slice(&b).ok())
}

fn write_turn(dir: &Path, event: &str) -> Option<()> {
    // These hooks can overlap. Serialize the read/modify/write without retaining the input JSON.
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(dir.join("turn.lock"))
        .ok()?;
    if !file
        .metadata()
        .is_ok_and(|m| m.is_file() && m.uid() == geteuid().as_raw())
    {
        return None;
    }
    let _guard = Flock::lock(file, FlockArg::LockExclusive).ok()?;
    let event = if event == "Elicitation" {
        "elicitation_dialog"
    } else {
        event
    };
    let state = Turn::event(turn(dir), event, store::now())?;
    store::write_atomic(&dir.join("turn.json"), &serde_json::to_vec(&state).ok()?).ok()
}

/// The provider supplies these JSON objects on stdin. Only recognized numeric readings or a
/// fixed lifecycle enum leave the helper. It produces no stdout and modifies no CLI file.
pub fn receive(args: &[OsString], stdin: impl Read) {
    let [dir, channel] = args else { return };
    let dir = Path::new(dir);
    if !private(dir) {
        return;
    }
    let Some(id) = session_id(dir) else { return };
    let mut data = Vec::new();
    if stdin.take(MAX_INPUT + 1).read_to_end(&mut data).is_err() || data.len() as u64 > MAX_INPUT {
        return;
    }
    match channel.to_str() {
        Some("usage") => {
            if let Some(reading) = usage::claude_reading(&data, id, store::now())
                && let Ok(bytes) = serde_json::to_vec(&reading)
            {
                let _ = store::write_atomic(&dir.join("usage.json"), &bytes);
            }
        }
        Some("events") => {
            if let Some(event) = hook(&data, id) {
                write_turn(dir, &event);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_status_and_hooks_survive() {
        let previous = json!({"statusLine":{"type":"command","command":"original"},
            "hooks":{"Stop":[{"matcher":"","hooks":[{"type":"command","command":"original stop"}]}]}});
        let next = compose(previous.clone(), Some("wyss"), "monitor").unwrap();
        assert_eq!(next["statusLine"], previous["statusLine"]);
        assert_eq!(next["hooks"]["Stop"][0], previous["hooks"]["Stop"][0]);
        assert_eq!(next["hooks"]["Stop"][1]["hooks"][0]["command"], "monitor");
        assert!(compose(json!({"hooks":{"Stop":"unknown"}}), None, "monitor").is_none());
    }

    #[test]
    fn lifecycle_checks_session_and_known_events() {
        let id = Uuid::new_v4();
        let raw = json!({"session_id":id.to_string(),"hook_event_name":"Notification","notification_type":"idle_prompt"});
        assert_eq!(
            hook(raw.to_string().as_bytes(), id).as_deref(),
            Some("idle_prompt")
        );
        assert!(hook(raw.to_string().as_bytes(), Uuid::new_v4()).is_none());
        assert!(hook(b"{", id).is_none());
        assert!(
            hook(
                json!({"session_id":id.to_string(),"hook_event_name":"NewEvent"})
                    .to_string()
                    .as_bytes(),
                id
            )
            .is_none()
        );
    }
}
