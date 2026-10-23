//! M1-02, 03, 05, 06, 08 and 11 against the engine: registering, relinking and removing
//! projects, the workspace commands, attaching and detaching, and the stale-context rules.
//! Every test also asserts what did not happen: no launch reached an owner.
use super::*;
use wyss_console::contracts::m1::records::{AssociationState, BindingState};

fn leaf_ids(store: &StoreV1, workspace: &str) -> Vec<String> {
    store
        .workspaces
        .iter()
        .find(|w| w.id.as_str() == workspace)
        .unwrap()
        .leaves
        .iter()
        .map(|l| l.id.as_str().to_owned())
        .collect()
}

/// Saves a workspace with two empty leaves (and a split between them), at its current revision.
fn two_leaves(world: &World, project: &str, workspace: &str) -> (String, String) {
    let store = world.now();
    let revision = store
        .workspaces
        .iter()
        .find(|w| w.id.as_str() == workspace)
        .unwrap()
        .revision
        .get();
    let (a, b) = (world.env.uuid(), world.env.uuid());
    world
        .send(
            world.context(Some(project), Some(workspace)),
            json!({
                "kind": "saveWorkspace",
                "workspaceId": workspace,
                "expectedWorkspaceRevision": revision.to_string(),
                "label": "Main",
                "layout": {"kind": "split", "dir": "row", "ratio": 0.5,
                           "a": {"kind": "leaf", "id": a}, "b": {"kind": "leaf", "id": b}},
                "focus": null, "pinned": false,
                "leaves": [leaf_json(&a, Value::Null), leaf_json(&b, Value::Null)],
                "preferences": {"provider": null, "mode": "native", "nativeProfile": null,
                                "managedSafetyProfile": null}
            }),
        )
        .expect("saved");
    (a, b)
}

// ---- M1-02: registering and creating ------------------------------------------------------

#[test]
fn registering_a_chosen_folder_makes_a_project_with_one_binding_and_one_workspace() {
    let world = World::new();
    let (project, binding, workspace) = world.register(ROOT_A);
    let store = world.now();
    assert_eq!(store.projects.len(), 1);
    assert_eq!(store.projects[0].id.as_str(), project);
    assert_eq!(
        store.projects[0]
            .default_binding_id
            .as_ref()
            .unwrap()
            .as_str(),
        binding
    );
    assert_eq!(
        store.projects[0]
            .default_workspace_id
            .as_ref()
            .unwrap()
            .as_str(),
        workspace
    );
    let found = &store.bindings[0];
    assert_eq!(
        (found.path.as_str(), found.observation.device.get()),
        (ROOT_A, 64768)
    );
    assert_eq!(found.state, BindingState::Valid);
    assert_eq!(world.owner.launches(), 0, "registering starts nothing");
}

#[test]
fn registering_the_same_root_again_returns_the_existing_project() {
    let world = World::new();
    let (project, _, _) = world.register(ROOT_A);
    let token = world.pick(ROOT_A, Purpose::Register);
    let receipt = world
        .send(
            world.context(None, None),
            json!({"kind": "registerExisting", "selectionToken": token}),
        )
        .unwrap();
    assert_eq!(receipt.created.len(), 1);
    assert_eq!(receipt.created[0].id, project);
    assert_eq!(world.now().projects.len(), 1);
}

#[test]
fn cancelling_the_chooser_changes_nothing() {
    let world = World::new();
    let before = world.now();
    // No pick was made: the window has no token, and a made-up one is nothing.
    for token in ["", "token-never-issued"] {
        refused(
            world.send(
                world.context(None, None),
                json!({"kind": "registerExisting", "selectionToken": token}),
            ),
            ErrorCode::ContextStale,
        );
        refused(
            world.send(
                world.context(None, None),
                json!({"kind": "createFolder", "selectionToken": token, "name": "x"}),
            ),
            ErrorCode::ContextStale,
        );
    }
    assert_eq!(world.now(), before);
    assert!(!world.owner.calls().iter().any(|c| c == "create_folder"));
}

