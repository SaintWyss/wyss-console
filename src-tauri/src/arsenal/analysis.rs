//! What can be said about a stored capability without running it: what it declares or points at,
//! and how its format relates to a provider. Every rule is a fixed reading of the files; nothing
//! here guesses, calls a model or runs a script, and an unknown stays unknown.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::agentconfig::catalogue;

/// Most dependencies listed for one capability.
const MAX_DEPENDENCIES: usize = 64;
/// Top-level folders a skill keeps its own resources in. A path into one of them, or into a folder
/// the skill holds, is a reference to the skill's own tree.
const RESOURCE_FOLDERS: [&str; 6] = [
    "scripts",
    "references",
    "assets",
    "templates",
    "examples",
    "resources",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Dependency {
    /// `tool` (a provider tool the front matter declares), `mcp_server` (an MCP server its text
    /// calls), or `file` (a file of its own tree its text points at).
    pub kind: &'static str,
    pub name: String,
    /// `declared` (named, not checkable offline), `present`, `missing`, or `unknown`.
    pub state: &'static str,
}

/// The dependencies of one revision, read from its files. `mcp_servers` are the MCP servers the
/// provider has configured, when known; without them an MCP dependency is `unknown`.
pub fn dependencies(
    files: &[(String, Vec<u8>)],
    mcp_servers: Option<&BTreeSet<String>>,
) -> Vec<Dependency> {
    let paths: BTreeSet<&str> = files.iter().map(|(path, _)| path.as_str()).collect();
    let folders: BTreeSet<&str> = paths
        .iter()
        .filter_map(|path| path.split_once('/').map(|(first, _)| first))
        .collect();
    let mut tools = BTreeSet::new();
    let mut servers = BTreeSet::new();
    let mut references = BTreeSet::new();
    for (path, bytes) in files {
        let Ok(text) = std::str::from_utf8(bytes) else {
            continue;
        };
        if path.ends_with(".md") {
            if let Some(front) = front_matter(text) {
                tools.extend(declared_tools(front));
            }
            references.extend(file_references(text, &folders));
        }
        servers.extend(mcp_references(text));
    }
    let mut found = Vec::new();
    for tool in tools {
        found.push(Dependency {
            kind: "tool",
            name: tool,
            state: "declared",
        });
    }
    for server in servers {
        let state = match mcp_servers {
            Some(known) if known.contains(&server) => "present",
            Some(_) => "missing",
            None => "unknown",
        };
        found.push(Dependency {
            kind: "mcp_server",
            name: server,
            state,
        });
    }
    for reference in references {
        let state = if paths.contains(reference.as_str()) {
            "present"
        } else {
            "missing"
        };
        found.push(Dependency {
            kind: "file",
            name: reference,
            state,
        });
    }
    found.truncate(MAX_DEPENDENCIES);
    found
}

/// The YAML front matter of a Markdown file: the text between a first `---` line and the next.
fn front_matter(text: &str) -> Option<&str> {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

/// `allowed-tools` (skills) or `tools` (agents): a comma list or a YAML list of names.
fn declared_tools(front: &str) -> Vec<String> {
    let mut tools = Vec::new();
    let mut in_list = false;
    for line in front.lines() {
        let trimmed = line.trim();
        if in_list {
            if let Some(item) = trimmed.strip_prefix("- ") {
                push_tool(&mut tools, item);
                continue;
            }
            in_list = false;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        if !matches!(key.trim(), "allowed-tools" | "tools") {
            continue;
        }
        let value = value.trim().trim_start_matches('[').trim_end_matches(']');
        if value.is_empty() {
            in_list = true;
            continue;
        }
        for item in value.split(',') {
            push_tool(&mut tools, item);
        }
    }
    tools
}

fn push_tool(tools: &mut Vec<String>, raw: &str) {
    let name = raw.trim().trim_matches(|c| c == '"' || c == '\'');
    // A tool may carry a pattern, `Bash(git:*)`; the tool is the word before it.
    let name = name.split('(').next().unwrap_or("").trim();
    if !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        && !tools.iter().any(|known| known == name)
    {
        tools.push(name.to_owned());
    }
}

/// MCP servers named through their tools: `mcp__<server>__<tool>`.
fn mcp_references(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = text;
    while let Some(start) = rest.find("mcp__") {
        rest = &rest[start + 5..];
        let server: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if let Some(name) = server.split("__").next()
            && server.contains("__")
            && !name.is_empty()
            && name.len() <= 64
        {
            found.insert(name.to_owned());
        }
    }
    found
}

/// Relative paths the text points at that would be inside the capability: Markdown link targets
/// and code spans whose first folder is a resource folder or one the skill holds.
fn file_references(text: &str, folders: &BTreeSet<&str>) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut consider = |raw: &str| {
        let candidate = raw.trim().trim_start_matches("./");
        let Some((first, _)) = candidate.split_once('/') else {
            return;
        };
        let own_folder = RESOURCE_FOLDERS.contains(&first) || folders.contains(first);
        let plain = candidate.len() <= 256
            && !candidate.contains("..")
            && !candidate.contains("//")
            && candidate
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'-'))
            && candidate
                .rsplit('/')
                .next()
                .is_some_and(|last| last.contains('.') && !last.ends_with('.'));
        if own_folder && plain {
            found.insert(candidate.to_owned());
        }
    };
    let mut rest = text;
    while let Some(start) = rest.find("](") {
        rest = &rest[start + 2..];
        if let Some(end) = rest.find(')') {
            let target = rest[..end].split('#').next().unwrap_or("");
            if !target.contains(':') {
                consider(target);
            }
        }
    }
    for (index, span) in text.split('`').enumerate() {
        if index % 2 == 1 && !span.contains(' ') {
            consider(span);
        }
    }
    found
}

