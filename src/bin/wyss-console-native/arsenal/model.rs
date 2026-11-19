use wyss_console::{
    agentconfig::guide::GuideEntry,
    arsenal::{
        store::{Incoming, IncomingFile, Selection},
        sync::{Note, Status},
    },
    launch::{Kind, Launch},
};

use crate::t;

pub const TOOLS: [(&str, &str, &str, wyss_console::agentconfig::ProviderId); 3] = [
    (
        "claude_code",
        "Anthropic · Claude Code",
        "Claude Code",
        wyss_console::agentconfig::ProviderId::ClaudeCode,
    ),
    (
        "codex",
        "OpenAI · Codex",
        "Codex",
        wyss_console::agentconfig::ProviderId::Codex,
    ),
    (
        "opencode",
        "OpenCode · OpenCode",
        "OpenCode",
        wyss_console::agentconfig::ProviderId::Opencode,
    ),
];

pub fn category_counts<'a>(
    entries: impl Iterator<Item = &'a GuideEntry>,
    tool: &str,
    set: &str,
) -> std::collections::BTreeMap<&'static str, usize> {
    let mut counts = std::collections::BTreeMap::new();
    for entry in entries.filter(|entry| entry.tool == tool && entry.set == set) {
        *counts.entry(category(entry)).or_default() += 1;
    }
    counts
}

pub fn matches(entry: &GuideEntry, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    [&entry.name, &entry.description]
        .iter()
        .any(|text| text.to_lowercase().contains(&query))
}

pub fn first_sentence(text: &str) -> &str {
    let text = text.trim();
    for (start, char) in text.char_indices() {
        if char == '\n' {
            return text[..start].trim_end();
        }
        let end = start + char.len_utf8();
        if matches!(char, '.' | '!' | '?' | '。')
            && text[end..].chars().next().is_none_or(char::is_whitespace)
        {
            return &text[..end];
        }
    }
    text
}

pub const CATEGORIES: [&str; 14] = [
    t!("Models"),
    t!("Research"),
    t!("Decisions"),
    t!("Planning and method"),
    t!("Building"),
    t!("Design and interface"),
    t!("Animation and video"),
    t!("Content and SEO"),
    t!("Docs and writing"),
    t!("Review and testing"),
    t!("Fixing bugs"),
    t!("Security"),
    t!("System and setup"),
    t!("Study and knowledge"),
];

pub fn category(entry: &GuideEntry) -> &'static str {
    let name = entry.name.to_lowercase();
    if entry.kind == "agent"
        && ["astra_", "luna_", "sol_"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
    {
        return t!("Models");
    }
    crate::workspace::categories::classify(&entry.name, &entry.description).1
}

pub fn launches_for<'a>(selection: &Selection, launches: &'a [Launch]) -> Vec<&'a Launch> {
    launches
        .iter()
        .filter(|launch| {
            matches!(&launch.kind, Kind::Session { agent, preset: Some(name), .. }
            if agent == &selection.provider && name == &selection.name)
        })
        .collect()
}

pub fn duplicate(selection: &Selection, selections: &[Selection]) -> bool {
    selections.iter().any(|other| {
        other.id != selection.id
            && other.provider == selection.provider
            && other.name == selection.name
    })
}

pub fn notes(sync: &Status) -> [(&'static str, &[Note]); 4] {
    [
        (t!("Waiting"), &sync.pending),
        (t!("Conflicts kept both versions"), &sync.conflicts),
        (t!("Refused"), &sync.rejected),
        (t!("Deleted on another computer"), &sync.removed),
    ]
}

/// What a change received from another computer does here, in words.
pub fn incoming_change(incoming: &Incoming) -> &'static str {
    match (incoming.kind.as_str(), incoming.change.as_str()) {
        ("capability", "new") => t!("New capability"),
        ("capability", "revision") => t!("New revision of a capability you have"),
        ("capability", "restore") => t!("Goes back to an older revision you have"),
        ("preset", "new") => t!("New preset"),
        ("preset", "deleted") => t!("Deletes this preset"),
        ("preset", _) => t!("Changes this preset"),
        ("selection", "new") => t!("New selection"),
        ("selection", "deleted") => t!("Deletes this selection"),
        (_, _) => t!("Changes this selection"),
    }
}

