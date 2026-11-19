use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use wyss_console::{
    arsenal::{
        self, environment,
        role_instructions::{self, Choice},
        roles,
        store::{Item, Selection, SelectionInput, Store},
    },
    launch::{self, Kind, Launch, Places, SaveRefusal, SaveRequest},
    native::{
        launch_home::{HomeSpec, HookOption, Options, Owner},
        launch_home_codex::{self, CodexHomeSpec, CodexOptions, CodexOwner},
    },
};

pub const CLAUDE: &str = "claude_code";
pub const CODEX: &str = "codex";

pub const GENERAL_FIELDS: [&str; 5] = [
    crate::t!("Application"),
    crate::t!("Working directory"),
    crate::t!("Permissions"),
    crate::t!("Project instructions"),
    crate::t!("Global instructions"),
];

pub fn tabs(agent: &str) -> impl Iterator<Item = (&'static str, &'static str)> {
    [
        ("general", crate::t!("General")),
        ("skills", crate::t!("Skills")),
        ("mcp", "MCP"),
        ("hooks", crate::t!("Hooks")),
        ("summary", crate::t!("Summary")),
    ]
    .into_iter()
    .filter(move |(id, _)| *id != "mcp" || agent == CLAUDE)
}

pub fn permission_label(id: &str) -> &'static str {
    match id {
        "ask" => crate::t!("Ask before acting"),
        "read_only" => crate::t!("Read only"),
        "edits" => crate::t!("Accept edits"),
        "bypass" => crate::t!("Full access"),
        _ => crate::t!("As configured"),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CommandSection {
    Options,
    Commands,
}

pub fn command_sections(has_recipe: bool) -> impl Iterator<Item = CommandSection> {
    has_recipe
        .then_some(CommandSection::Options)
        .into_iter()
        .chain([CommandSection::Commands])
}

pub fn groups<'a>(launches: &'a [Launch], query: &str) -> (Vec<&'a Launch>, Vec<&'a Launch>) {
    let query = query.trim().to_lowercase();
    launches
        .iter()
        .filter(|entry| entry.name.to_lowercase().contains(&query))
        .partition(|entry| matches!(entry.kind, Kind::Commands { .. }))
}

#[cfg(test)]
#[test]
fn groups_filter_cached_launches_without_changing_group_order() {
    let launches = [
        Launch {
            id: "agent-1".into(),
            name: "Review Alpha".into(),
            directory: "/absent".into(),
            kind: Kind::Session {
                agent: CLAUDE.into(),
                permission: None,
                before: vec![],
                preset: None,
            },
        },
        Launch {
            id: "cmd".into(),
            name: "System Review".into(),
            directory: "/absent".into(),
            kind: Kind::Commands {
                commands: vec![],
                variants: vec![],
                active: None,
                template: None,
            },
        },
        Launch {
            id: "agent-2".into(),
            name: "Review Beta".into(),
            directory: "/absent".into(),
            kind: Kind::Session {
                agent: CODEX.into(),
                permission: None,
                before: vec![],
                preset: None,
            },
        },
    ];
    let (commands, agents) = groups(&launches, "  rEVIEW ");
    assert_eq!(commands, [&launches[1]]);
    assert_eq!(agents, [&launches[0], &launches[2]]);
    assert_eq!(groups(&launches, "beta").1, [&launches[2]]);
    assert_eq!(groups(&launches, "missing"), (vec![], vec![]));
}

pub fn permissions(agent: &str) -> impl Iterator<Item = &'static str> {
    launch::PERMISSIONS
        .into_iter()
        .filter(move |permission| agent == CLAUDE || *permission != "edits")
}

pub fn project_choice(index: u32, text: String) -> Choice {
    match index {
        1 => Choice::Exclude { role: text },
        2 => Choice::Role { role: text },
        _ => Choice::Load,
    }
}

pub fn set_off(values: &mut Vec<String>, key: &str, enabled: bool) {
    values.retain(|value| value != key);
    if !enabled {
        values.push(key.to_owned());
    }
}

pub fn set_skill(items: &mut Vec<Item>, id: &str, on: bool) {
    if on && !items.iter().any(|item| item.capability == id) {
        items.push(Item {
            capability: id.to_owned(),
            revision: None,
        });
    } else if !on {
        items.retain(|item| item.capability != id);
    }
}

pub fn set_mcp(spec: &mut HomeSpec, servers: &[String], keys: &[String], enabled: bool) {
    let mut kept = spec.mcp.clone().unwrap_or_default();
    let mut seen = spec.mcp_seen.clone().unwrap_or_default();
    for name in servers {
        if spec.mcp_enabled(name) {
            set_off(&mut kept, name, false);
        }
    }
    for key in keys {
        set_off(&mut kept, key, !enabled);
    }
    for name in servers {
        set_off(&mut seen, name, false);
    }
    spec.mcp = Some(kept);
    spec.mcp_seen = Some(seen);
}

const HOOK_EVENTS: [&str; 3] = ["SessionStart", "PreToolUse", "PostToolUse"];

