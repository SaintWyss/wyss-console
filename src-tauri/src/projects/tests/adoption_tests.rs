//! Moving a Scratch shell into a project (contract §3, `adoptScratch`): the user is shown the
//! directory the shell is really in, read from its process, and the move goes through only with
//! that directory, still the shell's. What cannot be verified, or changed, moves nothing.
use super::*;
use wyss_console::contracts::m1::{
    records::Target,
    scalar::{AssociationId, ProjectId},
};

const TERMINAL: &str = "aaaaaaaa-0000-4000-8000-000000000001";

struct Shell {
    world: World,
    project: String,
    binding: String,
    workspace: String,
    association: String,
}

fn shell_in_scratch() -> Shell {
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
    world.owner.script(|s| {
        s.shell.push(Ok(ShellLaunched {
            terminal_id: TERMINAL.into(),
        }));
    });
    let association = world
        .send(
            world.context(None, Some(&scratch)),
            json!({"kind": "launch", "workspaceId": scratch, "leafId": leaf,
                   "provider": "shell", "mode": "native", "nativeProfile": null,
                   "managedSafetyProfile": null}),
        )
        .unwrap()
        .created[0]
        .id
        .clone();
    Shell {
        world,
        project,
        binding,
        workspace,
        association,
    }
}

impl Shell {
    fn cwd(&self, cwd: &str, pid: u32, ticks: u64) {
        self.world.owner.script(|s| {
            s.cwd.insert(
                TERMINAL.into(),
                Ok(ShellCwd {
                    cwd: cwd.to_owned(),
                    pid,
                    start_ticks: ticks,
                }),
            );
        });
    }

    fn show(&self, project: &str) -> Result<super::super::apply::AdoptionPreview, Failure> {
        self.world.engine().disclose_adoption(
            &AssociationId::parse(&self.association).unwrap(),
            &ProjectId::parse(project).unwrap(),
        )
    }

    fn adopt(&self) -> Result<Receipt, Failure> {
        self.world.send(
            self.world
                .context(Some(&self.project), Some(&self.workspace)),
            json!({"kind": "adoptScratch", "associationId": self.association,
                   "projectId": self.project, "bindingId": self.binding}),
        )
    }

    fn origin(&self) -> Option<String> {
        self.world
            .now()
            .sessions
            .iter()
            .find(|a| a.id.as_str() == self.association)
            .unwrap()
            .origin_project_id
            .as_ref()
            .map(|p| p.as_str().to_owned())
    }
}

#[test]
fn a_shell_is_adopted_only_with_the_directory_the_user_was_shown() {
    let shell = shell_in_scratch();
    shell.cwd("/home/fixture/work/a/src", 4242, 777);
    // Nothing was shown yet: nothing moves.
    let failure = refused(shell.adopt(), ErrorCode::ContextStale);
    assert_eq!(failure.field, Some("cwd"));
    assert_eq!(shell.origin(), None);
    let shown = shell.show(&shell.project).unwrap();
    assert_eq!(shown.cwd, "/home/fixture/work/a/src");
    assert_eq!(shown.binding_id.as_str(), shell.binding);
    assert!(shown.inside);
    shell.adopt().unwrap();
    assert_eq!(shell.origin(), Some(shell.project.clone()));
}

#[test]
fn a_directory_shown_is_used_once() {
    let shell = shell_in_scratch();
    shell.cwd("/home/fixture/work/a", 4242, 777);
    shell.show(&shell.project).unwrap();
    // The same disclosure cannot be spent twice: after a refusal the user is shown again.
    shell.cwd("/home/fixture/work/a/elsewhere", 4242, 777);
    refused(shell.adopt(), ErrorCode::ContextStale);
    refused(shell.adopt(), ErrorCode::ContextStale);
    assert_eq!(shell.origin(), None);
    shell.show(&shell.project).unwrap();
    shell.adopt().unwrap();
}

