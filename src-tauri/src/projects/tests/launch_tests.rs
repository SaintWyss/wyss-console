//! M1-08, M1-10 and the idempotency rules: a launch is journalled before it is dispatched,
//! dispatched once, and completed afterwards. Whatever goes wrong in between, the owner is
//! never asked a second time for the same intent, and nothing is guessed from a title, a
//! working directory or the time.
use super::*;
use std::{sync::atomic::AtomicUsize, sync::mpsc, thread, time::Duration};
use wyss_console::contracts::m1::records::{AssociationState, OperationState, Target};

const TERMINAL_1: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const TERMINAL_2: &str = "aaaaaaaa-0000-4000-8000-000000000002";
const NATIVE_1: &str = "0123456789abcdef0123456789abcdef";
const NATIVE_2: &str = "fedcba9876543210fedcba9876543210";
const STREAM_1: &str = "bbbbbbbb-0000-4000-8000-000000000001";
const STREAM_2: &str = "bbbbbbbb-0000-4000-8000-000000000002";
const MANAGED_1: &str = "cccccccc-0000-4000-8000-000000000001";

struct Ready {
    world: World,
    project: String,
    workspace: String,
    leaf: String,
}

fn ready() -> Ready {
    let world = World::new();
    let (project, _, workspace) = world.register(ROOT_A);
    let leaf = world.env.uuid();
    world
        .send(
            world.context(Some(&project), Some(&workspace)),
            json!({"kind": "saveWorkspace", "workspaceId": workspace, "expectedWorkspaceRevision": "1",
                   "label": "Main", "layout": {"kind": "leaf", "id": leaf}, "focus": null,
                   "pinned": false, "leaves": [leaf_json(&leaf, Value::Null)],
                   "preferences": {"provider": null, "mode": "native", "nativeProfile": null,
                                   "managedSafetyProfile": null}}),
        )
        .unwrap();
    Ready {
        world,
        project,
        workspace,
        leaf,
    }
}

impl Ready {
    fn command(&self, provider: &str, mode: &str, native: Value, safety: Value) -> Value {
        json!({"kind": "launch", "workspaceId": self.workspace, "leafId": self.leaf,
               "provider": provider, "mode": mode, "nativeProfile": native,
               "managedSafetyProfile": safety})
    }

    fn send_as(&self, operation: &str, command: Value) -> Result<Receipt, Failure> {
        self.engine_send(&self.world.engine(), operation, command)
    }

    fn engine_send(
        &self,
        engine: &Engine<'_>,
        operation: &str,
        command: Value,
    ) -> Result<Receipt, Failure> {
        let revision = self.world.now().revision;
        engine.execute(
            self.world.envelope(
                operation,
                revision,
                self.world
                    .context(Some(&self.project), Some(&self.workspace)),
                command,
            ),
        )
    }

    fn native_ok(&self) {
        self.world.owner.script(|s| {
            s.native.push(Ok(NativeLaunched {
                session_id: NATIVE_1.into(),
                terminal_id: TERMINAL_1.into(),
            }));
        });
    }

    fn operation(&self, id: &str) -> Option<wyss_console::contracts::m1::records::Operation> {
        self.world
            .now()
            .operations
            .into_iter()
            .find(|o| o.id.as_str() == id)
    }
}

fn op_id(world: &World) -> String {
    world.env.uuid()
}

// ---- the happy paths ---------------------------------------------------------------------

