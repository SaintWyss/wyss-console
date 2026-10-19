//! Planning a change to OpenCode's `opencode.jsonc` (F4a): the switch of an MCP server
//! (`mcp.<name>.enabled`), and taking a plugin out of the `plugin` list and putting it back.
//!
//! Every change is a byte edit made from the spans of the user's own file, so comments, key
//! order, spacing and every other entry stay the bytes they were. It is checked against the
//! same change made to the parsed value; a stretch that would take a comment with it, a
//! repeated key, or a shape the editor does not place with certainty is refused and the file
//! is not rewritten as a whole. `provider.*.options` (the API keys) is never read here: no
//! function in this module looks at it, and nothing it returns can hold it.
use super::change::Prior;
use super::inventory::{SourceKey, item_id_for};
use super::jsonspan::{self, Edit, Node};
use super::model::{Bounds, ProviderId};
use super::plan::{PlanError, hash};
use super::reader::parse_jsonc as parse_value;
use super::restore::{Level, RecordBody};
use serde_json::Value;

/// What the plan does to the value tree, for checking the byte edit.
#[derive(Clone, PartialEq)]
pub enum Logical {
    /// `mcp.<name>.enabled` becomes this, or is removed (`None`).
    McpEnabled {
        name: String,
        value: Option<bool>,
    },
    RemovePlugin {
        index: usize,
    },
    /// The entry goes back at this place in the list.
    InsertPlugin {
        index: usize,
        unit: Value,
    },
    AppendPlugin {
        unit: Value,
    },
    /// `plugin` did not exist: it is created with this one entry.
    CreatePlugins {
        unit: Value,
    },
    Nothing,
}

/// A plugin entry may hold options with a key: what is inside it is never printed.
impl std::fmt::Debug for Logical {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::McpEnabled { name, value } => {
                write!(f, "McpEnabled({} chars, {value:?})", name.len())
            }
            Self::RemovePlugin { index } => write!(f, "RemovePlugin({index})"),
            Self::InsertPlugin { index, .. } => write!(f, "InsertPlugin({index}, ..)"),
            Self::AppendPlugin { .. } => write!(f, "AppendPlugin(..)"),
            Self::CreatePlugins { .. } => write!(f, "CreatePlugins(..)"),
            Self::Nothing => write!(f, "Nothing"),
        }
    }
}

pub fn apply_logical(before: &Value, edit: &Logical) -> Option<Value> {
    let mut root = before.clone();
    match edit {
        Logical::Nothing => {}
        Logical::McpEnabled { name, value } => {
            let entry = root.get_mut("mcp")?.get_mut(name)?.as_object_mut()?;
            match value {
                Some(on) => {
                    entry.insert("enabled".to_owned(), Value::Bool(*on));
                }
                None => {
                    entry.remove("enabled")?;
                }
            }
        }
        Logical::RemovePlugin { index } => {
            let plugins = root.get_mut("plugin")?.as_array_mut()?;
            if *index >= plugins.len() {
                return None;
            }
            plugins.remove(*index);
        }
        Logical::InsertPlugin { index, unit } => {
            let plugins = root.get_mut("plugin")?.as_array_mut()?;
            if *index > plugins.len() {
                return None;
            }
            plugins.insert(*index, unit.clone());
        }
        Logical::AppendPlugin { unit } => {
            root.get_mut("plugin")?.as_array_mut()?.push(unit.clone());
        }
        Logical::CreatePlugins { unit } => {
            let map = root.as_object_mut()?;
            if map.contains_key("plugin") {
                return None;
            }
            map.insert("plugin".to_owned(), Value::Array(vec![unit.clone()]));
        }
    }
    Some(root)
}

/// Whether `after` is `before` with exactly this edit made and nothing else.
pub fn is_exactly(before: &[u8], after: &[u8], edit: &Logical) -> bool {
    let bounds = Bounds::PROPOSED;
    let (Ok(Some(before)), Ok(Some(after))) =
        (parse_value(before, &bounds), parse_value(after, &bounds))
    else {
        return false;
    };
    apply_logical(&before, edit).is_some_and(|expected| expected == after)
}

