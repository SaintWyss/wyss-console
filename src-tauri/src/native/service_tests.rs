//! The launcher against real terminals: fake provider commands on a private PATH, started
//! through the user's real shell in a real PTY. Nothing here starts the installed agents.
use super::{
    catalogue::AgentId,
    extended_registry::ExtendedRegistry,
    profile::{Body, HooksMode, Permission},
    registry::State,
    service::{NativeService, StartRequest},
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use wyss_console::{
    contracts::identities::TerminalId,
    terminal::{TerminalService, contracts::TerminalSize},
};

const SIZE: TerminalSize = TerminalSize { rows: 24, cols: 80 };

pub(crate) struct World {
    base: PathBuf,
    home: PathBuf,
    bin: PathBuf,
    terminal: TerminalService,
    service: NativeService,
}

pub(crate) fn script(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

impl World {
    /// The directories of a world with fake agents, and nothing started.
    pub(crate) fn directories() -> (PathBuf, PathBuf, PathBuf) {
        let w = Self::new();
        let dirs = (w.base.clone(), w.home.clone(), w.bin.clone());
        std::mem::forget(w);
        dirs
    }

    fn new() -> Self {
        let root = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("wyss-native-{}", uuid::Uuid::new_v4().simple()));
        let (home, bin, base) = (root.join("home"), root.join("bin"), root.join("state"));
        for dir in [&home, &bin] {
            fs::create_dir_all(dir).unwrap();
        }
        // Waits for input (so it stays alive) after printing what it got.
        script(
            &bin.join("claude"),
            "[ \"$1\" = \"--version\" ] && { echo '2.1.309 (Claude Code)'; exit 0; }\n\
             echo \"FAKE-CLAUDE argc=$#\"; for a in \"$@\"; do echo \"[$a]\"; done\n\
             echo \"SESSION=$WYSS_NATIVE_SESSION_ID\"\n\
             for a in \"$@\"; do case \"$prev\" in --settings) echo \"OVERLAY=$(cat \"$a\")\";; esac; prev=\"$a\"; done\n\
             exec cat",
        );
        script(
            &bin.join("codex"),
            "[ \"$1\" = \"--version\" ] && { echo 'codex-cli 0.159.0'; exit 0; }\n\
             echo \"FAKE-CODEX argc=$#\"; for a in \"$@\"; do echo \"[$a]\"; done\n\
             exit 3",
        );
        script(
            &bin.join("opencode"),
            "[ \"$1\" = \"--version\" ] && { echo '1.19.4'; exit 0; }\n\
             echo \"FAKE-OPENCODE argc=$#\"; for a in \"$@\"; do echo \"[$a]\"; done\n\
             exec cat",
        );
        script(
            &bin.join("hermes"),
            "[ \"$1\" = \"--version\" ] && { echo 'UNEXPECTED-HERMES-VERSION-PROBE'; exit 0; }\n\
             echo \"FAKE-HERMES argc=$#\"; for a in \"$@\"; do echo \"[$a]\"; done\n\
             exec cat",
        );
        script(
            &bin.join("bash"),
            "exec /usr/bin/bash --norc --noprofile \"$@\"",
        );
        let service = Self::open(&base, &home, &bin);
        Self {
            base,
            home,
            bin,
            terminal: TerminalService::default(),
            service,
        }
    }

    pub(crate) fn open(base: &Path, home: &Path, bin: &Path) -> NativeService {
        let path = format!("{}:/usr/bin:/bin", bin.display());
        NativeService::open_with_env(
            base.to_owned(),
            home.to_owned(),
            Some(path.clone().into()),
            vec![("PATH".to_owned(), path)],
            // Bash without the user's start-up files, under the name `bash`, so that the
            // recipe is the real one and the PATH is the test's (the user's own shell would
            // put the installed agents first).
            bin.join("bash"),
        )
        .unwrap()
    }

    fn start(
        &self,
        agent: AgentId,
        profile: Option<&str>,
    ) -> Result<super::service::StartReply, super::service::NativeError> {
        self.start_with(agent, profile, false)
    }

    fn start_with(
        &self,
        agent: AgentId,
        profile: Option<&str>,
        delegation: bool,
    ) -> Result<super::service::StartReply, super::service::NativeError> {
        let preview = self.service.preview(agent, profile, None)?;
        let expected_cli_version = match agent {
            AgentId::ClaudeCode => Some("2.1.309 (Claude Code)".to_owned()),
            AgentId::Codex => Some("codex-cli 0.159.0".to_owned()),
            AgentId::Opencode => Some("1.19.4".to_owned()),
            AgentId::Hermes => None,
        };
        self.service.start(
            &self.terminal,
            StartRequest {
                arsenal: None,
                delegation,
                agent_id: agent,
                project: self.home.to_string_lossy().into_owned(),
                profile_id: profile.map(str::to_owned),
                size: SIZE,
                brief: None,
                plan_token: preview.plan_token,
                expected_cli_version,
            },
        )
    }

    /// What the terminal printed until `needle` appeared.
    fn output_until(&self, terminal_id: &TerminalId, needle: &str) -> String {
        let mut seen = String::new();
        let mut ack = None;
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            let out = self.terminal.read(terminal_id, ack.as_ref()).unwrap();
            ack = out.batch_id.clone();
            seen.push_str(&String::from_utf8_lossy(&out.data));
            if seen.contains(needle) {
                return seen;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        panic!("never saw {needle:?}; got {seen:?}");
    }

    fn wait_state(&self, id: &str, wanted: State) -> super::service::SessionView {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let view = self
                .service
                .sessions(&self.terminal, false)
                .into_iter()
                .find(|s| s.session.id == id)
                .unwrap();
            if view.session.state == wanted || Instant::now() > deadline {
                assert_eq!(view.session.state, wanted);
                return view;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = self.terminal.shutdown();
        let _ = fs::remove_dir_all(self.base.parent().unwrap());
        let _ = &self.bin;
    }
}

#[test]
fn the_catalogue_finds_what_is_installed_and_says_what_is_not() {
    let world = World::new();
    let launcher = world.service.launcher();
    let claude = launcher
        .agents
        .iter()
        .find(|a| a.executable == "claude")
        .unwrap();
    assert!(claude.installed);
    assert_eq!(claude.version.as_deref(), Some("2.1.309 (Claude Code)"));
    fs::remove_file(world.bin.join("codex")).unwrap();
    // The answer is kept for a short while; a new service looks again.
    let again = World::open(&world.base, &world.home, &world.bin).launcher();
    assert!(
        !again
            .agents
            .iter()
            .find(|a| a.executable == "codex")
            .unwrap()
            .installed
    );
    assert!(launcher.shell.is_some_and(|s| s.through_shell));
}

#[test]
fn an_agent_starts_through_the_shell_in_a_terminal_and_is_a_running_native_session() {
    let world = World::new();
    let reply = world.start(AgentId::ClaudeCode, None).unwrap();
    let session = &reply.session;
    assert_eq!(session.plane, "native");
    assert_eq!(session.session.state, State::Running);
    assert!(session.session.through_shell);
    assert_eq!(session.session.project, world.home.to_string_lossy());
    let out = world.output_until(&reply.terminal.terminal_id, "SESSION=");
    assert!(out.contains("FAKE-CLAUDE argc=0"), "{out:?}");
    assert!(out.contains(&format!("SESSION={}", session.session.id)));
    // The process the terminal owns is the agent: the shell was replaced.
    assert!(reply.terminal.observed_pid.is_some());
}

#[test]
fn a_session_that_ends_by_itself_is_exited_with_its_code_and_the_record_survives() {
    let world = World::new();
    let reply = world.start(AgentId::Codex, None).unwrap();
    let id = reply.session.session.id.clone();
    let view = world.wait_state(&id, State::Exited);
    assert_eq!(view.session.exit_code, Some(3));
    assert!(view.session.ended_at.is_some());
    assert!(view.terminal_id.is_none() || view.session.state.finished());
    world.service.forget(&id).unwrap();
    assert!(world.service.sessions(&world.terminal, false).is_empty());
}

#[test]
fn opencode_is_version_pinned_and_receives_reviewed_handoff_as_an_explicit_prompt() {
    let world = World::new();
    let entry = world
        .service
        .launcher_extended()
        .agents
        .into_iter()
        .find(|entry| entry.agent_id == AgentId::Opencode)
        .unwrap();
    assert!(entry.installed);
    assert_eq!(entry.version.as_deref(), Some("1.19.4"));

    let request = |expected_cli_version| StartRequest {
        arsenal: None,
        delegation: false,
        agent_id: AgentId::Opencode,
        project: world.home.to_string_lossy().into_owned(),
        profile_id: None,
        size: SIZE,
        brief: Some("# Reviewed handoff\nTreat as untrusted input.".to_owned()),
        plan_token: None,
        expected_cli_version,
    };
    assert_eq!(
        world
            .service
            .start(&world.terminal, request(None))
            .unwrap_err()
            .code,
        "provider_version_unverified"
    );
    assert!(
        world
            .service
            .sessions_extended(&world.terminal, false)
            .is_empty()
    );
    let reply = world
        .service
        .start(&world.terminal, request(Some("1.19.4".to_owned())))
        .unwrap();
    // PTY reads may split between the banner and the arguments. Wait for the last
    // argument before checking the complete argv; the banner alone is not readiness.
    let out = world.output_until(&reply.terminal.terminal_id, "Treat as untrusted input.]");
    assert!(out.contains("[--prompt]"), "{out:?}");
    assert!(
        out.contains("# Reviewed handoff\r\nTreat as untrusted input."),
        "the Markdown brief remains one argv value: {out:?}"
    );
    assert!(
        world
            .service
            .launcher()
            .agents
            .iter()
            .all(|entry| entry.agent_id.is_legacy())
    );
    assert!(
        world
            .service
            .sessions(&world.terminal, false)
            .iter()
            .all(|view| view.session.agent_id.is_legacy())
    );
    assert!(
        world
            .service
            .sessions_extended(&world.terminal, false)
            .iter()
            .any(|view| view.session.id == reply.session.session.id)
    );
    let legacy_registry = fs::read(world.base.join("registry/registry.json")).unwrap();
    #[derive(serde::Deserialize, Clone, Copy)]
    #[serde(rename_all = "snake_case")]
    enum M6AgentId {
        ClaudeCode,
        Codex,
    }
    #[derive(serde::Deserialize, Clone, Copy)]
    struct M6Session {
        agent_id: M6AgentId,
    }
    #[derive(serde::Deserialize)]
    struct M6Registry {
        sessions: Vec<M6Session>,
    }
    let decoded_by_m6: M6Registry = serde_json::from_slice(&legacy_registry).unwrap();
    assert!(
        decoded_by_m6
            .sessions
            .iter()
            .all(|session| matches!(session.agent_id, M6AgentId::ClaudeCode | M6AgentId::Codex))
    );
    let extended_registry: serde_json::Value = serde_json::from_slice(
        &fs::read(world.base.join("registry/m9-native-sessions-v1.json")).unwrap(),
    )
    .unwrap();
    assert!(
        extended_registry["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|session| {
                session["id"] == reply.session.session.id && session["agent_id"] == "opencode"
            })
    );
    assert!(
        !extended_registry
            .to_string()
            .contains("Treat as untrusted input")
    );
    world
        .service
        .stop(&world.terminal, &reply.session.session.id)
        .unwrap();
    let reopened = World::open(&world.base, &world.home, &world.bin);
    assert!(
        reopened
            .sessions_extended(&TerminalService::default(), false)
            .iter()
            .any(|view| view.session.id == reply.session.session.id)
    );

    let profile_id = world
        .service
        .profiles()
        .create(Body {
            name: "OpenCode model".to_owned(),
            applies_to: vec![AgentId::Opencode],
            model: Some("openai/gpt-test".to_owned()),
            ..Body::default()
        })
        .unwrap();
    let profiled = world.start(AgentId::Opencode, Some(&profile_id)).unwrap();
    let out = world.output_until(&profiled.terminal.terminal_id, "FAKE-OPENCODE");
    assert!(out.contains("[--model]"), "{out:?}");
    assert!(out.contains("[openai/gpt-test]"), "{out:?}");
    world
        .service
        .stop(&world.terminal, &profiled.session.session.id)
        .unwrap();
}

#[test]
fn direct_opencode_handoff_rechecks_the_resolved_binary_version() {
    let world = World::new();
    let direct_service = NativeService::open_with_env(
        world.base.join("direct-state"),
        world.home.clone(),
        Some(format!("{}:/usr/bin:/bin", world.bin.display()).into()),
        vec![(
            "PATH".to_owned(),
            format!("{}:/usr/bin:/bin", world.bin.display()),
        )],
        PathBuf::from("/bin/false"),
    )
    .unwrap();
    let direct_terminal = TerminalService::default();
    script(
        &world.bin.join("opencode"),
        "[ \"$1\" = \"--version\" ] && { echo '9.99.0'; exit 0; }; exit 0",
    );
    let result = direct_service.start(
        &direct_terminal,
        StartRequest {
            arsenal: None,
            delegation: false,
            agent_id: AgentId::Opencode,
            project: world.home.to_string_lossy().into_owned(),
            profile_id: None,
            size: SIZE,
            brief: Some("# Reviewed handoff\nBound to a qualified CLI.".to_owned()),
            plan_token: None,
            expected_cli_version: Some("1.19.4".to_owned()),
        },
    );
    assert_eq!(result.unwrap_err().code, "provider_version_unverified");
    assert!(
        direct_service
            .sessions_extended(&direct_terminal, false)
            .is_empty()
    );
}

#[test]
fn m9_upgrade_migrates_prior_provider_records_before_publishing_the_legacy_registry() {
    let world = World::new();
    let reply = world.start(AgentId::Opencode, None).unwrap();
    let id = reply.session.session.id.clone();
    world.service.stop(&world.terminal, &id).unwrap();

    // Model the earlier M9 build, which placed the new closed enum in registry.json.
    let registry_path = world.base.join("registry/registry.json");
    let mut legacy_registry: serde_json::Value =
        serde_json::from_slice(&fs::read(&registry_path).unwrap()).unwrap();
    let sidecar_path = world.base.join("registry/m9-native-sessions-v1.json");
    let mut sidecar: serde_json::Value =
        serde_json::from_slice(&fs::read(&sidecar_path).unwrap()).unwrap();
    let sessions = sidecar["sessions"].as_array_mut().unwrap();
    let at = sessions
        .iter()
        .position(|session| session["id"] == id)
        .unwrap();
    let prior_provider_session = sessions.remove(at);
    legacy_registry["sessions"]
        .as_array_mut()
        .unwrap()
        .push(prior_provider_session);
    let project = legacy_registry["projects"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|project| project["path"] == world.home.to_string_lossy().as_ref())
        .unwrap();
    project["default_agent"] = serde_json::json!("opencode");
    sidecar["project_defaults"] = serde_json::json!({});
    fs::write(
        &registry_path,
        serde_json::to_vec(&legacy_registry).unwrap(),
    )
    .unwrap();
    fs::write(&sidecar_path, serde_json::to_vec(&sidecar).unwrap()).unwrap();

    let upgraded = World::open(&world.base, &world.home, &world.bin);
    let upgraded_registry = fs::read(&registry_path).unwrap();
    #[derive(serde::Deserialize, Clone, Copy)]
    #[serde(rename_all = "snake_case")]
    enum M6AgentId {
        ClaudeCode,
        Codex,
    }
    #[derive(serde::Deserialize)]
    struct M6Session {
        agent_id: M6AgentId,
    }
    #[derive(serde::Deserialize)]
    struct M6Project {
        default_agent: Option<M6AgentId>,
    }
    #[derive(serde::Deserialize)]
    struct M6Registry {
        sessions: Vec<M6Session>,
        projects: Vec<M6Project>,
    }
    let decoded: M6Registry = serde_json::from_slice(&upgraded_registry).unwrap();
    assert!(
        decoded
            .sessions
            .iter()
            .all(|session| matches!(session.agent_id, M6AgentId::ClaudeCode | M6AgentId::Codex))
    );
    assert!(decoded.projects.iter().all(|project| matches!(
        project.default_agent,
        None | Some(M6AgentId::ClaudeCode | M6AgentId::Codex)
    )));
    assert!(
        upgraded
            .sessions_extended(&TerminalService::default(), false)
            .iter()
            .any(|session| session.session.id == id)
    );
    assert!(
        upgraded
            .launcher_extended()
            .projects
            .iter()
            .any(|project| project.path == world.home.to_string_lossy()
                && project.default_agent == Some(AgentId::Opencode))
    );
    assert!(
        upgraded
            .launcher()
            .projects
            .iter()
            .all(|project| project.default_agent.is_none_or(AgentId::is_legacy))
    );
}

#[test]
fn a_legacy_provider_replaces_the_previous_m9_project_default() {
    let world = World::new();
    let opencode = world.start(AgentId::Opencode, None).unwrap();
    let project = world.home.to_string_lossy().into_owned();
    assert_eq!(
        world
            .service
            .launcher_extended()
            .projects
            .iter()
            .find(|entry| entry.path == project)
            .unwrap()
            .default_agent,
        Some(AgentId::Opencode)
    );

    let claude = world.start(AgentId::ClaudeCode, None).unwrap();
    assert_eq!(
        world
            .service
            .launcher_extended()
            .projects
            .iter()
            .find(|entry| entry.path == project)
            .unwrap()
            .default_agent,
        Some(AgentId::ClaudeCode),
        "a legacy selection must not be shadowed by an old sidecar default"
    );
    world
        .service
        .stop(&world.terminal, &opencode.session.session.id)
        .unwrap();
    world
        .service
        .stop(&world.terminal, &claude.session.session.id)
        .unwrap();
}

#[test]
fn legacy_launch_without_m9_default_ignores_sidecar_write_failure() {
    let world = World::new();
    let sidecar = world.base.join("registry/m9-native-sessions-v1.json");
    let original = fs::read(&sidecar).unwrap();
    // The service has already loaded valid M9 metadata. A concurrent filesystem failure
    // must not create a new dependency for a legacy launch with nothing to clear.
    fs::remove_file(&sidecar).unwrap();
    fs::create_dir(&sidecar).unwrap();

    let legacy = world.start(AgentId::ClaudeCode, None).unwrap();
    let output = world.output_until(&legacy.terminal.terminal_id, "FAKE-CLAUDE");
    assert!(output.contains("FAKE-CLAUDE"), "{output:?}");
    world
        .service
        .stop(&world.terminal, &legacy.session.session.id)
        .unwrap();
    fs::remove_dir(&sidecar).unwrap();
    fs::write(&sidecar, original).unwrap();
}

#[test]
fn legacy_launch_with_existing_m9_default_refuses_unpersistable_clear() {
    let world = World::new();
    let opencode = world.start(AgentId::Opencode, None).unwrap();
    let project = world.home.to_string_lossy().into_owned();
    let sidecar = world.base.join("registry/m9-native-sessions-v1.json");
    let original = fs::read(&sidecar).unwrap();
    fs::remove_file(&sidecar).unwrap();
    fs::create_dir(&sidecar).unwrap();

    let rejected = world.start(AgentId::ClaudeCode, None);
    assert_eq!(rejected.unwrap_err().code, "store_unavailable");
    assert_eq!(
        world
            .service
            .launcher_extended()
            .projects
            .iter()
            .find(|entry| entry.path == project)
            .unwrap()
            .default_agent,
        Some(AgentId::Opencode),
        "failed persistence must not silently discard an M9 selection"
    );

    fs::remove_dir(&sidecar).unwrap();
    fs::write(&sidecar, original).unwrap();
    world
        .service
        .stop(&world.terminal, &opencode.session.session.id)
        .unwrap();
}

#[test]
fn a_stop_that_cannot_be_persisted_is_reported_and_withdraws_m9_readiness() {
    let world = World::new();
    let running = world.start(AgentId::Opencode, None).unwrap();
    let id = running.session.session.id.clone();
    let sidecar = world.base.join("registry/m9-native-sessions-v1.json");
    let original = fs::read(&sidecar).unwrap();
    // A directory where the file should be makes the replacing rename fail before it happens,
    // so the authoritative file still says running.
    fs::remove_file(&sidecar).unwrap();
    fs::create_dir(&sidecar).unwrap();

    assert_eq!(
        world.service.stop(&world.terminal, &id).unwrap_err().code,
        "store_unavailable"
    );
    // The process ended, and memory records that. Nothing claims the ending is durable.
    assert_eq!(
        world.service.session_record(&id).unwrap().state,
        State::Stopped
    );
    assert_ne!(
        world.service.launcher_extended().m9_native_status,
        "ready",
        "a failed finish write must not advertise M9 storage as ready"
    );

    fs::remove_dir(&sidecar).unwrap();
    fs::write(&sidecar, &original).unwrap();
    // A reopen re-reads the authoritative file, which says running, and recovers the session
    // as lost. Nothing is started again.
    let reopened = World::open(&world.base, &world.home, &world.bin);
    assert_eq!(reopened.session_record(&id).unwrap().state, State::Lost);
}

#[test]
fn a_failed_clear_in_one_project_does_not_block_legacy_launch_in_another() {
    let world = World::new();
    let opencode = world.start(AgentId::Opencode, None).unwrap();
    let sidecar = world.base.join("registry/m9-native-sessions-v1.json");
    let original = fs::read(&sidecar).unwrap();
    fs::remove_file(&sidecar).unwrap();
    fs::create_dir(&sidecar).unwrap();

    // Project A still has the M9 default that could not be removed durably.
    assert_eq!(
        world.start(AgentId::ClaudeCode, None).unwrap_err().code,
        "store_unavailable"
    );

    // Project B has no M9 default to clear, so its legacy launch does not depend on the sidecar.
    let other = world.home.join("unrelated-project");
    fs::create_dir(&other).unwrap();
    let project = other.to_string_lossy().into_owned();
    world.service.remember_chosen(project.clone());
    let legacy = world
        .service
        .start(
            &world.terminal,
            StartRequest {
                arsenal: None,
                delegation: false,
                agent_id: AgentId::ClaudeCode,
                project,
                profile_id: None,
                size: SIZE,
                brief: None,
                plan_token: None,
                expected_cli_version: None,
            },
        )
        .unwrap();
    let output = world.output_until(&legacy.terminal.terminal_id, "FAKE-CLAUDE");
    assert!(output.contains("FAKE-CLAUDE"), "{output:?}");

    // The fence stays on project A while its default remains unresolved.
    assert_eq!(
        world.start(AgentId::ClaudeCode, None).unwrap_err().code,
        "store_unavailable"
    );

    world
        .service
        .stop(&world.terminal, &legacy.session.session.id)
        .unwrap();
    fs::remove_dir(&sidecar).unwrap();
    fs::write(&sidecar, &original).unwrap();
    world
        .service
        .stop(&world.terminal, &opencode.session.session.id)
        .unwrap();
}

#[test]
fn a_forget_that_cannot_be_written_keeps_the_record() {
    let world = World::new();
    let opencode = world.start(AgentId::Opencode, None).unwrap();
    let id = opencode.session.session.id.clone();
    world.service.stop(&world.terminal, &id).unwrap();
    let sidecar = world.base.join("registry/m9-native-sessions-v1.json");
    let original = fs::read(&sidecar).unwrap();
    fs::remove_file(&sidecar).unwrap();
    fs::create_dir(&sidecar).unwrap();

    assert_eq!(
        world.service.forget(&id).unwrap_err().code,
        "store_unavailable"
    );
    assert!(
        world.service.session_record(&id).is_some(),
        "a removal that was not written must not hide the record"
    );

    fs::remove_dir(&sidecar).unwrap();
    fs::write(&sidecar, &original).unwrap();
    world.service.forget(&id).unwrap();
    assert!(world.service.session_record(&id).is_none());
}

#[test]
fn failed_m9_default_write_preserves_previous_default_and_reports_storage_unavailable() {
    let world = World::new();
    let opencode = world.start(AgentId::Opencode, None).unwrap();
    let project = world.home.to_string_lossy().into_owned();
    let sidecar = world.base.join("registry/m9-native-sessions-v1.json");
    let original = fs::read(&sidecar).unwrap();
    fs::remove_file(&sidecar).unwrap();
    fs::create_dir(&sidecar).unwrap();

    // Hermes may start because the terminal effect already succeeded, but its failed optional
    // default write must not replace OpenCode in memory or be reported as durable.
    let hermes = world.start(AgentId::Hermes, None).unwrap();
    assert!(
        world
            .output_until(&hermes.terminal.terminal_id, "FAKE-HERMES")
            .contains("FAKE-HERMES")
    );
    let launcher = world.service.launcher_extended();
    assert_eq!(
        launcher
            .projects
            .iter()
            .find(|entry| entry.path == project)
            .unwrap()
            .default_agent,
        Some(AgentId::Opencode)
    );
    assert_eq!(launcher.m9_native_status, "storage_unavailable");
    assert_eq!(
        world
            .service
            .session_record(&hermes.session.session.id)
            .map(|session| session.state),
        Some(State::Running),
        "a live provider remains manageable even when its sidecar record was rejected"
    );
    assert!(
        world
            .service
            .sessions_extended(&world.terminal, false)
            .iter()
            .any(|entry| entry.session.id == hermes.session.session.id)
    );

    world
        .service
        .stop(&world.terminal, &hermes.session.session.id)
        .unwrap();
    assert_eq!(
        world
            .service
            .session_record(&hermes.session.session.id)
            .map(|session| session.state),
        Some(State::Stopped),
        "stopping a volatile M9 session must retain its terminal state in memory"
    );
    fs::remove_dir(&sidecar).unwrap();
    fs::write(&sidecar, original).unwrap();
    let reopened = World::open(&world.base, &world.home, &world.bin);
    assert_eq!(
        reopened
            .launcher_extended()
            .projects
            .iter()
            .find(|entry| entry.path == project)
            .unwrap()
            .default_agent,
        Some(AgentId::Opencode),
        "restarting must preserve the last successfully persisted default"
    );
    world
        .service
        .stop(&world.terminal, &opencode.session.session.id)
        .unwrap();
}

#[test]
fn a_corrupt_m9_sidecar_blocks_only_m9_and_keeps_legacy_state_writable() {
    let world = World::new();
    let sidecar_path = world.base.join("registry/m9-native-sessions-v1.json");
    let corrupt = b"{ intentionally corrupt M9 metadata";
    fs::write(&sidecar_path, corrupt).unwrap();

    let reopened = World {
        base: world.base.clone(),
        home: world.home.clone(),
        bin: world.bin.clone(),
        terminal: TerminalService::default(),
        service: World::open(&world.base, &world.home, &world.bin),
    };
    let launcher = reopened.service.launcher_extended();
    assert_eq!(launcher.m9_native_status, "storage_unavailable");
    assert!(
        launcher
            .agents
            .iter()
            .all(|entry| entry.agent_id.is_legacy())
    );
    assert_eq!(fs::read(&sidecar_path).unwrap(), corrupt);

    let blocked = reopened.service.start(
        &reopened.terminal,
        StartRequest {
            arsenal: None,
            delegation: false,
            agent_id: AgentId::Opencode,
            project: reopened.home.to_string_lossy().into_owned(),
            profile_id: None,
            size: SIZE,
            brief: None,
            plan_token: None,
            expected_cli_version: Some("1.19.4".to_owned()),
        },
    );
    assert_eq!(blocked.unwrap_err().code, "store_unavailable");

    let legacy = reopened.start(AgentId::ClaudeCode, None).unwrap();
    let output = reopened.output_until(&legacy.terminal.terminal_id, "FAKE-CLAUDE");
    assert!(output.contains("FAKE-CLAUDE"), "{output:?}");
    assert_eq!(fs::read(&sidecar_path).unwrap(), corrupt);
    let registry: serde_json::Value =
        serde_json::from_slice(&fs::read(world.base.join("registry/registry.json")).unwrap())
            .unwrap();
    assert!(
        registry["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|session| {
                session["agent_id"] == "claude_code" && session["id"] == legacy.session.session.id
            })
    );
    reopened
        .service
        .stop(&reopened.terminal, &legacy.session.session.id)
        .unwrap();
    assert!(
        !world
            .base
            .join("registry/m9-native-default-overrides-v2.json")
            .exists(),
        "a legacy launch with no recovered M9 default does not spend an override slot"
    );
}

#[test]
fn a_malformed_v1_override_blocks_m9_and_preserves_legacy_bytes() {
    let world = World::new();
    let legacy_overrides_path = world
        .base
        .join("registry/m9-native-default-overrides-v1.json");
    let malformed = b"{ malformed v1 override";
    fs::write(&legacy_overrides_path, malformed).unwrap();

    let reopened = World {
        base: world.base.clone(),
        home: world.home.clone(),
        bin: world.bin.clone(),
        terminal: TerminalService::default(),
        service: World::open(&world.base, &world.home, &world.bin),
    };
    assert_eq!(
        reopened.service.launcher_extended().m9_native_status,
        "storage_unavailable"
    );
    assert_eq!(fs::read(&legacy_overrides_path).unwrap(), malformed);
    assert!(
        !world
            .base
            .join("registry/m9-native-default-overrides-v2.json")
            .exists(),
        "invalid legacy state is preserved rather than partially migrated"
    );

    let legacy = reopened.start(AgentId::ClaudeCode, None).unwrap();
    assert_eq!(fs::read(&legacy_overrides_path).unwrap(), malformed);
    reopened
        .service
        .stop(&reopened.terminal, &legacy.session.session.id)
        .unwrap();
}

#[test]
fn a_capacity_failed_recovery_durably_preserves_interim_m9_records_across_restart() {
    let world = World::new();
    let opencode = world.start(AgentId::Opencode, None).unwrap();
    let session_id = opencode.session.session.id.clone();
    let registry_dir = world.base.join("registry");
    let registry_path = registry_dir.join("registry.json");
    let mut interim: serde_json::Value =
        serde_json::from_slice(&fs::read(&registry_path).unwrap()).unwrap();
    let mut sidecar: serde_json::Value =
        serde_json::from_slice(&fs::read(registry_dir.join("m9-native-sessions-v1.json")).unwrap())
            .unwrap();
    let sidecar_sessions = sidecar["sessions"].as_array_mut().unwrap();
    let at = sidecar_sessions
        .iter()
        .position(|session| session["id"] == session_id)
        .unwrap();
    interim["sessions"]
        .as_array_mut()
        .unwrap()
        .push(sidecar_sessions.remove(at));
    let home_path = world.home.to_string_lossy().into_owned();
    let home_project = interim["projects"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|project| project["path"] == home_path)
        .unwrap();
    home_project["default_agent"] = serde_json::json!("opencode");
    fs::write(&registry_path, serde_json::to_vec(&interim).unwrap()).unwrap();
    fs::write(
        registry_dir.join("m9-native-sessions-v1.json"),
        serde_json::to_vec(&sidecar).unwrap(),
    )
    .unwrap();

    // Fill the healthy sidecar to its exact project-default ceiling, then add one durable
    // recovery default so migration fails before it reaches the shared-registry records.
    let mut extended = ExtendedRegistry::open(registry_dir.clone()).unwrap();
    let mut defaults = extended.recovery_snapshot().project_defaults;
    let mut candidate = 0;
    while defaults.len() < super::registry::MAX_PROJECTS {
        let path = format!("/capacity-fill-{candidate}");
        candidate += 1;
        if defaults.contains_key(&path) {
            continue;
        }
        extended
            .set_project_default(&path, AgentId::Opencode)
            .unwrap();
        defaults.insert(path, AgentId::Opencode);
    }
    assert_eq!(defaults.len(), super::registry::MAX_PROJECTS);
    ExtendedRegistry::preserve_recovery(
        &registry_dir,
        &[],
        &[("/recovered-capacity-overflow".to_owned(), AgentId::Hermes)],
    )
    .unwrap();
    world
        .terminal
        .close(&opencode.terminal.terminal_id)
        .unwrap();

    let first = World::open(&world.base, &world.home, &world.bin);
    assert_eq!(
        first.launcher_extended().m9_native_status,
        "storage_unavailable"
    );
    let first_sessions = first.sessions_extended(&TerminalService::default(), false);
    assert!(first_sessions.iter().any(|session| {
        session.session.id == session_id
            && session.session.agent_id == AgentId::Opencode
            && session.session.state == State::Lost
    }));
    let recovery = ExtendedRegistry::read_recovery(&registry_dir).unwrap();
    assert!(recovery.snapshots.iter().any(|snapshot| {
        snapshot
            .sessions
            .iter()
            .any(|session| session.id == session_id)
    }));

    // A second crash/reopen still finds the interim session in durable recovery, even though the
    // new default remains over capacity and M9 stays explicitly unavailable.
    let second = World::open(&world.base, &world.home, &world.bin);
    assert_eq!(
        second.launcher_extended().m9_native_status,
        "storage_unavailable"
    );
    let second_sessions = second.sessions_extended(&TerminalService::default(), false);
    assert!(second_sessions.iter().any(|session| {
        session.session.id == session_id
            && session.session.agent_id == AgentId::Opencode
            && session.session.state == State::Lost
    }));
    let recovery = ExtendedRegistry::read_recovery(&registry_dir).unwrap();
    assert!(recovery.snapshots.iter().any(|snapshot| {
        snapshot
            .sessions
            .iter()
            .any(|session| session.id == session_id)
    }));
}

#[test]
fn a_corrupt_sidecar_migration_recovers_m9_metadata_before_sanitizing_the_m6_registry() {
    let world = World::new();
    let opencode = world.start(AgentId::Opencode, None).unwrap();
    let id = opencode.session.session.id.clone();
    let project_path = world.home.to_string_lossy().into_owned();

    let registry_path = world.base.join("registry/registry.json");
    let mut legacy_registry: serde_json::Value =
        serde_json::from_slice(&fs::read(&registry_path).unwrap()).unwrap();
    let sidecar_path = world.base.join("registry/m9-native-sessions-v1.json");
    let mut sidecar: serde_json::Value =
        serde_json::from_slice(&fs::read(&sidecar_path).unwrap()).unwrap();
    let sessions = sidecar["sessions"].as_array_mut().unwrap();
    let at = sessions
        .iter()
        .position(|session| session["id"] == id)
        .unwrap();
    legacy_registry["sessions"]
        .as_array_mut()
        .unwrap()
        .push(sessions.remove(at));
    let project = legacy_registry["projects"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|project| project["path"] == world.home.to_string_lossy().as_ref())
        .unwrap();
    project["default_agent"] = serde_json::json!("opencode");
    fs::write(
        &registry_path,
        serde_json::to_vec(&legacy_registry).unwrap(),
    )
    .unwrap();
    let corrupt = b"{ sidecar bytes remain untouched";
    fs::write(&sidecar_path, corrupt).unwrap();
    let legacy_overrides_path = world
        .base
        .join("registry/m9-native-default-overrides-v1.json");
    let overrides_path = world
        .base
        .join("registry/m9-native-default-overrides-v2.json");
    let other_project_key =
        ExtendedRegistry::default_override_key("/another-previously-overridden-project");
    fs::write(
        &overrides_path,
        serde_json::to_vec(&serde_json::json!({
            "schema": 2,
            "project_hashes": [other_project_key],
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        &legacy_overrides_path,
        serde_json::to_vec(&serde_json::json!({
            "schema": 1,
            "projects": [project_path],
        }))
        .unwrap(),
    )
    .unwrap();
    world
        .terminal
        .close(&opencode.terminal.terminal_id)
        .unwrap();

    let recovered = World::open(&world.base, &world.home, &world.bin);
    assert_eq!(fs::read(&sidecar_path).unwrap(), corrupt);
    assert!(
        !legacy_overrides_path.exists(),
        "v1 override bytes are retired only after their hashed v2 image is durable"
    );
    let overrides_path = world
        .base
        .join("registry/m9-native-default-overrides-v2.json");
    let migrated_overrides: serde_json::Value =
        serde_json::from_slice(&fs::read(&overrides_path).unwrap()).unwrap();
    let migrated_keys = migrated_overrides["project_hashes"].as_array().unwrap();
    assert!(
        migrated_keys.contains(&serde_json::json!(ExtendedRegistry::default_override_key(
            &project_path
        ))),
        "the legacy user's default replacement survives v1-to-v2 migration"
    );
    assert!(migrated_keys.contains(&serde_json::json!(other_project_key)));
    assert!(!String::from_utf8_lossy(&fs::read(&overrides_path).unwrap()).contains(&project_path));
    let safe_registry = fs::read(&registry_path).unwrap();
    #[derive(serde::Deserialize, Clone, Copy)]
    #[serde(rename_all = "snake_case")]
    enum M6AgentId {
        ClaudeCode,
        Codex,
    }
    #[derive(serde::Deserialize, Clone, Copy)]
    struct M6Session {
        agent_id: M6AgentId,
    }
    #[derive(serde::Deserialize)]
    struct M6Registry {
        sessions: Vec<M6Session>,
    }
    let m6_view: M6Registry = serde_json::from_slice(&safe_registry).unwrap();
    assert!(
        m6_view.sessions.iter().all(|session| {
            matches!(session.agent_id, M6AgentId::ClaudeCode | M6AgentId::Codex)
        })
    );
    assert_eq!(
        recovered.launcher_extended().m9_native_status,
        "storage_unavailable"
    );
    let sessions = recovered.sessions_extended(&TerminalService::default(), false);
    assert!(sessions.iter().any(|session| {
        session.session.id == id
            && session.session.agent_id == AgentId::Opencode
            && session.session.state == State::Lost
    }));
    let recovery_dir = world.base.join("registry/m9-native-recovery-v1");
    let recovery_files: Vec<_> = fs::read_dir(&recovery_dir).unwrap().collect();
    assert_eq!(recovery_files.len(), 1);
    let recovery_file = recovery_files[0].as_ref().unwrap().path();
    let immutable_snapshot = fs::read(&recovery_file).unwrap();

    let degraded = World {
        base: world.base.clone(),
        home: world.home.clone(),
        bin: world.bin.clone(),
        terminal: TerminalService::default(),
        service: recovered,
    };
    let claude = degraded.start(AgentId::ClaudeCode, None).unwrap();
    assert_eq!(
        degraded
            .service
            .launcher_extended()
            .projects
            .iter()
            .find(|project| project.path == project_path)
            .unwrap()
            .default_agent,
        Some(AgentId::ClaudeCode),
        "a later legacy-provider selection suppresses the recovered M9 default"
    );
    assert_eq!(
        fs::read(&recovery_file).unwrap(),
        immutable_snapshot,
        "the original bounded recovery snapshot remains byte-for-byte preserved"
    );
    let overrides: serde_json::Value =
        serde_json::from_slice(&fs::read(&overrides_path).unwrap()).unwrap();
    assert!(
        overrides["project_hashes"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!(ExtendedRegistry::default_override_key(
                &project_path
            ))),
        "the bounded override journal stores the project's digest, not its workspace path"
    );
    degraded
        .service
        .stop(&degraded.terminal, &claude.session.session.id)
        .unwrap();

    // Once the primary store is repaired, recovery migrates before the immutable copy is
    // removed. The durable default override prevents OpenCode from resurfacing, and Forget
    // must then remain forgotten after a further restart.
    fs::write(
        &sidecar_path,
        br#"{"schema":1,"sessions":[],"project_defaults":{}}"#,
    )
    .unwrap();
    let restored = World::open(&world.base, &world.home, &world.bin);
    assert_eq!(restored.launcher_extended().m9_native_status, "ready");
    assert_eq!(
        restored
            .launcher_extended()
            .projects
            .iter()
            .find(|project| project.path == project_path)
            .unwrap()
            .default_agent,
        Some(AgentId::ClaudeCode)
    );
    assert!(
        restored
            .sessions_extended(&TerminalService::default(), false)
            .iter()
            .any(|session| session.session.id == id)
    );
    assert_eq!(fs::read_dir(&recovery_dir).unwrap().count(), 0);
    assert!(!overrides_path.exists());
    restored.forget(&id).unwrap();
    let after_forget = World::open(&world.base, &world.home, &world.bin);
    assert!(
        after_forget
            .sessions_extended(&TerminalService::default(), false)
            .iter()
            .all(|session| session.session.id != id)
    );
}

#[test]
fn an_over_capacity_recovery_merge_blocks_only_m9_and_keeps_legacy_available() {
    let world = World::new();
    let registry_dir = world.base.join("registry");
    let mut extended = ExtendedRegistry::open(registry_dir.clone()).unwrap();
    for index in 0..super::registry::MAX_PROJECTS {
        extended
            .set_project_default(&format!("/fixture-project-{index}"), AgentId::Opencode)
            .unwrap();
    }
    ExtendedRegistry::preserve_recovery(
        &registry_dir,
        &[],
        &[("/recovered-project".to_owned(), AgentId::Hermes)],
    )
    .unwrap();

    let reopened = World {
        base: world.base.clone(),
        home: world.home.clone(),
        bin: world.bin.clone(),
        terminal: TerminalService::default(),
        service: World::open(&world.base, &world.home, &world.bin),
    };
    let launcher = reopened.service.launcher_extended();
    assert_eq!(launcher.m9_native_status, "storage_unavailable");
    assert!(
        launcher
            .agents
            .iter()
            .all(|entry| entry.agent_id.is_legacy())
    );

    let claude = reopened.start(AgentId::ClaudeCode, None).unwrap();
    assert!(
        reopened
            .output_until(&claude.terminal.terminal_id, "FAKE-CLAUDE")
            .contains("FAKE-CLAUDE")
    );
    reopened
        .service
        .stop(&reopened.terminal, &claude.session.session.id)
        .unwrap();
}

#[test]
fn hermes_native_launches_as_configured_without_probe_or_automatic_handoff() {
    let world = World::new();
    let entry = world
        .service
        .launcher_extended()
        .agents
        .into_iter()
        .find(|entry| entry.agent_id == AgentId::Hermes)
        .unwrap();
    assert!(entry.installed);
    assert_eq!(
        entry.version, None,
        "version probe would perform update checks"
    );

    let reply = world.start(AgentId::Hermes, None).unwrap();
    let out = world.output_until(&reply.terminal.terminal_id, "FAKE-HERMES");
    assert!(out.contains("FAKE-HERMES argc=0"), "{out:?}");
    assert!(!out.contains("UNEXPECTED-HERMES-VERSION-PROBE"), "{out:?}");
    world
        .service
        .stop(&world.terminal, &reply.session.session.id)
        .unwrap();

    let error = world
        .service
        .start(
            &world.terminal,
            StartRequest {
                arsenal: None,
                delegation: false,
                agent_id: AgentId::Hermes,
                project: world.home.to_string_lossy().into_owned(),
                profile_id: None,
                size: SIZE,
                brief: Some("# Do not submit automatically".to_owned()),
                plan_token: None,
                expected_cli_version: None,
            },
        )
        .unwrap_err();
    assert_eq!(error.code, "handoff_target_unverified");

    let profile_id = world
        .service
        .profiles()
        .create(Body {
            name: "Hermes model".to_owned(),
            applies_to: vec![AgentId::Hermes],
            model: Some("provider/model".to_owned()),
            ..Body::default()
        })
        .unwrap();
    let error = world.start(AgentId::Hermes, Some(&profile_id)).unwrap_err();
    assert_eq!(error.code, "profile_not_applicable");
}

#[test]
fn stopping_ends_the_process_and_restarting_makes_a_new_session_of_the_same_kind() {
    let world = World::new();
    let first = world
        .start(AgentId::ClaudeCode, Some("template-review"))
        .unwrap();
    let id = first.session.session.id.clone();
    world.output_until(&first.terminal.terminal_id, "SESSION=");
    world.service.stop(&world.terminal, &id).unwrap();
    assert_eq!(
        world.service.stop(&world.terminal, &id).unwrap_err().code,
        "not_running"
    );
    let stopped = world.wait_state(&id, State::Stopped);
    assert!(stopped.session.ended_at.is_some());

    let preview = world
        .service
        .preview(AgentId::ClaudeCode, Some("template-review"), None)
        .unwrap();
    let second = world
        .service
        .restart(
            &world.terminal,
            &id,
            SIZE,
            preview.plan_token,
            Some("2.1.309 (Claude Code)".to_owned()),
        )
        .unwrap();
    assert_ne!(second.session.session.id, id);
    assert_eq!(
        second.session.session.restart_of.as_deref(),
        Some(id.as_str())
    );
    assert_eq!(second.session.session.restarts, 1);
    assert_eq!(
        second.session.session.profile.as_ref().unwrap().id,
        "template-review"
    );
    let out = world.output_until(&second.terminal.terminal_id, "SESSION=");
    // The profile's start flags reach the agent as words.
    assert!(
        out.contains("[--permission-mode]") && out.contains("[plan]"),
        "{out:?}"
    );
    // Both records are kept; a running one cannot be forgotten.
    assert_eq!(world.service.sessions(&world.terminal, false).len(), 2);
    assert_eq!(
        world
            .service
            .forget(&second.session.session.id)
            .unwrap_err()
            .code,
        "still_running"
    );
}

#[test]
fn a_stale_restart_preview_leaves_the_existing_native_session_running() {
    let world = World::new();
    let body = Body {
        name: "Restart model".to_owned(),
        applies_to: vec![AgentId::ClaudeCode],
        model: Some("sonnet".to_owned()),
        ..Body::default()
    };
    let profile_id = world.service.profiles().create(body.clone()).unwrap();
    let first = world.start(AgentId::ClaudeCode, Some(&profile_id)).unwrap();
    let id = first.session.session.id.clone();
    world.output_until(&first.terminal.terminal_id, "SESSION=");
    let preview = world
        .service
        .preview(AgentId::ClaudeCode, Some(&profile_id), None)
        .unwrap();
    let mut changed = body;
    changed.model = Some("opus".to_owned());
    world.service.profiles().save(&profile_id, changed).unwrap();

    let error = world
        .service
        .restart(
            &world.terminal,
            &id,
            SIZE,
            preview.plan_token,
            Some("2.1.309 (Claude Code)".to_owned()),
        )
        .unwrap_err();
    assert_eq!(error.code, "profile_preview_stale");
    let old = world
        .service
        .sessions(&world.terminal, false)
        .into_iter()
        .find(|session| session.session.id == id)
        .unwrap();
    assert_eq!(old.session.state, State::Running);
}

#[test]
fn a_running_native_session_is_stopped_only_after_its_replacement_starts() {
    let world = World::new();
    let first = world.start(AgentId::ClaudeCode, None).unwrap();
    let id = first.session.session.id.clone();
    world.output_until(&first.terminal.terminal_id, "SESSION=");

    let replacement = world
        .service
        .restart(&world.terminal, &id, SIZE, None, None)
        .unwrap();
    assert_ne!(replacement.session.session.id, id);
    let old = world
        .service
        .sessions(&world.terminal, false)
        .into_iter()
        .find(|session| session.session.id == id)
        .unwrap();
    assert_eq!(old.session.state, State::Stopped);
    assert_eq!(replacement.session.session.state, State::Running);
    assert_eq!(
        replacement.session.session.restart_of.as_deref(),
        Some(id.as_str())
    );
}

#[test]
fn a_shell_version_mismatch_during_restart_keeps_the_previous_session_running() {
    let world = World::new();
    let profile_id = world
        .service
        .profiles()
        .create(Body {
            name: "Pinned model".to_owned(),
            applies_to: vec![AgentId::ClaudeCode],
            model: Some("sonnet".to_owned()),
            ..Body::default()
        })
        .unwrap();
    let first = world.start(AgentId::ClaudeCode, Some(&profile_id)).unwrap();
    let id = first.session.session.id.clone();
    world.output_until(&first.terminal.terminal_id, "SESSION=");
    let preview = world
        .service
        .preview(AgentId::ClaudeCode, Some(&profile_id), None)
        .unwrap();

    let changed_bin = world.base.parent().unwrap().join("changed-bin");
    fs::create_dir_all(&changed_bin).unwrap();
    let started = changed_bin.join("provider-started");
    script(
        &changed_bin.join("claude"),
        &format!(
            "[ \"$1\" = \"--version\" ] && {{ echo '2.1.310 (Claude Code)'; exit 0; }}\nprintf started > '{}'\n",
            started.display()
        ),
    );
    script(
        &world.bin.join("bash"),
        &format!(
            "export PATH='{}':\"$PATH\"\nexec /usr/bin/bash --norc --noprofile \"$@\"",
            changed_bin.display()
        ),
    );

    let error = world
        .service
        .restart(
            &world.terminal,
            &id,
            SIZE,
            preview.plan_token,
            Some("2.1.309 (Claude Code)".to_owned()),
        )
        .unwrap_err();
    assert_eq!(error.code, "provider_version_unverified");
    assert!(
        !started.exists(),
        "mismatched CLI must not receive profile flags"
    );
    let old = world
        .service
        .sessions(&world.terminal, false)
        .into_iter()
        .find(|session| session.session.id == id)
        .unwrap();
    assert_eq!(old.session.state, State::Running);
    assert!(
        world
            .terminal
            .status(&first.terminal.terminal_id)
            .unwrap()
            .state
            == wyss_console::terminal::contracts::TerminalState::Running
    );
}

#[test]
fn only_a_folder_the_user_chose_or_a_known_project_can_be_a_project() {
    let world = World::new();
    let elsewhere = world.home.parent().unwrap().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    let request = |path: &Path| StartRequest {
        arsenal: None,
        delegation: false,
        agent_id: AgentId::Codex,
        project: path.to_string_lossy().into_owned(),
        profile_id: None,
        size: SIZE,
        brief: None,
        plan_token: None,
        expected_cli_version: None,
    };
    assert_eq!(
        world
            .service
            .start(&world.terminal, request(&elsewhere))
            .unwrap_err()
            .code,
        "project_invalid"
    );
    assert_eq!(
        world
            .service
            .start(&world.terminal, request(Path::new("/etc/../etc")))
            .unwrap_err()
            .code,
        "project_invalid"
    );
    world
        .service
        .remember_chosen(elsewhere.to_string_lossy().into_owned());
    world
        .service
        .start(&world.terminal, request(&elsewhere))
        .unwrap();
    // Having started there, it is a known project, offered with the agent it was used with.
    let projects = world.service.launcher().projects;
    let project = projects
        .iter()
        .find(|p| p.path == elsewhere.to_string_lossy())
        .unwrap();
    assert_eq!(project.default_agent, Some(AgentId::Codex));
    assert!(projects[0].home);
}

#[test]
fn a_profile_the_agent_cannot_apply_refuses_to_start_and_starts_nothing() {
    let world = World::new();
    let id = world
        .service
        .profiles()
        .create(Body {
            name: "Hooks off".to_owned(),
            applies_to: vec![AgentId::ClaudeCode, AgentId::Codex],
            hooks: Some(HooksMode::Off),
            ..Body::default()
        })
        .unwrap();
    let refused = world.start(AgentId::Codex, Some(&id)).unwrap_err();
    assert_eq!(refused.code, "profile_not_applicable");
    assert!(refused.reasons[0].contains("Codex hooks"));
    assert!(world.service.sessions(&world.terminal, false).is_empty());
    let preview = world
        .service
        .preview(AgentId::Codex, Some(&id), None)
        .unwrap();
    assert!(!preview.can_start);
    // The same profile is fine for the agent that can do it, and says what it costs.
    let preview = world
        .service
        .preview(AgentId::ClaudeCode, Some(&id), None)
        .unwrap();
    assert!(preview.can_start && preview.with_cost[0].contains("this session only"));
    // A profile for another agent is not applicable either.
    let only_claude = world
        .service
        .profiles()
        .create(Body {
            name: "Claude only".to_owned(),
            applies_to: vec![AgentId::ClaudeCode],
            ..Body::default()
        })
        .unwrap();
    assert_eq!(
        world
            .start(AgentId::Codex, Some(&only_claude))
            .unwrap_err()
            .code,
        "profile_not_applicable"
    );
}

#[test]
fn a_profile_change_after_preview_refuses_the_stale_start_without_creating_a_session() {
    let world = World::new();
    let mut body = Body {
        name: "Pinned model".to_owned(),
        applies_to: vec![AgentId::ClaudeCode],
        model: Some("sonnet".to_owned()),
        ..Body::default()
    };
    let id = world.service.profiles().create(body.clone()).unwrap();
    let preview = world
        .service
        .preview(AgentId::ClaudeCode, Some(&id), None)
        .unwrap();
    body.model = Some("opus".to_owned());
    world.service.profiles().save(&id, body).unwrap();

    let error = world
        .service
        .start(
            &world.terminal,
            StartRequest {
                arsenal: None,
                delegation: false,
                agent_id: AgentId::ClaudeCode,
                project: world.home.to_string_lossy().into_owned(),
                profile_id: Some(id),
                size: SIZE,
                brief: None,
                plan_token: preview.plan_token,
                expected_cli_version: Some("2.1.309 (Claude Code)".to_owned()),
            },
        )
        .unwrap_err();
    assert_eq!(error.code, "profile_preview_stale");
    assert!(world.service.sessions(&world.terminal, false).is_empty());
}

#[test]
fn a_profile_writes_its_overlay_under_the_sessions_own_directory_and_never_elsewhere() {
    let world = World::new();
    let id = world
        .service
        .profiles()
        .create(Body {
            name: "Quiet".to_owned(),
            applies_to: vec![AgentId::ClaudeCode],
            hooks: Some(HooksMode::Off),
            skills_off: vec!["noisy".to_owned()],
            model: Some("sonnet".to_owned()),
            permission: Some(Permission::Ask),
            ..Body::default()
        })
        .unwrap();
    let before = fs::read_dir(&world.home).unwrap().count();
    let reply = world.start(AgentId::ClaudeCode, Some(&id)).unwrap();
    let out = world.output_until(&reply.terminal.terminal_id, "OVERLAY=");
    assert!(
        out.contains("[--model]") && out.contains("[sonnet]"),
        "{out:?}"
    );
    let overlay_path = world
        .base
        .join("sessions")
        .join(&reply.session.session.id)
        .join("settings.json");
    assert!(
        out.contains(&format!("[{}]", overlay_path.display())),
        "{out:?}"
    );
    let overlay: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&overlay_path).unwrap()).unwrap();
    assert_eq!(overlay["disableAllHooks"], true);
    assert_eq!(overlay["skillOverrides"]["noisy"], "off");
    assert_eq!(
        fs::metadata(&overlay_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(overlay_path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    // Nothing was written into the project or the provider's home.
    assert_eq!(fs::read_dir(&world.home).unwrap().count(), before);
    // The files of a session end with it.
    world
        .service
        .stop(&world.terminal, &reply.session.session.id)
        .unwrap();
    assert!(!overlay_path.exists());
}

#[test]
fn after_a_restart_of_the_console_what_was_running_is_lost_not_exited() {
    let world = World::new();
    let reply = world.start(AgentId::ClaudeCode, None).unwrap();
    let id = reply.session.session.id.clone();
    world.output_until(&reply.terminal.terminal_id, "SESSION=");
    // The window dies without closing anything.
    let reopened = World::open(&world.base, &world.home, &world.bin);
    let sessions = reopened.sessions(&TerminalService::default(), false);
    let session = sessions.iter().find(|s| s.session.id == id).unwrap();
    assert_eq!(session.session.state, State::Lost);
    assert!(session.terminal_id.is_none() && session.session.ended_at.is_none());
    // Its generated files are gone and it can be forgotten or restarted as a new session.
    assert!(!world.base.join("sessions").join(&id).exists());
    reopened.forget(&id).unwrap();
}

#[test]
fn resources_are_sampled_only_when_asked() {
    let world = World::new();
    let reply = world.start(AgentId::ClaudeCode, None).unwrap();
    world.output_until(&reply.terminal.terminal_id, "SESSION=");
    let idle = world.service.sessions(&world.terminal, false);
    assert!(idle[0].usage.is_none());
    let first = world.service.sessions(&world.terminal, true);
    let usage = first[0].usage.unwrap();
    assert!(usage.rss_bytes > 0 && usage.processes >= 1);
    assert!(usage.cpu_percent.is_none(), "one sample has no rate yet");
    std::thread::sleep(Duration::from_millis(300));
    let second = world.service.sessions(&world.terminal, true);
    assert!(second[0].usage.unwrap().cpu_percent.is_some());
}

#[test]
fn a_request_with_an_unknown_field_or_a_free_command_is_not_a_start_request() {
    let parsed: Result<StartRequest, _> = serde_json::from_str(
        r#"{"agent_id":"codex","project":"/","profile_id":null,"size":{"rows":24,"cols":80},"command":"rm -rf /"}"#,
    );
    assert!(parsed.is_err());
    let parsed: Result<StartRequest, _> = serde_json::from_str(
        r#"{"agent_id":"/bin/sh","project":"/","profile_id":null,"size":{"rows":24,"cols":80}}"#,
    );
    assert!(
        parsed.is_err(),
        "an agent is an identifier of the catalogue, not a program"
    );
}

#[test]
fn closing_the_tab_of_an_agent_is_a_stop_not_a_failure() {
    let world = World::new();
    let reply = world.start(AgentId::ClaudeCode, None).unwrap();
    let id = reply.session.session.id.clone();
    world.output_until(&reply.terminal.terminal_id, "SESSION=");
    // What the window's close button does: it closes the terminal itself.
    world.terminal.close(&reply.terminal.terminal_id).unwrap();
    let view = world.wait_state(&id, State::Stopped);
    assert!(
        view.terminal_id.is_none(),
        "nothing is observed for a finished session"
    );
    assert!(!world.base.join("sessions").join(&id).exists());
}

#[test]
fn an_agent_does_not_inherit_the_markers_of_a_session_the_console_was_started_from() {
    let world = World::new();
    // The Console's own environment holds the markers of a running agent session, and one
    // variable of the user's that is not a marker.
    // SAFETY: only this test uses these two names, and the terminal reads the environment
    // through Rust's own locked accessors when it builds the child's.
    unsafe {
        std::env::set_var("WYSS_SCRUB_TEST_MARKER", "leaked");
        std::env::set_var("WYSS_SCRUB_TEST_KEPT", "kept");
    }
    script(
        &world.bin.join("claude"),
        "echo \"MARK=[$WYSS_SCRUB_TEST_MARKER] KEPT=[$WYSS_SCRUB_TEST_KEPT]\"; exec cat",
    );
    let reply = world.start(AgentId::ClaudeCode, None).unwrap();
    let out = world.output_until(&reply.terminal.terminal_id, "KEPT=");
    assert!(out.contains("MARK=[]"), "{out:?}");
    assert!(
        out.contains("KEPT=[kept]"),
        "the user's own variables stay: {out:?}"
    );
    unsafe {
        std::env::remove_var("WYSS_SCRUB_TEST_MARKER");
        std::env::remove_var("WYSS_SCRUB_TEST_KEPT");
    }
}

#[test]
fn the_markers_are_provider_session_variables_and_not_the_users_configuration() {
    use super::service::SESSION_MARKERS;
    for kept in [
        "CLAUDE_CONFIG_DIR",
        "ANTHROPIC_API_KEY",
        "CODEX_HOME",
        "CLAUDE_CODE_USE_BEDROCK",
        "PATH",
        "HOME",
    ] {
        assert!(!SESSION_MARKERS.contains(&kept), "{kept}");
    }
    for marker in [
        "CLAUDE_CODE_MESSAGING_TOKEN",
        "CLAUDE_CODE_SESSION_ID",
        "CLAUDECODE",
    ] {
        assert!(SESSION_MARKERS.contains(&marker), "{marker}");
    }
}

/// The installed Claude Code and Codex, started through the user's real shell in real
/// terminals, in a folder of their own: each must reach its own first screen (the folder
/// trust question, which nothing here answers), carry the session's marker and none of the
/// markers of a session the test runs inside, and stop on Stop. Nothing is sent to either, so
/// no model is called. By hand: `cargo test -- --ignored real_agents --nocapture`.
#[test]
#[ignore = "starts the installed Claude Code and Codex"]
fn real_agents_start_in_native_terminals_and_stop() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!(
            "wyss-real-agents-{}",
            uuid::Uuid::new_v4().simple()
        ));
    let (home, state, project) = (
        PathBuf::from(std::env::var_os("HOME").unwrap()),
        root.join("state"),
        root.join("project"),
    );
    fs::create_dir_all(&project).unwrap();
    let service = NativeService::open(state, home, std::env::var_os("PATH")).unwrap();
    service.remember_chosen(project.to_string_lossy().into_owned());
    let terminal = TerminalService::default();
    let launcher = service.launcher();
    println!("shell: {:?}", launcher.shell);
    for (agent, screen) in [
        (AgentId::ClaudeCode, "Accessing"),
        (AgentId::Codex, "Folder"),
    ] {
        let entry = launcher
            .agents
            .iter()
            .find(|a| a.agent_id == agent)
            .unwrap();
        println!(
            "{} installed={} version={:?}",
            entry.name, entry.installed, entry.version
        );
        assert!(entry.installed);
        let reply = service
            .start(
                &terminal,
                StartRequest {
                    arsenal: None,
                    delegation: false,
                    agent_id: agent,
                    project: project.to_string_lossy().into_owned(),
                    profile_id: None,
                    size: SIZE,
                    brief: None,
                    plan_token: None,
                    expected_cli_version: None,
                },
            )
            .unwrap();
        let mut seen = String::new();
        let mut ack = None;
        let deadline = Instant::now() + Duration::from_secs(40);
        // Codex may first ask about a background server of a different version: its own
        // question about its own daemon, left unanswered, and the folder question can then
        // not be reached. Either screen proves the real agent is running in the terminal.
        while Instant::now() < deadline
            && !(seen.contains(screen) || seen.contains("Background server"))
        {
            let out = terminal
                .read(&reply.terminal.terminal_id, ack.as_ref())
                .unwrap();
            ack = out.batch_id.clone();
            seen.push_str(&String::from_utf8_lossy(&out.data));
            std::thread::sleep(Duration::from_millis(60));
        }
        assert!(
            seen.contains(screen) || seen.contains("Background server"),
            "{agent:?} never reached a screen of its own: {:.200}",
            seen
        );
        let pid = reply.terminal.observed_pid.unwrap();
        let environ = fs::read(format!("/proc/{pid}/environ")).unwrap();
        let environ = String::from_utf8_lossy(&environ);
        assert!(environ.contains("WYSS_NATIVE_SESSION_ID="));
        for marker in super::service::SESSION_MARKERS {
            if marker != "WYSS_NATIVE_SESSION_ID" {
                assert!(
                    !environ
                        .split('\0')
                        .any(|v| v.starts_with(&format!("{marker}="))),
                    "{marker} leaked into the agent"
                );
            }
        }
        let comm = fs::read_to_string(format!("/proc/{pid}/comm")).unwrap();
        println!(
            "{agent:?}: process {pid} is `{}`, first screen reached",
            comm.trim()
        );
        let usage = service.sessions(&terminal, true);
        let usage = service
            .sessions(&terminal, true)
            .into_iter()
            .chain(usage)
            .find(|s| s.session.id == reply.session.session.id)
            .unwrap()
            .usage
            .unwrap();
        println!(
            "  {} MiB, {} process(es)",
            usage.rss_bytes / 1048576,
            usage.processes
        );
        service.stop(&terminal, &reply.session.session.id).unwrap();
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists() || {
                std::thread::sleep(Duration::from_millis(500));
                !Path::new(&format!("/proc/{pid}")).exists()
            }
        );
    }
    let _ = terminal.shutdown();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_reviewed_brief_is_the_agents_first_message_as_one_word_and_a_bad_one_is_refused() {
    let world = World::new();
    let start = |brief: Option<&str>| {
        world.service.start(
            &world.terminal,
            StartRequest {
                arsenal: None,
                delegation: false,
                agent_id: AgentId::ClaudeCode,
                project: world.home.to_string_lossy().into_owned(),
                profile_id: None,
                size: SIZE,
                brief: brief.map(str::to_owned),
                plan_token: None,
                expected_cli_version: None,
            },
        )
    };
    let brief = "# Handoff from Codex\n\n$(touch /tmp/wyss-brief-injected); `x` \"quoted\" -n\n";
    let reply = start(Some(brief)).unwrap();
    let out = world.output_until(&reply.terminal.terminal_id, "SESSION=");
    assert!(out.contains("argc=1"), "{out:?}");
    assert!(out.contains("[# Handoff from Codex"), "{out:?}");
    assert!(!Path::new("/tmp/wyss-brief-injected").exists());
    for bad in [
        "- not a heading",
        "no heading",
        "# nul\0",
        &format!("# {}", "x".repeat(20_000)),
    ] {
        assert_eq!(
            start(Some(bad)).unwrap_err().code,
            "brief_invalid",
            "{bad:.12}"
        );
    }
}

fn arsenal_launch(off: &[&str]) -> super::arsenal::ArsenalLaunch {
    super::arsenal::ArsenalLaunch {
        selection: "6f1d2b8e-0000-4000-8000-00000000a001".to_owned(),
        selection_name: "Frontend".to_owned(),
        config_hash: "c".repeat(64),
        skills_off: off.iter().map(|s| (*s).to_owned()).collect(),
        skill_files_off: Vec::new(),
    }
}

fn arsenal_start(
    world: &World,
    launch: Option<super::arsenal::ArsenalLaunch>,
    plan_token: Option<String>,
    version: &str,
) -> Result<super::service::StartReply, super::service::NativeError> {
    world.service.start(
        &world.terminal,
        StartRequest {
            agent_id: AgentId::ClaudeCode,
            project: world.home.to_string_lossy().into_owned(),
            profile_id: None,
            size: SIZE,
            brief: None,
            plan_token,
            expected_cli_version: Some(version.to_owned()),
            arsenal: launch,
            delegation: false,
        },
    )
}

#[test]
fn an_arsenal_exclusion_is_refused_on_a_build_without_its_own_evidence() {
    let world = World::new();
    let launch = arsenal_launch(&["design"]);
    let preview = world
        .service
        .preview(AgentId::ClaudeCode, None, Some(&launch))
        .unwrap();
    // 2.1.309 has profile syntax evidence, which does not qualify skill exclusion.
    let error = arsenal_start(
        &world,
        Some(launch.clone()),
        preview.plan_token.clone(),
        "2.1.309 (Claude Code)",
    )
    .unwrap_err();
    assert_eq!(error.code, "arsenal_unqualified");
    // Without the preview's token the start is refused before any version is considered.
    let error = arsenal_start(&world, Some(launch), None, "2.1.309 (Claude Code)").unwrap_err();
    assert_eq!(error.code, "profile_preview_required");
    assert!(world.service.sessions(&world.terminal, false).is_empty());
}

#[test]
fn a_qualified_build_launches_with_only_the_exclusion_overlay_and_keeps_it_for_restart() {
    let world = World::new();
    let version = super::capabilities::ARSENAL_TEST_VERSION;
    super::service_tests::script(
        &world.bin.join("claude"),
        &format!(
            "[ \"$1\" = \"--version\" ] && {{ echo '{version}'; exit 0; }}\n\
             echo \"FAKE-CLAUDE argc=$#\"; prev=''; for a in \"$@\"; do\n\
             [ \"$prev\" = \"--settings\" ] && {{ echo OVERLAY-BEGIN; tr -d '\\n ' < \"$a\"; echo; echo OVERLAY-END; }}\n\
             prev=\"$a\"; done\n\
             exec cat"
        ),
    );
    let launch = arsenal_launch(&["design", "gsd-help"]);
    let preview = world
        .service
        .preview(AgentId::ClaudeCode, None, Some(&launch))
        .unwrap();
    assert!(preview.can_start);
    assert!(preview.applied[0].contains("2 installed skill(s) switched off"));
    // The token binds the exclusion: another exclusion does not start with it.
    let other = arsenal_launch(&["design"]);
    assert_eq!(
        arsenal_start(&world, Some(other), preview.plan_token.clone(), version)
            .unwrap_err()
            .code,
        "profile_preview_stale"
    );
    let reply = arsenal_start(&world, Some(launch.clone()), preview.plan_token, version).unwrap();
    let out = world.output_until(&reply.terminal.terminal_id, "OVERLAY-END");
    assert!(
        out.contains(r#"{"skillOverrides":{"design":"off","gsd-help":"off"}}"#),
        "{out:?}"
    );
    assert_eq!(reply.session.session.arsenal.as_ref(), Some(&launch));
    let id = reply.session.session.id.clone();
    let restart_preview = world
        .service
        .preview(AgentId::ClaudeCode, None, Some(&launch))
        .unwrap();
    let restarted = world
        .service
        .restart(
            &world.terminal,
            &id,
            SIZE,
            restart_preview.plan_token,
            Some(version.to_owned()),
        )
        .unwrap();
    assert_eq!(restarted.session.session.arsenal.as_ref(), Some(&launch));
}

#[test]
fn an_orchestrator_gets_the_delegation_tool_for_its_own_session_and_never_on_its_command_line() {
    let world = World::new();
    let claude = world.start_with(AgentId::ClaudeCode, None, true).unwrap();
    assert!(claude.session.session.delegation);
    let out = world.output_until(&claude.terminal.terminal_id, "SESSION=");
    let config = out
        .lines()
        .find_map(|line| line.strip_prefix("[--mcp-config=")?.strip_suffix(']'))
        .unwrap_or_else(|| panic!("{out:?}"));
    let config: serde_json::Value = serde_json::from_slice(&fs::read(config).unwrap()).unwrap();
    let server = &config["mcpServers"]["wyss-delegate"];
    assert_eq!(server["args"], serde_json::json!(["--wyss-delegate-mcp"]));
    let token = server["env"][crate::delegate::TOKEN_VAR]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(token.len(), 64);
    assert!(
        !out.contains(&token),
        "the token must not be on the command line"
    );
    // The right is this session's: the host knows it, and it lists nothing of another session.
    assert!(
        crate::delegate::manager()
            .list(Some(&token))
            .unwrap()
            .is_empty()
    );

    let codex = world.start_with(AgentId::Codex, None, true).unwrap();
    let out = world.output_until(&codex.terminal.terminal_id, "tool_timeout_sec");
    assert!(out.contains("[--no-daemon]"), "{out:?}");
    assert!(
        out.contains("[mcp_servers.wyss_delegate.args=[\"--wyss-delegate-mcp\"]]"),
        "{out:?}"
    );
    assert!(out.contains(&format!(
        "[mcp_servers.wyss_delegate.env_vars=[\"{}\"]]",
        crate::delegate::TOKEN_VAR
    )));

    // Without delegation nothing of it is added.
    let plain = world.start(AgentId::ClaudeCode, None).unwrap();
    let out = world.output_until(&plain.terminal.terminal_id, "SESSION=");
    assert!(!out.contains("mcp-config"), "{out:?}");
    assert!(!plain.session.session.delegation);
}