#[test]
fn a_native_launch_is_journalled_dispatched_once_and_committed() {
    let r = ready();
    r.native_ok();
    let op = op_id(&r.world);
    let receipt = r
        .send_as(&op, r.command("claude", "native", Value::Null, Value::Null))
        .unwrap();
    assert!(!receipt.replayed);
    let store = r.world.now();
    let operation = r.operation(&op).unwrap();
    assert_eq!(operation.state, OperationState::Committed);
    assert!(operation.claim.is_none());
    let association = store
        .sessions
        .iter()
        .find(|a| Some(&a.id) == operation.association_id.as_ref())
        .unwrap();
    assert_eq!(association.state, AssociationState::Attached);
    assert_eq!(association.operation_id.as_ref().unwrap().as_str(), op);
    match &association.target {
        Target::Native {
            session_id,
            terminal_id,
            host_epoch,
        } => {
            assert_eq!(session_id.as_str(), NATIVE_1);
            assert_eq!(terminal_id.as_str(), TERMINAL_1);
            assert_eq!(host_epoch.as_str(), "h1:1:1:100:1000");
        }
        other => panic!("{other:?}"),
    }
    let leaf = &store
        .workspaces
        .iter()
        .find(|w| w.id.as_str() == r.workspace)
        .unwrap()
        .leaves[0];
    assert_eq!(leaf.association_id.as_ref(), Some(&association.id));
    assert_eq!(r.world.owner.launches(), 1);
    assert!(
        r.world
            .owner
            .calls()
            .iter()
            .any(|c| c.contains("launch_native /home/fixture/work/a h1:1:1:100:1000"))
    );
}

#[test]
fn the_same_operation_again_returns_the_commit_and_never_launches_twice() {
    let r = ready();
    r.native_ok();
    let op = op_id(&r.world);
    let command = r.command("codex", "native", Value::Null, Value::Null);
    r.send_as(&op, command.clone()).unwrap();
    let again = r.send_as(&op, command.clone()).unwrap();
    assert!(again.replayed);
    assert_eq!(r.world.owner.launches(), 1);
    // The same ID with another provider is not the same request.
    let different = r.command("claude", "native", Value::Null, Value::Null);
    refused(r.send_as(&op, different), ErrorCode::InvalidRecord);
    assert_eq!(r.world.owner.launches(), 1);
    // A different operation on the now-occupied leaf is busy.
    refused(r.send_as(&op_id(&r.world), command), ErrorCode::LeafBusy);
}

#[test]
fn a_launch_names_exactly_one_kind_of_profile() {
    let r = ready();
    let native = json!({"id": NATIVE_2, "version": "1"});
    for (provider, mode, n, m, code) in [
        (
            "claude",
            "native",
            Value::Null,
            json!("inference"),
            ErrorCode::InvalidRecord,
        ),
        (
            "codex",
            "native",
            native.clone(),
            json!("local_tools"),
            ErrorCode::InvalidRecord,
        ),
        (
            "codex",
            "managed",
            native.clone(),
            json!("inference"),
            ErrorCode::InvalidRecord,
        ),
        (
            "codex",
            "managed",
            Value::Null,
            Value::Null,
            ErrorCode::InvalidRecord,
        ),
        (
            "shell",
            "native",
            native.clone(),
            Value::Null,
            ErrorCode::InvalidRecord,
        ),
        (
            "shell",
            "managed",
            Value::Null,
            json!("inference"),
            ErrorCode::InvalidRecord,
        ),
        (
            "claude",
            "managed",
            Value::Null,
            json!("inference"),
            ErrorCode::UnsupportedCapability,
        ),
    ] {
        refused(
            r.send_as(&op_id(&r.world), r.command(provider, mode, n, m)),
            code,
        );
    }
    assert_eq!(r.world.owner.launches(), 0);
    assert!(
        r.world.now().operations.is_empty(),
        "nothing refused before dispatch is journalled"
    );
}

