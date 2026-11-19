//! The configuration one terminal would start with: what a saved selection becomes for one provider
//! build, in one project, against the skills installed right now. It is computed, never assumed:
//! a selected capability is `prepared` when the provider already has exactly that revision
//! installed, and `from_arsenal` (copied into the session's own folder) when it is not installed or
//! its installed copy differs. Unselected, switchable skills are named for exclusion. Native
//! checks the installed CLI before passing flags. Nothing here writes a file or starts a process.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::analysis::{self, Compatibility, Dependency};
use super::store::{self, Selection};
use crate::agentconfig::catalogue::{Composite, LaunchScan};
use crate::native::arsenal::MAX_SKILLS_OFF;

/// Codex's own skills, which it installs and replaces itself: never switched off.
const SYSTEM_SKILLS: &str = "codex_system_skills";
use crate::native::capabilities::ArsenalEvidence;

/// One item of the selection as the library holds it now.
pub struct Resolved {
    pub capability: String,
    /// `resolved`, `missing` (the capability is gone) or `revision_missing` (the pin is gone).
    pub state: &'static str,
    pub name: String,
    pub kind: String,
    pub provider: String,
    pub revision: Option<i64>,
    pub pinned: bool,
    pub tree_hash: String,
    pub files: Vec<(String, Vec<u8>)>,
}

