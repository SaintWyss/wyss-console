//! Authoring for Codex's `config.toml` (F5): a new MCP server as a new table, a change to the
//! program, arguments or address of one, and a server taken out and put back.
//!
//! Every change is a byte edit of the user's own file, made from the positions `tomlspan`
//! reports, and is checked against the same change made to the parsed value. A layout the
//! scanner does not place with certainty (a server defined through dotted keys or an inline
//! table, tables of one server that are not together, a table of the same name twice) is
//! refused, and the file is never rewritten as a whole. Comments, blank lines, order and line
//! endings stay the user's. What the user writes is validated first (`McpSpec::check`) and is
//! typed in a window of the desktop's own, as for OpenCode.
use super::change::{Operation, Risk};
use super::inventory::{SourceKey, item_id_for};
use super::jsonspan::Edit;
use super::model::{Bounds, ProviderId};
use super::opencode_plan::{McpSpec, mcp_name_ok};
use super::plan::{
    DiffKind, Drafted, Logical as PlanLogical, PlanError, Prepared, hash, redact_lines,
};
use super::restore::{Anchor, Level, Protection, RecordBody, RestoreError};
use super::tomlspan::{self, Document, Table};
use serde_json::{Value, json};

const KEY: SourceKey = SourceKey::CodexUserConfig;

/// What the plan does to the value tree, for checking the byte edit.
#[derive(Clone, PartialEq)]
pub enum Logical {
    /// `mcp_servers.<name>` becomes this entry (new, replaced or put back).
    SetServer {
        name: String,
        value: Value,
    },
    RemoveServer {
        name: String,
    },
}

/// A server's arguments and address are where a token lives: nothing of the value is printed.
impl std::fmt::Debug for Logical {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetServer { name, .. } => write!(f, "SetServer({} chars, ..)", name.len()),
            Self::RemoveServer { name } => write!(f, "RemoveServer({} chars)", name.len()),
        }
    }
}

pub fn apply_logical(before: &Value, edit: &Logical) -> Option<Value> {
    let mut root = before.clone();
    match edit {
        Logical::SetServer { name, value } => {
            let servers = root
                .as_object_mut()?
                .entry("mcp_servers".to_owned())
                .or_insert_with(|| Value::Object(serde_json::Map::new()))
                .as_object_mut()?;
            servers.insert(name.clone(), value.clone());
        }
        Logical::RemoveServer { name } => {
            root.get_mut("mcp_servers")?.as_object_mut()?.remove(name)?;
        }
    }
    Some(root)
}

pub fn is_exactly(before: &str, after: &str, edit: &Logical) -> bool {
    // An empty file is an empty table, and an empty `mcp_servers` is no `mcp_servers`: taking
    // the last server out of a file that has no `[mcp_servers]` header of its own leaves none.
    let value = |text: &str| {
        parse_value(text).ok().map(|v| {
            let mut v = v.unwrap_or(Value::Object(serde_json::Map::new()));
            normalise(&mut v);
            v
        })
    };
    let (Some(before), Some(after)) = (value(before), value(after)) else {
        return false;
    };
    apply_logical(&before, edit).is_some_and(|mut expected| {
        normalise(&mut expected);
        expected == after
    })
}

fn normalise(value: &mut Value) {
    if let Some(map) = value.as_object_mut()
        && map
            .get("mcp_servers")
            .and_then(Value::as_object)
            .is_some_and(|m| m.is_empty())
    {
        map.remove("mcp_servers");
    }
}

fn parse_value(text: &str) -> Result<Option<Value>, ()> {
    super::reader::parse_toml(text.as_bytes(), &Bounds::PROPOSED).map_err(|_| ())
}

fn parse(text: &str) -> Result<(Document, Value), PlanError> {
    let document = tomlspan::parse(text).map_err(|_| PlanError::NotSupported)?;
    let value = parse_value(text)
        .map_err(|()| PlanError::Malformed)?
        .unwrap_or(Value::Object(serde_json::Map::new()));
    Ok((document, value))
}

/// A string as a TOML basic string: JSON's escapes are TOML's.
fn string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_default()
}

