//! The facade: the release gate, the views, pagination, picks, the migration through the
//! service, rollback, and M1-09, the exclusive terminal input claim across windows and
//! processes.
use super::*;
use crate::projects::{
    claim::{ClaimError, Claims},
    command::Failure,
    service::{ProjectsService, StoreStatus},
};
use std::process::{Command, Stdio};

const TERMINAL_1: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const TERMINAL_2: &str = "aaaaaaaa-0000-4000-8000-000000000002";
const EPOCH: &str = "h1:1:1:100:1000";

struct Fixture {
    dir: TestDir,
    owner: Arc<FakeOwner>,
    service: ProjectsService,
    env: Arc<FakeEnv>,
}

/// `<dir>/state/wyss-console/projects/v1` is the store, so the older files are in
/// `<dir>/state/wyss-console`, as in the real layout.
fn fixture() -> Fixture {
    let dir = TestDir::new();
    let owner = FakeOwner::new();
    owner.add_root(ROOT_A, 64768, 1001);
    owner.add_root(ROOT_B, 64768, 1002);
    let env = Arc::new(FakeEnv::new());
    let store_dir = dir
        .0
        .join("state")
        .join("wyss-console")
        .join("projects")
        .join("v1");
    let service = ProjectsService::new(
        Some(store_dir),
        owner.clone(),
        env.clone(),
        Claims::new(Some(dir.0.join("run").join("claims"))),
    )
    // Every read here asks the owner: the tests change what it says between two of them.
    .with_inventory_ttl(std::time::Duration::ZERO);
    Fixture {
        dir,
        owner,
        service,
        env,
    }
}

impl Fixture {
    fn enabled(self) -> Self {
        self.service.set_enabled(true).unwrap();
        self.service.initialize_empty().unwrap();
        self
    }

    fn store(&self) -> StoreV1 {
        let store = Store::open(self.dir.0.join("state/wyss-console/projects/v1")).unwrap();
        match store.read().unwrap() {
            Loaded::Ready(s) => *s,
            other => panic!("{other:?}"),
        }
    }

    fn send(&self, context: Value, command: Value) -> Result<Receipt, Failure> {
        let store = self.store();
        self.service.execute(
            serde_json::from_value(json!({
                "schema": 1, "operationId": self.env.uuid(),
                "expectedStoreRevision": store.revision.get().to_string(),
                "context": context, "command": command,
            }))
            .unwrap(),
        )
    }

    fn context(&self, project: Option<&str>, workspace: Option<&str>) -> Value {
        let store = self.store();
        let binding = project.and_then(|p| {
            store
                .projects
                .iter()
                .find(|x| x.id.as_str() == p)
                .and_then(|x| x.default_binding_id.clone())
                .and_then(|b| store.bindings.iter().find(|x| x.id == b).cloned())
        });
        json!({"hostId": store.host_id.as_str(), "projectId": project, "workspaceId": workspace,
               "bindingId": binding.as_ref().map(|b| b.id.as_str().to_owned()),
               "bindingRevision": binding.as_ref().map(|b| b.revision.get().to_string()),
               "selectionRevision": "1"})
    }

    fn register(&self, path: &str) -> (String, String) {
        let pick = self.service.issue_pick(path, Purpose::Register, 1).unwrap();
        let receipt = self
            .send(
                self.context(None, None),
                json!({"kind": "registerExisting", "selectionToken": pick.token}),
            )
            .unwrap();
        let id = |role: &str| {
            receipt
                .created
                .iter()
                .find(|c| c.role == role)
                .unwrap()
                .id
                .clone()
        };
        (id("project"), id("workspace"))
    }

    /// A shell session in `workspace`, launched through the engine.
    fn shell(&self, project: &str, workspace: &str, terminal: &str) -> String {
        let store = self.store();
        let revision = store
            .workspaces
            .iter()
            .find(|w| w.id.as_str() == workspace)
            .unwrap()
            .revision
            .get();
        let leaf = self.env.uuid();
        self.send(
            self.context(Some(project), Some(workspace)),
            json!({"kind": "saveWorkspace", "workspaceId": workspace,
                   "expectedWorkspaceRevision": revision.to_string(), "label": "Main",
                   "layout": {"kind": "leaf", "id": leaf}, "focus": null, "pinned": false,
                   "leaves": [leaf_json(&leaf, Value::Null)],
                   "preferences": {"provider": null, "mode": "native", "nativeProfile": null,
                                   "managedSafetyProfile": null}}),
        )
        .unwrap();
        self.owner.script(|s| {
            s.shell.push(Ok(ShellLaunched {
                terminal_id: terminal.into(),
            }))
        });
        self.send(
            self.context(Some(project), Some(workspace)),
            json!({"kind": "launch", "workspaceId": workspace, "leafId": leaf, "provider": "shell",
                   "mode": "native", "nativeProfile": null, "managedSafetyProfile": null}),
        )
        .unwrap()
        .created[0]
            .id
            .clone()
    }
}

// ---- the release gate -------------------------------------------------------------------------

