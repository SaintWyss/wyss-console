//! Provider instruction files (F5). The page names an inventory item, never a path or
//! instruction text. Only the native editor receives the original text. Its inverse uses
//! the existing sealed restoration store and its writes use the configuration transaction.
use super::author::inverse_of;
use super::change::{Operation, Risk};
use super::inventory::{Environment, SourceKey, item_id_for};
use super::plan::{ABSENT, DiffKind, DiffLine, Drafted, Logical, PlanError, Prepared, hash};
use super::restore::{Protection, RecordBody};
use std::path::{Path, PathBuf};

const EVENT: &str = "document_v1";
pub const MOST_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub source: SourceKey,
    pub name: String,
    /// The project folder, for the sources that belong to one; `None` for the account's own.
    pub project: Option<String>,
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

/// Whether a source is a project's own file rather than the account's.
const fn is_project(source: SourceKey) -> bool {
    matches!(
        source,
        SourceKey::ClaudeProjectSkills
            | SourceKey::ClaudeProjectRules
            | SourceKey::CodexProjectRules
            | SourceKey::OpencodeProjectRules
    )
}

/// A project folder as a record or a plan may carry it: absolute, made of plain names.
fn project_ok(project: &str) -> bool {
    let path = Path::new(project);
    project.len() <= 4096
        && path.is_absolute()
        && super::place::names(path).is_ok_and(|n| !n.is_empty())
}

impl Target {
    pub fn user(source: SourceKey, name: impl Into<String>) -> Self {
        Self {
            source,
            name: name.into(),
            project: None,
        }
    }

    pub fn in_project(source: SourceKey, name: impl Into<String>, project: &Path) -> Self {
        Self {
            source,
            name: name.into(),
            project: Some(project.to_string_lossy().into_owned()),
        }
    }

    /// A closed provider root and a checked local name; no imported path is trusted.
    pub fn check(&self) -> Result<(), PlanError> {
        // A project's file carries its project; an account's file carries none.
        if is_project(self.source) != self.project.as_deref().is_some_and(project_ok)
            || (self.project.is_some() && !is_project(self.source))
        {
            return Err(PlanError::NotSupported);
        }
        let rules_name = |name: &str| {
            name == "CLAUDE.md"
                || name
                    .strip_prefix("rules/")
                    .is_some_and(|n| component(n) && n.ends_with(".md"))
        };
        let ok = match self.source {
            SourceKey::ClaudeUserSkills
            | SourceKey::CodexUserSkills
            | SourceKey::ClaudeProjectSkills => component(&self.name),
            SourceKey::ClaudeUserRules | SourceKey::ClaudeProjectRules => rules_name(&self.name),
            SourceKey::CodexUserRules => {
                matches!(self.name.as_str(), "AGENTS.md" | "AGENTS.override.md")
            }
            // The inventory reads only a project's `AGENTS.md`, so only that can be made.
            SourceKey::OpencodeUserRules
            | SourceKey::CodexProjectRules
            | SourceKey::OpencodeProjectRules => self.name == "AGENTS.md",
            _ => false,
        };
        ok.then_some(()).ok_or(PlanError::NotSupported)
    }

    /// The project folder of a project file, which must be the one now selected.
    fn project_dir(&self, env: &Environment) -> Result<PathBuf, PlanError> {
        let selected = env.project.as_deref().ok_or(PlanError::NotSupported)?;
        if self.project.as_deref().map(Path::new) != Some(selected) {
            return Err(PlanError::NotSupported);
        }
        Ok(selected.to_owned())
    }

    pub fn path(&self, env: &Environment) -> Result<PathBuf, PlanError> {
        self.check()?;
        Ok(match self.source {
            SourceKey::ClaudeUserSkills => env
                .claude_dir()
                .join("skills")
                .join(&self.name)
                .join("SKILL.md"),
            SourceKey::CodexUserSkills => env.agents_skills_dir().join(&self.name).join("SKILL.md"),
            SourceKey::ClaudeUserRules => env.claude_dir().join(&self.name),
            SourceKey::CodexUserRules => env.codex_dir().join(&self.name),
            SourceKey::OpencodeUserRules => env.opencode_dir().join(&self.name),
            SourceKey::ClaudeProjectSkills => self
                .project_dir(env)?
                .join(".claude/skills")
                .join(&self.name)
                .join("SKILL.md"),
            // `CLAUDE.md` sits at the project's root; its rules in `.claude/rules`.
            SourceKey::ClaudeProjectRules => match self.name.strip_prefix("rules/") {
                Some(rule) => self.project_dir(env)?.join(".claude/rules").join(rule),
                None => self.project_dir(env)?.join(&self.name),
            },
            SourceKey::CodexProjectRules | SourceKey::OpencodeProjectRules => {
                self.project_dir(env)?.join(&self.name)
            }
            _ => return Err(PlanError::NotSupported),
        })
    }

