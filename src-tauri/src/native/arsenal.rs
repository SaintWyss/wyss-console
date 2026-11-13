//! What an Arsenal selection adds to a Native launch: installed skills this session must not
//! expose, through the same per-session `--settings` overlay a profile writes, and, for Claude Code,
//! the launch environment the preset chose (its own configuration folder: instructions, hooks, MCP
//! servers and plugins left out). It never adds a skill, a hook or an MCP server the owner does not
//! already have, and never switches every hook off. The window's desktop compiles it from the Arsenal library and the installed skills;
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
    /// For Codex, which switches skills off by file: the `SKILL.md` of each skill switched off.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skill_files_off: Vec<String>,
    /// The launch environment the preset chose (instructions, hooks, MCP, plugins, memory). Absent
    /// when the preset keeps the owner's own, so an older host still accepts every such launch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<super::launch_home::HomeSpec>,
}

/// Longest `SKILL.md` path one launch may name.
const MAX_FILE_PATH: usize = 4096;

/// A `SKILL.md` Codex reads, named the way the launch names it: absolute, plain, never one of Codex's
/// own system skills.
fn skill_file_ok(path: &str) -> bool {
    path.len() <= MAX_FILE_PATH
        && path.starts_with('/')
        && path.ends_with("/SKILL.md")
        && !path.chars().any(char::is_control)
        && !path
            .split('/')
            .any(|part| part == ".." || part == "." || part == ".system")
}

/// A `SKILL.md` the host may name for Codex: inside a Codex skill folder (`<CODEX_HOME>/skills`, or an
/// `.agents/skills` folder of the account or a project), and a regular file now, not a link.
fn codex_skill_file_on_disk(file: &str, codex_home: Option<&std::path::Path>) -> bool {
    let path = std::path::Path::new(file);
    let in_codex_home = codex_home.is_some_and(|home| path.starts_with(home.join("skills")));
    let parts: Vec<&std::ffi::OsStr> = path.iter().collect();
    let in_agents = parts
        .windows(2)
        .any(|pair| pair[0] == ".agents" && pair[1] == "skills");
    (in_codex_home || in_agents)
        && std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_file())
}