#[test]
fn a_shell_that_changed_directory_or_process_since_it_was_shown_moves_nothing() {
    for (cwd, pid, ticks) in [
        ("/home/fixture/work/b", 4242, 777),
        ("/home/fixture/work/a", 4243, 777),
        ("/home/fixture/work/a", 4242, 778),
    ] {
        let shell = shell_in_scratch();
        shell.cwd("/home/fixture/work/a", 4242, 777);
        shell.show(&shell.project).unwrap();
        shell.cwd(cwd, pid, ticks);
        let failure = refused(shell.adopt(), ErrorCode::ContextStale);
        assert_eq!(failure.field, Some("cwd"), "{cwd} {pid} {ticks}");
        assert_eq!(shell.origin(), None);
    }
}

#[test]
fn a_directory_that_cannot_be_read_is_never_guessed() {
    let shell = shell_in_scratch();
    // The host reports no readable process: nothing is shown, nothing moves.
    shell.world.owner.script(|s| {
        s.cwd.insert(TERMINAL.into(), Err(OwnerError::Refused));
    });
    let failure = refused(
        shell.show(&shell.project).map(|_| unreachable_receipt()),
        ErrorCode::OwnerRefused,
    );
    assert_eq!(failure.field, Some("cwd"));
    refused(shell.adopt(), ErrorCode::ContextStale);
    assert_eq!(shell.origin(), None);
    // It stops being readable after it was shown: the move is refused as well.
    shell.cwd("/home/fixture/work/a", 4242, 777);
    shell.show(&shell.project).unwrap();
    shell.world.owner.script(|s| {
        s.cwd.insert(TERMINAL.into(), Err(OwnerError::Unsupported));
    });
    refused(shell.adopt(), ErrorCode::OwnerRefused);
    assert_eq!(shell.origin(), None);
}

#[test]
fn a_replaced_host_is_refused_before_any_directory_is_read() {
    let shell = shell_in_scratch();
    shell.cwd("/home/fixture/work/a", 4242, 777);
    shell.world.owner.script(|s| s.epoch = "h1:9:9:2:2".into());
    refused(
        shell.show(&shell.project).map(|_| unreachable_receipt()),
        ErrorCode::HostChanged,
    );
    assert_eq!(shell.origin(), None);
}

#[test]
fn what_was_shown_for_one_project_does_not_move_a_shell_into_another() {
    let shell = shell_in_scratch();
    let other = shell.world.register(ROOT_B);
    shell.cwd("/home/fixture/work/b", 4242, 777);
    shell.show(&other.0).unwrap();
    // The window then asks for the first project instead.
    refused(shell.adopt(), ErrorCode::ContextStale);
    assert_eq!(shell.origin(), None);
}

#[test]
fn the_folder_comparison_follows_path_components_not_text() {
    let shell = shell_in_scratch();
    for (cwd, inside) in [
        ("/home/fixture/work/a", true),
        ("/home/fixture/work/a/deep/er", true),
        ("/home/fixture/work/ab", false),
        ("/home/fixture/work", false),
        ("/tmp", false),
    ] {
        shell.cwd(cwd, 4242, 777);
        assert_eq!(shell.show(&shell.project).unwrap().inside, inside, "{cwd}");
    }
}

#[test]
fn only_a_scratch_shell_has_a_directory_to_show() {
    let shell = shell_in_scratch();
    shell.cwd("/home/fixture/work/a", 4242, 777);
    shell.adopt_for_real();
    // Adopted: no longer Scratch's.
    refused(
        shell.show(&shell.project).map(|_| unreachable_receipt()),
        ErrorCode::InvalidRecord,
    );
    // A Native session is not a shell and has no directory to show.
    let store = shell.world.now();
    assert!(matches!(
        store
            .sessions
            .iter()
            .find(|a| a.id.as_str() == shell.association)
            .unwrap()
            .target,
        Target::Shell { .. }
    ));
}

impl Shell {
    fn adopt_for_real(&self) {
        self.show(&self.project).unwrap();
        self.adopt().unwrap();
    }
}

/// `refused` takes a command's result; a disclosure that was refused is checked the same way.
fn unreachable_receipt() -> Receipt {
    unreachable!("the disclosure succeeded where a refusal was expected")
}
