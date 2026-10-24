use super::{super::worker::Identity, *};
use sha2::{Digest, Sha256};
use std::os::unix::fs::symlink;

const SDK: &str = "0.2.173";
const CLI: &str = "2.1.309";
const FAKE_CLI: &[u8] = b"#!/bin/sh\necho fake claude\n";

fn dir() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "wyss-claude-launch-{}",
        crate::control::store::uuid().as_str()
    ));
    fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
    fs::canonicalize(p).unwrap()
}
fn put(path: &Path, body: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
    fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).unwrap();
}
use std::os::unix::fs::PermissionsExt;

fn pin() -> pins::BuildPin {
    pins::BuildPin {
        sdk_version: SDK,
        cli_version: CLI,
        platform: "linux-x86_64",
        cli_sha256: Box::leak(hex(&Sha256::digest(FAKE_CLI)).into_boxed_str()),
    }
}

/// A virtual environment shaped like the one `uv pip install claude-agent-sdk==0.2.173` makes.
struct Venv {
    root: PathBuf,
    site: PathBuf,
}
fn venv() -> Venv {
    let root = dir();
    put(
        &root.join("pyvenv.cfg"),
        b"home = /usr/bin\ninclude-system-site-packages = false\nversion = 3.14.8\n",
    );
    fs::create_dir_all(root.join("bin")).unwrap();
    symlink("/usr/bin/python3", root.join("bin/python")).unwrap();
    let site = root.join("lib/python3.14/site-packages");
    let version = format!("__version__ = \"{SDK}\"\n");
    let cli_version = format!("__cli_version__ = \"{CLI}\"\n");
    let files: [(&str, &[u8]); 4] = [
        ("claude_agent_sdk/__init__.py", b"# sdk\n"),
        ("claude_agent_sdk/_version.py", version.as_bytes()),
        ("claude_agent_sdk/_cli_version.py", cli_version.as_bytes()),
        ("claude_agent_sdk/_bundled/claude", FAKE_CLI),
    ];
    let mut record = String::new();
    for (rel, body) in files {
        put(&site.join(rel), body);
        record.push_str(&format!(
            "{rel},sha256={},{}\n",
            b64(&Sha256::digest(body)),
            body.len()
        ));
    }
    record.push_str(&format!("claude_agent_sdk-{SDK}.dist-info/RECORD,,\n"));
    record.push_str("../../../bin/tool,sha256=AAAA,1\n");
    put(
        &site.join(format!("claude_agent_sdk-{SDK}.dist-info/RECORD")),
        record.as_bytes(),
    );
    Venv { root, site }
}
fn install(v: &Venv) -> ClaudeInstall {
    let mut i = ClaudeInstall::new(v.root.clone());
    i.policy_dir = v.root.join("no-policy");
    i
}
fn verify(v: &Venv) -> Result<Verified, Unverified> {
    install(v).verify_against(&[pin()])
}

#[test]
fn the_record_encoding_matches_pips() {
    // The SHA-256 of the empty string, as pip writes it in a RECORD.
    assert_eq!(
        b64(&Sha256::digest(b"")),
        "47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU"
    );
    assert_eq!(b64(&[0xfb, 0xff]), "-_8");
}

#[test]
fn an_installation_made_by_pip_verifies_and_reports_its_identity() {
    let v = venv();
    let ok = verify(&v).unwrap();
    assert_eq!(
        (ok.sdk_version.as_str(), ok.cli_version.as_str()),
        (SDK, CLI)
    );
    assert_eq!(ok.cli_sha256, hex(&Sha256::digest(FAKE_CLI)));
    assert_eq!(ok.python, v.root.join("bin/python"));
}

#[test]
fn a_modified_file_a_swapped_binary_or_a_planted_file_is_refused() {
    let v = venv();
    put(
        &v.site.join("claude_agent_sdk/__init__.py"),
        b"# sdk\nimport os\n",
    );
    assert_eq!(verify(&v), Err(Unverified::RecordMismatch));

    let v = venv();
    put(
        &v.site.join("claude_agent_sdk/_bundled/claude"),
        b"#!/bin/sh\necho evil\n",
    );
    assert_eq!(verify(&v), Err(Unverified::RecordMismatch));

    for planted in [
        "evil.pth",
        "claude_agent_sdk/extra.py",
        "sitecustomize.py",
        "zzz/x.py",
    ] {
        let v = venv();
        put(&v.site.join(planted), b"import os\n");
        assert_eq!(verify(&v), Err(Unverified::UnlistedFile), "{planted}");
    }
}