#[test]
fn everything_that_acts_is_off_until_the_owner_turns_it_on() {
    let f = fixture();
    assert!(!f.service.overview().enabled);
    let command = json!({"kind": "registerExisting", "selectionToken": "x"});
    let envelope: Envelope = serde_json::from_value(json!({
        "schema": 1, "operationId": f.env.uuid(), "expectedStoreRevision": "1",
        "context": {"hostId": f.env.uuid(), "projectId": null, "workspaceId": null, "bindingId": null,
                    "bindingRevision": null, "selectionRevision": "1"},
        "command": command,
    }))
    .unwrap();
    let off = f.service.execute(envelope).unwrap_err();
    assert_eq!(
        (off.code, off.field),
        (ErrorCode::UnsupportedCapability, Some("disabled"))
    );
    assert_eq!(
        f.service.initialize_empty().unwrap_err().field,
        Some("disabled")
    );
    assert_eq!(
        f.service.preview_migration().unwrap_err().field,
        Some("disabled")
    );
    assert_eq!(
        f.service.claim_input("w", "a").unwrap_err().field,
        Some("disabled")
    );
    assert_eq!(f.service.recover(), 0);
    assert_eq!(f.owner.launches(), 0);
    // Off means inert: no view makes the store directory or asks the host.
    assert_eq!(
        f.service.rollback_report().unwrap_err().field,
        Some("disabled")
    );
    assert_eq!(
        f.service.compact_receipts().unwrap_err().field,
        Some("disabled")
    );
    let off_view = f.service.overview();
    assert!(!off_view.host.reachable && off_view.host.epoch.is_none());
    assert!(
        !f.dir.0.join("state/wyss-console/projects").exists(),
        "the workspace off must not make its directory"
    );
    assert!(f.owner.calls().is_empty(), "{:?}", f.owner.calls());
    // The gate is a private file beside the store, not inside it, and turning it off is clean.
    f.service.set_enabled(true).unwrap();
    assert!(f.service.overview().enabled);
    let gate = f.dir.0.join("state/wyss-console/projects/feature");
    assert_eq!(std::fs::read(&gate).unwrap(), b"enabled");
    f.service.set_enabled(false).unwrap();
    assert!(!gate.exists() && !f.service.overview().enabled);
    f.service.set_enabled(false).unwrap();
}

