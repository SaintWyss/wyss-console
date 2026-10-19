//! The Claude Code adapter: hooks, skills, rule files, MCP server names and model
//! preferences from the places 06 §3.1 lists, and nothing else.
//!
//! Provider-owned `~/.claude.json` is read through a projection of the MCP servers it
//! names and nothing more of it (07 §4.4). Nothing is written, and the effective state of
//! anything is not read from a running Claude Code (F1 excludes it): hooks and models are
//! `documented`, MCP servers and switched-off skills are `not_verifiable`.
use super::inventory::{
    Document, Environment, Format, Place, Sink, SourceKey, detail, hook_items, report_listing,
    rule_files, skill_directories,
};
use super::model::{Bounds, Effective, ItemKind, Persisted};
use super::reader::ConfigReader;
use super::roots::{ApprovedRoots, InvalidRoot};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub(super) fn roots(env: &Environment) -> Result<ApprovedRoots, InvalidRoot> {
    let mut roots = vec![env.claude_dir(), env.system.join("claude-code")];
    if env.claude_config_dir.is_none() {
        roots.push(state_file(env));
    }
    if let Some(project) = &env.project {
        roots.push(project.join(".claude"));
        roots.push(project.join(".mcp.json"));
        roots.push(project.join("CLAUDE.md"));
    }
    ApprovedRoots::new(roots)
        .map(|roots| roots.with_declared_writers(super::declared::uids(&env.declared_writers)))
}

/// `~/.claude.json`, or the file of the same name inside a relocated configuration
/// directory.
fn state_file(env: &Environment) -> PathBuf {
    match &env.claude_config_dir {
        Some(dir) => dir.join(".claude.json"),
        None => env.home.join(".claude.json"),
    }
}

pub(super) fn read(reader: &ConfigReader, env: &Environment, sink: &mut Sink) {
    let bounds = *sink.bounds();
    let dir = env.claude_dir();

    let mut places = vec![
        (
            SourceKey::ClaudeManagedSettings,
            env.system.join("claude-code").join("managed-settings.json"),
        ),
        (SourceKey::ClaudeUserSettings, dir.join("settings.json")),
    ];
    if let Some(project) = &env.project {
        places.push((
            SourceKey::ClaudeProjectSettings,
            project.join(".claude/settings.json"),
        ));
        places.push((
            SourceKey::ClaudeProjectLocalSettings,
            project.join(".claude/settings.local.json"),
        ));
    }
    let settings: Vec<Document> = places
        .into_iter()
        .filter_map(|(key, path)| sink.document(reader, &path, key, Format::Json))
        .collect();

    // What a setting says about all hooks, a skill or a server applies wherever it is
    // read, so it is gathered across the files first. Managed settings come first and a
    // later file overrides an earlier one, as in the provider's precedence.
    let disable_all = settings
        .iter()
        .any(|document| document.value.get("disableAllHooks") == Some(&Value::Bool(true)));
    let mut overrides: BTreeMap<&str, &str> = BTreeMap::new();
    let mut disabled_servers: BTreeSet<&str> = BTreeSet::new();
    for document in &settings {
        if let Some(map) = document
            .value
            .get("skillOverrides")
            .and_then(Value::as_object)
        {
            overrides.extend(
                map.iter()
                    .filter_map(|(k, v)| Some((k.as_str(), v.as_str()?))),
            );
        }
        if let Some(list) = document
            .value
            .get("disabledMcpjsonServers")
            .and_then(Value::as_array)
        {
            disabled_servers.extend(list.iter().filter_map(Value::as_str));
        }
    }

    for document in &settings {
        let place = Place::of(document);
        let mut unrecognised = 0;
        if let Some(hooks) = document.value.get("hooks") {
            let persisted = if disable_all {
                Persisted::PresentDisabled
            } else {
                Persisted::PresentEnabled
            };
            unrecognised += hook_items(
                place,
                hooks,
                persisted,
                Effective::Documented,
                &bounds,
                sink,
            );
        }
        for key in ["model", "effortLevel"] {
            if let Some(value) = document.value.get(key) {
                sink.push(place.item(
                    ItemKind::ModelPreference,
                    &[key],
                    key,
                    Persisted::PresentEnabled,
                    Effective::Documented,
                    detail(value, &bounds),
                ));
            }
        }
        sink.read(document, unrecognised);
    }

    skills(
        reader,
        sink,
        SourceKey::ClaudeUserSkills,
        &dir.join("skills"),
        &overrides,
    );
    if let Some(project) = &env.project {
        skills(
            reader,
            sink,
            SourceKey::ClaudeProjectSkills,
            &project.join(".claude/skills"),
            &overrides,
        );
    }

    mcp_servers(reader, env, sink, &bounds, &disabled_servers);
    rules(reader, env, sink);
}