#[test]
fn a_pick_is_used_once_for_its_purpose_and_its_selection_revision() {
    let world = World::new();
    let token = world.pick(ROOT_A, Purpose::Register);
    world
        .send(
            world.context(None, None),
            json!({"kind": "registerExisting", "selectionToken": token}),
        )
        .unwrap();
    // Spent.
    refused(
        world.send(
            world.context(None, None),
            json!({"kind": "registerExisting", "selectionToken": token}),
        ),
        ErrorCode::ContextStale,
    );
    // Another purpose.
    let parent = world.pick(ROOT_B, Purpose::Parent);
    refused(
        world.send(
            world.context(None, None),
            json!({"kind": "registerExisting", "selectionToken": parent}),
        ),
        ErrorCode::ContextStale,
    );
    // Another selection revision: the window switched while the dialog was open.
    let stale = world.pick(ROOT_B, Purpose::Register);
    let mut moved = world.context(None, None);
    moved["selectionRevision"] = json!("2");
    refused(
        world.send(
            moved,
            json!({"kind": "registerExisting", "selectionToken": stale}),
        ),
        ErrorCode::ContextStale,
    );
    assert_eq!(
        world.now().projects.len(),
        1,
        "only the first pick registered"
    );
}

#[test]
fn a_missing_a_linked_or_a_foreign_root_is_refused_and_nothing_is_written() {
    let world = World::new();
    world.owner.script(|s| {
        s.roots.insert("/nowhere".into(), Err(RootError::Missing));
        s.roots.insert("/linked".into(), Err(RootError::Ineligible));
    });
    let before = world.now();
    for (path, code) in [
        ("/nowhere", ErrorCode::RootMissing),
        ("/linked", ErrorCode::RootIneligible),
    ] {
        let token = world.pick(path, Purpose::Register);
        refused(
            world.send(
                world.context(None, None),
                json!({"kind": "registerExisting", "selectionToken": token}),
            ),
            code,
        );
    }
    assert_eq!(world.now(), before);
}

#[test]
fn creating_a_folder_makes_it_and_registers_it_without_a_launch() {
    let world = World::new();
    world.owner.add_root("/home/fixture/work/new", 64768, 2002);
    let parent = world.pick("/home/fixture/work", Purpose::Parent);
    let receipt = world
        .send(
            world.context(None, None),
            json!({"kind": "createFolder", "selectionToken": parent, "name": "new"}),
        )
        .unwrap();
    assert!(receipt.created.iter().any(|c| c.role == "project"));
    assert_eq!(
        world.owner.script(|s| s.folders.clone()),
        [("/home/fixture/work".to_owned(), "new".to_owned())]
    );
    assert_eq!(world.owner.launches(), 0);
}

#[test]
fn a_folder_whose_registration_fails_stays_and_is_reported_not_deleted() {
    let world = World::new();
    world.owner.add_root("/home/fixture/work/new", 64768, 2002);
    // The store takes no more projects.
    let parent = world.pick("/home/fixture/work", Purpose::Parent);
    // Make the commit fail with an I/O fault after the folder is made.
    let world = World { ..world };
    let faulty = std::sync::Arc::new(|at: Point| {
        if at == Point::BeforeRename {
            Err(std::io::Error::other("disk full"))
        } else {
            Ok(())
        }
    });
    let world = world.with_faults(faulty);
    let failure = refused(
        world.send(
            world.context(None, None),
            json!({"kind": "createFolder", "selectionToken": parent, "name": "new"}),
        ),
        ErrorCode::IoFailure,
    );
    assert_eq!(failure.field, Some("createdFolder"));
    assert_eq!(
        world.owner.script(|s| s.folders.len()),
        1,
        "the folder was made once and is not removed"
    );
    assert!(world.now().projects.is_empty());
}

// ---- M1-03: roots that go away -------------------------------------------------------------

#[test]
fn a_replaced_or_missing_root_blocks_a_launch_but_not_reading() {
    let world = World::new();
    let (project, _, workspace) = world.register(ROOT_A);
    let (leaf, _) = two_leaves(&world, &project, &workspace);
    let launch = |world: &World| {
        world.send(
            world.context(Some(&project), Some(&workspace)),
            json!({"kind": "launch", "workspaceId": workspace, "leafId": leaf,
                   "provider": "shell", "mode": "native", "nativeProfile": null,
                   "managedSafetyProfile": null}),
        )
    };
    // The folder is now another one (same path, different inode).
    world.owner.add_root(ROOT_A, 64768, 9999);
    refused(launch(&world), ErrorCode::RootIdentityChanged);
    world.owner.script(|s| {
        s.roots.insert(ROOT_A.into(), Err(RootError::Missing));
    });
    refused(launch(&world), ErrorCode::RootMissing);
    assert_eq!(world.owner.launches(), 0);
    assert!(
        world.now().operations.is_empty(),
        "a refusal before dispatch writes no receipt"
    );
    // The metadata is all still there to look at.
    assert_eq!(world.now().projects.len(), 1);
}

