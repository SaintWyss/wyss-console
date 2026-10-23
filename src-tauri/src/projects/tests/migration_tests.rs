//! M1-07: the legacy registry and layout corpus. Duplicate roots, an absent host, missing
//! agents, malformed or newer files, sixteen tabs, changed sources, interrupted publication and
//! a second import; a deterministic map; no `Registry::open`; no remade shell; no legacy write.
use super::*;
use crate::projects::migration::{
    ConflictKind, Mode as ImportMode, Plan, Sources, commit_incremental, commit_initial, derive,
    plan_incremental, plan_initial, read_sources, sha256,
};
use wyss_console::contracts::m1::records::Target;

const ALIAS: &str = "/home/fixture/work/a-link";
const GONE: &str = "/home/fixture/work/gone";
const T_NATIVE: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const T_SHELL_A: &str = "aaaaaaaa-0000-4000-8000-000000000002";
const T_SHELL_X: &str = "aaaaaaaa-0000-4000-8000-000000000003";
const GONE_1: &str = "aaaaaaaa-0000-4000-8000-0000000000f1";
const GONE_2: &str = "aaaaaaaa-0000-4000-8000-0000000000f2";
const GONE_3: &str = "aaaaaaaa-0000-4000-8000-0000000000f3";
const SESSION: &str = "0123456789abcdef0123456789abcdef";
const MIGRATION: &str = "99999999-0000-4000-8000-000000000001";

fn registry(paths: &[&str], sessions: &[(&str, &str, &str)]) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema": 1,
        "projects": paths.iter().map(|p| json!({"path": p, "last_used": 1})).collect::<Vec<_>>(),
        "sessions": sessions.iter().map(|(id, project, state)| json!({
            "id": id, "agent_id": "codex", "agent_name": "Codex", "project": project,
            "state": state, "started_at": 1, "through_shell": true, "restarts": 0
        })).collect::<Vec<_>>(),
    }))
    .unwrap()
}

fn leaf(id: &str) -> Value {
    json!({"kind": "leaf", "id": id})
}

fn split(a: Value, b: Value) -> Value {
    json!({"kind": "split", "dir": "row", "ratio": 0.5, "a": a, "b": b})
}

fn layout(tabs: Vec<Value>) -> Vec<u8> {
    let mut titles = serde_json::Map::new();
    for tab in &tabs {
        let mut stack = vec![&tab["root"]];
        while let Some(node) = stack.pop() {
            if node["kind"] == "leaf" {
                titles.insert(node["id"].as_str().unwrap().to_owned(), json!({"title": "shell", "cwd": "", "native": false}));
            } else {
                stack.push(&node["a"]);
                stack.push(&node["b"]);
            }
        }
    }
    serde_json::to_vec(&json!({"version": 1, "tabs": tabs, "active": 0, "terminals": titles})).unwrap()
}

fn tab(root: Value) -> Value {
    json!({"root": root, "focus": null})
}

fn terminal(id: &str, session: Option<&str>, cwd: &str) -> TerminalRow {
    TerminalRow {
        terminal_id: id.to_owned(),
        running: true,
        cwd: Some(cwd.to_owned()),
        native_session: session.map(str::to_owned),
    }
}

fn owner() -> Arc<FakeOwner> {
    let owner = FakeOwner::new();
    owner.add_root(ROOT_A, 64768, 1001);
    owner.add_root(ROOT_B, 64768, 1002);
    owner.script(|s| {
        s.roots.insert(
            ALIAS.to_owned(),
            Ok(RootInfo {
                path: ROOT_A.to_owned(),
                device: 64768,
                inode: 1001,
                kind: CheckoutKind::Git,
                native_eligible: true,
                config_eligible: true,
            }),
        );
        s.roots.insert(GONE.to_owned(), Err(RootError::Missing));
    });
    owner
}