pub fn event_label(event: &str) -> &str {
    use crate::t;
    match event {
        "SessionStart" => t!("When the session starts"),
        "PreToolUse" => t!("Before using a tool"),
        "PostToolUse" => t!("After using a tool"),
        "PostToolUseFailure" => t!("After a tool fails"),
        "UserPromptSubmit" => t!("Before a message is sent"),
        "PermissionRequest" => t!("When permission is requested"),
        "Notification" => t!("When a notification arrives"),
        "Stop" => t!("When the agent stops"),
        "SubagentStart" => t!("When an agent starts"),
        "SubagentStop" => t!("When an agent stops"),
        "PreCompact" => t!("Before the conversation is compacted"),
        "SessionEnd" => t!("When the session ends"),
        "TeammateIdle" => t!("When a teammate is idle"),
        "TaskCompleted" => t!("When a task completes"),
        "ConfigChange" => t!("When configuration changes"),
        "WorktreeCreate" => t!("When a worktree is created"),
        "WorktreeRemove" => t!("When a worktree is removed"),
        "InstructionsLoaded" => t!("When instructions are loaded"),
        "" => "Unknown",
        _ => event,
    }
}

fn hook_label(command: &str) -> &str {
    command
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '\'' | '"'))
        .rev()
        .find(|part| {
            [".sh", ".py", ".js", ".mjs"]
                .iter()
                .any(|ext| part.ends_with(ext))
        })
        .and_then(|path| path.rsplit('/').next())
        .unwrap_or_else(|| command.trim())
}

type HookRows = Vec<(String, Vec<HookOption>)>;

pub fn hooks(entries: &[HookOption]) -> Vec<(String, HookRows)> {
    let mut events = BTreeMap::<String, HookRows>::new();
    for hook in entries {
        let event = if hook.event.is_empty() {
            "Unknown"
        } else {
            &hook.event
        };
        let script = hook_label(&hook.command);
        let rows = events.entry(event.into()).or_default();
        if let Some((_, grouped)) = rows.iter_mut().find(|(name, _)| name == script) {
            grouped.push(hook.clone());
        } else {
            rows.push((script.into(), vec![hook.clone()]));
        }
    }
    let mut events = events.into_iter().collect::<Vec<_>>();
    events.sort_by_key(|(event, _)| {
        HOOK_EVENTS
            .iter()
            .position(|name| name == event)
            .unwrap_or(HOOK_EVENTS.len())
    });
    events
}

pub fn fixed_hooks(options: &CodexOptions) -> Vec<HookOption> {
    options
        .config_hooks
        .iter()
        .map(|hook| HookOption {
            key: String::new(),
            event: hook.event.clone(),
            matcher: hook.matcher.clone(),
            command: hook.command.clone(),
        })
        .collect()
}

#[derive(Clone, Default)]
struct Initial {
    claude: HomeSpec,
    codex: CodexHomeSpec,
    project: Option<Choice>,
}

#[derive(Clone)]
pub struct Draft {
    pub id: Option<String>,
    pub name: String,
    pub directory: String,
    pub agent: String,
    pub role: String,
    pub permission: Option<String>,
    pub before: Vec<String>,
    pub selection: Option<Selection>,
    pub items: Vec<Item>,
    pub new_skills: BTreeSet<String>,
    initial_items: Vec<Item>,
    read_only_selection: bool,
    initial: Initial,
    baseline: Option<Box<Draft>>,
    pub claude: HomeSpec,
    pub codex: CodexHomeSpec,
    pub project: Choice,
}

impl Draft {
    pub fn load(
        places: &Places,
        launch: Option<&Launch>,
        agent: &str,
        store: Option<&Store>,
    ) -> Result<Self, String> {
        let dir = environment::directory().ok_or("Arsenal directory unavailable")?;
        let selections = store
            .map(Store::selections)
            .transpose()
            .map_err(|e| format!("{e:?}"))?
            .unwrap_or_default();
        let mut draft = Self {
            id: None,
            name: String::new(),
            directory: places.home.to_string_lossy().into_owned(),
            agent: agent.into(),
            role: roles::DEFAULT_ROLE.into(),
            permission: None,
            before: vec![],
            selection: None,
            items: vec![],
            new_skills: BTreeSet::new(),
            initial_items: vec![],
            read_only_selection: false,
            initial: Initial::default(),
            baseline: None,
            claude: HomeSpec::default(),
            codex: CodexHomeSpec::default(),
            project: Choice::Load,
        };
        if let Some(Launch {
            id,
            name,
            directory,
            kind:
                Kind::Session {
                    agent,
                    permission,
                    before,
                    preset,
                },
        }) = launch
        {
            draft.id = Some(id.clone());
            draft.name.clone_from(name);
            draft.directory.clone_from(directory);
            draft.agent.clone_from(agent);
            draft.permission.clone_from(permission);
            draft.before.clone_from(before);
            draft.role = roles::read(&dir)
                .assigned
                .remove(id)
                .unwrap_or_else(|| roles::DEFAULT_ROLE.into());
            if let Some(preset) = preset {
                let selection = selections
                    .into_iter()
                    .find(|selection| selection.name == *preset && selection.provider == *agent)
                    .ok_or("The launch's Arsenal preset could not be read")?;
                draft.project =
                    role_instructions::get(&dir, &selection.id).map_err(|e| e.to_string())?;
                if agent == CLAUDE {
                    draft.claude = environment::get(&dir, &selection.id)
                        .map_err(|e| e.to_string())?
                        .unwrap_or_default();
                } else {
                    draft.codex = environment::codex::get(&dir, &selection.id)
                        .map_err(|e| e.to_string())?
                        .unwrap_or_default();
                }
                draft.items.clone_from(&selection.items);
                draft.selection = Some(selection);
            }
        }
        if draft.selection.is_none() {
            draft.items = all_items(store, &draft.agent)?;
        }
        draft.initial_items.clone_from(&draft.items);
        draft.initial = Initial {
            claude: draft.claude.clone(),
            codex: draft.codex.clone(),
            project: Some(draft.project.clone()),
        };
        draft.baseline = Some(Box::new(draft.clone()));
        draft.include_new_skills(store);
        Ok(draft)
    }