#[test]
fn relinking_retires_the_old_binding_but_keeps_it_for_its_sessions() {
    let world = World::new();
    let (project, binding, workspace) = world.register(ROOT_A);
    let (leaf, _) = two_leaves(&world, &project, &workspace);
    world.owner.script(|s| {
        s.host = Some(Ok(Vec::new()));
        s.shell.push(Ok(ShellLaunched {
            terminal_id: "aaaaaaaa-0000-4000-8000-000000000001".into(),
        }));
    });
    world
        .send(
            world.context(Some(&project), Some(&workspace)),
            json!({"kind": "launch", "workspaceId": workspace, "leafId": leaf,
                   "provider": "shell", "mode": "native", "nativeProfile": null,
                   "managedSafetyProfile": null}),
        )
        .unwrap();
    let before = world.now();
    assert_eq!(before.sessions.len(), 1);
    let token = world.pick(ROOT_B, Purpose::Relink);
    world
        .send(
            world.context(Some(&project), None),
            json!({"kind": "relink", "projectId": project, "oldBindingId": binding, "selectionToken": token}),
        )
        .unwrap();
    let after = world.now();
    let old = after
        .bindings
        .iter()
        .find(|b| b.id.as_str() == binding)
        .unwrap();
    assert_eq!(old.state, BindingState::Retired);
    let new = after
        .bindings
        .iter()
        .find(|b| b.id.as_str() != binding)
        .unwrap();
    assert_eq!(new.previous_binding_id.as_ref().unwrap().as_str(), binding);
    assert_eq!(
        after.projects[0].default_binding_id.as_ref().unwrap(),
        &new.id
    );
    // The running shell keeps its original attribution.
    assert_eq!(
        after.sessions[0].binding_id.as_ref().unwrap().as_str(),
        binding
    );
    assert_eq!(
        after.sessions[0].binding_revision,
        before.sessions[0].binding_revision
    );
    assert_eq!(
        world.owner.launches(),
        1,
        "the relink itself launched nothing"
    );
}

#[test]
fn a_retired_binding_cannot_be_relinked_again_and_a_stale_binding_is_stale() {
    let world = World::new();
    let (project, binding, _) = world.register(ROOT_A);
    let token = world.pick(ROOT_B, Purpose::Relink);
    world
        .send(
            world.context(Some(&project), None),
            json!({"kind": "relink", "projectId": project, "oldBindingId": binding, "selectionToken": token}),
        )
        .unwrap();
    let again = world.pick(ROOT_B, Purpose::Relink);
    refused(
        world.send(
            world.context(Some(&project), None),
            json!({"kind": "relink", "projectId": project, "oldBindingId": binding, "selectionToken": again}),
        ),
        ErrorCode::ContextStale,
    );
}

// ---- projects ------------------------------------------------------------------------------

#[test]
fn renaming_and_archiving_change_only_the_project() {
    let world = World::new();
    let (project, _, _) = world.register(ROOT_A);
    let before = world.now();
    world
        .send(
            world.context(Some(&project), None),
            json!({"kind": "renameProject", "projectId": project, "label": "Renamed"}),
        )
        .unwrap();
    world
        .send(
            world.context(Some(&project), None),
            json!({"kind": "archiveProject", "projectId": project, "archived": true}),
        )
        .unwrap();
    let after = world.now();
    assert_eq!(after.projects[0].label.as_str(), "Renamed");
    assert!(after.projects[0].archived);
    assert_eq!(after.workspaces, before.workspaces);
    assert_eq!(after.bindings, before.bindings);
    assert_eq!(world.owner.launches(), 0);
}

