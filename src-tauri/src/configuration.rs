//! The two window commands of the inventory (F1-09). Both are reads of what the service
//! already holds: neither reads a file, takes a path, or starts anything, so a window that
//! asks as often as it likes costs the providers' files nothing.
use crate::agentconfig::author::HookSpec;
use crate::agentconfig::changes::Authoring;
use crate::agentconfig::changes::{Applied, ChangeFailure, ChangeService, PlanView};
use crate::agentconfig::opencode_plan::{McpSpec, mcp_name_ok};
use crate::agentconfig::{
    Bounds, Change, Changes, Environment, Faults, Inventory, InventoryService, Operation,
    SecretToolStore, Summary, Vault,
};
use crate::native_dialog::{Field, GtkConfirm, choose_file, passphrase, text_form};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};
use tauri::AppHandle;

/// The service as the desktop holds it. `None` when there is no home to read from, which
/// the window is told rather than shown an empty inventory (an empty one would say the
/// user has no configuration).
pub struct Configuration {
    inventory: Option<InventoryService>,
    /// Changes: present when the private state directory could be made safely. Without it
    /// the inventory still works and every change command says it is unavailable.
    changes: Option<Arc<ChangeService>>,
    app: Arc<OnceLock<AppHandle>>,
}

/// Where the Console keeps what it owns about configuration changes (14 §3).
fn state_directory() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".local/state"))
        })?;
    Some(base.join("wyss-console").join("configuration"))
}

impl Configuration {
    pub fn start() -> Self {
        let app = Arc::new(OnceLock::new());
        let mut environment = Environment::from_process(None);
        // The accounts the owner named as allowed to write a provider's directory (file only
        // he can write, under the state directory; see `agentconfig::declared`).
        if let (Some(environment), Some(state)) = (environment.as_mut(), state_directory()) {
            environment.declared_writers =
                crate::agentconfig::declared::load(state.parent().unwrap_or(&state));
        }
        let inventory = environment
            .clone()
            .and_then(|environment| InventoryService::start(environment, Bounds::PROPOSED));
        let changes = environment
            .zip(inventory.clone())
            .zip(state_directory())
            .and_then(|((environment, inventory), state)| {
                let changes = ChangeService::for_providers(
                    environment,
                    inventory,
                    &state,
                    Vault::new(Box::new(SecretToolStore::new())),
                    Box::new(GtkConfirm::new(app.clone())),
                    Faults::default(),
                )
                .ok()?;
                // What a crash left is finished before anything new is planned.
                changes.recover();
                Some(Arc::new(changes))
            });
        Self {
            inventory,
            changes,
            app,
        }
    }

    fn service(&self) -> Result<&InventoryService, ConfigurationError> {
        self.inventory
            .as_ref()
            .ok_or(ConfigurationError::Unavailable)
    }

