//! Authoring (F5): a hook of the user's own, and the command of one, written into Claude Code's
//! settings; and the exact way back for any authored change.
//!
//! What the user writes is data to this module: validated against closed rules, placed with
//! the same editor as every other change (a byte edit of the one place, checked against the
//! same change made to the parsed value), and kept for the undo as the exact bytes that went
//! in and the ones they replaced. Nothing here runs a command, and nothing here is reachable
//! with text the page chose: the desktop asks for the command in a window of its own.
use super::change::{Operation, Risk};
use super::classify::may_hold_secret;
use super::inventory::SourceKey;
use super::jsonspan;
use super::model::{Bounds, ProviderId};
use super::plan::{
    DiffKind, Drafted, Logical, PlanError, Prepared, draft_restore, find_hook, hash, parse,
    redact_lines,
};
use super::restore::{Anchor, Level, Protection, RecordBody, RestoreError};
use serde_json::{Value, json};

/// Claude Code's hook events, the closed list a new hook may name.
pub const HOOK_EVENTS: [&str; 12] = [
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "UserPromptSubmit",
    "Notification",
    "Stop",
    "SubagentStop",
    "PreCompact",
    "SessionStart",
    "SessionEnd",
    "PermissionRequest",
    "SubagentStart",
];
const MOST_COMMAND: usize = 4096;
const MOST_MATCHER: usize = 128;
const MOST_TIMEOUT: u32 = 3600;

/// A character that can disguise what is written when it is shown: a line or paragraph
/// separator, a bidirectional control, a zero-width or invisible format character.
pub fn disguises(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
    )
}

/// A line of text with no control character but a tab, and nothing that disguises it.
pub fn line_ok(text: &str, most: usize) -> bool {
    !text.trim().is_empty()
        && text.len() <= most
        && text
            .chars()
            .all(|c| (!c.is_control() || c == '\t') && !disguises(c))
}