#[test]
fn a_cli_that_is_not_the_pinned_one_is_refused_even_when_the_record_agrees() {
    // pip recorded whatever it installed; the pin is the independent anchor.
    let v = venv();
    let other = b"#!/bin/sh\necho different but consistently recorded\n";
    put(&v.site.join("claude_agent_sdk/_bundled/claude"), other);
    let rec = v
        .site
        .join(format!("claude_agent_sdk-{SDK}.dist-info/RECORD"));
    let text = fs::read_to_string(&rec).unwrap().replace(
        &b64(&Sha256::digest(FAKE_CLI)),
        &b64(&Sha256::digest(other)),
    );
    put(&rec, text.as_bytes());
    assert_eq!(verify(&v), Err(Unverified::CliDigest));
}

#[test]
fn versions_outside_the_pin_are_refused() {
    let v = venv();
    put(
        &v.site.join("claude_agent_sdk/_version.py"),
        b"__version__ = \"0.2.172\"\n",
    );
    let rec = v
        .site
        .join(format!("claude_agent_sdk-{SDK}.dist-info/RECORD"));
    let text = fs::read_to_string(&rec).unwrap();
    let new = "__version__ = \"0.2.172\"\n".to_string();
    let line_old = text
        .lines()
        .find(|l| l.starts_with("claude_agent_sdk/_version.py"))
        .unwrap()
        .to_owned();
    let line_new = format!(
        "claude_agent_sdk/_version.py,sha256={},{}",
        b64(&Sha256::digest(new.as_bytes())),
        new.len()
    );
    put(&rec, text.replace(&line_old, &line_new).as_bytes());
    assert_eq!(verify(&v), Err(Unverified::SdkVersion));

    let v = venv();
    let new = "__cli_version__ = \"2.1.310\"\n";
    put(
        &v.site.join("claude_agent_sdk/_cli_version.py"),
        new.as_bytes(),
    );
    let rec = v
        .site
        .join(format!("claude_agent_sdk-{SDK}.dist-info/RECORD"));
    let text = fs::read_to_string(&rec).unwrap();
    let line_old = text
        .lines()
        .find(|l| l.starts_with("claude_agent_sdk/_cli_version.py"))
        .unwrap()
        .to_owned();
    let line_new = format!(
        "claude_agent_sdk/_cli_version.py,sha256={},{}",
        b64(&Sha256::digest(new.as_bytes())),
        new.len()
    );
    put(&rec, text.replace(&line_old, &line_new).as_bytes());
    assert_eq!(verify(&v), Err(Unverified::CliVersion));
}

#[test]
fn links_writable_files_and_foreign_owners_are_refused() {
    let v = venv();
    symlink("/etc/passwd", v.site.join("claude_agent_sdk/link.py")).unwrap();
    assert_eq!(verify(&v), Err(Unverified::UnsafePath));

    let v = venv();
    fs::set_permissions(
        v.site.join("claude_agent_sdk"),
        std::fs::Permissions::from_mode(0o777),
    )
    .unwrap();
    assert_eq!(verify(&v), Err(Unverified::UnsafePath));

    let v = venv();
    fs::set_permissions(
        v.site.join("claude_agent_sdk/__init__.py"),
        std::fs::Permissions::from_mode(0o666),
    )
    .unwrap();
    assert_eq!(verify(&v), Err(Unverified::UnsafePath));
}

