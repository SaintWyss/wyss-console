//! What an Arsenal selection adds to a Native launch: installed skills this session must not
//! expose. It only ever switches skills off, through the same per-session `--settings` overlay a
//! profile writes; it never adds a skill, a hook, an MCP server or a flag, and it never switches
//! hooks off. The window's desktop compiles it from the Arsenal library and the installed skills;
//! the host checks its shape again here and binds it to the preview's `plan_token`.
use super::catalogue::AgentId;
use super::profile::LaunchPlan;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// Most installed skills one launch may switch off.
pub const MAX_SKILLS_OFF: usize = 1024;
const MAX_SELECTION_NAME: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArsenalLaunch {
    /// The Arsenal selection this launch was compiled from: its id and its name, for display.
    pub selection: String,
    pub selection_name: String,
    /// The SHA-256 of the effective configuration the window previewed.
    pub config_hash: String,
    /// Installed skills switched off for this session, by their exact provider names.
    pub skills_off: Vec<String>,
}

/// What a session keeps of the launch: enough to say what it started with, and to restart it.
pub type ArsenalRef = ArsenalLaunch;

fn hex64(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The provider's skill name: the folder name a skill is installed under.
fn skill_ok(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && !name.starts_with('.')
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':'))
}

impl ArsenalLaunch {
    pub fn validate(&self) -> Result<(), &'static str> {
        let id_ok = (1..=64).contains(&self.selection.len())
            && self
                .selection
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-');
        let name_ok = !self.selection_name.trim().is_empty()
            && self.selection_name.chars().count() <= MAX_SELECTION_NAME
            && !self.selection_name.chars().any(char::is_control);
        if !id_ok || !name_ok || !hex64(&self.config_hash) {
            return Err("arsenal_invalid");
        }
        let mut seen = BTreeSet::new();
        if self.skills_off.len() > MAX_SKILLS_OFF
            || self
                .skills_off
                .iter()
                .any(|name| !skill_ok(name) || !seen.insert(name.as_str()))
        {
            return Err("arsenal_invalid");
        }
        Ok(())
    }
}

/// Adds the exclusion to a plan. For a provider without a per-session skill switch, or a plan that
/// switches hooks off, the plan says it cannot apply: an Arsenal launch never runs without the
/// owner's hooks, and never pretends a selection applied where nothing could carry it.
pub fn apply(plan: &mut LaunchPlan, agent: AgentId, launch: &ArsenalLaunch) {
    if launch.validate().is_err() {
        plan.not_applicable
            .push("The Arsenal selection is not well formed.".to_owned());
        return;
    }
    if agent != AgentId::ClaudeCode {
        plan.not_applicable
            .push("Arsenal can switch skills off for one session only in Claude Code.".to_owned());
        return;
    }
    let mut overlay: Map<String, Value> = match plan.overlay.as_deref() {
        None => Map::new(),
        Some(text) => match serde_json::from_str(text) {
            Ok(Value::Object(map)) => map,
            _ => {
                plan.not_applicable
                    .push("The session settings could not be combined.".to_owned());
                return;
            }
        },
    };
    if overlay.contains_key("disableAllHooks") {
        plan.not_applicable.push(
            "An Arsenal selection does not launch with hooks off. Choose a profile that keeps hooks."
                .to_owned(),
        );
        return;
    }
    let skills = overlay
        .entry("skillOverrides")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(skills) = skills.as_object_mut() else {
        plan.not_applicable
            .push("The session settings could not be combined.".to_owned());
        return;
    };
    for name in &launch.skills_off {
        skills.insert(name.clone(), Value::String("off".to_owned()));
    }
    plan.applied.push(format!(
        "Arsenal selection \"{}\": {} installed skill(s) switched off for this session.",
        launch.selection_name,
        launch.skills_off.len()
    ));
    if plan.overlay.is_none() {
        plan.words
            .push(super::profile::Word::Literal("--settings".to_owned()));
        plan.words.push(super::profile::Word::Overlay);
    }
    plan.overlay = serde_json::to_string_pretty(&Value::Object(overlay)).ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::profile::{self, Body, HooksMode, Resolved, Word};

    fn launch(off: &[&str]) -> ArsenalLaunch {
        ArsenalLaunch {
            selection: "0b1c4e1a-0000-4000-8000-000000000001".to_owned(),
            selection_name: "Frontend".to_owned(),
            config_hash: "a".repeat(64),
            skills_off: off.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    fn resolved(body: Body) -> Resolved {
        Resolved {
            body,
            from: Default::default(),
        }
    }

    #[test]
    fn an_exclusion_becomes_a_settings_overlay_with_only_skill_overrides() {
        let mut plan = LaunchPlan::default();
        apply(
            &mut plan,
            AgentId::ClaudeCode,
            &launch(&["design", "gsd-help"]),
        );
        assert!(plan.not_applicable.is_empty());
        assert_eq!(
            plan.words,
            vec![Word::Literal("--settings".to_owned()), Word::Overlay]
        );
        let overlay: Value = serde_json::from_str(plan.overlay.as_deref().unwrap()).unwrap();
        assert_eq!(
            overlay,
            serde_json::json!({"skillOverrides": {"design": "off", "gsd-help": "off"}})
        );
    }

    #[test]
    fn a_profile_overlay_is_extended_not_replaced_and_the_flag_is_not_repeated() {
        let body = Body {
            name: "P".into(),
            applies_to: vec![AgentId::ClaudeCode],
            skills_off: vec!["old".into()],
            ..Body::default()
        };
        let mut plan = profile::plan(&resolved(body), AgentId::ClaudeCode, true);
        apply(&mut plan, AgentId::ClaudeCode, &launch(&["new"]));
        assert_eq!(
            plan.words
                .iter()
                .filter(|w| **w == Word::Literal("--settings".into()))
                .count(),
            1
        );
        let overlay: Value = serde_json::from_str(plan.overlay.as_deref().unwrap()).unwrap();
        assert_eq!(overlay["skillOverrides"]["old"], "off");
        assert_eq!(overlay["skillOverrides"]["new"], "off");
    }

    #[test]
    fn hooks_off_codex_and_malformed_launches_do_not_apply() {
        let body = Body {
            name: "P".into(),
            applies_to: vec![AgentId::ClaudeCode],
            hooks: Some(HooksMode::Off),
            ..Body::default()
        };
        let mut plan = profile::plan(&resolved(body), AgentId::ClaudeCode, true);
        apply(&mut plan, AgentId::ClaudeCode, &launch(&["a"]));
        assert_eq!(plan.not_applicable.len(), 1);

        let mut codex = LaunchPlan::default();
        apply(&mut codex, AgentId::Codex, &launch(&["a"]));
        assert_eq!(codex.not_applicable.len(), 1);
        assert!(codex.overlay.is_none());

        for bad in [
            launch(&["../x"]),
            launch(&["a", "a"]),
            launch(&["--bare"]),
            ArsenalLaunch {
                config_hash: "z".repeat(64),
                ..launch(&[])
            },
            ArsenalLaunch {
                selection_name: "line\nbreak".into(),
                ..launch(&[])
            },
        ] {
            let mut plan = LaunchPlan::default();
            apply(&mut plan, AgentId::ClaudeCode, &bad);
            assert_eq!(plan.not_applicable.len(), 1, "{bad:?}");
            assert!(plan.overlay.is_none());
        }
        let too_many: Vec<String> = (0..=MAX_SKILLS_OFF).map(|i| format!("s{i}")).collect();
        let refs: Vec<&str> = too_many.iter().map(String::as_str).collect();
        assert!(launch(&refs).validate().is_err());
    }
}
