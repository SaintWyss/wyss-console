//! M1-13: measured costs of the backend, not budgets. Run on request:
//!
//! `cargo test --bin wyss-console projects::tests::bench_tests -- --ignored --nocapture`
//!
//! Each scenario prints one `M1-13` line with raw numbers (p50/p95/p99 in microseconds over many
//! samples, store bytes, resident memory, thread count). Three runs are meant to be kept with
//! their outliers. A fixture-only figure proves nothing about the real desktop; it only shows what
//! this code costs when everything else is free.
use super::*;
use crate::projects::{
    claim::Claims,
    owner::{HostSocket, NoManaged, RealOwner},
    service::ProjectsService,
};
use std::time::{Duration, Instant};
use wyss_console::contracts::m1::records::{
    Association, AssociationState, Leaf, Node, Target, Workspace,
};
use wyss_console::contracts::m1::scalar::{
    AssociationId, EpochText, HostId, LeafId, TerminalId, Text, WorkspaceId,
};

fn percentile(sorted: &[u128], p: f64) -> u128 {
    sorted[((sorted.len() as f64 - 1.0) * p).round() as usize]
}

fn measure(samples: usize, mut work: impl FnMut()) -> (u128, u128, u128) {
    let mut times: Vec<u128> = (0..samples)
        .map(|_| {
            let start = Instant::now();
            work();
            start.elapsed().as_micros()
        })
        .collect();
    times.sort_unstable();
    (
        percentile(&times, 0.5),
        percentile(&times, 0.95),
        percentile(&times, 0.99),
    )
}

fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

fn threads() -> usize {
    std::fs::read_dir("/proc/self/task")
        .map(|d| d.count())
        .unwrap_or(0)
}

fn uuid(family: u32, n: u32) -> String {
    format!("{family:08x}-0000-4000-8000-{n:012x}")
}

/// A store with `workspaces` workspaces in one project, each with `leaves` panes holding a shell.
fn store_with(
    workspaces: u32,
    leaves: u32,
    dir: &TestDir,
) -> (ProjectsService, String, Vec<String>, Arc<FakeOwner>) {
    let owner = FakeOwner::new();
    owner.add_root(ROOT_A, 64768, 1001);
    let env = Arc::new(FakeEnv::new());
    let service = ProjectsService::new(
        Some(dir.0.join("state/wyss-console/projects/v1")),
        owner.clone(),
        env.clone(),
        Claims::new(Some(dir.0.join("claims"))),
    );
    service.set_enabled(true).unwrap();
    service.initialize_empty().unwrap();
    let pick = service.issue_pick(ROOT_A, Purpose::Register, 1).unwrap();
    let store = Store::open(dir.0.join("state/wyss-console/projects/v1")).unwrap();
    let current = match store.read().unwrap() {
        Loaded::Ready(s) => *s,
        other => panic!("{other:?}"),
    };
    let ctx = json!({"hostId": current.host_id.as_str(), "projectId": null, "workspaceId": null,
        "bindingId": null, "bindingRevision": null, "selectionRevision": "1"});
    service
        .execute(
            serde_json::from_value(json!({"schema": 1, "operationId": env.uuid(),
            "expectedStoreRevision": current.revision.get().to_string(), "context": ctx,
            "command": {"kind": "registerExisting", "selectionToken": pick.token}}))
            .unwrap(),
        )
        .unwrap();
    let current = match store.read().unwrap() {
        Loaded::Ready(s) => *s,
        other => panic!("{other:?}"),
    };
    let project = current.projects[0].id.clone();
    let mut ids = Vec::new();
    let mut terminals = Vec::new();
    let host: HostId = current.host_id.clone();
    store
        .transact(current.revision, |s| {
            let mut n = 0u32;
            for w in 0..workspaces {
                let workspace_id = WorkspaceId::parse(&uuid(0x30, 1000 + w)).unwrap();
                let mut leaf_nodes = Vec::new();
                let mut records = Vec::new();
                for l in 0..leaves {
                    n += 1;
                    let leaf = LeafId::parse(&uuid(0x40, n)).unwrap();
                    let association = AssociationId::parse(&uuid(0x50, n)).unwrap();
                    let terminal = uuid(0x60, n);
                    terminals.push(terminal.clone());
                    s.sessions.push(Association {
                        id: association.clone(),
                        origin_project_id: Some(project.clone()),
                        host_id: host.clone(),
                        binding_id: None,
                        binding_revision: None,
                        target: Target::Shell {
                            terminal_id: TerminalId::parse(&terminal).unwrap(),
                            host_epoch: EpochText::parse("h1:1:1:100:1000").unwrap(),
                        },
                        state: AssociationState::Attached,
                        operation_id: None,
                    });
                    records.push(Leaf {
                        id: leaf.clone(),
                        title: Text::parse(&format!("Pane {l}")).unwrap(),
                        association_id: Some(association),
                    });
                    leaf_nodes.push(Node::Leaf { id: leaf });
                }
                fn tree(mut nodes: Vec<Node>) -> Node {
                    if nodes.len() == 1 {
                        return nodes.remove(0);
                    }
                    let right = nodes.split_off(nodes.len() / 2);
                    Node::Split {
                        dir: wyss_console::contracts::m1::records::SplitDir::Row,
                        ratio: 0.5,
                        a: Box::new(tree(nodes)),
                        b: Box::new(tree(right)),
                    }
                }
                let mut workspace = s
                    .workspaces
                    .iter()
                    .find(|x| x.project_id.is_some())
                    .cloned()
                    .unwrap();
                workspace.id = workspace_id.clone();
                workspace.label = Text::parse(&format!("Workspace {w}")).unwrap();
                workspace.layout = Some(tree(leaf_nodes));
                workspace.leaves = records;
                if w == 0 {
                    // The registered default is replaced by the first generated one.
                    s.workspaces.retain(|x| x.project_id.is_none());
                    s.projects[0].default_workspace_id = Some(workspace_id.clone());
                }
                let w: Workspace = workspace;
                ids.push(w.id.as_str().to_owned());
                s.workspaces.push(w);
            }
            Ok::<(), ()>(())
        })
        .unwrap()
        .unwrap();
    // Scratch again, since the generator replaced the registered default and Scratch is needed.
    (service, project.as_str().to_owned(), ids, owner)
}