#[test]
fn a_missing_store_is_offered_not_invented_and_a_start_is_explicit() {
    let f = fixture();
    f.service.set_enabled(true).unwrap();
    assert!(matches!(f.service.overview().store, StoreStatus::Missing));
    assert!(f.service.list_projects(None).is_err());
    f.service.initialize_empty().unwrap();
    assert!(matches!(
        f.service.overview().store,
        StoreStatus::Ready { .. }
    ));
    assert_eq!(
        f.service.initialize_empty().unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    let page = f.service.list_projects(None).unwrap();
    assert!(page.items.is_empty() && page.cursor.is_none());
}

#[test]
fn a_store_that_cannot_be_read_is_reported_and_no_view_pretends_otherwise() {
    let f = fixture().enabled();
    std::fs::write(
        f.dir.0.join("state/wyss-console/projects/v1/store.json"),
        b"{ no",
    )
    .unwrap();
    std::fs::set_permissions(
        f.dir.0.join("state/wyss-console/projects/v1/store.json"),
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    )
    .unwrap();
    assert!(matches!(
        f.service.overview().store,
        StoreStatus::Unusable {
            code: ErrorCode::InvalidRecord,
            ..
        }
    ));
    assert_eq!(
        f.service.list_projects(None).unwrap_err().code,
        ErrorCode::InvalidRecord
    );
    assert_eq!(
        f.service.initialize_empty().unwrap_err().code,
        ErrorCode::InvalidRecord
    );
    assert_eq!(
        std::fs::read(f.dir.0.join("state/wyss-console/projects/v1/store.json")).unwrap(),
        b"{ no"
    );
}

// ---- views --------------------------------------------------------------------------------------

#[test]
fn views_show_the_root_by_name_and_what_the_owners_say_now() {
    let f = fixture().enabled();
    let (project, workspace) = f.register(ROOT_A);
    let view = f.service.get_project(&project).unwrap();
    let root = view.root.as_ref().unwrap();
    assert_eq!((root.label.as_str(), root.availability), ("a", "available"));
    let json = serde_json::to_string(&view).unwrap();
    assert!(
        !json.contains("/home/fixture"),
        "a path never crosses to the window: {json}"
    );
    // The folder was replaced, then went away: the view says so, and nothing else changes.
    f.owner.add_root(ROOT_A, 64768, 7777);
    assert_eq!(
        f.service
            .get_project(&project)
            .unwrap()
            .root
            .unwrap()
            .availability,
        "identity_changed"
    );
    f.owner.script(|s| {
        s.roots.insert(ROOT_A.into(), Err(RootError::Missing));
    });
    assert_eq!(
        f.service
            .get_project(&project)
            .unwrap()
            .root
            .unwrap()
            .availability,
        "missing"
    );
    assert_eq!(
        f.service.get_project(&project).unwrap().workspace_ids,
        std::slice::from_ref(&workspace)
    );
    assert_eq!(f.owner.launches(), 0);
}

#[test]
fn a_session_is_live_ended_or_unknown_by_what_the_owner_answered_and_never_ended_for_lack_of_an_answer()
 {
    let f = fixture().enabled();
    let (project, workspace) = f.register(ROOT_A);
    f.shell(&project, &workspace, TERMINAL_1);
    let view = |f: &Fixture| {
        f.service
            .get_workspace(&workspace, Some("w1"))
            .unwrap()
            .leaves[0]
            .session
            .clone()
            .unwrap()
    };
    f.owner
        .script(|s| s.host = Some(Ok(vec![native_row(TERMINAL_1, None)])));
    assert_eq!((view(&f).availability, view(&f).reason), ("live", None));
    f.owner.script(|s| s.host = Some(Ok(Vec::new())));
    assert_eq!(view(&f).availability, "ended");
    f.owner
        .script(|s| s.host = Some(Err(OwnerError::HostOffline)));
    assert_eq!(
        (view(&f).availability, view(&f).reason),
        ("unknown", Some("host_offline"))
    );
    f.owner.script(|s| {
        s.host = Some(Ok(vec![native_row(TERMINAL_1, None)]));
        s.epoch = "h1:9:9:2:2".into();
    });
    assert_eq!(
        (view(&f).availability, view(&f).reason),
        ("ended", Some("host_changed"))
    );
}

#[test]
fn managed_sessions_show_the_effective_profile_and_flag_a_mismatch_without_changing_it() {
    let f = fixture().enabled();
    let (project, workspace) = f.register(ROOT_A);
    let (stream, id) = (
        "bbbbbbbb-0000-4000-8000-000000000002",
        "cccccccc-0000-4000-8000-000000000001",
    );
    f.owner.script(|s| {
        s.advert = Ok(advert(
            ManagedProtocol::P2,
            stream,
            &[ManagedSafetyProfile::Inference],
        ));
        s.prepared.push(Ok(ManagedLaunched {
            protocol: ManagedProtocol::P2,
            store_stream_id: stream.into(),
            session_id: id.into(),
        }))
    });
    let leaf = f.env.uuid();
    f.send(f.context(Some(&project), Some(&workspace)), json!({"kind": "saveWorkspace", "workspaceId": workspace,
        "expectedWorkspaceRevision": "1", "label": "Main", "layout": {"kind": "leaf", "id": leaf}, "focus": null,
        "pinned": false, "leaves": [leaf_json(&leaf, Value::Null)],
        "preferences": {"provider": null, "mode": "native", "nativeProfile": null, "managedSafetyProfile": null}})).unwrap();
    f.send(f.context(Some(&project), Some(&workspace)), json!({"kind": "launch", "workspaceId": workspace, "leafId": leaf,
        "provider": "codex", "mode": "managed", "nativeProfile": null, "managedSafetyProfile": "inference"})).unwrap();
    let session = |f: &Fixture| {
        f.service.get_workspace(&workspace, None).unwrap().leaves[0]
            .session
            .clone()
            .unwrap()
    };
    f.owner.script(|s| {
        s.managed = Ok(vec![managed_row(
            ManagedProtocol::P2,
            stream,
            id,
            Some(ManagedSafetyProfile::LocalTools),
        )])
    });
    let shown = session(&f);
    assert_eq!((shown.availability, shown.protocol), ("live", Some(2)));
    assert_eq!(
        shown.requested_safety_profile,
        Some(ManagedSafetyProfile::Inference)
    );
    assert_eq!(
        shown.effective_safety_profile,
        Some(ManagedSafetyProfile::LocalTools)
    );
    assert!(
        shown.profile_mismatch,
        "a request is not what the authority attached"
    );
    f.owner.script(|s| s.managed = Err(OwnerError::HostOffline));
    assert_eq!(
        (session(&f).availability, session(&f).reason),
        ("unknown", Some("owner_offline"))
    );
    f.owner.script(|s| s.managed = Ok(Vec::new()));
    assert_eq!(session(&f).reason, Some("not_listed"));
}

#[test]
fn lists_are_paged_at_the_revision_they_were_read_at() {
    let f = fixture().enabled();
    let revision = f.store().revision;
    Store::open(f.dir.0.join("state/wyss-console/projects/v1"))
        .unwrap()
        .transact(revision, |s| {
            for n in 0..120u32 {
                s.sessions
                    .push(wyss_console::contracts::m1::records::Association {
                        id: wyss_console::contracts::m1::scalar::AssociationId::parse(&format!(
                            "dddddddd-0000-4000-8000-{n:012x}"
                        ))
                        .unwrap(),
                        origin_project_id: None,
                        host_id: s.host_id.clone(),
                        binding_id: None,
                        binding_revision: None,
                        target: wyss_console::contracts::m1::records::Target::Shell {
                            terminal_id: wyss_console::contracts::m1::scalar::TerminalId::parse(
                                &format!("eeeeeeee-0000-4000-8000-{n:012x}"),
                            )
                            .unwrap(),
                            host_epoch: wyss_console::contracts::m1::scalar::EpochText::parse(
                                "old",
                            )
                            .unwrap(),
                        },
                        state: wyss_console::contracts::m1::records::AssociationState::Unavailable,
                        operation_id: None,
                    });
            }
            Ok::<(), ()>(())
        })
        .unwrap()
        .unwrap();
    let first = f.service.list_sessions(None, None, None).unwrap();
    assert_eq!(first.items.len(), 100);
    let cursor = first.cursor.clone().unwrap();
    let second = f.service.list_sessions(None, Some(&cursor), None).unwrap();
    assert_eq!((second.items.len(), second.cursor.is_none()), (20, true));
    // Unassigned history is all discoverable, with no invented owner.
    assert!(first.items.iter().all(|s| s.origin_project_id.is_none()));
    assert_eq!(
        f.service
            .list_sessions(Some(None), None, None)
            .unwrap()
            .items
            .len(),
        100
    );
    assert_eq!(
        f.service
            .list_sessions(
                Some(Some("00000000-0000-4000-8000-000000000001")),
                None,
                None
            )
            .unwrap()
            .items
            .len(),
        0
    );
    // A cursor from before a change is a refetch, and a made-up one is not a cursor.
    f.register(ROOT_A);
    assert_eq!(
        f.service
            .list_sessions(None, Some(&cursor), None)
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        f.service
            .list_sessions(None, Some("nonsense"), None)
            .unwrap_err()
            .code,
        ErrorCode::InvalidRecord
    );
}

#[test]
fn a_pick_for_a_missing_or_foreign_folder_is_never_issued() {
    let f = fixture().enabled();
    f.owner.script(|s| {
        s.roots.insert("/gone".into(), Err(RootError::Missing));
        s.roots.insert("/theirs".into(), Err(RootError::Ineligible));
    });
    assert_eq!(
        f.service
            .issue_pick("/gone", Purpose::Register, 1)
            .unwrap_err()
            .code,
        ErrorCode::RootMissing
    );
    assert_eq!(
        f.service
            .issue_pick("/theirs", Purpose::Register, 1)
            .unwrap_err()
            .code,
        ErrorCode::RootIneligible
    );
    let pick = f.service.issue_pick(ROOT_A, Purpose::Register, 1).unwrap();
    assert!(pick.native_eligible && pick.config_eligible && pick.label == "a");
    f.service.forget_picks();
    let result = f.send(
        f.context(None, None),
        json!({"kind": "registerExisting", "selectionToken": pick.token}),
    );
    assert_eq!(
        result.unwrap_err().code,
        ErrorCode::ContextStale,
        "a pick does not survive a switch"
    );
    assert!(f.store().projects.is_empty());
}

// ---- M1-09: one window types ----------------------------------------------------------------------

#[test]
fn only_the_window_with_the_claim_writes_or_resizes_and_a_mirror_reads() {
    let f = fixture().enabled();
    let (project, workspace) = f.register(ROOT_A);
    let association = f.shell(&project, &workspace, TERMINAL_1);
    f.owner
        .script(|s| s.host = Some(Ok(vec![native_row(TERMINAL_1, None)])));
    // Nobody holds input yet: a view can read and cannot write.
    assert_eq!(
        f.service.read_output(&association, 0, 0).unwrap().data,
        b"out"
    );
    assert_eq!(
        f.service
            .write_input("w1", &association, b"ls\n")
            .unwrap_err()
            .field,
        Some("input_claim")
    );
    f.service.claim_input("w1", &association).unwrap();
    f.service.claim_input("w1", &association).unwrap();
    let second = f.service.claim_input("w2", &association).unwrap_err();
    assert_eq!(
        (second.code, second.field),
        (ErrorCode::OwnerRefused, Some("input_claim"))
    );
    // The mirror still reads, and everything it tries to send is refused before the host.
    assert_eq!(f.service.read_output(&association, 3, 0).unwrap().next, 6);
    assert!(f.service.write_input("w2", &association, b"rm\n").is_err());
    assert!(f.service.resize("w2", &association, 10, 10).is_err());
    f.service.write_input("w1", &association, b"ls\n").unwrap();
    f.service.resize("w1", &association, 40, 120).unwrap();
    assert_eq!(
        f.owner.script(|s| s.written.clone()),
        [(TERMINAL_1.to_owned(), b"ls\n".to_vec())]
    );
    assert_eq!(
        f.owner.script(|s| s.resized.clone()),
        [(TERMINAL_1.to_owned(), 40, 120)]
    );
    // The views say who types.
    let view = |window: &str| {
        f.service
            .get_workspace(&workspace, Some(window))
            .unwrap()
            .leaves[0]
            .session
            .clone()
            .unwrap()
            .input
    };
    assert_eq!((view("w1"), view("w2")), ("this", "other"));
    // Blur releases it, and the other window can take it.
    f.service.release_input("w1", &association);
    assert!(f.service.write_input("w1", &association, b"x").is_err());
    f.service.claim_input("w2", &association).unwrap();
    f.service.release_window("w2");
    assert_eq!(view("w1"), "none");
}

#[test]
fn a_replaced_host_ends_the_claim_before_anything_is_sent() {
    let f = fixture().enabled();
    let (project, workspace) = f.register(ROOT_A);
    let association = f.shell(&project, &workspace, TERMINAL_1);
    f.owner
        .script(|s| s.host = Some(Ok(vec![native_row(TERMINAL_1, None)])));
    f.service.claim_input("w1", &association).unwrap();
    f.owner.script(|s| {
        s.epoch = "h1:9:9:2:2".into();
        s.calls.clear();
    });
    assert_eq!(
        f.service
            .write_input("w1", &association, b"ls\n")
            .unwrap_err()
            .code,
        ErrorCode::HostChanged
    );
    assert_eq!(
        f.service.resize("w1", &association, 5, 5).unwrap_err().code,
        ErrorCode::HostChanged
    );
    assert_eq!(
        f.service.read_output(&association, 0, 0).unwrap_err().code,
        ErrorCode::HostChanged
    );
    assert_eq!(
        f.service.claim_input("w2", &association).unwrap_err().code,
        ErrorCode::HostChanged
    );
    assert!(
        f.owner.script(|s| s.written.is_empty()),
        "nothing reached the new host"
    );
}

#[test]
fn managed_sessions_have_no_terminal_input_and_a_missing_claim_directory_fails_closed() {
    let f = fixture().enabled();
    let (project, workspace) = f.register(ROOT_A);
    let association = f.shell(&project, &workspace, TERMINAL_1);
    f.owner
        .script(|s| s.host = Some(Ok(vec![native_row(TERMINAL_1, None)])));
    // A Managed session is not a terminal: it has no input to claim.
    let (stream, id) = (
        "bbbbbbbb-0000-4000-8000-000000000002",
        "cccccccc-0000-4000-8000-000000000001",
    );
    let store = Store::open(f.dir.0.join("state/wyss-console/projects/v1")).unwrap();
    let managed = f.env.uuid();
    let managed_id = managed.clone();
    store
        .transact(f.store().revision, |s| {
            s.sessions
                .push(wyss_console::contracts::m1::records::Association {
                    id: wyss_console::contracts::m1::scalar::AssociationId::parse(&managed)
                        .unwrap(),
                    origin_project_id: Some(s.projects[0].id.clone()),
                    host_id: s.host_id.clone(),
                    binding_id: Some(s.bindings[0].id.clone()),
                    binding_revision: Some(s.bindings[0].revision),
                    target: wyss_console::contracts::m1::records::Target::Managed {
                        protocol: ManagedProtocol::P2,
                        store_stream_id: wyss_console::contracts::m1::scalar::StoreStreamId::parse(
                            stream,
                        )
                        .unwrap(),
                        session_id: wyss_console::contracts::m1::scalar::ManagedSessionId::parse(
                            id,
                        )
                        .unwrap(),
                        requested_safety_profile: None,
                    },
                    state: wyss_console::contracts::m1::records::AssociationState::Detached,
                    operation_id: None,
                });
            Ok::<(), ()>(())
        })
        .unwrap()
        .unwrap();
    assert_eq!(
        f.service.claim_input("w1", &managed_id).unwrap_err().code,
        ErrorCode::UnsupportedCapability
    );
    assert!(f.service.write_input("w1", &managed_id, b"x").is_err());
    // With nowhere private to coordinate, no claim can be taken: input fails closed.
    let none = Claims::new(None);
    assert_eq!(none.claim("w", "e", "t"), Err(ClaimError::Unavailable));
    let service = ProjectsService::new(
        Some(f.dir.0.join("state/wyss-console/projects/v1")),
        f.owner.clone(),
        f.env.clone(),
        Claims::new(None),
    );
    assert_eq!(
        service.claim_input("w1", &association).unwrap_err().field,
        Some("input_claim")
    );
    assert!(service.write_input("w1", &association, b"x").is_err());
}

/// Run as a disposable child by the test below. Alone it does nothing.
#[test]
fn child_claim_holder() {
    let (Ok(dir), Ok(epoch), Ok(terminal)) = (
        std::env::var("WYSS_M1_CLAIM_DIR"),
        std::env::var("WYSS_M1_CLAIM_EPOCH"),
        std::env::var("WYSS_M1_CLAIM_TERMINAL"),
    ) else {
        return;
    };
    let claims = Claims::new(Some(PathBuf::from(dir)));
    claims.claim("child", &epoch, &terminal).unwrap();
    println!("CLAIMED");
    // Held until the parent kills this process.
    std::thread::sleep(std::time::Duration::from_secs(60));
}

#[test]
fn a_claim_held_by_another_console_process_blocks_and_dies_with_that_process() {
    use std::io::BufRead;
    let dir = TestDir::new();
    let claims_dir = dir.0.join("claims");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "projects::tests::service_tests::child_claim_holder",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("WYSS_M1_CLAIM_DIR", &claims_dir)
        .env("WYSS_M1_CLAIM_EPOCH", EPOCH)
        .env("WYSS_M1_CLAIM_TERMINAL", TERMINAL_2)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut out = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    while !line.contains("CLAIMED") {
        line.clear();
        assert!(
            out.read_line(&mut line).unwrap() > 0,
            "the child must claim"
        );
    }
    let mine = Claims::new(Some(claims_dir));
    assert_eq!(
        mine.claim("w1", EPOCH, TERMINAL_2),
        Err(ClaimError::HeldByOther)
    );
    // Another terminal, and the same terminal under another epoch, are free.
    assert_eq!(mine.claim("w1", EPOCH, TERMINAL_1), Ok(()));
    assert_eq!(mine.claim("w1", "h1:9:9:9:9", TERMINAL_2), Ok(()));
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(
        mine.claim("w1", EPOCH, TERMINAL_2),
        Ok(()),
        "the kernel dropped the dead process's lock"
    );
    assert!(mine.holds("w1", EPOCH, TERMINAL_2));
}

// ---- migration through the service ---------------------------------------------------------------------

fn older(f: &Fixture, registry: &str) {
    let dir = f.dir.0.join("state/wyss-console/native");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("registry.json"), registry).unwrap();
}