/// The entry a spec means as a value, with the program split as Codex keeps it.
fn entry_of(spec: &McpSpec) -> Value {
    match spec {
        McpSpec::Local { command } => {
            let mut entry = json!({ "command": command[0] });
            if command.len() > 1 {
                entry["args"] = json!(command[1..]);
            }
            entry
        }
        McpSpec::Remote { url } => json!({ "url": url }),
    }
}

/// The text of a new server's table, in the file's own line endings.
fn table_text(text: &str, name: &str, spec: &McpSpec) -> String {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = format!("[mcp_servers.{name}]{eol}");
    match spec {
        McpSpec::Local { command } => {
            out.push_str(&format!("command = {}{eol}", string(&command[0])));
            if command.len() > 1 {
                let args: Vec<String> = command[1..].iter().map(|a| string(a)).collect();
                out.push_str(&format!("args = [{}]{eol}", args.join(", ")));
            }
        }
        McpSpec::Remote { url } => out.push_str(&format!("url = {}{eol}", string(url))),
    }
    out
}

/// The tables of one server, together: its own and those under it. `None` when it is defined
/// any way the scanner does not place (twice, apart, through dotted keys or an inline table).
fn server_tables<'a>(document: &'a Document, name: &str) -> Option<Vec<&'a Table>> {
    let own = document.tables_at(&["mcp_servers", name]);
    let [first] = own.as_slice() else { return None };
    if first.array {
        return None;
    }
    let start = document
        .tables
        .iter()
        .position(|t| std::ptr::eq(t, *first))?;
    let mut group = vec![*first];
    for table in &document.tables[start + 1..] {
        let under = table.path.len() > 2
            && table.path[0] == "mcp_servers"
            && table.path[1] == name
            && !table.array;
        if under {
            group.push(table);
        } else {
            break;
        }
    }
    // A table of the same server that is not in the group (apart from it) is not placed.
    let all = document
        .tables
        .iter()
        .filter(|t| t.path.len() >= 2 && t.path[0] == "mcp_servers" && t.path[1] == name)
        .count();
    (all == group.len()).then_some(group)
}

/// A server defined anywhere but its own table (`mcp_servers.x.command = …` under another
/// header, or an inline table) is in the value but has no table: not placed.
fn defined_in_own_table(document: &Document, value: &Value, name: &str) -> bool {
    value
        .pointer(&format!("/mcp_servers/{}", pointer_key(name)))
        .is_some()
        && document.tables_at(&["mcp_servers", name]).len() == 1
}

fn pointer_key(name: &str) -> String {
    name.replace('~', "~0").replace('/', "~1")
}

/// Which server an item identifier names.
pub fn resolve_server(text: &str, item: &str) -> Result<String, PlanError> {
    let (_, value) = parse(text)?;
    value
        .get("mcp_servers")
        .and_then(Value::as_object)
        .ok_or(PlanError::EntryGone)?
        .keys()
        .find(|name| item_id_for(KEY, &[name]) == item)
        .cloned()
        .ok_or(PlanError::EntryGone)
}

/// A server's current spec, for the window that changes it.
pub fn current(text: &str, name: &str) -> Option<McpSpec> {
    let (_, value) = parse(text).ok()?;
    let entry = value.get("mcp_servers")?.get(name)?;
    if let Some(url) = entry.get("url").and_then(Value::as_str) {
        return Some(McpSpec::Remote {
            url: url.to_owned(),
        });
    }
    let mut command = vec![entry.get("command")?.as_str()?.to_owned()];
    for arg in entry
        .get("args")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        command.push(arg.as_str()?.to_owned());
    }
    Some(McpSpec::Local { command })
}

fn label(text: &str) -> String {
    super::redact::text(text).text.as_str().to_owned()
}