fn skills(
    reader: &ConfigReader,
    sink: &mut Sink,
    key: SourceKey,
    path: &std::path::Path,
    overrides: &BTreeMap<&str, &str>,
) {
    let Some(listing) = sink.directory(reader, path, key) else {
        return;
    };
    let place = Place {
        key,
        trust: listing.trust,
    };
    for name in skill_directories(&listing) {
        // `skillOverrides: { name: "off" }` switches a skill off; what other values or
        // the key itself mean is open (OQ-15), so a skill it names is not verifiable.
        let (persisted, effective) = match overrides.get(name) {
            Some(&"off") => (Persisted::PresentDisabled, Effective::NotVerifiable),
            Some(_) => (Persisted::PresentEnabled, Effective::NotVerifiable),
            None => (Persisted::PresentEnabled, Effective::Documented),
        };
        sink.push(place.item(ItemKind::Skill, &[name], name, persisted, effective, None));
    }
    report_listing(sink, key, &listing, 0);
}

fn mcp_servers(
    reader: &ConfigReader,
    env: &Environment,
    sink: &mut Sink,
    bounds: &Bounds,
    disabled: &BTreeSet<&str>,
) {
    if let Some(document) = sink.document(
        reader,
        &state_file(env),
        SourceKey::ClaudeUserState,
        Format::Json,
    ) {
        let place = Place::of(&document);
        let mut unrecognised = 0;
        if let Some(servers) = document.value.get("mcpServers") {
            unrecognised += servers_items(place, servers, &BTreeSet::new(), bounds, sink);
        }
        // The servers of the selected project, kept by path in the same file.
        if let Some(project) = &env.project {
            let local = document
                .value
                .get("projects")
                .and_then(|projects| projects.get(project.to_string_lossy().as_ref()))
                .and_then(|entry| entry.get("mcpServers"));
            if let Some(servers) = local {
                unrecognised += servers_items(place, servers, &BTreeSet::new(), bounds, sink);
            }
        }
        sink.read(&document, unrecognised);
    }
    if let Some(project) = &env.project {
        let path = project.join(".mcp.json");
        if let Some(document) =
            sink.document(reader, &path, SourceKey::ClaudeProjectMcp, Format::Json)
        {
            let unrecognised = document.value.get("mcpServers").map_or(0, |servers| {
                servers_items(Place::of(&document), servers, disabled, bounds, sink)
            });
            sink.read(&document, unrecognised);
        }
    }
}

/// `{ name: { ...server... } }`: each name is an item; its detail is a redacted projection
/// of the server (the `env` map keeps its names and loses its values).
pub(super) fn servers_items(
    place: Place,
    servers: &Value,
    disabled: &BTreeSet<&str>,
    bounds: &Bounds,
    sink: &mut Sink,
) -> u32 {
    let Some(map) = servers.as_object() else {
        return 1;
    };
    let mut unrecognised = 0;
    for (name, server) in map {
        if !server.is_object() {
            unrecognised += 1;
            continue;
        }
        let persisted = if disabled.contains(name.as_str()) {
            Persisted::PresentDisabled
        } else {
            Persisted::PresentEnabled
        };
        sink.push(place.item(
            ItemKind::McpServer,
            &[name],
            name,
            persisted,
            Effective::NotVerifiable,
            detail(server, bounds),
        ));
    }
    unrecognised
}

fn rules(reader: &ConfigReader, env: &Environment, sink: &mut Sink) {
    let dir = env.claude_dir();
    let mut files = vec![("CLAUDE.md".to_owned(), dir.join("CLAUDE.md"))];
    if let Some(listing) =
        sink.directory_optional(reader, &dir.join("rules"), SourceKey::ClaudeUserRules)
    {
        files.extend(markdown_files(&listing, &dir.join("rules")));
    }
    rule_files(sink, reader, SourceKey::ClaudeUserRules, &files);

    if let Some(project) = &env.project {
        let mut files = vec![
            ("CLAUDE.md".to_owned(), project.join("CLAUDE.md")),
            (
                ".claude/CLAUDE.md".to_owned(),
                project.join(".claude/CLAUDE.md"),
            ),
        ];
        if let Some(listing) = sink.directory_optional(
            reader,
            &project.join(".claude/rules"),
            SourceKey::ClaudeProjectRules,
        ) {
            files.extend(markdown_files(&listing, &project.join(".claude/rules")));
        }
        rule_files(sink, reader, SourceKey::ClaudeProjectRules, &files);
    }
}

/// The Markdown files of a rules directory, named as `rules/<name>`.
fn markdown_files(
    listing: &super::reader::Listing,
    dir: &std::path::Path,
) -> Vec<(String, PathBuf)> {
    listing
        .entries
        .iter()
        .filter(|entry| entry.kind == super::reader::EntryKind::File)
        .filter_map(|entry| entry.name.to_str())
        .filter(|name| name.ends_with(".md"))
        .map(|name| (format!("rules/{name}"), dir.join(name)))
        .collect()
}