#[test]
fn a_managed_launch_keeps_the_requested_profile_and_prepares_only() {
    for (profile, expected) in [
        ("inference", ManagedSafetyProfile::Inference),
        ("local_tools", ManagedSafetyProfile::LocalTools),
    ] {
        let r = ready();
        r.world.owner.script(|s| {
            s.prepared.push(Ok(ManagedLaunched {
                protocol: ManagedProtocol::P1,
                store_stream_id: STREAM_1.into(),
                session_id: MANAGED_1.into(),
            }));
        });
        let op = op_id(&r.world);
        r.send_as(
            &op,
            r.command("codex", "managed", Value::Null, json!(profile)),
        )
        .unwrap();
        let store = r.world.now();
        let operation = r.operation(&op).unwrap();
        assert_eq!(operation.managed_safety_profile, Some(expected));
        assert!(operation.native_profile.is_none());
        let association = &store.sessions[0];
        match &association.target {
            Target::Managed {
                protocol,
                store_stream_id,
                session_id,
                requested_safety_profile,
            } => {
                assert_eq!(
                    (*protocol, store_stream_id.as_str(), session_id.as_str()),
                    (ManagedProtocol::P1, STREAM_1, MANAGED_1)
                );
                assert_eq!(*requested_safety_profile, Some(expected));
            }
            other => panic!("{other:?}"),
        }
        // The intent is the operation itself, so a retry is the same intent.
        assert!(
            r.world
                .owner
                .calls()
                .iter()
                .any(|c| c == &format!("launch_managed /home/fixture/work/a {op}"))
        );
        assert!(
            !r.world
                .owner
                .calls()
                .iter()
                .any(|c| c.starts_with("launch_native"))
        );
    }
}

// ---- before dispatch: no journal, no effect ----------------------------------------------------

#[test]
fn an_unreachable_or_changed_host_stops_a_native_launch_before_anything_is_written() {
    let r = ready();
    r.world
        .owner
        .script(|s| s.host = Some(Err(OwnerError::HostOffline)));
    refused(
        r.send_as(
            &op_id(&r.world),
            r.command("codex", "native", Value::Null, Value::Null),
        ),
        ErrorCode::HostOffline,
    );
    assert!(r.world.now().operations.is_empty());
    assert_eq!(r.world.owner.launches(), 0);
}

#[test]
fn a_binding_revision_that_moved_is_a_stale_context() {
    let r = ready();
    let stale = r.world.context(Some(&r.project), Some(&r.workspace));
    let token = r.world.pick(ROOT_B, Purpose::Relink);
    let store = r.world.now();
    let binding = store.projects[0].default_binding_id.clone().unwrap();
    r.world
        .send(
            r.world.context(Some(&r.project), None),
            json!({"kind": "relink", "projectId": r.project, "oldBindingId": binding.as_str(), "selectionToken": token}),
        )
        .unwrap();
    let revision = r.world.now().revision;
    let result = r.world.engine().execute(r.world.envelope(
        &op_id(&r.world),
        revision,
        stale,
        r.command("codex", "native", Value::Null, Value::Null),
    ));
    refused(result, ErrorCode::ContextStale);
    assert_eq!(r.world.owner.launches(), 0);
}

#[test]
fn an_owner_that_refuses_before_dispatch_leaves_a_refused_receipt_and_a_free_leaf() {
    let r = ready();
    r.world.owner.script(|s| {
        s.native.push(Err(LaunchError::NotDispatched(
            OwnerError::ProfileUnavailable,
        )))
    });
    let op = op_id(&r.world);
    let native = json!({"id": NATIVE_2, "version": "3"});
    refused(
        r.send_as(&op, r.command("codex", "native", native, Value::Null)),
        ErrorCode::ProfileUnavailable,
    );
    assert_eq!(r.operation(&op).unwrap().state, OperationState::Refused);
    assert!(r.operation(&op).unwrap().claim.is_none());
    // Nothing is held: another operation can use the leaf.
    r.native_ok();
    r.send_as(
        &op_id(&r.world),
        r.command("codex", "native", Value::Null, Value::Null),
    )
    .unwrap();
    assert_eq!(r.world.owner.launches(), 2);
}

// ---- M1-10: lost acknowledgements, concurrency and crashes ----------------------------------