#[test]
#[ignore = "a measurement, run on request"]
fn m1_13_backend_costs() {
    for (label, workspaces, leaves, live) in [
        ("1 terminal / 2 workspaces", 2, 1, 1u32),
        ("16 terminals / 32 workspaces", 32, 16, 16),
    ] {
        let dir = TestDir::new();
        let threads_before = threads();
        let rss_before = rss_kib();
        let (service, project, ids, owner) = store_with(workspaces, leaves, &dir);
        let _ = (&owner, live);
        let bytes = std::fs::metadata(dir.0.join("state/wyss-console/projects/v1/store.json"))
            .map(|m| m.len())
            .unwrap_or(0);
        owner.script(|s| {
            s.host = Some(Ok((0..live)
                .map(|n| native_row(&uuid(0x60, n + 1), None))
                .collect()));
        });
        let load = measure(200, || {
            service.list_projects(None).unwrap();
        });
        let switch = measure(200, || {
            for id in ids.iter().take(2) {
                service.get_workspace(id, Some("w1")).unwrap();
            }
        });
        let sessions = measure(100, || {
            service.list_sessions(None, None, Some("w1")).unwrap();
        });
        let overview = measure(200, || {
            let _ = service.overview();
        });
        let _ = project;
        println!(
            "M1-13 {{\"scenario\":\"{label}\",\"storeBytes\":{bytes},\"listProjectsUs\":{load:?},\"twoWorkspaceViewsUs\":{switch:?},\"listSessionsUs\":{sessions:?},\"overviewUs\":{overview:?},\"rssKibDelta\":{},\"threadsDelta\":{}}}",
            rss_kib() as i64 - rss_before as i64,
            threads() as i64 - threads_before as i64
        );
    }
}

#[test]
#[ignore = "a measurement, run on request"]
fn m1_13_real_host_read_cost() {
    // The cost that dominates a view: reading a real host with 1 and with 16 real terminals.
    let dir = TestDir::new();
    let run = dir.0.join("run");
    crate::native::store::private_dir(&run).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "projects::tests::process_tests::child_real_host",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("WYSS_M1_REAL_HOST", "1")
        .env("XDG_RUNTIME_DIR", &run)
        .env("XDG_STATE_HOME", dir.0.join("hs"))
        .env("HOME", dir.0.join("home"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let socket = run.join("wyss-console").join("host.sock");
    let deadline = Instant::now() + Duration::from_secs(10);
    while HostSocket::new(socket.clone()).epoch().is_err() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    let owner = RealOwner::new(Some(HostSocket::new(socket.clone())), NoManaged);
    let epoch = owner.host(None).unwrap().epoch;
    for target in [1usize, 16] {
        while owner.host(None).unwrap().terminals.len() < target {
            owner.launch_shell("/tmp", &epoch).unwrap();
        }
        let read = measure(200, || {
            owner.host(Some(&epoch)).unwrap();
        });
        println!(
            "M1-13 {{\"scenario\":\"real host read, {target} terminals\",\"hostReadUs\":{read:?}}}"
        );
    }
    let _ = child.kill();
    let _ = child.wait();
}
