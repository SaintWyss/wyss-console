//! Planning a change (07 §5.1): from the bytes of a file and what the user asked, the bytes
//! the file would have, what to show of it, how risky it is, and what has to be kept to
//! undo it. Nothing here touches the disk.
//!
//! Every plan carries the same change made twice: once to the bytes, by `jsonspan`, and once
//! to the parsed value, as a `Logical` edit. The plan is offered only if the two agree, and
//! the writer checks them again on what it read back. The first protects the user's layout
//! and every byte that is not the change; the second protects against a mistake in the first.
use super::change::{Operation, Prior, Risk};
use super::classify::may_hold_secret;
use super::inventory::{SourceKey, item_id_for};
use super::jsonspan::{self, Edit, Node, Removal, SpanError};
use super::model::{Bounds, ContentHash, ProviderId};
use super::redact;
use super::restore::{Level, Protection, RecordBody, RestoreError};
use serde::Serialize;
use serde_json::Value;

/// Events whose hooks can stop an action: switching one off weakens a safeguard (11 §4).
const BLOCKING_EVENTS: [&str; 3] = ["PreToolUse", "PermissionRequest", "UserPromptSubmit"];
const MOST_DIFF_LINES: usize = 120;
const MOST_DIFF_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanError {
    /// Not an operation this provider or source supports in this phase.
    NotSupported,
    /// The file is not valid JSON, or has a layout the editor will not guess at.
    Malformed,
    EntryGone,
    EntryChanged,
    /// The change is already what the file says.
    AlreadyDone,
    /// A different entry sits where the restored one would go.
    RestoreConflict,
    /// The entry may hold a secret and the key store is unavailable (ADR 0034).
    KeyStoreUnavailable,
    RecordUnreadable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffKind {
    Removed,
    Added,
    Note,
}

/// One line of what the plan shows. Always redacted: the diff is never the file's text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffLine {
    pub kind: DiffKind,
    pub text: String,
}

/// The change as a value-level edit, for checking the byte-level one.
#[derive(Debug, Clone, PartialEq)]
pub enum Logical {
    RemoveHook {
        event: String,
        group: usize,
        index: usize,
        level: Level,
    },
    SetRoot {
        key: String,
        value: Value,
    },
    RemoveRoot {
        key: String,
    },
    SetNested {
        key: String,
        name: String,
        value: Value,
    },
    RemoveNested {
        key: String,
        name: String,
    },
    InsertUnit {
        level: Level,
        event: String,
        matcher: Option<String>,
        unit: Value,
    },
    /// Nothing changes (a restoration whose entry is already there).
    Nothing,
    /// A change to a TOML file (Codex): checked by `codex_plan` against the parsed value.
    Toml(super::codex_plan::Logical),
}

