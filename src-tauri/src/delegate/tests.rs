use super::*;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Barrier, Mutex};

static ENVIRONMENT: Mutex<()> = Mutex::new(());

struct EnvGuard(Vec<(&'static str, Option<OsString>)>);

impl EnvGuard {
    fn set(values: &[(&'static str, Option<OsString>)]) -> Self {
        let previous = values
            .iter()
            .map(|(key, _)| (*key, std::env::var_os(key)))
            .collect();
        for (key, value) in values {
            unsafe {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
        Self(previous)
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in &self.0 {
            unsafe {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }
}

fn temp(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("delegate-{tag}-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A fake provider executable: a shell script.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn parent(session: &str) -> Parent {
    Parent {
        session_id: session.into(),
        provider: "claude_code".into(),
        project: "/p".into(),
    }
}

fn request(token: &str) -> Request {
    Request {
        token: token.into(),
        provider: "codex".into(),
        model: "gpt-test".into(),
        effort: "low".into(),
        task: "write hello.txt".into(),
        title: None,
        skills: Vec::new(),
        preset: None,
        selection: None,
        timeout_minutes: None,
    }
}

fn record(manager: &Manager, provider: &str, session: &str) -> Record {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let workspace = manager.base.join(&id).join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    Record {
        id,
        parent_session: session.into(),
        provider: provider.into(),
        model: "m".into(),
        workspace: workspace.display().to_string(),
        status: "running".into(),
        created_at: now(),
        ..Record::default()
    }
}

fn spec(executable: PathBuf, dir: PathBuf, deadline: Duration) -> WorkerSpec {
    WorkerSpec {
        executable,
        argv: Vec::new(),
        dir,
        requested_model: "haiku".into(),
        requested_effort: "low".into(),
        excluded: vec!["combo".into()],
        excluded_files: Vec::new(),
        kept_names: vec!["gsap-react".into()],
        kept_files: Vec::new(),
        deadline,
    }
}

fn wait_finished(manager: &Manager, id: &str) -> Record {
    for _ in 0..200 {
        let record = manager.get(id, None).unwrap();
        if record.status != "running" && record.ended_at.is_some() {
            return record;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("the worker did not finish");
}

#[test]
fn concurrent_start_reservations_respect_the_limit() {
    let base = temp("concurrent-limit");
    let manager = Arc::new(Manager::at(base.clone()));
    let sleeper = script(&base, "sleeper", "sleep 30");
    let gate = Arc::new(Barrier::new(2));
    let starts: Vec<_> = (0..2)
        .map(|_| {
            let manager = manager.clone();
            let gate = gate.clone();
            let sleeper = sleeper.clone();
            std::thread::spawn(move || {
                let reservation = manager.reserve_start(1).ok();
                gate.wait();
                let reservation = reservation?;
                let record = record(&manager, "codex", "s1");
                let id = record.id.clone();
                manager
                    .launch_worker_with_reservation(
                        record,
                        spec(sleeper, manager.base.join(&id), Duration::from_secs(30)),
                        Some(reservation),
                    )
                    .ok()
                    .map(|_| id)
            })
        })
        .collect();
    let ids: Vec<_> = starts
        .into_iter()
        .filter_map(|thread| thread.join().unwrap())
        .collect();
    let count = ids.len();
    for id in &ids {
        manager.cancel(id, None).unwrap();
        let _ = wait_finished(&manager, id);
    }
    fs::remove_dir_all(base).unwrap();
    assert_eq!(
        count, 1,
        "both concurrent checks must not pass a limit of one"
    );
}

#[test]
fn cancellation_keeps_the_live_slot_until_the_worker_is_reaped() {
    let base = temp("cancel-slot");
    let manager = Arc::new(Manager::at(base.clone()));
    let worker = script(&base, "codex", "trap '' TERM; echo ready > ready; sleep 2");
    let record = record(&manager, "codex", "s1");
    let id = record.id.clone();
    manager
        .launch_worker(
            record,
            spec(worker, manager.base.join(&id), Duration::from_secs(30)),
        )
        .unwrap();
    for _ in 0..100 {
        if manager.base.join(&id).join("workspace/ready").exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(manager.base.join(&id).join("workspace/ready").exists());

    let done = manager.cancel(&id, None).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let still_running = manager.running();
    let pgid = lock(&manager.live).workers.get(&id).unwrap().pgid;
    let child_alive = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pgid), None).is_ok();
    let finished_early = manager.get(&id, None).unwrap().finished();
    let _ = wait_finished(&manager, &id);
    fs::remove_dir_all(base).unwrap();

    assert!(
        child_alive,
        "the TERM-resistant fake should still be running at the check"
    );
    assert_eq!(
        still_running, 1,
        "the active process group still owns its slot"
    );
    assert!(
        !finished_early,
        "cancel is not complete until the child is reaped"
    );
    assert_eq!(done.status, "cancelled");
    assert_eq!(manager.running(), 0);
}

#[test]
fn completed_workers_are_removed_from_the_live_map() {
    let base = temp("live-prune");
    let manager = Manager::at(base.clone());
    let worker = script(
        &base,
        "codex",
        r#"echo '{"type":"thread.started","thread_id":"t-none"}'
echo '{"type":"item.completed","item":{"type":"agent_message","text":"done"}}'
echo '{"type":"turn.completed"}'"#,
    );
    let record = record(&manager, "codex", "s1");
    let id = record.id.clone();
    manager
        .launch_worker(
            record,
            spec(worker, manager.base.join(&id), Duration::from_secs(30)),
        )
        .unwrap();
    let _ = wait_finished(&manager, &id);
    let retained = lock(&manager.live).workers.contains_key(&id);
    fs::remove_dir_all(base).unwrap();
    assert!(!retained, "a reaped worker must not remain in the live map");
}

#[test]
fn preview_prepares_without_launch_capacity_or_history_and_cleans_its_workspace() {
    let _env_lock = ENVIRONMENT.lock().unwrap();
    let root = temp("preview");
    let home = root.join("home");
    let config = root.join("config");
    let data = root.join("data");
    let history = root.join("delegations");
    let skill_text = "---\nname: demo\ndescription: Preview fixture\n---\nDemo skill.\n";
    fs::create_dir_all(home.join(".claude/skills/demo")).unwrap();
    fs::write(home.join(".claude/skills/demo/SKILL.md"), skill_text).unwrap();
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(data.join("wyss-console/arsenal")).unwrap();
    let _env = EnvGuard::set(&[
        ("HOME", Some(home.as_os_str().to_owned())),
        ("XDG_CONFIG_HOME", Some(config.as_os_str().to_owned())),
        ("XDG_DATA_HOME", Some(data.as_os_str().to_owned())),
        (
            "CLAUDE_CONFIG_DIR",
            Some(home.join(".claude").into_os_string()),
        ),
        ("CODEX_HOME", None),
    ]);

    let db = data.join("wyss-console/arsenal/arsenal.sqlite");
    let mut store =
        crate::arsenal::store::Store::open(rusqlite::Connection::open(db).unwrap()).unwrap();
    let imported = store
        .import(&crate::arsenal::store::NewCapability {
            identity: "preview-fixture-demo".into(),
            provider: "claude_code".into(),
            source: "claude_user_skills".into(),
            kind: "skill".into(),
            name: "demo".into(),
            description: "Preview fixture".into(),
            origin: "test fixture".into(),
            files: vec![("SKILL.md".into(), skill_text.as_bytes().to_vec())],
        })
        .unwrap();
    let preset = store
        .save_preset(&crate::arsenal::store::PresetInput {
            id: None,
            name: "Preview preset".into(),
            description: String::new(),
            items: vec![crate::arsenal::store::Item {
                capability: imported.capability.clone(),
                revision: None,
            }],
        })
        .unwrap();
    let selection = store
        .save_selection(&crate::arsenal::store::SelectionInput {
            id: None,
            name: "Preview selection".into(),
            provider: "claude_code".into(),
            items: Vec::new(),
            composites: Vec::new(),
        })
        .unwrap();
    drop(store);

    let provider = script(
        &root,
        "fake-claude",
        &format!(
            "if [ \"$1\" = \"--version\" ]; then echo '{}'; else echo started > \"$HOME/provider-task-ran\"; fi",
            crate::native::capabilities::ARSENAL_TEST_VERSION
        ),
    );
    let manager = Manager::at(history.clone());
    let token = manager.register(parent("s1"));
    let outstanding = manager.reserve_start(1).unwrap();
    let preview = manager
        .preview_with_policy(
            Request {
                provider: "claude_code".into(),
                model: "haiku".into(),
                effort: "low".into(),
                preset: Some(preset.id.clone()),
                selection: Some(selection.id.clone()),
                ..request(&token)
            },
            &provider,
            Policy::default(),
            None,
            "test_policy".into(),
        )
        .unwrap();
    let json = serde_json::to_value(&preview).unwrap();
    assert!(preview.preview);
    assert_eq!(preview.provider, "claude_code");
    assert_eq!(preview.model, "haiku");
    assert_eq!(preview.effort, "low");
    assert_eq!(preview.preset.as_deref(), Some(preset.id.as_str()));
    assert_eq!(preview.selection.as_deref(), Some(selection.id.as_str()));
    assert_eq!(preview.skills.len(), 1);
    assert_eq!(preview.skills[0]["capability"], imported.capability);
    assert_eq!(preview.skills[0]["revision"], imported.revision);
    assert_eq!(preview.policy_source, "test_policy");
    assert_eq!(
        preview.applied.provider_version.as_deref(),
        Some(crate::native::capabilities::ARSENAL_TEST_VERSION)
    );
    assert_eq!(preview.applied.config_hash.len(), 64);
    assert!(json.get("id").is_none());
    assert!(json.get("execution_id").is_none());
    assert_eq!(manager.running(), 1);
    assert!(manager.records().is_empty());
    assert!(!home.join("provider-task-ran").exists());
    assert_eq!(fs::read_dir(&history).unwrap().count(), 0);
    drop(outstanding);
    assert_eq!(manager.running(), 0);
    drop(_env);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn preview_and_start_return_the_same_request_validation_error() {
    let base = temp("preview-errors");
    let manager = Manager::at(base.clone());
    let token = manager.register(parent("s1"));
    let invalid = Request {
        provider: "claude_code".into(),
        model: "not-allowed".into(),
        ..request(&token)
    };
    let preview = manager
        .preview_with_policy(
            invalid.clone(),
            Path::new("/unused"),
            Policy::default(),
            None,
            "defaults".into(),
        )
        .unwrap_err();
    let start = manager
        .start_with_policy(invalid, Path::new("/unused"), Policy::default(), None)
        .unwrap_err();
    assert_eq!((preview.code, preview.reasons), (start.code, start.reasons));
    assert_eq!(manager.running(), 0);
    assert!(manager.records().is_empty());
    assert_eq!(fs::read_dir(&base).unwrap().count(), 0);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn old_records_default_new_preset_and_selection_fields() {
    let record = Record {
        preset: Some("p-1".into()),
        selection: Some("s-1".into()),
        ..Record::default()
    };
    let mut old = serde_json::to_value(record).unwrap();
    old.as_object_mut().unwrap().remove("preset");
    old.as_object_mut().unwrap().remove("selection");
    let restored: Record = serde_json::from_value(old).unwrap();
    assert_eq!(restored.preset, None);
    assert_eq!(restored.selection, None);

    let retained: Record = serde_json::from_value(
        serde_json::to_value(Record {
            preset: Some("p-2".into()),
            selection: Some("s-2".into()),
            ..Record::default()
        })
        .unwrap(),
    )
    .unwrap();
    assert_eq!(retained.preset.as_deref(), Some("p-2"));
    assert_eq!(retained.selection.as_deref(), Some("s-2"));
}

#[test]
fn options_label_sources_and_only_report_verified_runtime_pairs() {
    let base = temp("options-runtime");
    let manager = Manager::at(base.clone());
    let verified = Record {
        id: "verified".into(),
        provider: "codex".into(),
        status: "succeeded".into(),
        verification: Verification {
            state: "verified".into(),
            model: Some("gpt-test".into()),
            effort: Some("low".into()),
            ..Verification::default()
        },
        ..Record::default()
    };
    let unverified = Record {
        id: "unverified".into(),
        provider: "codex".into(),
        status: "succeeded".into(),
        verification: Verification {
            state: "unavailable".into(),
            model: Some("gpt-other".into()),
            effort: Some("high".into()),
            ..Verification::default()
        },
        ..Record::default()
    };
    manager.save(&verified);
    manager.save(&unverified);
    let options = manager.options("claude_code", None);
    assert_eq!(options["model_source"], "owner_policy_and_cli_syntax");
    assert_eq!(options["effort_source"], "owner_policy_and_cli_syntax");
    assert!(options["runtime_confirmed"].as_array().unwrap().is_empty());
    let executable = script(
        &base,
        "codex",
        r#"echo '{"models":[{"slug":"gpt-test","visibility":"list","supported_reasoning_levels":[{"effort":"low"}]}]}'"#,
    );
    let codex = manager.options("codex", Some(&executable));
    assert_eq!(codex["model_source"], "local_provider_catalogue");
    assert_eq!(codex["models"][0]["source"], "local_provider_catalogue");
    assert_eq!(
        codex["runtime_confirmed"],
        json!([{"model": "gpt-test", "effort": "low"}])
    );
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn a_successful_provider_exit_cleans_up_its_descendant_processes() {
    let base = temp("descendant-cleanup");
    let manager = Manager::at(base.clone());
    let worker = script(
        &base,
        "codex",
        r#"sleep 60 & echo $! > child.pid
echo '{"type":"thread.started","thread_id":"t-none"}'
echo '{"type":"item.completed","item":{"type":"agent_message","text":"done"}}'
echo '{"type":"turn.completed"}'"#,
    );
    let record = record(&manager, "codex", "s1");
    let id = record.id.clone();
    let dir = manager.base.join(&id);
    manager
        .launch_worker(record, spec(worker, dir.clone(), Duration::from_secs(30)))
        .unwrap();
    let done = wait_finished(&manager, &id);
    let child: i32 = fs::read_to_string(dir.join("workspace/child.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let remained = nix::sys::signal::kill(nix::unistd::Pid::from_raw(child), None).is_ok();
    if remained {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(child),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    fs::remove_dir_all(base).unwrap();
    assert_eq!(done.status, "succeeded");
    assert!(
        !remained,
        "the worker's process group must not outlive success"
    );
}

#[test]
fn a_cancelled_worker_cannot_leave_a_setsid_descendant_running() {
    let base = temp("setsid-escape");
    let manager = Manager::at(base.clone());
    let worker = script(
        &base,
        "codex",
        "setsid sh -c 'echo ready > escape-ready; sleep 1; echo escaped > escaped.marker' >/dev/null 2>&1 & echo ready > ready; sleep 30",
    );
    let record = record(&manager, "codex", "s1");
    let id = record.id.clone();
    let workspace = manager.base.join(&id).join("workspace");
    manager
        .launch_worker(
            record,
            spec(worker, manager.base.join(&id), Duration::from_secs(30)),
        )
        .unwrap();
    for _ in 0..200 {
        if workspace.join("escape-ready").exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(workspace.join("escape-ready").exists());
    manager.cancel(&id, None).unwrap();
    let done = wait_finished(&manager, &id);
    std::thread::sleep(Duration::from_millis(1_200));
    let escaped = workspace.join("escaped.marker").exists();
    fs::remove_dir_all(base).unwrap();
    assert_eq!(done.status, "cancelled");
    assert!(!escaped, "a setsid descendant must die with its worker");
}

#[test]
fn a_policy_file_is_taken_only_within_its_bounds() {
    assert_eq!(parse_policy("{}"), Some(Policy::default()));
    let custom = parse_policy(r#"{"max_concurrent": 3, "max_minutes": 5}"#).unwrap();
    assert_eq!((custom.max_concurrent, custom.max_minutes), (3, 5));
    for bad in [
        r#"{"max_concurrent": 0}"#,
        r#"{"max_minutes": 500}"#,
        r#"{"providers": ["opencode"]}"#,
        r#"{"unknown": true}"#,
        "not json",
    ] {
        assert_eq!(parse_policy(bad), None, "{bad}");
    }
}

#[test]
fn missing_policy_defaults_but_invalid_or_unreadable_policy_blocks_start() {
    let base = temp("policy-load");
    let missing = base.join("missing.json");
    let (policy, note, source) = load_policy_from(Some(&missing));
    assert_eq!(policy, Policy::default());
    assert!(note.is_none());
    assert_eq!(source, "defaults");

    let invalid = base.join("invalid.json");
    fs::write(&invalid, "not json").unwrap();
    let (policy, note, source) = load_policy_from(Some(&invalid));
    assert_eq!(policy, Policy::default());
    assert!(note.is_some());
    assert_eq!(source, invalid.display().to_string());

    let unreadable = base.join("unreadable");
    fs::create_dir(&unreadable).unwrap();
    let (_, note, source) = load_policy_from(Some(&unreadable));
    assert!(note.is_some());
    assert_eq!(source, unreadable.display().to_string());

    let manager = Manager::at(base.clone());
    let token = manager.register(parent("s1"));
    let refusal = manager
        .start_with_policy(
            request(&token),
            Path::new("/unused-provider"),
            Policy::default(),
            note,
        )
        .unwrap_err();
    fs::remove_dir_all(base).unwrap();
    assert_eq!(refusal.code, "policy_invalid");
}

#[test]
fn duplicate_capabilities_preserve_pins_and_reject_conflicts() {
    let item = |revision| store::Item {
        capability: "skill-a".into(),
        revision,
    };
    let conflict = merge_capability_items(vec![item(Some(1)), item(Some(99))]).unwrap_err();
    assert_eq!(conflict.code, "capability_revision_conflict");

    for items in [
        vec![item(None), item(Some(99))],
        vec![item(Some(99)), item(None)],
    ] {
        let merged = merge_capability_items(items).unwrap();
        assert_eq!(merged, [item(Some(99))]);
    }

    let merged = merge_capability_items(vec![item(None), item(None)]).unwrap();
    assert_eq!(merged, [item(None)]);
}

#[test]
fn codex_verification_records_turn_context_model_and_effort_and_flags_mismatch() {
    let skills = "### Skill roots\n- `r0` = `/h/.codex/skills`\n### Available skills\n- demo: Demo (file: r0/demo/SKILL.md)\n";
    let transcript = [
        serde_json::json!({"type":"session_meta","payload":{"id":"t-1"}}).to_string(),
        serde_json::json!({
            "type":"response_item",
            "payload":{"content":[{"text":skills}]}
        })
        .to_string(),
        serde_json::json!({
            "type":"turn_context",
            "payload":{"model":"gpt-6-luna","effort":"high","turn_id":"turn-1"}
        })
        .to_string(),
    ]
    .join("\n");
    let verified = codex_verification_from_text(
        &transcript,
        "t-1",
        "gpt-6-luna",
        "high",
        &[],
        &[],
        "fake rollout".into(),
    );
    assert_eq!(verified.state, "verified");
    assert_eq!(verified.model.as_deref(), Some("gpt-6-luna"));
    assert_eq!(verified.effort.as_deref(), Some("high"));

    let mismatch = codex_verification_from_text(
        &transcript,
        "t-1",
        "gpt-6.1-sol",
        "low",
        &[],
        &[],
        "fake rollout".into(),
    );
    assert_eq!(mismatch.state, "mismatch");
    assert_eq!(mismatch.problems.len(), 2);
}

#[test]
fn a_record_left_running_by_a_stopped_host_is_interrupted() {
    let base = temp("restart");
    let first = Manager::at(base.clone());
    let left = record(&first, "codex", "s1");
    first.save(&left);
    let second = Manager::at(base.clone());
    let again = second.get(&left.id, None).unwrap();
    assert_eq!(again.status, "interrupted");
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn a_start_is_refused_without_its_right_and_outside_the_policy() {
    let base = temp("refuse");
    let manager = Manager::at(base.clone());
    let codex = script(
        &base,
        "codex",
        r#"echo '{"models":[{"slug":"gpt-test","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}]},{"slug":"gpt-hidden","visibility":"hide","supported_reasoning_levels":[{"effort":"low"}]}]}'"#,
    );
    let code = |request: Request| manager.start(request, &codex).unwrap_err().code;
    assert_eq!(code(request("not-a-token")), "delegation_unauthorized");
    let token = manager.register(parent("s1"));
    assert_eq!(
        code(Request {
            task: " ".into(),
            ..request(&token)
        }),
        "task_invalid"
    );
    assert_eq!(
        code(Request {
            timeout_minutes: Some(21),
            ..request(&token)
        }),
        "timeout_not_allowed"
    );
    assert_eq!(
        code(Request {
            provider: "opencode".into(),
            ..request(&token)
        }),
        "provider_unsupported"
    );
    assert_eq!(
        code(Request {
            model: "gpt-hidden".into(),
            ..request(&token)
        }),
        "model_unsupported"
    );
    assert_eq!(
        code(Request {
            model: "gpt-other".into(),
            ..request(&token)
        }),
        "model_unsupported"
    );
    assert_eq!(
        code(Request {
            effort: "max".into(),
            ..request(&token)
        }),
        "effort_unsupported"
    );
    let claude = |model: &str, effort: &str| Request {
        provider: "claude_code".into(),
        model: model.into(),
        effort: effort.into(),
        ..request(&token)
    };
    assert_eq!(code(claude("gpt-test", "low")), "model_unsupported");
    assert_eq!(code(claude("haiku", "ultra")), "effort_unsupported");
    // Two running workers fill the default policy: a third is refused before anything else.
    let sleeper = script(&base, "sleeper", "sleep 30");
    for _ in 0..2 {
        let r = record(&manager, "codex", "s1");
        let dir = manager.base.join(&r.id);
        manager
            .launch_worker(r, spec(sleeper.clone(), dir, Duration::from_secs(30)))
            .unwrap();
    }
    assert_eq!(code(request(&token)), "delegation_limit");
    for r in manager.list(None).unwrap() {
        manager.cancel(&r.id, None).unwrap();
        let _ = wait_finished(&manager, &r.id);
    }
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn a_worker_stream_becomes_its_result_and_its_files_are_listed() {
    let base = temp("run");
    let manager = Manager::at(base.clone());
    let worker = script(
        &base,
        "codex",
        r#"test -z "$WYSS_DELEGATION_TOKEN" || exit 9
echo hi > hello.txt
echo '{"type":"thread.started","thread_id":"t-none"}'
echo '{"type":"item.completed","item":{"type":"agent_message","text":"wrote hello.txt"}}'
echo '{"type":"turn.completed","usage":{"output_tokens":3}}'"#,
    );
    let r = record(&manager, "codex", "s1");
    let dir = manager.base.join(&r.id);
    let id = r.id.clone();
    manager
        .launch_worker(r, spec(worker, dir.clone(), Duration::from_secs(30)))
        .unwrap();
    let done = wait_finished(&manager, &id);
    assert_eq!(done.status, "succeeded", "{:?}", done.error);
    assert_eq!(done.result.as_deref(), Some("wrote hello.txt"));
    assert_eq!(done.artifacts, [json!({"path": "hello.txt", "size": 3})]);
    // A Codex session record that cannot be found is said, not assumed verified.
    assert_eq!(done.verification.state, "unavailable");
    assert!(
        fs::read_to_string(dir.join("events.jsonl"))
            .unwrap()
            .contains("turn.completed")
    );
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn a_cancel_and_a_deadline_stop_the_whole_process_group() {
    let base = temp("stop");
    let manager = Manager::at(base.clone());
    let worker = script(&base, "codex", "sleep 60 & echo $! > child.pid; wait");
    let r = record(&manager, "codex", "s1");
    let (id, dir) = (r.id.clone(), manager.base.join(&r.id));
    manager
        .launch_worker(
            r,
            spec(worker.clone(), dir.clone(), Duration::from_secs(60)),
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(manager.cancel(&id, None).unwrap().status, "cancelled");
    let done = wait_finished(&manager, &id);
    assert_eq!(done.status, "cancelled");
    let grandchild: i32 = fs::read_to_string(dir.join("workspace/child.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(nix::sys::signal::kill(nix::unistd::Pid::from_raw(grandchild), None).is_err());

    let late = record(&manager, "codex", "s1");
    let late_id = late.id.clone();
    let late_dir = manager.base.join(&late.id);
    manager
        .launch_worker(late, spec(worker, late_dir, Duration::from_secs(1)))
        .unwrap();
    assert_eq!(wait_finished(&manager, &late_id).status, "timed_out");
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn a_claude_worker_reporting_another_configuration_is_stopped() {
    let base = temp("mismatch");
    let manager = Manager::at(base.clone());
    let worker = script(
        &base,
        "claude",
        r#"echo '{"type":"system","subtype":"init","model":"claude-opus-5-5","skills":["combo"],"plugins":[],"mcp_servers":[]}'
sleep 30"#,
    );
    let r = record(&manager, "claude_code", "s1");
    let (id, dir) = (r.id.clone(), manager.base.join(&r.id));
    manager
        .launch_worker(r, spec(worker, dir, Duration::from_secs(30)))
        .unwrap();
    let done = wait_finished(&manager, &id);
    assert_eq!(done.status, "failed");
    assert_eq!(done.verification.state, "mismatch");
    assert_eq!(
        done.verification.problems.len(),
        3,
        "{:?}",
        done.verification.problems
    );
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn an_orchestrator_sees_and_stops_only_its_own_delegations() {
    let base = temp("owner");
    let manager = Manager::at(base.clone());
    let mine = manager.register(parent("s1"));
    let theirs = manager.register(parent("s2"));
    let a = record(&manager, "codex", "s1");
    let b = record(&manager, "codex", "s2");
    manager.save(&a);
    manager.save(&b);
    let ids = |token: &str| -> Vec<String> {
        manager
            .list(Some(token))
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect()
    };
    assert_eq!(ids(&mine), std::slice::from_ref(&a.id));
    assert_eq!(ids(&theirs), std::slice::from_ref(&b.id));
    assert_eq!(
        manager.get(&b.id, Some(&mine)).unwrap_err().code,
        "delegation_unknown"
    );
    assert_eq!(
        manager.cancel(&b.id, Some(&mine)).unwrap_err().code,
        "delegation_unknown"
    );
    assert_eq!(
        manager.list(Some("forged")).unwrap_err().code,
        "delegation_unauthorized"
    );
    assert_eq!(manager.list(None).unwrap().len(), 2);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn a_worker_line_longer_than_the_limit_is_dropped_and_reading_goes_on() {
    let input = b"first\n0123456789abcdef\nlast-without-newline";
    let mut reader = std::io::BufReader::with_capacity(4, &input[..]);
    assert_eq!(next_line(&mut reader, 10), Some(Some("first".into())));
    assert_eq!(next_line(&mut reader, 10), Some(None));
    assert_eq!(
        next_line(&mut reader, 30),
        Some(Some("last-without-newline".into()))
    );
    assert_eq!(next_line(&mut reader, 10), None);
}

/// The layout of the owner's state: the delegations live inside a folder the worker must not see
/// (sessions' delegation rights, credentials, sockets), next to a read-only policy folder.
#[test]
fn a_worker_sees_neither_wyss_secrets_nor_sockets_and_cannot_change_the_policy() {
    let root = temp("isolation");
    let hidden = root.join("state/wyss-console");
    let policy = root.join("config/wyss-console");
    fs::create_dir_all(hidden.join("native/s1")).unwrap();
    fs::create_dir_all(&policy).unwrap();
    fs::write(
        hidden.join("native/s1/wyss-delegate.mcp.json"),
        "secret-token",
    )
    .unwrap();
    fs::write(policy.join("delegation.json"), "{}").unwrap();
    let manager = Manager::at(hidden.join("delegations")).with_isolation(Isolation {
        hidden: vec![hidden.clone(), root.join("absent")],
        read_only: vec![policy.clone()],
    });
    let bin = temp("isolation-bin");
    let worker = script(
        &bin,
        "worker",
        &format!(
            r#"if [ -e '{token}' ]; then echo leaked > seen.txt; else echo hidden > seen.txt; fi
if echo '{{"max_concurrent":8}}' > '{policy}' 2>/dev/null; then echo changed >> seen.txt; else echo kept >> seen.txt; fi
echo own > ../own.txt && echo own-writable >> seen.txt
echo '{{"type":"thread.started","thread_id":"t-iso"}}'
echo '{{"type":"item.completed","item":{{"type":"agent_message","text":"done"}}}}'
echo '{{"type":"turn.completed"}}'"#,
            token = hidden.join("native/s1/wyss-delegate.mcp.json").display(),
            policy = policy.join("delegation.json").display(),
        ),
    );
    let record = record(&manager, "codex", "s1");
    let id = record.id.clone();
    let dir = manager.base.join(&id);
    manager
        .launch_worker(record, spec(worker, dir.clone(), Duration::from_secs(30)))
        .unwrap();
    let done = wait_finished(&manager, &id);
    let seen = fs::read_to_string(dir.join("workspace/seen.txt")).unwrap();
    let token_kept = fs::read_to_string(hidden.join("native/s1/wyss-delegate.mcp.json")).unwrap();
    let policy_kept = fs::read_to_string(policy.join("delegation.json")).unwrap();
    let absent_created = root.join("absent").exists();
    fs::remove_dir_all(&root).unwrap();
    fs::remove_dir_all(&bin).unwrap();
    assert_eq!(done.status, "succeeded", "{:?}", done.error);
    assert_eq!(seen, "hidden\nkept\nown-writable\n");
    assert_eq!(token_kept, "secret-token");
    assert_eq!(policy_kept, "{}");
    assert!(
        !absent_created,
        "a missing folder must not be created on the host"
    );
}

#[test]
fn a_provider_command_that_hangs_or_floods_is_cut_off() {
    let bin = temp("bounded");
    let slow = script(&bin, "slow", "sleep 30");
    let flood = script(&bin, "flood", "yes model");
    let fine = script(&bin, "fine", "echo '{\"models\":[]}'");
    let started = Instant::now();
    let hung = bounded_stdout(&mut Command::new(&slow), 1024, Duration::from_millis(300));
    let elapsed = started.elapsed();
    let flooded = bounded_stdout(&mut Command::new(&flood), 1024, Duration::from_secs(5));
    let answered = bounded_stdout(&mut Command::new(&fine), 1024, Duration::from_secs(5));
    fs::remove_dir_all(&bin).unwrap();
    assert!(hung.is_none());
    assert!(elapsed < Duration::from_secs(5), "took {elapsed:?}");
    assert!(flooded.is_none());
    assert_eq!(answered.unwrap(), b"{\"models\":[]}\n");
}

#[test]
fn the_folders_a_worker_cannot_see_come_from_the_hosts_own_variables() {
    let _serial = lock(&ENVIRONMENT);
    let _env = EnvGuard::set(&[
        ("HOME", Some("/h".into())),
        ("XDG_STATE_HOME", None),
        ("XDG_CONFIG_HOME", Some("/cfg".into())),
        ("XDG_DATA_HOME", Some("relative".into())),
        ("XDG_RUNTIME_DIR", Some("/run/user/7".into())),
    ]);
    let isolation = Isolation::from_environment();
    let paths = |list: &[PathBuf]| {
        list.iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        paths(&isolation.hidden),
        [
            "/h/.local/state/wyss-console",
            "/h/.local/state/wyss-console-f8",
            "/run/user/7/wyss-console",
            "/run/user/7/wyss-console-f8",
        ]
    );
    // A relative XDG folder is ignored, as the host itself ignores it.
    assert_eq!(
        paths(&isolation.read_only),
        ["/cfg/wyss-console", "/h/.local/share/wyss-console"]
    );
}
