//! M1-13 — the world the graphical measurements run in. Not a test of anything: with
//! `WYSS_M1_WORLD` naming a directory made for it, and a disposable host already listening at
//! `<dir>/run/wyss-console/host.sock`, it makes the folders, the projects, the workspaces and the
//! real shells of one of three shapes, through the very commands the window sends, and writes
//! `<dir>/world.json` for the measuring script. Without the variable it does nothing; it is
//! ignored by default and never touches a store or a host that is not named.
//!
//! * `a`: 1 terminal, 2 workspaces (Scratch and one project's)
//! * `b`: 16 terminals, 32 workspaces (16 projects with a shell each, Scratch, 15 more)
//! * `c`: 16 terminals in one workspace (a 4×4 grid), 2 workspaces
use super::*;
use crate::projects::{
    apply::SystemEnv,
    claim::Claims,
    owner::{HostSocket, NoManaged, Owner, RealOwner},
    service::ProjectsService,
};

struct World {
    service: ProjectsService,
    owner: Arc<RealOwner<NoManaged>>,
    epoch: String,
    next: u64,
}

impl World {
    fn send(&mut self, project: Option<&str>, workspace: Option<&str>, command: Value) -> Receipt {
        let store = self.service.overview().store;
        let (revision, host) = match serde_json::to_value(&store).unwrap() {
            Value::Object(o) => (
                o["revision"].as_str().unwrap().to_owned(),
                o["hostId"].as_str().unwrap().to_owned(),
            ),
            other => panic!("{other}"),
        };
        let root = project.map(|p| {
            let view = serde_json::to_value(self.service.get_project(p).unwrap()).unwrap();
            (
                view["root"]["bindingId"].as_str().unwrap().to_owned(),
                view["root"]["bindingRevision"].as_str().unwrap().to_owned(),
            )
        });
        self.next += 1;
        self.service
            .execute(
                serde_json::from_value(json!({
                    "schema": 1,
                    "operationId": format!("{:08x}-0000-4000-8000-{:012x}", 0xfeed, self.next),
                    "expectedStoreRevision": revision,
                    "context": {
                        "hostId": host, "projectId": project, "workspaceId": workspace,
                        "bindingId": root.as_ref().map(|r| r.0.clone()),
                        "bindingRevision": root.as_ref().map(|r| r.1.clone()),
                        "selectionRevision": "1",
                    },
                    "command": command,
                }))
                .unwrap(),
            )
            .unwrap_or_else(|failure| panic!("{failure:?}"))
    }

    fn id(&mut self) -> String {
        self.next += 1;
        format!("{:08x}-0000-4000-8000-{:012x}", 0xbeef, self.next)
    }
}

/// A balanced layout for `leaves`, alternating the direction at each level.
fn layout(leaves: &[String], row: bool) -> Value {
    match leaves {
        [one] => json!({"kind": "leaf", "id": one}),
        _ => {
            let (a, b) = leaves.split_at(leaves.len() / 2);
            json!({"kind": "split", "dir": if row {"row"} else {"col"}, "ratio": 0.5,
                   "a": layout(a, !row), "b": layout(b, !row)})
        }
    }
}