/// What `apply` has to do besides write the file.
#[derive(Debug, Clone, PartialEq)]
pub enum Prepared {
    Nothing,
    /// Keep this entry before it leaves the file.
    Park {
        body: Box<RecordBody>,
        label: String,
        secret_bearing: bool,
        protection: Protection,
    },
    /// Put back what a record holds, and mark it used.
    Restore {
        record: String,
        already_present: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Drafted {
    pub operation: Operation,
    pub item: String,
    pub name: String,
    pub risk: Risk,
    pub h0: String,
    pub h1: String,
    pub new_bytes: Vec<u8>,
    pub logical: Logical,
    pub prepared: Prepared,
    pub diff: Vec<DiffLine>,
    pub holds_hidden_values: bool,
    /// What a switch's key held before this change (Codex), for an exact undo.
    pub prior: Option<Prior>,
    /// What this change does to managed Inference, as a prediction (Codex).
    pub managed: Option<super::codex_plan::Prediction>,
}

pub fn hash(bytes: &[u8]) -> String {
    ContentHash::of(bytes).hex()
}

fn parse(bytes: &[u8]) -> Result<(Node, Value), PlanError> {
    let node = jsonspan::parse(bytes).map_err(|_: SpanError| PlanError::Malformed)?;
    let value = serde_json::from_slice(bytes).map_err(|_| PlanError::Malformed)?;
    Ok((node, value))
}

/// The value after a logical edit; `None` when the edit does not apply to this value.
pub fn apply_logical(before: &Value, edit: &Logical) -> Option<Value> {
    let mut value = before.clone();
    let root = value.as_object_mut()?;
    match edit {
        Logical::Nothing => (),
        Logical::Toml(_) => return None,
        Logical::SetRoot { key, value } => {
            root.insert(key.clone(), value.clone());
        }
        Logical::RemoveRoot { key } => {
            root.remove(key)?;
        }
        Logical::SetNested { key, name, value } => {
            let inner = root
                .entry(key.clone())
                .or_insert_with(|| Value::Object(serde_json::Map::new()))
                .as_object_mut()?;
            inner.insert(name.clone(), value.clone());
        }
        Logical::RemoveNested { key, name } => {
            root.get_mut(key)?.as_object_mut()?.remove(name)?;
        }
        Logical::RemoveHook {
            event,
            group,
            index,
            level,
        } => {
            let hooks = root.get_mut("hooks")?.as_object_mut()?;
            match level {
                Level::Event => {
                    hooks.remove(event)?;
                }
                Level::Group => {
                    hooks.get_mut(event)?.as_array_mut()?.remove(*group);
                }
                Level::Handler => {
                    let groups = hooks.get_mut(event)?.as_array_mut()?;
                    groups
                        .get_mut(*group)?
                        .get_mut("hooks")?
                        .as_array_mut()?
                        .remove(*index);
                }
            }
        }
        Logical::InsertUnit {
            level,
            event,
            matcher,
            unit,
        } => {
            let hooks = root.get_mut("hooks")?.as_object_mut()?;
            match level {
                Level::Event => {
                    hooks.insert(event.clone(), unit.clone());
                }
                Level::Group => {
                    match hooks.get_mut(event) {
                        Some(groups) => groups.as_array_mut()?.push(unit.clone()),
                        None => {
                            hooks.insert(event.clone(), Value::Array(vec![unit.clone()]));
                        }
                    };
                }
                Level::Handler => {
                    let group_for = |matcher: &Option<String>, handler: &Value| {
                        let mut group = serde_json::Map::new();
                        if let Some(matcher) = matcher {
                            group.insert("matcher".into(), Value::String(matcher.clone()));
                        }
                        group.insert("hooks".into(), Value::Array(vec![handler.clone()]));
                        Value::Object(group)
                    };
                    match hooks.get_mut(event).map(Value::as_array_mut) {
                        None => {
                            hooks.insert(
                                event.clone(),
                                Value::Array(vec![group_for(matcher, unit)]),
                            );
                        }
                        Some(None) => return None,
                        Some(Some(groups)) => {
                            let existing = groups.iter_mut().find(|g| {
                                g.get("matcher").and_then(Value::as_str) == matcher.as_deref()
                                    && g.get("hooks").is_some_and(Value::is_array)
                            });
                            match existing {
                                Some(group) => {
                                    group.get_mut("hooks")?.as_array_mut()?.push(unit.clone())
                                }
                                None => groups.push(group_for(matcher, unit)),
                            }
                        }
                    }
                }
            }
        }
    }
    Some(value)
}

/// Whether `after` is `before` with `edit` made to it, and nothing else.
pub fn is_exactly(before: &[u8], after: &[u8], edit: &Logical) -> bool {
    let (Ok(before), Ok(after)) = (
        serde_json::from_slice::<Value>(before),
        serde_json::from_slice::<Value>(after),
    ) else {
        return false;
    };
    apply_logical(&before, edit).is_some_and(|expected| expected == after)
}

/// The lines a plan shows for an entry: its parsed value through the redaction that
/// protects the page, one line per leaf. The file's own text is never what is shown: a
/// secret inside a quoted string is only recognised once the string has been read.
fn redact_lines(unit: &Value, kind: DiffKind, bounds: &Bounds) -> (Vec<DiffLine>, bool) {
    use super::redact::{Projection, project};
    fn walk(value: &Projection, name: &str, out: &mut Vec<(String, bool)>) {
        match value {
            Projection::Null => out.push((format!("{name}: null"), false)),
            Projection::Bool(b) => out.push((format!("{name}: {b}"), false)),
            Projection::Number(n) => out.push((format!("{name}: {n}"), false)),
            Projection::Text(t) => {
                out.push((format!("{name}: {}", t.text.as_str()), t.may_hold_secret()))
            }
            Projection::Hidden(_) => out.push((format!("{name}: [hidden]"), true)),
            Projection::List(items) => items
                .iter()
                .enumerate()
                .for_each(|(i, item)| walk(item, &format!("{name}[{i}]"), out)),
            Projection::Map(entries) => entries.iter().for_each(|(key, item)| {
                let path = if name.is_empty() {
                    key.as_str().to_owned()
                } else {
                    format!("{name}.{}", key.as_str())
                };
                walk(item, &path, out);
            }),
        }
    }
    let Ok(projection) = project(unit, bounds) else {
        return (
            vec![DiffLine {
                kind: DiffKind::Note,
                text: "(too deep to show)".to_owned(),
            }],
            true,
        );
    };
    let mut leaves = Vec::new();
    walk(&projection, "", &mut leaves);
    let (mut lines, mut hidden, mut bytes) = (Vec::new(), false, 0);
    for (text, secret) in leaves {
        hidden |= secret;
        bytes += text.len();
        if lines.len() >= MOST_DIFF_LINES || bytes > MOST_DIFF_BYTES {
            lines.push(DiffLine {
                kind: DiffKind::Note,
                text: "…".to_owned(),
            });
            break;
        }
        lines.push(DiffLine { kind, text });
    }
    (lines, hidden)
}

/// One hook handler as found in a file.
struct Found<'a> {
    event_index: usize,
    event: String,
    group: usize,
    index: usize,
    hooks: &'a Node,
    groups: &'a Node,
    handlers: &'a Node,
    handler: &'a Node,
}

fn find_hook<'a>(root: &'a Node, source: SourceKey, item: &str) -> Option<Found<'a>> {
    let hooks = &root.member("hooks")?.value;
    for (event_index, event) in hooks.members()?.iter().enumerate() {
        let Some(groups) = event.value.items() else {
            continue;
        };
        for (group, node) in groups.iter().enumerate() {
            let Some(handlers) = node.member("hooks").map(|m| &m.value) else {
                continue;
            };
            for (index, handler) in handlers.items()?.iter().enumerate() {
                let (g, i) = (group.to_string(), index.to_string());
                if item_id_for(source, &[&event.key, &g, &i]) == item {
                    return Some(Found {
                        event_index,
                        event: event.key.clone(),
                        group,
                        index,
                        hooks,
                        groups: &event.value,
                        handlers,
                        handler,
                    });
                }
            }
        }
    }
    None
}