    pub fn id(&self) -> String {
        item_id_for(self.source, &[&self.name])
    }

    /// How many directories, at most, a new file of this kind may bring with it. A provider's
    /// own folder in the account is never made: if `~/.claude` is not there, that is a
    /// different problem. A project's `.claude` is the project's own and may be.
    pub fn most_new_dirs(&self) -> usize {
        match self.source {
            SourceKey::ClaudeUserSkills | SourceKey::CodexUserSkills => 2,
            SourceKey::ClaudeUserRules if self.name.starts_with("rules/") => 1,
            SourceKey::ClaudeProjectSkills => 3,
            SourceKey::ClaudeProjectRules if self.name.starts_with("rules/") => 2,
            _ => 0,
        }
    }

    /// The directories a file of this kind sits in below the provider's folder, deepest
    /// first, at most `count` of them: `skills/<name>` then `skills`; `rules`.
    pub fn directories(&self, env: &Environment, count: usize) -> Vec<PathBuf> {
        let mut chain = Vec::new();
        match self.source {
            SourceKey::ClaudeUserSkills => {
                chain.push(env.claude_dir().join("skills").join(&self.name));
                chain.push(env.claude_dir().join("skills"));
            }
            SourceKey::CodexUserSkills => {
                chain.push(env.agents_skills_dir().join(&self.name));
                chain.push(env.agents_skills_dir());
            }
            SourceKey::ClaudeUserRules if self.name.starts_with("rules/") => {
                chain.push(env.claude_dir().join("rules"));
            }
            SourceKey::ClaudeProjectSkills => {
                if let Ok(project) = self.project_dir(env) {
                    chain.push(project.join(".claude/skills").join(&self.name));
                    chain.push(project.join(".claude/skills"));
                    chain.push(project.join(".claude"));
                }
            }
            SourceKey::ClaudeProjectRules if self.name.starts_with("rules/") => {
                if let Ok(project) = self.project_dir(env) {
                    chain.push(project.join(".claude/rules"));
                    chain.push(project.join(".claude"));
                }
            }
            _ => (),
        }
        chain.truncate(count);
        chain
    }

    /// A skill is a folder: the file may be removed together with it only if it is alone.
    pub fn owns_folder(&self) -> bool {
        matches!(
            self.source,
            SourceKey::ClaudeUserSkills
                | SourceKey::CodexUserSkills
                | SourceKey::ClaudeProjectSkills
        )
    }