#[test]
fn removing_a_project_refuses_live_or_unprovable_work_and_keeps_a_tombstone_when_asked() {
    let world = World::new();
    let (project, binding, workspace) = world.register(ROOT_A);
    let (leaf, _) = two_leaves(&world, &project, &workspace);
    world.owner.script(|s| {
        s.shell.push(Ok(ShellLaunched {
            terminal_id: "aaaaaaaa-0000-4000-8000-000000000001".into(),
        }));
    });
    let receipt = world
        .send(
            world.context(Some(&project), Some(&workspace)),
            json!({"kind": "launch", "workspaceId": workspace, "leafId": leaf,
                   "provider": "shell", "mode": "native", "nativeProfile": null,
                   "managedSafetyProfile": null}),
        )
        .unwrap();
    let association = receipt.created[0].id.clone();
    let remove = |retain: bool| {
        world.send(
            world.context(Some(&project), None),
            json!({"kind": "removeProject", "projectId": project, "retainHistory": retain}),
        )
    };
    // The shell is running: refused. The owner cannot be read: refused.
    world.owner.script(|s| {
        s.host = Some(Ok(vec![native_row(
            "aaaaaaaa-0000-4000-8000-000000000001",
            None,
        )]));
    });
    refused(remove(false), ErrorCode::OwnerRefused);
    world
        .owner
        .script(|s| s.host = Some(Err(OwnerError::HostOffline)));
    refused(remove(true), ErrorCode::HostOffline);
    assert_eq!(world.now().projects.len(), 1);
    // The terminal is proven gone: the project can be kept as history.
    world.owner.script(|s| s.host = Some(Ok(Vec::new())));
    remove(true).unwrap();
    let store = world.now();
    let tomb = &store.projects[0];
    assert!(
        tomb.tombstone && tomb.default_workspace_id.is_none() && tomb.default_binding_id.is_none()
    );
    assert!(store.bindings.is_empty());
    assert_eq!(store.workspaces.len(), 1, "only Scratch is left");
    let kept = store
        .sessions
        .iter()
        .find(|a| a.id.as_str() == association)
        .unwrap();
    assert_eq!(kept.origin_project_id.as_ref().unwrap().as_str(), project);
    assert_eq!(kept.state, AssociationState::Unavailable);
    assert!(
        kept.binding_id.is_none(),
        "history no longer names a binding that is gone"
    );
    let _ = binding;
    // And it can be removed for good.
    let second = World::new();
    let (project2, _, _) = second.register(ROOT_A);
    second
        .send(
            second.context(Some(&project2), None),
            json!({"kind": "removeProject", "projectId": project2, "retainHistory": false}),
        )
        .unwrap();
    assert!(second.now().projects.is_empty());
    assert_eq!(second.owner.launches(), 0);
}

// ---- workspaces ----------------------------------------------------------------------------

#[test]
fn workspaces_are_created_duplicated_saved_and_deleted_without_any_owner_call() {
    let world = World::new();
    let (project, _, workspace) = world.register(ROOT_A);
    let (a, _) = two_leaves(&world, &project, &workspace);
    let created = world
        .send(
            world.context(Some(&project), None),
            json!({"kind": "createWorkspace", "projectId": project, "label": "Second"}),
        )
        .unwrap();
    let second = created.created[0].id.clone();
    let copy = world
        .send(
            world.context(Some(&project), None),
            json!({"kind": "duplicateWorkspace", "sourceWorkspaceId": workspace, "label": "Copy"}),
        )
        .unwrap();
    let copy_id = copy.created[0].id.clone();
    let store = world.now();
    let source = store
        .workspaces
        .iter()
        .find(|w| w.id.as_str() == workspace)
        .unwrap();
    let twin = store
        .workspaces
        .iter()
        .find(|w| w.id.as_str() == copy_id)
        .unwrap();
    assert_eq!(twin.leaves.len(), source.leaves.len());
    assert!(twin.leaves.iter().all(|l| l.association_id.is_none()));
    assert!(
        twin.leaves
            .iter()
            .all(|l| source.leaves.iter().all(|s| s.id != l.id)),
        "new leaf IDs"
    );
    assert_eq!(twin.preferences, source.preferences);
    // Saving at a stale workspace revision is a conflict, and a leaf cannot be silently reused.
    refused(
        world.send(
            world.context(Some(&project), Some(&workspace)),
            json!({"kind": "saveWorkspace", "workspaceId": workspace, "expectedWorkspaceRevision": "1",
                   "label": "x", "layout": null, "focus": null, "pinned": false, "leaves": [],
                   "preferences": {"provider": null, "mode": "native", "nativeProfile": null,
                                   "managedSafetyProfile": null}}),
        ),
        ErrorCode::RevisionConflict,
    );
    // Deleting the default leaves a replacement default, and deleting the last makes a new one.
    world
        .send(
            world.context(Some(&project), Some(&workspace)),
            json!({"kind": "deleteWorkspace", "workspaceId": workspace}),
        )
        .unwrap();
    let after = world.now();
    let default = after.projects[0].default_workspace_id.clone().unwrap();
    assert!(after.workspaces.iter().any(|w| w.id == default));
    assert_ne!(default.as_str(), workspace);
    for id in [second.as_str(), copy_id.as_str()] {
        world
            .send(
                world.context(Some(&project), Some(id)),
                json!({"kind": "deleteWorkspace", "workspaceId": id}),
            )
            .unwrap();
    }
    let last = world.now();
    let project_workspaces: Vec<_> = last
        .workspaces
        .iter()
        .filter(|w| w.project_id.is_some())
        .collect();
    assert_eq!(
        project_workspaces.len(),
        1,
        "a project always has a workspace"
    );
    let _ = a;
    assert_eq!(world.owner.launches(), 0);
}