fn text_at(bytes: &[u8], node: &Node) -> String {
    let span = node.span();
    String::from_utf8_lossy(&bytes[span.start..span.end]).into_owned()
}

fn matcher_of(bytes: &[u8], group: &Node) -> Option<String> {
    let node = &group.member("matcher")?.value;
    serde_json::from_slice::<String>(text_at(bytes, node).as_bytes()).ok()
}

/// Parks a hook: Claude Code has no switch for one, so it leaves the file and is kept.
/// `protection` answers how an entry that may or may not hold a secret would be kept.
pub fn draft_park(
    bytes: &[u8],
    source: SourceKey,
    item: &str,
    bounds: &Bounds,
    protection: impl Fn(bool) -> Result<Protection, RestoreError>,
) -> Result<Drafted, PlanError> {
    let (root, before) = parse(bytes)?;
    let found = find_hook(&root, source, item).ok_or(PlanError::EntryGone)?;
    let group_nodes = found.groups.items().ok_or(PlanError::EntryGone)?;
    let group = &group_nodes[found.group];
    let matcher = matcher_of(bytes, group);
    let handler_text = text_at(bytes, found.handler);

    let (level, removal): (Level, Removal) = if found.handlers.items().map_or(0, <[Node]>::len) > 1
    {
        (
            Level::Handler,
            jsonspan::remove_child(bytes, found.handlers, found.index),
        )
    } else if group_nodes.len() > 1 {
        (
            Level::Group,
            jsonspan::remove_child(bytes, found.groups, found.group),
        )
    } else {
        (
            Level::Event,
            jsonspan::remove_child(bytes, found.hooks, found.event_index),
        )
    }
    .into_pair()
    .ok_or(PlanError::Malformed)?;

    let unit_value: Value = match level {
        Level::Handler | Level::Group => serde_json::from_str(&removal.unit),
        Level::Event => serde_json::from_str(&format!("{{{}}}", removal.unit)),
    }
    .map_err(|_| PlanError::Malformed)?;
    let secret_bearing = may_hold_secret(&unit_value, bounds);
    let protection = protection(secret_bearing).map_err(|e| match e {
        RestoreError::NoKeyStoreForSecret => PlanError::KeyStoreUnavailable,
        _ => PlanError::NotSupported,
    })?;

    let new_bytes = removal.edit.apply(bytes);
    let logical = Logical::RemoveHook {
        event: found.event.clone(),
        group: found.group,
        index: found.index,
        level,
    };
    if !is_exactly(bytes, &new_bytes, &logical) || apply_logical(&before, &logical).is_none() {
        return Err(PlanError::Malformed);
    }
    let (h0, h1) = (hash(bytes), hash(&new_bytes));
    let name = format!("{} · {}", found.event, matcher.as_deref().unwrap_or("*"));
    let label = redact::text(&name).text.as_str().to_owned();
    let (diff, hidden) = redact_lines(&unit_value, DiffKind::Removed, bounds);
    let body = RecordBody {
        provider: ProviderId::ClaudeCode,
        source,
        event: found.event.clone(),
        matcher,
        group: found.group,
        index: found.index,
        level,
        anchor: removal.anchor,
        removed_text: removal.removed,
        removed_at: removal.edit.at,
        unit_text: removal.unit,
        handler_text,
        h0: h0.clone(),
        h1: h1.clone(),
    };
    let risk = if BLOCKING_EVENTS.contains(&found.event.as_str()) {
        Risk::WeakensSecurity
    } else {
        Risk::Normal
    };
    Ok(Drafted {
        operation: Operation::ParkHook,
        item: item.to_owned(),
        name: label.clone(),
        risk,
        h0,
        h1,
        new_bytes,
        logical,
        prepared: Prepared::Park {
            body: Box::new(body),
            label,
            secret_bearing,
            protection,
        },
        diff,
        holds_hidden_values: hidden || secret_bearing,
        prior: None,
        managed: None,
    })
}