#[test]
fn two_windows_launching_at_once_dispatch_once() {
    let r = Arc::new(ready());
    let (release, gate) = mpsc::channel();
    r.world.owner.script(|s| s.gate = Some(gate));
    r.native_ok();
    let first_op = op_id(&r.world);
    let command = r.command("codex", "native", Value::Null, Value::Null);
    let worker = {
        let (r, op, command) = (r.clone(), first_op.clone(), command.clone());
        thread::spawn(move || r.send_as(&op, command))
    };
    // Wait until the first launch is journalled and inside the owner.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while r.operation(&first_op).is_none() {
        assert!(std::time::Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        r.operation(&first_op).unwrap().state,
        OperationState::Pending
    );
    // The same intent from a second window, and a different intent for the same leaf.
    let same = r.send_as(&first_op, command.clone());
    assert_eq!(same.unwrap_err().code, ErrorCode::LeafBusy);
    let other = r.send_as(&op_id(&r.world), command);
    assert_eq!(other.unwrap_err().code, ErrorCode::LeafBusy);
    release.send(()).unwrap();
    worker.join().unwrap().unwrap();
    assert_eq!(r.world.owner.launches(), 1);
    assert_eq!(
        r.operation(&first_op).unwrap().state,
        OperationState::Committed
    );
}

#[test]
fn a_launch_whose_receipt_cannot_be_saved_is_ambiguous_and_never_repeated() {
    let seen = Arc::new(AtomicUsize::new(0));
    let counter = seen.clone();
    // The journal write (the first publish after registering and saving) succeeds; the
    // completion after the owner answered fails, as a lost acknowledgement would.
    let r0 = ready();
    let baseline = r0.world.now().revision;
    let _ = baseline;
    drop(r0);
    let r = ready();
    let faults: Faults = Arc::new(move |at| {
        if at == Point::BeforeRename && counter.fetch_add(1, Ordering::SeqCst) >= 1 {
            Err(std::io::Error::other("injected"))
        } else {
            Ok(())
        }
    });
    let r = Ready {
        world: r.world.with_faults(faults),
        ..r
    };
    r.native_ok();
    let op = op_id(&r.world);
    let failure = refused(
        r.send_as(&op, r.command("codex", "native", Value::Null, Value::Null)),
        ErrorCode::EffectAmbiguous,
    );
    assert_eq!(failure.effect, super::super::command::Effect::Unknown);
    assert_eq!(r.world.owner.launches(), 1);
    // The journal still says pending (its claim is this live process's), so another try waits.
    assert_eq!(r.operation(&op).unwrap().state, OperationState::Pending);
    refused(
        r.send_as(&op, r.command("codex", "native", Value::Null, Value::Null)),
        ErrorCode::LeafBusy,
    );
    // The process restarts: its claim is dead. Recovery makes the receipt ambiguous and never
    // replays it; the leaf stays held until the user reconciles.
    let restarted_env = FakeEnv::with_claim("c1:2:2:restarted");
    let restarted_store = Store::open(r.world.dir.0.join("v1")).unwrap();
    let engine = Engine {
        store: &restarted_store,
        owner: &*r.world.owner,
        env: &restarted_env,
        selections: &r.world.selections,
    };
    assert_eq!(engine.recover_pending().unwrap(), 1);
    assert_eq!(r.operation(&op).unwrap().state, OperationState::Ambiguous);
    assert_eq!(engine.recover_pending().unwrap(), 0);
    let command = r.command("codex", "native", Value::Null, Value::Null);
    refused(
        r.engine_send(&engine, &op, command.clone()),
        ErrorCode::EffectAmbiguous,
    );
    refused(
        r.engine_send(&engine, &op_id(&r.world), command),
        ErrorCode::LeafBusy,
    );
    assert_eq!(
        r.world.owner.launches(),
        1,
        "not one more launch, from any path"
    );
    // The user finds the session the launch started, and it becomes the leaf's.
    r.world
        .owner
        .script(|s| s.host = Some(Ok(vec![native_row(TERMINAL_1, Some(NATIVE_1))])));
    let revision = r.world.now().revision;
    engine
        .execute(r.world.envelope(
            &op_id(&r.world),
            revision,
            r.world.context(Some(&r.project), Some(&r.workspace)),
            json!({"kind": "reconcileLaunch", "launchOperationId": op,
                   "resolution": {"kind": "identified", "terminalId": TERMINAL_1, "sessionId": NATIVE_1}}),
        ))
        .unwrap();
    assert_eq!(r.operation(&op).unwrap().state, OperationState::Committed);
    assert_eq!(r.world.owner.launches(), 1);
}

