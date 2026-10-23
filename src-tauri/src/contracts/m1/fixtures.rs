//! Deterministic synthetic fixtures for the M1 store. Nothing here is read from a machine:
//! every ID, path and digest is built from a number, so a fixture never changes between runs.
//! Records are written as JSON, the wire form, so a test exercises the same entry point a
//! real store would use.
use serde_json::{Value, json};

pub const NATIVE_A: &str = "0123456789abcdef0123456789abcdef";
pub const NATIVE_B: &str = "fedcba9876543210fedcba9876543210";
pub const STAMP: &str = "2026-10-22T12:00:00Z";

/// A canonical UUID in a numbered family. Families keep kinds of record apart in a fixture.
pub fn id(family: u32, n: u32) -> String {
    format!("{family:08x}-0000-4000-8000-{n:012x}")
}

pub fn digest(n: u32) -> String {
    format!("{n:064x}")
}

pub fn host() -> String {
    id(1, 1)
}

pub fn profile(native: &str, version: &str) -> Value {
    json!({"id": native, "version": version})
}

pub fn project(n: u32, default_workspace: &str, default_binding: Value) -> Value {
    json!({
        "id": id(0x10, n), "label": format!("Project {n}"),
        "createdAt": STAMP, "updatedAt": STAMP, "revision": "1",
        "archived": false, "tombstone": false,
        "defaultBindingId": default_binding, "defaultWorkspaceId": default_workspace,
        "profileDefaults": {"claude": null, "codex": null}
    })
}

pub fn binding(n: u32, project: &str, previous: Value, state: &str) -> Value {
    json!({
        "id": id(0x20, n), "projectId": project, "hostId": host(), "revision": "1",
        "path": format!("/home/fixture/work/project-{n}"),
        "observation": {"device": "64768", "inode": format!("{}", 1000 + n)},
        "checkoutKind": "git", "state": state, "previousBindingId": previous
    })
}

pub fn preferences(mode: &str) -> Value {
    json!({"provider": null, "mode": mode, "nativeProfile": null, "managedSafetyProfile": null})
}

pub fn leaf(n: u32, association: Value) -> Value {
    json!({"id": id(0x40, n), "title": format!("Leaf {n}"), "associationId": association})
}

pub fn leaf_node(n: u32) -> Value {
    json!({"kind": "leaf", "id": id(0x40, n)})
}

pub fn split(a: Value, b: Value) -> Value {
    json!({"kind": "split", "dir": "row", "ratio": 0.5, "a": a, "b": b})
}

pub fn workspace(n: u32, project: Value, label: &str, layout: Value, leaves: Value) -> Value {
    json!({
        "id": id(0x30, n), "projectId": project, "hostId": host(), "revision": "1",
        "label": label, "layout": layout, "focus": null, "pinned": false,
        "leaves": leaves, "preferences": preferences("native")
    })
}

pub fn shell_association(n: u32) -> Value {
    json!({
        "id": id(0x50, n), "originProjectId": null, "hostId": host(),
        "bindingId": null, "bindingRevision": null,
        "target": {"kind": "shell", "terminalId": id(0x60, n), "hostEpoch": "epoch-1"},
        "state": "attached", "operationId": null
    })
}

/// The Managed authority of a receipt: its protocol and its stream.
pub fn authority(protocol: u8, n: u32) -> Value {
    json!({"protocol": protocol, "storeStreamId": id(0x80, n)})
}

#[allow(clippy::too_many_arguments)]
pub fn operation(
    n: u32,
    workspace: u32,
    leaf: u32,
    provider: &str,
    mode: &str,
    native_profile: Value,
    safety: Value,
    state: &str,
    association: Value,
    claim: Value,
) -> Value {
    json!({
        "id": id(0x70, n), "kind": "launch", "projectId": id(0x10, 1),
        "workspaceId": id(0x30, workspace), "leafId": id(0x40, leaf),
        "bindingId": id(0x20, 2), "bindingRevision": "3",
        "provider": provider, "mode": mode,
        "nativeProfile": native_profile, "managedSafetyProfile": safety,
        "managedAuthority": if mode == "managed" { authority(1, 1) } else { Value::Null },
        "state": state, "associationId": association, "claim": claim, "createdAt": STAMP
    })
}

/// Only the Scratch workspace: the smallest valid store.
pub fn scratch_only() -> Value {
    json!({
        "schema": 1, "storeId": id(2, 1), "revision": "1", "hostId": host(),
        "projects": [], "bindings": [],
        "workspaces": [workspace(1, Value::Null, "Scratch", Value::Null, json!([]))],
        "sessions": [], "operations": [], "closedOperations": [], "migration": null
    })
}