/// A new server, as a new table at the end of the file.
#[allow(clippy::too_many_arguments)]
pub fn draft_add(
    text: &str,
    name: &str,
    spec: &McpSpec,
    bounds: &Bounds,
    protection: impl Fn(bool) -> Result<Protection, RestoreError>,
) -> Result<Drafted, PlanError> {
    if !mcp_name_ok(name) {
        return Err(PlanError::Invalid);
    }
    spec.check()?;
    let (_, before) = parse(text)?;
    if before
        .pointer(&format!("/mcp_servers/{}", pointer_key(name)))
        .is_some()
    {
        return Err(PlanError::AlreadyDone);
    }
    // `mcp_servers` defined as something that is not a table of servers is not touched.
    if before.get("mcp_servers").is_some_and(|m| !m.is_object()) {
        return Err(PlanError::NotSupported);
    }
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut insert = String::new();
    if !text.is_empty() && !text.ends_with('\n') {
        insert.push_str(eol);
    }
    if !text.trim().is_empty() {
        insert.push_str(eol);
    }
    insert.push_str(&table_text(text, name, spec));
    let edit = Edit {
        at: text.len(),
        remove: 0,
        insert,
    };
    let new = String::from_utf8(edit.apply(text.as_bytes())).map_err(|_| PlanError::Malformed)?;
    let logical = Logical::SetServer {
        name: name.to_owned(),
        value: entry_of(spec),
    };
    if !is_exactly(text, &new, &logical) {
        return Err(PlanError::Malformed);
    }
    authored(
        Operation::CodexAddMcp,
        &item_id_for(KEY, &[name]),
        &format!("add MCP server · {name}"),
        text,
        new,
        logical,
        &entry_of(spec),
        bounds,
        protection,
    )
}

/// The program, arguments or address of a server, replaced in place. A server does not change
/// kind here: that is a deletion and a new server.
pub fn draft_edit(
    text: &str,
    name: &str,
    spec: &McpSpec,
    bounds: &Bounds,
    protection: impl Fn(bool) -> Result<Protection, RestoreError>,
) -> Result<Drafted, PlanError> {
    spec.check()?;
    let (document, before) = parse(text)?;
    if !defined_in_own_table(&document, &before, name) {
        return Err(
            if before
                .pointer(&format!("/mcp_servers/{}", pointer_key(name)))
                .is_none()
            {
                PlanError::EntryGone
            } else {
                PlanError::NotSupported
            },
        );
    }
    let old = before["mcp_servers"][name].clone();
    let remote_now = old.get("url").is_some();
    if remote_now != matches!(spec, McpSpec::Remote { .. }) {
        return Err(PlanError::NotSupported);
    }
    let mut want = old.clone();
    let new_entry = entry_of(spec);
    for key in ["command", "args", "url"] {
        match new_entry.get(key) {
            Some(v) => want[key] = v.clone(),
            None if key != "url" || !remote_now => {
                want.as_object_mut().map(|m| m.remove(key));
            }
            None => {}
        }
    }
    if want == old {
        return Err(PlanError::AlreadyDone);
    }
    let mut current_text = text.to_owned();
    for key in ["command", "args", "url"] {
        let wanted = want.get(key).cloned();
        if old.get(key) == wanted.as_ref() {
            continue;
        }
        current_text = set_field(&current_text, name, key, wanted.as_ref())?;
    }
    let logical = Logical::SetServer {
        name: name.to_owned(),
        value: want,
    };
    if !is_exactly(text, &current_text, &logical) {
        return Err(PlanError::Malformed);
    }
    authored(
        Operation::CodexEditMcp,
        &item_id_for(KEY, &[name]),
        &format!("edit MCP server · {name}"),
        text,
        current_text,
        logical,
        &new_entry,
        bounds,
        protection,
    )
}

/// One key of a server's own table: replaced, added after the last statement, or removed.
fn set_field(
    text: &str,
    name: &str,
    key: &str,
    value: Option<&Value>,
) -> Result<String, PlanError> {
    let (document, _) = parse(text)?;
    let group = server_tables(&document, name).ok_or(PlanError::NotSupported)?;
    let table = group[0];
    let path = vec!["mcp_servers".to_owned(), name.to_owned(), key.to_owned()];
    let existing = table.entry(&path);
    // A comment on the statement's own line would be lost by a removal, kept by a replacement.
    let literal = value.map(toml_text);
    let edit = match (existing, literal) {
        (Some(entry), Some(literal)) => tomlspan::replace_value(entry, &literal),
        (None, Some(literal)) => tomlspan::insert_statement(text, table, key, &literal),
        (Some(entry), None) => {
            if text[entry.value.1..entry.line.1].contains('#') {
                return Err(PlanError::NotSupported);
            }
            tomlspan::remove_statement(entry)
        }
        (None, None) => return Ok(text.to_owned()),
    };
    String::from_utf8(edit.apply(text.as_bytes())).map_err(|_| PlanError::Malformed)
}