    pub fn from_preset(
        places: &Places,
        selection: &Selection,
        store: &Store,
    ) -> Result<Self, String> {
        let source = Launch {
            id: String::new(),
            name: selection.name.clone(),
            directory: places.home.to_string_lossy().into_owned(),
            kind: Kind::Session {
                agent: selection.provider.clone(),
                permission: None,
                before: Vec::new(),
                preset: Some(selection.name.clone()),
            },
        };
        let mut draft = Self::load(places, Some(&source), &selection.provider, Some(store))?;
        if draft.selection.as_ref().map(|preset| &preset.id) != Some(&selection.id) {
            return Err("That preset is no longer available. Refresh and try again.".into());
        }
        draft.id = None;
        draft.new_skills.clear();
        draft.items.clone_from(&draft.initial_items);
        draft.read_only_selection = true;
        if let Some(baseline) = &mut draft.baseline {
            baseline.name.clear();
        }
        Ok(draft)
    }

    pub fn include_new_skills(&mut self, store: Option<&Store>) {
        let (Some(selection), Some(store)) = (&self.selection, store) else {
            return;
        };
        if let Ok(items) = store.list() {
            for item in items {
                if item.provider == self.agent
                    && item.kind == "skill"
                    && item.updated_at > selection.updated_at
                    && !self.new_skills.contains(&item.id)
                    && !self
                        .initial_items
                        .iter()
                        .any(|selected| selected.capability == item.id)
                    && store.detail(&item.id, None).is_ok_and(|detail| {
                        detail.revisions.iter().any(|revision| {
                            revision.number == 1 && revision.created_at > selection.updated_at
                        })
                    })
                {
                    set_skill(&mut self.items, &item.id, true);
                    self.new_skills.insert(item.id);
                }
            }
        }
    }

    pub fn set_skill(&mut self, id: &str, on: bool) {
        set_skill(&mut self.items, id, on);
        if on
            && let Some(item) = self.items.iter_mut().find(|item| item.capability == id)
            && let Some(pin) = self
                .selection
                .as_ref()
                .and_then(|selection| selection.items.iter().find(|item| item.capability == id))
        {
            item.revision = pin.revision;
        }
    }

    pub fn can_save(&self) -> bool {
        Path::new(&self.directory).is_absolute()
            && !launch::slug(&self.name).is_empty()
            && self.name.trim().len() <= launch::MAX_NAME
            && self.before.is_empty()
            && match &self.project {
                Choice::Role { role } => {
                    !role.is_empty() && role.len() <= 64 * 1024 && !role.contains('\0')
                }
                Choice::Exclude { role } => role.len() <= 64 * 1024 && !role.contains('\0'),
                Choice::Load => true,
            }
    }

    pub fn dirty(&self) -> bool {
        self.baseline.as_ref().is_some_and(|saved| {
            self.name != saved.name
                || self.directory != saved.directory
                || self.agent != saved.agent
                || self.permission != saved.permission
                || self.selection != saved.selection
                || self.items.len() != saved.items.len()
                || self.items.iter().any(|item| !saved.items.contains(item))
                || self.claude != saved.claude
                || self.codex != saved.codex
                || self.project != saved.project
        })
    }

    pub fn uses_global(&self) -> bool {
        self.selection.is_none() && !self.needs_selection()
    }

    fn needs_selection(&self) -> bool {
        !self.new_skills.is_empty()
            || self.items != self.initial_items
            || &self.project != self.initial.project.as_ref().unwrap_or(&Choice::Load)
            || if self.agent == CLAUDE {
                self.claude != self.initial.claude
            } else {
                self.codex != self.initial.codex
            }
    }
}

fn all_items(store: Option<&Store>, agent: &str) -> Result<Vec<Item>, String> {
    Ok(store
        .map(Store::list)
        .transpose()
        .map_err(|e| format!("{e:?}"))?
        .unwrap_or_default()
        .into_iter()
        .filter(|item| item.provider == agent && matches!(item.kind.as_str(), "skill" | "agent"))
        .map(|item| Item {
            capability: item.id,
            revision: None,
        })
        .collect())
}