/// A project with a relinked binding, a tombstone, a Scratch shell and the three Native and
/// Managed session kinds, with launch receipts in every state.
pub fn full() -> Value {
    let project_a = id(0x10, 1);
    let mut a = project(1, &id(0x30, 2), json!(id(0x20, 2)));
    a["profileDefaults"]["claude"] = profile(NATIVE_B, "2");
    let mut b = project(2, "", Value::Null);
    b["tombstone"] = json!(true);
    b["defaultWorkspaceId"] = Value::Null;

    let mut scratch = workspace(
        1,
        Value::Null,
        "Scratch",
        split(leaf_node(1), leaf_node(2)),
        json!([leaf(1, json!(id(0x50, 1))), leaf(2, Value::Null)]),
    );
    scratch["focus"] = json!(id(0x40, 1));
    let mut main = workspace(
        2,
        json!(project_a),
        "Main",
        json!({"kind": "split", "dir": "col", "ratio": 0.5, "a": leaf_node(3),
               "b": {"kind": "split", "dir": "row", "ratio": 0.3, "a": leaf_node(4), "b": leaf_node(5)}}),
        json!([
            leaf(3, json!(id(0x50, 2))),
            leaf(4, json!(id(0x50, 3))),
            leaf(5, json!(id(0x50, 4)))
        ]),
    );
    main["preferences"] = json!({"provider": "codex", "mode": "managed", "nativeProfile": null,
                                 "managedSafetyProfile": "inference"});
    let spare = workspace(
        3,
        json!(project_a),
        "Spare",
        split(leaf_node(6), leaf_node(7)),
        json!([leaf(6, Value::Null), leaf(7, Value::Null)]),
    );

    let native = json!({
        "id": id(0x50, 2), "originProjectId": project_a, "hostId": host(),
        "bindingId": id(0x20, 2), "bindingRevision": "3",
        "target": {"kind": "native", "sessionId": NATIVE_A, "terminalId": id(0x60, 2), "hostEpoch": "epoch-1"},
        "state": "attached", "operationId": id(0x70, 1)
    });
    let p1 = json!({
        "id": id(0x50, 3), "originProjectId": project_a, "hostId": host(),
        "bindingId": id(0x20, 2), "bindingRevision": "3",
        "target": {"kind": "managed", "protocol": 1, "storeStreamId": id(0x80, 1),
                   "sessionId": id(0x90, 1), "requestedSafetyProfile": null},
        "state": "detached", "operationId": null
    });
    let p2 = json!({
        "id": id(0x50, 4), "originProjectId": project_a, "hostId": host(),
        "bindingId": id(0x20, 2), "bindingRevision": "3",
        "target": {"kind": "managed", "protocol": 2, "storeStreamId": id(0x80, 2),
                   "sessionId": id(0x90, 2), "requestedSafetyProfile": "local_tools"},
        "state": "attached", "operationId": id(0x70, 2)
    });

    // The committed Managed receipt was asked of the protocol-2 authority its association names.
    let mut second = operation(
        2,
        2,
        5,
        "codex",
        "managed",
        Value::Null,
        json!("local_tools"),
        "committed",
        json!(id(0x50, 4)),
        Value::Null,
    );
    second["managedAuthority"] = authority(2, 2);

    json!({
        "schema": 1, "storeId": id(2, 1), "revision": "7", "hostId": host(),
        "projects": [a, b],
        "bindings": [
            binding(1, &project_a, Value::Null, "retired"),
            binding(2, &project_a, json!(id(0x20, 1)), "valid")
        ],
        "workspaces": [scratch, main, spare],
        "sessions": [shell_association(1), native, p1, p2],
        "operations": [
            operation(1, 2, 3, "claude", "native", profile(NATIVE_B, "2"), Value::Null,
                      "committed", json!(id(0x50, 2)), Value::Null),
            second,
            operation(3, 3, 6, "codex", "managed", Value::Null, json!("inference"),
                      "pending", Value::Null, json!("claim-1")),
            operation(4, 3, 7, "codex", "native", Value::Null, Value::Null,
                      "ambiguous", Value::Null, Value::Null),
            operation(5, 2, 3, "claude", "managed", Value::Null, json!("inference"),
                      "refused", Value::Null, Value::Null)
        ],
        "closedOperations": [id(0x70, 99)],
        "migration": {
            "version": 1, "id": id(0xa0, 1),
            "sources": [{"kind": "registry", "digest": digest(1)}, {"kind": "layout", "digest": digest(2)}],
            "map": [
                {"sourceKind": "registry_project", "sourceKey": digest(3), "targetId": project_a},
                {"sourceKind": "registry_project", "sourceKey": digest(4), "targetId": project_a},
                {"sourceKind": "registry_session", "sourceKey": digest(5), "targetId": id(0x50, 2)},
                {"sourceKind": "layout_leaf", "sourceKey": digest(6), "targetId": id(0x40, 3)}
            ],
            "state": "committed"
        }
    })
}

/// A store with `count` live projects, each with one workspace, besides Scratch.
pub fn with_projects(count: u32) -> Value {
    let mut store = scratch_only();
    for n in 1..=count {
        let workspace_n = n + 100;
        store["projects"].as_array_mut().unwrap().push(project(
            n,
            &id(0x30, workspace_n),
            Value::Null,
        ));
        store["workspaces"].as_array_mut().unwrap().push(workspace(
            workspace_n,
            json!(id(0x10, n)),
            "Main",
            Value::Null,
            json!([]),
        ));
    }
    store
}

/// A balanced layout of `count` leaves numbered from `first`, with its leaf records.
pub fn balanced(first: u32, count: u32) -> (Value, Vec<Value>) {
    fn node(first: u32, count: u32) -> Value {
        if count == 1 {
            leaf_node(first)
        } else {
            let half = count / 2;
            split(node(first, half), node(first + half, count - half))
        }
    }
    let leaves = (first..first + count)
        .map(|n| leaf(n, Value::Null))
        .collect();
    (node(first, count), leaves)
}

/// A left-leaning chain of splits whose deepest leaves sit at `depth` (root is level 0).
pub fn chain(depth: u32) -> (Value, Vec<Value>) {
    let mut node = leaf_node(1000);
    let mut leaves = vec![leaf(1000, Value::Null)];
    for level in 1..=depth {
        node = split(node, leaf_node(1000 + level));
        leaves.push(leaf(1000 + level, Value::Null));
    }
    (node, leaves)
}