trait IntoPair {
    fn into_pair(self) -> Option<(Level, Removal)>;
}
impl IntoPair for (Level, Option<Removal>) {
    fn into_pair(self) -> Option<(Level, Removal)> {
        self.1.map(|removal| (self.0, removal))
    }
}

/// Puts a parked hook back. Exact when the file is as the removal left it; otherwise
/// planned against the file as it is, and refused when a different entry is in the way.
pub fn draft_restore(
    bytes: &[u8],
    record: &str,
    body: &RecordBody,
    secret_bearing: bool,
    bounds: &Bounds,
) -> Result<Drafted, PlanError> {
    let (root, before) = parse(bytes)?;
    let handler: Value =
        serde_json::from_str(&body.handler_text).map_err(|_| PlanError::RecordUnreadable)?;
    let unit: Value = match body.level {
        Level::Handler | Level::Group => serde_json::from_str(&body.unit_text),
        Level::Event => serde_json::from_str(&format!("{{{}}}", body.unit_text)),
    }
    .map_err(|_| PlanError::RecordUnreadable)?;
    let unit = match (body.level, unit) {
        (Level::Event, Value::Object(mut member)) => member
            .remove(&body.event)
            .ok_or(PlanError::RecordUnreadable)?,
        (_, other) => other,
    };
    let name = format!(
        "{} · {}",
        body.event,
        body.matcher.as_deref().unwrap_or("*")
    );
    let label = redact::text(&name).text.as_str().to_owned();
    // An event's member is shown under its event's name, as the parked hook's diff is.
    let shown = if body.level == Level::Event {
        serde_json::json!({ body.event.as_str(): unit.clone() })
    } else {
        unit.clone()
    };
    let (diff, hidden) = redact_lines(&shown, DiffKind::Added, bounds);
    let h0 = hash(bytes);
    let finish = |new_bytes: Vec<u8>, logical: Logical, already: bool| -> Drafted {
        Drafted {
            operation: Operation::RestoreHook,
            item: record.to_owned(),
            name: label.clone(),
            risk: Risk::ExecutesCode,
            h1: hash(&new_bytes),
            h0: h0.clone(),
            new_bytes,
            logical,
            prepared: Prepared::Restore {
                record: record.to_owned(),
                already_present: already,
            },
            diff: diff.clone(),
            holds_hidden_values: hidden || secret_bearing,
            prior: None,
            managed: None,
        }
    };

    // The same entry is already in the file: nothing to write.
    if let Some(groups) = before
        .get("hooks")
        .and_then(|h| h.get(&body.event))
        .and_then(Value::as_array)
    {
        let same_matcher =
            |g: &&Value| g.get("matcher").and_then(Value::as_str) == body.matcher.as_deref();
        let handlers = |g: &Value| {
            g.get("hooks")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        };
        if groups
            .iter()
            .filter(same_matcher)
            .any(|g| handlers(g).contains(&handler))
        {
            return Ok(finish(bytes.to_vec(), Logical::Nothing, true));
        }
        // The same command with other options is a different entry in the same place.
        let command = handler.get("command");
        if command.is_some()
            && groups
                .iter()
                .filter(same_matcher)
                .any(|g| handlers(g).iter().any(|h| h.get("command") == command))
        {
            return Err(PlanError::RestoreConflict);
        }
    }

    // Exact: the file is as the removal left it, so the bytes go back where they were.
    if hash(bytes) == body.h1 && body.removed_at <= bytes.len() {
        let edit = Edit {
            at: body.removed_at,
            remove: 0,
            insert: body.removed_text.clone(),
        };
        let restored = edit.apply(bytes);
        if hash(&restored) == body.h0 {
            let logical = Logical::InsertUnit {
                level: body.level,
                event: body.event.clone(),
                matcher: body.matcher.clone(),
                unit: if body.level == Level::Handler {
                    handler
                } else {
                    unit
                },
            };
            // The original file is the strongest check there is; the logical edit is only
            // recorded so that the writer can say the same thing of what it reads back.
            let mut drafted = finish(restored, logical, false);
            drafted.logical = Logical::Nothing;
            return Ok(drafted);
        }
    }

    // Structural: against the file as it is.
    let hooks = root.member("hooks").map(|m| &m.value);
    let Some(hooks) = hooks.filter(|h| h.members().is_some()) else {
        return Err(PlanError::RestoreConflict);
    };
    let event = hooks.member(&body.event);
    let (edit, logical) = match (body.level, event) {
        (Level::Event, None) => (
            jsonspan::append_child(bytes, hooks, &body.unit_text),
            Logical::InsertUnit {
                level: Level::Event,
                event: body.event.clone(),
                matcher: body.matcher.clone(),
                unit,
            },
        ),
        // The event is there again: the one handler the removal took goes into it as a handler.
        (Level::Event, Some(member)) => {
            let handler_logical = Logical::InsertUnit {
                level: Level::Handler,
                event: body.event.clone(),
                matcher: body.matcher.clone(),
                unit: handler,
            };
            let groups = member.value.items().ok_or(PlanError::RestoreConflict)?;
            let target = groups.iter().find(|g| {
                matcher_of(bytes, g) == body.matcher
                    && g.member("hooks").is_some_and(|h| h.value.items().is_some())
            });
            match target {
                Some(group) => (
                    jsonspan::append_child(
                        bytes,
                        &group
                            .member("hooks")
                            .ok_or(PlanError::RestoreConflict)?
                            .value,
                        &body.handler_text,
                    ),
                    handler_logical,
                ),
                None => {
                    let matcher_part = body
                        .matcher
                        .as_ref()
                        .map(|m| {
                            format!(
                                "\"matcher\": {}, ",
                                serde_json::to_string(m).unwrap_or_default()
                            )
                        })
                        .unwrap_or_default();
                    (
                        jsonspan::append_child(
                            bytes,
                            &member.value,
                            &format!("{{{matcher_part}\"hooks\": [{}]}}", body.handler_text),
                        ),
                        handler_logical,
                    )
                }
            }
        }
        (Level::Group, Some(member)) => (
            jsonspan::append_child(bytes, &member.value, &body.unit_text),
            Logical::InsertUnit {
                level: Level::Group,
                event: body.event.clone(),
                matcher: body.matcher.clone(),
                unit,
            },
        ),
        (Level::Group, None) => (
            jsonspan::append_child(
                bytes,
                hooks,
                &jsonspan::member_text(&body.event, &format!("[{}]", body.unit_text)),
            ),
            Logical::InsertUnit {
                level: Level::Group,
                event: body.event.clone(),
                matcher: body.matcher.clone(),
                unit,
            },
        ),
        (Level::Handler, member) => {
            let group_text = |matcher: &Option<String>| {
                let matcher_part = matcher
                    .as_ref()
                    .map(|m| {
                        format!(
                            "\"matcher\": {}, ",
                            serde_json::to_string(m).unwrap_or_default()
                        )
                    })
                    .unwrap_or_default();
                format!("{{{matcher_part}\"hooks\": [{}]}}", body.handler_text)
            };
            let logical = Logical::InsertUnit {
                level: Level::Handler,
                event: body.event.clone(),
                matcher: body.matcher.clone(),
                unit: handler,
            };
            match member {
                None => (
                    jsonspan::append_child(
                        bytes,
                        hooks,
                        &jsonspan::member_text(
                            &body.event,
                            &format!("[{}]", group_text(&body.matcher)),
                        ),
                    ),
                    logical,
                ),
                Some(member) => {
                    let groups = member.value.items().ok_or(PlanError::RestoreConflict)?;
                    let target = groups.iter().find(|g| {
                        matcher_of(bytes, g) == body.matcher
                            && g.member("hooks").is_some_and(|h| h.value.items().is_some())
                    });
                    match target {
                        Some(group) => (
                            jsonspan::append_child(
                                bytes,
                                &group
                                    .member("hooks")
                                    .ok_or(PlanError::RestoreConflict)?
                                    .value,
                                &body.handler_text,
                            ),
                            logical,
                        ),
                        None => (
                            jsonspan::append_child(
                                bytes,
                                &member.value,
                                &group_text(&body.matcher),
                            ),
                            logical,
                        ),
                    }
                }
            }
        }
    };
    let restored = edit.apply(bytes);
    if !is_exactly(bytes, &restored, &logical) {
        return Err(PlanError::RestoreConflict);
    }
    Ok(finish(restored, logical, false))
}