fn host(terminals: Vec<TerminalRow>) -> HostView {
    HostView {
        epoch: "h1:1:1:100:1000".into(),
        features: vec![],
        terminals,
    }
}

/// The corpus: two registered folders (one listed twice), a missing one, a live Native session
/// in A, a shell in A, a shell elsewhere, and three tabs: one mixing A and Scratch, one in A
/// with a terminal that is gone, and one with nothing alive.
fn corpus() -> (Sources, HostView) {
    let sources = Sources {
        registry: Some(registry(
            &[ROOT_A, ROOT_A, ROOT_B, GONE],
            &[(SESSION, ROOT_A, "running"), ("ffffffffffffffffffffffffffffffff", ROOT_B, "stopped")],
        )),
        layout: Some(layout(vec![
            tab(split(leaf(T_NATIVE), leaf(T_SHELL_X))),
            tab(split(leaf(T_SHELL_A), leaf(GONE_1))),
            tab(split(leaf(GONE_2), leaf(GONE_3))),
        ])),
    };
    let view = host(vec![
        terminal(T_NATIVE, Some(SESSION), ROOT_A),
        terminal(T_SHELL_A, None, ROOT_A),
        terminal(T_SHELL_X, None, "/tmp"),
    ]);
    (sources, host_ok(view))
}

fn host_ok(view: HostView) -> HostView {
    view
}

fn plan(env: &FakeEnv, owner: &FakeOwner, sources: &Sources, view: Option<&HostView>) -> Plan {
    plan_initial(env, owner, sources, view, MIGRATION).expect("a plan")
}

fn kinds(plan: &Plan) -> Vec<ConflictKind> {
    plan.summary.conflicts.iter().map(|c| c.kind).collect()
}

// ---- the plan ------------------------------------------------------------------------------

#[test]
fn nothing_to_import_is_a_valid_scratch_only_store() {
    let env = FakeEnv::new();
    let p = plan(&env, &owner(), &Sources { registry: None, layout: None }, None);
    assert!(p.summary.can_commit);
    assert_eq!(p.store.workspaces.len(), 1);
    assert!(p.store.projects.is_empty() && p.store.migration.is_none() && p.snapshot.is_empty());
    assert_eq!(kinds(&p), [ConflictKind::HostOffline]);
}

