//! Session profiles (F3c, 09 §4): what one session should run with. A profile holds names and
//! closed words, never a secret and never a free command; it is applied when the session
//! starts and changes no global file.
use super::catalogue::AgentId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_NAME: usize = 60;
pub const MAX_NOTES: usize = 500;
pub const MAX_SKILLS: usize = 64;
pub const MAX_DEPTH: usize = 8;

/// A start flag the provider has, as one closed word. Full access is its own explicit word
/// (`bypass`, ADR 0059), never reached through another one; `danger-full-access` alone is not
/// in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// Ask before acting.
    Ask,
    /// Plan or read only: nothing is changed.
    ReadOnly,
    /// Edits are accepted without asking (Claude Code only).
    Edits,
    /// Nothing is asked and nothing is contained: the provider's own skip-everything flag
    /// (`--dangerously-skip-permissions`, `--dangerously-bypass-approvals-and-sandbox`).
    Bypass,
}

/// What `Permission::Bypass` costs, shown before a start and on the session.
pub const BYPASS_COST: &str = "Full access: the agent asks nothing and runs every command and \
     edit without a sandbox, with your account's rights.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HooksMode {
    /// Whatever the user's files say.
    Inherit,
    /// None for this session (Claude Code only).
    Off,
}

/// The content of one version of a profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Body {
    pub name: String,
    pub applies_to: Vec<AgentId>,
    #[serde(default)]
    pub extends: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// A reasoning effort, as the provider names it. Opaque like the model: it is passed, never
    /// interpreted, and the product cannot tell whether the agent knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default)]
    pub permission: Option<Permission>,
    #[serde(default)]
    pub hooks: Option<HooksMode>,
    #[serde(default)]
    pub skills_off: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileError {
    NameInvalid,
    AppliesToInvalid,
    ModelInvalid,
    EffortInvalid,
    SkillInvalid,
    NotesInvalid,
    NotesHoldSecret,
    ExtendsUnknown,
    ProfileCycle,
    TooDeep,
    Unknown,
    ReadOnlyTemplate,
    TooMany,
    VersionUnknown,
    ImportInvalid,
    StoreUnavailable,
    InUse,
}

fn text_ok(text: &str, most: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= most && !text.chars().any(char::is_control)
}

fn model_ok(model: &str) -> bool {
    (1..=64).contains(&model.len())
        && model.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-' | b'/' | b'[' | b']')
        })
        && !model.starts_with('-')
}

/// An effort word: letters, digits and `. _ -`, short, and not starting with a dash. Nothing
/// that could end a TOML string or start a flag, because it is passed on a command line.
fn effort_ok(effort: &str) -> bool {
    (1..=32).contains(&effort.len())
        && effort
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        && !effort.starts_with('-')
}

/// The documented Claude Code `--effort` levels; the desktop separately checks exact-build
/// syntax evidence before passing a profile override to an installed CLI.
pub const CLAUDE_EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

fn skill_ok(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':'))
        && !name.starts_with('-')
}

impl Body {
    /// The shape rules; whether `extends` names a profile that exists is the store's.
    pub fn validate(&self) -> Result<(), ProfileError> {
        if !text_ok(&self.name, MAX_NAME) {
            return Err(ProfileError::NameInvalid);
        }
        let mut seen = self.applies_to.clone();
        seen.sort();
        seen.dedup();
        if self.applies_to.is_empty() || seen.len() != self.applies_to.len() {
            return Err(ProfileError::AppliesToInvalid);
        }
        if self.model.as_deref().is_some_and(|m| !model_ok(m)) {
            return Err(ProfileError::ModelInvalid);
        }
        if self.effort.as_deref().is_some_and(|e| !effort_ok(e)) {
            return Err(ProfileError::EffortInvalid);
        }
        if self.skills_off.len() > MAX_SKILLS || self.skills_off.iter().any(|s| !skill_ok(s)) {
            return Err(ProfileError::SkillInvalid);
        }
        if self.notes.chars().count() > MAX_NOTES
            || self.notes.chars().any(|c| c.is_control() && c != '\n')
        {
            return Err(ProfileError::NotesInvalid);
        }
        // A profile holds no secret (T-16): free text is the only place one could be typed.
        if crate::agentconfig::redact::text(&self.notes).may_hold_secret()
            || crate::agentconfig::redact::text(&self.name).may_hold_secret()
        {
            return Err(ProfileError::NotesHoldSecret);
        }
        Ok(())
    }
}

/// A profile after inheritance, with the profile each field came from (09 §4.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Resolved {
    pub body: Body,
    pub from: BTreeMap<&'static str, String>,
}