/// A matcher is a tool-name pattern, chosen in the page and shown in a native window: plain
/// printable ASCII, no quote or backslash, so it can say nothing but a pattern.
fn matcher_ok(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MOST_MATCHER
        && text
            .bytes()
            .all(|b| (b == b' ' || b.is_ascii_graphic()) && b != b'"' && b != b'\\')
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookSpec {
    pub event: String,
    pub matcher: Option<String>,
    pub command: String,
    pub timeout: Option<u32>,
}

impl HookSpec {
    pub fn check(&self) -> Result<(), PlanError> {
        let ok = HOOK_EVENTS.contains(&self.event.as_str())
            && line_ok(&self.command, MOST_COMMAND)
            && self.matcher.as_deref().is_none_or(matcher_ok)
            && self.timeout.is_none_or(|t| (1..=MOST_TIMEOUT).contains(&t));
        ok.then_some(()).ok_or(PlanError::Invalid)
    }
    fn handler(&self) -> Value {
        let mut handler = json!({"type": "command", "command": self.command});
        if let Some(timeout) = self.timeout {
            handler["timeout"] = json!(timeout);
        }
        handler
    }
}

/// The record that takes a change back: the bytes that went in, the bytes they replaced, and
/// where. A change is one stretch of the file, so the common head and tail of the two files
/// are all that is not in it.
pub fn inverse_of(provider: ProviderId, source: SourceKey, old: &[u8], new: &[u8]) -> RecordBody {
    // A byte that continues a multibyte character is never where the change starts or ends.
    let inside = |bytes: &[u8], at: usize| at < bytes.len() && (bytes[at] & 0xC0) == 0x80;
    let mut head = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    while head > 0 && (inside(old, head) || inside(new, head)) {
        head -= 1;
    }
    let mut tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    while tail > 0 && (inside(old, old.len() - tail) || inside(new, new.len() - tail)) {
        tail -= 1;
    }
    let text =
        |bytes: &[u8]| String::from_utf8_lossy(&bytes[head..bytes.len() - tail]).into_owned();
    RecordBody {
        provider,
        source,
        event: "author".to_owned(),
        matcher: None,
        group: 0,
        index: 0,
        level: Level::Handler,
        anchor: Anchor::Alone,
        removed_text: text(old),
        removed_at: head,
        unit_text: String::new(),
        handler_text: text(new),
        h0: hash(old),
        h1: hash(new),
    }
}

#[allow(clippy::too_many_arguments)]
fn inverse_drafted(
    operation: Operation,
    item: &str,
    name: &str,
    old: &[u8],
    new: Vec<u8>,
    logical: Logical,
    shown: &Value,
    provider: ProviderId,
    source: SourceKey,
    secret_bearing: bool,
    protection: impl Fn(bool) -> Result<Protection, RestoreError>,
    bounds: &Bounds,
) -> Result<Drafted, PlanError> {
    let protection = protection(secret_bearing).map_err(|e| match e {
        RestoreError::NoKeyStoreForSecret => PlanError::KeyStoreUnavailable,
        _ => PlanError::NotSupported,
    })?;
    let body = inverse_of(provider, source, old, &new);
    let (diff, hidden) = redact_lines(&brief(shown).0, DiffKind::Added, bounds);
    let hidden = hidden || brief(shown).1;
    Ok(Drafted {
        operation,
        item: item.to_owned(),
        name: super::redact::text(name).text.as_str().to_owned(),
        // What the user writes here runs: the native dialog, always.
        risk: Risk::ExecutesCode,
        h0: hash(old),
        h1: hash(&new),
        new_bytes: new,
        logical,
        prepared: Prepared::Inverse {
            body: Box::new(body),
            label: super::redact::text(name).text.as_str().to_owned(),
            secret_bearing,
            protection,
        },
        diff,
        holds_hidden_values: hidden || secret_bearing,
        prior: None,
        managed: None,
    })
}

/// A new hook in a Claude Code settings file: in the group of its event that has the same
/// matcher, else a new group, else a new event, else a new `hooks` object. The item named in
/// the journal is where the handler ended up.
pub fn draft_add_hook(
    bytes: &[u8],
    source: SourceKey,
    spec: &HookSpec,
    bounds: &Bounds,
    protection: impl Fn(bool) -> Result<Protection, RestoreError>,
) -> Result<Drafted, PlanError> {
    spec.check()?;
    let (root, before) = parse(bytes)?;
    let handler = spec.handler();
    let secret_bearing = may_hold_secret(&handler, bounds);
    let (new, logical) = if root.member("hooks").is_none() {
        let hooks = json!({ spec.event.as_str(): [group(spec, &handler)] });
        let edit = jsonspan::append_child(
            bytes,
            &root,
            &jsonspan::member_text("hooks", &hooks.to_string()),
        );
        (
            edit.apply(bytes),
            Logical::SetRoot {
                key: "hooks".to_owned(),
                value: hooks,
            },
        )
    } else {
        if !before.get("hooks").is_some_and(Value::is_object) {
            return Err(PlanError::Malformed);
        }
        let handler_text = handler.to_string();
        let body = RecordBody {
            provider: ProviderId::ClaudeCode,
            source,
            event: spec.event.clone(),
            matcher: spec.matcher.clone(),
            group: 0,
            index: 0,
            level: Level::Handler,
            anchor: Anchor::Alone,
            removed_text: String::new(),
            removed_at: 0,
            unit_text: handler_text.clone(),
            handler_text,
            // Neither is this file: the exact path is never taken for a new entry.
            h0: String::new(),
            h1: String::new(),
        };
        let drafted = draft_restore(bytes, &"0".repeat(32), &body, secret_bearing, bounds)
            .map_err(|e| match e {
                // Only when the same command does run on that event is it "already done":
                // any other conflict is said as one.
                PlanError::RestoreConflict
                    if before
                        .pointer(&format!("/hooks/{}", spec.event))
                        .and_then(Value::as_array)
                        .is_some_and(|groups| {
                            groups.iter().any(|g| {
                                g.get("hooks").and_then(Value::as_array).is_some_and(|hs| {
                                    hs.iter().any(|h| {
                                        h.get("command").and_then(Value::as_str)
                                            == Some(spec.command.as_str())
                                    })
                                })
                            })
                        }) =>
                {
                    PlanError::AlreadyDone
                }
                other => other,
            })?;
        if matches!(drafted.logical, Logical::Nothing) {
            return Err(PlanError::AlreadyDone);
        }
        (drafted.new_bytes, drafted.logical)
    };
    let name = format!(
        "add hook · {} · {}",
        spec.event,
        spec.matcher.as_deref().unwrap_or("*")
    );
    let item = new_hook_item(&new, source, spec).unwrap_or_default();
    inverse_drafted(
        Operation::AddHook,
        &item,
        &name,
        bytes,
        new,
        logical,
        &match spec.timeout {
            Some(timeout) => json!({ "command": spec.command, "timeout": timeout }),
            None => json!({ "command": spec.command }),
        },
        ProviderId::ClaudeCode,
        source,
        secret_bearing,
        protection,
        bounds,
    )
}

fn group(spec: &HookSpec, handler: &Value) -> Value {
    let mut group = json!({ "hooks": [handler] });
    if let Some(matcher) = &spec.matcher {
        group["matcher"] = json!(matcher);
    }
    group
}

/// The identifier of the handler a new hook became: the last one of its event with its command.
fn new_hook_item(bytes: &[u8], source: SourceKey, spec: &HookSpec) -> Option<String> {
    let (_, value) = parse(bytes).ok()?;
    let groups = value.get("hooks")?.get(&spec.event)?.as_array()?;
    let mut found = None;
    for (g, group) in groups.iter().enumerate() {
        if group.get("matcher").and_then(Value::as_str) != spec.matcher.as_deref() {
            continue;
        }
        let Some(handlers) = group.get("hooks").and_then(Value::as_array) else {
            continue;
        };
        for (i, handler) in handlers.iter().enumerate() {
            if handler.get("command").and_then(Value::as_str) == Some(&spec.command) {
                found = Some(super::inventory::item_id_for(
                    source,
                    &[&spec.event, &g.to_string(), &i.to_string()],
                ));
            }
        }
    }
    found
}

/// The command of an existing hook handler, replaced in place. Anything else about the
/// handler, its neighbours and every comment stay as they are.
pub fn draft_edit_hook(
    bytes: &[u8],
    source: SourceKey,
    item: &str,
    command: &str,
    bounds: &Bounds,
    protection: impl Fn(bool) -> Result<Protection, RestoreError>,
) -> Result<Drafted, PlanError> {
    if !line_ok(command, MOST_COMMAND) {
        return Err(PlanError::Invalid);
    }
    let (root, before) = parse(bytes)?;
    let found = find_hook(&root, source, item).ok_or(PlanError::EntryGone)?;
    let member = found
        .handler
        .member("command")
        .ok_or(PlanError::NotSupported)?;
    let old: Value =
        serde_json::from_slice(&bytes[member.value.span().start..member.value.span().end])
            .map_err(|_| PlanError::NotSupported)?;
    if !old.is_string() {
        return Err(PlanError::NotSupported);
    }
    if old.as_str() == Some(command) {
        return Err(PlanError::AlreadyDone);
    }
    let edit = jsonspan::replace_value(
        &member.value,
        &Value::String(command.to_owned()).to_string(),
    );
    let new = edit.apply(bytes);
    let logical = Logical::SetHookCommand {
        event: found.event.clone(),
        group: found.group,
        index: found.index,
        command: command.to_owned(),
    };
    if !super::plan::is_exactly(bytes, &new, &logical)
        || super::plan::apply_logical(&before, &logical).is_none()
    {
        return Err(PlanError::Malformed);
    }
    let secret_bearing = may_hold_secret(&json!({"command": command}), bounds)
        || may_hold_secret(&json!({"command": old}), bounds);
    let name = format!("edit hook · {}", found.event);
    inverse_drafted(
        Operation::EditHook,
        item,
        &name,
        bytes,
        new,
        logical,
        &json!({ "command": command }),
        ProviderId::ClaudeCode,
        source,
        secret_bearing,
        protection,
        bounds,
    )
}

/// Takes an authored change back, exactly: only while the file is as the change left it, and
/// only if the bytes that go back make the file the one it was.
pub fn draft_revert(bytes: &[u8], record: &str, body: &RecordBody) -> Result<Drafted, PlanError> {
    if body.event != "author" {
        return Err(PlanError::RecordUnreadable);
    }
    let (at, new_len) = (body.removed_at, body.handler_text.len());
    let end = at.checked_add(new_len).ok_or(PlanError::RecordUnreadable)?;
    if hash(bytes) != body.h1 || bytes.get(at..end) != Some(body.handler_text.as_bytes()) {
        // The file moved on since: nothing is forced over what somebody changed.
        return Err(PlanError::EntryChanged);
    }
    let edit = jsonspan::Edit {
        at,
        remove: new_len,
        insert: body.removed_text.clone(),
    };
    let restored = edit.apply(bytes);
    if hash(&restored) != body.h0 {
        return Err(PlanError::RecordUnreadable);
    }
    // A record is believed only for what it was made for: the two files differ in the one
    // place an authored change writes (`hooks` for Claude Code, `mcp` for OpenCode), and
    // nowhere else. A record that would reach `permissions` or anything else is refused.
    let (Some(now), Some(then)) = (top_level(bytes), top_level(&restored)) else {
        return Err(PlanError::Malformed);
    };
    let home = if body.source == SourceKey::OpencodeUserConfig {
        "mcp"
    } else {
        "hooks"
    };
    if now
        .keys()
        .chain(then.keys())
        .any(|k| now.get(k) != then.get(k) && k != home)
    {
        return Err(PlanError::RecordUnreadable);
    }
    Ok(Drafted {
        operation: Operation::RevertAuthored,
        item: record.to_owned(),
        name: "take back an authored change".to_owned(),
        // Taking back is a change to something that runs: the native dialog, always.
        risk: Risk::ExecutesCode,
        h0: hash(bytes),
        h1: hash(&restored),
        new_bytes: restored,
        // The original file is the strongest check there is: its hash was matched above.
        logical: Logical::Nothing,
        prepared: Prepared::Restore {
            record: record.to_owned(),
            already_present: false,
        },
        diff: vec![super::plan::DiffLine {
            kind: DiffKind::Note,
            text: "the file goes back to exactly what it was".to_owned(),
        }],
        holds_hidden_values: false,
        prior: None,
        managed: None,
    })
}

/// The top-level members of a settings file, parsed as JSON or JSONC.
fn top_level(bytes: &[u8]) -> Option<serde_json::Map<String, Value>> {
    super::reader::parse_jsonc(bytes, &Bounds::PROPOSED)
        .ok()??
        .as_object()
        .cloned()
}

/// What the page is shown of something the user typed: each text cut short, and a flag that
/// something was left out. The whole text is in the native window that asked for it.
pub fn brief(value: &Value) -> (Value, bool) {
    const MOST: usize = 60;
    match value {
        Value::String(text) if text.chars().count() > MOST => {
            let cut: String = text.chars().take(MOST).collect();
            (Value::String(format!("{cut}…")), true)
        }
        Value::Array(items) => {
            let mut cut = false;
            let items = items
                .iter()
                .map(|v| {
                    let (v, c) = brief(v);
                    cut |= c;
                    v
                })
                .collect();
            (Value::Array(items), cut)
        }
        Value::Object(map) => {
            let mut cut = false;
            let map = map
                .iter()
                .map(|(k, v)| {
                    let (v, c) = brief(v);
                    cut |= c;
                    (k.clone(), v)
                })
                .collect();
            (Value::Object(map), cut)
        }
        other => (other.clone(), false),
    }
}