pub fn change_agent(draft: &mut Draft, store: Option<&Store>, agent: &str) -> Result<(), String> {
    let items = all_items(store, agent)?;
    draft.agent = agent.into();
    draft.new_skills.clear();
    draft.permission = draft
        .permission
        .take()
        .filter(|value| permissions(agent).any(|p| p == value));
    draft.selection = None;
    draft.initial_items.clone_from(&items);
    draft.items = items;
    draft.claude = HomeSpec::default();
    draft.codex = CodexHomeSpec::default();
    draft.project = Choice::Load;
    draft.initial = Initial::default();
    Ok(())
}

struct Known<'a>(&'a [Selection]);

impl launch::Known for Known<'_> {
    fn presets(&self, agent: &str) -> Result<Vec<String>, String> {
        Ok(self
            .0
            .iter()
            .filter(|selection| selection.provider == agent)
            .map(|selection| selection.name.clone())
            .collect())
    }
}

pub fn save(places: &Places, draft: &Draft) -> Result<Launch, String> {
    if draft.read_only_selection && draft.needs_selection() {
        return Err(
            crate::t!("Changes that write to the Arsenal library are unavailable here.").into(),
        );
    }
    let id = launch::slug(&draft.name);
    let replace = draft.id.as_deref() == Some(&id);
    if id.is_empty() || draft.name.trim().len() > launch::MAX_NAME {
        return Err("Invalid launch name".into());
    }
    if !replace && std::fs::symlink_metadata(places.folder.join(format!("{id}.toml"))).is_ok() {
        return Err("A launch with that name already exists".into());
    }
    if !replace && draft.id.is_some() && !draft.before.is_empty() {
        return Err("This launch has startup commands and cannot be renamed here".into());
    }
    let directory = environment::directory().ok_or("Arsenal directory unavailable")?;
    let current = draft.selection.as_ref();
    let mut selections = Vec::new();
    let preset = if draft.needs_selection() {
        let (mut store, _) = arsenal::open().map_err(|e| format!("{e:?}"))?;
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let name = format!("Workspace-{id}-{}", &suffix[..12]);
        let target = store
            .save_selection(&SelectionInput {
                id: None,
                name,
                provider: draft.agent.clone(),
                items: draft.items.clone(),
                composites: current
                    .map(|selection| selection.composites.clone())
                    .unwrap_or_default(),
            })
            .map_err(|e| format!("{e:?}"))?;
        if draft.agent == CLAUDE {
            environment::set(&directory, &target.id, &draft.claude).map_err(|e| e.to_string())?;
        } else {
            environment::codex::set(&directory, &target.id, &draft.codex)
                .map_err(|e| e.to_string())?;
        }
        role_instructions::set(&directory, &target.id, &draft.project)
            .map_err(|e| e.to_string())?;
        let name = target.name.clone();
        selections.push(target);
        Some(name)
    } else if let Some(current) = current {
        let store = arsenal::open_read_only().map_err(|e| format!("{e:?}"))?;
        selections = store.selections().map_err(|e| format!("{e:?}"))?;
        Some(current.name.clone())
    } else {
        None
    };
    let saved = launch::save_session(
        places,
        &SaveRequest {
            name: draft.name.trim().into(),
            directory: draft.directory.clone(),
            agent: draft.agent.clone(),
            permission: draft.permission.clone(),
            preset,
            replace,
        },
        &Known(&selections),
        &draft.before,
    )
    .map_err(|error| match error {
        SaveRefusal::Exists => "A launch with that name already exists".into(),
        SaveRefusal::Invalid(message) => message,
    })?;
    // The owner chose everything this file says here, full access included, so its text is
    // approved. `before` commands are not shown in this editor: a file with them stays to be
    // approved, with the commands in view, when it is opened.
    if draft.before.is_empty()
        && let Ok(text) = std::fs::read_to_string(places.folder.join(format!("{}.toml", saved.id)))
        && let Some(approvals) = launch::Approvals::from_env()
    {
        let _ = approvals.approve(&launch::approval_print(&text, &saved.directory));
    }
    roles::write(
        &directory,
        roles::Change::Assign {
            launch: saved.id.clone(),
            role: draft.role.clone(),
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    if let Some(old) = &draft.id
        && *old != saved.id
    {
        launch::delete(places, old)?;
        roles::write(
            &directory,
            roles::Change::Forget {
                launch: old.clone(),
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    }
    Ok(saved)
}

pub type Inventory = (Options, CodexOptions);

pub fn inventory(home: &Path) -> Inventory {
    (
        wyss_console::native::launch_home::options(&Owner::from_home(home)),
        launch_home_codex::options(&CodexOwner::from_process(home)),
    )
}

impl Draft {
    pub fn hook_exclusions(&self) -> &[String] {
        if self.agent == CLAUDE {
            &self.claude.hooks_off
        } else {
            &self.codex.hooks_off
        }
    }

    pub fn set_hooks(&mut self, entries: &[HookOption], on: bool) {
        let off = if self.agent == CLAUDE {
            &mut self.claude.hooks_off
        } else {
            &mut self.codex.hooks_off
        };
        for entry in entries {
            set_off(off, &entry.key, on);
        }
    }
}

pub fn hook_counts(draft: &Draft, groups: &[(String, HookRows)]) -> (usize, usize) {
    let rows = groups.iter().flat_map(|(_, rows)| rows);
    (
        rows.clone()
            .filter(|(_, hooks)| {
                hooks
                    .iter()
                    .all(|hook| !draft.hook_exclusions().contains(&hook.key))
            })
            .count(),
        rows.count(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_update_exposes_commands_below_options() {
        use CommandSection::{Commands, Options};

        assert_eq!(
            command_sections(true).collect::<Vec<_>>(),
            [Options, Commands]
        );
        assert_eq!(command_sections(false).collect::<Vec<_>>(), [Commands]);
    }

    #[test]
    fn general_permission_is_provider_specific() {
        assert!(permissions(CLAUDE).any(|p| p == "edits"));
        assert!(!permissions(CODEX).any(|p| p == "edits"));
    }

    #[test]
    fn instructions_keep_custom_text() {
        assert_eq!(
            project_choice(2, "Guide".into()),
            Choice::Role {
                role: "Guide".into()
            }
        );
        assert_eq!(project_choice(0, "Unused".into()), Choice::Load);
    }

    #[test]
    fn skill_toggle_preserves_pinned_revision() {
        let mut items = vec![Item {
            capability: "a".into(),
            revision: Some(2),
        }];
        set_skill(&mut items, "a", true);
        assert_eq!(items[0].revision, Some(2));
        set_skill(&mut items, "a", false);
        assert!(items.is_empty());
    }

    #[test]
    fn mcp_keeps_global_configuration_until_first_change() {
        let mut spec = HomeSpec::default();
        assert_eq!(spec.mcp, None);
        set_mcp(
            &mut spec,
            &["local".into(), "npm".into()],
            &["local".into()],
            false,
        );
        assert_eq!(spec.mcp, Some(vec!["npm".into()]));
        assert!(spec.mcp_is_new("added"));
        assert!(spec.mcp_enabled("added"));
        assert!(!spec.mcp_enabled("local"));
        set_mcp(
            &mut spec,
            &["local".into(), "npm".into(), "added".into()],
            &["added".into()],
            false,
        );
        assert!(!spec.mcp_is_new("added"));
        assert!(!spec.mcp_enabled("added"));
    }

    #[test]
    fn hooks_group_the_same_script_within_each_event() {
        let entries = ["SessionStart", "PreToolUse", "PreToolUse"].map(|event| HookOption {
            key: event.into(),
            event: event.into(),
            matcher: String::new(),
            command: format!("bash first.sh hook.sh {event}"),
        });
        let groups = hooks(&entries);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, "SessionStart");
        assert_eq!(groups[0].1[0].0, "hook.sh");
        assert_eq!(groups[0].1[0].1.len(), 1);
        assert_eq!(groups[1].1[0].1.len(), 2);
    }

    #[test]
    fn launch_editor_uses_installed_tab_order_and_general_fields() {
        let ids = |agent| tabs(agent).map(|(id, _)| id).collect::<Vec<_>>();
        assert_eq!(
            ids(CLAUDE),
            ["general", "skills", "mcp", "hooks", "summary"]
        );
        assert_eq!(ids(CODEX), ["general", "skills", "hooks", "summary"]);
        assert_eq!(
            GENERAL_FIELDS,
            [
                "Application",
                "Working directory",
                "Permissions",
                "Project instructions",
                "Global instructions"
            ]
        );
    }

    #[test]
    fn workspace_save_and_reload_in_private_xdg_configuration() {
        let root = std::env::temp_dir().join(format!("workspace-{}", uuid::Uuid::new_v4()));
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        let result = command
            .args(["--exact", "workspace::model::tests::round_trip_child"])
            .env("HOME", root.join("home"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("XDG_STATE_HOME", root.join("state"))
            .env("XDG_CACHE_HOME", root.join("cache"))
            .env("XDG_RUNTIME_DIR", root.join("run"))
            .env("CODEX_HOME", root.join("codex"))
            .env("WYSS_WORKSPACE_ROUND_TRIP", &root)
            .output()
            .unwrap();
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stdout)
        );
    }

    #[test]
    fn round_trip_child() {
        let Some(root) = std::env::var_os("WYSS_WORKSPACE_ROUND_TRIP").map(std::path::PathBuf::from) else {
            return;
        };
        assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
        assert!(
            root.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.strip_prefix("workspace-")
                        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
                })
        );
        assert_eq!(
            environment::directory().unwrap(),
            root.join("data/wyss-console/arsenal")
        );
        let mut places = Places::from_env().unwrap();
        assert_eq!(places.home, root.join("home"));
        assert_eq!(places.folder, root.join("config/wyss-console/launch"));
        std::fs::create_dir_all(&places.home).unwrap();
        places.os_release = root.join("os-release");
        let mut estimate_draft = Draft::load(&places, None, CLAUDE, None).unwrap();
        estimate_draft.claude.rules_off.push("off".into());
        estimate_draft
            .claude
            .account_skills_off
            .push("private".into());
        let options = Options {
            rules: vec!["on".into(), "off".into()],
            account_skills: vec!["account".into(), "private".into()],
            plugins: vec!["plugin".into()],
            sizes: [
                ("instructions".into(), 5),
                ("rule:on".into(), 1),
                ("rule:off".into(), 400),
                ("account:account".into(), 5),
                ("account:private".into(), 400),
                ("plugin:plugin".into(), 4),
            ]
            .into(),
            ..Options::default()
        };
        assert_eq!(estimate(&estimate_draft, &options, &[]), 14_008);
        estimate_draft.claude.instructions = false;
        assert_eq!(estimate(&estimate_draft, &options, &[]), 14_005);
        let missing = places.folder.join("missing-directory.toml");
        launch::list_for_editor(&places).unwrap();
        std::fs::write(
            &missing,
            "name = \"Missing fixture\"\ndirectory = \"~/absent\"\n[session]\nagent = \"codex\"\n",
        )
        .unwrap();
        assert_eq!(
            launch::get(&places, "missing-directory").unwrap_err(),
            "directory not found"
        );
        let editable = launch::get_for_editor(&places, "missing-directory").unwrap();
        assert_eq!(editable.name, "Missing fixture");
        assert_eq!(
            editable.directory,
            places.home.join("absent").display().to_string()
        );
        assert!(
            launch::list_for_editor(&places)
                .unwrap()
                .launches
                .contains(&editable)
        );

        let mut draft = Draft::load(&places, None, CODEX, None).unwrap();
        assert!(draft.uses_global());
        assert!(!draft.dirty());
        assert!(!draft.can_save());
        draft.name = "Workspace fixture".into();
        assert!(draft.dirty());
        assert!(draft.can_save());
        draft.permission = Some("read_only".into());
        draft.project = project_choice(2, "Fixture only".into());
        assert!(!draft.uses_global());
        let saved = save(&places, &draft).unwrap();
        assert_eq!(launch::get(&places, &saved.id).unwrap(), saved);
        // Saved here, the file is approved: opening it with full access asks nothing.
        let text = std::fs::read_to_string(places.folder.join(format!("{}.toml", saved.id))).unwrap();
        let approvals = launch::Approvals::from_env().unwrap();
        let runs = launch::Runs { directory: &saved.directory, commands: &[], before: &[], bypass: true };
        assert!(approvals.check(&saved.id, &launch::approval_print(&text, &saved.directory), runs).is_ok());
        assert!(approvals.check(&saved.id, &launch::approval_print(&format!("{text}#"), &saved.directory), runs).is_err());
        assert!(approvals.check(&saved.id, &launch::approval_print(&text, "/"), runs).is_err(), "another folder asks");
        let listing = launch::list_for_editor(&places).unwrap();
        assert_eq!(
            groups(&listing.launches, "workspace fixture").1,
            vec![&saved]
        );
        let store = arsenal::open_read_only().unwrap();
        let read = Draft::load(&places, Some(&saved), CODEX, Some(&store)).unwrap();
        assert_eq!(read.permission, draft.permission);
        assert_eq!(read.project, draft.project);
        let selection = read.selection.as_ref().unwrap();
        let mut from_preset = Draft::from_preset(&places, selection, &store).unwrap();
        from_preset.name = "Another launch".into();
        assert!(from_preset.dirty() && from_preset.can_save());
        let copy = save(&places, &from_preset).unwrap();
        assert_eq!(launch::get(&places, &copy.id).unwrap(), copy);
        assert!(
            matches!(copy.kind, Kind::Session { preset: Some(name), .. } if name == selection.name)
        );
        from_preset.project = Choice::Load;
        assert_eq!(
            save(&places, &from_preset).unwrap_err(),
            "Changes that write to the Arsenal library are unavailable here."
        );
        assert_eq!(store.selections().unwrap(), std::slice::from_ref(selection));
        let variant = super::super::distros::DISTROS[0].fresh(true);
        let commands =
            launch::create_commands(&places, "Fixture commands", vec![variant.clone()]).unwrap();
        assert!(launch::can_delete_commands(&places, &commands.id));
        assert_eq!(launch::get(&places, &commands.id).unwrap(), commands);
        launch::save_variants(
            &places,
            &launch::VariantsRequest {
                id: commands.id.clone(),
                variants: vec![variant],
            },
        )
        .unwrap();
        launch::delete_commands(&places, &commands.id).unwrap();
        assert!(launch::get(&places, &commands.id).is_err());
        assert!(launch::can_delete_commands(&places, "actualizar-sistema"));
        launch::delete_commands(&places, "actualizar-sistema").unwrap();
        assert!(launch::get(&places, "actualizar-sistema").is_err());
    }
}

pub fn estimate(draft: &Draft, options: &Options, library: &[arsenal::store::Summary]) -> u64 {
    estimate_with(&draft.claude, options, library, |id| {
        draft.items.iter().any(|selected| selected.capability == id)
    })
}

pub fn estimate_ceiling(options: &Options, library: &[arsenal::store::Summary]) -> u64 {
    estimate_with(&HomeSpec::default(), options, library, |_| true)
}

fn estimate_with(
    spec: &HomeSpec,
    options: &Options,
    library: &[arsenal::store::Summary],
    includes_skill: impl Fn(&str) -> bool,
) -> u64 {
    let sized = |prefix: &str, entries: &[String], off: &[String]| -> u64 {
        entries
            .iter()
            .filter(|name| !off.contains(name))
            .map(|name| {
                options
                    .sizes
                    .get(&format!("{prefix}:{name}"))
                    .copied()
                    .unwrap_or(0)
                    .div_ceil(4)
            })
            .sum()
    };
    let instructions = if spec.instructions {
        options
            .sizes
            .get("instructions")
            .copied()
            .unwrap_or(0)
            .div_ceil(4)
            + sized("rule", &options.rules, &spec.rules_off)
    } else {
        0
    };
    let listings = library
        .iter()
        .filter(|item| item.provider == CLAUDE)
        .filter(|item| match item.kind.as_str() {
            "agent" => spec.agents,
            "skill" => includes_skill(&item.id),
            _ => false,
        })
        .map(listing_tokens)
        .sum::<u64>();
    14_002
        + instructions
        + listings
        + sized("account", &options.account_skills, &spec.account_skills_off)
        + sized("plugin", &options.plugins, &spec.plugins_off)
}

pub fn k_tokens(tokens: u64) -> String {
    if (1..1000).contains(&tokens) {
        "<1k".into()
    } else {
        format!("{}k", tokens / 1000 + u64::from(tokens % 1000 >= 500))
    }
}

pub fn mcp_counts(spec: &HomeSpec, options: &Options) -> (usize, usize) {
    (
        options
            .mcp
            .iter()
            .filter(|server| spec.mcp_enabled(server))
            .count()
            + usize::from(spec.mcp.is_none()),
        options.mcp.len() + 1,
    )
}

pub struct Skills {
    pub library: Vec<arsenal::store::Summary>,
    pub reasons: BTreeMap<String, String>,
}

type SkillEntry<'a> = (&'a str, &'a str, bool);

pub struct SkillCategory<'a> {
    pub label: &'static str,
    pub open: bool,
    pub entries: Vec<SkillEntry<'a>>,
}

pub struct SkillGroup<'a> {
    pub label: &'static str,
    pub open: bool,
    pub categories: Vec<SkillCategory<'a>>,
}

pub fn skill_category_rank(category: &str) -> usize {
    [
        "Research",
        "Decisions",
        "Planning and method",
        "Building",
        "Design and interface",
        "Animation and video",
        "Content and SEO",
        "Docs and writing",
        "Review and testing",
        "Fixing bugs",
        "Security",
        "System and setup",
        "Study and knowledge",
        "Other",
        "Claude account",
    ]
    .iter()
    .position(|name| *name == category)
    .unwrap_or(usize::MAX)
}

pub fn ordered_skill_groups<T>(groups: BTreeMap<&str, T>) -> Vec<(&str, T)> {
    let mut groups = groups.into_iter().collect::<Vec<_>>();
    groups.sort_by_key(|(group, _)| skill_group_rank(group));
    groups
}

fn skill_group_rank(group: &str) -> usize {
    [
        crate::t!("General"),
        crate::t!("By type of work"),
        crate::t!("Environment"),
        crate::t!("Own topics"),
    ]
    .iter()
    .position(|section| *section == group)
    .unwrap_or(usize::MAX)
}

impl Skills {
    pub fn groups<'a>(
        &'a self,
        provider: &str,
        account_skills: &'a [String],
        needle: &str,
    ) -> Vec<SkillGroup<'a>> {
        let mut groups = BTreeMap::<_, BTreeMap<_, Vec<SkillEntry<'a>>>>::new();
        for item in self.library.iter().filter(|item| {
            item.provider == provider
                && item.kind == "skill"
                && item.name.to_lowercase().contains(needle)
        }) {
            let (section, category) = super::categories::category(item);
            groups
                .entry(section)
                .or_default()
                .entry(category)
                .or_default()
                .push((&item.name, &item.id, false));
        }
        if provider == CLAUDE {
            for name in account_skills
                .iter()
                .filter(|name| name.to_lowercase().contains(needle))
            {
                groups
                    .entry(crate::t!("Environment"))
                    .or_default()
                    .entry(crate::t!("Claude account"))
                    .or_default()
                    .push((name, name, true));
            }
        }
        let searching = !needle.is_empty();
        ordered_skill_groups(groups)
            .into_iter()
            .map(|(label, categories)| {
                let general = label == crate::t!("General");
                let mut categories = categories.into_iter().collect::<Vec<_>>();
                categories.sort_by_key(|(category, _)| skill_category_rank(category));
                SkillGroup {
                    label,
                    open: general || searching,
                    categories: categories
                        .into_iter()
                        .enumerate()
                        .map(|(index, (label, mut entries))| {
                            entries.sort_by_key(|(name, _, _)| name.to_lowercase());
                            SkillCategory {
                                label,
                                open: searching || (general && index == 0),
                                entries,
                            }
                        })
                        .collect(),
                }
            })
            .collect()
    }

    pub fn load(draft: &Draft, store: Option<&Store>, library: &[arsenal::store::Summary]) -> Self {
        let plan = store.and_then(|store| {
            arsenal::library::installed_items(
                store,
                &draft.agent,
                Path::new(&draft.directory),
                &draft.items,
                library,
            )
        });
        let reasons = match plan {
            Some(items) => items
                .into_iter()
                .filter(|item| item.state != "prepared")
                .map(|item| (item.capability, item.reason))
                .collect(),
            None => library
                .iter()
                .filter(|item| item.provider == draft.agent && item.kind == "skill")
                .map(|item| {
                    (
                        item.id.clone(),
                        crate::t!("Could not check what can load.").into(),
                    )
                })
                .collect(),
        };
        Self {
            library: library.to_vec(),
            reasons,
        }
    }

    pub fn active(&self, draft: &Draft, id: &str) -> bool {
        !self.reasons.contains_key(id) && draft.items.iter().any(|item| item.capability == id)
    }

    pub fn estimate(&self, draft: &Draft, options: &Options) -> u64 {
        estimate_with(&draft.claude, options, &self.library, |id| {
            self.active(draft, id)
        })
    }

    pub fn listings(&self, draft: &Draft) -> u64 {
        self.library
            .iter()
            .filter(|item| {
                item.provider == CLAUDE && item.kind == "skill" && self.active(draft, &item.id)
            })
            .map(listing_tokens)
            .sum()
    }
}