fn parse(bytes: &[u8]) -> Result<(Node, Value), PlanError> {
    let node = jsonspan::parse_jsonc(bytes).map_err(|_| PlanError::Malformed)?;
    let value = parse_value(bytes, &Bounds::PROPOSED)
        .map_err(|_| PlanError::Malformed)?
        .ok_or(PlanError::Malformed)?;
    Ok((node, value))
}

/// A plugin entry's spec: the string, or the first of `[spec, options]`.
fn spec_of(value: &Value) -> Option<&str> {
    match value {
        Value::String(spec) => Some(spec),
        Value::Array(pair) => pair.first().and_then(Value::as_str),
        _ => None,
    }
}

/// Which entry of `plugin` an item identifier names.
pub fn resolve_plugin(
    bytes: &[u8],
    key: SourceKey,
    item: &str,
) -> Result<(usize, String), PlanError> {
    let (_, value) = parse(bytes)?;
    let plugins = value
        .get("plugin")
        .and_then(Value::as_array)
        .ok_or(PlanError::EntryGone)?;
    let mut found = plugins.iter().enumerate().filter_map(|(index, plugin)| {
        let spec = spec_of(plugin)?;
        (item_id_for(key, &["plugin", spec]) == item).then(|| (index, spec.to_owned()))
    });
    let first = found.next().ok_or(PlanError::EntryGone)?;
    // The same plugin twice is two entries the identifier cannot tell apart: which one the
    // user meant is not guessed.
    if found.next().is_some() {
        return Err(PlanError::NotSupported);
    }
    Ok(first)
}

/// Which server of `mcp` an item identifier names.
pub fn resolve_server(bytes: &[u8], key: SourceKey, item: &str) -> Result<String, PlanError> {
    let (_, value) = parse(bytes)?;
    value
        .get("mcp")
        .and_then(Value::as_object)
        .ok_or(PlanError::EntryGone)?
        .keys()
        .find(|name| item_id_for(key, &[name]) == item)
        .cloned()
        .ok_or(PlanError::EntryGone)
}

pub struct Planned {
    pub new_bytes: Vec<u8>,
    pub logical: Logical,
    pub prior: Prior,
}

/// The switch of one MCP server. `restore` puts the key back to what it held (an undo).
pub fn draft_mcp(
    bytes: &[u8],
    name: &str,
    on: bool,
    restore: Option<Prior>,
) -> Result<Planned, PlanError> {
    let (root, before) = parse(bytes)?;
    let mcp = root
        .member("mcp")
        .map(|m| &m.value)
        .filter(|n| n.members().is_some())
        .ok_or(PlanError::EntryGone)?;
    let entry = mcp
        .member(name)
        .map(|m| &m.value)
        .filter(|n| n.members().is_some())
        .ok_or(PlanError::EntryGone)?;
    let current = before
        .pointer(&format!(
            "/mcp/{}/enabled",
            name.replace('~', "~0").replace('/', "~1")
        ))
        .map(|v| v.as_bool().ok_or(PlanError::EntryChanged))
        .transpose()?;
    let wanted: Option<bool> = match restore {
        Some(Prior::Absent) => None,
        Some(Prior::Enabled) => Some(true),
        Some(Prior::Disabled) => Some(false),
        None => Some(on),
    };
    let already = match restore {
        Some(_) => current == wanted,
        None => current.unwrap_or(true) == wanted.unwrap_or(true),
    };
    if already {
        return Err(PlanError::AlreadyDone);
    }
    // An undo only puts back what its change left; anything else was changed by somebody.
    if restore.is_some() && current != Some(!on) {
        return Err(PlanError::EntryChanged);
    }
    let prior = match current {
        None => Prior::Absent,
        Some(true) => Prior::Enabled,
        Some(false) => Prior::Disabled,
    };
    let member = entry.member("enabled");
    let edit = match (wanted, member) {
        (None, Some(member)) => {
            let at = entry
                .members()
                .and_then(|m| m.iter().position(|x| x.key == "enabled"))
                .ok_or(PlanError::Malformed)?;
            let _ = member;
            jsonspan::remove_child_checked(bytes, entry, at)
                .ok_or(PlanError::NotSupported)?
                .edit
        }
        (None, None) => return Err(PlanError::AlreadyDone),
        (Some(on), Some(member)) => {
            jsonspan::replace_value(&member.value, if on { "true" } else { "false" })
        }
        (Some(on), None) => jsonspan::append_child_lenient(
            bytes,
            entry,
            &jsonspan::member_text("enabled", if on { "true" } else { "false" }),
        )
        .ok_or(PlanError::NotSupported)?,
    };
    let new_bytes = edit.apply(bytes);
    let logical = Logical::McpEnabled {
        name: name.to_owned(),
        value: wanted,
    };
    if !is_exactly(bytes, &new_bytes, &logical) || apply_logical(&before, &logical).is_none() {
        return Err(PlanError::Malformed);
    }
    Ok(Planned {
        new_bytes,
        logical,
        prior,
    })
}