/// The value of Codex's `-c` flag that switches these skills off for one session:
/// `skills.config=[{path = "…/SKILL.md", enabled = false}, …]`, serialized as TOML so no path can
/// change its shape. Codex adds these entries to the account's own and never switches one on.
pub fn codex_config_value(files: &[String]) -> String {
    let entries: Vec<toml::Value> = files
        .iter()
        .map(|file| {
            let mut table = toml::map::Map::new();
            table.insert("path".to_owned(), toml::Value::String(file.clone()));
            table.insert("enabled".to_owned(), toml::Value::Boolean(false));
            toml::Value::Table(table)
        })
        .collect();
    format!("skills.config={}", toml::Value::Array(entries))
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
        let mut files = BTreeSet::new();
        if self.skill_files_off.len() > MAX_SKILLS_OFF
            || self
                .skill_files_off
                .iter()
                .any(|file| !skill_file_ok(file) || !files.insert(file.as_str()))
        {
            return Err("arsenal_invalid");
        }
        if let Some(home) = &self.home {
            home.validate().map_err(|_| "arsenal_invalid")?;
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
    let home = launch.home.as_ref().filter(|home| !home.is_default());
    match agent {
        AgentId::ClaudeCode if launch.skill_files_off.is_empty() => {}
        AgentId::Codex if home.is_some() => {
            plan.not_applicable.push(
                "Choosing instructions, hooks, MCP or plugins per terminal is not available for Codex yet."
                    .to_owned(),
            );
            return;
        }
        AgentId::Codex => return apply_codex(plan, launch),
        AgentId::ClaudeCode => {
            plan.not_applicable
                .push("The Arsenal selection is not well formed.".to_owned());
            return;
        }
        _ => {
            plan.not_applicable.push(
                "Arsenal can switch skills off for one session only in Claude Code and Codex."
                    .to_owned(),
            );
            return;
        }
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
    if let Some(home) = home {
        plan.applied.push(home_summary(home));
        plan.home = Some(home.clone());
    }
    if plan.overlay.is_none() {
        plan.words
            .push(super::profile::Word::Literal("--settings".to_owned()));
        plan.words.push(super::profile::Word::Overlay);
    }
    plan.overlay = serde_json::to_string_pretty(&Value::Object(overlay)).ok();
}

/// What the launch home changes, in one line the owner reads before starting.
fn home_summary(home: &super::launch_home::HomeSpec) -> String {
    let mut parts = Vec::new();
    if !home.instructions {
        parts.push("without CLAUDE.md and rules".to_owned());
    } else if !home.rules_off.is_empty() {
        parts.push(format!("{} rule file(s) left out", home.rules_off.len()));
    }
    if !home.hooks_off.is_empty() {
        parts.push(format!("{} hook(s) left out", home.hooks_off.len()));
    }
    if let Some(mcp) = &home.mcp {
        parts.push(format!("only {} MCP server(s)", mcp.len()));
    }
    if !home.plugins_off.is_empty() {
        parts.push(format!("{} plugin(s) off", home.plugins_off.len()));
    }
    if !home.memory {
        parts.push("auto memory off".to_owned());
    }
    format!(
        "Own launch folder, signed in with the stored long-lived token: {}.",
        parts.join(", ")
    )
}

/// Codex: the session runs without the shared background server, so the switch reaches only this
/// process, and with one `-c skills.config=[…]` naming each skill's `SKILL.md` with
/// `enabled = false`. Every file must exist now: Codex ignores a path it does not know silently.
fn apply_codex(plan: &mut LaunchPlan, launch: &ArsenalLaunch) {
    if !launch.skills_off.is_empty() && launch.skill_files_off.is_empty() {
        plan.not_applicable
            .push("The Arsenal selection names skills but not their files.".to_owned());
        return;
    }
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let codex_home = std::env::var_os("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| home.as_ref().map(|home| home.join(".codex")));
    if launch
        .skill_files_off
        .iter()
        .any(|file| !codex_skill_file_on_disk(file, codex_home.as_deref()))
    {
        plan.not_applicable.push(
            "A skill the Arsenal selection switches off is no longer installed. Check the plan again."
                .to_owned(),
        );
        return;
    }
    let literal = |plan: &mut LaunchPlan, word: String| {
        plan.words.push(super::profile::Word::Literal(word));
    };
    literal(plan, "--no-daemon".to_owned());
    if !launch.skill_files_off.is_empty() {
        literal(plan, "-c".to_owned());
        literal(plan, codex_config_value(&launch.skill_files_off));
    }
    plan.applied.push(format!(
        "Arsenal selection \"{}\": {} installed skill(s) switched off for this session, which runs without the shared Codex server.",
        launch.selection_name,
        launch.skills_off.len()
    ));
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
            skill_files_off: Vec::new(),
            home: None,
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
    fn a_codex_exclusion_runs_without_the_shared_server_and_names_each_file_off() {
        let dir = std::env::temp_dir().join(format!("arsenal-codex-{}", std::process::id()));
        let skills = dir.join("project/.agents/skills");
        std::fs::create_dir_all(skills.join("a b")).unwrap();
        std::fs::write(skills.join("a b/SKILL.md"), "x").unwrap();
        let file = skills.join("a b/SKILL.md").to_string_lossy().into_owned();
        // Outside a Codex skill folder, or a link, it is refused at the host as well.
        std::fs::create_dir_all(dir.join("elsewhere")).unwrap();
        std::fs::write(dir.join("elsewhere/SKILL.md"), "x").unwrap();
        std::fs::create_dir_all(skills.join("linked")).unwrap();
        std::os::unix::fs::symlink(
            dir.join("elsewhere/SKILL.md"),
            skills.join("linked/SKILL.md"),
        )
        .unwrap();
        for refused in [
            dir.join("elsewhere/SKILL.md"),
            skills.join("linked/SKILL.md"),
        ] {
            let mut plan = LaunchPlan::default();
            apply(
                &mut plan,
                AgentId::Codex,
                &ArsenalLaunch {
                    skill_files_off: vec![refused.to_string_lossy().into_owned()],
                    ..launch(&["a"])
                },
            );
            assert_eq!(plan.not_applicable.len(), 1, "{refused:?}");
            assert!(plan.words.is_empty());
        }
        let with = |files: Vec<String>| ArsenalLaunch {
            skill_files_off: files,
            ..launch(&["a"])
        };
        let mut plan = LaunchPlan::default();
        apply(&mut plan, AgentId::Codex, &with(vec![file.clone()]));
        assert!(plan.not_applicable.is_empty(), "{:?}", plan.not_applicable);
        assert!(plan.overlay.is_none());
        let words: Vec<String> = plan
            .words
            .iter()
            .map(|w| match w {
                Word::Literal(text) => text.clone(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(words[..2], ["--no-daemon".to_owned(), "-c".to_owned()]);
        let value: toml::Table =
            toml::from_str(&words[2].replacen("skills.config=", "x=", 1)).unwrap();
        assert_eq!(value["x"][0]["path"].as_str(), Some(file.as_str()));
        assert_eq!(value["x"][0]["enabled"].as_bool(), Some(false));
        // A file that is gone by start time is not silently ignored by the provider: it is refused.
        std::fs::remove_dir_all(&dir).unwrap();
        let mut gone = LaunchPlan::default();
        apply(&mut gone, AgentId::Codex, &with(vec![file]));
        assert_eq!(gone.not_applicable.len(), 1);
        assert!(gone.words.is_empty());
        // Claude never takes files.
        let mut claude = LaunchPlan::default();
        apply(
            &mut claude,
            AgentId::ClaudeCode,
            &with(vec!["/h/.agents/skills/a/SKILL.md".into()]),
        );
        assert_eq!(claude.not_applicable.len(), 1);
    }

    #[test]
    fn a_chosen_environment_reaches_the_claude_plan_and_is_refused_for_codex() {
        use crate::native::launch_home::{HomeSpec, hook_key};
        let home = HomeSpec {
            instructions: false,
            memory: true,
            hooks_off: vec![hook_key("PreToolUse", "Bash", "gate.py")],
            mcp: Some(vec!["context7".into()]),
            ..HomeSpec::default()
        };
        let with = ArsenalLaunch {
            home: Some(home.clone()),
            ..launch(&["design"])
        };
        let mut plan = LaunchPlan::default();
        apply(&mut plan, AgentId::ClaudeCode, &with);
        assert!(plan.not_applicable.is_empty());
        assert_eq!(plan.home, Some(home.clone()));
        assert!(
            plan.applied
                .iter()
                .any(|line| line.contains("without CLAUDE.md and rules")
                    && line.contains("1 hook(s) left out")
                    && line.contains("only 1 MCP server(s)"))
        );
        // A default environment changes nothing: the owner's own folder is used.
        let default = ArsenalLaunch {
            home: Some(HomeSpec {
                instructions: true,
                memory: true,
                ..HomeSpec::default()
            }),
            ..launch(&["design"])
        };
        let mut plain = LaunchPlan::default();
        apply(&mut plain, AgentId::ClaudeCode, &default);
        assert_eq!(plain.home, None);
        let mut codex = LaunchPlan::default();
        apply(&mut codex, AgentId::Codex, &with);
        assert_eq!(codex.not_applicable.len(), 1);
        assert!(codex.words.is_empty());
        let bad = ArsenalLaunch {
            home: Some(HomeSpec {
                rules_off: vec!["../x".into()],
                ..home
            }),
            ..launch(&[])
        };
        assert!(bad.validate().is_err());
        // An older host's launch (no environment) still parses, and the field is not sent when absent.
        let text = serde_json::to_string(&launch(&["a"])).unwrap();
        assert!(!text.contains("home"));
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

        // Codex needs the file of each name: names alone do not apply.
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
        for bad_file in [
            "relative/SKILL.md",
            "/h/.agents/skills/a",
            "/h/.codex/skills/.system/imagegen/SKILL.md",
            "/h/../etc/SKILL.md",
            "/h/a\nb/SKILL.md",
        ] {
            let bad = ArsenalLaunch {
                skill_files_off: vec![bad_file.into()],
                ..launch(&["a"])
            };
            assert!(bad.validate().is_err(), "{bad_file}");
        }
        let too_many: Vec<String> = (0..=MAX_SKILLS_OFF).map(|i| format!("s{i}")).collect();
        let refs: Vec<&str> = too_many.iter().map(String::as_str).collect();
        assert!(launch(&refs).validate().is_err());
    }
}