fn listing_tokens(item: &arsenal::store::Summary) -> u64 {
    (item.name.len() as u64 + item.description.len() as u64 + 24).div_ceil(4)
}

pub fn counts(
    draft: &Draft,
    options: &Options,
    codex: &CodexOptions,
    skills: &Skills,
    tab: &str,
) -> (usize, usize) {
    match tab {
        "skills" => {
            let library = skills
                .library
                .iter()
                .filter(|item| item.provider == draft.agent && item.kind == "skill");
            let active = library
                .clone()
                .filter(|item| skills.active(draft, &item.id))
                .count();
            let total = library.count();
            if draft.agent == CLAUDE {
                let account = &options.account_skills;
                (
                    active
                        + account
                            .iter()
                            .filter(|name| !draft.claude.account_skills_off.contains(name))
                            .count(),
                    total + account.len(),
                )
            } else {
                (active, total)
            }
        }
        "mcp" => mcp_counts(&draft.claude, options),
        "hooks" => {
            let (entries, fixed) = if draft.agent == CLAUDE {
                (&options.hooks, 0)
            } else {
                (
                    &codex.hooks,
                    hooks(&fixed_hooks(codex))
                        .iter()
                        .map(|(_, rows)| rows.len())
                        .sum(),
                )
            };
            let (on, total) = hook_counts(draft, &hooks(entries));
            (on + fixed, total + fixed)
        }
        _ => (0, 0),
    }
}