    fn changes(&self, app: &AppHandle) -> Result<Arc<ChangeService>, ConfigurationError> {
        let _ = self.app.set(app.clone());
        self.changes.clone().ok_or(ConfigurationError::Unavailable)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigurationError {
    Unavailable,
    /// The request is not one the desktop accepts; it names something it does not know.
    Invalid,
    /// The change commands answer with the reason, a closed word.
    Change(ChangeFailure),
}

impl From<ChangeFailure> for ConfigurationError {
    fn from(failure: ChangeFailure) -> Self {
        Self::Change(failure)
    }
}

/// The whole inventory at a revision.
#[derive(Debug, Serialize)]
pub struct InventoryReply {
    pub revision: u64,
    pub inventory: Inventory,
}

pub fn inventory_reply(
    configuration: &Configuration,
) -> Result<InventoryReply, ConfigurationError> {
    let snapshot = configuration.service()?.snapshot();
    Ok(InventoryReply {
        revision: snapshot.revision,
        inventory: (*snapshot.inventory).clone(),
    })
}

pub fn changes_reply(
    configuration: &Configuration,
    since: u64,
) -> Result<Changes, ConfigurationError> {
    Ok(configuration.service()?.changes_since(since))
}

#[tauri::command]
fn configuration_inventory(
    configuration: tauri::State<'_, Configuration>,
) -> Result<InventoryReply, ConfigurationError> {
    inventory_reply(&configuration)
}

#[tauri::command]
fn configuration_changes(
    configuration: tauri::State<'_, Configuration>,
    since: u64,
) -> Result<Changes, ConfigurationError> {
    changes_reply(&configuration, since)
}

// ---- changing (F2a) ---------------------------------------------------------------------
//
// Every request names something the desktop already holds: an item, a record, a plan, a
// change, a source's closed word. None carries a path, text of an entry, or a command.

/// Identifiers are lowercase hex; a source's word is lowercase letters and underscores.
fn well_formed(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanRequest {
    operation: Operation,
    target: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanId {
    plan: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeId {
    change: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordId {
    record: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryRequest {
    cursor: Option<String>,
    limit: u32,
}

#[derive(Debug, Serialize)]
pub struct HistoryReply {
    changes: Vec<Change>,
    next: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct Done {
    done: bool,
}

fn checked(text: &str) -> Result<&str, ConfigurationError> {
    well_formed(text)
        .then_some(text)
        .ok_or(ConfigurationError::Invalid)
}

/// Runs `work` off the main thread: an apply waits for a dialog that the main thread draws.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ConfigurationError> + Send + 'static,
) -> Result<T, ConfigurationError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| ConfigurationError::Change(ChangeFailure::Internal))?
}

#[tauri::command]
async fn configuration_plan(
    app: AppHandle,
    configuration: tauri::State<'_, Configuration>,
    request: PlanRequest,
) -> Result<PlanView, ConfigurationError> {
    let changes = configuration.changes(&app)?;
    blocking(move || {
        let target = checked(&request.target)?.to_owned();
        Ok(changes.plan(request.operation, &target)?)
    })
    .await
}

#[tauri::command]
async fn configuration_apply(
    app: AppHandle,
    configuration: tauri::State<'_, Configuration>,
    request: PlanId,
) -> Result<Applied, ConfigurationError> {
    let changes = configuration.changes(&app)?;
    blocking(move || Ok(changes.apply(checked(&request.plan)?)?)).await
}

#[tauri::command]
async fn configuration_history(
    app: AppHandle,
    configuration: tauri::State<'_, Configuration>,
    request: HistoryRequest,
) -> Result<HistoryReply, ConfigurationError> {
    let changes = configuration.changes(&app)?;
    blocking(move || {
        let cursor = request.cursor.as_deref().map(checked).transpose()?;
        let (changes, next) = changes.history(cursor, request.limit.min(50) as usize);
        Ok(HistoryReply { changes, next })
    })
    .await
}

#[tauri::command]
async fn configuration_undo(
    app: AppHandle,
    configuration: tauri::State<'_, Configuration>,
    request: ChangeId,
) -> Result<PlanView, ConfigurationError> {
    let changes = configuration.changes(&app)?;
    blocking(move || Ok(changes.undo(checked(&request.change)?)?)).await
}

// ---- authoring (F5) ------------------------------------------------------------------------
//
// The page names what kind of thing the user wants to write and where (an event, a name), and
// nothing else. The text that will run is typed in a window the desktop draws, which says what
// it will become, and only what that window returns reaches the planner. A plan made from it
// still asks again, natively, before anything is written.

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorHook {
    event: String,
    matcher: Option<String>,
    timeout: Option<u32>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemId {
    item: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorMcp {
    name: String,
    /// `local` or `remote`.
    kind: String,
    /// `opencode` or `codex`: whose configuration the server goes into.
    agent: String,
}

fn cancelled() -> ConfigurationError {
    ConfigurationError::Change(ChangeFailure::Cancelled)
}
fn invalid() -> ConfigurationError {
    ConfigurationError::Change(ChangeFailure::InvalidEntry)
}

/// A program and its arguments as the window shows them: the program, then one argument per
/// line. Nothing is split on spaces and nothing goes through a shell.
fn mcp_fields(current: Option<&McpSpec>, kind_remote: bool) -> Vec<Field> {
    match (current, kind_remote) {
        (Some(McpSpec::Local { command }), _) => vec![
            Field {
                label: "Program".to_owned(),
                initial: command.first().cloned().unwrap_or_default(),
                multiline: false,
            },
            Field {
                label: "Arguments, one per line".to_owned(),
                initial: command
                    .iter()
                    .skip(1)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n"),
                multiline: true,
            },
        ],
        (Some(McpSpec::Remote { url }), _) => vec![Field {
            label: "Address (https://…)".to_owned(),
            initial: url.clone(),
            multiline: false,
        }],
        (None, true) => vec![Field {
            label: "Address (https://…)".to_owned(),
            initial: String::new(),
            multiline: false,
        }],
        (None, false) => vec![
            Field {
                label: "Program".to_owned(),
                initial: String::new(),
                multiline: false,
            },
            Field {
                label: "Arguments, one per line".to_owned(),
                initial: String::new(),
                multiline: true,
            },
        ],
    }
}

fn mcp_from(typed: &[String], remote: bool) -> McpSpec {
    if remote {
        McpSpec::Remote {
            url: typed.first().cloned().unwrap_or_default().trim().to_owned(),
        }
    } else {
        let mut command = vec![typed.first().cloned().unwrap_or_default().trim().to_owned()];
        command.extend(
            typed
                .get(1)
                .map(|a| {
                    a.lines()
                        .map(str::to_owned)
                        .filter(|l| !l.is_empty())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        );
        McpSpec::Local { command }
    }
}

/// Instruction text never crosses the webview boundary, in either direction.
#[tauri::command]
async fn configuration_edit_document(
    app: AppHandle,
    configuration: tauri::State<'_, Configuration>,
    request: ItemId,
) -> Result<PlanView, ConfigurationError> {
    let changes = configuration.changes(&app)?;
    blocking(move || {
        let item = checked(&request.item)?.to_owned();
        let (label, expected) = changes.document_current(&item)?;
        let typed = text_form(
            &app,
            "Change an instruction file",
            &format!("File: {label} in your account. These instructions can change an agent's behaviour. Review the full text here. The previous bytes are kept encrypted for exact undo; a later outside edit is never overwritten."),
            vec![Field { label: "Instructions (Markdown, at most 64 KiB)".to_owned(), initial: expected.clone(), multiline: true }],
        ).ok_or_else(cancelled)?;
        Ok(changes.plan_authored(Authoring::EditDocument { item, text: typed[0].clone(), expected })?)
    }).await
}

#[tauri::command]
async fn configuration_author_hook(
    app: AppHandle,
    configuration: tauri::State<'_, Configuration>,
    request: AuthorHook,
) -> Result<PlanView, ConfigurationError> {
    let changes = configuration.changes(&app)?;
    blocking(move || {
        let mut spec = HookSpec {
            event: request.event,
            matcher: request.matcher,
            command: "x".to_owned(),
            timeout: request.timeout,
        };
        // What the page named is judged before a window is drawn for it.
        spec.check().map_err(|_| invalid())?;
        let where_ = format!(
            "This command will run on every {} event{} of every Claude Code session you start. \
             It is written into your own Claude Code settings.",
            spec.event,
            spec.matcher
                .as_deref()
                .map_or(String::new(), |m| format!(" for \"{m}\""))
        );
        let typed = text_form(
            &app,
            "Add a hook",
            &where_,
            vec![Field {
                label: "Command".to_owned(),
                initial: String::new(),
                multiline: false,
            }],
        )
        .ok_or_else(cancelled)?;
        spec.command = typed[0].trim().to_owned();
        Ok(changes.plan_authored(Authoring::AddHook(spec))?)
    })
    .await
}

#[tauri::command]
async fn configuration_edit_hook(
    app: AppHandle,
    configuration: tauri::State<'_, Configuration>,
    request: ItemId,
) -> Result<PlanView, ConfigurationError> {
    let changes = configuration.changes(&app)?;
    blocking(move || {
        let item = checked(&request.item)?.to_owned();
        let current = changes
            .hook_command(&item)
            .ok_or(ConfigurationError::Change(ChangeFailure::EntryGone))?;
        // The window names which hook it is: the backend's own name for it, not the page's.
        let which = changes.item_label(&item).unwrap_or_default();
        let typed = text_form(
            &app,
            "Change a hook",
            &format!(
                "This is the command that runs for the hook \"{which}\". What you write replaces it in your own Claude Code settings."
            ),
            vec![Field {
                label: "Command".to_owned(),
                initial: current.clone(),
                multiline: false,
            }],
        )
        .ok_or_else(cancelled)?;
        Ok(changes.plan_authored(Authoring::EditHook {
            item,
            command: typed[0].trim().to_owned(),
            expected: current,
        })?)
    })
    .await
}

#[tauri::command]
async fn configuration_author_mcp(
    app: AppHandle,
    configuration: tauri::State<'_, Configuration>,
    request: AuthorMcp,
) -> Result<PlanView, ConfigurationError> {
    let changes = configuration.changes(&app)?;
    blocking(move || {
        if !mcp_name_ok(&request.name) {
            return Err(invalid());
        }
        let remote = match request.kind.as_str() {
            "local" => false,
            "remote" => true,
            _ => return Err(invalid()),
        };
        let codex = match request.agent.as_str() {
            "codex" => true,
            "opencode" => false,
            _ => return Err(invalid()),
        };
        let intro = format!(
            "{} will {} for the server \"{}\". It is written into your own {} configuration.",
            if codex { "Codex" } else { "OpenCode" },
            if remote {
                "connect to the address you write"
            } else {
                "start the program you write"
            },
            request.name,
            if codex { "Codex" } else { "OpenCode" }
        );
        let typed = text_form(&app, "Add an MCP server", &intro, mcp_fields(None, remote))
            .ok_or_else(cancelled)?;
        let spec = mcp_from(&typed, remote);
        Ok(changes.plan_authored(if codex {
            Authoring::AddCodexMcp {
                name: request.name,
                spec,
            }
        } else {
            Authoring::AddMcp {
                name: request.name,
                spec,
            }
        })?)
    })
    .await
}

#[tauri::command]
async fn configuration_edit_mcp(
    app: AppHandle,
    configuration: tauri::State<'_, Configuration>,
    request: ItemId,
) -> Result<PlanView, ConfigurationError> {
    let changes = configuration.changes(&app)?;
    blocking(move || {
        let item = checked(&request.item)?.to_owned();
        let current = changes
            .mcp_current(&item)
            .ok_or(ConfigurationError::Change(ChangeFailure::EntryGone))?;
        let remote = matches!(current, McpSpec::Remote { .. });
        let which = changes.item_label(&item).unwrap_or_default();
        let codex = changes.item_provider(&item) == Some(crate::agentconfig::ProviderId::Codex);
        let agent = if codex { "Codex" } else { "OpenCode" };
        let typed = text_form(
            &app,
            "Change an MCP server",
            &format!(
                "What you write replaces what the {agent} server \"{which}\" starts or connects to."
            ),
            mcp_fields(Some(&current), remote),
        )
        .ok_or_else(cancelled)?;
        let spec = mcp_from(&typed, remote);
        Ok(changes.plan_authored(if codex {
            Authoring::EditCodexMcp {
                item,
                spec,
                expected: current,
            }
        } else {
            Authoring::EditMcp {
                item,
                spec,
                expected: current,
            }
        })?)
    })
    .await
}

#[tauri::command]
async fn configuration_records(
    app: AppHandle,
    configuration: tauri::State<'_, Configuration>,
) -> Result<Vec<Summary>, ConfigurationError> {
    let changes = configuration.changes(&app)?;
    blocking(move || Ok(changes.records())).await
}

#[tauri::command]
async fn configuration_record_delete(
    app: AppHandle,
    configuration: tauri::State<'_, Configuration>,
    request: RecordId,
) -> Result<Done, ConfigurationError> {
    let changes = configuration.changes(&app)?;
    blocking(move || {
        changes.delete_record(checked(&request.record)?)?;
        Ok(Done { done: true })
    })
    .await
}

/// A record written to a file the user chooses, sealed under a passphrase typed in a native
/// window. Cancelling anywhere writes nothing.
#[tauri::command]
async fn configuration_export(
    app: AppHandle,
    configuration: tauri::State<'_, Configuration>,
    request: RecordId,
) -> Result<Done, ConfigurationError> {
    let changes = configuration.changes(&app)?;
    blocking(move || {
        let record = checked(&request.record)?.to_owned();
        let Some(secret) = passphrase(&app, true) else {
            return Ok(Done { done: false });
        };
        let bytes = changes.export_record(&record, &secret)?;
        let Some(path) = choose_file(&app, true, "wyss-restoration.export") else {
            return Ok(Done { done: false });
        };
        write_private_file(&path, &bytes)
            .map_err(|()| ConfigurationError::Change(ChangeFailure::Internal))?;
        Ok(Done { done: true })
    })
    .await
}

#[tauri::command]
async fn configuration_import(
    app: AppHandle,
    configuration: tauri::State<'_, Configuration>,
) -> Result<Done, ConfigurationError> {
    let changes = configuration.changes(&app)?;
    blocking(move || {
        let Some(path) = choose_file(&app, false, "") else {
            return Ok(Done { done: false });
        };
        // A record is a few kilobytes; a file far larger than that is not one.
        let bytes = read_small_file(&path)
            .map_err(|()| ConfigurationError::Change(ChangeFailure::ImportInvalid))?;
        let Some(secret) = passphrase(&app, false) else {
            return Ok(Done { done: false });
        };
        changes.import_record(&bytes, &secret)?;
        Ok(Done { done: true })
    })
    .await
}

/// A file the user named in the native chooser, made 0600 and not followed through a link.
pub(crate) fn write_private_file(path: &std::path::Path, bytes: &[u8]) -> Result<(), ()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| ())?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| ())
}

pub(crate) fn read_small_file(path: &std::path::Path) -> Result<Vec<u8>, ()> {
    use std::{io::Read, os::unix::fs::OpenOptionsExt};
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| ())?;
    let mut bytes = Vec::new();
    file.take(512 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    (bytes.len() <= 512 * 1024).then_some(bytes).ok_or(())
}

const COMMANDS: &[&str] = &[
    "configuration_inventory",
    "configuration_changes",
    "configuration_plan",
    "configuration_apply",
    "configuration_history",
    "configuration_undo",
    "configuration_records",
    "configuration_record_delete",
    "configuration_export",
    "configuration_import",
    "configuration_author_hook",
    "configuration_edit_hook",
    "configuration_author_mcp",
    "configuration_edit_mcp",
    "configuration_edit_document",
];

/// What the desktop binary hands to the library: the commands, the state they read, and
/// nothing else.
pub fn extension() -> wyss_console::Extension {
    wyss_console::Extension {
        commands: COMMANDS,
        handler: Box::new(tauri::generate_handler![
            configuration_inventory,
            configuration_changes,
            configuration_plan,
            configuration_apply,
            configuration_history,
            configuration_undo,
            configuration_records,
            configuration_record_delete,
            configuration_export,
            configuration_import,
            configuration_author_hook,
            configuration_edit_hook,
            configuration_author_mcp,
            configuration_edit_mcp,
            configuration_edit_document
        ]),
        install: Box::new(|builder| builder.manage(Configuration::start())),
        on_exit: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::DirBuilderExt};

    fn configuration_over(files: &[(&str, &str)]) -> (Configuration, std::path::PathBuf) {
        let base = fs::canonicalize(std::env::temp_dir()).unwrap();
        let home = base.join(format!("wyss-cmd-{}", uuid::Uuid::new_v4().simple()));
        fs::DirBuilder::new().mode(0o700).create(&home).unwrap();
        for (relative, text) in files {
            let path = home.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let environment = Environment {
            home: home.clone(),
            claude_config_dir: None,
            codex_home: None,
            project: None,
            system: home.join("etc"),
            declared_writers: Vec::new(),
        };
        let service = InventoryService::start(environment, Bounds::PROPOSED).unwrap();
        (
            Configuration {
                inventory: Some(service),
                changes: None,
                app: Arc::new(OnceLock::new()),
            },
            home,
        )
    }

    #[test]
    fn the_inventory_reply_has_the_shape_the_window_is_written_against() {
        let (configuration, home) = configuration_over(&[(
            ".claude/settings.json",
            r#"{ "model": "opus", "env": { "K": "SEKRET" } }"#,
        )]);
        let reply = serde_json::to_value(inventory_reply(&configuration).unwrap()).unwrap();
        assert_eq!(reply["revision"], 1);
        assert_eq!(reply["inventory"]["truncated"], false);
        let item = &reply["inventory"]["items"][0];
        assert_eq!(item["provider"], "claude_code");
        assert_eq!(item["kind"], "model_preference");
        assert_eq!(item["persisted"], "present_enabled");
        assert_eq!(item["effective"], "documented");
        assert_eq!(item["session_use"], "unknown");
        assert_eq!(item["not_writable"], "not_in_this_phase");
        let source = reply["inventory"]["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["key"] == "claude_user_settings")
            .unwrap();
        assert_eq!(source["outcome"], "read");
        assert_eq!(source["unrecognised"], 0);
        assert!(source["hash"].as_str().unwrap().len() == 64);
        assert!(!reply.to_string().contains("SEKRET"));
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn the_changes_reply_names_its_result() {
        let (configuration, home) = configuration_over(&[]);
        assert_eq!(
            serde_json::to_value(changes_reply(&configuration, 1).unwrap()).unwrap(),
            serde_json::json!({ "result": "same", "revision": 1 })
        );
        assert_eq!(
            serde_json::to_value(changes_reply(&configuration, 9).unwrap()).unwrap(),
            serde_json::json!({ "result": "resync", "revision": 1 })
        );
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn without_a_service_both_commands_say_unavailable_and_serialize_so() {
        let none = Configuration {
            inventory: None,
            changes: None,
            app: Arc::new(OnceLock::new()),
        };
        assert_eq!(
            inventory_reply(&none).unwrap_err(),
            ConfigurationError::Unavailable
        );
        assert_eq!(
            changes_reply(&none, 0).unwrap_err(),
            ConfigurationError::Unavailable
        );
        assert_eq!(
            serde_json::to_value(ConfigurationError::Unavailable).unwrap(),
            serde_json::json!("unavailable")
        );
    }

    #[test]
    fn the_commands_the_extension_routes_are_the_ones_the_manifest_and_capability_name() {
        let routed = extension().commands;
        let build = include_str!("../build.rs");
        let capability = include_str!("../capabilities/main.json");
        for command in routed {
            assert!(
                build.contains(&format!("\"{command}\"")),
                "{command} not in build.rs"
            );
            let permission = format!("allow-{}", command.replace('_', "-"));
            assert!(
                capability.contains(&permission),
                "{permission} not in the capability"
            );
        }
    }

    /// The contract the window is written against, as one file both sides read:
    /// `tests/fixtures/configuration-inventory-v1.json`. Set `WYSS_UPDATE_FIXTURES=1` to
    /// write it again after a deliberate change, then run Prettier on it.
    #[test]
    fn the_inventory_reply_is_exactly_the_shared_fixture() {
        let (configuration, home) = configuration_over(&[
            (
                ".claude/settings.json",
                r#"{ "model": "opus",
                    "skillOverrides": { "noisy": "off" },
                    "hooks": { "PreToolUse": [ { "matcher": "Bash", "hooks": [
                      { "type": "command", "command": "guard.sh --token SEKRET", "timeout": 5 } ] } ] } }"#,
            ),
            (".claude/skills/useful/SKILL.md", "# useful"),
            (".claude/skills/noisy/SKILL.md", "# noisy"),
            (".claude/CLAUDE.md", "rules"),
            (
                ".claude.json",
                r#"{ "mcpServers": { "context7": { "command": "npx", "args": ["-y", "ctx"],
                     "env": { "CONTEXT7_API_KEY": "SEKRET" } } } }"#,
            ),
            (
                ".codex/config.toml",
                "model = \"gpt-6\"\n[mcp_servers.off]\ncommand = \"x\"\nenabled = false\n",
            ),
            (".codex/hooks.json", "{ not json"),
        ]);
        // A provider directory a group can write: read, and never writable.
        // A provider directory a group with others in it can write (root's group, which is
        // never private): read, and never writable. An ACL, so that the answer does not
        // depend on which groups this machine has.
        use crate::agentconfig::{
            ACL_GROUP, ACL_GROUP_OBJ, ACL_MASK, ACL_OTHER, ACL_USER_OBJ, set_acl_for_test,
        };
        assert!(set_acl_for_test(
            &home.join(".codex"),
            &[
                (ACL_USER_OBJ, 7, 0),
                (ACL_GROUP_OBJ, 5, 0),
                (ACL_GROUP, 7, 0),
                (ACL_MASK, 7, 0),
                (ACL_OTHER, 5, 0),
            ]
        ));
        configuration
            .service()
            .unwrap()
            .select_project(Some(home.clone()));
        let reply = inventory_reply(&configuration).unwrap();
        let mut value = serde_json::to_value(&reply).unwrap();
        // The revision depends on how many readings the setup caused; the fixture pins
        // the shape, not that count.
        value["revision"] = serde_json::json!(1);
        let text = serde_json::to_string_pretty(&value).unwrap() + "\n";
        assert!(!text.contains("SEKRET") && !text.contains(home.to_str().unwrap()));
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/configuration-inventory-v1.json");
        if std::env::var_os("WYSS_UPDATE_FIXTURES").is_some() {
            fs::write(&path, &text).unwrap();
        }
        // Compared as values: the file is formatted by Prettier, not by this test.
        let expected: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            value, expected,
            "the contract changed: update the fixture on purpose"
        );
        let _ = fs::remove_dir_all(home);
    }

    /// TS-CFG-15: the desktop's prediction of what a Codex configuration does to managed
    /// Inference equals what the authority's own checks decide on the same definitions.
    #[test]
    fn the_managed_inference_prediction_equals_the_authoritys_rule() {
        use crate::agentconfig::codex_plan::{Facts, Verdict};
        use wyss_console::adapters::codex::managed_inference_blocker;
        let names = |n: usize| (0..n).map(|i| format!("s{i}")).collect::<Vec<_>>();
        let mut cases: Vec<(Vec<Vec<bool>>, Vec<String>)> = vec![
            (vec![], vec![]),
            (vec![vec![]], vec![]),
            (vec![vec![false]], names(3)),
            (vec![vec![true]], vec![]),
            (vec![vec![false, true]], vec![]),
            (vec![vec![false], vec![false]], names(64)),
            (vec![vec![false]], names(65)),
            (vec![vec![false; 256]], vec![]),
            (vec![vec![false; 257]], vec![]),
            (vec![vec![false]; 64], vec![]),
            (vec![vec![false]; 65], vec![]),
            (vec![], vec!["ok_name-1".into()]),
            (vec![], vec!["bad name".into()]),
            (vec![], vec!["x".repeat(65)]),
            (vec![], vec!["x".repeat(64)]),
            (vec![], vec![String::new()]),
            (vec![vec![true]], vec!["bad name".into()]),
        ];
        // Every combination of a few hooks and a few names, exhaustively on a small grid.
        for hooks in [vec![], vec![false], vec![true], vec![false, true]] {
            for name in ["", "a", "a-b_c", "a b", "é"] {
                cases.push((vec![hooks.clone()], vec![name.to_owned()]));
            }
        }
        for (sources, servers) in cases {
            let facts = Facts {
                hook_sources: sources.iter().filter(|s| !s.is_empty()).cloned().collect(),
                mcp_names: servers.clone(),
            };
            let authority = managed_inference_blocker(&facts.hook_sources, &servers);
            let predicted = facts.verdict();
            assert_eq!(
                authority.is_none(),
                predicted == Verdict::Available,
                "{sources:?} {servers:?}: authority {authority:?}, predicted {predicted:?}"
            );
            // And the cause agrees, not only the yes or no.
            match predicted {
                Verdict::Available => assert_eq!(authority, None, "{sources:?} {servers:?}"),
                Verdict::ExternalHooks => assert!(
                    matches!(authority, Some("external_hooks_enabled" | "hook_bound")),
                    "{authority:?}"
                ),
                Verdict::McpUnprovable => assert!(
                    matches!(authority, Some("mcp_bound" | "mcp_clamp_unproven")),
                    "{authority:?}"
                ),
            }
        }
    }
}