/// One received file as the list shows it; a script says so first.
pub fn incoming_file(file: &IncomingFile) -> (bool, String) {
    (file.script, format!("{} · {} B", file.path, file.size))
}

#[derive(Debug, PartialEq, Eq)]
pub enum SyncMark {
    Error,
    Receiving,
    Attention {
        conflicts: usize,
        refused: usize,
        removed: usize,
    },
    Published,
    Quiet,
}

pub fn sync_mark(sync: &Status, now: i64) -> SyncMark {
    let recent = |notes: &[Note]| {
        notes
            .iter()
            .filter(|note| now.saturating_sub(note.at) < 86400)
            .count()
    };
    if sync.error.is_some() {
        return SyncMark::Error;
    }
    if !sync.pending.is_empty() {
        return SyncMark::Receiving;
    }
    let (conflicts, refused, removed) = (
        recent(&sync.conflicts),
        recent(&sync.rejected),
        recent(&sync.removed),
    );
    if conflicts + refused + removed != 0 {
        return SyncMark::Attention {
            conflicts,
            refused,
            removed,
        };
    }
    if sync
        .peers
        .iter()
        .any(|peer| peer.applied_mine < peer.published_mine)
    {
        return SyncMark::Published;
    }
    SyncMark::Quiet
}

pub struct ProviderCounts {
    pub skills: usize,
    pub agents: usize,
    pub presets: usize,
    pub hooks: usize,
    pub configuration: usize,
    pub mcp: usize,
}

impl ProviderCounts {
    pub fn total(&self) -> usize {
        // The total counts skills, agents, presets and hooks. Configuration and MCP have separate counts.
        self.skills + self.agents + self.presets + self.hooks
    }
}

