//! The Native side of the host protocol (`native_launcher`, `native_preview`, `native_start`,
//! `native_restart`, `native_stop`, `native_forget`, `native_sessions`) and the flow of opening a
//! launch. The parsers are pure
//! and tested against answers shaped like the host's structs (`src/native/service.rs`).

use std::time::Duration;

use serde_json::{json, Value};

use crate::host::{Host, HostError, ListEntry};
use crate::i18n::{tr, trf};
use crate::launches::{self, Kind, Launch};
use crate::t;

/// The first size of a terminal that has not been fitted yet (`FIRST_SIZE` in the Console).
pub const FIRST_SIZE: (u16, u16) = (24, 80);
fn bad(what: &str) -> HostError {
    HostError::Protocol(format!("native: {what}"))
}

/// What marks a terminal as an agent's (`NativeTag`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub session_id: String,
    pub agent_name: String,
    pub project: String,
    /// The name of the profile the host recorded (a template's name, or `None` as configured).
    pub profile: Option<String>,
}

pub fn parse_tag(v: &Value) -> Option<Tag> {
    Some(Tag {
        session_id: v["session_id"].as_str()?.to_owned(),
        agent_name: v["agent_name"].as_str()?.to_owned(),
        project: v["project"].as_str().unwrap_or("").to_owned(),
        profile: v["profile"].as_str().map(str::to_owned),
    })
}

/// `native_launcher`: home, registered project paths and agents found.
#[derive(Debug, PartialEq, Eq)]
pub struct Launcher {
    pub home: Option<String>,
    pub installed: Vec<String>,
    pub projects: Vec<(String, u64)>,
}

pub fn parse_launcher(v: &Value) -> Result<Launcher, HostError> {
    let agents = v["agents"].as_array().ok_or_else(|| bad("launcher has no agents"))?;
    let projects = v["projects"].as_array().into_iter().flatten();
    Ok(Launcher {
        home: projects.clone().find(|project| project["home"] == true)
            .and_then(|project| project["path"].as_str()).map(str::to_owned),
        installed: agents.iter().filter(|a| a["installed"] == true)
            .filter_map(|a| a["agent_id"].as_str().map(str::to_owned)).collect(),
        projects: projects.filter_map(|project| Some((
            project["path"].as_str()?.to_owned(),
            project["last_used"].as_u64().unwrap_or_default(),
        ))).collect(),
    })
}

/// `native_preview`: whether the start may go on, and the token that binds the start to this check.
#[derive(Debug, PartialEq, Eq)]
pub struct Preview {
    pub can_start: bool,
    pub plan_token: Option<String>,
    pub provider_version: Option<String>,
    pub bypass: Option<bool>,
}

pub fn parse_preview(v: &Value) -> Result<Preview, HostError> {
    Ok(Preview {
        can_start: v["can_start"].as_bool().ok_or_else(|| bad("preview has no can_start"))?,
        plan_token: v["plan_token"].as_str().map(str::to_owned),
        provider_version: v["provider_version"].as_str().map(str::to_owned),
        bypass: v["bypass"].as_bool(),
    })
}

/// A session as `native_sessions` lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub agent_name: String,
    /// `requested`, `running`, `exited`, `stopped`, `failed_start` or `lost`.
    pub state: String,
    pub terminal_id: Option<String>,
    pub project: String,
    pub profile: Option<String>,
    /// What a preview of the same start names: the agent and the profile it was started with.
    pub agent_id: Option<String>,
    pub profile_id: Option<String>,
    pub restart_bypass: Option<bool>,
    /// The Arsenal exclusion the host started it with: a preview of its restart sends it again.
    pub arsenal: Option<Value>,
}

pub fn parse_session(v: &Value) -> Option<Session> {
    Some(Session {
        arsenal: v.get("arsenal").filter(|a| a.is_object()).cloned(),
        id: v["id"].as_str()?.to_owned(),
        agent_id: v["agent_id"].as_str().map(str::to_owned),
        profile_id: v["profile"]["id"].as_str().map(str::to_owned),
        restart_bypass: v["restart_bypass"].as_bool(),
        agent_name: v["agent_name"].as_str()?.to_owned(),
        state: v["state"].as_str()?.to_owned(),
        terminal_id: v["terminal_id"].as_str().map(str::to_owned),
        project: v["project"].as_str().unwrap_or("").to_owned(),
        profile: v["profile"]["name"].as_str().map(str::to_owned),
    })
}

pub fn parse_sessions(v: &Value) -> Result<Vec<Session>, HostError> {
    Ok(v.as_array().ok_or_else(|| bad("sessions is not a list"))?.iter().filter_map(parse_session).collect())
}

/// `native_start` and `native_restart`: the new session and the terminal it runs in.
#[derive(Debug, PartialEq, Eq)]
pub struct Started {
    pub session: Session,
    pub terminal_id: String,
    /// Stored skills of the preset the host could not prepare: (name, reason code).
    pub skills_skipped: Vec<(String, String)>,
    pub omitted_options: Vec<String>,
}