/// How a stored capability's format relates to the provider a selection is for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Compatibility {
    /// `supported`, `unsupported`, `unverified`, `requires_qualification`, `conflicting` or
    /// `missing_dependency`.
    pub status: &'static str,
    pub reason: String,
}

pub fn provider_label(provider: &str) -> &'static str {
    match provider {
        "claude_code" => "Claude Code",
        "codex" => "Codex",
        _ => "another provider",
    }
}

/// The format verdict for a capability of `kind`, imported from `owner`'s folders, used for `target`.
/// `supported` means the provider reads this format from its own folders, where it was found; it
/// does not mean a launched session used this revision. That is the terminal plan's question.
pub fn compatibility(
    kind: &str,
    owner: &str,
    target: &str,
    paths: &[&str],
    dependencies: &[Dependency],
) -> Compatibility {
    let owner_label = provider_label(owner);
    let target_label = provider_label(target);
    if !matches!(kind, "skill" | "agent" | "rule") {
        return Compatibility {
            status: "unsupported",
            reason: "Arsenal does not hold this kind.".into(),
        };
    }
    if owner != target {
        return match kind {
            // Both providers read SKILL.md folders, but this copy was never exposed to the other one.
            "skill" => Compatibility {
                status: "requires_qualification",
                reason: format!(
                    "A {owner_label} skill. {target_label} reads the same SKILL.md format, but using it there needs a qualified way to add it to a session."
                ),
            },
            _ => Compatibility {
                status: "unsupported",
                reason: format!(
                    "A {owner_label} {kind} file. {target_label} uses a different format for this."
                ),
            },
        };
    }
    if let Some(missing) = dependencies.iter().find(|dep| dep.state == "missing") {
        return Compatibility {
            status: "missing_dependency",
            reason: format!(
                "It refers to {} \"{}\", which is not there.",
                match missing.kind {
                    "mcp_server" => "the MCP server",
                    "file" => "the file",
                    _ => "the tool",
                },
                missing.name
            ),
        };
    }
    if paths.iter().any(|path| catalogue::is_script(path)) {
        return Compatibility {
            status: "unverified",
            reason: format!(
                "{target_label} reads this {kind} format, but it holds scripts whose own requirements are not checked. Wyss never runs them."
            ),
        };
    }
    Compatibility {
        status: "supported",
        reason: format!("{target_label}'s own {kind} format, read from its own folder."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, text: &str) -> (String, Vec<u8>) {
        (path.to_owned(), text.as_bytes().to_vec())
    }

    #[test]
    fn declared_tools_mcp_servers_and_own_files_are_read_from_the_text() {
        let files = vec![
            file(
                "SKILL.md",
                "---\nname: x\nallowed-tools: Read, Bash(git:*), \"Grep\"\n---\nRun `scripts/run.sh` then read [the guide](references/guide.md#top). \
                 Use mcp__context7__query-docs and mcp__notebooklm__ask. See https://e.com/a/b.md and `src/app.ts`.",
            ),
            file("scripts/run.sh", "echo"),
        ];
        let known: BTreeSet<String> = ["context7".to_owned()].into();
        let deps = dependencies(&files, Some(&known));
        let pick = |kind: &str, name: &str| {
            deps.iter()
                .find(|d| d.kind == kind && d.name == name)
                .map(|d| d.state)
        };
        assert_eq!(pick("tool", "Read"), Some("declared"));
        assert_eq!(pick("tool", "Bash"), Some("declared"));
        assert_eq!(pick("tool", "Grep"), Some("declared"));
        assert_eq!(pick("mcp_server", "context7"), Some("present"));
        assert_eq!(pick("mcp_server", "notebooklm"), Some("missing"));
        assert_eq!(pick("file", "scripts/run.sh"), Some("present"));
        assert_eq!(pick("file", "references/guide.md"), Some("missing"));
        // A URL and a path into the user's project are not the skill's own files.
        assert!(deps.iter().all(|d| d.name != "src/app.ts"));
        assert!(deps.iter().all(|d| !d.name.contains("e.com")));
        // Without the provider's MCP list the server is unknown, never "missing".
        let unknown = dependencies(&files, None);
        assert!(
            unknown
                .iter()
                .any(|d| d.name == "notebooklm" && d.state == "unknown")
        );
    }

    #[test]
    fn yaml_list_tools_and_traversal_are_handled() {
        let files = vec![file(
            "agent.md",
            "---\ntools:\n  - Read\n  - Write\nmodel: x\n---\n[bad](scripts/../../etc/passwd.txt)",
        )];
        let deps = dependencies(&files, None);
        let names: Vec<&str> = deps.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Read", "Write"]);
    }

    #[test]
    fn compatibility_states_what_is_known_and_never_more() {
        let none: Vec<Dependency> = Vec::new();
        assert_eq!(
            compatibility("skill", "claude_code", "claude_code", &["SKILL.md"], &none).status,
            "supported"
        );
        assert_eq!(
            compatibility("skill", "claude_code", "codex", &["SKILL.md"], &none).status,
            "requires_qualification"
        );
        assert_eq!(
            compatibility("agent", "claude_code", "codex", &["a.md"], &none).status,
            "unsupported"
        );
        assert_eq!(
            compatibility(
                "skill",
                "codex",
                "codex",
                &["SKILL.md", "scripts/x.py"],
                &none
            )
            .status,
            "unverified"
        );
        let missing = vec![Dependency {
            kind: "file",
            name: "references/a.md".into(),
            state: "missing",
        }];
        let verdict = compatibility("skill", "codex", "codex", &["SKILL.md"], &missing);
        assert_eq!(verdict.status, "missing_dependency");
        assert!(verdict.reason.contains("references/a.md"));
        assert_eq!(
            compatibility("hook", "codex", "codex", &[], &none).status,
            "unsupported"
        );
    }
}