pub struct Parked {
    pub new_bytes: Vec<u8>,
    pub logical: Logical,
    pub body: RecordBody,
    pub unit: Value,
}

/// Takes the plugin at `index` out of the list, keeping what is needed to put it back.
pub fn draft_park_plugin(bytes: &[u8], key: SourceKey, index: usize) -> Result<Parked, PlanError> {
    let (root, before) = parse(bytes)?;
    let plugins = root
        .member("plugin")
        .map(|m| &m.value)
        .filter(|n| n.items().is_some())
        .ok_or(PlanError::EntryGone)?;
    let removal =
        jsonspan::remove_child_checked(bytes, plugins, index).ok_or(PlanError::NotSupported)?;
    // A comment inside the entry itself makes this fail: it is refused, not guessed at.
    let unit: Value = serde_json::from_str(&removal.unit).map_err(|_| PlanError::Malformed)?;
    let new_bytes = removal.edit.apply(bytes);
    let logical = Logical::RemovePlugin { index };
    if !is_exactly(bytes, &new_bytes, &logical) || apply_logical(&before, &logical).is_none() {
        return Err(PlanError::Malformed);
    }
    let body = RecordBody {
        provider: ProviderId::Opencode,
        source: key,
        event: "plugin".to_owned(),
        matcher: None,
        group: 0,
        index,
        level: Level::Handler,
        anchor: removal.anchor,
        removed_text: removal.removed,
        removed_at: removal.edit.at,
        unit_text: removal.unit.clone(),
        handler_text: removal.unit,
        h0: hash(bytes),
        h1: hash(&new_bytes),
    };
    Ok(Parked {
        new_bytes,
        logical,
        body,
        unit,
    })
}

pub struct Restored {
    pub new_bytes: Vec<u8>,
    pub logical: Logical,
    pub already_present: bool,
    pub unit: Value,
}