#[test]
fn it_must_be_an_isolated_virtual_environment_with_a_system_interpreter() {
    let v = venv();
    put(
        &v.root.join("pyvenv.cfg"),
        b"home = /usr/bin\ninclude-system-site-packages = true\n",
    );
    assert_eq!(verify(&v), Err(Unverified::NotAVirtualEnvironment));

    let v = venv();
    fs::remove_file(v.root.join("pyvenv.cfg")).unwrap();
    assert_eq!(verify(&v), Err(Unverified::NotAVirtualEnvironment));

    // An interpreter this user owns (and could replace) is not a trusted interpreter.
    let v = venv();
    let mine = v.root.join("my-python");
    put(&mine, b"#!/bin/sh\n");
    fs::remove_file(v.root.join("bin/python")).unwrap();
    symlink(&mine, v.root.join("bin/python")).unwrap();
    assert_eq!(verify(&v), Err(Unverified::UnsafePath));

    // No second `site-packages`.
    let v = venv();
    fs::create_dir_all(v.root.join("lib/python3.13/site-packages")).unwrap();
    assert_eq!(verify(&v), Err(Unverified::NotAVirtualEnvironment));

    // A tree no package vouches for at all.
    let v = venv();
    fs::remove_file(
        v.site
            .join(format!("claude_agent_sdk-{SDK}.dist-info/RECORD")),
    )
    .unwrap();
    assert!(verify(&v).is_err());
}

#[test]
fn the_root_must_be_canonical_and_absolute() {
    let v = venv();
    let link = dir().join("alias");
    symlink(&v.root, &link).unwrap();
    let mut i = install(&v);
    i.sdk_root = link;
    assert_eq!(i.verify_against(&[pin()]), Err(Unverified::UnsafePath));
    i.sdk_root = PathBuf::from("relative/venv");
    assert_eq!(i.verify_against(&[pin()]), Err(Unverified::UnsafePath));
    i.sdk_root = v.root.join("missing");
    assert!(i.verify_against(&[pin()]).is_err());
}

#[test]
fn an_administrators_policy_makes_the_runtime_unavailable_it_is_never_hidden() {
    let v = venv();
    let mut i = install(&v);
    i.policy_dir = v.root.join("etc-claude-code");
    fs::create_dir_all(&i.policy_dir).unwrap();
    assert!(
        i.verify_against(&[pin()]).is_ok(),
        "an empty policy directory is no policy"
    );
    put(
        &i.policy_dir.join("managed-settings.json"),
        b"{\"hooks\":{}}",
    );
    assert_eq!(i.verify_against(&[pin()]), Err(Unverified::ManagedPolicy));
    fs::remove_file(i.policy_dir.join("managed-settings.json")).unwrap();
    put(&i.policy_dir.join("managed-mcp.json"), b"{}");
    assert_eq!(i.verify_against(&[pin()]), Err(Unverified::ManagedPolicy));
    fs::remove_dir_all(&i.policy_dir).unwrap();
    assert!(
        i.verify_against(&[pin()]).is_ok(),
        "no policy directory is no policy"
    );
}

#[test]
fn the_real_policy_directory_of_this_machine_is_the_default() {
    assert_eq!(
        ClaudeInstall::new(PathBuf::from("/x")).policy_dir,
        PathBuf::from("/etc/claude-code")
    );
}

