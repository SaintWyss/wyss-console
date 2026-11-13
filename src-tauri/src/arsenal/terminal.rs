//! The configuration one terminal would start with: what a saved selection becomes for one provider
//! build, in one project, against the skills installed right now. It is computed, never assumed:
//! a selected capability is `prepared` only when the provider already has exactly that revision
//! installed, every other installed skill is named for exclusion, and the exclusion is applied only
//! where the exact build has its own evidence. Nothing here writes a file or starts a process.

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
    /// `prepared`, `rejected`, `unsupported` or `missing`.
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
            match installed_hash.get(item.name.as_str()) {
                None => (
                    "rejected",
                    "Not installed for this provider. Adding stored content to one session needs a qualified session plugin, which is not available.".to_owned(),
                ),
                Some(copies) if copies.iter().any(|hash| hash.as_deref() == Some(item.tree_hash.as_str())) => {
                    kept.insert(item.name.clone());
                    (
                        "prepared",
                        format!(
                            "The installed copy is exactly revision {}. It stays exposed.",
                            item.revision.unwrap_or(0)
                        ),
                    )
                }
                Some(copies) if copies.iter().all(Option::is_none) => (
                    "rejected",
                    "Installed as a link, so its content cannot be compared with this revision.".to_owned(),
                ),
                Some(_) => (
                    "rejected",
                    format!(
                        "The installed copy differs from revision {}. Restore or import so they match; Arsenal does not expose a revision that is not installed.",
                        item.revision.unwrap_or(0)
                    ),
                ),
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
    if version.is_none() {
        return Enforcement {
            state: "version_unknown",
            reason: "The installed provider build could not be identified. Nothing is applied."
                .into(),
            evidence: None,
        };
    }
    match (provider, input.evidence) {
        (_, Some(evidence)) => Enforcement {
            state: "available",
            reason: format!(
                "Skill exclusion is qualified for {}. Start applies it to this terminal only.",
                evidence.qualified_version
            ),
            evidence: Some(evidence),
        },
        ("codex", None) => Enforcement {
            state: "unqualified",
            reason: format!(
                "{} has no qualified per-session skill switch: the session layer of [[skills.config]] and the shared app-server daemon are not proven. The selection can be recorded, not applied.",
                version_text(version)
            ),
            evidence: None,
        },
        _ => Enforcement {
            state: "unqualified",
            reason: format!(
                "Skill exclusion is not qualified for Claude Code {}. The selection can be recorded, not applied.",
                version_text(version)
            ),
            evidence: None,
        },
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

/// The allow-list check run before anything reaches a provider (A0 isolation report: invalid
/// values drop the whole overlay, and misspelled or case-variant names are silent no-ops). Every
/// name must be one this provider has installed, exactly, once.
pub fn validate_exclusion(excluded: &[String], installed: &LaunchScan) -> Result<(), &'static str> {
    let names: BTreeSet<&str> = installed.skills.iter().map(|s| s.name.as_str()).collect();
    let mut seen = BTreeSet::new();
    if excluded.len() > MAX_SKILLS_OFF {
        return Err("too_many");
    }
    for name in excluded {
        if !names.contains(name.as_str()) || !seen.insert(name.as_str()) {
            return Err("unknown_or_repeated_name");
        }
    }
    let text = overlay_text(excluded);
    let parsed: serde_json::Value = serde_json::from_str(&text).map_err(|_| "malformed")?;
    let object = parsed.as_object().ok_or("malformed")?;
    if object.len() != 1 || !object.contains_key("skillOverrides") {
        return Err("unexpected_key");
    }
    let overrides = object["skillOverrides"].as_object().ok_or("malformed")?;
    if overrides.len() != excluded.len() || overrides.values().any(|v| v != "off") {
        return Err("unexpected_value");
    }
    Ok(())
}

/// The same check for exclusion by file: every path is the `SKILL.md` of an installed skill this
/// provider may switch, read through its folder, exists as a file now, and appears once. Codex
/// ignores a path it does not know without saying so, so an unknown path is refused here instead.
pub fn validate_files(files: &[String], installed: &LaunchScan) -> Result<(), &'static str> {
    let known: BTreeSet<String> = installed
        .skills
        .iter()
        .zip(installed.skill_files.iter().zip(&installed.nested_files))
        .filter(|(skill, _)| skill.source != SYSTEM_SKILLS)
        .flat_map(|(_, (file, nested))| file.iter().chain(nested))
        .map(|f| f.to_string_lossy().into_owned())
        .collect();
    let mut seen = BTreeSet::new();
    if files.len() > MAX_SKILLS_OFF {
        return Err("too_many");
    }
    for file in files {
        if !known.contains(file) || !seen.insert(file.as_str()) {
            return Err("unknown_or_repeated_file");
        }
        if !std::path::Path::new(file).is_file() {
            return Err("missing_file");
        }
    }
    Ok(())
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
    fn a_matching_installed_skill_is_prepared_and_every_other_installed_skill_is_excluded() {
        let installed = scan(
            vec![
                skill("design", "user", b"one"),
                skill("tokens", "user", b"two"),
                skill("local", "project", b"three"),
                linked("gsd-help"),
            ],
            vec![gsd(&["gsd-help"])],
        );
        let sel = selection(&[]);
        let plan = plan(input(&sel, vec![item("design", b"one")], &installed, None));
        assert_eq!(plan.items[0].state, "prepared");
        assert_eq!(plan.kept, ["design"]);
        assert_eq!(plan.excluded, ["gsd-help", "local", "tokens"]);
        assert_eq!(plan.status, "ready");
        // Not qualified: the plan is complete, and it still cannot be applied.
        assert_eq!(plan.enforcement.state, "unqualified");
        assert!(!plan.can_apply);
        assert!(validate_exclusion(&plan.excluded, &installed).is_ok());
        let overlay: serde_json::Value =
            serde_json::from_str(plan.overlay.as_deref().unwrap()).unwrap();
        assert_eq!(overlay["skillOverrides"]["tokens"], "off");
        assert_eq!(overlay.as_object().unwrap().len(), 1);
    }

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
    fn different_missing_or_foreign_content_is_rejected_or_unsupported_never_prepared() {
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
            ["rejected", "rejected", "unsupported", "unsupported"]
        );
        assert_eq!(plan.status, "blocked");
        assert!(!plan.can_apply);
        // A rejected skill is not kept: the installed copy would be other content.
        assert_eq!(plan.excluded, ["design"]);
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
    fn the_hash_follows_the_configuration_and_the_validator_refuses_unknown_names() {
        let installed = scan(vec![skill("design", "user", b"one")], Vec::new());
        let sel = selection(&[]);
        let first = plan(input(&sel, Vec::new(), &installed, None));
        let again = plan(input(&sel, Vec::new(), &installed, None));
        assert_eq!(first.config_hash, again.config_hash);
        let mut other = input(&sel, Vec::new(), &installed, None);
        other.version = Some("2.1.315 (Claude Code)".into());
        assert_ne!(plan(other).config_hash, first.config_hash);
        assert!(validate_exclusion(&["Design".into()], &installed).is_err());
        assert!(validate_exclusion(&["design".into(), "design".into()], &installed).is_err());
        assert!(validate_exclusion(&["desing".into()], &installed).is_err());
    }

    #[test]
    fn codex_without_its_own_record_is_not_applied_and_says_why() {
        let installed = scan(Vec::new(), Vec::new());
        let mut sel = selection(&[]);
        sel.provider = "codex".into();
        let mut inp = input(&sel, Vec::new(), &installed, None);
        inp.provider = "codex";
        inp.version = Some("codex-cli 0.160.0".into());
        let plan = plan(inp);
        assert_eq!(plan.enforcement.state, "unqualified");
        assert!(plan.enforcement.reason.contains("skills.config"));
        assert!(!plan.can_apply);
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

    #[test]
    fn a_file_outside_the_installed_ones_or_gone_from_disk_is_refused() {
        let dir = std::env::temp_dir().join(format!("arsenal-files-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::write(dir.join("a/SKILL.md"), "x").unwrap();
        let real = dir.join("a/SKILL.md").to_string_lossy().into_owned();
        let gone = dir.join("b/SKILL.md").to_string_lossy().into_owned();
        let mut installed = scan(
            vec![
                codex_skill("a", "codex_user_skills", b"x"),
                codex_skill("b", "codex_user_skills", b"y"),
            ],
            Vec::new(),
        );
        installed.skill_files = vec![Some(real.clone().into()), Some(gone.clone().into())];
        installed.nested_files = vec![Vec::new(), Vec::new()];
        assert!(validate_files(std::slice::from_ref(&real), &installed).is_ok());
        assert_eq!(validate_files(&[gone], &installed), Err("missing_file"));
        assert!(validate_files(&["/etc/SKILL.md".into()], &installed).is_err());
        assert!(validate_files(&[real.clone(), real], &installed).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