/// `disableAllHooks`: Claude Code's own coarse switch. On sets it; off removes it.
pub fn draft_all_hooks(bytes: &[u8], source: SourceKey, on: bool) -> Result<Drafted, PlanError> {
    let (root, before) = parse(bytes)?;
    let current = before.get("disableAllHooks");
    let (operation, risk) = if on {
        (Operation::DisableAllHooks, Risk::WeakensSecurity)
    } else {
        (Operation::EnableAllHooks, Risk::ExecutesCode)
    };
    let (edit, logical, diff) = match (on, current, root.member("disableAllHooks")) {
        (true, Some(Value::Bool(true)), _) | (false, None | Some(Value::Bool(false)), _) => {
            return Err(PlanError::AlreadyDone);
        }
        (true, None, _) => (
            jsonspan::append_child(
                bytes,
                &root,
                &jsonspan::member_text("disableAllHooks", "true"),
            ),
            Logical::SetRoot {
                key: "disableAllHooks".into(),
                value: Value::Bool(true),
            },
            vec![DiffLine {
                kind: DiffKind::Added,
                text: "\"disableAllHooks\": true".into(),
            }],
        ),
        (true, Some(Value::Bool(false)), Some(member)) => (
            jsonspan::replace_value(&member.value, "true"),
            Logical::SetRoot {
                key: "disableAllHooks".into(),
                value: Value::Bool(true),
            },
            vec![
                DiffLine {
                    kind: DiffKind::Removed,
                    text: "\"disableAllHooks\": false".into(),
                },
                DiffLine {
                    kind: DiffKind::Added,
                    text: "\"disableAllHooks\": true".into(),
                },
            ],
        ),
        (false, Some(Value::Bool(true)), Some(_)) => {
            let index = root
                .members()
                .and_then(|m| m.iter().position(|m| m.key == "disableAllHooks"))
                .ok_or(PlanError::Malformed)?;
            let removal =
                jsonspan::remove_child(bytes, &root, index).ok_or(PlanError::Malformed)?;
            (
                removal.edit,
                Logical::RemoveRoot {
                    key: "disableAllHooks".into(),
                },
                vec![DiffLine {
                    kind: DiffKind::Removed,
                    text: "\"disableAllHooks\": true".into(),
                }],
            )
        }
        _ => return Err(PlanError::EntryChanged),
    };
    let new_bytes = edit.apply(bytes);
    if !is_exactly(bytes, &new_bytes, &logical) {
        return Err(PlanError::Malformed);
    }
    Ok(Drafted {
        operation,
        item: item_id_for(source, &["disableAllHooks"]),
        name: "All hooks".to_owned(),
        risk,
        h0: hash(bytes),
        h1: hash(&new_bytes),
        new_bytes,
        logical,
        prepared: Prepared::Nothing,
        diff,
        holds_hidden_values: false,
        prior: None,
        managed: None,
    })
}