#[test]
fn the_credential_file_must_be_private_plain_and_non_empty() {
    let d = dir();
    let mut i = ClaudeInstall::new(d.clone());
    assert!(i.read_key().unwrap().is_none());
    let good = d.join("key");
    fs::write(
        &good,
        format!("{}\n", ["sk", "-ant-api03-", "abc"].concat()),
    )
    .unwrap();
    fs::set_permissions(&good, std::fs::Permissions::from_mode(0o600)).unwrap();
    i.api_key_file = Some(good.clone());
    assert!(i.read_key().unwrap().is_some());
    fs::set_permissions(&good, std::fs::Permissions::from_mode(0o640)).unwrap();
    assert_eq!(i.read_key().err(), Some(Unverified::Credential));
    fs::set_permissions(&good, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = d.join("keylink");
    symlink(&good, &link).unwrap();
    i.api_key_file = Some(link);
    assert_eq!(i.read_key().err(), Some(Unverified::Credential));
    let empty = d.join("empty");
    fs::write(&empty, "").unwrap();
    fs::set_permissions(&empty, std::fs::Permissions::from_mode(0o600)).unwrap();
    i.api_key_file = Some(empty);
    assert_eq!(i.read_key().err(), Some(Unverified::Credential));
    let spaced = d.join("spaced");
    fs::write(&spaced, "two words\n").unwrap();
    fs::set_permissions(&spaced, std::fs::Permissions::from_mode(0o600)).unwrap();
    i.api_key_file = Some(spaced);
    assert_eq!(i.read_key().err(), Some(Unverified::Credential));
    i.api_key_file = Some(d.join("missing"));
    assert_eq!(i.read_key().err(), Some(Unverified::Credential));
}

#[test]
fn the_bridge_is_written_once_exactly_and_refuses_tampering() {
    let parent = dir();
    let a = materialize_bridge(&parent).unwrap();
    assert_eq!(a, materialize_bridge(&parent).unwrap());
    assert_eq!(fs::metadata(&a).unwrap().mode() & 0o777, 0o700);
    for (name, body) in BRIDGE_FILES {
        let m = fs::metadata(a.join(name)).unwrap();
        assert_eq!(m.mode() & 0o777, 0o400, "{name}");
        assert_eq!(fs::read_to_string(a.join(name)).unwrap(), body);
    }
    // Tampered content.
    fs::set_permissions(a.join("bridge.py"), std::fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(a.join("bridge.py"), "import os; os.system('id')").unwrap();
    assert_eq!(materialize_bridge(&parent), Err(E::UnsafePath));
    // An extra file in the directory.
    let parent = dir();
    let a = materialize_bridge(&parent).unwrap();
    fs::write(a.join("sitecustomize.py"), "x").unwrap();
    assert_eq!(materialize_bridge(&parent), Err(E::UnsafePath));
    // A loose directory.
    let parent = dir();
    let a = materialize_bridge(&parent).unwrap();
    fs::set_permissions(&a, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(materialize_bridge(&parent), Err(E::UnsafePath));
}

#[test]
fn the_compiled_in_bridge_is_the_one_in_the_source_tree() {
    for (name, body) in BRIDGE_FILES {
        let on_disk = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("claude-bridge")
                .join(name),
        )
        .unwrap();
        assert_eq!(on_disk, body, "{name}");
    }
}

#[test]
fn session_directories_are_private_per_managed_session_and_the_cwd_is_empty() {
    let state = dir();
    let s = "5f0c1f3e-8a42-4b0e-9a6f-0b6c1d2e3f40";
    let (home, cwd) = session_dirs(&state, s).unwrap();
    for p in [&home, &cwd] {
        assert_eq!(fs::metadata(p).unwrap().mode() & 0o777, 0o700);
        assert!(p.starts_with(state.join("claude").join(s)));
    }
    assert_eq!(
        session_dirs(&state, s).unwrap(),
        (home.clone(), cwd.clone())
    );
    assert_ne!(
        session_dirs(&state, "7a1d2c3b-4e5f-4a6b-8c7d-9e0f1a2b3c4d")
            .unwrap()
            .0,
        home
    );
    fs::write(cwd.join("stray"), "x").unwrap();
    assert_eq!(session_dirs(&state, s).err(), Some(E::UnsafePath));
    for bad in ["../escape", "", "5F0C1F3E-8A42-4B0E-9A6F-0B6C1D2E3F40", "x"] {
        assert_eq!(
            session_dirs(&state, bad).err(),
            Some(E::InvalidRequest),
            "{bad}"
        );
    }
    let state = dir();
    let (home, _) = session_dirs(&state, s).unwrap();
    fs::set_permissions(&home, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(session_dirs(&state, s).err(), Some(E::UnsafePath));
}

fn launcher(v: &Venv) -> (ClaudeLauncher, PathBuf, PathBuf) {
    let (runtime, state) = (dir(), dir());
    let verified = verify(v).unwrap();
    let l = ClaudeLauncher {
        verified,
        bridge_dir: materialize_bridge(&runtime).unwrap(),
        state: state.clone(),
        key: None,
        upstream: None,
    };
    (l, runtime, state)
}

#[test]
fn the_bridge_command_names_only_what_it_needs() {
    let v = venv();
    let (l, _, state) = launcher(&v);
    let (home, cwd) = session_dirs(&state, "5f0c1f3e-8a42-4b0e-9a6f-0b6c1d2e3f40").unwrap();
    let c = l.command(&home, &cwd);
    assert_eq!(c.get_program(), super::super::sandbox::BWRAP);
    let args: Vec<String> = c
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let joined = args.join(" ");
    for must in [
        "--unshare-pid",
        "--die-with-parent",
        "--new-session",
        "--clearenv",
    ] {
        assert!(args.iter().any(|a| a == must), "{must}");
    }
    // The provider needs the network, so it gets the host's; nothing else of the host.
    assert!(!args.iter().any(|a| a == "--unshare-net"));
    assert!(joined.contains(&format!(
        "--ro-bind {} {}",
        v.root.display(),
        v.root.display()
    )));
    assert!(joined.contains(&format!("--bind {} {SANDBOX_HOME}", home.display())));
    assert!(joined.contains(&format!("--bind {} {SANDBOX_CWD}", cwd.display())));
    assert!(
        !joined.contains(&std::env::var("HOME").unwrap_or_else(|_| "/nonexistent-home".into()))
            && !joined.contains(".claude")
            && !joined.contains(".codex")
    );
    for var in ["ANTHROPIC", "TOKEN", "SECRET", "DISPLAY", "DBUS"] {
        assert!(!joined.contains(var), "{var}");
    }
    assert_eq!(
        args[args.len() - 1],
        l.bridge_dir.join("bridge.py").to_str().unwrap()
    );
    assert_eq!(args[args.len() - 2], "-I");
    assert!(args[args.len() - 3].ends_with("/bin/python"));
}

#[test]
fn start_refuses_a_profile_workspace_mismatch_before_any_process_exists() {
    let v = venv();
    let (l, _, state) = launcher(&v);
    let s = "5f0c1f3e-8a42-4b0e-9a6f-0b6c1d2e3f40";
    let c = "7a1d2c3b-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    let id = |m: &str| Identity {
        managed_session: m.into(),
        runtime_instance: "r".into(),
        generation: 1,
    };
    assert_eq!(
        l.start(Profile::Inference, Some("/w"), &id(s), c, None, None)
            .err(),
        Some(E::InvalidRequest)
    );
    assert_eq!(
        l.start(Profile::LocalTools, None, &id(s), c, None, None)
            .err(),
        Some(E::InvalidRequest)
    );
    assert_eq!(
        l.start(Profile::LocalTools, Some("relative"), &id(s), c, None, None)
            .err(),
        Some(E::InvalidRequest)
    );
    assert_eq!(
        l.start(Profile::Inference, None, &id("bad"), c, None, None)
            .err(),
        Some(E::InvalidRequest)
    );
    assert_eq!(
        l.start(Profile::Inference, None, &id(s), "bad", None, None)
            .err(),
        Some(E::InvalidRequest)
    );
    assert!(!state.join("claude/bad").exists());
}

#[test]
fn the_inference_executor_cannot_dispatch_anything() {
    let mut e = NoExecutor;
    let a = super::super::exec::Approved::Command(super::super::exec::ApprovedCommand {
        command: "id".into(),
        cwd: "/".into(),
        workspace: "/".into(),
    });
    assert_eq!(e.dispatch(a), Err(E::Unsupported));
    assert!(!e.busy());
    assert_eq!(e.poll(Instant::now()), None);
    assert_eq!(e.poll_stop(Instant::now()), StopState::Reaped);
}

/// Needs the installation the operator built: `WYSS_CLAUDE_SDK_ROOT=<venv> cargo test -- --ignored`.
#[test]
#[ignore = "needs WYSS_CLAUDE_SDK_ROOT: a virtual environment with claude-agent-sdk==0.2.173"]
fn the_real_installation_verifies_against_the_real_pin() {
    let root = PathBuf::from(std::env::var("WYSS_CLAUDE_SDK_ROOT").expect("WYSS_CLAUDE_SDK_ROOT"));
    let mut i = ClaudeInstall::new(fs::canonicalize(root).unwrap());
    i.policy_dir = dir().join("no-policy");
    let ok = i.verify().unwrap();
    assert_eq!(
        (ok.sdk_version.as_str(), ok.cli_version.as_str()),
        ("0.2.173", "2.1.309")
    );
    assert_eq!(ok.cli_sha256, pins::BUILDS[0].cli_sha256);
}