#[test]
fn the_corpus_maps_projects_sessions_and_tabs_by_what_can_be_proven() {
    let env = FakeEnv::new();
    let (sources, view) = corpus();
    let o = owner();
    let p = plan(&env, &o, &sources, Some(&view));
    // Two real roots (A once, though listed twice); the missing one is reported, not invented;
    // the mention of A twice by the same spelling is not an alias.
    assert_eq!(p.store.projects.len(), 2);
    assert_eq!(kinds(&p), [ConflictKind::RootUnavailable]);
    assert!(p.summary.can_commit);
    let a = p.store.bindings.iter().find(|b| b.path.as_str() == ROOT_A).unwrap();
    let project_a = a.project_id.clone();
    // Three live terminals become three associations with proven origins only.
    assert_eq!(p.store.sessions.len(), 3);
    let by_terminal = |id: &str| {
        p.store.sessions.iter().find(|s| match &s.target {
            Target::Native { terminal_id, .. } | Target::Shell { terminal_id, .. } => terminal_id.as_str() == id,
            _ => false,
        }).unwrap()
    };
    assert_eq!(by_terminal(T_NATIVE).origin_project_id.as_ref(), Some(&project_a));
    assert_eq!(by_terminal(T_SHELL_A).origin_project_id.as_ref(), Some(&project_a), "exact cwd = root");
    assert!(by_terminal(T_SHELL_X).origin_project_id.is_none(), "nothing proves it");
    // Tab 1 mixed A and Scratch: it is split by origin. Tab 2 stays in A with an empty
    // placement for the terminal that is gone (never remade). Tab 3 had nothing alive.
    let a_workspaces: Vec<_> = p.store.workspaces.iter().filter(|w| w.project_id.as_ref() == Some(&project_a)).collect();
    assert_eq!(a_workspaces.len(), 2);
    assert_eq!(a_workspaces[0].label.as_str(), "Tab 1");
    assert_eq!(a_workspaces[1].leaves.len(), 2);
    assert_eq!(a_workspaces[1].leaves.iter().filter(|l| l.association_id.is_none()).count(), 1);
    let project = p.store.projects.iter().find(|x| x.id == project_a).unwrap();
    assert_eq!(project.default_workspace_id.as_ref(), Some(&a_workspaces[0].id));
    let scratch = p.store.workspaces.iter().find(|w| w.project_id.is_none()).unwrap();
    assert_eq!(scratch.leaves.len(), 1);
    assert_eq!(p.summary.counts.tabs_skipped, 1);
    // B has no tab: it gets an empty default workspace.
    let b = p.store.projects.iter().find(|x| x.id != project_a).unwrap();
    assert!(p.store.workspaces.iter().any(|w| Some(&w.id) == b.default_workspace_id.as_ref() && w.leaves.is_empty()));
    // Every legacy record that mapped is in the deterministic map, and the digests are the bytes'.
    let migration = p.store.migration.as_ref().unwrap();
    assert_eq!(migration.sources.len(), 2);
    assert_eq!(
        migration.sources[0].digest.as_str(),
        sha256(sources.registry.as_ref().unwrap())
    );
    assert!(migration.map.len() >= 5);
    // The whole store satisfies the contract and re-encodes.
    wyss_console::contracts::m1::encode(&p.store).unwrap();
}

#[test]
fn the_same_inputs_give_the_same_map_and_a_new_migration_gives_new_identities() {
    let (sources, view) = corpus();
    let first = plan_initial(&FakeEnv::new(), &*owner(), &sources, Some(&view), MIGRATION).unwrap();
    let mut again = plan_initial(&FakeEnv::new(), &*owner(), &sources, Some(&view), MIGRATION).unwrap();
    // The host and store identities are generated once per preview; everything else is derived.
    again.store.store_id = first.store.store_id.clone();
    again.store.host_id = first.store.host_id.clone();
    for w in &mut again.store.workspaces { w.host_id = first.store.host_id.clone(); }
    for b in &mut again.store.bindings { b.host_id = first.store.host_id.clone(); }
    for s in &mut again.store.sessions { s.host_id = first.store.host_id.clone(); }
    assert_eq!(first.store, again.store);
    assert_eq!(first.store.migration, again.store.migration);
    assert_eq!(first.store.projects, again.store.projects);
    assert_eq!(first.store.sessions, again.store.sessions);
    assert_eq!(first.store.workspaces, again.store.workspaces);
    let other = plan_initial(&FakeEnv::new(), &*owner(), &sources, Some(&view), "99999999-0000-4000-8000-000000000002").unwrap();
    assert_ne!(first.store.projects[0].id, other.store.projects[0].id);
    assert_eq!(first.store.projects.len(), other.store.projects.len());
    // The derivation is stable, name-based and shaped like a UUID.
    let id = derive(MIGRATION, "project", "/x");
    assert_eq!(id, derive(MIGRATION, "project", "/x"));
    assert_ne!(id, derive(MIGRATION, "project", "/y"));
    assert_ne!(id, derive(MIGRATION, "binding", "/x"));
    assert_eq!(&id[14..15], "8");
    assert!(wyss_console::contracts::m1::scalar::ProjectId::parse(&id).is_ok());
}