/// Merges `chain` (the profile first, then its parent, then the parent's parent…): a scalar
/// of the child replaces the parent's; the lists combine. `applies_to` is the child's own.
pub fn merge(chain: &[(String, Body)]) -> Resolved {
    let mut resolved = Body::default();
    let mut from = BTreeMap::new();
    let (first_name, first) = &chain[0];
    resolved.name.clone_from(&first.name);
    resolved.applies_to.clone_from(&first.applies_to);
    resolved.extends.clone_from(&first.extends);
    resolved.notes.clone_from(&first.notes);
    // Parents first, so that a nearer profile overrides.
    for (name, body) in chain.iter().rev() {
        if let Some(model) = &body.model {
            resolved.model = Some(model.clone());
            from.insert("model", name.clone());
        }
        if let Some(effort) = &body.effort {
            resolved.effort = Some(effort.clone());
            from.insert("effort", name.clone());
        }
        if let Some(permission) = body.permission {
            resolved.permission = Some(permission);
            from.insert("permission", name.clone());
        }
        if let Some(hooks) = body.hooks {
            resolved.hooks = Some(hooks);
            from.insert("hooks", name.clone());
        }
        for skill in &body.skills_off {
            if !resolved.skills_off.contains(skill) {
                resolved.skills_off.push(skill.clone());
            }
            from.entry("skills_off").or_insert_with(|| name.clone());
        }
    }
    let _ = first_name;
    Resolved {
        body: resolved,
        from,
    }
}

/// One word of the command line. The settings overlay's path is only known once the session's
/// own directory exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Word {
    Literal(String),
    Overlay,
}

/// What a profile turns into for one agent (09 §4.2): the words, the file to generate, and
/// three lists the user reads before starting.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct LaunchPlan {
    /// After the executable name.
    pub words: Vec<Word>,
    /// The overlay's content, when there is one.
    pub overlay: Option<String>,
    pub applied: Vec<String>,
    pub with_cost: Vec<String>,
    pub not_applicable: Vec<String>,
}

pub fn plan(profile: &Resolved, agent: AgentId, applies: bool) -> LaunchPlan {
    let body = &profile.body;
    let mut plan = LaunchPlan::default();
    let literal = |plan: &mut LaunchPlan, words: &[&str]| {
        plan.words
            .extend(words.iter().map(|w| Word::Literal((*w).to_owned())));
    };
    if !applies {
        plan.not_applicable
            .push("This profile was not written for this agent.".to_owned());
        return plan;
    }
    if let Some(model) = &body.model {
        match agent {
            AgentId::ClaudeCode => literal(&mut plan, &["--model", model]),
            AgentId::Codex => literal(&mut plan, &["-m", model]),
            AgentId::Opencode => literal(&mut plan, &["--model", model]),
            AgentId::Hermes => plan.not_applicable.push(
                "Hermes profile overrides remain unavailable until this exact installed build can be identified without running provider update checks.".to_owned(),
            ),
        }
        plan.applied.push(format!("Model: {model}."));
    }
    if let Some(effort) = &body.effort {
        match agent {
            // Claude Code documents a closed set; a word outside it is not passed.
            AgentId::ClaudeCode if CLAUDE_EFFORTS.contains(&effort.as_str()) => {
                literal(&mut plan, &["--effort", effort]);
                plan.applied.push(format!("Reasoning effort: {effort}."));
            }
            AgentId::ClaudeCode => plan.not_applicable.push(format!(
                "Claude Code documents these effort levels: {}. \"{effort}\" is not one of them, so it is not passed.",
                CLAUDE_EFFORTS.join(", ")
            )),
            // Codex has no start flag of its own for effort; its documented way is the
            // configuration key, overridden for this session only. The word is shape-checked,
            // so it cannot end the string.
            AgentId::Codex => {
                literal(
                    &mut plan,
                    &["-c", &format!("model_reasoning_effort=\"{effort}\"")],
                );
                plan.applied.push(format!(
                    "Reasoning effort: {effort} (this session only; Codex reads it as a configuration override)."
                ));
            }
            AgentId::Opencode | AgentId::Hermes => plan.not_applicable.push(
                "This provider has no qualified reasoning-effort profile interface.".to_owned(),
            ),
        }
    }
    match (agent, body.permission) {
        (_, None) => {}
        (AgentId::ClaudeCode, Some(permission)) => {
            let mode = match permission {
                Permission::Ask => "manual",
                Permission::ReadOnly => "plan",
                Permission::Edits => "acceptEdits",
                Permission::Bypass => {
                    literal(&mut plan, &["--dangerously-skip-permissions"]);
                    plan.applied
                        .push("Permissions: skipped (--dangerously-skip-permissions).".to_owned());
                    plan.with_cost.push(BYPASS_COST.to_owned());
                    ""
                }
            };
            if !mode.is_empty() {
                literal(&mut plan, &["--permission-mode", mode]);
                plan.applied.push(format!("Permission mode: {mode}."));
            }
        }
        (AgentId::Codex, Some(Permission::Bypass)) => {
            literal(&mut plan, &["--dangerously-bypass-approvals-and-sandbox"]);
            plan.applied.push(
                "Codex: no approvals and no sandbox (--dangerously-bypass-approvals-and-sandbox)."
                    .to_owned(),
            );
            plan.with_cost.push(BYPASS_COST.to_owned());
        }
        (AgentId::Codex, Some(Permission::Ask)) => {
            literal(&mut plan, &["-s", "workspace-write", "-a", "on-request"]);
            plan.applied
                .push("Codex: workspace-write sandbox, asks when it decides to.".to_owned());
        }
        (AgentId::Codex, Some(Permission::ReadOnly)) => {
            literal(&mut plan, &["-s", "read-only", "-a", "on-request"]);
            plan.applied
                .push("Codex: read-only sandbox, asks when it decides to.".to_owned());
        }
        (AgentId::Codex, Some(Permission::Edits)) => plan
            .not_applicable
            .push("Codex has no separate mode that accepts edits without asking.".to_owned()),
        (AgentId::Opencode | AgentId::Hermes, Some(_)) => plan.not_applicable.push(
            "This provider's permission controls are not managed by Native profiles.".to_owned(),
        ),
    }
    let mut overlay = serde_json::Map::new();
    match (agent, body.hooks) {
        (_, None | Some(HooksMode::Inherit)) => {}
        (AgentId::ClaudeCode, Some(HooksMode::Off)) => {
            overlay.insert("disableAllHooks".to_owned(), serde_json::Value::Bool(true));
            plan.with_cost.push(
                "Hooks are off for this session only. Your user settings still load and no \
                 global file is changed."
                    .to_owned(),
            );
        }
        (AgentId::Codex, Some(HooksMode::Off)) => plan
            .not_applicable
            .push("Codex hooks cannot be switched off for one session.".to_owned()),
        (AgentId::Opencode | AgentId::Hermes, Some(HooksMode::Off)) => plan
            .not_applicable
            .push("This provider's hooks cannot be changed by a Native profile.".to_owned()),
    }
    if !body.skills_off.is_empty() {
        match agent {
            AgentId::ClaudeCode => {
                let mut skills = serde_json::Map::new();
                for skill in &body.skills_off {
                    skills.insert(skill.clone(), serde_json::Value::String("off".to_owned()));
                }
                overlay.insert(
                    "skillOverrides".to_owned(),
                    serde_json::Value::Object(skills),
                );
                plan.applied
                    .push(format!("{} skill(s) switched off.", body.skills_off.len()));
            }
            AgentId::Codex => plan
                .not_applicable
                .push("Codex skills cannot be switched off for one session.".to_owned()),
            AgentId::Opencode | AgentId::Hermes => plan
                .not_applicable
                .push("This provider's skills cannot be changed by a Native profile.".to_owned()),
        }
    }
    if !overlay.is_empty() {
        plan.words.push(Word::Literal("--settings".to_owned()));
        plan.words.push(Word::Overlay);
        plan.overlay = serde_json::to_string_pretty(&serde_json::Value::Object(overlay)).ok();
    }
    plan
}