#[test]
#[ignore = "builds the world of the graphical measurements; needs WYSS_M1_WORLD and a host"]
fn build_the_world_of_the_measurements() {
    let Some(root) = std::env::var_os("WYSS_M1_WORLD").map(PathBuf::from) else {
        return;
    };
    let shape = std::env::var("WYSS_M1_SHAPE").unwrap_or_else(|_| "a".into());
    let socket = root.join("run/wyss-console/host.sock");
    let owner = Arc::new(RealOwner::new(
        Some(HostSocket::new(socket.clone())),
        NoManaged,
    ));
    let epoch = HostSocket::new(socket).epoch().expect("a host listening");
    let service = ProjectsService::new(
        Some(root.join("state/wyss-console/projects/v1")),
        owner.clone(),
        Arc::new(SystemEnv::new()),
        Claims::new(None),
    );
    service.set_enabled(true).unwrap();
    service.initialize_empty().unwrap();
    let mut world = World {
        service,
        owner,
        epoch,
        next: 0,
    };

    let (projects, panes, extra) = match shape.as_str() {
        "a" => (1, 1, 0),
        "b" => (16, 1, 15),
        "c" => (1, 16, 0),
        other => panic!("unknown shape {other}"),
    };
    let mut made = Vec::new();
    for n in 1..=projects {
        let folder = root.join(format!("work/project-{n:02}"));
        std::fs::create_dir_all(folder.join(".git")).unwrap();
        let canonical = std::fs::canonicalize(&folder).unwrap();
        let pick = world
            .service
            .issue_pick(canonical.to_str().unwrap(), Purpose::Register, 1)
            .unwrap();
        let receipt = world.send(
            None,
            None,
            json!({"kind": "registerExisting", "selectionToken": pick.token}),
        );
        let role = |name: &str| {
            receipt
                .created
                .iter()
                .find(|c| c.role == name)
                .unwrap()
                .id
                .clone()
        };
        let (project, workspace) = (role("project"), role("workspace"));
        let leaves: Vec<String> = (0..panes).map(|_| world.id()).collect();
        let revision = {
            let view = serde_json::to_value(world.service.get_workspace(&workspace, None).unwrap())
                .unwrap();
            view["revision"].as_str().unwrap().to_owned()
        };
        world.send(
            Some(&project),
            Some(&workspace),
            json!({"kind": "saveWorkspace", "workspaceId": workspace,
                   "expectedWorkspaceRevision": revision, "label": format!("Project {n}"),
                   "layout": layout(&leaves, true), "focus": leaves[0], "pinned": false,
                   "leaves": leaves.iter().map(|l| json!({"id": l, "title": "Pane", "associationId": null})).collect::<Vec<_>>(),
                   "preferences": {"provider": null, "mode": "native", "nativeProfile": null,
                                   "managedSafetyProfile": null}}),
        );
        let mut terminals = Vec::new();
        for (k, leaf) in leaves.iter().enumerate() {
            let launched = world.send(
                Some(&project),
                Some(&workspace),
                json!({"kind": "launch", "workspaceId": workspace, "leafId": leaf,
                       "provider": "shell", "mode": "native", "nativeProfile": null,
                       "managedSafetyProfile": null}),
            );
            let association = launched
                .created
                .iter()
                .find(|c| c.role == "association")
                .unwrap()
                .id
                .clone();
            terminals.push((k, leaf.clone(), association));
        }
        made.push((n, project, workspace, folder, terminals));
    }
    // 15 more workspaces (shape b) so that the store holds 32 with Scratch.
    for n in 0..extra {
        let (_, project, _, _, _) = &made[n % made.len()];
        world.send(
            Some(project),
            None,
            json!({"kind": "createWorkspace", "projectId": project, "label": format!("Extra {n}")}),
        );
    }
    // Each shell is told apart on screen: it clears itself and prints a mark.
    let listed = world.service.list_sessions(None, None, None).unwrap();
    let mut terminal_of = std::collections::HashMap::new();
    for item in listed.items {
        let v = serde_json::to_value(&item).unwrap();
        terminal_of.insert(
            v["id"].as_str().unwrap().to_owned(),
            v["terminalId"].as_str().unwrap().to_owned(),
        );
    }
    let mut out = Vec::new();
    for (n, project, workspace, folder, terminals) in &made {
        let mut ids = Vec::new();
        for (k, leaf, association) in terminals {
            let terminal = &terminal_of[association];
            // A bar as wide as five columns a project number: each project's picture is far from
            // its neighbour's, so a picture can be told from another by what is on the screen.
            let bar = "█".repeat(n * 5);
            let text = format!(
                "printf '\\033c'; echo M1-PERF-{n:02}-{k:02}; seq 1 8; echo {bar}; echo {bar}; echo {bar}\r"
            );
            world
                .owner
                .term_write(&world.epoch, terminal, text.as_bytes())
                .unwrap();
            ids.push(
                json!({"pane": k, "leaf": leaf, "association": association, "terminal": terminal}),
            );
        }
        out.push(json!({"n": n, "project": project, "workspace": workspace,
                        "folder": folder, "terminals": ids}));
    }
    std::fs::write(
        root.join("world.json"),
        serde_json::to_vec_pretty(&json!({"shape": shape, "projects": out})).unwrap(),
    )
    .unwrap();
}