    pub fn label(&self) -> String {
        if self.owns_folder() {
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
        project: body.project.clone(),
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

/// A new skill starts with the front matter the provider reads: a name is the folder's, and
/// a description says when to use it. Anything else is not a skill the provider would load.
fn skill_text_ok(text: &str) -> bool {
    let mut lines = text.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return false;
    }
    let mut described = false;
    for line in lines {
        if line.trim_end() == "---" {
            return described;
        }
        described |= line
            .strip_prefix("description:")
            .is_some_and(|rest| !rest.trim().is_empty());
    }
    false
}

fn note(text: String) -> Vec<DiffLine> {
    vec![DiffLine {
        kind: DiffKind::Note,
        text,
    }]
}

/// A new file where nothing is. The inverse is the removal of exactly these bytes.
pub fn draft_create(
    target: Target,
    text: &str,
    dirs: usize,
    mode: u32,
) -> Result<Drafted, PlanError> {
    target.check()?;
    if !valid_text(text)
        || dirs > target.most_new_dirs()
        || (target.owns_folder() && !skill_text_ok(text))
    {
        return Err(PlanError::Invalid);
    }
    let mut body = inverse_of(target.source.provider(), target.source, b"", b"");
    body.event = EVENT.to_owned();
    body.matcher = Some(target.name.clone());
    body.group = dirs;
    body.project.clone_from(&target.project);
    body.index = mode as usize;
    body.h0 = ABSENT.to_owned();
    body.h1 = hash(text.as_bytes());
    Ok(Drafted {
        operation: Operation::CreateDocument,
        item: target.id(),
        name: target.label(),
        risk: Risk::WeakensSecurity,
        h0: ABSENT.to_owned(),
        h1: hash(text.as_bytes()),
        new_bytes: text.as_bytes().to_vec(),
        logical: Logical::DocumentCreate {
            target: target.clone(),
            dirs,
            mode,
        },
        prepared: Prepared::Inverse {
            body: Box::new(body),
            label: target.label(),
            secret_bearing: true,
            protection: Protection::Sealed,
        },
        diff: note(format!(
            "New instruction file {} ({} bytes, mode {:04o}){}. It does not exist now; undo removes it again. Content is kept out of this page.",
            target.label(),
            text.len(),
            mode & 0o7777,
            if dirs == 0 {
                String::new()
            } else {
                format!(", with {dirs} new folder(s)")
            }
        )),
        holds_hidden_values: true,
        prior: None,
        managed: None,
    })
}

/// A file leaves. Its exact bytes and mode are sealed first; the file is removed only if it
/// still holds them.
pub fn draft_delete(
    target: Target,
    before: &[u8],
    dirs: usize,
    mode: u32,
) -> Result<Drafted, PlanError> {
    target.check()?;
    let Ok(old) = std::str::from_utf8(before) else {
        // Bytes that are not text cannot be sealed in the record, so cannot be put back.
        return Err(PlanError::Invalid);
    };
    if before.len() > MOST_BYTES || dirs > usize::from(target.owns_folder()) {
        return Err(PlanError::Invalid);
    }
    let mut body = inverse_of(target.source.provider(), target.source, b"", b"");
    body.event = EVENT.to_owned();
    body.matcher = Some(target.name.clone());
    body.group = dirs;
    body.project.clone_from(&target.project);
    body.index = mode as usize;
    body.removed_text = old.to_owned();
    body.h0 = hash(before);
    body.h1 = ABSENT.to_owned();
    Ok(Drafted {
        operation: Operation::DeleteDocument,
        item: target.id(),
        name: target.label(),
        risk: Risk::WeakensSecurity,
        h0: hash(before),
        h1: ABSENT.to_owned(),
        new_bytes: Vec::new(),
        logical: Logical::DocumentRemove {
            target: target.clone(),
            dirs,
        },
        prepared: Prepared::Inverse {
            body: Box::new(body),
            label: target.label(),
            secret_bearing: true,
            protection: Protection::Sealed,
        },
        diff: note(format!(
            "Remove instruction file {} ({} bytes{}). Its exact bytes and mode are kept encrypted; undo puts it back only while the name is free.",
            target.label(),
            before.len(),
            if dirs == 0 {
                ""
            } else {
                ", and its now empty folder"
            }
        )),
        holds_hidden_values: true,
        prior: None,
        managed: None,
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
    body.project.clone_from(&target.project);
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

pub fn revert(bytes: Option<&[u8]>, record: &str, body: &RecordBody) -> Result<Drafted, PlanError> {
    let target = from_record(body)?;
    let restore = || Prepared::Restore {
        record: record.to_owned(),
        already_present: false,
    };
    // The undo of a creation removes exactly the bytes that were created.
    if body.h0 == ABSENT {
        let bytes = bytes.ok_or(PlanError::EntryGone)?;
        if hash(bytes) != body.h1 || bytes.len() > MOST_BYTES {
            return Err(PlanError::EntryChanged);
        }
        return Ok(Drafted {
            operation: Operation::RevertAuthored,
            item: target.id(),
            name: target.label(),
            risk: Risk::WeakensSecurity,
            h0: body.h1.clone(),
            h1: ABSENT.to_owned(),
            new_bytes: Vec::new(),
            logical: Logical::DocumentRemove {
                target,
                dirs: body.group,
            },
            prepared: restore(),
            diff: note("Remove the instruction file this change created, so it is absent again. A file that was edited since is refused.".to_owned()),
            holds_hidden_values: true,
            prior: None,
            managed: None,
        });
    }
    // The undo of a removal puts the exact bytes and mode back, if the name is still free.
    if body.h1 == ABSENT {
        if bytes.is_some() {
            return Err(PlanError::EntryChanged);
        }
        let prior = body.removed_text.clone().into_bytes();
        if prior.len() > MOST_BYTES || hash(&prior) != body.h0 {
            return Err(PlanError::RecordUnreadable);
        }
        let mode = u32::try_from(body.index).map_err(|_| PlanError::RecordUnreadable)?;
        return Ok(Drafted {
            operation: Operation::RevertAuthored,
            item: target.id(),
            name: target.label(),
            risk: Risk::WeakensSecurity,
            h0: ABSENT.to_owned(),
            h1: body.h0.clone(),
            new_bytes: prior,
            logical: Logical::DocumentCreate {
                target,
                dirs: body.group,
                mode: mode & 0o7777,
            },
            prepared: restore(),
            diff: note("Put the removed instruction file back with its exact bytes and mode. A file now at that name is never overwritten.".to_owned()),
            holds_hidden_values: true,
            prior: None,
            managed: None,
        });
    }
    let bytes = bytes.ok_or(PlanError::EntryGone)?;
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

/// What the page may ask to create: a closed word, never a path. The native window
/// collects the name where one is needed and the text; this says where each kind goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewKind {
    ClaudeSkill,
    ClaudeRule,
    ClaudeMemory,
    CodexSkill,
    CodexAgents,
    CodexOverride,
    OpencodeAgents,
}

impl NewKind {
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word {
            "claude_skill" => Self::ClaudeSkill,
            "claude_rule" => Self::ClaudeRule,
            "claude_memory" => Self::ClaudeMemory,
            "codex_skill" => Self::CodexSkill,
            "codex_agents" => Self::CodexAgents,
            "codex_override" => Self::CodexOverride,
            "opencode_agents" => Self::OpencodeAgents,
            _ => return None,
        })
    }

    pub const fn source(self) -> SourceKey {
        match self {
            Self::ClaudeSkill => SourceKey::ClaudeUserSkills,
            Self::ClaudeRule | Self::ClaudeMemory => SourceKey::ClaudeUserRules,
            Self::CodexSkill => SourceKey::CodexUserSkills,
            Self::CodexAgents | Self::CodexOverride => SourceKey::CodexUserRules,
            Self::OpencodeAgents => SourceKey::OpencodeUserRules,
        }
    }

    /// Where the same kind goes in a project; `None` for a kind the project level does not have
    /// (Codex skills and the override file are read only from the account).
    pub const fn project_source(self) -> Option<SourceKey> {
        Some(match self {
            Self::ClaudeSkill => SourceKey::ClaudeProjectSkills,
            Self::ClaudeRule | Self::ClaudeMemory => SourceKey::ClaudeProjectRules,
            Self::CodexAgents => SourceKey::CodexProjectRules,
            Self::OpencodeAgents => SourceKey::OpencodeProjectRules,
            Self::CodexSkill | Self::CodexOverride => return None,
        })
    }

    /// A kind with one possible file has its name here; the others ask for one.
    pub const fn fixed_name(self) -> Option<&'static str> {
        match self {
            Self::ClaudeMemory => Some("CLAUDE.md"),
            Self::CodexAgents | Self::OpencodeAgents => Some("AGENTS.md"),
            Self::CodexOverride => Some("AGENTS.override.md"),
            _ => None,
        }
    }

    pub const fn is_skill(self) -> bool {
        matches!(self, Self::ClaudeSkill | Self::CodexSkill)
    }

    /// The name as the target keeps it, from what was typed: a rule sits under `rules/` and
    /// ends in `.md`. The result is checked again by the target before anything is planned.
    pub fn name_from(self, typed: &str) -> Option<String> {
        let typed = typed.trim();
        match (self.fixed_name(), self) {
            (Some(name), _) => Some(name.to_owned()),
            (None, Self::ClaudeRule) => {
                let file = if typed.ends_with(".md") {
                    typed.to_owned()
                } else {
                    format!("{typed}.md")
                };
                Some(format!("rules/{file}"))
            }
            (None, _) => Some(typed.to_owned()),
        }
    }

    /// The name from what was typed, only if it is one this kind of file can have: a name with a
    /// separator, a dot at its start or an odd character is refused here, not later.
    pub fn checked_name(self, typed: &str) -> Option<String> {
        let name = self.name_from(typed)?;
        Target::user(self.source(), name.clone())
            .check()
            .ok()
            .map(|()| name)
    }

    /// What the editor starts with. A skill needs the front matter the provider reads.
    pub fn starting_text(self, name: &str) -> String {
        if self.is_skill() {
            format!("---\nname: {name}\ndescription: \n---\n\n")
        } else {
            String::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_is_a_closed_word_and_every_name_it_makes_passes_the_target_check() {
        for word in [
            "claude_skill",
            "claude_rule",
            "claude_memory",
            "codex_skill",
            "codex_agents",
            "codex_override",
            "opencode_agents",
        ] {
            let kind = NewKind::parse(word).unwrap();
            let name = kind.name_from("review").unwrap();
            let target = Target::user(kind.source(), name);
            assert!(target.check().is_ok(), "{word}");
            if kind.is_skill() {
                assert!(skill_text_ok(
                    &kind
                        .starting_text("review")
                        .replace("description: \n", "description: when reviewing\n")
                ));
            }
        }
        for word in ["", "CLAUDE.md", "../x", "claude_settings", "claude_skill "] {
            assert!(NewKind::parse(word).is_none(), "{word:?}");
        }
        // Typed text can never choose the file of a fixed kind.
        let kind = NewKind::parse("claude_memory").unwrap();
        assert_eq!(kind.name_from("../../etc/passwd").unwrap(), "CLAUDE.md");
        // A rule's typed name gets its extension and its folder, and no more.
        let rule = NewKind::parse("claude_rule").unwrap();
        assert_eq!(rule.name_from(" a ").unwrap(), "rules/a.md");
        assert_eq!(rule.name_from("a.md").unwrap(), "rules/a.md");
        let bad = Target::user(SourceKey::ClaudeUserRules, rule.name_from("../x").unwrap());
        assert!(bad.check().is_err());
        for typed in ["../x", "a/b", ".h", "wyss/console/f5b", "", "a\0b", " "] {
            assert!(rule.checked_name(typed).is_none(), "{typed:?}");
            let skill = NewKind::parse("claude_skill").unwrap();
            assert!(skill.checked_name(typed).is_none(), "{typed:?}");
        }
        assert_eq!(rule.checked_name("review").unwrap(), "rules/review.md");
    }

    #[test]
    fn a_files_project_must_match_its_source_in_names_and_in_records() {
        let project = Path::new("/home/u/repo");
        let ok = Target::in_project(SourceKey::ClaudeProjectRules, "CLAUDE.md", project);
        assert!(ok.check().is_ok());
        // A project source without a project, an account source with one, a relative or
        // dotted project, and names only the account's kind has.
        assert!(
            Target::user(SourceKey::ClaudeProjectRules, "CLAUDE.md")
                .check()
                .is_err()
        );
        assert!(
            Target::in_project(SourceKey::ClaudeUserRules, "CLAUDE.md", project)
                .check()
                .is_err()
        );
        for bad in ["repo", "/home/u/../x", "/", ""] {
            let target =
                Target::in_project(SourceKey::CodexProjectRules, "AGENTS.md", Path::new(bad));
            assert!(target.check().is_err(), "{bad:?}");
        }
        assert!(
            Target::in_project(SourceKey::CodexProjectRules, "AGENTS.override.md", project)
                .check()
                .is_err()
        );
        assert!(
            Target::in_project(SourceKey::ClaudeProjectSkills, "../x", project)
                .check()
                .is_err()
        );
        // A record names its project, and one without it (older records) is an account file.
        let mut body = inverse_of(ok.source.provider(), ok.source, b"", b"");
        body.event = EVENT.to_owned();
        body.matcher = Some("CLAUDE.md".to_owned());
        assert!(from_record(&body).is_err());
        body.project = Some("/home/u/repo".to_owned());
        assert_eq!(from_record(&body).unwrap(), ok);
        body.source = SourceKey::ClaudeUserRules;
        assert!(from_record(&body).is_err());
        // Without the field a stored record still reads, as an account file.
        let json = serde_json::to_string(&inverse_of(
            crate::agentconfig::ProviderId::ClaudeCode,
            SourceKey::ClaudeUserRules,
            b"",
            b"",
        ))
        .unwrap();
        assert!(!json.contains("project"));
        assert!(
            serde_json::from_str::<RecordBody>(&json)
                .unwrap()
                .project
                .is_none()
        );
    }

    #[test]
    fn absent_is_not_the_digest_of_an_empty_file() {
        assert_ne!(ABSENT, hash(b""));
        assert!(!ABSENT.bytes().all(|b| b.is_ascii_hexdigit()));
    }
}
