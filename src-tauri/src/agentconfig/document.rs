//! Provider instruction files (F5). The page names an inventory item, never a path or
//! instruction text. Only the native editor receives the original text. Its inverse uses
//! the existing sealed restoration store and its writes use the configuration transaction.
use super::author::inverse_of;
use super::change::{Operation, Risk};
use super::inventory::{Environment, SourceKey, item_id_for};
use super::plan::{DiffKind, DiffLine, Drafted, Logical, PlanError, Prepared, hash};
use super::restore::{Protection, RecordBody};
use std::path::PathBuf;

const EVENT: &str = "document_v1";
pub const MOST_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub source: SourceKey,
    pub name: String,
}

fn component(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_- .".contains(&c))
        && name != ".."
}

impl Target {
    /// A closed provider root and a checked local name; no imported path is trusted.
    pub fn check(&self) -> Result<(), PlanError> {
        let ok = match self.source {
            SourceKey::ClaudeUserSkills => component(&self.name),
            SourceKey::ClaudeUserRules => {
                self.name == "CLAUDE.md"
                    || self
                        .name
                        .strip_prefix("rules/")
                        .is_some_and(|n| component(n) && n.ends_with(".md"))
            }
            SourceKey::CodexUserRules => {
                matches!(self.name.as_str(), "AGENTS.md" | "AGENTS.override.md")
            }
            SourceKey::OpencodeUserRules => self.name == "AGENTS.md",
            _ => false,
        };
        ok.then_some(()).ok_or(PlanError::NotSupported)
    }

    pub fn path(&self, env: &Environment) -> Result<PathBuf, PlanError> {
        self.check()?;
        Ok(match self.source {
            SourceKey::ClaudeUserSkills => env
                .claude_dir()
                .join("skills")
                .join(&self.name)
                .join("SKILL.md"),
            SourceKey::ClaudeUserRules => env.claude_dir().join(&self.name),
            SourceKey::CodexUserRules => env.codex_dir().join(&self.name),
            SourceKey::OpencodeUserRules => env.opencode_dir().join(&self.name),
            _ => return Err(PlanError::NotSupported),
        })
    }

    pub fn id(&self) -> String {
        item_id_for(self.source, &[&self.name])
    }

    pub fn label(&self) -> String {
        if self.source == SourceKey::ClaudeUserSkills {
            format!("skills/{}/SKILL.md", self.name)
        } else {
            self.name.clone()
        }
    }
}

pub fn from_record(body: &RecordBody) -> Result<Target, PlanError> {
    if body.event != EVENT || body.provider != body.source.provider() {
        return Err(PlanError::RecordUnreadable);
    }
    let target = Target {
        source: body.source,
        name: body.matcher.clone().ok_or(PlanError::RecordUnreadable)?,
    };
    target.check()?;
    Ok(target)
}

pub fn is_record(body: &RecordBody) -> bool {
    body.event == EVENT
}

pub fn valid_text(text: &str) -> bool {
    text.len() <= MOST_BYTES
        && text.chars().all(|c| {
            (!c.is_control() || matches!(c, '\n' | '\r' | '\t')) && !super::author::disguises(c)
        })
}

pub fn draft(target: Target, before: &[u8], text: &str) -> Result<Drafted, PlanError> {
    target.check()?;
    if !valid_text(text) || before.len() > MOST_BYTES || std::str::from_utf8(before).is_err() {
        return Err(PlanError::Invalid);
    }
    if before == text.as_bytes() {
        return Err(PlanError::AlreadyDone);
    }
    let mut body = inverse_of(
        target.source.provider(),
        target.source,
        before,
        text.as_bytes(),
    );
    body.event = EVENT.to_owned();
    body.matcher = Some(target.name.clone());
    Ok(Drafted {
        operation: Operation::EditDocument,
        item: target.id(),
        name: target.label(),
        risk: Risk::WeakensSecurity,
        h0: hash(before),
        h1: hash(text.as_bytes()),
        new_bytes: text.as_bytes().to_vec(),
        logical: Logical::Document(target.clone()),
        // Instructions may contain private data. No plaintext fallback, even for a
        // classifier miss. Existing key-store failure handling refuses the write.
        prepared: Prepared::Inverse {
            body: Box::new(body),
            label: target.label(),
            secret_bearing: true,
            protection: Protection::Sealed,
        },
        diff: vec![DiffLine {
            kind: DiffKind::Note,
            text: format!(
                "Instruction file reviewed in the native editor: {} → {} bytes. Content is kept out of this page; exact undo requires the file to stay unchanged.",
                before.len(),
                text.len()
            ),
        }],
        holds_hidden_values: true,
        prior: None,
        managed: None,
    })
}

pub fn revert(bytes: &[u8], record: &str, body: &RecordBody) -> Result<Drafted, PlanError> {
    let target = from_record(body)?;
    if hash(bytes) != body.h1 || bytes.len() > MOST_BYTES {
        return Err(PlanError::EntryChanged);
    }
    let end = body
        .removed_at
        .checked_add(body.handler_text.len())
        .ok_or(PlanError::RecordUnreadable)?;
    if bytes.get(body.removed_at..end) != Some(body.handler_text.as_bytes()) {
        return Err(PlanError::RecordUnreadable);
    }
    let mut prior = bytes.to_vec();
    prior.splice(body.removed_at..end, body.removed_text.bytes());
    if prior.len() > MOST_BYTES || hash(&prior) != body.h0 || std::str::from_utf8(&prior).is_err() {
        return Err(PlanError::RecordUnreadable);
    }
    Ok(Drafted {
        operation: Operation::RevertAuthored,
        item: target.id(),
        name: target.label(),
        risk: Risk::WeakensSecurity,
        h0: body.h1.clone(),
        h1: body.h0.clone(),
        new_bytes: prior,
        logical: Logical::Document(target),
        prepared: Prepared::Restore { record: record.to_owned(), already_present: false },
        diff: vec![DiffLine { kind: DiffKind::Note, text: "Restore the exact instruction-file bytes from before the edit. A changed file is refused.".to_owned() }],
        holds_hidden_values: true,
        prior: None,
        managed: None,
    })
}