/// `skillOverrides.<name>`: "off" switches a skill off; switching it on removes the override.
pub fn draft_skill(bytes: &[u8], item: &str, name: &str, off: bool) -> Result<Drafted, PlanError> {
    let (root, before) = parse(bytes)?;
    let current = before.get("skillOverrides").and_then(|o| o.get(name));
    let overrides = root.member("skillOverrides");
    let (operation, risk) = if off {
        (Operation::SkillOff, Risk::Normal)
    } else {
        (Operation::SkillOn, Risk::Normal)
    };
    let quoted = serde_json::to_string(name).map_err(|_| PlanError::Malformed)?;
    let line = |value: &str| format!("{quoted}: {value}");
    let (edit, logical, diff) = match (off, current) {
        (true, Some(Value::String(v))) if v == "off" => return Err(PlanError::AlreadyDone),
        (false, None) => return Err(PlanError::AlreadyDone),
        // An override the editor does not know the meaning of (OQ-15) is not replaced.
        (true, Some(other)) if other != &Value::String("on".into()) => {
            return Err(PlanError::EntryChanged);
        }
        (false, Some(Value::String(v))) if v != "off" => return Err(PlanError::EntryChanged),
        (false, Some(Value::String(_))) => {
            let overrides = overrides.ok_or(PlanError::EntryGone)?;
            let index = overrides
                .value
                .members()
                .and_then(|m| m.iter().position(|m| m.key == name))
                .ok_or(PlanError::EntryGone)?;
            let removal = jsonspan::remove_child(bytes, &overrides.value, index)
                .ok_or(PlanError::Malformed)?;
            (
                removal.edit,
                Logical::RemoveNested {
                    key: "skillOverrides".into(),
                    name: name.into(),
                },
                vec![DiffLine {
                    kind: DiffKind::Removed,
                    text: line("\"off\""),
                }],
            )
        }
        (true, Some(_)) => {
            let overrides = overrides.ok_or(PlanError::EntryGone)?;
            let member = overrides.value.member(name).ok_or(PlanError::EntryGone)?;
            (
                jsonspan::replace_value(&member.value, "\"off\""),
                Logical::SetNested {
                    key: "skillOverrides".into(),
                    name: name.into(),
                    value: Value::String("off".into()),
                },
                vec![DiffLine {
                    kind: DiffKind::Added,
                    text: line("\"off\""),
                }],
            )
        }
        (true, None) => {
            let logical = Logical::SetNested {
                key: "skillOverrides".into(),
                name: name.into(),
                value: Value::String("off".into()),
            };
            let diff = vec![DiffLine {
                kind: DiffKind::Added,
                text: line("\"off\""),
            }];
            match overrides {
                Some(member) if member.value.members().is_some() => (
                    jsonspan::append_child(bytes, &member.value, &line("\"off\"")),
                    logical,
                    diff,
                ),
                Some(_) => return Err(PlanError::Malformed),
                None => (
                    jsonspan::append_child(
                        bytes,
                        &root,
                        &jsonspan::member_text(
                            "skillOverrides",
                            &format!("{{{}}}", line("\"off\"")),
                        ),
                    ),
                    logical,
                    diff,
                ),
            }
        }
        (false, Some(_)) => return Err(PlanError::EntryChanged),
    };
    let new_bytes = edit.apply(bytes);
    if !is_exactly(bytes, &new_bytes, &logical) {
        return Err(PlanError::Malformed);
    }
    Ok(Drafted {
        operation,
        item: item.to_owned(),
        name: redact::text(name).text.as_str().to_owned(),
        risk,
        h0: hash(bytes),
        h1: hash(&new_bytes),
        new_bytes,
        logical,
        prepared: Prepared::Nothing,
        diff,
        holds_hidden_values: false,
        prior: None,
        managed: None,
    })
}

/// How long a drafted plan lives (07 §5.6).
pub const PLAN_LIFETIME_SECONDS: u64 = 60;
