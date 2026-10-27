use super::*;
use std::{ffi::OsString, os::unix::fs::PermissionsExt};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let root = fs::canonicalize(env::temp_dir())
            .unwrap()
            .join(format!("wyss-isolation-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        Self(root)
    }
    fn dir(&self, relative: &str) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(&path).unwrap();
        path
    }
    fn file(&self, relative: &str, content: &str, mode: u32) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        path
    }
    /// A private empty directory, as the authority keeps for the runtime to start in.
    fn place(&self, relative: &str) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        path
    }
    /// A repository with a nested workspace, a user Codex home with a credential, and
    /// the isolation that serves that workspace.
    fn layout(&self) -> (Isolation, PathBuf, PathBuf) {
        self.dir("repo/.git");
        let workspace = self.dir("repo/sub/ws");
        self.file("user/auth.json", "FIXTURE-CREDENTIAL", 0o600);
        self.file("user/config.toml", "model = \"private\"", 0o600);
        let executable = self.file("bin/codex", "#!/bin/sh\n", 0o700);
        let isolation = Isolation {
            workspace,
            state: self.0.join("state"),
            control: Vec::new(),
        };
        (isolation, self.0.join("user"), executable)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn argv(command: &Command) -> Vec<String> {
    command
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}
fn text(path: &Path) -> String {
    path.to_str().unwrap().to_owned()
}
/// The working directory the helpers launch with: a private empty directory at the root
/// of the test's own tree, next to its repository, user home and private state.
fn place_of(i: &Isolation) -> PathBuf {
    let root = i
        .state
        .ancestors()
        .find(|a| {
            a.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("wyss-isolation-"))
        })
        .unwrap();
    let path = root.join("runtime-cwd");
    let _ = fs::DirBuilder::new().mode(0o700).create(&path);
    path
}
fn launch(i: &Isolation, executable: &Path, user: &Path) -> ProbeResult<Command> {
    let masked = i.layers()?;
    let credential = credential(user)?;
    i.launch(
        executable,
        vec!["-c".into(), "a=b".into(), "app-server".into()],
        &place_of(i),
        &Grants {
            masked: &masked,
            catalog: None,
            credential: credential.as_ref(),
        },
        user,
        None,
        &[],
    )
}
/// The argv with the one run-specific value, the credential descriptor number, replaced.
fn stable(mut args: Vec<String>) -> Vec<String> {
    if let Some(at) = args.iter().position(|a| a == "--bind-fd") {
        let fd: i32 = args[at + 1].parse().expect("descriptor number");
        assert!(fd > 2, "never a standard stream: {fd}");
        args[at + 1] = "<credential>".into();
    }
    args
}
/// Runs `script` with `/bin/sh` inside the real namespace and returns what it printed.
fn inside(
    i: &Isolation,
    user: &Path,
    cwd: &Path,
    grants: &Grants<'_>,
    script: &str,
    environment: &[(&str, &Path)],
) -> String {
    let mut command = i
        .launch(
            Path::new("/bin/sh"),
            vec!["-c".into(), script.into()],
            cwd,
            grants,
            user,
            None,
            &[],
        )
        .unwrap();
    for (key, value) in environment {
        command.env(key, value);
    }
    let output = command.stdin(std::process::Stdio::null()).output().unwrap();
    assert!(
        output.status.success(),
        "bwrap must be able to create the namespace: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn launch_is_one_exact_shell_free_argv_with_private_policy_masks_and_one_credential() {
    let t = Temp::new();
    let (i, user, executable) = t.layout();
    let root_layer = t.dir("repo/.codex");
    let middle_layer = t.dir("repo/sub/.codex");
    t.file(
        "repo/.codex/config.toml",
        "approval_policy = \"never\"",
        0o644,
    );
    assert_eq!(
        i.layers().unwrap(),
        [root_layer.clone(), middle_layer.clone()]
    );
    assert_eq!(
        i.trust_roots().unwrap(),
        [text(&t.0.join("repo")), text(&i.workspace)]
    );
    let inherited: Vec<OsString> = [
        "CODEX_HOME",
        "CODEX_ROLLOUT_TRACE_ROOT",
        "OPENAI_API_KEY",
        "PATH",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    let held = credential(&user).unwrap();
    let cwd = place_of(&i);
    let command = i
        .launch(
            &executable,
            vec!["-c".into(), "a=b".into(), "app-server".into()],
            &cwd,
            &Grants {
                masked: &i.layers().unwrap(),
                catalog: None,
                credential: held.as_ref(),
            },
            &user,
            None,
            &inherited,
        )
        .unwrap();
    assert_eq!(command.get_program(), "/usr/bin/bwrap");
    let state = &i.state;
    assert_eq!(
        stable(argv(&command)),
        [
            "--die-with-parent".to_owned(),
            "--dev-bind".into(),
            "/".into(),
            "/".into(),
            "--overlay-src".into(),
            "/etc".into(),
            "--tmp-overlay".into(),
            "/etc".into(),
            "--ro-bind".into(),
            text(&state.join("etc")),
            "/etc/codex".into(),
            "--tmpfs".into(),
            text(&user),
            "--ro-bind".into(),
            text(&state.join("empty")),
            text(&root_layer),
            "--ro-bind".into(),
            text(&state.join("empty")),
            text(&middle_layer),
            "--bind-fd".into(),
            "<credential>".into(),
            text(&state.join("home/auth.json")),
            "--perms".into(),
            "0700".into(),
            "--tmpfs".into(),
            text(&cwd),
            "--remount-ro".into(),
            text(&cwd),
            "--chdir".into(),
            text(&cwd),
            "--".into(),
            text(&executable),
            "-c".into(),
            "a=b".into(),
            "app-server".into(),
        ]
    );
    // The credential path itself is never part of the launch: only its open descriptor.
    assert!(!argv(&command).contains(&text(&user.join("auth.json"))));
    let environment: Vec<_> = command
        .get_envs()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.map(|v| v.to_os_string()),
            )
        })
        .collect();
    assert_eq!(
        environment,
        [
            ("CODEX_HOME".to_owned(), Some(i.home().into_os_string())),
            ("CODEX_ROLLOUT_TRACE_ROOT".to_owned(), None),
            ("OPENAI_API_KEY".to_owned(), None),
        ]
    );
    assert_eq!(
        fs::read_to_string(state.join("etc/requirements.toml")).unwrap(),
        "[features]\nunified_exec = false\n"
    );
    for dir in [
        state.clone(),
        i.home(),
        state.join("etc"),
        state.join("empty"),
    ] {
        assert_eq!(mode(&dir), 0o700, "{dir:?}");
    }
    for file in [
        "etc/requirements.toml",
        "etc/managed_config.toml",
        "home/auth.json",
    ] {
        assert_eq!(mode(&state.join(file)), 0o600, "{file}");
    }
    // The credential is never copied: the placeholder is an empty mount point.
    assert_eq!(fs::metadata(state.join("home/auth.json")).unwrap().len(), 0);
    assert_eq!(fs::read_dir(state.join("empty")).unwrap().count(), 0);
    // A second launch of the same generation is identical.
    assert_eq!(
        stable(argv(&launch(&i, &executable, &user).unwrap())),
        stable(argv(&command))
    );
}
#[test]
fn only_the_pinned_release_returns_when_the_executable_lives_in_the_hidden_home() {
    let t = Temp::new();
    let (i, user, _) = t.layout();
    let inside = t.file("user/packages/release-1/bin/codex", "#!/bin/sh\n", 0o700);
    let args = argv(&launch(&i, &inside, &user).unwrap());
    let release = text(&user.join("packages/release-1"));
    let at = args.iter().position(|a| *a == "--tmpfs").unwrap();
    assert_eq!(
        args[at..at + 5],
        [
            "--tmpfs".to_owned(),
            text(&user),
            "--ro-bind".into(),
            release.clone(),
            release
        ]
    );
    // An executable that would require exposing the whole hidden home is refused.
    let bare = t.file("user/codex", "#!/bin/sh\n", 0o700);
    assert!(launch(&i, &bare, &user).is_err());
    assert!(launch(&i, &user.join("missing"), &user).is_err());
}
#[test]
fn the_project_chain_starts_at_the_outermost_marker_and_every_candidate_root_is_untrusted() {
    let t = Temp::new();
    // These cases own every marker below the temporary directory; one above it would be
    // part of every chain, exactly as it would be for a real workspace there.
    assert!(
        t.0.ancestors()
            .skip(1)
            .all(|dir| fs::symlink_metadata(dir.join(".git")).is_err()),
        "the system temporary directory lies inside a repository"
    );
    let workspace = t.dir("plain/a/b");
    let i = Isolation {
        workspace: workspace.clone(),
        state: t.0.join("state"),
        control: Vec::new(),
    };
    // No marker at all: the workspace alone, whatever its ancestors hold.
    t.dir("plain/.codex");
    assert_eq!(i.project_dirs().unwrap(), std::slice::from_ref(&workspace));
    assert_eq!(i.trust_roots().unwrap(), [text(&workspace)]);
    assert!(i.layers().unwrap().is_empty());
    t.dir("plain/.git");
    t.dir("above-plain-is-ignored/.codex");
    assert_eq!(
        i.trust_roots().unwrap(),
        [text(&t.0.join("plain")), text(&workspace)]
    );
    assert_eq!(i.layers().unwrap(), [t.0.join("plain/.codex")]);
    t.dir("plain/a/b/.codex");
    assert_eq!(
        i.layers().unwrap(),
        [t.0.join("plain/.codex"), workspace.join(".codex")]
    );
    // A nearer marker adds a candidate root and removes nothing: the layers of the outer
    // repository stay masked whichever one the runtime takes for the project root.
    t.dir("plain/a/.git");
    assert_eq!(
        i.trust_roots().unwrap(),
        [
            text(&t.0.join("plain")),
            text(&t.0.join("plain/a")),
            text(&workspace)
        ]
    );
    t.dir("plain/a/.codex");
    assert_eq!(
        i.layers().unwrap(),
        [
            t.0.join("plain/.codex"),
            t.0.join("plain/a/.codex"),
            workspace.join(".codex")
        ]
    );
    // So does a marker in the workspace itself, which is what an interrupted sandbox of
    // the runtime leaves there, and a marker that is a file, as in a linked worktree.
    t.dir("plain/a/b/.git");
    assert_eq!(i.project_dirs().unwrap().len(), 3);
    assert_eq!(i.layers().unwrap().len(), 3);
    assert_eq!(i.trust_roots().unwrap().len(), 3);
    fs::remove_dir(t.0.join("plain/a/.git")).unwrap();
    t.file("plain/a/.git", "gitdir: elsewhere", 0o644);
    assert_eq!(i.trust_roots().unwrap().len(), 3);
    // More candidate roots than a thread can be told about keeps the profile off.
    let deep = t.dir("nested/1/2/3/4/5/6/7/8");
    for depth in [
        "nested",
        "nested/1",
        "nested/1/2",
        "nested/1/2/3",
        "nested/1/2/3/4",
    ] {
        t.dir(&format!("{depth}/.git"));
    }
    for depth in [
        "nested/1/2/3/4/5",
        "nested/1/2/3/4/5/6",
        "nested/1/2/3/4/5/6/7",
    ] {
        t.dir(&format!("{depth}/.git"));
    }
    let nested = Isolation {
        workspace: deep,
        state: t.0.join("state"),
        control: Vec::new(),
    };
    assert_eq!(nested.project_dirs().unwrap().len(), 9);
    assert!(nested.trust_roots().is_err());
    fs::remove_dir(t.0.join("nested/1/2/3/4/5/6/7/.git")).unwrap();
    assert_eq!(nested.trust_roots().unwrap().len(), safety::MAX_TRUST_ROOTS);
}
#[test]
fn every_unsafe_input_keeps_the_profile_off() {
    // Credential: absent means no capability; anything but a private regular file is refused.
    let t = Temp::new();
    let (i, user, executable) = t.layout();
    fs::remove_file(user.join("auth.json")).unwrap();
    assert!(!argv(&launch(&i, &executable, &user).unwrap()).contains(&"--bind-fd".to_owned()));
    assert!(credential(&t.0.join("no-such-home")).unwrap().is_none());
    t.file("user/auth.json", "x", 0o640);
    assert!(launch(&i, &executable, &user).is_err());
    fs::remove_file(user.join("auth.json")).unwrap();
    let elsewhere = t.file("elsewhere.json", "x", 0o600);
    std::os::unix::fs::symlink(&elsewhere, user.join("auth.json")).unwrap();
    assert!(launch(&i, &executable, &user).is_err());
    // A second name for the credential may live anywhere: it is not the user's one file.
    fs::remove_file(user.join("auth.json")).unwrap();
    t.file("user/auth.json", "x", 0o600);
    assert!(launch(&i, &executable, &user).is_ok());
    fs::hard_link(user.join("auth.json"), t.0.join("second-name")).unwrap();
    assert!(launch(&i, &executable, &user).is_err());
    fs::remove_file(t.0.join("second-name")).unwrap();
    // A directory or a device under the credential's name is not a credential.
    fs::remove_file(user.join("auth.json")).unwrap();
    fs::create_dir(user.join("auth.json")).unwrap();
    assert!(launch(&i, &executable, &user).is_err());
    fs::remove_dir(user.join("auth.json")).unwrap();
    t.file("user/auth.json", "x", 0o600);
    // A home another user could rewrite cannot vouch for the file it names.
    for open in [0o775, 0o757] {
        fs::set_permissions(&user, fs::Permissions::from_mode(open)).unwrap();
        assert!(launch(&i, &executable, &user).is_err(), "{open:o}");
    }
    fs::set_permissions(&user, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(launch(&i, &executable, &user).is_ok());
    // A policy layer that is not a real directory cannot be masked.
    for kind in ["file", "symlink"] {
        let t = Temp::new();
        let (i, user, executable) = t.layout();
        let layer = t.0.join("repo/sub/.codex");
        if kind == "file" {
            fs::write(&layer, "x").unwrap();
        } else {
            std::os::unix::fs::symlink(t.dir("real-policy"), &layer).unwrap();
        }
        assert!(i.layers().is_err(), "{kind}");
        assert!(launch(&i, &executable, &user).is_err(), "{kind}");
    }
    // The workspace must be the canonical path of an existing directory.
    let t = Temp::new();
    let (i, user, executable) = t.layout();
    let alias = t.0.join("alias");
    std::os::unix::fs::symlink(&i.workspace, &alias).unwrap();
    for workspace in [alias, t.0.join("missing"), PathBuf::from("relative/ws")] {
        let other = Isolation {
            workspace,
            state: i.state.clone(),
            control: Vec::new(),
        };
        assert!(other.layers().is_err() && launch(&other, &executable, &user).is_err());
    }
    // Hiding the user's Codex home must never hide the workspace or the private state.
    let inside = Isolation {
        workspace: t.dir("user/project"),
        state: i.state.clone(),
        control: Vec::new(),
    };
    assert!(launch(&inside, &executable, &user).is_err());
    let state_inside = Isolation {
        workspace: i.workspace.clone(),
        state: user.join("state"),
        control: Vec::new(),
    };
    assert!(launch(&state_inside, &executable, &user).is_err());
    // Hiding the control plane must never hide the workspace, and what is hidden is
    // exactly a real private directory of this user.
    let control = t.dir("control");
    let alias = t.0.join("control-alias");
    std::os::unix::fs::symlink(&control, &alias).unwrap();
    for (workspace, hidden) in [
        (t.dir("control/project"), control.clone()),
        (i.workspace.clone(), alias),
        (i.workspace.clone(), t.0.join("control-missing")),
        (i.workspace.clone(), t.file("control-file", "x", 0o600)),
        (i.workspace.clone(), t.dir("user/control")),
    ] {
        let other = Isolation {
            workspace,
            state: i.state.clone(),
            control: vec![hidden.clone()],
        };
        assert!(launch(&other, &executable, &user).is_err(), "{hidden:?}");
    }
    let hiding = Isolation {
        workspace: i.workspace.clone(),
        state: i.state.clone(),
        control: vec![control],
    };
    assert!(launch(&hiding, &executable, &user).is_ok());
    // Tampered private state.
    for sabotage in ["credential", "policy", "mask", "mode"] {
        let t = Temp::new();
        let (i, user, executable) = t.layout();
        launch(&i, &executable, &user).unwrap();
        match sabotage {
            "credential" => fs::write(i.state.join("home/auth.json"), "stolen").unwrap(),
            "policy" => fs::write(i.state.join("etc/extra.toml"), "x").unwrap(),
            "mask" => fs::write(i.state.join("empty/config.toml"), "x").unwrap(),
            _ => fs::set_permissions(&i.state, fs::Permissions::from_mode(0o750)).unwrap(),
        }
        assert!(launch(&i, &executable, &user).is_err(), "{sabotage}");
    }
    // A weakened policy file is rewritten, never trusted.
    let t = Temp::new();
    let (i, user, executable) = t.layout();
    launch(&i, &executable, &user).unwrap();
    fs::write(
        i.state.join("etc/requirements.toml"),
        "[features]\nunified_exec = true\n",
    )
    .unwrap();
    launch(&i, &executable, &user).unwrap();
    assert_eq!(
        fs::read_to_string(i.state.join("etc/requirements.toml")).unwrap(),
        safety::LOCAL_TOOLS_REQUIREMENTS
    );
}
#[test]
fn the_real_namespace_hides_user_state_and_exposes_only_policy_and_the_credential() {
    let t = Temp::new();
    let (i, user, _) = t.layout();
    let layer = t.dir("repo/sub/.codex");
    t.file("repo/sub/.codex/allow.rules", "allow everything", 0o644);
    let script = r#"cat /etc/codex/requirements.toml; printf '|'; ls -A /etc/codex | tr '\n' ' '; printf '|'
ls -A "$WYSS_TEST_USER" | wc -l | tr -d ' \n'; printf '|'; cat "$CODEX_HOME/auth.json"; printf '|'
ls -A "$WYSS_TEST_LAYER" | wc -l | tr -d ' \n'; printf '|'
(echo x > "$WYSS_TEST_LAYER/new") 2>/dev/null && printf writable || printf readonly; printf '|'
printf REFRESHED > "$CODEX_HOME/auth.json" && printf written"#;
    let held = credential(&user).unwrap();
    let seen = inside(
        &i,
        &user,
        &place_of(&i),
        &Grants {
            masked: &i.layers().unwrap(),
            catalog: None,
            credential: held.as_ref(),
        },
        script,
        &[("WYSS_TEST_USER", &user), ("WYSS_TEST_LAYER", &layer)],
    );
    assert_eq!(
        seen,
        "[features]\nunified_exec = false\n|managed_config.toml requirements.toml |0|FIXTURE-CREDENTIAL|0|readonly|written"
    );
    // Outside the namespace nothing was hidden, copied or created.
    assert_eq!(
        fs::read_to_string(user.join("config.toml")).unwrap(),
        "model = \"private\""
    );
    assert_eq!(
        fs::read_to_string(layer.join("allow.rules")).unwrap(),
        "allow everything"
    );
    assert!(!layer.join("new").exists() && !i.workspace.join(".codex").exists());
    // The in-place refresh reached the single credential store, not a private copy.
    assert_eq!(
        fs::read_to_string(user.join("auth.json")).unwrap(),
        "REFRESHED"
    );
    assert_eq!(
        fs::metadata(i.state.join("home/auth.json")).unwrap().len(),
        0
    );
}
#[test]
fn the_bound_credential_is_the_validated_inode_and_its_identity_is_watched() {
    let t = Temp::new();
    let (i, user, _) = t.layout();
    let auth = user.join("auth.json");
    let held = credential(&user).unwrap().unwrap();
    assert!(Isolation::same_credential(&held, &user).unwrap());
    // The path is given another file after validation: what enters the namespace is still
    // the inode that was validated, and the changed identity is noticed.
    fs::rename(&auth, user.join("moved.json")).unwrap();
    t.file("user/auth.json", "REPLACEMENT", 0o600);
    let seen = inside(
        &i,
        &user,
        &place_of(&i),
        &Grants {
            masked: &[],
            catalog: None,
            credential: Some(&held),
        },
        r#"cat "$CODEX_HOME/auth.json""#,
        &[],
    );
    assert_eq!(seen, "FIXTURE-CREDENTIAL");
    assert!(!Isolation::same_credential(&held, &user).unwrap());
    fs::remove_file(&auth).unwrap();
    assert!(!Isolation::same_credential(&held, &user).unwrap());
    fs::rename(user.join("moved.json"), &auth).unwrap();
    assert!(Isolation::same_credential(&held, &user).unwrap());
    // An in-place refresh changes size and times, never the identity.
    fs::write(&auth, "A-LONGER-REFRESHED-CREDENTIAL").unwrap();
    assert!(Isolation::same_credential(&held, &user).unwrap());
    // Wider access, a second name or a looser home end the capability that was validated.
    fs::set_permissions(&auth, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(!Isolation::same_credential(&held, &user).unwrap());
    fs::set_permissions(&auth, fs::Permissions::from_mode(0o600)).unwrap();
    fs::hard_link(&auth, t.0.join("second-name")).unwrap();
    assert!(!Isolation::same_credential(&held, &user).unwrap());
    fs::remove_file(t.0.join("second-name")).unwrap();
    fs::set_permissions(&user, fs::Permissions::from_mode(0o777)).unwrap();
    assert!(!Isolation::same_credential(&held, &user).unwrap());
    fs::set_permissions(&user, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Isolation::same_credential(&held, &user).unwrap());
    // A link under the credential's name is never followed to the validated inode.
    fs::rename(&auth, t.0.join("elsewhere.json")).unwrap();
    std::os::unix::fs::symlink(t.0.join("elsewhere.json"), &auth).unwrap();
    assert!(!Isolation::same_credential(&held, &user).unwrap());
}
#[test]
fn the_control_plane_and_the_host_policy_directory_never_exist_inside_the_namespace() {
    let t = Temp::new();
    let (layout, user, _) = t.layout();
    let run = t.dir("control/run");
    let state = t.dir("control/state");
    let i = Isolation {
        workspace: layout.workspace.clone(),
        state: state.join("codex-local-tools"),
        control: vec![run.clone(), state.clone()],
    };
    let _socket = std::os::unix::net::UnixListener::bind(run.join("control.sock")).unwrap();
    t.file("control/run/control.lock", "", 0o600);
    let catalog = t.file("control/run/catalog.json", "CATALOG", 0o600);
    t.file("control/state/control.sqlite3", "CANONICAL", 0o600);
    t.file("control/state/control.lock", "", 0o600);
    // Where the runtime starts is one more directory of the control plane.
    let cwd = t.place("control/run/runtime-cwd");
    // No such directory exists on this host: the policy is mounted on a private view.
    let absent = PathBuf::from(format!("/etc/wyss-isolation-{}", uuid::Uuid::new_v4()));
    let probe = PathBuf::from(format!("/etc/wyss-isolation-{}", uuid::Uuid::new_v4()));
    assert!(!absent.exists() && !probe.exists());
    MANAGED_AT.with(|at| *at.borrow_mut() = absent.clone());
    let script = r#"ls -A "$RUN" | tr '\n' ' '; printf '|'; ls -A "$STATE" | tr '\n' ' '; printf '|'
ls -A "$STATE/codex-local-tools" | tr '\n' ' '; printf '|'
cat "$CATALOG" 2>/dev/null; printf '|'
if test -e "$CATALOG"; then (echo x >> "$CATALOG") 2>/dev/null && printf writable || printf readonly; else printf absent; fi; printf '|'
ls -A "$MANAGED" | tr '\n' ' '; printf '|'
(echo x > "$MANAGED/extra.toml") 2>/dev/null && printf writable || printf readonly; printf '|'
echo CACHE > "$CODEX_HOME/models_cache.json" && printf home; printf '|'
(echo x > "$PROBE") 2>/dev/null && printf private-etc || printf no-etc; printf '|'
test -r /etc/passwd && printf host-etc"#;
    let held = credential(&user).unwrap();
    let environment: [(&str, &Path); 5] = [
        ("RUN", &run),
        ("STATE", &state),
        ("CATALOG", &catalog),
        ("MANAGED", &absent),
        ("PROBE", &probe),
    ];
    // The thread-capable process: its catalog comes back read-only, and nothing else but
    // the name of the empty place the process stands in.
    let seen = inside(
        &i,
        &user,
        &cwd,
        &Grants {
            masked: &[],
            catalog: Some(&catalog),
            credential: held.as_ref(),
        },
        script,
        &environment,
    );
    assert_eq!(
        seen,
        "catalog.json runtime-cwd |codex-local-tools |home |CATALOG|readonly|managed_config.toml requirements.toml |readonly|home|private-etc|host-etc"
    );
    // Discovery has no catalog: of the authority's runtime directory there is only the
    // name of the empty place the process stands in.
    let seen = inside(
        &i,
        &user,
        &cwd,
        &Grants {
            masked: &[],
            catalog: None,
            credential: held.as_ref(),
        },
        script,
        &environment,
    );
    assert_eq!(
        seen,
        "runtime-cwd |codex-local-tools |home ||absent|managed_config.toml requirements.toml |readonly|home|private-etc|host-etc"
    );
    MANAGED_AT.with(|at| *at.borrow_mut() = PathBuf::from(MANAGED));
    // On the host nothing moved: the control plane is intact, the home received the write
    // and `/etc` gained neither the mount point nor the file written inside the namespace.
    assert!(!absent.exists() && !probe.exists());
    assert_eq!(
        fs::read_to_string(state.join("control.sqlite3")).unwrap(),
        "CANONICAL"
    );
    assert_eq!(fs::read_to_string(&catalog).unwrap(), "CATALOG");
    assert!(run.join("control.sock").exists() && run.join("control.lock").exists());
    assert_eq!(
        fs::read_to_string(i.home().join("models_cache.json")).unwrap(),
        "CACHE\n"
    );
}
#[test]
fn the_runtimes_own_empty_markers_never_change_what_the_masks_prove() {
    let t = Temp::new();
    let (i, user, executable) = t.layout();
    let policy = t.dir("repo/.codex");
    t.file(
        "repo/.codex/config.toml",
        "approval_policy = \"never\"",
        0o644,
    );
    let chain = i.project_dirs().unwrap();
    let masked = i.layers().unwrap();
    assert_eq!(chain.len(), 3);
    assert_eq!(masked, std::slice::from_ref(&policy));
    assert!(i.masks_hold(&chain, &masked).unwrap());
    // An interrupted sandbox of the runtime leaves empty protected directories in the
    // workspace it was writing to. The workspace now looks like a project root of its own,
    // and neither this generation's proof nor the next launch covers one directory less.
    for name in [".git", ".codex", ".agents", ".aws"] {
        fs::create_dir(i.workspace.join(name)).unwrap();
    }
    assert!(i.masks_hold(&chain, &masked).unwrap());
    assert_eq!(i.project_dirs().unwrap(), chain);
    assert_eq!(
        i.layers().unwrap(),
        [policy.clone(), i.workspace.join(".codex")]
    );
    assert_eq!(
        i.trust_roots().unwrap(),
        [text(&t.0.join("repo")), text(&i.workspace)]
    );
    let relaunched = stable(argv(&launch(&i, &executable, &user).unwrap()));
    for layer in [&policy, &i.workspace.join(".codex")] {
        assert!(
            relaunched.contains(&text(layer)),
            "{layer:?} is masked again"
        );
    }
    // Policy in a layer nothing masks is refused wherever it appears.
    t.file("repo/sub/ws/.codex/config.toml", "x", 0o644);
    assert!(!i.masks_hold(&chain, &masked).unwrap());
    fs::remove_file(i.workspace.join(".codex/config.toml")).unwrap();
    assert!(i.masks_hold(&chain, &masked).unwrap());
    t.file("repo/sub/.codex/rules/allow.rules", "allow", 0o644);
    assert!(!i.masks_hold(&chain, &masked).unwrap());
    fs::remove_dir_all(t.0.join("repo/sub/.codex")).unwrap();
    assert!(i.masks_hold(&chain, &masked).unwrap());
    // A masked layer that is gone, or no longer a directory, ends the proof.
    fs::remove_dir_all(&policy).unwrap();
    assert!(!i.masks_hold(&chain, &masked).unwrap());
    fs::write(&policy, "not a directory").unwrap();
    assert!(i.masks_hold(&chain, &masked).is_err());
    // A repository that appears around the workspace after launch brings its layers into
    // the proof: its policy was never masked.
    let t = Temp::new();
    let workspace = t.dir("late/sub/ws");
    let i = Isolation {
        workspace: workspace.clone(),
        state: t.0.join("state"),
        control: Vec::new(),
    };
    t.file(
        "late/.codex/config.toml",
        "approval_policy = \"never\"",
        0o644,
    );
    let chain = i.project_dirs().unwrap();
    let masked = i.layers().unwrap();
    assert_eq!((chain.len(), masked.len()), (1, 0));
    assert!(i.masks_hold(&chain, &masked).unwrap());
    t.dir("late/.git");
    assert!(!i.masks_hold(&chain, &masked).unwrap());
    // And one that disappears takes nothing out of the proof: a directory the namespace
    // was created for is still checked when its repository marker is gone.
    let t = Temp::new();
    let (i, _, _) = t.layout();
    let chain = i.project_dirs().unwrap();
    let masked = i.layers().unwrap();
    assert_eq!((chain.len(), masked.len()), (3, 0));
    fs::remove_dir(t.0.join("repo/.git")).unwrap();
    assert_eq!(
        i.project_dirs().unwrap(),
        std::slice::from_ref(&i.workspace)
    );
    assert!(i.masks_hold(&chain, &masked).unwrap());
    t.file(
        "repo/sub/.codex/config.toml",
        "approval_policy = \"never\"",
        0o644,
    );
    assert!(!i.masks_hold(&chain, &masked).unwrap());
}
#[test]
fn a_home_that_is_itself_a_repository_keeps_its_hidden_codex_home_and_pinned_release() {
    // The user's home is a repository and the workspace lives in it, so the user's Codex
    // home is also a project layer of the chain. It is hidden once, as the user's state:
    // masking it a second time would take the pinned release away again.
    let t = Temp::new();
    t.dir("home/.git");
    let workspace = t.dir("home/project");
    let user = t.dir("home/.codex");
    t.file("home/.codex/auth.json", "FIXTURE-CREDENTIAL", 0o600);
    t.file("home/.codex/config.toml", "model = \"private\"", 0o600);
    let executable = t.file(
        "home/.codex/packages/release-1/bin/codex",
        "#!/bin/sh\nprintf 'RELEASE|'; ls -A \"$WYSS_TEST_USER\" | tr '\\n' ' '\n",
        0o700,
    );
    let i = Isolation {
        workspace,
        state: t.0.join("state"),
        control: Vec::new(),
    };
    assert_eq!(i.layers().unwrap(), std::slice::from_ref(&user));
    let mut command = launch(&i, &executable, &user).unwrap();
    let args = stable(argv(&command));
    assert_eq!(args.iter().filter(|a| **a == text(&user)).count(), 1);
    assert!(!args.contains(&text(&i.state.join("empty"))));
    let output = command
        .env("WYSS_TEST_USER", &user)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The release ran, and of the user's Codex home only the path to it exists.
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "RELEASE|packages "
    );
}
/// The layout the authority really has: the working directory is inside the runtime
/// directory, which the namespace hides whole.
fn governed(t: &Temp) -> (Isolation, PathBuf, PathBuf, PathBuf, PathBuf) {
    let (layout, user, executable) = t.layout();
    let run = t.dir("control/run");
    let state = t.dir("control/state");
    let i = Isolation {
        workspace: layout.workspace,
        state: state.join("codex-local-tools"),
        control: vec![run.clone(), state],
    };
    let cwd = t.place("control/run/runtime-cwd");
    (i, user, executable, run, cwd)
}
#[test]
fn the_working_directory_is_the_last_thing_of_the_namespace_and_the_one_place_it_goes_to() {
    let t = Temp::new();
    let (i, user, executable, run, cwd) = governed(&t);
    let catalog = t.file("control/run/catalog.json", "CATALOG", 0o600);
    let held = credential(&user).unwrap();
    let command = i
        .launch(
            &executable,
            vec!["app-server".into()],
            &cwd,
            &Grants {
                masked: &[],
                catalog: Some(&catalog),
                credential: held.as_ref(),
            },
            &user,
            None,
            &[],
        )
        .unwrap();
    let state = &i.control[1];
    // The whole launch. The control plane is hidden first; of it only the generation's
    // home and catalog are brought back, each by itself; then the working directory is
    // made: a new mount of its own, never the host directory and never a parent of it.
    // Nothing is mounted after it, so nothing can cover the place the process goes to.
    assert_eq!(
        stable(argv(&command)),
        [
            "--die-with-parent".to_owned(),
            "--dev-bind".into(),
            "/".into(),
            "/".into(),
            "--overlay-src".into(),
            "/etc".into(),
            "--tmp-overlay".into(),
            "/etc".into(),
            "--ro-bind".into(),
            text(&i.state.join("etc")),
            "/etc/codex".into(),
            "--tmpfs".into(),
            text(&user),
            "--tmpfs".into(),
            text(&run),
            "--tmpfs".into(),
            text(state),
            "--bind".into(),
            text(&i.home()),
            text(&i.home()),
            "--ro-bind".into(),
            text(&catalog),
            text(&catalog),
            "--bind-fd".into(),
            "<credential>".into(),
            text(&i.home().join("auth.json")),
            "--perms".into(),
            "0700".into(),
            "--tmpfs".into(),
            text(&cwd),
            "--remount-ro".into(),
            text(&cwd),
            "--chdir".into(),
            text(&cwd),
            "--".into(),
            text(&executable),
            "app-server".into(),
        ]
    );
    // The launcher is told the place; where it is started itself is not part of the launch.
    assert_eq!(command.get_current_dir(), None);
}
#[test]
fn the_process_stands_in_an_empty_sealed_directory_of_the_namespace_wherever_the_launcher_was() {
    use std::os::unix::fs::MetadataExt;
    let t = Temp::new();
    let (i, user, _, run, cwd) = governed(&t);
    let _socket = std::os::unix::net::UnixListener::bind(run.join("control.sock")).unwrap();
    t.file("control/state/control.sqlite3", "CANONICAL", 0o600);
    let home = t.dir("home");
    let held = credential(&user).unwrap();
    let script = r#"pwd -P | tr -d '\n'; printf '|%s|' "$PWD"; ls -A . | wc -l | tr -d ' \n'; printf '|'
stat -c '%a %u %d:%i' . | tr -d '\n'; printf '|'
(echo x > ./new) 2>/dev/null && printf writable || printf sealed; printf '|'
ls -A "$RUN" | tr '\n' ' '; printf '|'; ls -A "$STATE" | tr '\n' ' '"#;
    let host = fs::metadata(&cwd).unwrap();
    // Whatever directory the launcher is started in, and whatever it is told is home, the
    // process ends up in the one place it was sent to.
    for launcher in [&cwd, &i.workspace, &home, &t.0] {
        let mut command = i
            .launch(
                Path::new("/bin/sh"),
                vec!["-c".into(), script.into()],
                &cwd,
                &Grants {
                    masked: &[],
                    catalog: None,
                    credential: held.as_ref(),
                },
                &user,
                None,
                &[],
            )
            .unwrap();
        let output = command
            .current_dir(launcher)
            .env("HOME", &home)
            .env("RUN", &run)
            .env("STATE", &i.control[1])
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let seen = String::from_utf8(output.stdout).unwrap();
        let fields: Vec<&str> = seen.split('|').collect();
        assert_eq!(fields[0], text(&cwd), "{launcher:?}");
        assert_eq!(fields[1], text(&cwd), "{launcher:?}");
        assert_eq!(fields[2], "0", "it is empty");
        // Private, this user's, and a directory of the namespace: not the host one.
        let (own, identity) = fields[3].rsplit_once(' ').unwrap();
        assert_eq!(own, format!("700 {}", own_uid()));
        assert_ne!(identity, format!("{}:{}", host.dev(), host.ino()));
        assert_eq!(fields[4], "sealed");
        // The directory it stands in is all there is of the control plane.
        assert_eq!(fields[5], "runtime-cwd ");
        assert_eq!(fields[6], "codex-local-tools ");
    }
    // On the host the directory is the one it was, still empty, and nothing was hidden.
    let after = fs::metadata(&cwd).unwrap();
    assert_eq!((after.dev(), after.ino()), (host.dev(), host.ino()));
    assert_eq!(fs::read_dir(&cwd).unwrap().count(), 0);
    assert!(run.join("control.sock").exists());
}
#[test]
fn a_working_directory_that_is_not_a_private_place_apart_keeps_the_profile_off() {
    let grants = |catalog| Grants {
        masked: &[],
        catalog,
        credential: None,
    };
    let try_in = |i: &Isolation, executable: &Path, user: &Path, cwd: &Path| {
        i.launch(
            executable,
            vec!["app-server".into()],
            cwd,
            &grants(None),
            user,
            None,
            &[],
        )
    };
    let t = Temp::new();
    let (i, user, executable, _, cwd) = governed(&t);
    assert!(try_in(&i, &executable, &user, &cwd).is_ok());
    // It has to be there, a real directory, reached through no link.
    assert!(try_in(&i, &executable, &user, &t.0.join("missing")).is_err());
    let file = t.file("control/run/file", "", 0o600);
    assert!(try_in(&i, &executable, &user, &file).is_err());
    let alias = t.0.join("control/run/alias");
    std::os::unix::fs::symlink(&cwd, &alias).unwrap();
    assert!(try_in(&i, &executable, &user, &alias).is_err());
    let through = t.0.join("control-alias");
    std::os::unix::fs::symlink(t.0.join("control"), &through).unwrap();
    assert!(try_in(&i, &executable, &user, &through.join("run/runtime-cwd")).is_err());
    // Nobody else may enter it.
    for open in [0o750, 0o705, 0o770, 0o707] {
        fs::set_permissions(&cwd, fs::Permissions::from_mode(open)).unwrap();
        assert!(try_in(&i, &executable, &user, &cwd).is_err(), "{open:o}");
    }
    fs::set_permissions(&cwd, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(try_in(&i, &executable, &user, &cwd).is_ok());
    // It is no part of a project: not the workspace, not inside it, not above it, and
    // nowhere in the repository the workspace belongs to.
    let private = |path: PathBuf| {
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    };
    for project in [
        private(i.workspace.clone()),
        t.place("repo/sub/ws/inner"),
        private(t.0.join("repo/sub")),
        private(t.0.join("repo")),
        t.place("repo/elsewhere"),
    ] {
        assert!(
            try_in(&i, &executable, &user, &project).is_err(),
            "{project:?}"
        );
    }
    // A workspace in no repository is still a project of its own.
    let t = Temp::new();
    let (i, user, executable, _, cwd) = governed(&t);
    fs::remove_dir(t.0.join("repo/.git")).unwrap();
    assert!(try_in(&i, &executable, &user, &cwd).is_ok());
    assert!(try_in(&i, &executable, &user, &t.place("repo/sub/ws/inner")).is_err());
    assert!(try_in(&i, &executable, &user, &private(t.0.join("repo/sub"))).is_err());
    // It is nowhere in the user's Codex state.
    assert!(try_in(&i, &executable, &user, &t.place("user/place")).is_err());
    // What is mounted there covers only itself: nothing the generation is given is below.
    assert!(try_in(&i, &executable, &user, &private(t.0.join("bin"))).is_err());
    assert!(try_in(&i, &executable, &user, &private(i.state.clone())).is_err());
    assert!(try_in(&i, &executable, &user, &private(t.0.join("control/state"))).is_err());
    let holder = t.place("control/run/holder");
    let catalog = t.file("control/run/holder/catalog.json", "CATALOG", 0o600);
    let with_catalog = |catalog| {
        i.launch(
            &executable,
            vec!["app-server".into()],
            &holder,
            &grants(catalog),
            &user,
            None,
            &[],
        )
    };
    assert!(with_catalog(Some(&catalog)).is_err());
    assert!(with_catalog(None).is_ok());
}

#[test]
fn f8_owned_grant_crosses_exec_and_is_consumed_by_the_real_namespace_launcher() {
    use crate::f8::{
        engine::{StopState, Worker},
        ownership::{ContainmentPolicy, Grace},
        process_worker::DeferredProcessWorker,
    };
    use std::time::{Duration, Instant};
    let t = Temp::new();
    let (i, user, _) = t.layout();
    let credential = credential(&user).unwrap().unwrap();
    let mut grants = Vec::new();
    let command = i
        .launch_inner(
            Path::new("/bin/sh"),
            vec![
                "-c".into(),
                // Only a synthetic credential in the test's own private directory.
                "cat \"$CODEX_HOME/auth.json\"; printf '\\n'; if test ! -e \"$WYSS_TEST_FD\"; then echo grant-consumed; else echo grant-leaked; fi".into(),
            ],
            &place_of(&i),
            &Grants {
                masked: &i.layers().unwrap(),
                catalog: None,
                credential: Some(&credential),
            },
            &user,
            None,
            &[],
            Some(&mut grants), Vec::new(),
            None,
        )
        .unwrap();
    assert_eq!(grants.len(), 1);
    let fd = grants[0].as_raw_fd();
    let mut command = command;
    command.env("WYSS_TEST_FD", format!("/proc/self/fd/{fd}"));
    let launch = IsolationLaunch { command, grants };
    // Remove the source owner before enqueueing: the launch owns a distinct FD.
    drop(credential);
    let mut worker = DeferredProcessWorker::spawn_isolated(
        launch,
        Grace::default(),
        ContainmentPolicy::DirectChildOnly,
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut frames = Vec::new();
    while !worker.has_exited() {
        frames.extend(worker.inbound(16).unwrap());
        assert!(
            Instant::now() < deadline,
            "namespace launch must be bounded"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    frames.extend(worker.inbound(16).unwrap());
    assert_eq!(
        frames,
        [b"FIXTURE-CREDENTIAL".to_vec(), b"grant-consumed".to_vec()]
    );
    worker.begin_stop(Instant::now());
    assert_eq!(worker.poll_stop(Instant::now()), StopState::Reaped);
    println!(
        "post-exec bwrap: synthetic validated inode mounted; source owner dropped before spawn"
    );
}

#[test]
fn f8_pinned_workspace_fd_mounts_original_object_after_host_path_replacement() {
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, process::CommandExt},
    };

    let t = Temp::new();
    let (i, user, _) = t.layout();
    let original = i.workspace.clone();
    fs::write(original.join("identity"), "pinned-object").unwrap();
    let c_path = std::ffi::CString::new(original.as_os_str().as_bytes()).unwrap();
    // Match RootPin: an O_PATH directory descriptor retained by F8.
    let raw = unsafe {
        nix::libc::open(
            c_path.as_ptr(),
            nix::libc::O_PATH | nix::libc::O_DIRECTORY | nix::libc::O_CLOEXEC,
        )
    };
    assert!(raw >= 0);
    let descriptor = unsafe { std::fs::File::from_raw_fd(raw) };

    let moved = t.0.join("repo/sub/moved-workspace");
    fs::rename(&original, &moved).unwrap();
    fs::create_dir(&original).unwrap();
    fs::write(original.join("identity"), "replacement-object").unwrap();

    let mut grants = Vec::new();
    let mut command = i
        .launch_inner(
            Path::new("/bin/sh"),
            vec!["-c".into(), "cat \"$WORKSPACE/identity\"".into()],
            &place_of(&i),
            &Grants {
                masked: &[],
                catalog: None,
                credential: None,
            },
            &user,
            None,
            &[],
            Some(&mut grants),
            Vec::new(),
            Some((descriptor.into(), i.workspace.clone())),
        )
        .unwrap();
    command.env("WORKSPACE", &i.workspace);
    assert_eq!(grants.len(), 1, "the pinned source stays owned until spawn");
    let inherited = grants.iter().map(AsRawFd::as_raw_fd).collect::<Vec<_>>();
    unsafe {
        command.pre_exec(move || {
            for fd in &inherited {
                if nix::libc::fcntl(*fd, nix::libc::F_SETFD, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "pinned namespace mount must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"pinned-object");
}

#[test]
fn sealed_f8_images_are_mountable_by_bwrap() {
    let t = Temp::new();
    let (i, user, _) = t.layout();
    let catalog = t.file("catalog.json", r#"{"models":[{"slug":"fixture","tool_mode":"direct","experimental_supported_tools":[],"multi_agent_version":null}]}"#, 0o600);
    let cwd = place_of(&i);
    let (launch, _proof) = i
        .prepare_f8(Path::new("/bin/true"), &user, &cwd, &catalog)
        .unwrap();
    let (mut command, grants) = launch.into_parts();
    unsafe {
        command.pre_exec(move || {
            for fd in &grants {
                if nix::libc::fcntl(fd.as_raw_fd(), nix::libc::F_SETFD, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn queued_f8_exec_uses_sealed_image_after_rename_and_in_place_source_replacement() {
    use crate::f8::ownership::{ContainmentPolicy, Grace, PendingOwnedSpawn};
    use std::{
        io::Read,
        time::{Duration, Instant},
    };
    for replace_inode in [false, true] {
        let t = Temp::new();
        let (i, user, _) = t.layout();
        let cwd = place_of(&i);
        let executable = t.file("bin/fixture", "#!/bin/sh\nprintf trusted-image", 0o700);
        let catalog = t.file("catalog.json", r#"{"models":[{"slug":"fixture","tool_mode":"direct","experimental_supported_tools":[],"multi_agent_version":null}]}"#, 0o600);
        let (mut launch, _proof) = i.prepare_f8(&executable, &user, &cwd, &catalog).unwrap();
        let sealed: Vec<_> = launch
            .grants
            .iter()
            .filter(|fd| unsafe { nix::libc::fcntl(fd.as_raw_fd(), nix::libc::F_GET_SEALS) } >= 0)
            .collect();
        assert_eq!(sealed.len(), 4);
        for fd in sealed {
            assert_ne!(
                unsafe { nix::libc::fcntl(fd.as_raw_fd(), nix::libc::F_GET_SEALS) }
                    & nix::libc::F_SEAL_WRITE,
                0
            );
            assert_eq!(
                unsafe { nix::libc::write(fd.as_raw_fd(), b"x".as_ptr().cast(), 1) },
                -1
            );
        }
        launch.pipe_stdio();
        let mut pending = PendingOwnedSpawn::spawn_isolated(
            launch,
            ContainmentPolicy::DescendantsContained,
            Grace::default(),
        )
        .unwrap();
        // The child cannot pass its ownership gate before poll. Substitute the
        // source while queued/pre-exec; neither inode nor byte reuse changes it.
        if replace_inode {
            fs::rename(&executable, executable.with_extension("old")).unwrap();
        }
        fs::write(&executable, "#!/bin/sh\nprintf SUBSTITUTED").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let (mut child, _) = loop {
            if let Some(result) = pending.poll(Instant::now()) {
                break result.unwrap();
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        let mut text = String::new();
        child
            .take_stdout()
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "trusted-image");
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[test]
fn f8_catalog_validation_covers_sealed_bytes_even_when_path_is_replaced() {
    let t = Temp::new();
    let valid = r#"{"models":[{"slug":"fixture","tool_mode":"direct","experimental_supported_tools":[],"multi_agent_version":null}]}"#;
    let path = t.file("catalog.json", valid, 0o600);
    assert!(safety::local_tools_catalog(&fs::read(&path).unwrap()).is_ok());
    fs::write(
        &path,
        r#"{"models":[{"slug":"fixture","tool_mode":"code_mode_only"}]}"#,
    )
    .unwrap();
    let image = sealed_file(&path, 4 * 1024 * 1024, false).unwrap();
    fs::write(&path, valid).unwrap();
    assert!(validate_catalog_image(&image).is_err());
    let image = sealed_file(&path, 4 * 1024 * 1024, false).unwrap();
    assert!(validate_catalog_image(&image).is_ok());
}

#[test]
fn f8_credential_consent_is_checked_on_the_acquired_grant_before_any_exec() {
    use crate::f8::{launcher::NativeLocalToolsPlan, production::CodexConfig};
    let t = Temp::new();
    let (i, user, executable) = t.layout();
    fs::remove_file(user.join("auth.json")).unwrap();
    let config = CodexConfig {
        executable: executable.clone(),
        home: user.clone(),
        allow_credential: false,
    };
    assert!(config.check().is_ok());
    // Deterministically place the credential in the exact check/acquisition interval.
    t.file("user/auth.json", "FIXTURE-CREDENTIAL", 0o600);
    let marker = t.0.join("executed");
    fs::write(
        &executable,
        format!(
            "#!/bin/sh\nprintf executed > {}\nprintf 'codex-cli 0.159.0\\n'\n",
            marker.display()
        ),
    )
    .unwrap();
    let catalogue = t.file("catalog.json", r#"{"models":[{"slug":"fixture","tool_mode":"direct","experimental_supported_tools":[],"multi_agent_version":null}]}"#, 0o600);
    let cwd = place_of(&i);
    let refused = NativeLocalToolsPlan::prepare_with_credential_policy(
        i,
        &executable,
        &user,
        &cwd,
        catalogue,
        "0.159.0",
        false,
    );
    assert!(matches!(
        refused,
        Err(crate::contracts::control::ControlError::GateDenied)
    ));
    assert!(
        !marker.exists(),
        "neither version probe nor payload may execute with an unauthorized grant"
    );
}