fn registry_of(paths: &[&str]) -> String {
    json!({"schema": 1, "sessions": [], "projects": paths.iter().map(|p| json!({"path": p})).collect::<Vec<_>>()}).to_string()
}

#[test]
fn a_preview_is_committed_only_as_shown_and_only_against_unchanged_files() {
    let f = fixture();
    f.service.set_enabled(true).unwrap();
    older(&f, &registry_of(&[ROOT_A]));
    let summary = f.service.preview_migration().unwrap();
    assert_eq!((summary.counts.projects, summary.can_commit), (1, true));
    assert!(
        matches!(f.service.overview().store, StoreStatus::Missing),
        "a preview writes nothing"
    );
    // Not the preview that was shown.
    assert_eq!(
        f.service
            .commit_migration("00000000-0000-4000-8000-000000000bad", false)
            .unwrap_err()
            .code,
        ErrorCode::ContextStale
    );
    // The old window changed its registry meanwhile: commit refuses and a new preview is needed.
    let again = f.service.preview_migration().unwrap();
    older(&f, &registry_of(&[ROOT_A, ROOT_B]));
    let changed = f
        .service
        .commit_migration(&again.preview_id, false)
        .unwrap_err();
    assert_eq!(
        (changed.code, changed.field),
        (ErrorCode::ContextStale, Some("sources_changed"))
    );
    assert!(matches!(f.service.overview().store, StoreStatus::Missing));
    let third = f.service.preview_migration().unwrap();
    assert_eq!(third.counts.projects, 2);
    f.service
        .commit_migration(&third.preview_id, false)
        .unwrap();
    assert!(matches!(
        f.service.overview().store,
        StoreStatus::Ready { .. }
    ));
    assert_eq!(f.service.list_projects(None).unwrap().items.len(), 2);
    // The older file was never written, and its exact bytes are in the private snapshot.
    let kept = std::fs::read_dir(f.dir.0.join("state/wyss-console/projects/v1"))
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().starts_with("migration-"))
        .unwrap()
        .path();
    assert_eq!(
        std::fs::read(kept.join("registry.json")).unwrap(),
        registry_of(&[ROOT_A, ROOT_B]).as_bytes()
    );
    // A later change is an incremental preview that adds only what is new.
    f.owner.add_root("/home/fixture/work/c", 64768, 1003);
    older(&f, &registry_of(&[ROOT_A, ROOT_B, "/home/fixture/work/c"]));
    let next = f.service.preview_migration().unwrap();
    assert_eq!(
        (next.mode, next.older_changed, next.counts.projects),
        (crate::projects::migration::Mode::Incremental, true, 1)
    );
    f.service.commit_migration(&next.preview_id, false).unwrap();
    assert_eq!(f.service.list_projects(None).unwrap().items.len(), 3);
    assert_eq!(f.owner.launches(), 0);
}