/// Puts a parked plugin back: exactly where it was when the file is as the removal left it,
/// otherwise at the end of the list, and refused when the same plugin is back already or a
/// different entry of the same name is in the way. A record is believed only as far as it can
/// be checked: its source, its removed text against its entry, and the result against the value
/// the same insertion makes.
pub fn draft_restore_plugin(bytes: &[u8], body: &RecordBody) -> Result<Restored, PlanError> {
    if body.source != SourceKey::OpencodeUserConfig || body.event != "plugin" {
        return Err(PlanError::RecordUnreadable);
    }
    let (root, before) = parse(bytes)?;
    let unit: Value =
        serde_json::from_str(&body.unit_text).map_err(|_| PlanError::RecordUnreadable)?;
    let spec = spec_of(&unit)
        .ok_or(PlanError::RecordUnreadable)?
        .to_owned();
    let same_spec: Vec<&Value> = before
        .get("plugin")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|p| spec_of(p) == Some(&spec))
        .collect();
    if same_spec.iter().any(|p| **p == unit) {
        return Ok(Restored {
            new_bytes: bytes.to_vec(),
            logical: Logical::Nothing,
            already_present: true,
            unit,
        });
    }
    // The same plugin with other options is a different entry in the same place.
    if !same_spec.is_empty() {
        return Err(PlanError::RestoreConflict);
    }
    // Exact: the file is as the removal left it, so the bytes go back where they were.
    let clean = body
        .removed_text
        .trim_matches(&[' ', '\t', '\r', '\n', ','][..])
        == body.unit_text;
    if clean && hash(bytes) == body.h1 && body.removed_at <= bytes.len() {
        let edit = Edit {
            at: body.removed_at,
            remove: 0,
            insert: body.removed_text.clone(),
        };
        let restored = edit.apply(bytes);
        let logical = Logical::InsertPlugin {
            index: body.index,
            unit: unit.clone(),
        };
        if hash(&restored) == body.h0 && is_exactly(bytes, &restored, &logical) {
            return Ok(Restored {
                new_bytes: restored,
                logical,
                already_present: false,
                unit,
            });
        }
    }
    // Structural: against the file as it is.
    let (edit, logical) = match root.member("plugin") {
        Some(member) if member.value.items().is_some() => (
            jsonspan::append_child_lenient(bytes, &member.value, &body.unit_text)
                .ok_or(PlanError::RestoreConflict)?,
            Logical::AppendPlugin { unit: unit.clone() },
        ),
        Some(_) => return Err(PlanError::RestoreConflict),
        None => (
            jsonspan::append_child_lenient(
                bytes,
                &root,
                &jsonspan::member_text("plugin", &format!("[{}]", body.unit_text)),
            )
            .ok_or(PlanError::RestoreConflict)?,
            Logical::CreatePlugins { unit: unit.clone() },
        ),
    };
    let new_bytes = edit.apply(bytes);
    if !is_exactly(bytes, &new_bytes, &logical) || apply_logical(&before, &logical).is_none() {
        return Err(PlanError::RestoreConflict);
    }
    Ok(Restored {
        new_bytes,
        logical,
        already_present: false,
        unit,
    })
}

use super::change::{Operation, Risk};
use super::plan::{DiffKind, DiffLine, Drafted, Logical as PlanLogical, Prepared, redact_lines};
use super::redact;
use super::restore::{Protection, RestoreError};

fn label(name: &str) -> String {
    redact::text(name).text.as_str().to_owned()
}

/// An MCP switch as a drafted plan. On starts a program, so it needs the native dialog.
pub fn drafted_mcp(
    bytes: &[u8],
    item: &str,
    name: &str,
    shown_name: &str,
    on: bool,
    restore: Option<Prior>,
) -> Result<Drafted, PlanError> {
    let planned = draft_mcp(bytes, name, on, restore)?;
    let word = |on: bool| if on { "true" } else { "false" };
    let was = match planned.prior {
        Prior::Absent => "(not set, so on)",
        Prior::Enabled => "true",
        Prior::Disabled => "false",
    };
    let diff = match planned.logical {
        Logical::McpEnabled {
            value: Some(now), ..
        } => vec![
            DiffLine {
                kind: DiffKind::Removed,
                text: format!("enabled: {was}"),
            },
            DiffLine {
                kind: DiffKind::Added,
                text: format!("enabled: {}", word(now)),
            },
        ],
        _ => vec![DiffLine {
            kind: DiffKind::Removed,
            text: "enabled: (the line is removed)".to_owned(),
        }],
    };
    let operation = if on {
        Operation::OpencodeMcpOn
    } else {
        Operation::OpencodeMcpOff
    };
    Ok(Drafted {
        operation,
        item: item.to_owned(),
        name: label(shown_name),
        risk: if on { Risk::ExecutesCode } else { Risk::Normal },
        h0: hash(bytes),
        h1: hash(&planned.new_bytes),
        new_bytes: planned.new_bytes,
        logical: PlanLogical::Opencode(planned.logical),
        prepared: Prepared::Nothing,
        diff,
        holds_hidden_values: false,
        prior: Some(planned.prior),
        managed: None,
    })
}

