//! Planning a change to Codex's `config.toml` (F2b, prepared and not wired): the switches of
//! a skill and of an MCP server, as a byte edit of the one key that holds each, checked
//! against the same edit made to the parsed value, with a **prediction** of what the change
//! does to managed Inference.
//!
//! Nothing here is reachable from a window command: there is no `Operation` for it, no
//! capability and no store. It exists so that the decisions F2b waits for (19, 22) can be
//! made on working, tested pieces, and so that turning F2b on is wiring and not design.
//! Individual Codex hooks are not here at all: where Codex keeps their switch is unverified
//! (OQ-01), and enabling one is blocked until G-INF.
use super::change::Prior;
use super::inventory::{SourceKey, item_id_for};
use super::jsonspan::Edit;
use super::tomlspan::{self, Document, SpanError, Table};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexOperation {
    SkillOff,
    SkillOn,
    McpOff,
    McpOn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexPlanError {
    /// Not TOML, or a layout the editor does not place with certainty.
    Malformed,
    Unsupported,
    EntryGone,
    /// The key holds something other than a boolean.
    EntryChanged,
    AlreadyDone,
}

/// What the plan does to the value tree, for checking the byte edit.
#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Key(String),
    Index(usize),
}
#[derive(Debug, Clone, PartialEq)]
pub enum Logical {
    Set { path: Vec<Part>, value: Value },
    Remove { path: Vec<Part> },
}

pub fn apply_logical(before: &Value, edit: &Logical) -> Option<Value> {
    let (path, value) = match edit {
        Logical::Set { path, value } => (path, Some(value)),
        Logical::Remove { path } => (path, None),
    };
    let mut root = before.clone();
    let mut at = &mut root;
    let (last, rest) = path.split_last()?;
    for part in rest {
        at = match part {
            Part::Key(key) => at.get_mut(key)?,
            Part::Index(index) => at.get_mut(*index)?,
        };
    }
    match (last, value) {
        (Part::Key(key), Some(value)) => {
            at.as_object_mut()?.insert(key.clone(), value.clone());
        }
        (Part::Key(key), None) => {
            at.as_object_mut()?.remove(key)?;
        }
        (Part::Index(_), _) => return None,
    }
    Some(root)
}

/// What the verdict of managed Inference is on a configuration, as the authority decides it
/// on the definitions (`zero_external_hooks`, `mcp_names`): not on a running Codex.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Available,
    /// A hook definition is enabled (or its enablement is unknown, which counts the same),
    /// or there are more hook sources or definitions than the check accepts.
    ExternalHooks,
    /// More than 64 MCP servers, or a name outside letters, digits, `_` and `-`, or longer
    /// than 64: the profile that turns them off by name cannot be proved.
    McpUnprovable,
}

/// The facts the authority's checks look at, read from a parsed configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Facts {
    /// Per source that defines hooks, one entry per definition: whether it is enabled. A
    /// definition with no explicit `enabled = false` is enabled.
    pub hook_sources: Vec<Vec<bool>>,
    pub mcp_names: Vec<String>,
}

impl Facts {
    /// From `config.toml` and, if there is one, `hooks.json`.
    pub fn from_files(config: &Value, hooks_json: Option<&Value>) -> Self {
        let mut facts = Self::default();
        let mut add = |map: Option<&Value>| {
            if let Some(map) = map {
                let (entries, _) = super::inventory::hook_entries(map);
                if !entries.is_empty() {
                    facts.hook_sources.push(
                        entries
                            .iter()
                            .map(|e| e.handler.get("enabled") != Some(&Value::Bool(false)))
                            .collect(),
                    );
                }
            }
        };
        add(config.get("hooks"));
        add(hooks_json.map(|v| v.get("hooks").unwrap_or(v)));
        if let Some(servers) = config.get("mcp_servers").and_then(Value::as_object) {
            facts.mcp_names = servers.keys().cloned().collect();
        }
        facts
    }

    /// Hooks first, then servers, as the authority checks them: when both are wrong, the
    /// cause named is the hooks'.
    pub fn verdict(&self) -> Verdict {
        let safe = |name: &str| {
            !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        };
        let total: usize = self.hook_sources.iter().map(Vec::len).sum();
        if self.hook_sources.len() > 64
            || total > 256
            || self.hook_sources.iter().flatten().any(|enabled| *enabled)
        {
            return Verdict::ExternalHooks;
        }
        if self.mcp_names.len() > 64 || self.mcp_names.iter().any(|n| !safe(n)) {
            return Verdict::McpUnprovable;
        }
        Verdict::Available
    }
}

