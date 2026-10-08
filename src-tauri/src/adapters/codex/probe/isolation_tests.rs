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
fn launch(i: &Isolation, executable: &Path, user: &Path) -> ProbeResult<Command> {
    let masked = i.layers()?;
    i.launch(
        executable,
        vec!["-c".into(), "a=b".into(), "app-server".into()],
        &masked,
        user,
        None,
        &[],
    )
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
    let command = i
        .launch(
            &executable,
            vec!["-c".into(), "a=b".into(), "app-server".into()],
            &i.layers().unwrap(),
            &user,
            None,
            &inherited,
        )
        .unwrap();
    assert_eq!(command.get_program(), "/usr/bin/bwrap");
    let state = &i.state;
    assert_eq!(
        argv(&command),
        [
            "--die-with-parent".to_owned(),
            "--dev-bind".into(),
            "/".into(),
            "/".into(),
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
            "--bind".into(),
            text(&user.join("auth.json")),
            text(&state.join("home/auth.json")),
            "--".into(),
            text(&executable),
            "-c".into(),
            "a=b".into(),
            "app-server".into(),
        ]
    );
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
        argv(&launch(&i, &executable, &user).unwrap()),
        argv(&command)
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
fn project_root_is_the_nearest_git_ancestor_and_only_its_descendants_contribute() {
    let t = Temp::new();
    let workspace = t.dir("plain/a/b");
    let i = Isolation {
        workspace: workspace.clone(),
        state: t.0.join("state"),
    };
    // A marker above the temporary directory is outside this test's control; one inside
    // it always wins because it is nearer.
    t.dir("plain/.git");
    t.dir("above-plain-is-ignored/.codex");
    assert_eq!(
        i.trust_roots().unwrap(),
        [text(&t.0.join("plain")), text(&workspace)]
    );
    assert!(i.layers().unwrap().is_empty());
    t.dir("plain/.codex");
    t.dir("plain/a/b/.codex");
    assert_eq!(
        i.layers().unwrap(),
        [t.0.join("plain/.codex"), workspace.join(".codex")]
    );
    t.dir("plain/a/.git");
    assert_eq!(
        i.trust_roots().unwrap(),
        [text(&t.0.join("plain/a")), text(&workspace)]
    );
    assert_eq!(i.layers().unwrap(), [workspace.join(".codex")]);
    t.dir("plain/a/.codex");
    assert_eq!(
        i.layers().unwrap(),
        [t.0.join("plain/a/.codex"), workspace.join(".codex")]
    );
    // The workspace is itself the project root: one root, one possible layer.
    t.dir("plain/a/b/.git");
    assert_eq!(i.trust_roots().unwrap(), [text(&workspace)]);
    assert_eq!(i.layers().unwrap(), [workspace.join(".codex")]);
}
#[test]
fn every_unsafe_input_keeps_the_profile_off() {
    // Credential: absent means no capability; anything but a private regular file is refused.
    let t = Temp::new();
    let (i, user, executable) = t.layout();
    fs::remove_file(user.join("auth.json")).unwrap();
    assert!(!argv(&launch(&i, &executable, &user).unwrap()).contains(&"--bind".to_owned()));
    t.file("user/auth.json", "x", 0o640);
    assert!(launch(&i, &executable, &user).is_err());
    fs::remove_file(user.join("auth.json")).unwrap();
    let elsewhere = t.file("elsewhere.json", "x", 0o600);
    std::os::unix::fs::symlink(&elsewhere, user.join("auth.json")).unwrap();
    assert!(launch(&i, &executable, &user).is_err());
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
        };
        assert!(other.layers().is_err() && launch(&other, &executable, &user).is_err());
    }
    // Hiding the user's Codex home must never hide the workspace or the private state.
    let inside = Isolation {
        workspace: t.dir("user/project"),
        state: i.state.clone(),
    };
    assert!(launch(&inside, &executable, &user).is_err());
    let state_inside = Isolation {
        workspace: i.workspace.clone(),
        state: user.join("state"),
    };
    assert!(launch(&state_inside, &executable, &user).is_err());
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
    let mut command = i
        .launch(
            Path::new("/bin/sh"),
            vec!["-c".into(), script.into()],
            &i.layers().unwrap(),
            &user,
            None,
            &[],
        )
        .unwrap();
    let output = command
        .env("WYSS_TEST_USER", &user)
        .env("WYSS_TEST_LAYER", &layer)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "bwrap must be able to create the namespace: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
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
