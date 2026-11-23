//! The launcher against real terminals: fake provider commands on a private PATH, started
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

const CODEX_HELP: &str = "Options:
  -m, --model <MODEL>
  -c, --config <KEY=VALUE>
  -s, --sandbox <MODE>
          [possible values: read-only, workspace-write, danger-full-access]
  -a, --ask-for-approval <POLICY>
          [possible values: untrusted, on-request, never]
  --no-daemon
  --dangerously-bypass-approvals-and-sandbox";
const CLAUDE_HELP: &str = "Options:
  --model <model>
  --effort <effort> (choices: low, medium, high, xhigh, max)
  --permission-mode <mode> (choices: manual, plan, acceptEdits)
  --settings <file>
  --append-system-prompt-file <file>
  --mcp-config <file>
  --strict-mcp-config
  --dangerously-skip-permissions";

pub(crate) fn script(path: &Path, body: &str) {
    let help = match path.file_name().and_then(|name| name.to_str()) {
        Some("claude") => Some(CLAUDE_HELP),
        Some("codex") => Some(CODEX_HELP),
        _ => None,
    };
    let probe = help
        .filter(|_| !body.contains("--help"))
        .map_or_else(String::new, |help| {
            format!("[ \"$1\" = --help ] && {{ printf '%s\\n' '{help}'; exit 0; }}\n")
        });
    fs::write(path, format!("#!/bin/sh\n{probe}{body}\n")).unwrap();
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
fn a_new_folder_in_the_home_can_start_and_one_outside_it_cannot() {
    let w = World::new();
    let inside = w.home.join("Proyectos Activos/nuevo");
    let outside = w.base.parent().unwrap().join("outside");
    fs::create_dir_all(&inside).unwrap();
    fs::create_dir_all(&outside).unwrap();
    let link = w.home.join("salida");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    fs::write(w.home.join("archivo"), "x").unwrap();
    assert_eq!(w.service.project_allowed(inside.to_str().unwrap()).unwrap(), inside);
    for refused in [
        outside.clone(),
        link,
        w.home.join("archivo"),
        w.home.join("no-existe"),
    ] {
        assert_eq!(
            w.service.project_allowed(refused.to_str().unwrap()).unwrap_err().code,
            "project_invalid"
        );
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
fn claude_launch_without_extended_default_ignores_sidecar_write_failure() {
    let world = World::new();
    let sidecar = world.base.join("registry/m9-native-sessions-v1.json");
    let original = fs::read(&sidecar).unwrap();
    // The service has loaded the extended registry. With no extended default to clear,
    // a Claude Code launch can succeed even if the sidecar is unwritable.
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
fn a_shell_path_change_cannot_redirect_the_probed_provider_on_restart() {
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

    let restarted = world
        .service
        .restart(
            &world.terminal,
            &id,
            SIZE,
            preview.plan_token,
            Some("2.1.309 (Claude Code)".to_owned()),
        )
        .unwrap();
    let argv = world.output_until(&restarted.terminal.terminal_id, "SESSION=");
    assert!(
        argv.contains("[--model]") && argv.contains("[sonnet]"),
        "{argv}"
    );
    assert!(!started.exists());
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
    let brief = "# Handoff from Codex\n\n$(touch wyss-brief-injected); `x` \"quoted\" -n\n";
    let reply = start(Some(brief)).unwrap();
    let out = world.output_until(&reply.terminal.terminal_id, "SESSION=");
    assert!(out.contains("argc=1"), "{out:?}");
    assert!(out.contains("[# Handoff from Codex"), "{out:?}");
    assert!(!world.home.join("wyss-brief-injected").exists());
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
        skills_on: Vec::new(),
        skill_files_off: Vec::new(),
        home: None,
        codex_home: None,
        launch_instructions: None,
        project_instructions_reviewed: false,
        suggestion_model: None,
        materialize: Vec::new(),
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
fn reviewed_claude_instructions_use_a_session_file_and_exclude_project_discovery() {
    use crate::arsenal::{role_instructions::Choice, project_instructions::Snapshot};
    let world = World::new();
    let project = world.home.join("project");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("CLAUDE.md"), "# Include\nkeep\n# Exclude\nleave out\n").unwrap();
    let snapshot = Snapshot::read(&project).unwrap();
    let ids = [snapshot.sections().next().unwrap().id.clone()].into();
    let base = Choice::Role { role: "Preset role".to_owned() };
    let composition = snapshot.compose(Some(&base), &ids);
    let expected = composition.text.clone();
    let mut launch = arsenal_launch(&["other"]);
    launch.skills_on = vec!["chosen".into()];
    launch.launch_instructions = Some(composition.into_choice().unwrap());
    launch.project_instructions_reviewed = true;
    let preview = world.service.preview(AgentId::ClaudeCode, None, Some(&launch)).unwrap();
    assert!(preview.can_start);
    let reply = world.service.start(&world.terminal, StartRequest {
        agent_id: AgentId::ClaudeCode,
        project: project.to_string_lossy().into(),
        profile_id: None,
        size: SIZE,
        brief: None,
        plan_token: preview.plan_token,
        expected_cli_version: preview.provider_version,
        arsenal: Some(launch),
        delegation: false,
    }).unwrap();
    let dir = world.base.join("sessions").join(&reply.session.session.id);
    assert_eq!(fs::read(dir.join("ROL.md")).unwrap(), expected.as_bytes());
    let overlay: serde_json::Value = serde_json::from_slice(&fs::read(dir.join("settings.json")).unwrap()).unwrap();
    assert_eq!(overlay["skillOverrides"], serde_json::json!({"chosen": "on", "other": "off"}));
    assert!(overlay["claudeMdExcludes"].as_array().unwrap().contains(&serde_json::json!(format!("{}/**", project.display()))));
    assert_eq!(fs::read_to_string(project.join("CLAUDE.md")).unwrap(), "# Include\nkeep\n# Exclude\nleave out\n");
}

#[test]
fn an_arsenal_exclusion_on_claude_is_not_refused_for_its_build() {
    let world = World::new();
    let launch = arsenal_launch(&["design"]);
    let preview = world
        .service
        .preview(AgentId::ClaudeCode, None, Some(&launch))
        .unwrap();
    // Without the preview's token the start is refused before any version is considered.
    let error =
        arsenal_start(&world, Some(launch.clone()), None, "2.1.309 (Claude Code)").unwrap_err();
    assert_eq!(error.code, "profile_preview_required");
    assert!(world.service.sessions(&world.terminal, false).is_empty());
    // A Claude Code build without an exact evidence record can still start.
    arsenal_start(
        &world,
        Some(launch),
        preview.plan_token.clone(),
        "2.1.309 (Claude Code)",
    )
    .expect("an unlisted Claude build is not refused");
    assert_eq!(world.service.sessions(&world.terminal, false).len(), 1);
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

/// The process environment a launch home reads (`HOME`, `CODEX_HOME`, `XDG_DATA_HOME`) pointed at a fake owner for
/// one test, and put back after. Never the real folders.
pub(crate) struct FakeOwnerEnv {
    saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl FakeOwnerEnv {
    pub(crate) fn new(home: &Path) -> Self {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let saved = ["HOME", "CODEX_HOME", "XDG_DATA_HOME"]
            .into_iter()
            .map(|name| (name, std::env::var_os(name)))
            .collect();
        // SAFETY: the lock above is the only writer of these names in the tests.
        unsafe {
            std::env::set_var("HOME", home);
            std::env::remove_var("CODEX_HOME");
            std::env::remove_var("XDG_DATA_HOME");
        }
        Self { saved, _lock: lock }
    }
}

impl Drop for FakeOwnerEnv {
    fn drop(&mut self) {
        for (name, value) in &self.saved {
            // SAFETY: see `new`.
            unsafe {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }
}

#[test]
fn a_codex_launch_home_is_built_for_a_changed_environment_only_and_a_failed_build_stops_the_start()
{
    use super::launch_home_codex::CodexHomeSpec;
    let world = World::new();
    let version = "codex-cli 0.169.0";
    script(
        &world.bin.join("codex"),
        &format!(
            "[ \"$1\" = \"--version\" ] && {{ echo '{version}'; exit 0; }}\n\
             echo \"CODEXHOME=[$CODEX_HOME]\"; echo CODEX-READY; exec cat"
        ),
    );
    // A fake owner: never the real `~/.codex`.
    let owner_home = world.home.parent().unwrap().join("owner-home");
    let owner = owner_home.join(".codex");
    fs::create_dir_all(owner.join("sessions")).unwrap();
    fs::write(owner.join("config.toml"), "model = \"x\"\n").unwrap();
    fs::write(owner.join("AGENTS.md"), "kernel").unwrap();
    fs::write(owner.join("auth.json"), "FAKE-TOKEN").unwrap();
    let key = super::launch_home::hook_key("Stop", "", "bye.sh");
    fs::write(
        owner.join("hooks.json"),
        r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"bye.sh"},{"type":"command","command":"keep.sh"}]}]}}"#,
    )
    .unwrap();
    let _env = FakeOwnerEnv::new(&owner_home);
    let codex_start = |launch: &super::arsenal::ArsenalLaunch| {
        let preview = world
            .service
            .preview(AgentId::Codex, None, Some(launch))
            .unwrap();
        assert!(preview.can_start, "{:?}", preview.not_applicable);
        world.service.start(
            &world.terminal,
            StartRequest {
                agent_id: AgentId::Codex,
                project: world.home.to_string_lossy().into_owned(),
                profile_id: None,
                size: SIZE,
                brief: None,
                plan_token: preview.plan_token,
                expected_cli_version: Some(version.to_owned()),
                arsenal: Some(launch.clone()),
                delegation: false,
            },
        )
    };
    let with = |spec: CodexHomeSpec| super::arsenal::ArsenalLaunch {
        codex_home: Some(spec),
        ..arsenal_launch(&[])
    };
    let homes = world.base.join("launch-homes").join("codex");

    // A default environment: the owner's own folder, no `CODEX_HOME`, nothing built.
    let reply = codex_start(&with(CodexHomeSpec::default())).unwrap();
    let out = world.output_until(&reply.terminal.terminal_id, "CODEX-READY");
    assert!(out.contains("CODEXHOME=[]"), "{out:?}");
    assert!(!homes.exists());

    // A changed one: `CODEX_HOME` is the home of this selection, built without the hook.
    let chosen = with(CodexHomeSpec {
        instructions: false,
        hooks_off: vec![key],
        ..Default::default()
    });
    let reply = codex_start(&chosen).unwrap();
    let out = world.output_until(&reply.terminal.terminal_id, "CODEX-READY");
    let home = homes.join(&chosen.selection);
    assert!(
        out.contains(&format!("CODEXHOME=[{}]", home.display())),
        "{out:?}"
    );
    let hooks = fs::read_to_string(home.join("hooks.json")).unwrap();
    assert!(
        !hooks.contains("bye.sh") && hooks.contains("keep.sh"),
        "{hooks}"
    );
    assert!(!home.join("AGENTS.md").exists());
    let before = world.service.sessions(&world.terminal, false).len();

    // A home that cannot be built (no owner folder): the start fails and nothing runs.
    fs::rename(&owner, owner_home.join(".codex-gone")).unwrap();
    let error = codex_start(&chosen).unwrap_err();
    assert_eq!(error.code, "arsenal_home_failed");
    assert_eq!(world.service.sessions(&world.terminal, false).len(), before);
}

/// `PATH` with a fake `secret-tool` first, for one test, and put back after: the launch home of
/// Claude reads its token from the key ring, and a test must never read the real one. Take it
/// after a [`FakeOwnerEnv`], whose lock it relies on.
struct FakeKeyRing {
    saved: Option<std::ffi::OsString>,
}

impl FakeKeyRing {
    const TOKEN: &'static str = "fake-keyring-token-0123456789-abcdef";

    fn new(dir: &Path) -> Self {
        fs::create_dir_all(dir).unwrap();
        script(&dir.join("secret-tool"), &format!("echo '{}'", Self::TOKEN));
        let saved = std::env::var_os("PATH");
        let mut parts = vec![dir.to_path_buf()];
        parts.extend(std::env::split_paths(&saved.clone().unwrap_or_default()));
        // SAFETY: the lock of `FakeOwnerEnv` is held by the caller for the whole test.
        unsafe { std::env::set_var("PATH", std::env::join_paths(parts).unwrap()) };
        Self { saved }
    }
}

impl Drop for FakeKeyRing {
    fn drop(&mut self) {
        // SAFETY: see `new`.
        unsafe {
            match &self.saved {
                Some(path) => std::env::set_var("PATH", path),
                None => std::env::remove_var("PATH"),
            }
        }
    }
}

/// Replaces the world's shell with one that writes the words it was started with, one per line
/// in brackets, to the returned file, and then runs the real recipe.
fn log_shell_argv(world: &World) -> PathBuf {
    let log = world.base.join("shell-argv.log");
    script(
        &world.bin.join("bash"),
        &format!(
            "for a in \"$@\"; do printf '[%s]\\n' \"$a\"; done > '{}'\n\
             exec /usr/bin/bash --norc --noprofile \"$@\"",
            log.display()
        ),
    );
    log
}

/// A Claude launch with an environment of its own, an owner folder, and a fake key ring.
fn claude_home_start(world: &World, home: Option<crate::native::launch_home::HomeSpec>) -> String {
    let version = super::capabilities::ARSENAL_TEST_VERSION;
    script(
        &world.bin.join("claude"),
        &format!(
            "[ \"$1\" = \"--version\" ] && {{ echo '{version}'; exit 0; }}\n\
             echo \"CFG=[$CLAUDE_CONFIG_DIR]\"; echo \"TOKENENV=[$CLAUDE_CODE_OAUTH_TOKEN]\"; echo CLAUDE-READY; exec cat"
        ),
    );
    let launch = super::arsenal::ArsenalLaunch {
        home,
        ..arsenal_launch(&[])
    };
    let preview = world
        .service
        .preview(AgentId::ClaudeCode, None, Some(&launch))
        .unwrap();
    assert!(preview.can_start, "{:?}", preview.not_applicable);
    let reply = arsenal_start(world, Some(launch), preview.plan_token, version).unwrap();
    world.output_until(&reply.terminal.terminal_id, "CLAUDE-READY")
}

fn claude_home_world() -> (World, FakeOwnerEnv, FakeKeyRing) {
    let world = World::new();
    let owner_home = world.home.parent().unwrap().join("owner-home");
    fs::create_dir_all(owner_home.join(".claude")).unwrap();
    fs::write(owner_home.join(".claude/settings.json"), "{}").unwrap();
    let env = FakeOwnerEnv::new(&owner_home);
    let ring = FakeKeyRing::new(&world.base.join("fake-ring"));
    (world, env, ring)
}

#[test]
fn a_claude_launch_with_a_home_pins_its_config_dir_and_never_puts_the_token_in_argv() {
    let (world, _env, _ring) = claude_home_world();
    let log = log_shell_argv(&world);
    let home = crate::native::launch_home::HomeSpec {
        instructions: false,
        ..Default::default()
    };
    let out = claude_home_start(&world, Some(home));
    let dir = world
        .base
        .join("launch-homes")
        .join(arsenal_launch(&[]).selection);
    let argv = fs::read_to_string(&log).unwrap();
    assert!(
        argv.contains(&format!("[CLAUDE_CONFIG_DIR={}]", dir.display())),
        "{argv}"
    );
    assert!(
        argv.contains("[/usr/bin/env]") || argv.contains("exec /usr/bin/env"),
        "{argv}"
    );
    assert!(!argv.contains("CODEX_HOME="), "{argv}");
    // The token reaches the agent by environment only (control: the agent does see it).
    assert!(
        out.contains(&format!("TOKENENV=[{}]", FakeKeyRing::TOKEN)),
        "{out:?}"
    );
    assert!(!argv.contains(FakeKeyRing::TOKEN), "token in argv: {argv}");
    assert!(!argv.contains("CLAUDE_CODE_OAUTH_TOKEN="), "{argv}");
    assert!(out.contains(&format!("CFG=[{}]", dir.display())), "{out:?}");
}

#[test]
fn a_claude_launch_without_a_home_has_no_pin() {
    let (world, _env, _ring) = claude_home_world();
    let log = log_shell_argv(&world);
    let out = claude_home_start(&world, None);
    let argv = fs::read_to_string(&log).unwrap();
    assert!(!argv.contains("CLAUDE_CONFIG_DIR="), "{argv}");
    assert!(!argv.contains("/usr/bin/env"), "{argv}");
    assert!(!argv.contains(FakeKeyRing::TOKEN), "{argv}");
    assert!(out.contains("TOKENENV=[]"), "{out:?}");
}

#[test]
fn a_codex_launch_pins_its_home_only_when_it_has_one() {
    use super::launch_home_codex::CodexHomeSpec;
    let world = World::new();
    let version = "codex-cli 0.169.0";
    script(
        &world.bin.join("codex"),
        &format!(
            "[ \"$1\" = \"--version\" ] && {{ echo '{version}'; exit 0; }}\n\
             echo \"CODEXHOME=[$CODEX_HOME]\"; echo CODEX-READY; exec cat"
        ),
    );
    let owner_home = world.home.parent().unwrap().join("owner-home");
    let owner = owner_home.join(".codex");
    fs::create_dir_all(&owner).unwrap();
    fs::write(owner.join("config.toml"), "model = \"x\"\n").unwrap();
    fs::write(owner.join("AGENTS.md"), "kernel").unwrap();
    fs::write(owner.join("auth.json"), "FAKE-TOKEN").unwrap();
    let _env = FakeOwnerEnv::new(&owner_home);
    let log = log_shell_argv(&world);
    let run = |spec: CodexHomeSpec| {
        let launch = super::arsenal::ArsenalLaunch {
            codex_home: Some(spec),
            ..arsenal_launch(&[])
        };
        let preview = world
            .service
            .preview(AgentId::Codex, None, Some(&launch))
            .unwrap();
        assert!(preview.can_start, "{:?}", preview.not_applicable);
        let reply = world
            .service
            .start(
                &world.terminal,
                StartRequest {
                    agent_id: AgentId::Codex,
                    project: world.home.to_string_lossy().into_owned(),
                    profile_id: None,
                    size: SIZE,
                    brief: None,
                    plan_token: preview.plan_token,
                    expected_cli_version: Some(version.to_owned()),
                    arsenal: Some(launch.clone()),
                    delegation: false,
                },
            )
            .unwrap();
        world.output_until(&reply.terminal.terminal_id, "CODEX-READY");
        (launch, fs::read_to_string(&log).unwrap())
    };
    // No home (the default environment): no pin, no `env` between the shell and the agent.
    let (_, argv) = run(CodexHomeSpec::default());
    assert!(!argv.contains("CODEX_HOME="), "{argv}");
    assert!(!argv.contains("/usr/bin/env"), "{argv}");
    // A home: the pin is one argv word with the built home.
    let (launch, argv) = run(CodexHomeSpec {
        instructions: false,
        hooks_off: Vec::new(),
        ..Default::default()
    });
    let dir = world
        .base
        .join("launch-homes")
        .join("codex")
        .join(&launch.selection);
    assert!(
        argv.contains(&format!("[CODEX_HOME={}]", dir.display())),
        "{argv}"
    );
    assert!(!argv.contains("CLAUDE_CONFIG_DIR="), "{argv}");
}

// ---- a launch with a folder of its own is refused in the owner's home -----------------------

const HOME_REFUSED: &str = "arsenal_home_cwd_is_home";
const CODEX_VERSION: &str = "codex-cli 0.169.0";

/// A world whose fake owner home IS the project folder (`world.home`): the case the host refuses.
/// The key ring is fake and holds the token; `HOME` is never the real one.
fn project_is_home_world() -> (World, FakeOwnerEnv, FakeKeyRing) {
    let world = World::new();
    let env = FakeOwnerEnv::new(&world.home);
    let ring = FakeKeyRing::new(&world.base.join("fake-ring"));
    (world, env, ring)
}

/// Fake `claude` and `codex` that leave a marker file when they run for real (not for `--version`).
fn marker_agents(world: &World) -> PathBuf {
    let marker = world.base.join("agent-spawned");
    let touch = format!("touch '{}'", marker.display());
    script(
        &world.bin.join("claude"),
        &format!(
            "[ \"$1\" = \"--version\" ] && {{ echo '{}'; exit 0; }}\n{touch}\n\
             echo CLAUDE-READY; exec cat",
            super::capabilities::ARSENAL_TEST_VERSION
        ),
    );
    script(
        &world.bin.join("codex"),
        &format!(
            "[ \"$1\" = \"--version\" ] && {{ echo '{CODEX_VERSION}'; exit 0; }}\n{touch}\n\
             echo CODEX-READY; exec cat"
        ),
    );
    marker
}

fn claude_owner_folder(world: &World) {
    fs::create_dir_all(world.home.join(".claude")).unwrap();
    fs::write(world.home.join(".claude/settings.json"), "{}").unwrap();
}

fn codex_owner_folder(world: &World) {
    let owner = world.home.join(".codex");
    fs::create_dir_all(&owner).unwrap();
    fs::write(owner.join("config.toml"), "model = \"x\"\n").unwrap();
    fs::write(owner.join("AGENTS.md"), "kernel").unwrap();
    fs::write(owner.join("auth.json"), "FAKE-TOKEN").unwrap();
}

fn claude_own_home() -> super::arsenal::ArsenalLaunch {
    super::arsenal::ArsenalLaunch {
        home: Some(crate::native::launch_home::HomeSpec {
            instructions: false,
            ..Default::default()
        }),
        ..arsenal_launch(&[])
    }
}

fn codex_own_home() -> super::arsenal::ArsenalLaunch {
    super::arsenal::ArsenalLaunch {
        codex_home: Some(super::launch_home_codex::CodexHomeSpec {
            instructions: false,
            hooks_off: Vec::new(),
            ..Default::default()
        }),
        ..arsenal_launch(&[])
    }
}

/// Previews and starts `launch` for `agent` in `project`.
fn start_in(
    world: &World,
    agent: AgentId,
    project: &Path,
    launch: &super::arsenal::ArsenalLaunch,
) -> Result<super::service::StartReply, super::service::NativeError> {
    let version = match agent {
        AgentId::Codex => CODEX_VERSION,
        _ => super::capabilities::ARSENAL_TEST_VERSION,
    };
    let preview = world.service.preview(agent, None, Some(launch)).unwrap();
    assert!(preview.can_start, "{:?}", preview.not_applicable);
    world.service.start(
        &world.terminal,
        StartRequest {
            agent_id: agent,
            project: project.to_string_lossy().into_owned(),
            profile_id: None,
            size: SIZE,
            brief: None,
            plan_token: preview.plan_token,
            expected_cli_version: Some(version.to_owned()),
            arsenal: Some(launch.clone()),
            delegation: false,
        },
    )
}

/// The refusal wrote nothing: no session record or folder, no launch home, no agent spawned.
fn assert_nothing_started(world: &World, marker: &Path) {
    assert!(!marker.exists(), "the agent was spawned");
    assert!(
        !world.base.join("launch-homes").exists(),
        "a launch home was built"
    );
    let sessions = world.base.join("sessions");
    let folders = fs::read_dir(&sessions).map_or(0, |entries| entries.count());
    assert_eq!(folders, 0, "a sessions/<id> folder was created");
    assert!(world.service.sessions(&world.terminal, false).is_empty());
}

#[test]
fn reviewed_codex_restart_keeps_each_review_in_its_own_launch_home() {
    use super::arsenal::InstructionReview;
    use crate::arsenal::role_instructions::Choice;
    let world = World::new();
    let _env = FakeOwnerEnv::new(&world.home);
    let owner = world.home.join(".codex");
    fs::create_dir_all(&owner).unwrap();
    fs::write(owner.join("AGENTS.md"), "Global").unwrap();
    let project = world.home.join("project");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("AGENTS.md"), "Both sections originally").unwrap();
    script(
        &world.bin.join("codex"),
        r#"[ "$1" = --version ] && { echo 'codex-cli 0.170.0'; exit 0; }
printf '%s\n' "$@"
exec cat"#,
    );
    let role = |role: &str| Choice::Role { role: role.into() };
    let mut launch = arsenal_launch(&[]);
    launch.apply_review(InstructionReview { choice: Some(role("First review")), reviewed: true, model: None });
    let first = start_in(&world, AgentId::Codex, &project, &launch).unwrap();
    let output = world.output_until(&first.terminal.terminal_id, "project_doc_max_bytes=0");
    assert!(output.contains("project_doc_max_bytes=0"), "{output:?}");
    let revision = InstructionReview { choice: Some(role("Second review")), reviewed: true, model: None };
    world.service.restart_with_review(
        &world.terminal, &first.session.session.id, SIZE, None, None, Some(revision),
    ).unwrap();
    let homes = world.base.join("launch-homes/codex").join(&launch.selection);
    let mut texts: Vec<_> = fs::read_dir(homes).unwrap().map(|entry| {
        fs::read_to_string(entry.unwrap().path().join("AGENTS.md")).unwrap()
    }).collect();
    texts.sort();
    assert_eq!(texts, ["Global\n\nFirst review", "Global\n\nSecond review"]);
    assert_eq!(fs::read_to_string(project.join("AGENTS.md")).unwrap(), "Both sections originally");
}

#[test]
fn a_claude_launch_with_a_home_of_its_own_is_refused_when_the_project_is_the_owner_home() {
    let (world, _env, _ring) = project_is_home_world();
    let marker = marker_agents(&world);
    claude_owner_folder(&world);
    let error = start_in(&world, AgentId::ClaudeCode, &world.home, &claude_own_home()).unwrap_err();
    assert_eq!(error.code, HOME_REFUSED);
    assert_nothing_started(&world, &marker);
}

#[test]
fn a_codex_launch_with_a_home_of_its_own_is_refused_when_the_project_is_the_owner_home() {
    let (world, _env, _ring) = project_is_home_world();
    let marker = marker_agents(&world);
    codex_owner_folder(&world);
    let error = start_in(&world, AgentId::Codex, &world.home, &codex_own_home()).unwrap_err();
    assert_eq!(error.code, HOME_REFUSED);
    assert_nothing_started(&world, &marker);
}

#[test]
fn launches_without_a_home_of_their_own_start_in_the_owner_home() {
    let (world, _env, _ring) = project_is_home_world();
    marker_agents(&world);
    claude_owner_folder(&world);
    codex_owner_folder(&world);
    // An Arsenal launch whose environment is the default: no home on either agent.
    let reply = start_in(
        &world,
        AgentId::ClaudeCode,
        &world.home,
        &arsenal_launch(&[]),
    )
    .unwrap();
    world.output_until(&reply.terminal.terminal_id, "CLAUDE-READY");
    let codex_default = super::arsenal::ArsenalLaunch {
        codex_home: Some(super::launch_home_codex::CodexHomeSpec::default()),
        ..arsenal_launch(&[])
    };
    let reply = start_in(&world, AgentId::Codex, &world.home, &codex_default).unwrap();
    world.output_until(&reply.terminal.terminal_id, "CODEX-READY");
    // And no Arsenal at all.
    let reply = world.start(AgentId::ClaudeCode, None).unwrap();
    world.output_until(&reply.terminal.terminal_id, "CLAUDE-READY");
}

#[test]
fn a_restart_of_a_launch_with_a_home_of_its_own_is_refused_in_the_owner_home_too() {
    let world = World::new();
    let marker = marker_agents(&world);
    // The session starts under another owner (a sibling folder): it is allowed.
    let first_owner = world.home.parent().unwrap().join("owner-home");
    fs::create_dir_all(first_owner.join(".claude")).unwrap();
    fs::write(first_owner.join(".claude/settings.json"), "{}").unwrap();
    let id = {
        let _env = FakeOwnerEnv::new(&first_owner);
        let _ring = FakeKeyRing::new(&world.base.join("fake-ring"));
        let reply = start_in(&world, AgentId::ClaudeCode, &world.home, &claude_own_home()).unwrap();
        world.output_until(&reply.terminal.terminal_id, "CLAUDE-READY");
        reply.session.session.id.clone()
    };
    fs::remove_file(&marker).unwrap();
    let before = world.service.sessions(&world.terminal, false).len();
    // Then the owner's home is the project's folder: the restart goes through `start_with`.
    let _env = FakeOwnerEnv::new(&world.home);
    let _ring = FakeKeyRing::new(&world.base.join("fake-ring"));
    let preview = world
        .service
        .preview(AgentId::ClaudeCode, None, Some(&claude_own_home()))
        .unwrap();
    let error = world
        .service
        .restart(
            &world.terminal,
            &id,
            SIZE,
            preview.plan_token,
            Some(super::capabilities::ARSENAL_TEST_VERSION.to_owned()),
        )
        .unwrap_err();
    assert_eq!(error.code, HOME_REFUSED);
    assert!(!marker.exists(), "the restart spawned the agent");
    assert_eq!(world.service.sessions(&world.terminal, false).len(), before);
}

fn review_request(world: &World, plan_token: Option<String>) -> StartRequest {
    StartRequest {
        arsenal: None,
        delegation: false,
        agent_id: AgentId::ClaudeCode,
        project: world.home.to_string_lossy().into_owned(),
        profile_id: Some("template-review".to_owned()),
        size: SIZE,
        brief: None,
        plan_token,
        expected_cli_version: Some("2.1.309 (Claude Code)".to_owned()),
    }
}

#[test]
fn a_preview_token_starts_once() {
    let world = World::new();
    let preview = world
        .service
        .preview(AgentId::ClaudeCode, Some("template-review"), None)
        .unwrap();
    let token = preview.plan_token.clone().unwrap();
    let first = world
        .service
        .start(&world.terminal, review_request(&world, Some(token.clone())))
        .unwrap();
    world.output_until(&first.terminal.terminal_id, "SESSION=");
    let again = world
        .service
        .start(&world.terminal, review_request(&world, Some(token)))
        .unwrap_err();
    assert_eq!(again.code, "profile_preview_stale");
    assert_eq!(world.service.sessions(&world.terminal, false).len(), 1);
}

#[test]
fn a_token_the_preview_never_issued_or_one_past_its_time_does_not_start() {
    let world = World::new();
    // A token is not something a client can compute from the plan.
    let forged = world
        .service
        .start(&world.terminal, review_request(&world, Some("ab".repeat(32))))
        .unwrap_err();
    assert_eq!(forged.code, "profile_preview_stale");
    let preview = world
        .service
        .preview(AgentId::ClaudeCode, Some("template-review"), None)
        .unwrap();
    world.service.expire_plan_tokens();
    let expired = world
        .service
        .start(&world.terminal, review_request(&world, preview.plan_token))
        .unwrap_err();
    assert_eq!(expired.code, "profile_preview_stale");
    assert!(world.service.sessions(&world.terminal, false).is_empty());
}

#[test]
fn a_restart_without_a_token_keeps_its_plan_but_never_grants_full_access_again() {
    let world = World::new();
    let reviewed = world.start(AgentId::ClaudeCode, Some("template-review")).unwrap();
    let id = reviewed.session.session.id.clone();
    world.output_until(&reviewed.terminal.terminal_id, "SESSION=");
    let restarted = world
        .service
        .restart(
            &world.terminal,
            &id,
            SIZE,
            None,
            Some("2.1.309 (Claude Code)".to_owned()),
        )
        .unwrap();
    world.output_until(&restarted.terminal.terminal_id, "SESSION=");

    let full = world
        .start(AgentId::ClaudeCode, Some("template-full-access"))
        .unwrap();
    let full_id = full.session.session.id.clone();
    world.output_until(&full.terminal.terminal_id, "SESSION=");
    let error = world
        .service
        .restart(
            &world.terminal,
            &full_id,
            SIZE,
            None,
            Some("2.1.309 (Claude Code)".to_owned()),
        )
        .unwrap_err();
    assert_eq!(error.code, "restart_needs_preview");
    let old = world
        .service
        .sessions(&world.terminal, false)
        .into_iter()
        .find(|session| session.session.id == full_id)
        .unwrap();
    assert_eq!(old.session.state, State::Running);
}

#[test]
fn the_shell_between_never_holds_the_sign_in_token_in_its_environment() {
    let (world, _env, _ring) = claude_home_world();
    let log = world.base.parent().unwrap().join("shell-env");
    script(
        &world.bin.join("bash"),
        &format!(
            "env > '{}'\nexec /usr/bin/bash --norc --noprofile \"$@\"",
            log.display()
        ),
    );
    let home = crate::native::launch_home::HomeSpec {
        instructions: false,
        ..Default::default()
    };
    let out = claude_home_start(&world, Some(home));
    // Control: the agent itself gets the token.
    assert!(
        out.contains(&format!("TOKENENV=[{}]", FakeKeyRing::TOKEN)),
        "{out:?}"
    );
    let shell_env = fs::read_to_string(&log).unwrap();
    assert!(shell_env.contains("WYSS_NATIVE_SESSION_ID="), "{shell_env}");
    assert!(!shell_env.contains(FakeKeyRing::TOKEN), "{shell_env}");
    assert!(!shell_env.contains("CLAUDE_CODE_OAUTH_TOKEN"), "{shell_env}");
    // The file it came through is gone once the agent runs.
    for session in fs::read_dir(world.base.join("sessions")).unwrap().flatten() {
        assert!(!session.path().join("oauth-token").exists());
    }
}

fn token_files(base: &Path) -> Vec<PathBuf> {
    fs::read_dir(base.join("sessions"))
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path().join("oauth-token"))
                .filter(|p| fs::symlink_metadata(p).is_ok())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_start_that_fails_to_save_the_registry_after_writing_the_token_leaves_no_token_file() {
    let (world, _env, _ring) = claude_home_world();
    // An extended default for the project: the Claude start must clear it durably first.
    let opencode = world.start(AgentId::Opencode, None).unwrap();
    world.output_until(&opencode.terminal.terminal_id, "FAKE-OPENCODE");
    let registry = world.base.join("registry");
    fs::set_permissions(&registry, fs::Permissions::from_mode(0o500)).unwrap();
    let version = super::capabilities::ARSENAL_TEST_VERSION;
    script(
        &world.bin.join("claude"),
        &format!("[ \"$1\" = \"--version\" ] && {{ echo '{version}'; exit 0; }}\nexec cat"),
    );
    let launch = super::arsenal::ArsenalLaunch {
        home: Some(crate::native::launch_home::HomeSpec {
            instructions: false,
            ..Default::default()
        }),
        ..arsenal_launch(&[])
    };
    let preview = world
        .service
        .preview(AgentId::ClaudeCode, None, Some(&launch))
        .unwrap();
    let error = arsenal_start(&world, Some(launch), preview.plan_token, version);
    fs::set_permissions(&registry, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(error.unwrap_err().code, "store_unavailable");
    assert_eq!(token_files(&world.base), Vec::<PathBuf>::new());
}

#[test]
fn a_token_file_a_stopped_host_left_behind_is_removed_when_the_host_opens() {
    let world = World::new();
    let stale = world.base.join("sessions/0123456789abcdef0123456789abcdef");
    fs::create_dir_all(&stale).unwrap();
    fs::write(stale.join("oauth-token"), "fixture-token-stale\n").unwrap();
    // A folder whose entries cannot be removed as it is: the whole-tree cleanup fails on it.
    fs::set_permissions(&stale, fs::Permissions::from_mode(0o500)).unwrap();
    let _again = World::open(&world.base, &world.home, &world.bin);
    let left = token_files(&world.base);
    let _ = fs::set_permissions(&stale, fs::Permissions::from_mode(0o700));
    assert_eq!(left, Vec::<PathBuf>::new());
}

#[test]
fn a_full_access_restart_goes_on_with_a_fresh_preview_token_used_once() {
    let world = World::new();
    let full = world
        .start(AgentId::ClaudeCode, Some("template-full-access"))
        .unwrap();
    let id = full.session.session.id.clone();
    world.output_until(&full.terminal.terminal_id, "SESSION=");
    let version = Some("2.1.309 (Claude Code)".to_owned());
    // Without a token: refused, the old session keeps running.
    let refused = world
        .service
        .restart(&world.terminal, &id, SIZE, None, version.clone())
        .unwrap_err();
    assert_eq!(refused.code, "restart_needs_preview");
    let token = world
        .service
        .preview(AgentId::ClaudeCode, Some("template-full-access"), None)
        .unwrap()
        .plan_token
        .unwrap();
    let restarted = world
        .service
        .restart(&world.terminal, &id, SIZE, Some(token.clone()), version.clone())
        .unwrap();
    world.output_until(&restarted.terminal.terminal_id, "SESSION=");
    assert_eq!(restarted.session.session.restart_of.as_deref(), Some(id.as_str()));
    // The same token again: already consumed.
    let new_id = restarted.session.session.id.clone();
    let again = world
        .service
        .restart(&world.terminal, &new_id, SIZE, Some(token), version)
        .unwrap_err();
    assert_eq!(again.code, "profile_preview_stale");
}

fn provider_with_help(file: &Path, version: &str, help: &str) {
    script(
        file,
        &format!(
            "case \"$1\" in --version) echo '{version}'; exit 0;; --help) printf '%s\\n' '{help}'; exit 0;; esac\nfor a in \"$@\"; do echo \"[$a]\"; done; echo READY; exec cat"
        ),
    );
}

#[test]
fn codex_new_help_applies_edits_and_mcp_without_a_version_record() {
    let world = World::new();
    provider_with_help(&world.bin.join("codex"), "codex-cli 0.999.0", CODEX_HELP);
    let selection = super::arsenal::ArsenalLaunch {
        codex_home: Some(super::launch_home_codex::CodexHomeSpec {
            mcp: [("notes-server".to_owned(), false)].into(),
            ..Default::default()
        }),
        ..arsenal_launch(&[])
    };
    let profile = Some("template-edits");
    let preview = world
        .service
        .preview(AgentId::Codex, profile, Some(&selection))
        .unwrap();
    assert!(preview.can_start, "{:?}", preview.not_applicable);
    assert_eq!(
        preview.provider_version.as_deref(),
        Some("codex-cli 0.999.0")
    );
    let started = world
        .service
        .start(
            &world.terminal,
            StartRequest {
                agent_id: AgentId::Codex,
                project: world.home.to_string_lossy().into_owned(),
                size: SIZE,
                profile_id: profile.map(str::to_owned),
                brief: None,
                plan_token: preview.plan_token,
                expected_cli_version: Some("codex-cli 0.159.0".into()),
                arsenal: Some(selection),
                delegation: false,
            },
        )
        .unwrap();
    let argv = world.output_until(&started.terminal.terminal_id, "READY");
    for arg in [
        "-s",
        "workspace-write",
        "-a",
        "on-request",
        "-c",
        "mcp_servers.notes-server.enabled=false",
    ] {
        assert!(argv.contains(&format!("[{arg}]")), "{argv}");
    }
    assert!(started.omitted_options.is_empty());
}

#[test]
fn codex_updated_or_unreadable_help_starts_without_unannounced_permission_options() {
    for (help, expected) in [
        (
            CODEX_HELP.replace("  -a, --ask-for-approval", "  --other-approval"),
            "--ask-for-approval on-request",
        ),
        ("unreadable options".to_owned(), "--sandbox read-only"),
    ] {
        let world = World::new();
        let cli = world.bin.join("codex");
        provider_with_help(&cli, "codex-cli 0.998.0", CODEX_HELP);
        let profile = Some("template-review");
        let preview = world
            .service
            .preview(AgentId::Codex, profile, None)
            .unwrap();
        assert!(preview.omitted_options.is_empty());
        provider_with_help(&cli, "codex-cli 0.999.0", &help);
        let started = world
            .service
            .start(
                &world.terminal,
                StartRequest {
                    agent_id: AgentId::Codex,
                    project: world.home.to_string_lossy().into_owned(),
                    size: SIZE,
                    profile_id: profile.map(str::to_owned),
                    brief: None,
                    plan_token: preview.plan_token,
                    expected_cli_version: preview.provider_version,
                    arsenal: None,
                    delegation: false,
                },
            )
            .unwrap();
        let argv = world.output_until(&started.terminal.terminal_id, "READY");
        assert!(!argv.contains("[-s]") && !argv.contains("[-a]"), "{argv}");
        assert_eq!(started.omitted_options, [expected]);
    }
}

#[test]
fn claude_help_controls_flags_on_an_unrecorded_version() {
    let world = World::new();
    let cli = world.bin.join("claude");
    let help = CLAUDE_HELP.replace("manual, plan, acceptEdits", "plan, acceptEdits");
    provider_with_help(&cli, "8.8.8 (Claude Code)", &help);
    let profile = world
        .service
        .profiles()
        .create(Body {
            name: "Requested options".into(),
            applies_to: vec![AgentId::ClaudeCode],
            model: Some("sonnet".into()),
            effort: Some("high".into()),
            permission: Some(Permission::Ask),
            ..Body::default()
        })
        .unwrap();
    let preview = world
        .service
        .preview(AgentId::ClaudeCode, Some(&profile), None)
        .unwrap();
    assert!(preview.can_start);
    assert_eq!(preview.omitted_options, ["--permission-mode manual"]);
    let start = world
        .service
        .start(
            &world.terminal,
            StartRequest {
                agent_id: AgentId::ClaudeCode,
                project: world.home.to_string_lossy().into_owned(),
                size: SIZE,
                profile_id: Some(profile),
                brief: None,
                plan_token: preview.plan_token,
                expected_cli_version: None,
                arsenal: None,
                delegation: false,
            },
        )
        .unwrap();
    let argv = world.output_until(&start.terminal.terminal_id, "READY");
    for arg in ["--model", "sonnet", "--effort", "high"] {
        assert!(argv.contains(&format!("[{arg}]")), "{argv}");
    }
    assert!(!argv.contains("[--permission-mode]"), "{argv}");
    assert_eq!(start.omitted_options, ["--permission-mode manual"]);
}

#[test]
fn a_preview_without_bypass_does_not_authorize_it_after_an_upgrade() {
    let world = World::new();
    let cli = world.bin.join("codex");
    let without_bypass = CODEX_HELP.replace("--dangerously-bypass-approvals-and-sandbox", "");
    provider_with_help(&cli, "codex-cli 0.998.0", &without_bypass);
    let profile = Some("template-full-access");
    let preview = world
        .service
        .preview(AgentId::Codex, profile, None)
        .unwrap();
    assert!(preview.can_start && !preview.bypass);
    provider_with_help(&cli, "codex-cli 0.999.0", CODEX_HELP);
    let request = StartRequest {
        agent_id: AgentId::Codex,
        project: world.home.to_string_lossy().into_owned(),
        size: SIZE,
        profile_id: profile.map(str::to_owned),
        brief: None,
        plan_token: preview.plan_token,
        expected_cli_version: preview.provider_version,
        arsenal: None,
        delegation: false,
    };
    let started = world
        .service
        .start(&world.terminal, request.clone())
        .unwrap();
    assert_eq!(started.omitted_options, ["bypass_preview_required"]);
    let argv = world.output_until(&started.terminal.terminal_id, "READY");
    assert!(
        !argv.contains("--dangerously-bypass-approvals-and-sandbox"),
        "{argv}"
    );
    assert_eq!(
        world
            .service
            .start(&world.terminal, request)
            .unwrap_err()
            .code,
        "profile_preview_stale"
    );
}

#[test]
fn a_preset_compiled_from_the_library_is_accepted_by_preview_and_start() {
    let world = World::new();
    let (store, scan) = crate::arsenal::library::tests::fixture("claude_code", b"keep body");
    let data = world.home.join("arsenal-data");
    std::fs::create_dir_all(&data).unwrap();
    let launch = crate::arsenal::library::launch_for(
        &store,
        &data,
        "Front",
        "claude_code",
        &world.home,
        &scan,
    )
    .unwrap();
    let preview = world
        .service
        .preview(AgentId::ClaudeCode, None, Some(&launch))
        .unwrap();
    assert!(preview.not_applicable.is_empty(), "{:?}", preview.not_applicable);
    assert!(preview.applied[0].contains("2 installed skill(s) switched off"));
    let reply = arsenal_start(
        &world,
        Some(launch.clone()),
        preview.plan_token,
        "2.1.309 (Claude Code)",
    )
    .expect("the host starts the compiled preset");
    assert_eq!(reply.session.session.arsenal.as_ref(), Some(&launch));
}

// ---- stored skills copied into the session's own folder -------------------------------------

type StoredFiles = Vec<(String, Vec<u8>)>;

/// A library in the fixture `HOME` (`~/.local/share/wyss-console/arsenal`) holding `skills` for
/// `provider`; the stored bytes `tamper` are changed afterwards, so their digest no longer
/// matches. Returns what a launch names to copy them.
fn stored_skills(
    provider: &str,
    skills: &[(&str, StoredFiles)],
    tamper: Option<&[u8]>,
) -> Vec<super::arsenal::Materialize> {
    use crate::arsenal::store::{self, NewCapability};
    let (mut library, folder) = crate::arsenal::open().unwrap();
    let source = format!("{}_user_skills", provider.trim_end_matches("_code"));
    let wanted = skills
        .iter()
        .map(|(name, files)| {
            let imported = library
                .import(&NewCapability {
                    identity: store::identity(provider, "skill", &source, name),
                    provider: provider.into(),
                    source: source.clone(),
                    kind: "skill".into(),
                    name: (*name).into(),
                    description: "test".into(),
                    origin: format!("~/skills/{name}"),
                    files: files.clone(),
                })
                .unwrap();
            super::arsenal::Materialize {
                capability: imported.capability,
                revision: imported.revision,
                name: (*name).into(),
            }
        })
        .collect();
    drop(library);
    if let Some(bytes) = tamper {
        let db = rusqlite::Connection::open(folder.join("arsenal.sqlite")).unwrap();
        // The library refuses to change stored bytes; the fixture plays a damaged file.
        db.execute_batch("DROP TRIGGER blob_fixed").unwrap();
        let changed = db
            .execute(
                "UPDATE blob SET content = ?2 WHERE hash = ?1",
                rusqlite::params![store::digest(bytes), b"altered".to_vec()],
            )
            .unwrap();
        assert_eq!(changed, 1, "the fixture blob to alter was not found");
    }
    wanted
}

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
}

fn files(list: &[(&str, &[u8])]) -> Vec<(String, Vec<u8>)> {
    list.iter()
        .map(|(path, bytes)| ((*path).to_owned(), bytes.to_vec()))
        .collect()
}

#[test]
fn a_claude_preset_copies_its_stored_skills_into_its_own_folder_and_skips_an_altered_one() {
    let (world, _env, _ring) = claude_home_world();
    let owner = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap()
        .join(".claude");
    fs::create_dir_all(owner.join("skills/design")).unwrap();
    fs::create_dir_all(owner.join("skills/other")).unwrap();
    fs::write(owner.join("skills/design/SKILL.md"), "owner design").unwrap();
    fs::write(owner.join("skills/other/SKILL.md"), "owner other").unwrap();
    let materialize = stored_skills(
        "claude_code",
        &[
            (
                "design",
                files(&[
                    ("SKILL.md", b"stored design"),
                    ("scripts/run.sh", b"#!/bin/sh\necho hi\n"),
                ]),
            ),
            ("tutor", files(&[("SKILL.md", b"stored tutor body")])),
        ],
        Some(b"stored tutor body"),
    );
    let version = super::capabilities::ARSENAL_TEST_VERSION;
    script(
        &world.bin.join("claude"),
        &format!(
            "[ \"$1\" = \"--version\" ] && {{ echo '{version}'; exit 0; }}\n\
             echo \"CFG=[$CLAUDE_CONFIG_DIR]\"; echo CLAUDE-READY; exec cat"
        ),
    );
    // A preset that keeps the owner's environment: the folder of its own is only for the copies.
    let launch = super::arsenal::ArsenalLaunch {
        materialize,
        ..arsenal_launch(&[])
    };
    let preview = world
        .service
        .preview(AgentId::ClaudeCode, None, Some(&launch))
        .unwrap();
    assert!(preview.can_start, "{:?}", preview.not_applicable);
    let reply = arsenal_start(&world, Some(launch), preview.plan_token, version).unwrap();
    let dir = world
        .base
        .join("launch-homes")
        .join(arsenal_launch(&[]).selection);
    let out = world.output_until(&reply.terminal.terminal_id, "CLAUDE-READY");
    assert!(out.contains(&format!("CFG=[{}]", dir.display())), "{out:?}");
    // The altered one is skipped with its reason; the agent started anyway.
    assert_eq!(
        reply.skills_skipped,
        vec![super::arsenal::SkippedSkill {
            name: "tutor".into(),
            reason: "digest_mismatch".into()
        }]
    );
    let skills = dir.join("skills");
    assert!(
        fs::symlink_metadata(&skills).unwrap().is_dir(),
        "the home's skills is a link"
    );
    assert_eq!(mode(&skills), 0o700);
    assert_eq!(
        fs::read(skills.join("design/SKILL.md")).unwrap(),
        b"stored design"
    );
    assert_eq!(mode(&skills.join("design")), 0o700);
    assert_eq!(mode(&skills.join("design/SKILL.md")), 0o600);
    assert_eq!(mode(&skills.join("design/scripts")), 0o700);
    assert_eq!(mode(&skills.join("design/scripts/run.sh")), 0o700);
    assert!(!skills.join("tutor").exists());
    // The owner's other skill is still there, as a link; the owner's own folder is unchanged.
    assert_eq!(
        fs::read_link(skills.join("other")).unwrap(),
        owner.join("skills/other")
    );
    assert_eq!(
        fs::read(owner.join("skills/design/SKILL.md")).unwrap(),
        b"owner design"
    );
    let mut listed: Vec<String> = fs::read_dir(owner.join("skills"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    listed.sort();
    assert_eq!(listed, ["design", "other"]);
    // The next launch of the preset without copies: they go, and the link to the owner's is back.
    let plain = claude_own_home();
    let preview = world
        .service
        .preview(AgentId::ClaudeCode, None, Some(&plain))
        .unwrap();
    let reply = arsenal_start(&world, Some(plain), preview.plan_token, version).unwrap();
    world.output_until(&reply.terminal.terminal_id, "CLAUDE-READY");
    assert!(reply.skills_skipped.is_empty());
    assert_eq!(fs::read_link(&skills).unwrap(), owner.join("skills"));
    assert_eq!(
        fs::read(owner.join("skills/design/SKILL.md")).unwrap(),
        b"owner design"
    );
}

#[test]
fn a_codex_preset_copies_its_stored_skills_into_its_codex_home() {
    let world = World::new();
    script(
        &world.bin.join("codex"),
        &format!(
            "[ \"$1\" = \"--version\" ] && {{ echo '{CODEX_VERSION}'; exit 0; }}\n\
             echo \"CODEXHOME=[$CODEX_HOME]\"; echo CODEX-READY; exec cat"
        ),
    );
    let owner_home = world.home.parent().unwrap().join("owner-home");
    let owner = owner_home.join(".codex");
    fs::create_dir_all(owner.join("skills/design")).unwrap();
    fs::write(owner.join("skills/design/SKILL.md"), "owner design").unwrap();
    fs::write(owner.join("config.toml"), "model = \"x\"\n").unwrap();
    fs::write(owner.join("auth.json"), "FAKE-TOKEN").unwrap();
    let _env = FakeOwnerEnv::new(&owner_home);
    let materialize = stored_skills(
        "codex",
        &[
            ("design", files(&[("SKILL.md", b"stored codex design")])),
            ("tutor", files(&[("SKILL.md", b"stored codex tutor")])),
        ],
        None,
    );
    let launch = super::arsenal::ArsenalLaunch {
        materialize,
        ..arsenal_launch(&[])
    };
    let reply = start_in(&world, AgentId::Codex, &world.home, &launch).unwrap();
    let dir = world
        .base
        .join("launch-homes")
        .join("codex")
        .join(&launch.selection);
    let out = world.output_until(&reply.terminal.terminal_id, "CODEX-READY");
    assert!(
        out.contains(&format!("CODEXHOME=[{}]", dir.display())),
        "{out:?}"
    );
    assert!(reply.skills_skipped.is_empty(), "{:?}", reply.skills_skipped);
    let skills = dir.join("skills");
    assert!(fs::symlink_metadata(&skills).unwrap().is_dir());
    assert_eq!(
        fs::read(skills.join("design/SKILL.md")).unwrap(),
        b"stored codex design"
    );
    assert_eq!(
        fs::read(skills.join("tutor/SKILL.md")).unwrap(),
        b"stored codex tutor"
    );
    assert_eq!(mode(&skills.join("tutor/SKILL.md")), 0o600);
    assert_eq!(
        fs::read(owner.join("skills/design/SKILL.md")).unwrap(),
        b"owner design"
    );
    assert!(
        !owner.join("skills/tutor").exists(),
        "a copy reached the owner's folder"
    );
}

/// A Claude agent that prints the folder it runs with.
fn claude_printing_its_folder(world: &World) {
    let version = super::capabilities::ARSENAL_TEST_VERSION;
    script(
        &world.bin.join("claude"),
        &format!(
            "[ \"$1\" = \"--version\" ] && {{ echo '{version}'; exit 0; }}\n\
             echo \"CFG=[$CLAUDE_CONFIG_DIR]\"; echo CLAUDE-READY; exec cat"
        ),
    );
}

#[test]
fn without_a_stored_token_a_preset_with_copies_starts_as_the_owner_and_says_why() {
    let (world, _env, _ring) = claude_home_world();
    // The key ring holds no token.
    script(&world.base.join("fake-ring/secret-tool"), "exit 1");
    claude_printing_its_folder(&world);
    let launch = super::arsenal::ArsenalLaunch {
        materialize: stored_skills(
            "claude_code",
            &[("tutor", files(&[("SKILL.md", b"tutor without token")]))],
            None,
        ),
        ..arsenal_launch(&[])
    };
    let preview = world
        .service
        .preview(AgentId::ClaudeCode, None, Some(&launch))
        .unwrap();
    assert!(
        preview
            .applied
            .iter()
            .any(|line| line.ends_with("your own environment, with the skills copied from Arsenal.")),
        "{:?}",
        preview.applied
    );
    let reply = arsenal_start(
        &world,
        Some(launch),
        preview.plan_token,
        super::capabilities::ARSENAL_TEST_VERSION,
    )
    .unwrap();
    let out = world.output_until(&reply.terminal.terminal_id, "CLAUDE-READY");
    assert!(out.contains("CFG=[]"), "{out:?}");
    assert_eq!(
        reply.skills_skipped,
        vec![super::arsenal::SkippedSkill {
            name: "tutor".into(),
            reason: "token_missing".into()
        }]
    );
}

#[test]
fn a_codex_preset_with_copies_whose_folder_cannot_be_built_starts_as_the_owner_and_says_why() {
    let world = World::new();
    script(
        &world.bin.join("codex"),
        &format!(
            "[ \"$1\" = \"--version\" ] && {{ echo '{CODEX_VERSION}'; exit 0; }}\n\
             echo \"CODEXHOME=[$CODEX_HOME]\"; echo CODEX-READY; exec cat"
        ),
    );
    // The owner has no `~/.codex`, so no folder of the session's own can be built.
    let owner_home = world.home.parent().unwrap().join("owner-home");
    fs::create_dir_all(&owner_home).unwrap();
    let _env = FakeOwnerEnv::new(&owner_home);
    let launch = super::arsenal::ArsenalLaunch {
        materialize: stored_skills(
            "codex",
            &[("tutor", files(&[("SKILL.md", b"tutor without folder")]))],
            None,
        ),
        ..arsenal_launch(&[])
    };
    let reply = start_in(&world, AgentId::Codex, &world.home, &launch).unwrap();
    let out = world.output_until(&reply.terminal.terminal_id, "CODEX-READY");
    assert!(out.contains("CODEXHOME=[]"), "{out:?}");
    assert_eq!(
        reply.skills_skipped,
        vec![super::arsenal::SkippedSkill {
            name: "tutor".into(),
            reason: "write_failed".into()
        }]
    );
}

#[test]
fn a_codex_preset_without_copies_any_more_gets_the_link_to_the_owner_skills_back() {
    let world = World::new();
    script(
        &world.bin.join("codex"),
        &format!(
            "[ \"$1\" = \"--version\" ] && {{ echo '{CODEX_VERSION}'; exit 0; }}\n\
             echo CODEX-READY; exec cat"
        ),
    );
    let owner_home = world.home.parent().unwrap().join("owner-home");
    let owner = owner_home.join(".codex");
    fs::create_dir_all(owner.join("skills/design")).unwrap();
    fs::write(owner.join("config.toml"), "model = \"x\"\n").unwrap();
    let _env = FakeOwnerEnv::new(&owner_home);
    let launch = super::arsenal::ArsenalLaunch {
        materialize: stored_skills(
            "codex",
            &[("tutor", files(&[("SKILL.md", b"codex tutor again")]))],
            None,
        ),
        ..arsenal_launch(&[])
    };
    let reply = start_in(&world, AgentId::Codex, &world.home, &launch).unwrap();
    world.output_until(&reply.terminal.terminal_id, "CODEX-READY");
    let skills = world
        .base
        .join("launch-homes/codex")
        .join(&launch.selection)
        .join("skills");
    assert!(skills.join("tutor/SKILL.md").is_file());
    let reply = start_in(&world, AgentId::Codex, &world.home, &codex_own_home()).unwrap();
    world.output_until(&reply.terminal.terminal_id, "CODEX-READY");
    assert_eq!(fs::read_link(&skills).unwrap(), owner.join("skills"));
    assert!(!owner.join("skills/tutor").exists());
}