/// A prediction, said as one: it comes from the files and the authority's rules, never from
/// a running Codex, and the page labels it so (DN-10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prediction {
    pub before: Verdict,
    pub after: Verdict,
}

impl Prediction {
    pub fn changes_anything(&self) -> bool {
        self.before != self.after
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexDrafted {
    pub operation: CodexOperation,
    pub name: String,
    pub new_bytes: Vec<u8>,
    pub logical: Logical,
    pub prediction: Prediction,
    /// What the key held before: all that an undo needs to put the file back exactly.
    pub prior: Prior,
    /// The edit that puts the file back, valid while the file is the one produced (tests).
    pub inverse: Edit,
}

fn parse(text: &str) -> Result<(Document, Value), CodexPlanError> {
    let document = tomlspan::parse(text).map_err(|_: SpanError| CodexPlanError::Unsupported)?;
    let value = super::reader::parse_toml(text.as_bytes(), &super::model::Bounds::PROPOSED)
        .map_err(|_| CodexPlanError::Malformed)?
        .unwrap_or(Value::Object(serde_json::Map::new()));
    Ok((document, value))
}

/// The table that holds the `enabled` key of the thing named, and its path in the value.
fn locate<'a>(
    document: &'a Document,
    value: &Value,
    operation: CodexOperation,
    name: &str,
) -> Result<(&'a Table, Vec<Part>), CodexPlanError> {
    match operation {
        CodexOperation::SkillOff | CodexOperation::SkillOn => {
            let index = value
                .pointer("/skills/config")
                .and_then(Value::as_array)
                .and_then(|entries| {
                    entries
                        .iter()
                        .position(|e| e.get("path").and_then(Value::as_str) == Some(name))
                })
                .ok_or(CodexPlanError::EntryGone)?;
            let table = document
                .tables_at(&["skills", "config"])
                .into_iter()
                .nth(index)
                .filter(|t| t.array)
                .ok_or(CodexPlanError::Unsupported)?;
            Ok((
                table,
                vec![
                    Part::Key("skills".into()),
                    Part::Key("config".into()),
                    Part::Index(index),
                    Part::Key("enabled".into()),
                ],
            ))
        }
        CodexOperation::McpOff | CodexOperation::McpOn => {
            if value.pointer(&format!("/mcp_servers/{name}")).is_none() {
                return Err(CodexPlanError::EntryGone);
            }
            let table = document
                .tables_at(&["mcp_servers", name])
                .into_iter()
                .next()
                .filter(|t| !t.array)
                .ok_or(CodexPlanError::Unsupported)?;
            Ok((
                table,
                vec![
                    Part::Key("mcp_servers".into()),
                    Part::Key(name.to_owned()),
                    Part::Key("enabled".into()),
                ],
            ))
        }
    }
}

/// The skill's configured path or the server's name that an inventory item stands for.
pub fn resolve(
    config: &str,
    source: SourceKey,
    skill: bool,
    item: &str,
) -> Result<String, CodexPlanError> {
    let (_, value) = parse(config)?;
    if skill {
        value
            .pointer("/skills/config")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.get("path").and_then(Value::as_str))
            .find(|path| item_id_for(source, &[path]) == item)
            .map(str::to_owned)
            .ok_or(CodexPlanError::EntryGone)
    } else {
        value
            .get("mcp_servers")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|servers| servers.keys())
            .find(|name| item_id_for(source, &[name]) == item)
            .cloned()
            .ok_or(CodexPlanError::EntryGone)
    }
}