pub struct Input<'a> {
    pub selection: &'a Selection,
    pub items: Vec<Resolved>,
    pub provider: &'a str,
    pub version: Option<String>,
    pub evidence: Option<ArsenalEvidence>,
    pub installed: &'a LaunchScan,
    /// The project's folder name, for display, and its key, for the hash.
    pub project: Option<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Enforcement {
    /// `available`, `unqualified`, `version_unknown` or `incomplete_inventory`.
    pub state: &'static str,
    pub reason: String,
    pub evidence: Option<ArsenalEvidence>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanItem {
    pub capability: String,
    pub name: String,
    pub kind: String,
    pub provider: String,
    pub revision: Option<i64>,
    pub pinned: bool,
    pub tree_hash: Option<String>,
    /// `prepared`, `from_arsenal` (copied to the session from the library), `rejected`,
    /// `unsupported` or `missing`.
    pub state: &'static str,
    pub reason: String,
    pub compatibility: Compatibility,
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanComposite {
    pub key: String,
    pub name: String,
    pub kind: &'static str,
    pub kept: bool,
    pub methodology: bool,
    pub enabled: Option<bool>,
    pub skills: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanConflict {
    /// `name_collision`, `duplicate_content`, `instruction_overlap`, `methodology_conflict` or
    /// `scope_shadow`.
    pub code: &'static str,
    pub names: Vec<String>,
    pub blocking: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TerminalPlan {
    pub selection: String,
    pub selection_name: String,
    pub provider: String,
    pub provider_version: Option<String>,
    pub project: Option<String>,
    /// `ready`, `partial` (some items cannot be carried; the rest can) or `blocked`.
    pub status: &'static str,
    pub can_apply: bool,
    pub enforcement: Enforcement,
    pub items: Vec<PlanItem>,
    /// Installed skills that stay exposed: the prepared ones and the members of kept composites.
    pub kept: Vec<String>,
    /// Installed skills the session would not expose.
    pub excluded: Vec<String>,
    /// Kept skills the provider's own configuration already switches off. Arsenal never switches a
    /// skill on, so these stay off in the session.
    pub inherited_off: Vec<String>,
    /// For a provider that switches skills off by file (Codex): the `SKILL.md` of each excluded skill.
    pub excluded_files: Vec<String>,
    /// What this configuration does not change, said so nothing is implied.
    pub untouched: Vec<String>,
    pub composites: Vec<PlanComposite>,
    pub conflicts: Vec<PlanConflict>,
    pub warnings: Vec<String>,
    /// The exact session settings file it would write, when the provider has one.
    pub overlay: Option<String>,
    pub config_hash: String,
    /// The launch environment the preset chose (Claude Code), when it changes the owner's own.
    pub environment: Option<crate::native::launch_home::HomeSpec>,
    /// The launch environment the preset chose (Codex), when it changes the owner's own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex_environment: Option<crate::native::launch_home_codex::CodexHomeSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_instructions: Option<super::role_instructions::Choice>,
    /// Stored skills the session gets from the library, copied into its own folder.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub materialize: Vec<crate::native::arsenal::Materialize>,
}

fn version_text(version: Option<&str>) -> &str {
    version.unwrap_or("an unidentified build")
}

pub fn plan(input: Input<'_>) -> TerminalPlan {
    let provider = input.provider;
    let installed = &input.installed.skills;
    let mut installed_hash: BTreeMap<&str, Vec<Option<String>>> = BTreeMap::new();
    for skill in installed {
        // A link's target is not read, so its content cannot be compared with a revision.
        let hash = (!skill.content.is_empty()).then(|| store::tree_digest(&skill.content));
        installed_hash
            .entry(skill.name.as_str())
            .or_default()
            .push(hash);
    }
    let kept_composites: BTreeSet<&str> = input
        .selection
        .composites
        .iter()
        .map(String::as_str)
        .collect();
    let mut kept: BTreeSet<String> = BTreeSet::new();
    let mut warnings = Vec::new();
    let mut items = Vec::with_capacity(input.items.len());
    let mut materialize = Vec::new();
    for item in &input.items {
        let dependencies = analysis::dependencies(&item.files, Some(&input.installed.mcp_servers));
        let paths: Vec<&str> = item.files.iter().map(|(path, _)| path.as_str()).collect();
        let compatibility =
            analysis::compatibility(&item.kind, &item.provider, provider, &paths, &dependencies);
        let (state, reason) = if item.state != "resolved" {
            (
                "missing",
                "The library no longer holds this capability or its pinned revision.".to_owned(),
            )
        } else if item.provider != provider {
            (
                "unsupported",
                format!(
                    "Built for {}; this terminal runs {}.",
                    analysis::provider_label(&item.provider),
                    analysis::provider_label(provider)
                ),
            )
        } else if item.kind != "skill" {
            (
                "unsupported",
                format!(
                    "A {} is not switched per session. The provider loads its installed ones as usual.",
                    item.kind
                ),
            )
        } else {
            let revision = item.revision.unwrap_or(0);
            let mut copy = |reason: String| {
                materialize.push(crate::native::arsenal::Materialize {
                    capability: item.capability.clone(),
                    revision,
                    name: item.name.clone(),
                });
                ("from_arsenal", reason)
            };
            match installed_hash.get(item.name.as_str()) {
                None => copy(format!(
                    "Not installed for this provider: revision {revision} is copied to this session from Arsenal."
                )),
                Some(copies) if copies.iter().any(|hash| hash.as_deref() == Some(item.tree_hash.as_str())) => {
                    kept.insert(item.name.clone());
                    (
                        "prepared",
                        format!(
                            "The installed copy is exactly revision {revision}. It stays exposed."
                        ),
                    )
                }
                Some(copies) if copies.iter().all(Option::is_none) => copy(format!(
                    "Installed as a link, so its content cannot be compared: this session gets revision {revision} from Arsenal. Your installed copy is not changed."
                )),
                Some(_) => copy(format!(
                    "The installed copy differs from revision {revision}: this session gets that revision from Arsenal. Your installed copy is not changed."
                )),
            }
        };
        items.push(PlanItem {
            capability: item.capability.clone(),
            name: item.name.clone(),
            kind: item.kind.clone(),
            provider: item.provider.clone(),
            revision: item.revision,
            pinned: item.pinned,
            tree_hash: (item.state == "resolved").then(|| item.tree_hash.clone()),
            state,
            reason,
            compatibility,
            dependencies,
        });
    }
    // Claude Code switches skills off by name: the copied skill's name must stay on. Codex
    // switches off by file, so the installed copy of another content is switched off and only
    // the copy in the session's own folder loads.
    if provider != "codex" {
        kept.extend(materialize.iter().map(|m: &crate::native::arsenal::Materialize| m.name.clone()));
    }
    let mut composites = Vec::new();
    for composite in &input.installed.composites {
        let keep = kept_composites.contains(composite.key.as_str());
        if keep && composite.kind == "gsd_core" {
            kept.extend(composite.skills.iter().cloned());
        }
        composites.push(plan_composite(composite, keep));
    }
    for key in &kept_composites {
        if !input.installed.composites.iter().any(|c| c.key == *key) {
            warnings.push(format!(
                "The selection keeps \"{key}\", which is not installed for this provider."
            ));
        }
    }
    // Skills the provider installs and replaces itself stay as they are.
    let switchable = |skill: &crate::agentconfig::catalogue::Candidate| {
        skill.source != SYSTEM_SKILLS && skill.blocked != Some("no_skill_file")
            // The launch home handles these independently of Arsenal library selection.
            && skill.source != "codex_home_skills"
    };
    let installed_names: BTreeSet<&str> = installed
        .iter()
        .filter(|skill| switchable(skill))
        .map(|s| s.name.as_str())
        .collect();
    let excluded: Vec<String> = installed_names
        .iter()
        .filter(|name| !kept.contains(**name))
        .map(|name| (*name).to_owned())
        .collect();
    let excluded_files: Vec<String> = if provider == "codex" {
        let mut files: Vec<String> = installed
            .iter()
            .zip(
                input
                    .installed
                    .skill_files
                    .iter()
                    .zip(&input.installed.nested_files),
            )
            .filter(|(skill, _)| switchable(skill) && !kept.contains(&skill.name))
            .flat_map(|(_, (file, nested))| file.iter().chain(nested))
            .map(|f| f.to_string_lossy().into_owned())
            .collect();
        files.sort();
        files.dedup();
        files
    } else {
        Vec::new()
    };
    let inherited_off: Vec<String> = kept
        .iter()
        .filter(|name| input.installed.base_off.contains(*name))
        .cloned()
        .collect();
    if !inherited_off.is_empty() {
        warnings.push(format!(
            "{} kept skill(s) are switched off in your own {} configuration and stay off: {}.",
            inherited_off.len(),
            analysis::provider_label(provider),
            inherited_off.join(", ")
        ));
    }
    let conflicts = conflicts(&items, &composites, &input.installed.skills);
    let mut untouched = vec![
        "Hooks, permissions and MCP servers: as your settings say.".to_owned(),
        "The provider's built-in skills.".to_owned(),
    ];
    if provider == "codex" {
        untouched.push(
            "Codex's own system skills (~/.codex/skills/.system) and the skills your config.toml already switches."
                .to_owned(),
        );
    }
    let enabled_plugins: Vec<&Composite> = input
        .installed
        .composites
        .iter()
        .filter(|c| {
            matches!(c.kind, "claude_plugin" | "codex_plugin")
                && c.enabled == Some(true)
                && !c.skills.is_empty()
        })
        .collect();
    for plugin in &enabled_plugins {
        untouched.push(format!(
            "Plugin {} keeps its {} skill(s): plugin skills are not switched off by skill overrides.",
            plugin.name,
            plugin.skills.len()
        ));
    }
    for item in &items {
        if let Some(dep) = item.dependencies.iter().find(|dep| dep.state == "missing") {
            warnings.push(format!(
                "{}: missing {} \"{}\".",
                item.name,
                dep.kind.replace('_', " "),
                dep.name
            ));
        }
    }
    let enforcement = enforcement(&input, provider, excluded.len());
    let blocked = items
        .iter()
        .any(|item| matches!(item.state, "rejected" | "missing"))
        || conflicts.iter().any(|conflict| conflict.blocking)
        || excluded.len() > MAX_SKILLS_OFF;
    let partial = items.iter().any(|item| item.state == "unsupported");
    let status = if blocked {
        "blocked"
    } else if partial {
        "partial"
    } else {
        "ready"
    };
    let overlay = match provider {
        "claude_code" => Some(overlay_text(&excluded)),
        "codex" => Some(crate::native::arsenal::codex_config_value(&excluded_files)),
        _ => None,
    };
    let config_hash = config_hash(&input, &items, &excluded, &excluded_files);
    TerminalPlan {
        selection: input.selection.id.clone(),
        selection_name: input.selection.name.clone(),
        provider: provider.to_owned(),
        provider_version: input.version.clone(),
        project: input.project.as_ref().map(|(name, _)| name.clone()),
        status,
        can_apply: status != "blocked" && enforcement.state == "available",
        enforcement,
        items,
        kept: kept.into_iter().collect(),
        excluded,
        inherited_off,
        excluded_files,
        untouched,
        composites,
        conflicts,
        warnings,
        overlay,
        config_hash,
        environment: None,
        codex_environment: None,
        launch_instructions: None,
        materialize,
    }
}

fn plan_composite(composite: &Composite, kept: bool) -> PlanComposite {
    PlanComposite {
        key: composite.key.clone(),
        name: composite.name.clone(),
        kind: composite.kind,
        kept: kept || (composite.kind == "claude_plugin" && composite.enabled == Some(true)),
        methodology: composite.methodology,
        enabled: composite.enabled,
        skills: composite.skills.len(),
    }
}

fn enforcement(input: &Input<'_>, provider: &str, excluded: usize) -> Enforcement {
    let version = input.version.as_deref();
    if !input.installed.unreadable.is_empty() || input.installed.truncated {
        return Enforcement {
            state: "incomplete_inventory",
            reason: "Some installed skills could not be read, so the exclusion would be incomplete. Nothing is applied.".into(),
            evidence: None,
        };
    }
    if excluded > MAX_SKILLS_OFF {
        return Enforcement {
            state: "incomplete_inventory",
            reason: format!("More than {MAX_SKILLS_OFF} installed skills would be switched off."),
            evidence: None,
        };
    }
    if matches!(provider, "claude_code" | "codex") {
        Enforcement {
            state: "available",
            reason: "Native checks the installed CLI before applying the selection.".into(),
            evidence: input.evidence,
        }
    } else {
        Enforcement {
            state: "unqualified",
            reason: format!("{} has no session skill switch.", version_text(version)),
            evidence: None,
        }
    }
}

fn conflicts(
    items: &[PlanItem],
    composites: &[PlanComposite],
    installed: &[crate::agentconfig::catalogue::Candidate],
) -> Vec<PlanConflict> {
    let mut found = Vec::new();
    let mut by_name: BTreeMap<&str, usize> = BTreeMap::new();
    let mut by_content: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut rules: Vec<&str> = Vec::new();
    for item in items.iter().filter(|item| item.state != "missing") {
        *by_name.entry(item.name.as_str()).or_insert(0) += 1;
        if let Some(tree) = &item.tree_hash {
            by_content
                .entry(tree.as_str())
                .or_default()
                .push(item.name.as_str());
        }
        if item.kind == "rule" {
            rules.push(item.name.as_str());
        }
    }
    for (name, count) in by_name {
        if count > 1 {
            found.push(PlanConflict {
                code: "name_collision",
                names: vec![name.to_owned()],
                blocking: true,
            });
        }
    }
    for names in by_content.into_values() {
        if names.len() > 1 {
            found.push(PlanConflict {
                code: "duplicate_content",
                names: names.into_iter().map(str::to_owned).collect(),
                blocking: false,
            });
        }
    }
    if rules.len() > 1 {
        found.push(PlanConflict {
            code: "instruction_overlap",
            names: rules.into_iter().map(str::to_owned).collect(),
            blocking: false,
        });
    }
    let methodologies: Vec<&PlanComposite> = composites
        .iter()
        .filter(|composite| composite.kept && composite.methodology)
        .collect();
    if methodologies.len() > 1 {
        found.push(PlanConflict {
            code: "methodology_conflict",
            names: methodologies.iter().map(|c| c.name.clone()).collect(),
            blocking: true,
        });
    }
    // One name installed in the account and in the project: the provider decides which loads.
    let mut scopes: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for skill in installed {
        scopes
            .entry(skill.name.as_str())
            .or_default()
            .insert(skill.scope);
    }
    for item in items.iter().filter(|item| item.state == "prepared") {
        if scopes
            .get(item.name.as_str())
            .is_some_and(|set| set.len() > 1)
        {
            found.push(PlanConflict {
                code: "scope_shadow",
                names: vec![item.name.clone()],
                blocking: false,
            });
        }
    }
    found
}

/// The session settings an exclusion writes: only `skillOverrides`, only `off`, only installed names.
pub fn overlay_text(excluded: &[String]) -> String {
    let skills: serde_json::Map<String, serde_json::Value> = excluded
        .iter()
        .map(|name| (name.clone(), serde_json::Value::String("off".into())))
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({ "skillOverrides": skills }))
        .unwrap_or_default()
}

fn config_hash(
    input: &Input<'_>,
    items: &[PlanItem],
    excluded: &[String],
    excluded_files: &[String],
) -> String {
    let material = serde_json::json!({
        "selection": input.selection.id,
        "updated": input.selection.updated_at,
        "provider": input.provider,
        "version": input.version,
        "project": input.project.as_ref().map(|(_, key)| key),
        "composites": input.selection.composites,
        "items": items.iter().map(|item| (
            &item.capability, item.revision, &item.tree_hash, item.state
        )).collect::<Vec<_>>(),
        "excluded": excluded,
        "excluded_files": excluded_files,
    });
    format!("{:x}", Sha256::digest(material.to_string().as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agentconfig::catalogue::{Candidate, ManagedBy};

    fn skill(name: &str, scope: &'static str, body: &[u8]) -> Candidate {
        Candidate {
            kind: "skill",
            provider: "claude_code",
            source: format!("claude_{scope}_skills"),
            scope,
            name: name.to_owned(),
            description: String::new(),
            origin: String::new(),
            files: Vec::new(),
            excluded: Vec::new(),
            review: Vec::new(),
            executes_code: false,
            blocked: None,
            managed_by: None,
            content: vec![("SKILL.md".to_owned(), body.to_vec())],
        }
    }

    fn linked(name: &str) -> Candidate {
        Candidate {
            content: Vec::new(),
            blocked: Some("symbolic_link"),
            managed_by: Some(ManagedBy {
                composite: "GSD Core",
                version: "1.15.0".into(),
                artifacts: 3,
            }),
            ..skill(name, "user", b"")
        }
    }

    fn gsd(skills: &[&str]) -> Composite {
        Composite {
            key: "gsd_core@1.15.0".into(),
            kind: "gsd_core",
            provider: "claude_code",
            name: "GSD Core 1.15.0".into(),
            version: "1.15.0".into(),
            description: String::new(),
            artifacts: 3,
            members: skills.len(),
            origin: String::new(),
            enabled: None,
            methodology: true,
            skills: skills.iter().map(|s| (*s).to_owned()).collect(),
            agents: Vec::new(),
            commands: 0,
            hooks: false,
            mcp: false,
            categories: Vec::new(),
            limitations: Vec::new(),
        }
    }

    fn plugin(name: &str, enabled: bool, methodology: bool) -> Composite {
        Composite {
            key: format!("claude_plugin:{name}@m"),
            kind: "claude_plugin",
            name: name.into(),
            enabled: Some(enabled),
            methodology,
            skills: vec!["p1".into()],
            ..gsd(&[])
        }
    }

    fn scan(skills: Vec<Candidate>, composites: Vec<Composite>) -> LaunchScan {
        LaunchScan {
            skills,
            composites,
            mcp_servers: ["context7".to_owned()].into(),
            truncated: false,
            unreadable: Vec::new(),
            skill_files: Vec::new(),
            nested_files: Vec::new(),
            base_off: Default::default(),
        }
    }

    fn selection(composites: &[&str]) -> Selection {
        Selection {
            id: "sel-1".into(),
            name: "Frontend".into(),
            provider: "claude_code".into(),
            items: Vec::new(),
            composites: composites.iter().map(|s| (*s).to_owned()).collect(),
            created_at: 1,
            updated_at: 2,
        }
    }

    fn item(name: &str, body: &[u8]) -> Resolved {
        let files = vec![("SKILL.md".to_owned(), body.to_vec())];
        Resolved {
            capability: format!("cap-{name}"),
            state: "resolved",
            name: name.into(),
            kind: "skill".into(),
            provider: "claude_code".into(),
            revision: Some(1),
            pinned: false,
            tree_hash: store::tree_digest(&files),
            files,
        }
    }

    fn input<'a>(
        selection: &'a Selection,
        items: Vec<Resolved>,
        installed: &'a LaunchScan,
        evidence: Option<ArsenalEvidence>,
    ) -> Input<'a> {
        Input {
            selection,
            items,
            provider: "claude_code",
            version: Some("2.1.314 (Claude Code)".into()),
            evidence,
            installed,
            project: Some(("app".into(), "key".into())),
        }
    }

    const CODEX_EVIDENCE: ArsenalEvidence = ArsenalEvidence {
        qualified_version: "codex-cli 0.159.0",
        checked_on: "test",
        operation: "test",
        source: "test",
        limitation: "test",
    };

    const EVIDENCE: ArsenalEvidence = ArsenalEvidence {
        qualified_version: "2.1.314 (Claude Code)",
        checked_on: "t",
        operation: "o",
        source: "s",
        limitation: "l",
    };

    #[test]
    fn a_kept_composite_keeps_its_members_and_a_qualified_build_can_apply() {
        let installed = scan(
            vec![skill("design", "user", b"one"), linked("gsd-help")],
            vec![gsd(&["gsd-help"])],
        );
        let sel = selection(&["gsd_core@1.15.0"]);
        let plan = plan(input(
            &sel,
            vec![item("design", b"one")],
            &installed,
            Some(EVIDENCE),
        ));
        assert_eq!(plan.kept, ["design", "gsd-help"]);
        assert!(plan.excluded.is_empty());
        assert_eq!(plan.enforcement.state, "available");
        assert!(plan.can_apply);
    }

    #[test]
    fn different_or_missing_content_is_copied_from_arsenal_and_foreign_items_are_unsupported() {
        let installed = scan(vec![skill("design", "user", b"changed")], Vec::new());
        let sel = selection(&[]);
        let mut codex = item("other", b"x");
        codex.provider = "codex".into();
        let mut agent = item("reviewer", b"y");
        agent.kind = "agent".into();
        let plan = plan(input(
            &sel,
            vec![item("design", b"one"), item("absent", b"z"), codex, agent],
            &installed,
            Some(EVIDENCE),
        ));
        let states: Vec<&str> = plan.items.iter().map(|i| i.state).collect();
        assert_eq!(
            states,
            ["from_arsenal", "from_arsenal", "unsupported", "unsupported"]
        );
        assert_eq!(plan.status, "partial");
        assert!(plan.can_apply);
        // Claude Code switches off by name: the copied skill's name stays on.
        assert!(plan.excluded.is_empty());
        let copies: Vec<(&str, i64)> = plan
            .materialize
            .iter()
            .map(|m| (m.name.as_str(), m.revision))
            .collect();
        assert_eq!(copies, [("design", 1), ("absent", 1)]);
    }

    #[test]
    fn a_skill_not_installed_is_prepared_from_arsenal_with_the_current_revision() {
        let installed = scan(Vec::new(), Vec::new());
        let sel = selection(&[]);
        let mut tutor = item("tutor", b"t");
        tutor.revision = Some(3);
        let plan = plan(input(&sel, vec![tutor], &installed, Some(EVIDENCE)));
        assert_eq!(plan.items[0].state, "from_arsenal");
        assert!(plan.items[0].reason.contains("revision 3 is copied"), "{}", plan.items[0].reason);
        assert_eq!(plan.status, "ready");
        assert!(plan.can_apply);
        assert_eq!(plan.materialize.len(), 1);
        assert_eq!(
            (plan.materialize[0].capability.as_str(), plan.materialize[0].revision, plan.materialize[0].name.as_str()),
            ("cap-tutor", 3, "tutor")
        );
    }

    #[test]
    fn an_installed_copy_that_differs_gives_way_to_the_preset_revision_and_codex_switches_it_off() {
        let mut installed = scan(
            vec![codex_skill("graphify", "codex_user_skills", b"installed")],
            Vec::new(),
        );
        installed.skill_files = vec![Some("/h/.agents/skills/graphify/SKILL.md".into())];
        installed.nested_files = vec![Vec::new()];
        let mut sel = selection(&[]);
        sel.provider = "codex".into();
        let mut graphify = item("graphify", b"stored revision 2");
        graphify.provider = "codex".into();
        graphify.revision = Some(2);
        graphify.pinned = true;
        let mut inp = input(&sel, vec![graphify], &installed, Some(CODEX_EVIDENCE));
        inp.provider = "codex";
        inp.version = Some("codex-cli 0.159.0".into());
        let plan = plan(inp);
        assert_eq!(plan.items[0].state, "from_arsenal");
        assert!(plan.items[0].reason.contains("differs from revision 2"), "{}", plan.items[0].reason);
        assert_eq!(plan.materialize[0].revision, 2);
        assert!(plan.can_apply);
        // The installed copy, outside the session's folder, does not load beside the copy.
        assert_eq!(plan.excluded_files, ["/h/.agents/skills/graphify/SKILL.md"]);
    }

    #[test]
    fn two_methodologies_conflict_and_an_unreadable_folder_never_applies() {
        let mut installed = scan(
            vec![linked("gsd-help")],
            vec![gsd(&["gsd-help"]), plugin("superpowers", true, true)],
        );
        let sel = selection(&["gsd_core@1.15.0"]);
        let plan_a = plan(input(&sel, Vec::new(), &installed, Some(EVIDENCE)));
        assert!(
            plan_a
                .conflicts
                .iter()
                .any(|c| c.code == "methodology_conflict" && c.blocking)
        );
        assert_eq!(plan_a.status, "blocked");
        assert!(
            plan_a
                .untouched
                .iter()
                .any(|line| line.contains("superpowers"))
        );
        installed.composites = vec![gsd(&["gsd-help"])];
        installed.unreadable = vec!["claude_project_skills"];
        let plan_b = plan(input(&sel, Vec::new(), &installed, Some(EVIDENCE)));
        assert_eq!(plan_b.enforcement.state, "incomplete_inventory");
        assert!(!plan_b.can_apply);
    }

    #[test]
    fn codex_account_skills_and_unknown_versions_leave_application_to_native() {
        let installed = scan(
            vec![codex_skill("account", "codex_home_skills", b"keep")],
            Vec::new(),
        );
        let mut sel = selection(&[]);
        sel.provider = "codex".into();
        let mut inp = input(&sel, Vec::new(), &installed, None);
        inp.provider = "codex";
        inp.version = Some("codex-cli 0.160.0".into());
        let plan = plan(inp);
        assert_eq!(plan.enforcement.state, "available");
        assert!(plan.enforcement.evidence.is_none());
        assert!(plan.can_apply);
        assert!(
            plan.excluded.is_empty(),
            "account skills are controlled in the launch home"
        );
    }

    fn codex_skill(name: &str, source: &str, body: &[u8]) -> Candidate {
        Candidate {
            provider: "codex",
            source: source.into(),
            ..skill(name, "user", body)
        }
    }

    #[test]
    fn codex_switches_off_by_file_keeps_system_skills_and_names_what_its_config_already_switched_off()
     {
        let mut installed = scan(
            vec![
                codex_skill("design", "codex_user_skills", b"one"),
                codex_skill("tokens", "codex_user_skills", b"two"),
                codex_skill("old", "codex_user_skills", b"three"),
                codex_skill("probe", "codex_project_skills", b"four"),
                codex_skill("imagegen", "codex_system_skills", b"sys"),
            ],
            Vec::new(),
        );
        installed.skill_files = vec![
            Some("/h/.agents/skills/design/SKILL.md".into()),
            Some("/h/.agents/skills/tokens/SKILL.md".into()),
            Some("/h/.agents/skills/old/SKILL.md".into()),
            Some("/p/.agents/skills/probe/SKILL.md".into()),
            Some("/h/.codex/skills/.system/imagegen/SKILL.md".into()),
        ];
        installed.nested_files = vec![
            Vec::new(),
            vec!["/h/.agents/skills/tokens/inner/SKILL.md".into()],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ];
        installed.base_off = ["old".to_owned()].into();
        let mut sel = selection(&[]);
        sel.provider = "codex".into();
        let mut design = item("design", b"one");
        design.provider = "codex".into();
        let mut old = item("old", b"three");
        old.provider = "codex".into();
        let mut inp = input(&sel, vec![design, old], &installed, Some(CODEX_EVIDENCE));
        inp.provider = "codex";
        inp.version = Some("codex-cli 0.159.0".into());
        let plan = plan(inp);
        assert!(plan.can_apply, "{:?}", plan.enforcement);
        assert_eq!(plan.excluded, ["probe", "tokens"]);
        assert_eq!(
            plan.excluded_files,
            [
                "/h/.agents/skills/tokens/SKILL.md",
                "/h/.agents/skills/tokens/inner/SKILL.md",
                "/p/.agents/skills/probe/SKILL.md"
            ]
        );
        assert_eq!(plan.inherited_off, ["old"]);
        assert!(plan.warnings.iter().any(|w| w.contains("stay off: old")));
        let value = plan.overlay.unwrap();
        let parsed: toml::Table =
            toml::from_str(&value.replacen("skills.config=", "x=", 1)).unwrap();
        let entries = parsed["x"].as_array().unwrap();
        assert_eq!(entries.len(), 3);
        assert!(
            entries
                .iter()
                .all(|e| e["enabled"].as_bool() == Some(false))
        );
        assert!(!value.contains(".system"));
    }

    #[test]
    fn a_claude_skill_its_settings_switch_off_is_kept_but_reported_off() {
        let mut installed = scan(vec![skill("design", "user", b"one")], Vec::new());
        installed.base_off = ["design".to_owned()].into();
        let sel = selection(&[]);
        let plan = plan(input(
            &sel,
            vec![item("design", b"one")],
            &installed,
            Some(EVIDENCE),
        ));
        assert_eq!(plan.kept, ["design"]);
        assert_eq!(plan.inherited_off, ["design"]);
        assert!(plan.excluded_files.is_empty());
    }

}