#[test]
fn an_alias_of_a_root_needs_an_explicit_acceptance_and_still_coalesces() {
    let env = FakeEnv::new();
    let sources = Sources { registry: Some(registry(&[ROOT_A, ALIAS], &[])), layout: None };
    let p = plan(&env, &owner(), &sources, Some(&host(vec![])));
    assert!(kinds(&p).contains(&ConflictKind::Alias));
    assert!(!p.summary.can_commit && p.summary.needs_alias_acceptance);
    assert_eq!(p.store.projects.len(), 1, "both spellings are the one root");
    assert_eq!(p.store.migration.as_ref().unwrap().map.len(), 2, "each spelling is in the map");
}

#[test]
fn an_absent_host_imports_no_session_and_remakes_nothing() {
    let env = FakeEnv::new();
    let (sources, _) = corpus();
    let o = owner();
    let p = plan(&env, &o, &sources, None);
    assert!(kinds(&p).contains(&ConflictKind::HostOffline));
    assert!(p.store.sessions.is_empty());
    assert_eq!(p.summary.counts.tabs_skipped, 3, "nothing is alive to place");
    assert_eq!(o.launches(), 0);
    assert!(!o.calls().iter().any(|c| c.starts_with("launch_")));
}

#[test]
fn malformed_newer_or_over_bound_sources_block_and_nothing_is_guessed() {
    let env = FakeEnv::new();
    let too_many: Vec<(String, &str, &str)> = (0..201).map(|n| (format!("{n:032x}"), ROOT_A, "stopped")).collect();
    let rows: Vec<(&str, &str, &str)> = too_many.iter().map(|(i, p, s)| (i.as_str(), *p, *s)).collect();
    let deep = (0..8).fold(leaf(T_NATIVE), |inner, n| split(inner, leaf(&format!("aaaaaaaa-0000-4000-8000-0000000001{n:02x}"))));
    let cases: Vec<(&str, Sources)> = vec![
        ("registry newer", Sources { registry: Some(br#"{"schema":2,"sessions":[],"projects":[]}"#.to_vec()), layout: None }),
        ("registry unknown key", Sources { registry: Some(br#"{"schema":1,"sessions":[],"projects":[],"x":1}"#.to_vec()), layout: None }),
        ("registry not json", Sources { registry: Some(b"{".to_vec()), layout: None }),
        ("registry 201 sessions", Sources { registry: Some(registry(&[], &rows)), layout: None }),
        ("layout newer", Sources { registry: None, layout: Some(br#"{"version":2,"tabs":[]}"#.to_vec()) }),
        ("layout 17 tabs", Sources { registry: None, layout: Some(layout((0..17).map(|n| tab(leaf(&format!("aaaaaaaa-0000-4000-8000-0000000002{n:02x}")))).collect())) }),
        ("layout duplicate leaf across tabs", Sources { registry: None, layout: Some(layout(vec![tab(leaf(T_NATIVE)), tab(leaf(T_NATIVE))])) }),
        ("layout too deep", Sources { registry: None, layout: Some(layout(vec![tab(deep)])) }),
    ];
    for (name, sources) in cases {
        let p = plan(&env, &owner(), &sources, Some(&host(vec![])));
        assert!(kinds(&p).contains(&ConflictKind::SourceUnreadable), "{name}");
        assert!(!p.summary.can_commit, "{name}");
    }
}

#[test]
fn more_unassigned_terminals_than_scratch_can_hold_is_refused_whole() {
    let env = FakeEnv::new();
    let ids: Vec<String> = (0..20).map(|n| format!("aaaaaaaa-0000-4000-8000-0000000003{n:02x}")).collect();
    fn tree(ids: &[String]) -> Value {
        match ids {
            [one] => leaf(one),
            _ => split(tree(&ids[..ids.len() / 2]), tree(&ids[ids.len() / 2..])),
        }
    }
    let tabs = vec![tab(tree(&ids[..10])), tab(tree(&ids[10..]))];
    let terminals: Vec<TerminalRow> = ids.iter().map(|id| terminal(id, None, "/tmp")).collect();
    let sources = Sources { registry: None, layout: Some(layout(tabs)) };
    let p = plan(&env, &owner(), &sources, Some(&host(terminals)));
    assert!(kinds(&p).contains(&ConflictKind::OverBound));
    assert!(!p.summary.can_commit);
    assert!(p.store.workspaces[0].leaves.is_empty(), "nothing was cut to fit");
}

// ---- reading the legacy files --------------------------------------------------------------

#[test]
fn legacy_files_are_read_as_bytes_without_opening_the_registry() {
    let dir = TestDir::new();
    let state = dir.0.join("state");
    std::fs::create_dir_all(state.join("native")).unwrap();
    let bytes = registry(&[ROOT_A], &[(SESSION, ROOT_A, "running")]);
    std::fs::write(state.join("native").join("registry.json"), &bytes).unwrap();
    let before = std::fs::metadata(state.join("native").join("registry.json")).unwrap().modified().unwrap();
    let read = read_sources(&state).unwrap();
    assert_eq!(read.registry.as_deref(), Some(bytes.as_slice()));
    assert!(read.layout.is_none());
    // `Registry::open` would have marked the running session lost and rewritten the file.
    let after = std::fs::metadata(state.join("native").join("registry.json")).unwrap().modified().unwrap();
    assert_eq!(before, after);
    assert_eq!(std::fs::read(state.join("native").join("registry.json")).unwrap(), bytes);
    assert!(!std::fs::read_dir(state.join("native")).unwrap().flatten().any(|e| e.file_name() != "registry.json"));
    // Oversized and linked sources are refused.
    std::fs::write(state.join("layout.json"), vec![b' '; 64 * 1024 + 1]).unwrap();
    assert_eq!(read_sources(&state).unwrap_err().0, ConflictKind::SourceOversized);
    std::fs::remove_file(state.join("layout.json")).unwrap();
    std::os::unix::fs::symlink(state.join("native").join("registry.json"), state.join("layout.json")).unwrap();
    assert_eq!(read_sources(&state).unwrap_err().0, ConflictKind::SourceUnreadable);
}

// ---- commit, interruption, second import -----------------------------------------------------

fn empty_world() -> (TestDir, Store) {
    let dir = TestDir::new();
    let store = Store::open(dir.0.join("v1")).unwrap();
    (dir, store)
}

#[test]
fn commit_preserves_the_legacy_bytes_then_publishes_the_store_last() {
    let (_dir, store) = empty_world();
    let (sources, view) = corpus();
    let p = plan(&FakeEnv::new(), &owner(), &sources, Some(&view));
    let snapshot = p.snapshot.clone();
    let committed = commit_initial(&store, p).unwrap();
    // The store is the plan; the snapshot holds the exact bytes with a hash manifest.
    assert_eq!(store.read().unwrap(), Loaded::Ready(Box::new(committed)));
    let dir = store.dir().join(format!("migration-{MIGRATION}"));
    for (name, bytes) in snapshot {
        assert_eq!(std::fs::read(dir.join(name)).unwrap(), bytes);
    }
    let manifest: Value = serde_json::from_slice(&std::fs::read(dir.join("MANIFEST.json")).unwrap()).unwrap();
    assert_eq!(manifest["files"].as_array().unwrap().len(), 2);
    assert_eq!(manifest["files"][0]["sha256"].as_str().unwrap().len(), 64);
    // A store that exists is never replaced by another commit.
    let again = plan(&FakeEnv::new(), &owner(), &sources, Some(&view));
    let failure = commit_initial(&store, again).unwrap_err();
    assert_eq!(failure.code, ErrorCode::RevisionConflict);
}

#[test]
fn an_interrupted_commit_leaves_no_store_and_the_retry_succeeds() {
    let dir = TestDir::new();
    let crash: Faults = Arc::new(|at| {
        if at == Point::BeforeRename {
            Err(std::io::Error::other("killed"))
        } else {
            Ok(())
        }
    });
    let broken = Store::open(dir.0.join("v1")).unwrap().with_faults(crash);
    let (sources, view) = corpus();
    let p = plan(&FakeEnv::new(), &owner(), &sources, Some(&view));
    assert_eq!(commit_initial(&broken, p).unwrap_err().code, ErrorCode::IoFailure);
    assert_eq!(broken.read().unwrap(), Loaded::Missing, "the old state: no store");
    assert!(broken.dir().join(format!("migration-{MIGRATION}")).is_dir(), "the snapshot was kept");
    // Retried with a fresh migration, on a healthy store: the complete new state.
    let store = Store::open(dir.0.join("v1")).unwrap();
    let retry = plan_initial(&FakeEnv::new(), &*owner(), &sources, Some(&view), "99999999-0000-4000-8000-000000000003").unwrap();
    commit_initial(&store, retry).unwrap();
    assert!(matches!(store.read().unwrap(), Loaded::Ready(_)));
}

#[test]
fn a_second_import_adds_only_what_appeared_and_touches_nothing_else() {
    let (_dir, store) = empty_world();
    let (sources, view) = corpus();
    let o = owner();
    let p = plan(&FakeEnv::new(), &o, &sources, Some(&view));
    commit_initial(&store, p).unwrap();
    let existing = match store.read().unwrap() { Loaded::Ready(s) => *s, other => panic!("{other:?}") };
    let env = FakeEnv::new();
    // Nothing changed: no additions and no change reported.
    let same = plan_incremental(&env, &*o, &sources, Some(&view), &existing, "99999999-0000-4000-8000-000000000010").unwrap();
    assert_eq!((same.summary.mode, same.summary.legacy_changed), (ImportMode::Incremental, false));
    assert_eq!((same.summary.counts.projects, same.summary.counts.sessions), (0, 0));
    // A folder was registered and a terminal started in the old window since.
    o.add_root("/home/fixture/work/c", 64768, 1003);
    let later = Sources {
        registry: Some(registry(&[ROOT_A, ROOT_B, "/home/fixture/work/c"], &[(SESSION, ROOT_A, "running")])),
        layout: sources.layout.clone(),
    };
    let new_terminal = "aaaaaaaa-0000-4000-8000-0000000000a9";
    let mut moved = view.clone();
    moved.terminals.push(terminal(new_terminal, None, "/home/fixture/work/c"));
    let more = plan_incremental(&env, &*o, &later, Some(&moved), &existing, "99999999-0000-4000-8000-000000000011").unwrap();
    assert!(more.summary.legacy_changed);
    assert_eq!((more.summary.counts.projects, more.summary.counts.sessions), (1, 1));
    assert_eq!(existing, match store.read().unwrap() { Loaded::Ready(s) => *s, other => panic!("{other:?}") }, "a preview changes nothing");
    let committed = commit_incremental(&store, existing.revision, &more).unwrap();
    assert_eq!(committed.projects.len(), existing.projects.len() + 1);
    assert_eq!(committed.sessions.len(), existing.sessions.len() + 1);
    // Everything that was there is exactly as it was.
    for old in &existing.projects {
        assert!(committed.projects.contains(old));
    }
    for old in &existing.workspaces {
        assert!(committed.workspaces.contains(old));
    }
    for old in &existing.sessions {
        assert!(committed.sessions.contains(old));
    }
    // The new session belongs to the new project, and the same import is not offered again.
    let again = plan_incremental(&env, &*o, &later, Some(&moved), &committed, "99999999-0000-4000-8000-000000000012").unwrap();
    assert_eq!((again.summary.counts.projects, again.summary.counts.sessions, again.summary.legacy_changed), (0, 0, false));
    // A preview made against an older revision is a conflict, not a merge.
    let stale = commit_incremental(&store, existing.revision, &more).unwrap_err();
    assert_eq!(stale.code, ErrorCode::RevisionConflict);
}