pub fn item_count_key(count: usize) -> &'static str {
    if count == 1 {
        crate::t!("{count} item")
    } else {
        crate::t!("{count} items")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wyss_console::arsenal::{self, store::SelectionInput, sync::Paths};

    #[test]
    fn provider_total_excludes_configuration_and_counts_agents() {
        let claude = ProviderCounts {
            skills: 118,
            agents: 0,
            presets: 5,
            hooks: 9,
            configuration: 3,
            mcp: 0,
        };
        assert_eq!(claude.total(), 132);
        let codex = ProviderCounts {
            skills: 0,
            agents: 3,
            presets: 0,
            hooks: 2,
            configuration: 1,
            mcp: 4,
        };
        assert_eq!(codex.total(), 5);
        let opencode = ProviderCounts {
            agents: 0,
            hooks: 0,
            ..codex
        };
        assert_eq!(opencode.total(), 0);
    }

    #[test]
    fn received_changes_are_described_by_kind_and_scripts_are_marked() {
        let mut incoming = Incoming {
            object: "capability/x".into(),
            record: "r".into(),
            records: vec![],
            device: "d".into(),
            host: "peer".into(),
            kind: "capability".into(),
            name: "design".into(),
            change: "new".into(),
            files: vec![],
            scripts: false,
            received_at: 0,
        };
        assert_eq!(incoming_change(&incoming), "New capability");
        incoming.change = "revision".into();
        assert_eq!(
            incoming_change(&incoming),
            "New revision of a capability you have"
        );
        incoming.kind = "preset".into();
        incoming.change = "deleted".into();
        assert_eq!(incoming_change(&incoming), "Deletes this preset");
        incoming.kind = "selection".into();
        incoming.change = "changed".into();
        assert_eq!(incoming_change(&incoming), "Changes this selection");
        let file = IncomingFile {
            path: "scripts/run.sh".into(),
            size: 12,
            script: true,
        };
        assert_eq!(incoming_file(&file), (true, "scripts/run.sh · 12 B".into()));
    }

    #[test]
    fn preset_item_count_uses_the_singular_only_for_one() {
        assert_eq!(item_count_key(0), "{count} items");
        assert_eq!(item_count_key(1), "{count} item");
        assert_eq!(item_count_key(2), "{count} items");
    }

    #[test]
    fn category_counts_are_scoped_to_the_tool_and_branch() {
        let mut entry = GuideEntry {
            id: "fixture".into(),
            tool: "claude_code",
            kind: "skill",
            name: "fixture".into(),
            description: String::new(),
            set: "mine".into(),
            origin: String::new(),
            linked_to: None,
            scope: None,
            codex_differs: false,
            unreadable: None,
            description_cut: false,
            description_es: None,
        };
        let initial = entry.clone();
        entry.tool = "codex";
        let codex = entry.clone();
        entry.tool = "claude_code";
        entry.set = "plugin".into();
        let items = [initial.clone(), initial.clone(), codex, entry];
        assert_eq!(
            category_counts(items.iter(), "claude_code", "mine"),
            [(category(&initial), 2)].into()
        );
        assert_eq!(
            category_counts(items.iter(), "codex", "mine")
                .values()
                .sum::<usize>(),
            1
        );
        assert_eq!(
            category_counts(items.iter(), "claude_code", "plugin")
                .values()
                .sum::<usize>(),
            1
        );
        assert!(category_counts(items.iter(), "opencode", "mine").is_empty());
    }

    #[test]
    fn summaries_end_at_the_first_sentence_or_line() {
        assert_eq!(
            first_sentence("  v1.2 works! Next sentence."),
            "v1.2 works!"
        );
        assert_eq!(first_sentence("A line\nAnother line."), "A line");
        assert_eq!(first_sentence("日本語。 Next sentence"), "日本語。");
    }

    #[test]
    fn distinguishes_pending_acknowledgments_and_recent_notes() {
        let mut status = Status::default();
        let note = Note {
            at: 1,
            kind: "selection".into(),
            name: "Test".into(),
            detail: String::new(),
        };
        status.conflicts.push(note.clone());
        assert_eq!(
            sync_mark(&status, 2),
            SyncMark::Attention {
                conflicts: 1,
                refused: 0,
                removed: 0
            }
        );
        assert_eq!(sync_mark(&status, 86401), SyncMark::Quiet);
        status.peers.push(arsenal::sync::Peer {
            host: "peer".into(),
            device: "peer".into(),
            last_heard: None,
            applied_mine: 2,
            published_mine: 3,
        });
        assert_eq!(sync_mark(&status, 86401), SyncMark::Published);
        status.pending.push(note);
        assert_eq!(sync_mark(&status, 86401), SyncMark::Receiving);
        status.error = Some("error".into());
        assert_eq!(sync_mark(&status, 86401), SyncMark::Error);
    }

    #[test]
    fn selection_save_round_trip_and_sync_in_fixture() {
        crate::personalization_round_trip("arsenal", "arsenal::model::tests::selection_save_child");
    }

    #[test]
    fn selection_save_child() {
        let Some(root) = crate::personalization_child_root("arsenal") else {
            return;
        };
        let paths = Paths::from_env().unwrap();
        assert_eq!(paths.shared, root.join("data/wyss-console-sync/arsenal"));
        std::fs::create_dir_all(paths.shared.parent().unwrap()).unwrap();
        assert_eq!(paths.local, root.join("state/wyss-console/arsenal-sync"));
        assert_eq!(
            arsenal::environment::directory().unwrap(),
            root.join("data/wyss-console/arsenal")
        );
        let (mut store, directory) = arsenal::open().unwrap();
        assert_eq!(directory, root.join("data/wyss-console/arsenal"));
        let saved = store
            .save_selection(&SelectionInput {
                id: None,
                name: "Example".into(),
                provider: "codex".into(),
                items: vec![],
                composites: vec![],
            })
            .unwrap();
        drop(store);
        let mut store = arsenal::open_read_only().unwrap();
        assert_eq!(
            store.selections().unwrap().as_slice(),
            std::slice::from_ref(&saved)
        );
        assert!(!duplicate(&saved, &store.selections().unwrap()));
        assert!(store.delete_selection(&saved.id).is_err());
        drop(store);
        let (mut store, _) = arsenal::open().unwrap();
        let status = arsenal::sync::reconcile(&mut store, &paths, "fixture").unwrap();
        assert_eq!(status.published, 1);
        assert_eq!(arsenal::sync::read_status(&paths, "fixture").published, 1);
    }
}