/// The profiles that ship with the product (09 §4.6): names for common shapes, read only,
/// meant to be copied. Each is only as strong as the provider's start flags make it.
pub fn templates() -> Vec<(&'static str, Body)> {
    let both = vec![AgentId::ClaudeCode, AgentId::Codex];
    vec![
        (
            "template-as-configured",
            Body {
                name: "As configured".to_owned(),
                applies_to: both.clone(),
                notes: "Nothing is changed: the agent starts as it does when typed.".to_owned(),
                ..Body::default()
            },
        ),
        (
            "template-review",
            Body {
                name: "Review, read only".to_owned(),
                applies_to: both.clone(),
                permission: Some(Permission::ReadOnly),
                notes: "Plan mode for Claude Code, read-only sandbox for Codex.".to_owned(),
                ..Body::default()
            },
        ),
        (
            "template-exploration",
            Body {
                name: "Exploration, fewer hooks".to_owned(),
                applies_to: vec![AgentId::ClaudeCode],
                hooks: Some(HooksMode::Off),
                notes: "No hooks in this session. Claude Code only.".to_owned(),
                ..Body::default()
            },
        ),
        (
            "template-full-access",
            Body {
                name: "Full access (bypass)".to_owned(),
                applies_to: both.clone(),
                permission: Some(Permission::Bypass),
                notes: "No permission prompts and no sandbox, as \
                        claude --dangerously-skip-permissions and \
                        codex --dangerously-bypass-approvals-and-sandbox."
                    .to_owned(),
                ..Body::default()
            },
        ),
        (
            "template-strict",
            Body {
                name: "Strict".to_owned(),
                applies_to: both,
                permission: Some(Permission::Ask),
                notes: "Asks before acting.".to_owned(),
                ..Body::default()
            },
        ),
    ]
}