#[test]
fn an_alias_is_committed_only_when_accepted() {
    let f = fixture();
    f.service.set_enabled(true).unwrap();
    f.owner.script(|s| {
        s.roots.insert(
            "/home/fixture/work/a-link".into(),
            Ok(RootInfo {
                path: ROOT_A.into(),
                device: 64768,
                inode: 1001,
                kind: CheckoutKind::Git,
                native_eligible: true,
                config_eligible: true,
            }),
        );
    });
    older(&f, &registry_of(&[ROOT_A, "/home/fixture/work/a-link"]));
    let summary = f.service.preview_migration().unwrap();
    assert!(summary.needs_alias_acceptance && !summary.can_commit);
    assert_eq!(
        f.service
            .commit_migration(&summary.preview_id, false)
            .unwrap_err()
            .code,
        ErrorCode::ContextStale
    );
    f.service
        .commit_migration(&summary.preview_id, true)
        .unwrap();
    assert_eq!(f.service.list_projects(None).unwrap().items.len(), 1);
}

#[test]
fn a_blocking_conflict_cannot_be_committed_even_when_aliases_are_accepted() {
    let f = fixture();
    f.service.set_enabled(true).unwrap();
    older(&f, r#"{"schema":2,"sessions":[],"projects":[]}"#);
    let summary = f.service.preview_migration().unwrap();
    assert!(!summary.can_commit);
    assert!(
        f.service
            .commit_migration(&summary.preview_id, true)
            .is_err()
    );
    assert!(matches!(f.service.overview().store, StoreStatus::Missing));
}

#[test]
fn accepting_aliases_never_overrides_a_conflict_that_blocks() {
    use crate::projects::owner::RootInfo;
    use wyss_console::contracts::m1::records::CheckoutKind;
    let f = fixture();
    f.service.set_enabled(true).unwrap();
    let alias = "/home/fixture/work/a-link";
    f.owner.script(|s| {
        s.roots.insert(
            alias.to_owned(),
            Ok(RootInfo {
                path: ROOT_A.to_owned(),
                device: 64768,
                inode: 1001,
                kind: CheckoutKind::Git,
                native_eligible: true,
                config_eligible: true,
            }),
        );
    });
    older(&f, &registry_of(&[ROOT_A, alias]));
    // A tab file the import cannot read blocks it, whatever else is accepted.
    std::fs::write(
        f.dir.0.join("state/wyss-console/layout.json"),
        br#"{"version":2,"tabs":[]}"#,
    )
    .unwrap();
    let summary = f.service.preview_migration().unwrap();
    assert!(summary.needs_alias_acceptance && !summary.can_commit);
    assert!(
        f.service
            .commit_migration(&summary.preview_id, true)
            .is_err()
    );
    assert!(matches!(f.service.overview().store, StoreStatus::Missing));
}

#[test]
fn an_update_preview_is_committed_at_its_own_revision_or_not_at_all() {
    let f = fixture();
    f.service.set_enabled(true).unwrap();
    older(&f, &registry_of(&[ROOT_A]));
    let first = f.service.preview_migration().unwrap();
    f.service
        .commit_migration(&first.preview_id, false)
        .unwrap();
    // The older state gained a folder; an update preview offers it.
    f.owner.add_root(ROOT_B, 64768, 1002);
    older(&f, &registry_of(&[ROOT_A, ROOT_B]));
    let update = f.service.preview_migration().unwrap();
    assert_eq!(update.counts.projects, 1);
    // Meanwhile another window registers that same folder by hand.
    f.register(ROOT_B);
    let before = f.store();
    let refused = f
        .service
        .commit_migration(&update.preview_id, false)
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::RevisionConflict);
    assert_eq!(f.store(), before, "nothing was added over the newer store");
    assert_eq!(
        before
            .bindings
            .iter()
            .filter(|b| b.path.as_str() == ROOT_B)
            .count(),
        1,
        "the folder is one project"
    );
}

