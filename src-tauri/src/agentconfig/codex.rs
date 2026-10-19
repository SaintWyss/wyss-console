//! The Codex adapter: hooks, skills, MCP servers, model preferences and instructions
//! files from the places 06 §3.2 lists.
//!
//! Codex's effective state is static only here (DN-10): what the files declare, never a
//! running Codex, so every Codex item is `not_verifiable` except instructions files,
//! which are `documented`. Where Codex stores the per-hook switch is not documented
//! (OQ-01); `[hooks.state]` is therefore skipped, not interpreted. A hook's `persisted`
//! state is the engine switch `features.hooks`, with the administrator's pin first.
use super::inventory::{
    Document, Environment, Format, Place, Sink, SourceKey, detail, hook_items, rule_files,
};
use super::model::{Bounds, Effective, ItemKind, Persisted};
use super::reader::ConfigReader;
use super::roots::{ApprovedRoots, InvalidRoot};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

pub(super) fn roots(env: &Environment) -> Result<ApprovedRoots, InvalidRoot> {
    let mut roots = vec![env.codex_dir(), env.system.join("codex")];
    if let Some(project) = &env.project {
        roots.push(project.join(".codex"));
        roots.push(project.join("AGENTS.md"));
    }
    ApprovedRoots::new(roots)
        .map(|roots| roots.with_declared_writers(super::declared::uids(&env.declared_writers)))
}

pub(super) fn read(reader: &ConfigReader, env: &Environment, sink: &mut Sink) {
    let bounds = *sink.bounds();
    let dir = env.codex_dir();
    let system = env.system.join("codex");

    let requirements = sink.document(
        reader,
        &system.join("requirements.toml"),
        SourceKey::CodexSystemRequirements,
        Format::Toml,
    );
    let managed = sink.document(
        reader,
        &system.join("managed_config.toml"),
        SourceKey::CodexSystemManaged,
        Format::Toml,
    );
    let user = sink.document(
        reader,
        &dir.join("config.toml"),
        SourceKey::CodexUserConfig,
        Format::Toml,
    );
    let hooks_file = sink.document(
        reader,
        &dir.join("hooks.json"),
        SourceKey::CodexUserHooks,
        Format::Json,
    );

    // The project's own configuration is loaded by Codex only for a project the user
    // config trusts; it is not even read otherwise.
    let project = env.project.as_ref().and_then(|project| {
        let trusted = user
            .as_ref()
            .is_some_and(|user| project_trusted(&user.value, project));
        if trusted {
            sink.document(
                reader,
                &project.join(".codex/config.toml"),
                SourceKey::CodexProjectConfig,
                Format::Toml,
            )
        } else {
            sink.not_trusted(SourceKey::CodexProjectConfig);
            None
        }
    });

    // The engine switch: the administrator's pin, then the managed file, then the user's.
    let engine = [&requirements, &managed, &user]
        .into_iter()
        .flatten()
        .find_map(|document| {
            document
                .value
                .pointer("/features/hooks")
                .and_then(Value::as_bool)
        });
    let hooks = if engine == Some(false) {
        Persisted::PresentDisabled
    } else {
        Persisted::PresentEnabled
    };

    for document in [&requirements, &managed, &user, &project]
        .into_iter()
        .flatten()
    {
        let unrecognised = from_toml(document, hooks, &bounds, sink);
        sink.read(document, unrecognised);
    }
    if let Some(document) = &hooks_file {
        let place = Place::of(document);
        let unrecognised = match document.value.get("hooks") {
            Some(map) => hook_items(place, map, hooks, Effective::NotVerifiable, &bounds, sink),
            // A bare map of events, with nothing else in the file.
            None if document
                .value
                .as_object()
                .is_some_and(|map| !map.is_empty()) =>
            {
                hook_items(
                    place,
                    &document.value,
                    hooks,
                    Effective::NotVerifiable,
                    &bounds,
                    sink,
                )
            }
            None => 0,
        };
        sink.read(document, unrecognised);
    }

    rule_files(
        sink,
        reader,
        SourceKey::CodexUserRules,
        &[
            ("AGENTS.md".to_owned(), dir.join("AGENTS.md")),
            (
                "AGENTS.override.md".to_owned(),
                dir.join("AGENTS.override.md"),
            ),
        ],
    );
    if let Some(project) = &env.project {
        rule_files(
            sink,
            reader,
            SourceKey::CodexProjectRules,
            &[("AGENTS.md".to_owned(), project.join("AGENTS.md"))],
        );
    }
}

/// Whether the user's configuration marks `project` as trusted.
fn project_trusted(user: &Value, project: &Path) -> bool {
    user.get("projects")
        .and_then(|projects| projects.get(project.to_string_lossy().as_ref()))
        .and_then(|entry| entry.get("trust_level"))
        .and_then(Value::as_str)
        == Some("trusted")
}

/// The items of one Codex TOML file. Returns how many entries it could not place.
fn from_toml(document: &Document, hooks: Persisted, bounds: &Bounds, sink: &mut Sink) -> u32 {
    let (place, value) = (Place::of(document), &document.value);
    let mut unrecognised = 0;

    if let Some(map) = value.get("hooks") {
        unrecognised += hook_items(place, map, hooks, Effective::NotVerifiable, bounds, sink);
    }

    match value.pointer("/skills/config") {
        Some(Value::Array(entries)) => {
            for entry in entries {
                let Some(path) = entry.get("path").and_then(Value::as_str) else {
                    unrecognised += 1;
                    continue;
                };
                let persisted = if entry.get("enabled").and_then(Value::as_bool) == Some(false) {
                    Persisted::PresentDisabled
                } else {
                    Persisted::PresentEnabled
                };
                sink.push(place.item(
                    ItemKind::Skill,
                    &[path],
                    &skill_name(path),
                    persisted,
                    Effective::NotVerifiable,
                    detail(entry, bounds),
                ));
            }
        }
        Some(_) => unrecognised += 1,
        None => (),
    }

    if let Some(servers) = value.get("mcp_servers") {
        let switched_off: BTreeSet<&str> = servers
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(_, server)| server.get("enabled").and_then(Value::as_bool) == Some(false))
            .map(|(name, _)| name.as_str())
            .collect();
        unrecognised += super::claude::servers_items(place, servers, &switched_off, bounds, sink);
    }

    for key in ["model", "model_reasoning_effort"] {
        if let Some(setting) = value.get(key) {
            sink.push(place.item(
                ItemKind::ModelPreference,
                &[key],
                key,
                Persisted::PresentEnabled,
                Effective::NotVerifiable,
                detail(setting, bounds),
            ));
        }
    }
    unrecognised
}

/// A skill's name from its configured path: the directory, or the one holding `SKILL.md`.
fn skill_name(path: &str) -> String {
    let path = Path::new(path);
    let named = if path.file_name().is_some_and(|name| name == "SKILL.md") {
        path.parent()
    } else {
        Some(path)
    };
    named.and_then(Path::file_name).map_or_else(
        || "skill".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}