/// A string or an array of strings as TOML text.
fn toml_text(value: &Value) -> String {
    match value {
        Value::String(s) => string(s),
        Value::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .filter_map(Value::as_str)
                .map(string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => other.to_string(),
    }
}

/// A server, with every table of it, taken out and kept; `RestoreHook` puts it back.
pub fn draft_park(
    text: &str,
    name: &str,
    bounds: &Bounds,
    protection: impl Fn(bool) -> Result<Protection, RestoreError>,
) -> Result<Drafted, PlanError> {
    let (document, before) = parse(text)?;
    if before
        .pointer(&format!("/mcp_servers/{}", pointer_key(name)))
        .is_none()
    {
        return Err(PlanError::EntryGone);
    }
    let group = server_tables(&document, name).ok_or(PlanError::NotSupported)?;
    let start = group[0].header.0;
    let end = group.iter().map(|t| t.body_end).max().unwrap_or(start);
    let removed = &text[start..end];
    let new = format!("{}{}", &text[..start], &text[end..]);
    let logical = Logical::RemoveServer {
        name: name.to_owned(),
    };
    if !is_exactly(text, &new, &logical) {
        return Err(PlanError::Malformed);
    }
    // Always sealed: a server's environment and arguments are where a token lives.
    let protection = protection(true).map_err(|e| match e {
        RestoreError::NoKeyStoreForSecret => PlanError::KeyStoreUnavailable,
        _ => PlanError::NotSupported,
    })?;
    let shown = before["mcp_servers"][name].clone();
    let body = RecordBody {
        provider: ProviderId::Codex,
        source: KEY,
        event: "mcp".to_owned(),
        matcher: Some(name.to_owned()),
        group: 0,
        index: 0,
        level: Level::Handler,
        anchor: Anchor::Alone,
        removed_text: removed.to_owned(),
        removed_at: start,
        unit_text: removed.to_owned(),
        handler_text: String::new(),
        project: None,
        h0: hash(text.as_bytes()),
        h1: hash(new.as_bytes()),
    };
    let (diff, _) = redact_lines(&super::author::brief(&shown).0, DiffKind::Removed, bounds);
    let name_shown = label(&format!("MCP server · {name}"));
    Ok(Drafted {
        operation: Operation::CodexParkMcp,
        item: item_id_for(KEY, &[name]),
        name: name_shown.clone(),
        risk: Risk::Normal,
        h0: body.h0.clone(),
        h1: body.h1.clone(),
        new_bytes: new.into_bytes(),
        logical: PlanLogical::CodexAuthor(logical),
        prepared: Prepared::Park {
            body: Box::new(body),
            label: name_shown,
            secret_bearing: true,
            protection,
        },
        diff,
        holds_hidden_values: true,
        prior: None,
        managed: None,
    })
}

/// A parked server put back: exactly where it was when the file is as the removal left it,
/// otherwise at the end of the file; refused when the name is taken. It starts a program: the
/// native dialog.
pub fn drafted_restore(
    text: &str,
    record: &str,
    body: &RecordBody,
    bounds: &Bounds,
) -> Result<Drafted, PlanError> {
    let name = body.matcher.as_deref().ok_or(PlanError::RecordUnreadable)?;
    if body.source != KEY || body.event != "mcp" || name.is_empty() || name.len() > 256 {
        return Err(PlanError::RecordUnreadable);
    }
    // The record's text must be exactly one server's tables, nothing else.
    let (record_doc, record_value) =
        parse(&body.removed_text).map_err(|_| PlanError::RecordUnreadable)?;
    let unit = record_value
        .get("mcp_servers")
        .and_then(|m| m.get(name))
        .cloned()
        .ok_or(PlanError::RecordUnreadable)?;
    if record_value.as_object().map(|m| m.len()) != Some(1)
        || record_value["mcp_servers"].as_object().map(|m| m.len()) != Some(1)
        || server_tables(&record_doc, name).is_none()
        || body.removed_text != body.unit_text
    {
        return Err(PlanError::RecordUnreadable);
    }
    let (_, before) = parse(text)?;
    if let Some(now) = before.pointer(&format!("/mcp_servers/{}", pointer_key(name))) {
        return Err(if *now == unit {
            PlanError::AlreadyDone
        } else {
            PlanError::RestoreConflict
        });
    }
    let logical = Logical::SetServer {
        name: name.to_owned(),
        value: unit.clone(),
    };
    let mut new = None;
    if hash(text.as_bytes()) == body.h1
        && body.removed_at <= text.len()
        && text.is_char_boundary(body.removed_at)
    {
        let restored = format!(
            "{}{}{}",
            &text[..body.removed_at],
            body.removed_text,
            &text[body.removed_at..]
        );
        if hash(restored.as_bytes()) == body.h0 && is_exactly(text, &restored, &logical) {
            new = Some(restored);
        }
    }
    let new = match new {
        Some(t) => t,
        None => {
            let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
            let mut tail = String::new();
            if !text.is_empty() && !text.ends_with('\n') {
                tail.push_str(eol);
            }
            if !text.trim().is_empty() {
                tail.push_str(eol);
            }
            tail.push_str(&body.removed_text);
            let restored = format!("{text}{tail}");
            if !is_exactly(text, &restored, &logical) {
                return Err(PlanError::RestoreConflict);
            }
            restored
        }
    };
    let (diff, _) = redact_lines(&super::author::brief(&unit).0, DiffKind::Added, bounds);
    Ok(Drafted {
        operation: Operation::RestoreHook,
        item: record.to_owned(),
        name: label(&format!("MCP server · {name}")),
        risk: Risk::ExecutesCode,
        h0: hash(text.as_bytes()),
        h1: hash(new.as_bytes()),
        new_bytes: new.into_bytes(),
        logical: PlanLogical::CodexAuthor(logical),
        prepared: Prepared::Restore {
            record: record.to_owned(),
            already_present: false,
        },
        diff,
        holds_hidden_values: true,
        prior: None,
        managed: None,
    })
}

#[allow(clippy::too_many_arguments)]
fn authored(
    operation: Operation,
    item: &str,
    name: &str,
    old: &str,
    new: String,
    logical: Logical,
    shown: &Value,
    bounds: &Bounds,
    protection: impl Fn(bool) -> Result<Protection, RestoreError>,
) -> Result<Drafted, PlanError> {
    let mut body =
        super::author::inverse_of(ProviderId::Codex, KEY, old.as_bytes(), new.as_bytes());
    // The record says which server it is about: a take-back may change that one and no other.
    body.matcher = Some(
        match &logical {
            Logical::SetServer { name, .. } | Logical::RemoveServer { name } => name,
        }
        .clone(),
    );
    // What was written, and anything it replaced (the record keeps both), is sealed when it
    // may hold a token; anything that replaced text seals.
    // Arguments and addresses are where a token lives: any that are written are sealed.
    let secret_bearing = super::classify::may_hold_secret(shown, bounds)
        || !body.removed_text.is_empty()
        || shown
            .get("args")
            .is_some_and(|a| a.as_array().is_some_and(|a| !a.is_empty()));
    let protection = protection(secret_bearing).map_err(|e| match e {
        RestoreError::NoKeyStoreForSecret => PlanError::KeyStoreUnavailable,
        _ => PlanError::NotSupported,
    })?;
    let (diff, hidden) = redact_lines(&super::author::brief(shown).0, DiffKind::Added, bounds);
    let hidden = hidden || super::author::brief(shown).1;
    Ok(Drafted {
        operation,
        item: item.to_owned(),
        name: label(name),
        risk: Risk::ExecutesCode,
        h0: hash(old.as_bytes()),
        h1: hash(new.as_bytes()),
        new_bytes: new.into_bytes(),
        logical: PlanLogical::CodexAuthor(logical),
        prepared: Prepared::Inverse {
            body: Box::new(body),
            label: label(name),
            secret_bearing,
            protection,
        },
        diff,
        holds_hidden_values: hidden || secret_bearing,
        prior: None,
        managed: None,
    })
}