#[cfg(test)]
mod estimate_tests {
    use super::*;

    #[test]
    fn skill_groups_follow_workspace_order_and_open_general_with_its_first_category() {
        let library = [
            (CLAUDE, "skill", "frontend-design"),
            (CLAUDE, "skill", "investigate"),
            (CLAUDE, "skill", "firecrawl-scrape"),
            (CODEX, "skill", "typescript"),
            (CLAUDE, "agent", "graphify"),
        ]
        .into_iter()
        .map(|(provider, kind, name)| arsenal::store::Summary {
            id: name.into(),
            provider: provider.into(),
            source: String::new(),
            kind: kind.into(),
            name: name.into(),
            description: String::new(),
            origin: String::new(),
            revision: 1,
            revisions: 1,
            tree_hash: String::new(),
            updated_at: 0,
        })
        .collect();
        let skills = Skills {
            library,
            reasons: BTreeMap::new(),
        };

        let groups = skills.groups(CLAUDE, &[], "");
        assert_eq!(
            groups
                .iter()
                .map(|group| (group.label, group.open))
                .collect::<Vec<_>>(),
            [("General", true), ("By type of work", false)]
        );
        assert_eq!(
            groups[0]
                .categories
                .iter()
                .map(|category| (category.label, category.open))
                .collect::<Vec<_>>(),
            [("Research", true), ("Fixing bugs", false)]
        );
        assert_eq!(groups[0].categories[0].entries.len(), 1);
        assert_eq!(groups[1].categories[0].entries.len(), 1);

        let work = skills.groups(CLAUDE, &[], "frontend");
        assert_eq!(work.len(), 1);
        assert!(work[0].open && work[0].categories[0].open);

        let general = skills.groups(CLAUDE, &[], "investigate");
        assert_eq!(general[0].categories[0].label, "Fixing bugs");
        assert!(general[0].categories[0].open);
    }

    #[test]
    fn formats_tokens_like_workspace() {
        assert_eq!(k_tokens(0), "0k");
        assert_eq!(k_tokens(999), "<1k");
        assert_eq!(k_tokens(34_500), "35k");
    }
}