pub fn parse_started(v: &Value) -> Result<Started, HostError> {
    let session = parse_session(&v["session"]).ok_or_else(|| bad("reply has no session"))?;
    let terminal_id = v["terminal"]["terminal_id"].as_str().ok_or_else(|| bad("reply has no terminal"))?.to_owned();
    let skills_skipped = v["skills_skipped"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| Some((item["name"].as_str()?.to_owned(), item["reason"].as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    let omitted_options = v["omitted_options"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|option| option.as_str().map(str::to_owned))
        .collect();
    Ok(Started {
        session,
        terminal_id,
        skills_skipped,
        omitted_options,
    })
}

pub fn start_notes(started: &Started) -> Vec<String> {
    let agent = started.session.agent_name.as_str();
    started.omitted_options.iter().map(|option| {
        if option == "bypass_preview_required" {
            return tr(t!("Full access became available after preview. The session opened without it."));
        }
        trf(
            t!("This version of {agent} does not offer {option}. The session opened without it."),
            &[("agent", agent), ("option", option)],
        )
    }).chain(started.skills_skipped.iter().map(|(name, reason)| skipped_skill_note(name, reason))).collect()
}

/// What the owner reads about a stored skill the host could not prepare for the session.
pub fn skipped_skill_note(name: &str, reason: &str) -> String {
    let why = match reason {
        "digest_mismatch" => tr(t!("its stored files do not match their digest")),
        "not_in_library" => tr(t!("the library no longer holds that revision")),
        "not_this_skill" => tr(t!("the stored revision is not that skill")),
        "unsafe_path" => tr(t!("a stored file has a path that is not allowed")),
        "name_taken" => tr(t!("the session folder already has something with that name")),
        "write_failed" => tr(t!("it could not be written to the session folder")),
        "token_missing" => tr(t!("there is no stored sign-in token for a folder of the session's own")),
        "cwd_is_home" => tr(t!("the project folder would load your own configuration again")),
        _ => tr(t!("the Arsenal library could not be read")),
    };
    trf(t!("Could not prepare the skill {name}: {reason}. The agent started without it."), &[("name", name), ("reason", &why)])
}

/// Panel actions by session state: stop or restart a running agent, or start or forget a finished one.
/// Actions are unavailable while the start is pending.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    Running,
    Finished,
}

pub fn lifecycle(state: &str) -> Option<Lifecycle> {
    match state {
        "running" => Some(Lifecycle::Running),
        "requested" => None,
        _ => Some(Lifecycle::Finished),
    }
}

/// `tabStartedWith`: what a tab says its agent was started with, from the name the host recorded.
pub fn started_with(profile: Option<&str>) -> Option<String> {
    match profile {
        None => None,
        Some("As configured") => None,
        Some("Strict") => Some(tr("Ask before acting")),
        Some("Review, read only") => Some(tr("Read only")),
        Some("Accept edits") => Some(tr("Accept edits")),
        Some("Full access (bypass)") => Some(tr("Full access")),
        Some(name) => Some(trf("Profile {name}", &[("name", name)])),
    }
}

/// The label of an agent's session in the status bar: `Native · {agent}` and, when the agent was
/// started with a permission, ` · ` and the permission.
pub fn agent_label(tag: &Tag) -> String {
    let kind = trf("Native · {agent}", &[("agent", &tag.agent_name)]);
    match started_with(tag.profile.as_deref()) {
        Some(p) => format!("{kind} · {p}"),
        None => kind,
    }
}

// ---- failure words -----------------------------------------------------------------------

/// What the window says for a host failure code (the codes a
/// start from a launch can meet); any other code is the general sentence.
pub fn failure_text(code: &str) -> String {
    match code {
        "agent_not_installed" => tr(t!("That agent is not installed on this computer.")),
        "project_invalid" => tr(t!("That folder cannot be used. Choose a folder with the folder button.")),
        "project_unknown" | "project_refused" => tr(t!("This launch’s folder is not a project the Console knows. Choose it once with Browse… in Workspace, then try again.")),
        "profile_not_applicable" => tr(t!("This permission cannot be applied to that agent.")),
        "profile_unknown" => tr(t!("What this session was started with no longer exists, so it cannot start that way. Start a new one from Workspace.")),
        "profile_preview_required" => tr(t!("The start was not checked first. Choose the permission again and start.")),
        "profile_preview_stale" => tr(t!("What the start would do changed after it was checked. Check it again and start.")),
        "restart_needs_preview" => tr(t!("A session with full access is not restarted without seeing it again. Start it again from Workspace.")),
        "provider_version_unverified" => tr(t!("This exact provider build is not verified for permission flags, so nothing was launched with them. Choose “As configured” or use a qualified build.")),
        "brief_invalid" => tr(t!("The brief must start with a “# ” heading, hold no NUL and be at most 16 KiB.")),
        "limit_reached" => tr(t!("The terminal limit (16) is reached. Close one to start another.")),
        "spawn_failed" => tr(t!("The agent could not be started.")),
        "busy" => tr(t!("A start is already in progress.")),
        "session_unknown" => tr(t!("That session is not known any more.")),
        "not_running" => tr(t!("That session is not running.")),
        "still_running" => tr(t!("A running session cannot be removed. Stop it first.")),
        "stop_failed" => tr(t!("The session could not be stopped.")),
        "arsenal_home_cwd_is_home" => tr(t!("A preset cannot run with your home folder (~) as the working directory: the agent would load your own ~/.claude again on top of the preset. Choose a project folder in Workspace → General → Working directory.")),
        "arsenal_token_missing" => tr(t!("This preset needs your Claude Code sign-in, and it was not found in the system keyring. Sign in once with claude in a terminal and try again.")),
        "arsenal_home_failed" => tr(t!("The preset’s own folder for the agent could not be prepared. Nothing was started.")),
        "arsenal_home_unqualified" => tr(t!("This agent build cannot use a preset’s own folder. Start it without the preset.")),
        "arsenal_unqualified" => tr(t!("This agent build cannot apply Arsenal presets. Start it without the preset.")),
        "arsenal_project_glob" => tr(t!("The working directory cannot be used with a preset (it is missing or its name has special characters). Choose another folder in Workspace.")),
        "unknown_terminal" => tr(t!("That terminal is no longer open.")),
        "unknown_op" => tr(t!("The terminal host is from another version. Close Wyss Console and open it again.")),
        "internal" => tr(t!("Something failed inside the terminal host. Nothing was started.")),
        "store_unavailable" => tr(t!("The Console could not write its own state.")),
        "unavailable" => tr(t!("The Console has no private folder to keep native sessions in, so it cannot start agents.")),
        "request_invalid" => tr(t!("The request was not valid.")),
        "host_unavailable" => tr(t!("The terminal host is not answering. Nothing was sent; nothing changed.")),
        "host_outdated" => tr(t!("The terminal host is older than this window and cannot start with this option. Update it before using it; existing sessions are unchanged.")),
        _ => trf(t!("The terminal host answered “{code}”. Nothing was started."), &[("code", code)]),
    }
}

/// A failure as a sentence for the banner.
pub fn explain(e: &HostError) -> String {
    match e {
        HostError::Remote { code, .. } => failure_text(code),
        HostError::Io(_) => failure_text("host_unavailable"),
        HostError::Protocol(_) => failure_text("internal"),
    }
}

// ---- calls -------------------------------------------------------------------------------

pub fn launcher(host: &Host) -> Result<Launcher, HostError> {
    parse_launcher(&host.call("native_launcher", json!({}))?)
}

/// `native_preview` for an agent and the profile a permission is sent as.
/// With `arsenal`, the plan and its token cover that exclusion: the start must send the same.
pub fn preview(host: &Host, agent: &str, profile: Option<&str>, arsenal: Option<&Value>) -> Result<Preview, HostError> {
    let mut params = json!({"agent_id": agent, "profile_id": profile});
    if let Some(arsenal) = arsenal {
        params["arsenal"] = arsenal.clone();
    }
    parse_preview(&host.call("native_preview", params)?)
}

pub fn start(host: &Host, agent: &str, project: &str, profile: Option<&str>, arsenal: Option<&Value>, preview: &Preview) -> Result<Started, HostError> {
    let mut params = json!({
        "agent_id": agent,
        "project": project,
        "profile_id": profile,
        "size": {"rows": FIRST_SIZE.0, "cols": FIRST_SIZE.1},
        "brief": null,
        "plan_token": preview.plan_token,
        "expected_cli_version": preview.provider_version,
    });
    if let Some(arsenal) = arsenal {
        params["arsenal"] = arsenal.clone();
    }
    parse_started(&host.call("native_start", params)?)
}

/// `native_restart`. A session with full access is restarted only with `checked`: a preview of
/// its own start ([`prepare_restart`]) the owner confirmed, whose token the host consumes.
pub fn restart(host: &Host, id: &str, checked: Option<&Preview>) -> Result<Started, HostError> {
    let mut params = json!({"id": id, "size": {"rows": FIRST_SIZE.0, "cols": FIRST_SIZE.1}});
    if let Some(preview) = checked {
        params["plan_token"] = json!(preview.plan_token);
        params["expected_cli_version"] = json!(preview.provider_version);
    }
    parse_started(&host.call("native_restart", params)?)
}

pub enum Restart {
    Started(Box<Started>),
    Confirm(Preview),
}

/// Uses the host's current profile plan for this session. An older host or a profile change
/// between calls can still request confirmation when asked to restart.
pub fn prepare_restart(host: &Host, id: &str) -> Result<Restart, HostError> {
    let session = sessions(host)?.into_iter().find(|s| s.id == id)
        .ok_or_else(|| HostError::Remote { code: "session_unknown".into(), reasons: vec![] })?;
    if !full_access(&session) {
        match restart(host, id, None) {
            Err(HostError::Remote { code, .. }) if code == "restart_needs_preview" => {}
            result => return result.map(|started| Restart::Started(Box::new(started))),
        }
    }
    let agent = session.agent_id.as_deref().ok_or_else(|| bad("session has no agent"))?;
    // The host restarts with the exclusion it stored, so the preview names that same one.
    let preview = preview(
        host,
        agent,
        session.profile_id.as_deref(),
        session.arsenal.as_ref(),
    )?;
    if preview.can_start && preview.bypass == Some(false) {
        restart(host, id, Some(&preview)).map(|started| Restart::Started(Box::new(started)))
    } else {
        Ok(Restart::Confirm(preview))
    }
}

fn full_access(session: &Session) -> bool {
    session.restart_bypass == Some(true)
}

pub fn stop(host: &Host, id: &str) -> Result<(), HostError> {
    host.call("native_stop", json!({"id": id})).map(|_| ())
}

pub fn forget(host: &Host, id: &str) -> Result<(), HostError> {
    host.call("native_forget", json!({"id": id})).map(|_| ())
}

pub fn sessions(host: &Host) -> Result<Vec<Session>, HostError> {
    parse_sessions(&host.call("native_sessions", json!({"sample": false}))?)
}

pub fn running_panel_pid(host: &Host, session: &str, terminal: &str) -> Option<u32> {
    let reply = host.call_timeout(
        "native_sessions",
        json!({"sample": false}),
        std::time::Duration::from_millis(750),
    ).ok()?;
    matching_running_pid(&reply, session, terminal)
}

fn matching_running_pid(reply: &Value, session: &str, terminal: &str) -> Option<u32> {
    let entry = reply.as_array()?.iter().find(|entry| entry["id"].as_str() == Some(session))?;
    if entry["state"].as_str() != Some("running") || entry["terminal_id"].as_str() != Some(terminal) {
        return None;
    }
    u32::try_from(entry["pid"].as_u64()?).ok().filter(|pid| *pid > 0)
}

#[test]
fn panel_security_ask_checks_fresh_session_and_terminal_identity() {
    let current = json!([{"id":"agent", "terminal_id":"pane", "state":"running", "pid":123}]);
    assert_eq!(matching_running_pid(&current, "agent", "pane"), Some(123));
    assert_eq!(matching_running_pid(&current, "agent", "other"), None);
    assert_eq!(matching_running_pid(&current, "other", "pane"), None);
    let exited = json!([{"id":"agent", "terminal_id":"pane", "state":"exited", "pid":123}]);
    assert_eq!(matching_running_pid(&exited, "agent", "pane"), None);
}

// ---- opening a launch --------------------------------------------------------------------

/// Why a launch did not open: a sentence in the language in force.
#[derive(Debug, PartialEq, Eq)]
pub struct Refusal(pub String);

impl From<HostError> for Refusal {
    fn from(e: HostError) -> Self {
        Refusal(explain(&e))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Opened {
    /// A terminal of the host with the commands already typed.
    Terminal { terminal_id: String, title: String },
    /// An agent the host started, and what its `before` commands said.
    Agent { started: Started, notes: Vec<String> },
    /// Nothing ran: the launch file runs `before` commands or starts with full access, and the
    /// owner has not approved its exact text. The window shows what it would do and asks.
    NeedsApproval(wyss_console::launch::NeedsApproval),
}

/// The commands, the folder and the fingerprint of the file they came from.
type BeforeCommands<'a> = dyn Fn(&[String], &str, &str) -> Vec<(String, String)> + 'a;

/// Opens one launch: a commands launch is a terminal in its folder
/// with the commands typed; a session launch is checked with `native_preview`, runs its `before`
/// commands and starts with `native_start`. `before` gets the commands, the folder and the file's
/// fingerprint and answers `(command, result)` for each; tests give it a stand-in. `fingerprint`
/// is of the very text `launch` was parsed from, and is bound here to the resolved folder: a
/// commands launch, or a session with `before` commands or full access, whose text and folder the
/// owner has not approved in `approvals` opens nothing and asks.
pub fn open_launch(
    host: &Host,
    launch: &Launch,
    fingerprint: &str,
    home: &str,
    before: &BeforeCommands<'_>,
    approvals: Option<&wyss_console::launch::Approvals>,
) -> Result<Opened, Refusal> {
    open_launch_with(host, launch, fingerprint, home, before, approvals, &|preset, agent, directory| {
        wyss_console::arsenal::library::launch_for_process(preset, agent, std::path::Path::new(directory))
    })
}

/// Compiles a preset (its name, the agent, the folder) into the Arsenal part of the launch.
type ArsenalFor<'a> = dyn Fn(&str, &str, &str) -> Result<wyss_console::native::arsenal::ArsenalLaunch, String> + 'a;

/// [`open_launch`] with the preset compiler given: tests give it a stand-in.
fn open_launch_with(
    host: &Host,
    launch: &Launch,
    fingerprint: &str,
    home: &str,
    before: &BeforeCommands<'_>,
    approvals: Option<&wyss_console::launch::Approvals>,
    arsenal_for: &ArsenalFor<'_>,
) -> Result<Opened, Refusal> {
    let directory = launches::resolve_directory(&launch.directory, home).map_err(|e| Refusal(e.to_owned()))?;
    // The approval covers the file's exact text and the folder it resolved to here.
    let fingerprint = &wyss_console::launch::bind(fingerprint, &directory);
    match &launch.kind {
        Kind::Commands { commands } => {
            if commands.is_empty() {
                return Err(Refusal(tr(t!("This launch has no commands for this distribution. Add one in Workspace."))));
            }
            // Typed with Enter, they run as the owner: nothing is typed from a text not approved.
            let Some(approvals) = approvals else {
                return Err(Refusal(failure_text("store_unavailable")));
            };
            let runs = wyss_console::launch::Runs { directory: &directory, commands, before: &[], bypass: false };
            if let Err(needed) = approvals.check(&launch.id, fingerprint, runs) {
                return Ok(Opened::NeedsApproval(*needed));
            }
            let id = host.create_in(FIRST_SIZE.0, FIRST_SIZE.1, Some(&directory), Some(&launch.name))?;
            host.write(&id, &launches::typed(commands))?;
            Ok(Opened::Terminal { terminal_id: id, title: launch.name.clone() })
        }
        Kind::Session { agent, permission, before: before_cmds, preset } => {
            // The preset is an Arsenal selection: compiled here against the skills installed now,
            // and the very same exclusion goes to every preview and to the start.
            let arsenal = match preset {
                None => None,
                Some(name) => {
                    let compiled = arsenal_for(name, agent.id(), &directory).map_err(|reason| {
                        Refusal(trf(t!("The preset “{name}” cannot be applied: {reason} Nothing was started."), &[("name", name), ("reason", &reason)]))
                    })?;
                    Some(serde_json::to_value(compiled).map_err(|_| Refusal(failure_text("internal")))?)
                }
            };
            let arsenal = arsenal.as_ref();
            let profile = permission.map(|p| p.profile_id());
            let pre = preview(host, agent.id(), profile, arsenal)?;
            if !pre.can_start {
                return Err(Refusal(trf(
                    t!(
                        "The launch “{name}” cannot start with this permission. No commands were run."
                    ),
                    &[("name", &launch.name)],
                )));
            }
            let bypass = launch.full_access() && pre.bypass != Some(false);
            // Approving before commands while bypass is unavailable must not approve bypass
            // for a later invocation after a provider update.
            let before_only;
            let fingerprint = if launch.full_access() && !bypass {
                before_only = wyss_console::launch::bind(fingerprint, "before-without-full-access");
                &before_only
            } else {
                fingerprint
            };
            if !before_cmds.is_empty() || bypass {
                let Some(approvals) = approvals else {
                    return Err(Refusal(failure_text("store_unavailable")));
                };
                let runs = wyss_console::launch::Runs { directory: &directory, commands: &[], before: before_cmds, bypass };
                if let Err(needed) = approvals.check(&launch.id, fingerprint, runs) {
                    return Ok(Opened::NeedsApproval(*needed));
                }
            }
            let notes: Vec<String> = before(before_cmds, &directory, fingerprint)
                .into_iter()
                .filter(|(_, result)| result != "ok")
                .map(|(_, result)| trf(t!("A command before the start did not succeed ({result}); the agent started anyway."), &[("result", &result)]))
                .collect();
            // The `before` commands may take minutes: the start uses a token of its own, fresh.
            let pre = if before_cmds.is_empty() {
                pre
            } else {
                let new = preview(host, agent.id(), profile, arsenal)?;
                if launch.full_access() && !bypass && new.bypass != Some(false) {
                    pre
                } else {
                    new
                }
            };
            if !pre.can_start {
                return Err(Refusal(trf(t!("The launch “{name}” cannot start with this permission. No commands were run."), &[("name", &launch.name)])));
            }
            let started = start(host, agent.id(), &directory, profile, arsenal, &pre)?;
            let mut notes = notes;
            notes.extend(start_notes(&started));
            Ok(Opened::Agent { started, notes })
        }
    }
}

/// Runs a session launch's `before` commands in order, each in `/bin/sh -c` in its folder with no
/// input and its output discarded, bounded in time (`run_before` of `launch.rs`).
pub fn run_before(commands: &[String], folder: &str, fingerprint: &str) -> Vec<(String, String)> {
    let approvals = wyss_console::launch::Approvals::from_env();
    before_with(commands, folder, fingerprint, wyss_console::launch::BEFORE_TIMEOUT, approvals.as_ref())
}

fn before_with(
    commands: &[String],
    folder: &str,
    fingerprint: &str,
    timeout: Duration,
    approvals: Option<&wyss_console::launch::Approvals>,
) -> Vec<(String, String)> {
    use wyss_console::launch::{Kind, Launch};
    let launch = Launch {
        id: String::new(),
        name: String::new(),
        directory: folder.to_owned(),
        kind: Kind::Session {
            agent: String::new(),
            permission: None,
            before: commands.to_vec(),
            preset: None,
        },
    };
    // Checked again where the commands run: nothing runs from a text the owner did not approve.
    let outcomes = approvals.ok_or(()).and_then(|approvals| {
        wyss_console::launch::run_before(&launch, fingerprint, timeout, approvals).map_err(|_| ())
    });
    match outcomes {
        Ok(outcomes) => outcomes.into_iter().map(|outcome| (outcome.command, outcome.result)).collect(),
        Err(()) => commands.iter().map(|c| (c.clone(), "not_approved".to_owned())).collect(),
    }
}

/// `term_list` entries that are agents' terminals, by terminal id.
pub fn tags(list: &[ListEntry]) -> Vec<(String, Tag)> {
    list.iter().filter_map(|e| e.native.clone().map(|t| (e.id.clone(), t))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::tests::fake_host;
    use crate::launches::{Agent, Permission};

    // Answers shaped like the host's structs (`LauncherView`, `PreviewView`, `StartReply`, `SessionView`, `NativeTag`).
    fn launcher_answer() -> Value {
        json!({"agents": [
            {"agent_id": "claude_code", "executable": "claude", "installed": true, "name": "Claude Code", "path": "/x/claude", "version": "2.1.327 (Claude Code)"},
            {"agent_id": "codex", "executable": "codex", "installed": false, "name": "Codex", "path": null, "version": null}],
          "m9_native_status": "legacy_host",
          "projects": [{"default_agent": null, "default_profile": null, "home": false, "last_used": 3, "path": "/work/a"}, {"default_agent": null, "default_profile": null, "home": true, "last_used": 0, "path": "/home/o"}],
          "shell": {"family": "fish", "path": "/bin/fish", "through_shell": true}})
    }

    fn session_answer(state: &str) -> Value {
        json!({"id": "0123456789abcdef0123456789abcdef", "agent_id": "codex", "agent_name": "Codex", "version": "codex-cli 0.159.0", "project": "/home/o",
          "profile": {"id": "template-full-access", "name": "Full access (bypass)", "version": 1}, "through_shell": true, "state": state, "started_at": 1,
          "ended_at": null, "exit_code": null, "exit_signal": null, "error": null, "restart_of": null, "restarts": 0, "delegation": false,
          "plane": "native", "terminal_id": "t-1", "pid": 42, "usage": null})
    }

    #[test]
    fn the_launcher_gives_the_home_and_the_installed_agents() {
        let l = parse_launcher(&launcher_answer()).unwrap();
        assert_eq!(l, Launcher {
            home: Some("/home/o".into()),
            installed: vec!["claude_code".into()],
            projects: vec![("/work/a".into(), 3), ("/home/o".into(), 0)],
        });
        assert!(parse_launcher(&json!({"shell": null})).is_err());
    }

    #[test]
    fn restart_access_uses_the_host_plan_instead_of_the_profile_name() {
        let mut row = session_answer("running");
        row["profile"]["name"] = json!("Custom automation");
        row["restart_bypass"] = json!(true);
        assert!(full_access(&parse_session(&row).unwrap()));
        row["profile"]["name"] = json!("Full access (bypass)");
        row["restart_bypass"] = json!(false);
        assert!(!full_access(&parse_session(&row).unwrap()));
    }

    #[test]
    fn previews_sessions_and_replies_are_read() {
        let p = parse_preview(&json!({"applied": [], "can_start": true, "not_applicable": [], "plan_token": "tok", "through_shell": true, "with_cost": []})).unwrap();
        assert_eq!(
            p,
            Preview {
                can_start: true,
                plan_token: Some("tok".into()),
                provider_version: None,
                bypass: None
            }
        );
        assert!(
            !parse_preview(&json!({"can_start": false, "plan_token": null}))
                .unwrap()
                .can_start
        );
        assert!(parse_preview(&json!({})).is_err());

        let s = parse_session(&session_answer("running")).unwrap();
        assert_eq!((s.id.as_str(), s.state.as_str(), s.terminal_id.as_deref(), s.profile.as_deref()), ("0123456789abcdef0123456789abcdef", "running", Some("t-1"), Some("Full access (bypass)")));
        assert_eq!(parse_sessions(&json!([session_answer("stopped"), {"nope": 1}])).unwrap().len(), 1, "an entry that is not a session is skipped");
        assert!(parse_sessions(&json!({})).is_err());

        let reply = json!({"session": session_answer("running"), "terminal": {"terminal_id": "t-9", "shell": "/bin/fish", "cwd": "/home/o", "observed_pid": 7, "state": "running"}});
        let st = parse_started(&reply).unwrap();
        assert_eq!((st.terminal_id.as_str(), st.session.agent_name.as_str()), ("t-9", "Codex"));
        assert!(parse_started(&json!({"session": session_answer("running")})).is_err());
    }

    #[test]
    fn the_tag_of_a_terminal_labels_the_agent_like_the_session_bar() {
        let tag = parse_tag(&json!({"session_id": "s", "agent_name": "Claude Code", "project": "/home/o", "profile": "Full access (bypass)", "through_shell": true})).unwrap();
        assert_eq!(agent_label(&tag), "Native · Claude Code · Full access");
        let plain = Tag { profile: None, ..tag.clone() };
        assert_eq!(agent_label(&plain), "Native · Claude Code");
        let old = Tag { profile: Some("My old profile".into()), ..tag.clone() };
        assert_eq!(agent_label(&old), "Native · Claude Code · Profile My old profile");
        assert_eq!(started_with(Some("As configured")), None);
        assert_eq!(started_with(Some("Strict")).as_deref(), Some("Ask before acting"));
        assert_eq!(started_with(Some("Review, read only")).as_deref(), Some("Read only"));
        assert_eq!(parse_tag(&json!(null)), None);
    }

    #[test]
    fn a_session_offers_stop_and_restart_while_it_runs_and_restart_and_forget_after() {
        assert_eq!(lifecycle("running"), Some(Lifecycle::Running));
        for s in ["exited", "stopped", "failed_start", "lost"] {
            assert_eq!(lifecycle(s), Some(Lifecycle::Finished), "{s}");
        }
        assert_eq!(lifecycle("requested"), None, "a start being prepared offers nothing");
    }

    #[test]
    fn failures_have_words_and_unknown_codes_the_general_sentence() {
        assert_eq!(failure_text("agent_not_installed"), "That agent is not installed on this computer.");
        assert_eq!(failure_text("limit_reached"), "The terminal limit (16) is reached. Close one to start another.");
        assert_eq!(failure_text("something_new"), "The terminal host answered “something_new”. Nothing was started.");
        assert_eq!(explain(&HostError::Io(std::io::Error::other("x"))), failure_text("host_unavailable"));
        assert_eq!(explain(&HostError::Remote { code: "busy".into(), reasons: vec![] }), "A start is already in progress.");
    }

    fn session_launch(permission: Option<Permission>, preset: Option<&str>) -> Launch {
        Launch {
            id: "codex".into(),
            name: "Codex".into(),
            directory: "~".into(),
            kind: Kind::Session { agent: Agent::Codex, permission, before: vec!["true".into()], preset: preset.map(str::to_owned) },
        }
    }

    #[test]
    fn a_session_launch_goes_preview_then_start_with_the_template_and_the_token() {
        let reply = json!({"session": session_answer("running"), "terminal": {"terminal_id": "t-9"}});
        let (path, h) = fake_host(vec![
            ("native_preview", json!({"ok": {"applied": [], "can_start": true, "not_applicable": [], "plan_token": "tok-1", "through_shell": true, "with_cost": [], "provider_version": "codex-cli 0.159.0"}})),
            ("native_preview", json!({"ok": {"applied": [], "can_start": true, "not_applicable": [], "plan_token": "tok-2", "through_shell": true, "with_cost": [], "provider_version": "codex-cli 0.159.0"}})),
            ("native_start", json!({"ok": reply})),
        ]);
        let home = std::env::temp_dir().to_string_lossy().into_owned();
        let ran = std::cell::RefCell::new(Vec::new());
        let before = |c: &[String], dir: &str, _: &str| {
            ran.borrow_mut().push((c.to_vec(), dir.to_owned()));
            vec![("true".to_owned(), "ok".to_owned()), ("curl x".to_owned(), "exit 7".to_owned())]
        };
        let approvals = test_approvals();
        let out = open_launch(&Host::new(&path), &session_launch(Some(Permission::Bypass), None), PRINT, &home, &before, Some(&approvals)).unwrap();
        let Opened::Agent { started, notes } = out else { panic!("{out:?}") };
        assert_eq!(started.terminal_id, "t-9");
        assert_eq!(notes, vec!["A command before the start did not succeed (exit 7); the agent started anyway."], "only what did not succeed is said");
        assert_eq!(ran.borrow()[0], (vec!["true".to_string()], home.clone()));
        let seen = h.join().unwrap();
        assert_eq!(seen[0]["params"], json!({"agent_id": "codex", "profile_id": "template-full-access"}), "the host receives agent_id and profile_id directly in params");
        assert_eq!(seen[1]["op"], seen[0]["op"], "a fresh preview after the before commands");
        let start = &seen[2]["params"];
        assert_eq!(start["agent_id"], "codex");
        assert_eq!(start["project"], home.as_str());
        assert_eq!(start["profile_id"], "template-full-access");
        assert_eq!(start["plan_token"], "tok-2", "the start uses the fresh token");
        assert_eq!(start["expected_cli_version"], "codex-cli 0.159.0");
        assert_eq!(start["size"], json!({"rows": 24, "cols": 80}));
        assert_eq!(start["brief"], Value::Null);
    }

    #[test]
    fn a_preview_that_cannot_start_stops_everything_before_any_command_runs() {
        let (path, h) = fake_host(vec![("native_preview", json!({"ok": {"can_start": false, "plan_token": null}}))]);
        let home = std::env::temp_dir().to_string_lossy().into_owned();
        let before = |_: &[String], _: &str, _: &str| -> Vec<(String, String)> { panic!("no command may run") };
        let r = open_launch(&Host::new(&path), &session_launch(None, None), PRINT, &home, &before, Some(&test_approvals())).unwrap_err();
        assert_eq!(r.0, "The launch “Codex” cannot start with this permission. No commands were run.");
        let seen = h.join().unwrap();
        assert_eq!(seen[0]["params"]["profile_id"], Value::Null, "as configured sends no template");
    }

    #[test]
    fn a_preset_that_cannot_be_compiled_is_refused_without_touching_the_host() {
        let host = Host::new("/nonexistent/none.sock");
        let before = |_: &[String], _: &str, _: &str| -> Vec<(String, String)> { panic!("no command may run") };
        let home = std::env::temp_dir().to_string_lossy().into_owned();
        let missing = |_: &str, _: &str, _: &str| Err("There is no Arsenal preset \"Reviewer\" for this agent.".to_owned());
        let r = open_launch_with(&host, &session_launch(Some(Permission::Ask), Some("Reviewer")), PRINT, &home, &before, Some(&test_approvals()), &missing).unwrap_err();
        assert!(r.0.contains("Reviewer") && r.0.contains("no Arsenal preset") && r.0.contains("Nothing was started"), "{}", r.0);
    }

    #[test]
    fn a_launch_with_a_preset_sends_the_same_compiled_arsenal_to_every_preview_and_the_start() {
        let reply = json!({"session": session_answer("running"), "terminal": {"terminal_id": "t-9"}, "skills_skipped": [{"name": "tutor", "reason": "digest_mismatch"}]});
        let (path, h) = fake_host(vec![
            ("native_preview", json!({"ok": {"applied": [], "can_start": true, "not_applicable": [], "plan_token": "tok-1", "through_shell": true, "with_cost": [], "provider_version": "codex-cli 0.159.0"}})),
            ("native_preview", json!({"ok": {"applied": [], "can_start": true, "not_applicable": [], "plan_token": "tok-2", "through_shell": true, "with_cost": [], "provider_version": "codex-cli 0.159.0"}})),
            ("native_start", json!({"ok": reply})),
        ]);
        let home = std::env::temp_dir().to_string_lossy().into_owned();
        let asked = std::cell::RefCell::new(Vec::new());
        let compile = |preset: &str, agent: &str, dir: &str| {
            asked.borrow_mut().push((preset.to_owned(), agent.to_owned(), dir.to_owned()));
            Ok(wyss_console::native::arsenal::ArsenalLaunch {
                selection: "6f1d2b8e-0000-4000-8000-00000000a001".into(),
                selection_name: "Reviewer".into(),
                config_hash: "c".repeat(64),
                skills_off: vec!["design".into()],
                skill_files_off: vec!["/h/.codex/skills/design/SKILL.md".into()],
                home: None,
                codex_home: None,
                launch_instructions: None,
                materialize: Vec::new(),
            })
        };
        let before = |_: &[String], _: &str, _: &str| vec![("true".to_owned(), "ok".to_owned())];
        let out = open_launch_with(&Host::new(&path), &session_launch(Some(Permission::Ask), Some("Reviewer")), PRINT, &home, &before, Some(&test_approvals()), &compile).unwrap();
        let Opened::Agent { notes, .. } = out else { panic!("{out:?}") };
        assert_eq!(notes, vec!["Could not prepare the skill tutor: its stored files do not match their digest. The agent started without it."]);
        assert_eq!(asked.borrow().as_slice(), &[("Reviewer".to_owned(), "codex".to_owned(), home.clone())]);
        let seen = h.join().unwrap();
        let arsenal = &seen[0]["params"]["arsenal"];
        assert_eq!(arsenal["skills_off"], json!(["design"]));
        assert_eq!(arsenal["selection_name"], "Reviewer");
        assert_eq!(&seen[1]["params"]["arsenal"], arsenal, "the fresh preview names the same exclusion");
        assert_eq!(&seen[2]["params"]["arsenal"], arsenal, "the start sends what its token covers");
        assert_eq!(seen[2]["params"]["plan_token"], "tok-2");
    }

    #[test]
    fn a_restart_preview_names_the_exclusion_the_session_started_with() {
        let mut row = session_answer("running");
        row["restart_bypass"] = json!(true);
        row["arsenal"] = json!({"selection": "s-1", "skills_off": ["design"]});
        let (path, h) = fake_host(vec![
            ("native_sessions", json!({"ok": [row]})),
            ("native_preview", json!({"ok": {"applied": [], "can_start": true, "not_applicable": [], "plan_token": "tok", "through_shell": true, "with_cost": []}})),
        ]);
        let Restart::Confirm(_) = prepare_restart(&Host::new(&path), "0123456789abcdef0123456789abcdef").unwrap() else { panic!("asks") };
        let seen = h.join().unwrap();
        assert_eq!(seen[1]["params"]["arsenal"], json!({"selection": "s-1", "skills_off": ["design"]}));
    }

    #[test]
    fn host_failures_become_the_consoles_sentences() {
        let (path, _h) = fake_host(vec![("native_preview", json!({"err": {"code": "agent_not_installed", "reasons": [], "bytes_written": 0}}))]);
        let home = std::env::temp_dir().to_string_lossy().into_owned();
        let before = |_: &[String], _: &str, _: &str| -> Vec<(String, String)> { Vec::new() };
        let r = open_launch(&Host::new(&path), &session_launch(None, None), PRINT, &home, &before, Some(&test_approvals())).unwrap_err();
        assert_eq!(r.0, "That agent is not installed on this computer.");
    }

    #[test]
    fn stop_restart_forget_and_sessions_send_what_the_console_sends() {
        let reply = json!({"session": session_answer("running"), "terminal": {"terminal_id": "t-10"}});
        let (path, h) = fake_host(vec![
            ("native_stop", json!({"ok": null})),
            ("native_restart", json!({"ok": reply})),
            ("native_forget", json!({"ok": null})),
            ("native_sessions", json!({"ok": [session_answer("stopped")]})),
        ]);
        let host = Host::new(&path);
        stop(&host, "sid").unwrap();
        assert_eq!(restart(&host, "sid", None).unwrap().terminal_id, "t-10");
        forget(&host, "sid").unwrap();
        assert_eq!(sessions(&host).unwrap()[0].state, "stopped");
        let seen = h.join().unwrap();
        assert_eq!(seen[0]["params"], json!({"id": "sid"}));
        assert_eq!(seen[1]["params"], json!({"id": "sid", "size": {"rows": 24, "cols": 80}}));
        assert_eq!(seen[2]["params"], json!({"id": "sid"}));
        assert_eq!(seen[3]["params"], json!({"sample": false}));
    }

    #[test]
    fn a_full_access_restart_previews_the_sessions_own_start_and_sends_its_token() {
        let reply = json!({"session": session_answer("running"), "terminal": {"terminal_id": "t-11"}});
        let mut custom = session_answer("running");
        custom["profile"]["name"] = json!("Custom automation");
        custom["restart_bypass"] = json!(true);
        let (path, h) = fake_host(vec![
            ("native_sessions", json!({"ok": [custom]})),
            ("native_preview", json!({"ok": {"can_start": true, "plan_token": "tok-r", "provider_version": "codex-cli 0.159.0"}})),
            ("native_restart", json!({"ok": reply})),
        ]);
        let host = Host::new(&path);
        let id = "0123456789abcdef0123456789abcdef";
        let Restart::Confirm(pre) = prepare_restart(&host, id).unwrap() else {
            panic!("bypass restart did not ask for confirmation");
        };
        assert_eq!(restart(&host, id, Some(&pre)).unwrap().terminal_id, "t-11");
        let seen = h.join().unwrap();
        assert_eq!(seen[1]["params"], json!({"agent_id": "codex", "profile_id": "template-full-access"}));
        assert_eq!(seen[2]["params"], json!({"id": id, "size": {"rows": 24, "cols": 80}, "plan_token": "tok-r", "expected_cli_version": "codex-cli 0.159.0"}));

    }

    #[test]
    fn a_commands_launch_changed_outside_types_nothing_until_approved_for_its_folder() {
        let dir = std::env::temp_dir().join(format!("wyss-approvals-{}", uuid::Uuid::new_v4().simple()));
        let approvals = wyss_console::launch::Approvals::at(dir.join("launch-approvals.json"));
        let l = Launch { id: "x".into(), name: "Preset de prueba".into(), directory: "/tmp".into(), kind: Kind::Commands { commands: vec!["echo preset-ok".into()] } };
        let before = |_: &[String], _: &str, _: &str| -> Vec<(String, String)> { panic!("a terminal launch has no before") };
        // No host at all: a terminal or a write would fail the call, not ask.
        let nowhere = Host::new("/nonexistent/none.sock");
        let Opened::NeedsApproval(needed) = open_launch(&nowhere, &l, PRINT, "/home/o", &before, Some(&approvals)).unwrap() else { panic!("asks") };
        assert_eq!((needed.directory.as_str(), needed.commands.clone(), needed.bypass), ("/tmp", vec!["echo preset-ok".to_owned()], false));
        assert_eq!(needed.fingerprint, wyss_console::launch::bind(PRINT, "/tmp"));
        assert!(matches!(open_launch(&nowhere, &l, PRINT, "/home/o", &before, None), Err(Refusal(_))), "without the store nothing is typed");
        approvals.approve(&needed.fingerprint).unwrap();
        // Approved, it types; the same text in another folder asks again.
        let (path, h) = fake_host(vec![("term_create", json!({"ok": {"terminal_id": "t-5"}})), ("term_write", json!({"ok": 15}))]);
        assert!(matches!(open_launch(&Host::new(&path), &l, PRINT, "/home/o", &before, Some(&approvals)).unwrap(), Opened::Terminal { .. }));
        assert_eq!(h.join().unwrap()[1]["params"]["data"], json!(b"echo preset-ok\r".to_vec()));
        let elsewhere = Launch { directory: "/".into(), ..l.clone() };
        assert!(matches!(open_launch(&nowhere, &elsewhere, PRINT, "/home/o", &before, Some(&approvals)).unwrap(), Opened::NeedsApproval(_)));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_commands_launch_creates_a_terminal_in_its_folder_and_types_the_commands() {
        let (path, h) = fake_host(vec![("term_create", json!({"ok": {"terminal_id": "t-5"}})), ("term_write", json!({"ok": 15}))]);
        let l = Launch { id: "x".into(), name: "Preset de prueba".into(), directory: "/tmp".into(), kind: Kind::Commands { commands: vec!["echo preset-ok".into()] } };
        let before = |_: &[String], _: &str, _: &str| -> Vec<(String, String)> { panic!("a terminal launch has no before") };
        let out = open_launch(&Host::new(&path), &l, PRINT, "/home/o", &before, Some(&test_approvals())).unwrap();
        assert_eq!(out, Opened::Terminal { terminal_id: "t-5".into(), title: "Preset de prueba".into() });
        let seen = h.join().unwrap();
        assert_eq!(seen[0]["params"]["cwd"], "/tmp");
        assert_eq!(seen[0]["params"]["title"], "Preset de prueba");
        assert_eq!(seen[1]["params"]["data"], json!(b"echo preset-ok\r".to_vec()));
        let empty = Launch { kind: Kind::Commands { commands: vec![] }, ..l };
        assert!(open_launch(&Host::new("/nonexistent/none.sock"), &empty, PRINT, "/home/o", &before, None).unwrap_err().0.contains("no commands"));
    }

    #[test]
    fn before_commands_report_ok_exit_codes_and_timeouts() {
        let temp = std::env::temp_dir();
        let folder = temp.to_str().unwrap();
        let approvals = Some(test_approvals());
        let a = approvals.as_ref();
        let r = before_with(&["true".into(), "exit 3".into(), "echo hi >/dev/null".into()], folder, PRINT, Duration::from_secs(30), a);
        assert_eq!(r.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>(), vec!["ok", "exit 3", "ok"]);
        assert_eq!(before_with(&["sleep 5".into()], folder, PRINT, Duration::from_millis(200), a)[0].1, "timeout");
        assert_eq!(before_with(&["true".into()], "/definitely/not/here", PRINT, Duration::from_secs(1), a)[0].1, "not_started");
        // A text nobody approved runs nothing.
        let other = "b".repeat(64);
        assert_eq!(before_with(&["true".into()], folder, &other, Duration::from_secs(1), a)[0].1, "not_approved");
    }

    /// The fingerprint the test launches stand for, approved in a private store of the test.
    const PRINT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn test_approvals() -> wyss_console::launch::Approvals {
        let dir = std::env::temp_dir().join(format!("wyss-approvals-{}", uuid::Uuid::new_v4().simple()));
        let approvals = wyss_console::launch::Approvals::at(dir.join("launch-approvals.json"));
        approvals.approve(PRINT).unwrap();
        // `open_launch` binds the text's print to the folder it resolves to.
        for folder in [std::env::temp_dir().to_string_lossy().into_owned(), "/tmp".to_owned()] {
            approvals.approve(&wyss_console::launch::bind(PRINT, &folder)).unwrap();
        }
        approvals
    }

    #[test]
    fn a_session_launch_whose_text_is_not_approved_shows_what_it_would_run_and_runs_nothing() {
        let (path, handle) = fake_host(vec![(
            "native_preview",
            json!({
                "ok": { "can_start": true, "plan_token": "one", "bypass": true }
            }),
        )]);
        let host = Host::new(&path);
        let before = |_: &[String], _: &str, _: &str| -> Vec<(String, String)> { panic!("no command may run") };
        let home = std::env::temp_dir().to_string_lossy().into_owned();
        let approvals = test_approvals();
        let other = "c".repeat(64);
        let out = open_launch(&host, &session_launch(Some(Permission::Bypass), None), &other, &home, &before, Some(&approvals)).unwrap();
        let Opened::NeedsApproval(needed) = out else { panic!("{out:?}") };
        assert_eq!((needed.before, needed.bypass, needed.fingerprint), (vec!["true".to_owned()], true, wyss_console::launch::bind(&other, &home)));
        assert_eq!(handle.join().unwrap().len(), 1);
    }

    #[test]
    fn every_error_the_host_can_send_has_its_own_message() {
        use std::path::Path;
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut codes = std::collections::BTreeSet::new();
        let mut stack = vec![root.join("native"), root.join("host")];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") && !path.to_string_lossy().contains("test") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    for part in text.split("Error::new(\"").skip(1) {
                        if let Some(code) = part.split('"').next().filter(|c| c.chars().all(|c| c.is_ascii_lowercase() || c == '_')) {
                            codes.insert(code.to_owned());
                        }
                    }
                }
            }
        }
        // Codes of host operations this window never calls, or that only reach other clients.
        let not_shown = ["delegation_unavailable", "handoff_target_unverified", "busy", "too_large"];
        let generic = failure_text("a_code_nobody_sends");
        for code in codes.iter().filter(|c| !not_shown.contains(&c.as_str())) {
            assert_ne!(failure_text(code).replace(code.as_str(), ""), generic.replace("a_code_nobody_sends", ""), "{code} has no message of its own");
        }
    }

}