#[test]
fn a_host_replaced_between_preview_and_commit_is_refused() {
    let f = fixture();
    f.service.set_enabled(true).unwrap();
    f.owner
        .script(|s| s.host = Some(Ok(vec![native_row(TERMINAL_1, None)])));
    older(&f, &registry_of(&[ROOT_A]));
    let summary = f.service.preview_migration().unwrap();
    f.owner.script(|s| s.epoch = "h1:9:9:2:2".into());
    assert_eq!(
        f.service
            .commit_migration(&summary.preview_id, false)
            .unwrap_err()
            .code,
        ErrorCode::HostChanged
    );
    assert!(matches!(f.service.overview().store, StoreStatus::Missing));
}

// ---- rollback ---------------------------------------------------------------------------------------------

#[test]
fn rollback_is_a_report_and_refuses_while_a_launch_is_unaccounted_for() {
    let f = fixture().enabled();
    let (project, workspace) = f.register(ROOT_A);
    let report = f.service.rollback_report().unwrap();
    assert!(report.can_disable);
    assert_eq!(
        (report.projects, report.workspaces, report.sessions),
        (1, 2, 0)
    );
    let path = f
        .dir
        .0
        .join("state/wyss-console/projects/v1/rollback-report.json");
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    // An ambiguous launch is unaccounted for.
    let leaf = f.env.uuid();
    f.send(f.context(Some(&project), Some(&workspace)), json!({"kind": "saveWorkspace", "workspaceId": workspace,
        "expectedWorkspaceRevision": "1", "label": "Main", "layout": {"kind": "leaf", "id": leaf}, "focus": null,
        "pinned": false, "leaves": [leaf_json(&leaf, Value::Null)],
        "preferences": {"provider": null, "mode": "native", "nativeProfile": null, "managedSafetyProfile": null}})).unwrap();
    f.owner
        .script(|s| s.shell.push(Err(LaunchError::Ambiguous)));
    assert!(f.send(f.context(Some(&project), Some(&workspace)), json!({"kind": "launch", "workspaceId": workspace,
        "leafId": leaf, "provider": "shell", "mode": "native", "nativeProfile": null, "managedSafetyProfile": null})).is_err());
    let blocked = f.service.rollback_report().unwrap();
    assert!(!blocked.can_disable);
    assert_eq!(blocked.unfinished_launches, 1);
    // What it could have started is read, never attached: only running terminals no association
    // holds, of the kind the launch asked for, and a folder name rather than a path.
    let launch = f.store().operations[0].id.as_str().to_owned();
    f.owner.script(|s| {
        let mut shell = native_row(TERMINAL_2, None);
        shell.cwd = Some("/home/user/work/repo/".to_owned());
        let mut stopped = native_row(TERMINAL_1, None);
        stopped.running = false;
        s.host = Some(Ok(vec![
            shell,
            stopped,
            native_row("aaaaaaaa-0000-4000-8000-000000000003", Some("native-1")),
        ]));
    });
    let found = f.service.orphans(&launch).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].terminal_id, TERMINAL_2);
    assert_eq!(found[0].folder.as_deref(), Some("repo"));
    assert_eq!(
        f.service.orphans(&f.env.uuid()).unwrap_err().code,
        ErrorCode::ContextStale
    );
    // Disabling the gate changes no session and no store.
    let before = f.store();
    f.service.set_enabled(false).unwrap();
    assert_eq!(f.store(), before);
    assert_eq!(f.owner.launches(), 1);
}

