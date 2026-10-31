use super::*;
use std::os::unix::fs::PermissionsExt;

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
    assert_eq!(ids(&mine), [a.id.clone()]);
    assert_eq!(ids(&theirs), [b.id.clone()]);
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