/// Plans one switch. `name` is a skill's configured path or an MCP server's name; `config`
/// is `config.toml` as text, `hooks_json` the parsed `hooks.json` if there is one. With
/// `restore`, the key is put to what it held before a change (an undo): absent removes its
/// line, a boolean sets it.
pub fn draft(
    config: &str,
    hooks_json: Option<&Value>,
    operation: CodexOperation,
    name: &str,
    restore: Option<Prior>,
) -> Result<CodexDrafted, CodexPlanError> {
    let (document, before) = parse(config)?;
    let (table, path) = locate(&document, &before, operation, name)?;
    let key_path: Vec<String> = table
        .path
        .iter()
        .cloned()
        .chain(["enabled".to_owned()])
        .collect();
    let current = before
        .pointer(&pointer(&path))
        .map(|v| v.as_bool().ok_or(CodexPlanError::EntryChanged))
        .transpose()?;
    let wanted: Option<bool> = match restore {
        Some(Prior::Absent) => None,
        Some(Prior::Enabled) => Some(true),
        Some(Prior::Disabled) => Some(false),
        None => Some(matches!(
            operation,
            CodexOperation::SkillOn | CodexOperation::McpOn
        )),
    };
    let already = match restore {
        Some(_) => current == wanted,
        None => current.unwrap_or(true) == wanted.unwrap_or(true),
    };
    if already {
        return Err(CodexPlanError::AlreadyDone);
    }
    // An undo only puts back what its change left: Off left `false`, On left `true`. Anything
    // else means the entry was changed by somebody since, and it is not overwritten.
    if restore.is_some() {
        let left = !matches!(operation, CodexOperation::SkillOn | CodexOperation::McpOn);
        if current != Some(left) {
            return Err(CodexPlanError::EntryChanged);
        }
    }
    let prior = match current {
        None => Prior::Absent,
        Some(true) => Prior::Enabled,
        Some(false) => Prior::Disabled,
    };
    let entry = table.entry(&key_path);
    let (edit, inverse) = match (wanted, entry) {
        (None, Some(entry)) => {
            let old = config[entry.line.0..entry.line.1].to_owned();
            (
                tomlspan::remove_statement(entry),
                Edit {
                    at: entry.line.0,
                    remove: 0,
                    insert: old,
                },
            )
        }
        (None, None) => return Err(CodexPlanError::AlreadyDone),
        (Some(on), Some(entry)) => {
            let literal = if on { "true" } else { "false" };
            let old = config[entry.value.0..entry.value.1].to_owned();
            (
                tomlspan::replace_value(entry, literal),
                Edit {
                    at: entry.value.0,
                    remove: literal.len(),
                    insert: old,
                },
            )
        }
        (Some(on), None) => {
            let literal = if on { "true" } else { "false" };
            let edit = tomlspan::insert_statement(config, table, "enabled", literal);
            let inverse = Edit {
                at: edit.at,
                remove: edit.insert.len(),
                insert: String::new(),
            };
            (edit, inverse)
        }
    };
    let new_text =
        String::from_utf8(edit.apply(config.as_bytes())).map_err(|_| CodexPlanError::Malformed)?;
    let logical = match wanted {
        Some(on) => Logical::Set {
            path,
            value: Value::Bool(on),
        },
        None => Logical::Remove { path },
    };
    // The edit must be exactly the logical one, and the result must still parse.
    let (_, after) = parse(&new_text).map_err(|_| CodexPlanError::Malformed)?;
    if apply_logical(&before, &logical).as_ref() != Some(&after) {
        return Err(CodexPlanError::Malformed);
    }
    let prediction = Prediction {
        before: Facts::from_files(&before, hooks_json).verdict(),
        after: Facts::from_files(&after, hooks_json).verdict(),
    };
    Ok(CodexDrafted {
        operation,
        name: super::redact::text(name).text.as_str().to_owned(),
        new_bytes: new_text.into_bytes(),
        logical,
        prediction,
        prior,
        inverse,
    })
}

/// Whether `after` is `before` with `edit` made to it and nothing else, both read as TOML.
pub fn is_exactly(before: &[u8], after: &[u8], edit: &Logical) -> bool {
    let bounds = super::model::Bounds::PROPOSED;
    let (Ok(Some(before)), Ok(Some(after))) = (
        super::reader::parse_toml(before, &bounds),
        super::reader::parse_toml(after, &bounds),
    ) else {
        return false;
    };
    apply_logical(&before, edit).is_some_and(|expected| expected == after)
}

/// A JSON pointer for a value path.
fn pointer(path: &[Part]) -> String {
    path.iter()
        .map(|part| match part {
            Part::Key(key) => format!("/{}", key.replace('~', "~0").replace('/', "~1")),
            Part::Index(index) => format!("/{index}"),
        })
        .collect()
}