// ---- the shapes the window parses --------------------------------------------------------------

/// What the desktop answers is written down once, here, and the window's parsers read the same
/// file (`tests/projects-contract.test.ts`). `WYSS_UPDATE_FIXTURES=1` rewrites it.
#[test]
fn the_answers_the_window_parses_match_the_recorded_shapes() {
    let f = fixture().enabled();
    let (project, workspace) = f.register(ROOT_A);
    let leaf = f.env.uuid();
    f.send(f.context(Some(&project), Some(&workspace)), json!({"kind": "saveWorkspace", "workspaceId": workspace,
        "expectedWorkspaceRevision": "1", "label": "Main", "layout": {"kind": "leaf", "id": leaf}, "focus": null,
        "pinned": false, "leaves": [leaf_json(&leaf, Value::Null)],
        "preferences": {"provider": null, "mode": "native", "nativeProfile": null, "managedSafetyProfile": null}})).unwrap();
    f.owner
        .script(|s| s.shell.push(Err(LaunchError::Ambiguous)));
    assert!(f.send(f.context(Some(&project), Some(&workspace)), json!({"kind": "launch", "workspaceId": workspace,
        "leafId": leaf, "provider": "shell", "mode": "native", "nativeProfile": null, "managedSafetyProfile": null})).is_err());
    f.owner.script(|s| {
        let mut row = native_row(TERMINAL_2, None);
        row.cwd = Some("/home/user/work/repo".to_owned());
        s.host = Some(Ok(vec![row]));
    });
    let launch = f.store().operations[0].id.as_str().to_owned();
    let answers = json!({
        "overview": f.service.overview(),
        "projects": f.service.list_projects(None).unwrap(),
        "workspace": f.service.get_workspace(&workspace, None).unwrap(),
        "orphans": f.service.orphans(&launch).unwrap(),
        "rollback": f.service.rollback_report().unwrap(),
    });
    // The test environment numbers its IDs from a counter shared by every test, so the IDs are
    // renumbered by first appearance: the file does not depend on which tests ran before.
    fn renumber(value: &mut Value, seen: &mut Vec<String>) {
        match value {
            Value::String(text) if text.starts_with("0000feed-0000-4000-8000-") => {
                let at = seen.iter().position(|s| s == text).unwrap_or_else(|| {
                    seen.push(text.clone());
                    seen.len() - 1
                });
                *text = format!("0000feed-0000-4000-8000-{:012x}", at + 1);
            }
            Value::Array(items) => items.iter_mut().for_each(|v| renumber(v, seen)),
            Value::Object(map) => map.values_mut().for_each(|v| renumber(v, seen)),
            _ => {}
        }
    }
    let mut answers = answers;
    renumber(&mut answers, &mut Vec::new());
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/projects-v1.json");
    let text = serde_json::to_string_pretty(&answers).unwrap() + "\n";
    if std::env::var_os("WYSS_UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    // By value: a formatter may lay the file out differently.
    let recorded: Value = serde_json::from_str(
        &std::fs::read_to_string(&path).expect("tests/fixtures/projects-v1.json"),
    )
    .unwrap();
    assert_eq!(
        recorded, answers,
        "the recorded answers changed; rerun with WYSS_UPDATE_FIXTURES=1 and read the diff"
    );
}

use std::os::unix::fs::PermissionsExt;

// ---- one shared inventory of the owners ---------------------------------------------------------

fn counted(f: &Fixture, what: &str) -> usize {
    f.owner
        .calls()
        .iter()
        .filter(|c| c.as_str() == what)
        .count()
}

#[test]
fn windows_reading_together_share_one_read_of_each_owner_until_something_acts() {
    let f = fixture().enabled();
    // The same service, with a time during which what the owners said is shared.
    let service = ProjectsService::new(
        Some(f.dir.0.join("state/wyss-console/projects/v1")),
        f.owner.clone(),
        f.env.clone(),
        Claims::new(Some(f.dir.0.join("run/claims"))),
    )
    .with_inventory_ttl(std::time::Duration::from_secs(60));
    let (project, workspace) = f.register(ROOT_A);
    f.owner.script(|s| s.calls.clear());
    // Many windows, many views, one read of the host and one of the Managed authorities.
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                service.overview();
                service.get_workspace(&workspace, Some("w-1")).unwrap();
                service.list_sessions(None, None, Some("w-2")).unwrap();
            });
        }
    });
    assert_eq!(counted(&f, "host"), 1, "{:?}", f.owner.calls());
    assert_eq!(counted(&f, "managed"), 1);
    // Showing a project is a read like the others and costs the owners nothing more.
    service.get_workspace(&workspace, None).unwrap();
    assert_eq!(counted(&f, "host"), 1);
    assert_eq!(counted(&f, "managed"), 1);
    // Anything that acts, or that decides what to do, asks the owner itself.
    service.orphans(&f.env.uuid()).unwrap_err();
    f.owner.script(|s| s.calls.clear());
    let leaf = f.env.uuid();
    let _ = (project, leaf);
    service
        .execute(
            serde_json::from_value(json!({
                "schema": 1, "operationId": f.env.uuid(),
                "expectedStoreRevision": f.store().revision.get().to_string(),
                "context": f.context(None, None),
                "command": {"kind": "closeWorkspace", "workspaceId": workspace},
            }))
            .unwrap(),
        )
        .unwrap();
    service.overview();
    service.get_workspace(&workspace, None).unwrap();
    assert_eq!(counted(&f, "host"), 1, "read again after a command");
    assert_eq!(counted(&f, "managed"), 1);
}

#[test]
fn what_the_owners_said_is_read_again_once_its_time_is_over() {
    let f = fixture().enabled();
    let service = ProjectsService::new(
        Some(f.dir.0.join("state/wyss-console/projects/v1")),
        f.owner.clone(),
        f.env.clone(),
        Claims::new(Some(f.dir.0.join("run/claims"))),
    )
    .with_inventory_ttl(std::time::Duration::from_millis(30));
    f.owner.script(|s| s.calls.clear());
    service.overview();
    service.overview();
    assert_eq!(counted(&f, "host"), 1);
    std::thread::sleep(std::time::Duration::from_millis(60));
    service.overview();
    assert_eq!(counted(&f, "host"), 2);
    // A host that was unreachable is shared for the same time and then asked again.
    f.owner
        .script(|s| s.host = Some(Err(OwnerError::HostOffline)));
    std::thread::sleep(std::time::Duration::from_millis(60));
    assert!(!service.overview().host.reachable);
    assert!(!service.overview().host.reachable);
    assert_eq!(counted(&f, "host"), 3);
}
