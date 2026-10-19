//! The OpenCode adapter: plugins, MCP servers, model preferences, which providers are
//! configured or switched off, and instructions files (F4a, read only).
//!
//! OpenCode reads `opencode.jsonc` (JSON with comments and trailing commas). Its provider
//! entries carry API keys in `provider.<id>.options`: a provider is shown by its name and
//! never by what is inside it, so no key can reach an item even if redaction missed one.
//! OpenCode's effective state is static here, as Codex's is: every item is `not_verifiable`.
use super::inventory::{Document, Environment, Format, Place, Sink, SourceKey, detail, rule_files};
use super::model::{Bounds, Effective, ItemKind, Persisted};
use super::reader::ConfigReader;
use super::roots::{ApprovedRoots, InvalidRoot};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub(super) fn roots(env: &Environment) -> Result<ApprovedRoots, InvalidRoot> {
    let mut roots = vec![env.opencode_dir()];
    if let Some(project) = &env.project {
        roots.push(project.join("opencode.jsonc"));
        roots.push(project.join("opencode.json"));
        roots.push(project.join("AGENTS.md"));
    }
    ApprovedRoots::new(roots)
        .map(|roots| roots.with_declared_writers(super::declared::uids(&env.declared_writers)))
}

/// `opencode.jsonc` when there is one, else `opencode.json`.
pub(super) fn config_in(dir: &Path) -> PathBuf {
    let jsonc = dir.join("opencode.jsonc");
    if jsonc.symlink_metadata().is_ok() {
        jsonc
    } else {
        dir.join("opencode.json")
    }
}

pub(super) fn read(reader: &ConfigReader, env: &Environment, sink: &mut Sink) {
    let bounds = *sink.bounds();
    let dir = env.opencode_dir();
    let mut documents = Vec::new();
    documents.extend(sink.document(
        reader,
        &config_in(&dir),
        SourceKey::OpencodeUserConfig,
        Format::Jsonc,
    ));
    if let Some(project) = &env.project {
        documents.extend(sink.document(
            reader,
            &config_in(project),
            SourceKey::OpencodeProjectConfig,
            Format::Jsonc,
        ));
    }
    for document in &documents {
        let unrecognised = items(document, &bounds, sink);
        sink.read(document, unrecognised);
    }
    rule_files(
        sink,
        reader,
        SourceKey::OpencodeUserRules,
        &[("AGENTS.md".to_owned(), dir.join("AGENTS.md"))],
    );
    if let Some(project) = &env.project {
        rule_files(
            sink,
            reader,
            SourceKey::OpencodeProjectRules,
            &[("AGENTS.md".to_owned(), project.join("AGENTS.md"))],
        );
    }
}

/// The items of one OpenCode configuration. Returns how many entries it could not place.
fn items(document: &Document, bounds: &Bounds, sink: &mut Sink) -> u32 {
    let (place, value) = (Place::of(document), &document.value);
    let mut unrecognised = 0;

    match value.get("plugin") {
        Some(Value::Array(plugins)) => {
            for plugin in plugins {
                // A plugin is a name or a path, or a name with options as `[name, {..}]`.
                let spec = match plugin {
                    Value::String(spec) => Some(spec.as_str()),
                    Value::Array(pair) => pair.first().and_then(Value::as_str),
                    _ => None,
                };
                let Some(spec) = spec else {
                    unrecognised += 1;
                    continue;
                };
                sink.push(place.item(
                    ItemKind::Plugin,
                    &["plugin", spec],
                    &plugin_name(spec),
                    Persisted::PresentEnabled,
                    Effective::NotVerifiable,
                    detail(&Value::String(spec.to_owned()), bounds),
                ));
            }
        }
        Some(_) => unrecognised += 1,
        None => (),
    }

    if let Some(servers) = value.get("mcp") {
        let switched_off: BTreeSet<&str> = servers
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(_, server)| server.get("enabled").and_then(Value::as_bool) == Some(false))
            .map(|(name, _)| name.as_str())
            .collect();
        unrecognised += super::claude::servers_items(place, servers, &switched_off, bounds, sink);
    }

    for key in ["model", "small_model"] {
        if let Some(setting) = value.get(key) {
            sink.push(place.item(
                ItemKind::ModelPreference,
                &["model", key],
                key,
                Persisted::PresentEnabled,
                Effective::NotVerifiable,
                detail(setting, bounds),
            ));
        }
    }

    // A provider is its name and whether it is switched off, never what is inside it.
    let off: BTreeSet<&str> = value
        .get("disabled_providers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut named: BTreeSet<&str> = BTreeSet::new();
    match value.get("provider") {
        Some(Value::Object(providers)) => named.extend(providers.keys().map(String::as_str)),
        Some(_) => unrecognised += 1,
        None => (),
    }
    named.extend(off.iter().copied());
    for name in named {
        let persisted = if off.contains(name) {
            Persisted::PresentDisabled
        } else {
            Persisted::PresentEnabled
        };
        sink.push(place.item(
            ItemKind::ModelPreference,
            &["provider", name],
            &format!("provider {name}"),
            persisted,
            Effective::NotVerifiable,
            None,
        ));
    }
    unrecognised
}

/// A plugin's short name: the last path segment, without a `file://` scheme or extension.
fn plugin_name(spec: &str) -> String {
    let last = spec
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(spec);
    last.strip_suffix(".ts")
        .or_else(|| last.strip_suffix(".js"))
        .unwrap_or(last)
        .to_owned()
}