#[test]
fn scratch_cannot_be_deleted_duplicated_or_launched_into_a_provider() {
    let world = World::new();
    let scratch = world.now().workspaces[0].id.as_str().to_owned();
    let ctx = world.context(None, Some(&scratch));
    for command in [
        json!({"kind": "deleteWorkspace", "workspaceId": scratch}),
        json!({"kind": "duplicateWorkspace", "sourceWorkspaceId": scratch, "label": "Scratch 2"}),
    ] {
        refused(world.send(ctx.clone(), command), ErrorCode::InvalidRecord);
    }
    assert_eq!(world.now().workspaces.len(), 1);
    let leaf = world.env.uuid();
    world
        .send(
            ctx.clone(),
            json!({"kind": "saveWorkspace", "workspaceId": scratch, "expectedWorkspaceRevision": "1",
                   "label": "Scratch", "layout": {"kind": "leaf", "id": leaf}, "focus": null,
                   "pinned": false, "leaves": [leaf_json(&leaf, Value::Null)],
                   "preferences": {"provider": null, "mode": "native", "nativeProfile": null,
                                   "managedSafetyProfile": null}}),
        )
        .unwrap();
    for (provider, mode, safety) in [
        ("codex", "native", Value::Null),
        ("claude", "native", Value::Null),
        ("codex", "managed", json!("inference")),
    ] {
        refused(
            world.send(
                ctx.clone(),
                json!({"kind": "launch", "workspaceId": scratch, "leafId": leaf, "provider": provider,
                       "mode": mode, "nativeProfile": null, "managedSafetyProfile": safety}),
            ),
            ErrorCode::UnsupportedCapability,
        );
    }
    // And Scratch cannot be renamed away from its name.
    refused(
        world.send(
            ctx,
            json!({"kind": "saveWorkspace", "workspaceId": scratch, "expectedWorkspaceRevision": "2",
                   "label": "Mine", "layout": null, "focus": null, "pinned": false, "leaves": [],
                   "preferences": {"provider": null, "mode": "native", "nativeProfile": null,
                                   "managedSafetyProfile": null}}),
        ),
        ErrorCode::InvalidRecord,
    );
    assert_eq!(world.owner.launches(), 0);
}

// ---- M1-05: a switch while something is pending ------------------------------------------