#[test]
fn a_live_claim_is_not_recovered_from_under_another_window_but_a_dead_one_is() {
    let r = ready();
    let store = r.world.now();
    let revision = store.revision;
    let binding = store.bindings[0].clone();
    r.world
        .store
        .transact(revision, |s| {
            s.operations
                .push(wyss_console::contracts::m1::records::Operation {
                    id: wyss_console::contracts::m1::scalar::OperationId::parse(
                        "dddddddd-0000-4000-8000-000000000001",
                    )
                    .unwrap(),
                    kind: wyss_console::contracts::m1::records::OperationKind::Launch,
                    project_id: Some(s.projects[0].id.clone()),
                    workspace_id: s.workspaces[1].id.clone(),
                    leaf_id: s.workspaces[1].leaves[0].id.clone(),
                    binding_id: Some(binding.id.clone()),
                    binding_revision: Some(binding.revision),
                    provider: wyss_console::contracts::m1::records::LaunchProvider::Shell,
                    mode: wyss_console::contracts::m1::records::Mode::Native,
                    native_profile: None,
                    managed_safety_profile: None,
                    state: OperationState::Pending,
                    association_id: None,
                    claim: Some(
                        wyss_console::contracts::m1::scalar::EpochText::parse(
                            "c1:3:3:other-window",
                        )
                        .unwrap(),
                    ),
                    created_at: wyss_console::contracts::UtcTimestamp::try_from(
                        "2026-10-23T12:00:00Z".to_owned(),
                    )
                    .unwrap(),
                });
            Ok::<(), ()>(())
        })
        .unwrap()
        .unwrap();
    // Another window of a live process is still completing it: left alone.
    r.world.env.mark_live("c1:3:3:other-window");
    assert_eq!(r.world.engine().recover_pending().unwrap(), 0);
    assert_eq!(r.world.now().operations[0].state, OperationState::Pending);
    // Its process is gone: the receipt becomes ambiguous, never replayed.
    let restarted = FakeEnv::with_claim("c1:9:9:restarted");
    let engine = Engine {
        store: &r.world.store,
        owner: &*r.world.owner,
        env: &restarted,
        selections: &r.world.selections,
    };
    assert_eq!(engine.recover_pending().unwrap(), 1);
    assert_eq!(r.world.now().operations[0].state, OperationState::Ambiguous);
    assert_eq!(r.world.owner.launches(), 0);
}

#[test]
fn an_owner_reply_that_cannot_be_trusted_is_ambiguous_and_creates_no_association() {
    let r = ready();
    r.world.owner.script(|s| {
        s.native.push(Ok(NativeLaunched {
            session_id: "not-a-native-id".into(),
            terminal_id: TERMINAL_1.into(),
        }));
    });
    let op = op_id(&r.world);
    refused(
        r.send_as(&op, r.command("codex", "native", Value::Null, Value::Null)),
        ErrorCode::EffectAmbiguous,
    );
    assert_eq!(r.operation(&op).unwrap().state, OperationState::Ambiguous);
    assert!(
        r.world.now().sessions.is_empty(),
        "an incompatible ID is never coerced into one"
    );
}

