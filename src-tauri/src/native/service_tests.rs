//! The launcher against real terminals: a fake `claude` and `codex` on a private PATH, started
//! through the user's real shell in a real PTY. Nothing here starts the installed agents.
use super::{
    catalogue::AgentId,
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
            "[ \"$1\" = \"--version\" ] && { echo '2.0.0 (fake)'; exit 0; }\n\
             echo \"FAKE-CLAUDE argc=$#\"; for a in \"$@\"; do echo \"[$a]\"; done\n\
             echo \"SESSION=$WYSS_NATIVE_SESSION_ID\"\n\
             for a in \"$@\"; do case \"$prev\" in --settings) echo \"OVERLAY=$(cat \"$a\")\";; esac; prev=\"$a\"; done\n\
             exec cat",
        );
        script(
            &bin.join("codex"),
            "[ \"$1\" = \"--version\" ] && { echo 'codex-cli 0.0.0 (fake)'; exit 0; }\n\
             echo \"FAKE-CODEX argc=$#\"; for a in \"$@\"; do echo \"[$a]\"; done\n\
             exit 3",
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
        self.service.start(
            &self.terminal,
            StartRequest {
                agent_id: agent,
                project: self.home.to_string_lossy().into_owned(),
                profile_id: profile.map(str::to_owned),
                size: SIZE,
                brief: None,
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
    assert_eq!(claude.version.as_deref(), Some("2.0.0 (fake)"));
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

    let second = world.service.restart(&world.terminal, &id, SIZE).unwrap();
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
fn only_a_folder_the_user_chose_or_a_known_project_can_be_a_project() {
    let world = World::new();
    let elsewhere = world.home.parent().unwrap().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    let request = |path: &Path| StartRequest {
        agent_id: AgentId::Codex,
        project: path.to_string_lossy().into_owned(),
        profile_id: None,
        size: SIZE,
        brief: None,
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
    let preview = world.service.preview(AgentId::Codex, Some(&id)).unwrap();
    assert!(!preview.can_start);
    // The same profile is fine for the agent that can do it, and says what it costs.
    let preview = world
        .service
        .preview(AgentId::ClaudeCode, Some(&id))
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
                    agent_id: agent,
                    project: project.to_string_lossy().into_owned(),
                    profile_id: None,
                    size: SIZE,
                    brief: None,
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
                agent_id: AgentId::ClaudeCode,
                project: world.home.to_string_lossy().into_owned(),
                profile_id: None,
                size: SIZE,
                brief: brief.map(str::to_owned),
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