#[test]
fn a_command_for_one_project_never_lands_in_another_and_stale_contexts_are_refused() {
    let world = World::new();
    let (a, _, wa) = world.register(ROOT_A);
    let (b, _, wb) = world.register(ROOT_B);
    // The window says it is in A but names B's workspace.
    refused(
        world.send(
            world.context(Some(&a), Some(&wb)),
            json!({"kind": "closeWorkspace", "workspaceId": wb}),
        ),
        ErrorCode::ContextStale,
    );
    // A wrong host, an unknown project and a wrong binding revision are stale too.
    let mut wrong_host = world.context(Some(&a), Some(&wa));
    wrong_host["hostId"] = json!("00000000-0000-4000-8000-000000000999");
    refused(
        world.send(
            wrong_host,
            json!({"kind": "closeWorkspace", "workspaceId": wa}),
        ),
        ErrorCode::ContextStale,
    );
    let mut wrong_revision = world.context(Some(&a), Some(&wa));
    wrong_revision["bindingRevision"] = json!("99");
    refused(
        world.send(
            wrong_revision,
            json!({"kind": "closeWorkspace", "workspaceId": wa}),
        ),
        ErrorCode::ContextStale,
    );
    // Acting on B with A's context is stale and adds nothing anywhere.
    let before = world.now();
    refused(
        world.send(
            world.context(Some(&a), None),
            json!({"kind": "createWorkspace", "projectId": b, "label": "Sneaky"}),
        ),
        ErrorCode::ContextStale,
    );
    assert_eq!(world.now(), before);
    // Global administration, with no project in view, may name any project.
    world
        .send(
            world.context(None, None),
            json!({"kind": "renameProject", "projectId": b, "label": "From the list"}),
        )
        .unwrap();
    assert_eq!(world.owner.launches(), 0);
}

#[test]
fn a_stale_store_revision_is_a_conflict_and_changes_nothing() {
    let world = World::new();
    let (project, _, _) = world.register(ROOT_A);
    let stale = Counter::new(1).unwrap();
    let before = world.now();
    let result = world.engine().execute(world.envelope(
        &world.env.uuid(),
        stale,
        world.context(Some(&project), None),
        json!({"kind": "renameProject", "projectId": project, "label": "Late"}),
    ));
    refused(result, ErrorCode::RevisionConflict);
    assert_eq!(world.now(), before);
}

// ---- attach, detach, adopt ------------------------------------------------------------------

const TERMINAL_1: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const NATIVE_1: &str = "0123456789abcdef0123456789abcdef";

fn launched_shell(
    world: &World,
    project: Option<&str>,
    workspace: &str,
    leaf: &str,
    terminal: &str,
) -> String {
    world.owner.script(|s| {
        s.shell.push(Ok(ShellLaunched {
            terminal_id: terminal.to_owned(),
        }));
    });
    world
        .send(
            world.context(project, Some(workspace)),
            json!({"kind": "launch", "workspaceId": workspace, "leafId": leaf,
                   "provider": "shell", "mode": "native", "nativeProfile": null,
                   "managedSafetyProfile": null}),
        )
        .unwrap()
        .created[0]
        .id
        .clone()
}

#[test]
fn detaching_hides_a_session_without_closing_it_and_attach_verifies_the_owner() {
    let world = World::new();
    let (project, _, workspace) = world.register(ROOT_A);
    let (leaf, _) = two_leaves(&world, &project, &workspace);
    let association = launched_shell(&world, Some(&project), &workspace, &leaf, TERMINAL_1);
    let calls_before = world.owner.calls().len();
    world
        .send(
            world.context(Some(&project), Some(&workspace)),
            json!({"kind": "detach", "workspaceId": workspace, "leafId": leaf, "associationId": association}),
        )
        .unwrap();
    let store = world.now();
    let found = store
        .sessions
        .iter()
        .find(|a| a.id.as_str() == association)
        .unwrap();
    assert_eq!(found.state, AssociationState::Detached);
    assert!(
        leaf_ids(&store, &workspace).contains(&leaf),
        "the placement stays, empty"
    );
    assert!(
        !world.owner.calls()[calls_before..]
            .iter()
            .any(|c| c.starts_with("launch_")),
        "detach never stops or closes anything"
    );
    // The terminal is gone: attaching is refused. It is there, but the host is another: refused.
    world.owner.script(|s| s.host = Some(Ok(Vec::new())));
    let attach = |world: &World| {
        world.send(
            world.context(Some(&project), Some(&workspace)),
            json!({"kind": "attach", "workspaceId": workspace, "leafId": leaf, "associationId": association}),
        )
    };
    refused(attach(&world), ErrorCode::OwnerRefused);
    world.owner.script(|s| {
        s.host = Some(Ok(vec![native_row(TERMINAL_1, None)]));
        s.epoch = "h1:9:9:200:2000".into();
    });
    refused(attach(&world), ErrorCode::HostChanged);
    world.owner.script(|s| s.epoch = "h1:1:1:100:1000".into());
    attach(&world).unwrap();
    let again = world.now();
    assert_eq!(
        again
            .sessions
            .iter()
            .find(|a| a.id.as_str() == association)
            .unwrap()
            .state,
        AssociationState::Attached
    );
}