#[test]
fn an_ambiguous_dispatch_blocks_a_retry_and_absent_needs_an_empty_owner() {
    let r = ready();
    r.world
        .owner
        .script(|s| s.native.push(Err(LaunchError::Ambiguous)));
    let op = op_id(&r.world);
    let command = r.command("codex", "native", Value::Null, Value::Null);
    refused(r.send_as(&op, command.clone()), ErrorCode::EffectAmbiguous);
    refused(r.send_as(&op, command.clone()), ErrorCode::EffectAmbiguous);
    refused(
        r.send_as(&op_id(&r.world), command.clone()),
        ErrorCode::LeafBusy,
    );
    assert_eq!(r.world.owner.launches(), 1);
    // A terminal nobody holds is running: "absent" cannot be confirmed.
    r.world
        .owner
        .script(|s| s.host = Some(Ok(vec![native_row(TERMINAL_2, Some(NATIVE_2))])));
    let reconcile = |resolution: Value| {
        let revision = r.world.now().revision;
        r.world.engine().execute(r.world.envelope(
            &op_id(&r.world),
            revision,
            r.world.context(Some(&r.project), Some(&r.workspace)),
            json!({"kind": "reconcileLaunch", "launchOperationId": op, "resolution": resolution}),
        ))
    };
    refused(
        reconcile(json!({"kind": "absent"})),
        ErrorCode::OwnerRefused,
    );
    // Identifying the wrong kind of session is refused too.
    refused(
        reconcile(json!({"kind": "identified", "terminalId": TERMINAL_2, "sessionId": NATIVE_1})),
        ErrorCode::OwnerRefused,
    );
    r.world.owner.script(|s| s.host = Some(Ok(Vec::new())));
    reconcile(json!({"kind": "absent"})).unwrap();
    assert_eq!(r.operation(&op).unwrap().state, OperationState::Refused);
    // The leaf is free again, with a new operation ID.
    r.native_ok();
    r.send_as(&op_id(&r.world), command).unwrap();
    assert_eq!(r.world.owner.launches(), 2);
}

#[test]
fn compacted_receipts_free_the_ceiling_but_their_ids_are_never_live_again() {
    let r = ready();
    let revision = r.world.now().revision;
    r.world
        .store
        .transact(revision, |s| {
            for n in 0..512u32 {
                let mut o = wyss_console::contracts::m1::records::Operation {
                    id: wyss_console::contracts::m1::scalar::OperationId::parse(&format!(
                        "dddddddd-0000-4000-8000-{n:012x}"
                    ))
                    .unwrap(),
                    kind: wyss_console::contracts::m1::records::OperationKind::Launch,
                    project_id: None,
                    workspace_id: s.workspaces[0].id.clone(),
                    leaf_id: wyss_console::contracts::m1::scalar::LeafId::parse(
                        "eeeeeeee-0000-4000-8000-000000000001",
                    )
                    .unwrap(),
                    binding_id: None,
                    binding_revision: None,
                    provider: wyss_console::contracts::m1::records::LaunchProvider::Shell,
                    mode: wyss_console::contracts::m1::records::Mode::Native,
                    native_profile: None,
                    managed_safety_profile: None,
                    state: OperationState::Refused,
                    association_id: None,
                    claim: None,
                    created_at: wyss_console::contracts::UtcTimestamp::try_from(
                        "2026-10-23T12:00:00Z".to_owned(),
                    )
                    .unwrap(),
                };
                o.state = OperationState::Refused;
                s.operations.push(o);
            }
            Ok::<(), ()>(())
        })
        .unwrap()
        .unwrap();
    r.native_ok();
    let command = r.command("codex", "native", Value::Null, Value::Null);
    let op = op_id(&r.world);
    refused(r.send_as(&op, command.clone()), ErrorCode::CapacityExceeded);
    assert_eq!(
        r.world.owner.launches(),
        0,
        "the ceiling refuses before the owner is asked"
    );
    assert_eq!(r.world.engine().compact_receipts().unwrap(), 512);
    let store = r.world.now();
    assert!(store.operations.is_empty() && store.closed_operations.len() == 512);
    // A closed ID is not an operation that can be started again.
    let closed = store.closed_operations[0].as_str().to_owned();
    refused(
        r.send_as(&closed, command.clone()),
        ErrorCode::InvalidRecord,
    );
    r.send_as(&op, command).unwrap();
    assert_eq!(r.world.owner.launches(), 1);
}

// ---- M1-08: Managed identities ------------------------------------------------------------