/// A plugin taken out of the list as a drafted plan.
pub fn drafted_park(
    bytes: &[u8],
    key: SourceKey,
    item: &str,
    index: usize,
    bounds: &Bounds,
    protection: impl Fn(bool) -> Result<Protection, RestoreError>,
) -> Result<Drafted, PlanError> {
    let parked = draft_park_plugin(bytes, key, index)?;
    // A plugin with options is a plausible place for a key: it is always sealed.
    let secret_bearing = matches!(parked.unit, Value::Array(_))
        || super::classify::may_hold_secret(&parked.unit, bounds);
    let protection = protection(secret_bearing).map_err(|e| match e {
        RestoreError::NoKeyStoreForSecret => PlanError::KeyStoreUnavailable,
        _ => PlanError::NotSupported,
    })?;
    let name = spec_of(&parked.unit)
        .map(plugin_label)
        .unwrap_or_else(|| "plugin".to_owned());
    let shown = label(&format!("plugin · {name}"));
    let (diff, hidden) = redact_lines(&parked.unit, DiffKind::Removed, bounds);
    Ok(Drafted {
        operation: Operation::ParkPlugin,
        item: item.to_owned(),
        name: shown.clone(),
        risk: Risk::Normal,
        h0: parked.body.h0.clone(),
        h1: parked.body.h1.clone(),
        new_bytes: parked.new_bytes,
        logical: PlanLogical::Opencode(parked.logical),
        prepared: Prepared::Park {
            body: Box::new(parked.body),
            label: shown,
            secret_bearing,
            protection,
        },
        diff,
        holds_hidden_values: hidden || secret_bearing,
        prior: None,
        managed: None,
    })
}

/// A parked plugin put back, as a drafted plan. A plugin is code that runs in every OpenCode
/// session: the native dialog.
pub fn drafted_restore(
    bytes: &[u8],
    record: &str,
    body: &RecordBody,
    secret_bearing: bool,
    bounds: &Bounds,
) -> Result<Drafted, PlanError> {
    let restored = draft_restore_plugin(bytes, body)?;
    let name = spec_of(&restored.unit)
        .map(plugin_label)
        .unwrap_or_else(|| "plugin".to_owned());
    let (diff, hidden) = redact_lines(&restored.unit, DiffKind::Added, bounds);
    // Already back: nothing to write, nothing to undo, and the record is kept.
    if restored.already_present {
        return Err(PlanError::AlreadyDone);
    }
    Ok(Drafted {
        operation: Operation::RestoreHook,
        item: record.to_owned(),
        name: label(&format!("plugin · {name}")),
        risk: Risk::ExecutesCode,
        h0: hash(bytes),
        h1: hash(&restored.new_bytes),
        new_bytes: restored.new_bytes,
        logical: PlanLogical::Opencode(restored.logical),
        prepared: Prepared::Restore {
            record: record.to_owned(),
            already_present: false,
        },
        diff,
        holds_hidden_values: hidden || secret_bearing,
        prior: None,
        managed: None,
    })
}

/// A plugin's short name for display: the last path segment without an extension.
pub fn plugin_label(spec: &str) -> String {
    let last = spec
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(spec);
    last.strip_suffix(".ts")
        .or_else(|| last.strip_suffix(".js"))
        .unwrap_or(last)
        .to_owned()
}