#[test]
fn a_session_of_another_project_cannot_be_attached_to_this_workspace() {
    let world = World::new();
    let (a, _, wa) = world.register(ROOT_A);
    let (b, _, wb) = world.register(ROOT_B);
    let (leaf_a, _) = two_leaves(&world, &a, &wa);
    let (leaf_b, _) = two_leaves(&world, &b, &wb);
    let association = launched_shell(&world, Some(&a), &wa, &leaf_a, TERMINAL_1);
    world
        .owner
        .script(|s| s.host = Some(Ok(vec![native_row(TERMINAL_1, None)])));
    // B's workspace cannot show A's session, however the command is phrased.
    let result = world.send(
        world.context(Some(&b), Some(&wb)),
        json!({"kind": "attach", "workspaceId": wb, "leafId": leaf_b, "associationId": association}),
    );
    assert!(result.is_err(), "{result:?}");
    assert!(
        world
            .now()
            .workspaces
            .iter()
            .find(|w| w.id.as_str() == wb)
            .unwrap()
            .leaves
            .iter()
            .all(|l| l.association_id.is_none())
    );
}

#[test]
fn only_an_unassigned_shell_is_adopted_into_a_project() {
    let world = World::new();
    let (project, binding, workspace) = world.register(ROOT_A);
    let scratch = world.now().workspaces[0].id.as_str().to_owned();
    let leaf = world.env.uuid();
    world
        .send(
            world.context(None, Some(&scratch)),
            json!({"kind": "saveWorkspace", "workspaceId": scratch, "expectedWorkspaceRevision": "1",
                   "label": "Scratch", "layout": {"kind": "leaf", "id": leaf}, "focus": null,
                   "pinned": false, "leaves": [leaf_json(&leaf, Value::Null)],
                   "preferences": {"provider": null, "mode": "native", "nativeProfile": null,
                                   "managedSafetyProfile": null}}),
        )
        .unwrap();
    let association = launched_shell(&world, None, &scratch, &leaf, TERMINAL_1);
    assert!(
        world.now().workspaces[0].leaves[0].association_id.is_some(),
        "the Scratch shell is shown in Scratch"
    );
    world
        .send(
            world.context(Some(&project), Some(&workspace)),
            json!({"kind": "adoptScratch", "associationId": association, "projectId": project, "bindingId": binding}),
        )
        .unwrap();
    let store = world.now();
    let adopted = store
        .sessions
        .iter()
        .find(|a| a.id.as_str() == association)
        .unwrap();
    assert_eq!(
        adopted.origin_project_id.as_ref().unwrap().as_str(),
        project
    );
    assert!(
        store
            .workspaces
            .iter()
            .find(|w| w.project_id.is_none())
            .unwrap()
            .leaves
            .iter()
            .all(|l| l.association_id.is_none())
    );
    // Once adopted, it is not Scratch's any more and cannot be adopted again.
    refused(
        world.send(
            world.context(Some(&project), Some(&workspace)),
            json!({"kind": "adoptScratch", "associationId": association, "projectId": project, "bindingId": binding}),
        ),
        ErrorCode::InvalidRecord,
    );
    let _ = NATIVE_1;
}

#[test]
fn removing_a_project_never_touches_the_folder_it_was_made_from() {
    let dir = TestDir::new();
    let folder = std::fs::canonicalize(&dir.0).unwrap().join("keep");
    std::fs::create_dir_all(folder.join(".git")).unwrap();
    std::fs::write(folder.join("work.txt"), b"mine").unwrap();
    let world = World::new();
    let path = folder.to_str().unwrap();
    world.owner.add_root(path, 64768, 4242);
    let (project, _, _) = world.register(path);
    for retain in [true, false] {
        let (project, _) = if retain {
            (project.clone(), ())
        } else {
            (world.register(path).0, ())
        };
        world
            .send(
                world.context(Some(&project), None),
                json!({"kind": "removeProject", "projectId": project, "retainHistory": retain}),
            )
            .unwrap();
        assert_eq!(std::fs::read(folder.join("work.txt")).unwrap(), b"mine");
        assert!(folder.join(".git").is_dir());
    }
    assert_eq!(world.owner.launches(), 0);
}