fn managed_store(
    r: &Ready,
    protocol: ManagedProtocol,
    stream: &str,
    requested: Option<ManagedSafetyProfile>,
) -> String {
    let revision = r.world.now().revision;
    let association = r.world.env.uuid();
    let id = association.clone();
    r.world
        .store
        .transact(revision, |s| {
            let project = s.projects[0].id.clone();
            let binding = s.bindings[0].clone();
            s.sessions
                .push(wyss_console::contracts::m1::records::Association {
                    id: wyss_console::contracts::m1::scalar::AssociationId::parse(&association)
                        .unwrap(),
                    origin_project_id: Some(project),
                    host_id: s.host_id.clone(),
                    binding_id: Some(binding.id),
                    binding_revision: Some(binding.revision),
                    target: Target::Managed {
                        protocol,
                        store_stream_id: wyss_console::contracts::m1::scalar::StoreStreamId::parse(
                            stream,
                        )
                        .unwrap(),
                        session_id: wyss_console::contracts::m1::scalar::ManagedSessionId::parse(
                            MANAGED_1,
                        )
                        .unwrap(),
                        requested_safety_profile: requested,
                    },
                    state: AssociationState::Detached,
                    operation_id: None,
                });
            Ok::<(), ()>(())
        })
        .unwrap()
        .unwrap();
    id
}

#[test]
fn managed_sessions_of_p1_and_p2_with_the_same_id_are_never_conflated() {
    let r = ready();
    let p1 = managed_store(
        &r,
        ManagedProtocol::P1,
        STREAM_1,
        Some(ManagedSafetyProfile::Inference),
    );
    let p2 = managed_store(
        &r,
        ManagedProtocol::P2,
        STREAM_2,
        Some(ManagedSafetyProfile::LocalTools),
    );
    // Only P2's session is in the authority's list, under its own stream.
    r.world.owner.script(|s| {
        s.managed = Ok(vec![managed_row(
            ManagedProtocol::P2,
            STREAM_2,
            MANAGED_1,
            Some(ManagedSafetyProfile::LocalTools),
        )]);
    });
    let attach = |association: &str| {
        r.world.send(
            r.world.context(Some(&r.project), Some(&r.workspace)),
            json!({"kind": "attach", "workspaceId": r.workspace, "leafId": r.leaf, "associationId": association}),
        )
    };
    refused(attach(&p1), ErrorCode::OwnerRefused);
    attach(&p2).unwrap();
    assert_eq!(
        r.world.owner.launches(),
        0,
        "attaching asks the owner questions and starts nothing"
    );
}

#[test]
fn a_requested_profile_must_match_what_the_authority_attached_and_unknown_is_not_a_match() {
    for (requested, effective, ok) in [
        (
            Some(ManagedSafetyProfile::Inference),
            Some(ManagedSafetyProfile::Inference),
            true,
        ),
        (
            Some(ManagedSafetyProfile::LocalTools),
            Some(ManagedSafetyProfile::Inference),
            false,
        ),
        (
            Some(ManagedSafetyProfile::Inference),
            Some(ManagedSafetyProfile::LocalTools),
            false,
        ),
        (Some(ManagedSafetyProfile::LocalTools), None, false),
        // An old import with no request is placed read-only, and nothing is inferred for it.
        (None, Some(ManagedSafetyProfile::LocalTools), true),
        (None, None, true),
    ] {
        let r = ready();
        let association = managed_store(&r, ManagedProtocol::P2, STREAM_2, requested);
        r.world.owner.script(|s| {
            s.managed = Ok(vec![managed_row(
                ManagedProtocol::P2,
                STREAM_2,
                MANAGED_1,
                effective,
            )])
        });
        let result = r.world.send(
            r.world.context(Some(&r.project), Some(&r.workspace)),
            json!({"kind": "attach", "workspaceId": r.workspace, "leafId": r.leaf, "associationId": association}),
        );
        match (ok, result) {
            (true, Ok(_)) => {}
            (false, Err(f)) => {
                assert_eq!(
                    (f.code, f.field),
                    (ErrorCode::OwnerRefused, Some("managedSafetyProfile")),
                    "{requested:?} {effective:?}"
                );
            }
            (ok, other) => panic!("{requested:?} {effective:?} ok={ok}: {other:?}"),
        }
        assert_eq!(r.world.owner.launches(), 0);
    }
}

#[test]
fn choosing_a_project_or_a_workspace_never_reaches_an_owner_with_an_effect() {
    let r = ready();
    let (other, _, other_workspace) = r.world.register(ROOT_B);
    for command in [
        json!({"kind": "closeWorkspace", "workspaceId": r.workspace}),
        json!({"kind": "archiveProject", "projectId": r.project, "archived": true}),
        json!({"kind": "archiveProject", "projectId": r.project, "archived": false}),
    ] {
        r.world
            .send(
                r.world.context(Some(&r.project), Some(&r.workspace)),
                command,
            )
            .unwrap();
    }
    r.world
        .send(
            r.world.context(Some(&other), Some(&other_workspace)),
            json!({"kind": "closeWorkspace", "workspaceId": other_workspace}),
        )
        .unwrap();
    assert_eq!(r.world.owner.launches(), 0);
    assert!(
        !r.world
            .owner
            .calls()
            .iter()
            .any(|c| c.starts_with("launch_") || c == "create_folder")
    );
}

#[test]
fn a_managed_launch_of_unknown_outcome_is_retried_only_by_the_user_and_with_its_original_intent() {
    let r = ready();
    r.world
        .owner
        .script(|s| s.prepared.push(Err(LaunchError::Ambiguous)));
    let op = op_id(&r.world);
    let command = r.command("codex", "managed", Value::Null, json!("local_tools"));
    refused(r.send_as(&op, command.clone()), ErrorCode::EffectAmbiguous);
    // Nothing retries by itself, from any path.
    refused(r.send_as(&op, command.clone()), ErrorCode::EffectAmbiguous);
    refused(r.send_as(&op_id(&r.world), command), ErrorCode::LeafBusy);
    assert_eq!(r.world.owner.launches(), 1);
    let retry = |r: &Ready| {
        let revision = r.world.now().revision;
        r.world.engine().execute(r.world.envelope(
            &op_id(&r.world),
            revision,
            r.world.context(Some(&r.project), Some(&r.workspace)),
            json!({"kind": "reconcileLaunch", "launchOperationId": op, "resolution": {"kind": "retryManaged"}}),
        ))
    };
    // The owner still refuses before doing anything: the receipt is still unknown.
    r.world.owner.script(|s| {
        s.prepared
            .push(Err(LaunchError::NotDispatched(OwnerError::HostOffline)))
    });
    refused(retry(&r), ErrorCode::HostOffline);
    assert_eq!(r.operation(&op).unwrap().state, OperationState::Ambiguous);
    // The owner settles the same intent.
    r.world.owner.script(|s| {
        s.prepared.push(Ok(ManagedLaunched {
            protocol: ManagedProtocol::P2,
            store_stream_id: STREAM_2.into(),
            session_id: MANAGED_1.into(),
        }));
    });
    retry(&r).unwrap();
    let operation = r.operation(&op).unwrap();
    assert_eq!(operation.state, OperationState::Committed);
    assert_eq!(
        operation.managed_safety_profile,
        Some(ManagedSafetyProfile::LocalTools)
    );
    let intents: Vec<String> = r
        .world
        .owner
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("launch_managed"))
        .map(|c| c.split(' ').nth(2).unwrap().to_owned())
        .collect();
    assert_eq!(intents.len(), 3);
    assert!(
        intents.iter().all(|i| i == &op),
        "every attempt used the original intent: {intents:?}"
    );
    match &r.world.now().sessions[0].target {
        Target::Managed {
            requested_safety_profile,
            protocol,
            ..
        } => {
            assert_eq!(
                (*protocol, *requested_safety_profile),
                (ManagedProtocol::P2, Some(ManagedSafetyProfile::LocalTools))
            );
        }
        other => panic!("{other:?}"),
    }
}
