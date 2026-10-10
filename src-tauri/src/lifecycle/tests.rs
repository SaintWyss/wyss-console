//! The lifecycle against a scripted link and a scripted service manager, in private
//! temporary directories. Nothing here starts a process: the real ones are in
//! `tests/lifecycle_process.rs`.
use super::{
    install::{self, Change, FileState, UNIT_MARKER},
    manager::parse_status,
    *,
};
use std::{
    fs,
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, PermissionsExt, symlink},
        net::UnixListener,
    },
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};

/// A clean account: empty private XDG roots, and a package beside them.
struct Account {
    root: PathBuf,
}
impl Account {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("wyss-lc-{}", uuid::Uuid::new_v4().simple()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        for dir in ["data", "config", "run", "package/libexec/wyss-console"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .recursive(true)
                .create(root.join(dir))
                .unwrap();
        }
        Self { root }
    }
    fn layout(&self) -> Layout {
        Layout::new(
            self.root.join("data"),
            self.root.join("config"),
            self.root.join("run"),
        )
        .unwrap()
    }
    /// A packaged authority whose content is `bytes`.
    fn package(&self, bytes: &[u8]) -> Package {
        let payload = self.root.join("package/libexec/wyss-console/wyss-control");
        let _ = fs::remove_file(&payload);
        fs::write(&payload, bytes).unwrap();
        fs::set_permissions(&payload, fs::Permissions::from_mode(0o755)).unwrap();
        Package::at(payload, Digest::of(bytes))
    }
    /// Every file and directory below `dir`, with its mode, relative to the account.
    fn tree(&self, dir: &str) -> Vec<(String, u32)> {
        fn walk(root: &Path, at: &Path, out: &mut Vec<(String, u32)>) {
            let mut entries: Vec<_> = fs::read_dir(at).unwrap().flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                let meta = fs::symlink_metadata(entry.path()).unwrap();
                out.push((
                    entry
                        .path()
                        .strip_prefix(root)
                        .unwrap()
                        .display()
                        .to_string(),
                    meta.mode() & 0o7777,
                ));
                if meta.is_dir() {
                    walk(root, &entry.path(), out);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &self.root.join(dir), &mut out);
        out
    }
}
impl Drop for Account {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn inode(path: &Path) -> u64 {
    fs::symlink_metadata(path).unwrap().ino()
}
const OLD: &[u8] = b"#!/bin/sh\n# authority, build one\n";
const NEW: &[u8] = b"#!/bin/sh\n# authority, build two\n";

#[test]
fn a_digest_is_sixty_four_lower_case_hex_digits_or_nothing() {
    let digest = Digest::of(b"abc");
    let hex = format!("{digest:?}");
    assert_eq!(
        hex,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(Digest::parse(&hex), Some(digest));
    for bad in [
        "",
        "ba78",
        &hex[..63],
        &format!("{hex}0"),
        &hex.to_uppercase(),
        &hex.replace('b', "g"),
        &format!(" {}", &hex[1..]),
    ] {
        assert_eq!(Digest::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn a_build_installs_only_the_authority_it_was_packaged_with() {
    // Not packaged: it has no authority to install, whatever lies around it.
    assert_eq!(Provision::of_build(None), Provision::Development);
    // A pin that is not a digest names nothing.
    for bad in [
        "",
        "latest",
        "target/release/wyss-control",
        "../wyss-control",
    ] {
        assert_eq!(Provision::of_build(Some(bad)), Provision::Invalid, "{bad}");
    }
    // A packaged build finds its payload from where it runs, and from nowhere else: not
    // PATH, not the working directory.
    let pin = format!("{:?}", Digest::of(NEW));
    let Provision::Packaged(package) = Provision::of_build(Some(&pin)) else {
        panic!("a pinned build is a packaged one");
    };
    let executable = fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
    let prefix = executable.parent().unwrap().parent().unwrap();
    assert_eq!(
        package,
        Package::at(
            prefix.join("libexec/wyss-console/wyss-control"),
            Digest::of(NEW)
        )
    );
    // Nothing is packaged beside this test binary: the payload is not there to verify.
    assert_eq!(
        package.verified().err(),
        Some(LifecycleError::PackageInvalid)
    );

    let account = Account::new();
    let package = account.package(NEW);
    assert_eq!(
        package.verified().map(|p| p.bytes().to_vec()).as_deref(),
        Ok(NEW)
    );
    let payload = account.root.join("package/libexec/wyss-control-real");
    // Another file under the payload's name is not the authority this build pinned.
    assert_eq!(
        Package::at(
            account
                .root
                .join("package/libexec/wyss-console/wyss-control"),
            Digest::of(OLD)
        )
        .verified()
        .err(),
        Some(LifecycleError::PackageInvalid)
    );
    // A link in the payload's place is not followed, wherever it points.
    fs::rename(
        account
            .root
            .join("package/libexec/wyss-console/wyss-control"),
        &payload,
    )
    .unwrap();
    symlink(
        &payload,
        account
            .root
            .join("package/libexec/wyss-console/wyss-control"),
    )
    .unwrap();
    assert_eq!(
        package.verified().err(),
        Some(LifecycleError::PackageInvalid)
    );
    // A payload others can write, or in a directory others can write, proves nothing.
    let package = account.package(NEW);
    let file = account
        .root
        .join("package/libexec/wyss-console/wyss-control");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o757)).unwrap();
    assert_eq!(
        package.verified().err(),
        Some(LifecycleError::PackageInvalid)
    );
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        package.verified().map(|p| p.bytes().to_vec()).as_deref(),
        Ok(NEW)
    );
    let dir = account.root.join("package/libexec/wyss-console");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o777)).unwrap();
    assert_eq!(
        package.verified().err(),
        Some(LifecycleError::PackageInvalid)
    );
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn the_layout_is_two_fixed_files_under_the_users_xdg_directories() {
    let layout = Layout::new(
        "/h/.local/share".into(),
        "/h/.config".into(),
        "/run/u".into(),
    )
    .unwrap();
    assert_eq!(
        layout.binary(),
        Path::new("/h/.local/share/wyss-console/authority/wyss-control")
    );
    assert_eq!(
        layout.unit(),
        Path::new("/h/.config/systemd/user/wyss-control.service")
    );
    assert_eq!(
        layout.socket(),
        Path::new("/run/u/wyss-console/control.sock")
    );
    assert_eq!(UNIT_NAME, "wyss-control.service");
    // A root that is relative, climbs, or could not be written into a unit is no root.
    for bad in ["relative/share", "/h/../etc", "/h/sha\nre", ""] {
        assert_eq!(
            Layout::new(bad.into(), "/h/.config".into(), "/run/u".into()),
            Err(LifecycleError::UnsafePath),
            "{bad:?}"
        );
        assert_eq!(
            Layout::new("/h/.local/share".into(), bad.into(), "/run/u".into()),
            Err(LifecycleError::UnsafePath),
            "{bad:?}"
        );
    }
}

#[test]
fn the_unit_is_the_same_bytes_for_the_same_place_and_names_one_fixed_binary() {
    let text = install::unit_text(Path::new(
        "/h/.local/share/wyss-console/authority/wyss-control",
    ))
    .unwrap();
    assert_eq!(
        text,
        install::unit_text(Path::new(
            "/h/.local/share/wyss-console/authority/wyss-control"
        ))
        .unwrap()
    );
    assert_eq!(text.lines().next(), Some(UNIT_MARKER));
    let line = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .unwrap_or_else(|| panic!("{key} is not in the unit"))
    };
    // One executable, by its whole path, with no shell and no argument.
    assert_eq!(
        line("ExecStart"),
        "\"/h/.local/share/wyss-console/authority/wyss-control\""
    );
    assert_eq!(text.matches("ExecStart").count(), 1);
    for word in [
        "/bin/sh",
        "sh -c",
        "bash",
        "ExecStartPre",
        "ExecStartPost",
        "ExecStop",
        "ExecReload",
    ] {
        assert!(!text.contains(word), "{word}");
    }
    // A crash is restarted, a crash loop ends, and what cannot succeed is not retried.
    assert_eq!(line("Restart"), "on-failure");
    assert_eq!(line("RestartPreventExitStatus"), "2");
    assert_eq!(
        (line("StartLimitBurst"), line("StartLimitIntervalSec")),
        ("5", "120")
    );
    // Stopping signals the authority alone; it stops its runtime itself.
    assert_eq!(line("KillMode"), "mixed");
    // Started on demand: there is nothing to enable.
    assert!(!text.contains("[Install]") && !text.contains("WantedBy"));
    // No root, no privilege to gain, nothing others can read.
    assert_eq!((line("NoNewPrivileges"), line("UMask")), ("yes", "0077"));
    assert!(!text.contains("User=") && !text.contains("sudo"));
    // What would break the isolation Codex runs in is not there.
    for breaking in [
        "RestrictSUIDSGID",
        "RestrictNamespaces",
        "PrivateUsers",
        "ProtectHome",
        "MemoryDenyWriteExecute",
    ] {
        assert!(!text.contains(breaking), "{breaking}");
    }
    // The loader is not told what to load, and the desktop's environment is not passed.
    assert!(line("UnsetEnvironment").contains("LD_PRELOAD"));
    assert!(!text.contains("\nEnvironment=") && !text.contains("PassEnvironment"));

    // A path with characters systemd reads specially is named literally.
    let odd = install::unit_text(Path::new("/h/My \"Data\" 100%/$HOME/a\\b/wyss-control")).unwrap();
    assert!(odd.contains("ExecStart=\"/h/My \\\"Data\\\" 100%%/$$HOME/a\\\\b/wyss-control\"\n"));
    for bad in [
        "relative/wyss-control",
        "/h/a\nExecStartPre=/bin/sh/wyss-control",
    ] {
        assert_eq!(
            install::unit_text(Path::new(bad)),
            Err(LifecycleError::UnsafePath),
            "{bad:?}"
        );
    }
}

#[test]
fn a_clean_account_gets_exactly_two_files_and_a_second_install_writes_nothing() {
    let account = Account::new();
    let layout = account.layout();
    assert_eq!(
        install::inspect(&layout, Some(Digest::of(NEW))),
        install::Installed {
            binary: FileState::Absent,
            unit: FileState::Absent
        }
    );
    let change = install::ensure(&layout, &Payload::of(NEW.to_vec())).unwrap();
    assert_eq!(
        change,
        Change {
            binary: true,
            unit: true
        }
    );
    // Exactly these, private to the user: the binary, the installer's lock, the unit.
    assert_eq!(
        account.tree("data"),
        [
            ("data/wyss-console".to_owned(), 0o700),
            ("data/wyss-console/authority".to_owned(), 0o700),
            (
                "data/wyss-console/authority/.install.lock".to_owned(),
                0o600
            ),
            ("data/wyss-console/authority/wyss-control".to_owned(), 0o700),
        ]
    );
    assert_eq!(
        account.tree("config"),
        [
            ("config/systemd".to_owned(), 0o700),
            ("config/systemd/user".to_owned(), 0o700),
            ("config/systemd/user/wyss-control.service".to_owned(), 0o600),
        ]
    );
    assert_eq!(fs::read(layout.binary()).unwrap(), NEW);
    assert_eq!(
        fs::read_to_string(layout.unit()).unwrap(),
        install::unit_text(&layout.binary()).unwrap()
    );
    assert_eq!(
        install::inspect(&layout, Some(Digest::of(NEW))),
        install::Installed {
            binary: FileState::Current,
            unit: FileState::Current
        }
    );
    // Current: nothing is written again, not even the same bytes.
    let (binary, unit) = (inode(&layout.binary()), inode(&layout.unit()));
    assert_eq!(
        install::ensure(&layout, &Payload::of(NEW.to_vec())),
        Ok(Change::default())
    );
    assert_eq!(
        (inode(&layout.binary()), inode(&layout.unit())),
        (binary, unit)
    );
}

#[test]
fn an_update_replaces_the_file_whole_and_never_the_one_that_is_running() {
    let account = Account::new();
    let layout = account.layout();
    install::ensure(&layout, &Payload::of(OLD.to_vec())).unwrap();
    // A process that runs the old authority holds its file.
    let running = fs::File::open(layout.binary()).unwrap();
    let old = inode(&layout.binary());
    assert_eq!(
        install::inspect(&layout, Some(Digest::of(NEW))).binary,
        FileState::Outdated
    );
    // A build that carries no authority cannot call one current.
    assert_eq!(install::inspect(&layout, None).binary, FileState::Outdated);
    assert_eq!(
        install::ensure(&layout, &Payload::of(NEW.to_vec())),
        Ok(Change {
            binary: true,
            unit: false
        })
    );
    // Another file took the name. The old one was not written to: what runs is unchanged.
    assert_ne!(inode(&layout.binary()), old);
    assert_eq!(fs::read(layout.binary()).unwrap(), NEW);
    assert_eq!(running.metadata().unwrap().ino(), old);
    let mut still = Vec::new();
    std::io::Read::read_to_end(&mut &running, &mut still).unwrap();
    assert_eq!(still, OLD);
    assert_eq!(
        fs::symlink_metadata(layout.binary()).unwrap().mode() & 0o777,
        0o700
    );
    // No temporary file is left beside it.
    assert_eq!(account.tree("data").len(), 4);

    // A unit this product wrote in another shape is this product's to rewrite.
    fs::write(
        layout.unit(),
        format!("{UNIT_MARKER}\n[Service]\nExecStart=/old/place\n"),
    )
    .unwrap();
    assert_eq!(install::inspect(&layout, None).unit, FileState::Outdated);
    assert_eq!(
        install::ensure(&layout, &Payload::of(NEW.to_vec())),
        Ok(Change {
            binary: false,
            unit: true
        })
    );
    assert_eq!(
        install::inspect(&layout, Some(Digest::of(NEW))).unit,
        FileState::Current
    );
}

#[test]
fn what_an_interrupted_install_left_is_cleared_and_the_install_completes() {
    let account = Account::new();
    let layout = account.layout();
    // Interrupted after the temporary binary was written: nothing has the real name.
    install::ensure(&layout, &Payload::of(OLD.to_vec())).unwrap();
    fs::remove_file(layout.binary()).unwrap();
    fs::remove_file(layout.unit()).unwrap();
    let stray_binary = layout.binary_dir().join(".wyss-control.0123abcd.tmp");
    let stray_unit = layout.unit_dir().join(".wyss-control.service.0123abcd.tmp");
    fs::write(&stray_binary, b"half a bina").unwrap();
    fs::write(&stray_unit, b"[Serv").unwrap();
    // Somebody else's files in the shared unit directory are nobody's to remove.
    let neighbour = layout.unit_dir().join("other.service");
    let lookalike = layout.unit_dir().join(".wyss-control.service.keep");
    fs::write(&neighbour, b"[Service]\n").unwrap();
    fs::write(&lookalike, b"x").unwrap();
    assert_eq!(
        install::inspect(&layout, Some(Digest::of(NEW))),
        install::Installed {
            binary: FileState::Absent,
            unit: FileState::Absent
        }
    );
    assert_eq!(
        install::ensure(&layout, &Payload::of(NEW.to_vec())),
        Ok(Change {
            binary: true,
            unit: true
        })
    );
    assert!(!stray_binary.exists() && !stray_unit.exists());
    assert!(neighbour.exists() && lookalike.exists());
    assert_eq!(fs::read(layout.binary()).unwrap(), NEW);

    // Interrupted after the binary was renamed and before the unit was written.
    fs::remove_file(layout.unit()).unwrap();
    assert_eq!(
        install::ensure(&layout, &Payload::of(NEW.to_vec())),
        Ok(Change {
            binary: false,
            unit: true
        })
    );
    // Interrupted after the unit was written, with the binary of another build.
    install::binary(&layout, OLD).unwrap();
    assert_eq!(
        install::ensure(&layout, &Payload::of(NEW.to_vec())),
        Ok(Change {
            binary: true,
            unit: false
        })
    );
}

#[test]
fn a_place_that_is_not_safely_the_users_is_refused_and_left_as_it_is() {
    let unsafe_path = Err(LifecycleError::UnsafePath);
    let fresh = || {
        let account = Account::new();
        let layout = account.layout();
        install::ensure(&layout, &Payload::of(OLD.to_vec())).unwrap();
        (account, layout)
    };
    // A link where the managed binary goes: not followed, not replaced, target untouched.
    let (account, layout) = fresh();
    let elsewhere = account.root.join("elsewhere");
    fs::write(&elsewhere, b"not ours").unwrap();
    fs::remove_file(layout.binary()).unwrap();
    symlink(&elsewhere, layout.binary()).unwrap();
    assert_eq!(
        install::inspect(&layout, Some(Digest::of(NEW))).binary,
        FileState::Unsafe
    );
    assert_eq!(
        install::ensure(&layout, &Payload::of(NEW.to_vec())),
        unsafe_path
    );
    assert_eq!(fs::read(&elsewhere).unwrap(), b"not ours");
    assert!(
        fs::symlink_metadata(layout.binary())
            .unwrap()
            .file_type()
            .is_symlink()
    );

    // A second name for the managed binary: it could be changed under the other one.
    let (account, layout) = fresh();
    fs::hard_link(layout.binary(), account.root.join("second-name")).unwrap();
    assert_eq!(
        install::inspect(&layout, Some(Digest::of(OLD))).binary,
        FileState::Unsafe
    );
    assert_eq!(
        install::ensure(&layout, &Payload::of(NEW.to_vec())),
        unsafe_path
    );
    assert_eq!(fs::read(layout.binary()).unwrap(), OLD);

    // A managed binary others can write.
    let (_account, layout) = fresh();
    fs::set_permissions(layout.binary(), fs::Permissions::from_mode(0o722)).unwrap();
    assert_eq!(
        install::inspect(&layout, Some(Digest::of(OLD))).binary,
        FileState::Unsafe
    );
    assert_eq!(
        install::ensure(&layout, &Payload::of(NEW.to_vec())),
        unsafe_path
    );

    // A directory on the way that others can write: whoever can write it can swap the
    // binary, whatever the file itself says. Nothing is written below it.
    for (dir, mode) in [
        ("data/wyss-console/authority", 0o777),
        ("data/wyss-console", 0o770),
        ("data", 0o777),
    ] {
        let (account, layout) = fresh();
        fs::set_permissions(account.root.join(dir), fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(
            install::inspect(&layout, Some(Digest::of(OLD))).binary,
            FileState::Unsafe,
            "{dir}"
        );
        assert_eq!(
            install::ensure(&layout, &Payload::of(NEW.to_vec())),
            unsafe_path,
            "{dir}"
        );
        assert_eq!(fs::read(layout.binary()).unwrap(), OLD, "{dir}");
        fs::set_permissions(account.root.join(dir), fs::Permissions::from_mode(0o700)).unwrap();
    }
    // The managed directory itself replaced by a link to another place.
    let (account, layout) = fresh();
    let real = account.root.join("data/wyss-console/real");
    fs::rename(layout.binary_dir(), &real).unwrap();
    symlink(&real, layout.binary_dir()).unwrap();
    assert_eq!(
        install::inspect(&layout, Some(Digest::of(OLD))).binary,
        FileState::Unsafe
    );
    assert_eq!(
        install::ensure(&layout, &Payload::of(NEW.to_vec())),
        unsafe_path
    );
    assert_eq!(fs::read(real.join("wyss-control")).unwrap(), OLD);

    // The unit: a link, a file others can write, a unit directory that is a link.
    let (account, layout) = fresh();
    let target = account.root.join("unit-elsewhere");
    fs::write(&target, install::unit_text(&layout.binary()).unwrap()).unwrap();
    fs::remove_file(layout.unit()).unwrap();
    symlink(&target, layout.unit()).unwrap();
    assert_eq!(install::inspect(&layout, None).unit, FileState::Unsafe);
    assert_eq!(
        install::ensure(&layout, &Payload::of(NEW.to_vec())),
        unsafe_path
    );
    // Refused before anything was written: the binary is still the old one.
    assert_eq!(fs::read(layout.binary()).unwrap(), OLD);
    let (_account, layout) = fresh();
    fs::set_permissions(layout.unit(), fs::Permissions::from_mode(0o666)).unwrap();
    assert_eq!(install::inspect(&layout, None).unit, FileState::Unsafe);
    assert_eq!(
        install::ensure(&layout, &Payload::of(NEW.to_vec())),
        unsafe_path
    );
    let (account, layout) = fresh();
    let real = account.root.join("config/systemd/real-user");
    fs::rename(layout.unit_dir(), &real).unwrap();
    symlink(&real, layout.unit_dir()).unwrap();
    assert_eq!(install::inspect(&layout, None).unit, FileState::Unsafe);
    assert_eq!(
        install::ensure(&layout, &Payload::of(NEW.to_vec())),
        unsafe_path
    );

    // A home whose parent others can write: nothing is created below it, not even the
    // directory that would have been refused next.
    let account = Account::new();
    let open = account.root.join("open");
    fs::create_dir(&open).unwrap();
    fs::set_permissions(&open, fs::Permissions::from_mode(0o777)).unwrap();
    let layout = Layout::new(
        open.join("share"),
        open.join("config"),
        account.root.join("run"),
    )
    .unwrap();
    assert_eq!(
        install::ensure(&layout, &Payload::of(NEW.to_vec())),
        unsafe_path
    );
    assert_eq!(fs::read_dir(&open).unwrap().count(), 0);
    fs::set_permissions(&open, fs::Permissions::from_mode(0o700)).unwrap();

    // A unit of that name this product did not write is not this product's to replace.
    let (_account, layout) = fresh();
    let theirs = "[Service]\nExecStart=/usr/bin/something-else\n";
    fs::write(layout.unit(), theirs).unwrap();
    assert_eq!(install::inspect(&layout, None).unit, FileState::Foreign);
    assert_eq!(
        install::ensure(&layout, &Payload::of(NEW.to_vec())),
        Err(LifecycleError::Foreign)
    );
    assert_eq!(fs::read_to_string(layout.unit()).unwrap(), theirs);
    assert_eq!(fs::read(layout.binary()).unwrap(), OLD);
}

#[test]
fn installers_that_run_at_once_leave_one_whole_installation() {
    let account = Account::new();
    let layout = account.layout();
    let changes: Vec<Change> = thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|i| {
                let layout = layout.clone();
                scope.spawn(move || {
                    let bytes = if i % 2 == 0 { NEW } else { OLD };
                    install::ensure(&layout, &Payload::of(bytes.to_vec())).unwrap()
                })
            })
            .collect();
        workers.into_iter().map(|w| w.join().unwrap()).collect()
    });
    assert_eq!(changes.len(), 8);
    // Whichever finished last, what is there is one of the two, whole, and nothing else.
    let installed = fs::read(layout.binary()).unwrap();
    assert!(installed == NEW || installed == OLD);
    assert_eq!(
        fs::read_to_string(layout.unit()).unwrap(),
        install::unit_text(&layout.binary()).unwrap()
    );
    assert_eq!(account.tree("data").len(), 4);
    assert_eq!(account.tree("config").len(), 3);
    // The unit is the same for both builds: it was written once.
    assert_eq!(changes.iter().filter(|c| c.unit).count(), 1);
}

#[test]
fn what_the_service_manager_says_is_read_word_by_word_and_nothing_is_assumed() {
    let running = parse_status(
        "LoadState=loaded\nActiveState=active\nResult=success\nMainPID=4242\nFragmentPath=/h/.config/systemd/user/wyss-control.service\n",
    );
    assert_eq!(
        running,
        UnitStatus {
            load: LoadState::Loaded,
            active: ActiveState::Active,
            result: UnitResult::Success,
            main_pid: Some(4242),
            fragment: Some("/h/.config/systemd/user/wyss-control.service".into()),
        }
    );
    assert!(running.in_hand());
    let absent = parse_status(
        "LoadState=not-found\nActiveState=inactive\nResult=success\nMainPID=0\nFragmentPath=\n",
    );
    assert_eq!(
        (absent.load, absent.main_pid, absent.fragment.as_ref()),
        (LoadState::NotFound, None, None)
    );
    assert!(!absent.in_hand());
    let restarting =
        parse_status("LoadState=loaded\nActiveState=activating\nResult=exit-code\nMainPID=0\n");
    assert!(restarting.in_hand());
    assert_eq!(restarting.result, UnitResult::Other);
    let limited =
        parse_status("LoadState=loaded\nActiveState=failed\nResult=start-limit-hit\nMainPID=0\n");
    assert_eq!(
        (limited.active, limited.result),
        (ActiveState::Failed, UnitResult::StartLimitHit)
    );
    assert!(!limited.in_hand());
    // Words this build does not know, and keys that are missing, are not taken for good ones.
    let odd = parse_status(
        "LoadState=masked\nActiveState=reloading\nResult=oom-kill\nMainPID=abc\nFragmentPath=relative\nActiveStateX=active\n",
    );
    assert_eq!(
        odd,
        UnitStatus {
            load: LoadState::Other,
            active: ActiveState::Other,
            result: UnitResult::Other,
            main_pid: None,
            fragment: None
        }
    );
    assert_eq!(parse_status(""), odd);
    assert!(!odd.in_hand());
    // No manager answers where there is none: every question says so.
    let nowhere = Systemd::new(Path::new("/nonexistent/wyss-no-manager"));
    assert_eq!(nowhere.status(), Err(ManagerError::Unavailable));
    assert_eq!(nowhere.start(), Err(ManagerError::Unavailable));
    assert_eq!(nowhere.reload(), Err(ManagerError::Unavailable));
    assert_eq!(nowhere.reset_failed(), Err(ManagerError::Unavailable));
}

#[test]
fn the_service_manager_is_asked_fixed_things_by_a_fixed_program_with_no_shell() {
    // The program is the system's own, by its whole path: never a name looked up in PATH.
    let real = Systemd::new(Path::new("/nonexistent/wyss-no-manager"));
    let executable = real.executable().expect("this system has systemctl");
    assert!(executable.is_absolute());
    assert!(
        ["/usr/bin/systemctl", "/bin/systemctl"]
            .iter()
            .any(|c| fs::canonicalize(c).is_ok_and(|p| p == executable))
    );
    let meta = fs::metadata(executable).unwrap();
    assert_eq!((meta.uid(), meta.mode() & 0o022), (0, 0));

    // A stand-in that writes down how it was run, and answers like a manager would.
    let account = Account::new();
    let record = account.root.join("calls");
    let stand_in = account.root.join("stand-in");
    fs::write(
        &stand_in,
        format!(
            "#!/bin/sh\n{{ printf 'ARGS'; for a in \"$@\"; do printf ' [%s]' \"$a\"; done; echo; env | sed 's/=.*//' | sort | tr '\\n' ' '; echo; }} >> {}\nprintf 'LoadState=loaded\\nActiveState=inactive\\nResult=success\\nMainPID=0\\nFragmentPath=\\nVersion=1\\n'\n",
            record.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&stand_in, fs::Permissions::from_mode(0o700)).unwrap();
    let manager = Systemd::with_executable(&stand_in, &account.root.join("run"));
    assert!(manager.status().is_ok());
    manager.reload().unwrap();
    manager.start().unwrap();
    manager.reset_failed().unwrap();
    let recorded = fs::read_to_string(&record).unwrap();
    let lines: Vec<&str> = recorded.lines().collect();
    // Four things, each a fixed argument list about the one unit. Nothing is quoted into
    // a shell, and nothing of it can be chosen by a caller.
    assert_eq!(
        lines.iter().step_by(2).copied().collect::<Vec<_>>(),
        [
            "ARGS [--user] [--no-ask-password] [show] [wyss-control.service] [--property=LoadState,ActiveState,Result,MainPID,FragmentPath]",
            "ARGS [--user] [--no-ask-password] [daemon-reload]",
            "ARGS [--user] [--no-ask-password] [start] [wyss-control.service]",
            "ARGS [--user] [--no-ask-password] [reset-failed] [wyss-control.service]",
        ]
    );
    // It is given where the manager is and plain output, and nothing of the desktop's
    // environment: not HOME, not PATH, not what a session may carry for the loader.
    assert!(std::env::var_os("PATH").is_some() && std::env::var_os("HOME").is_some());
    for environment in lines.iter().skip(1).step_by(2) {
        let given: Vec<&str> = environment
            .split(' ')
            .filter(|name| !name.is_empty() && !["PWD", "SHLVL", "_", "OLDPWD"].contains(name))
            .collect();
        assert_eq!(given, ["LC_ALL", "XDG_RUNTIME_DIR"]);
    }
    // There is no way to ask for anything else. The manager is never told to stop,
    // restart, kill, enable or disable anything, by this or any other code here.
    let source = include_str!("manager.rs");
    for verb in [
        "\"stop\"",
        "\"restart\"",
        "\"kill\"",
        "\"enable\"",
        "\"disable\"",
        "\"mask\"",
        "\"edit\"",
        "\"set-environment\"",
        "\"import-environment\"",
        "\"-c\"",
        "/bin/sh",
        "bash",
    ] {
        assert!(!source.contains(verb), "{verb}");
    }
    for source in [
        include_str!("bootstrap.rs"),
        include_str!("install.rs"),
        include_str!("package.rs"),
        include_str!("layout.rs"),
    ] {
        assert!(!source.contains("Command::new") && !source.contains("std::process"));
    }
}

// --- the state machine -----------------------------------------------------------

/// A link the test moves by hand.
struct Link {
    state: Mutex<(u64, LinkState)>,
    changed: Condvar,
    /// Becomes this the next time it is asked how it is now (and not what changed).
    turns: Mutex<Option<LinkState>>,
}
impl Link {
    fn new(state: LinkState) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new((0, state)),
            changed: Condvar::new(),
            turns: Mutex::new(None),
        })
    }
    fn set(&self, state: LinkState) {
        let mut held = self.state.lock().unwrap();
        *held = (held.0 + 1, state);
        self.changed.notify_all();
    }
}
impl AuthorityLink for Link {
    fn observe(&self, after: Option<u64>, timeout: Duration) -> (u64, LinkState) {
        let deadline = Instant::now() + timeout;
        // An attempt asks how the link is now, once, before it does anything.
        if after.is_none()
            && timeout.is_zero()
            && let Some(next) = self.turns.lock().unwrap().take()
        {
            self.set(next);
        }
        let mut held = self.state.lock().unwrap();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if after != Some(held.0) || left.is_zero() {
                return *held;
            }
            held = self.changed.wait_timeout(held, left).unwrap().0;
        }
    }
}

type OnStart = Box<dyn Fn(&mut Result<UnitStatus, ManagerError>) + Send>;
type Watcher = Box<dyn Fn(&'static str) + Send>;
/// A service manager that records what it was asked and answers what the test says.
struct Manager {
    status: Mutex<Result<UnitStatus, ManagerError>>,
    calls: Mutex<Vec<&'static str>>,
    start: Mutex<Result<(), ManagerError>>,
    on_start: Mutex<Option<OnStart>>,
    /// Told each thing that is asked, as it is asked.
    watch: Mutex<Option<Watcher>>,
}
fn unit(active: ActiveState, result: UnitResult) -> UnitStatus {
    UnitStatus {
        load: LoadState::Loaded,
        active,
        result,
        main_pid: None,
        fragment: None,
    }
}
fn not_installed() -> UnitStatus {
    UnitStatus {
        load: LoadState::NotFound,
        ..unit(ActiveState::Inactive, UnitResult::Success)
    }
}
impl Manager {
    fn new(status: Result<UnitStatus, ManagerError>) -> Arc<Self> {
        Arc::new(Self {
            status: Mutex::new(status),
            calls: Mutex::new(Vec::new()),
            start: Mutex::new(Ok(())),
            on_start: Mutex::new(None),
            watch: Mutex::new(None),
        })
    }
    fn set(&self, status: Result<UnitStatus, ManagerError>) {
        *self.status.lock().unwrap() = status;
    }
    fn on_start(&self, effect: impl Fn(&mut Result<UnitStatus, ManagerError>) + Send + 'static) {
        *self.on_start.lock().unwrap() = Some(Box::new(effect));
    }
    fn watch(&self, watcher: impl Fn(&'static str) + Send + 'static) {
        *self.watch.lock().unwrap() = Some(Box::new(watcher));
    }
    fn note(&self, call: &'static str) {
        self.calls.lock().unwrap().push(call);
        if let Some(watcher) = self.watch.lock().unwrap().as_ref() {
            watcher(call);
        }
    }
    /// What was asked that changes something, in order.
    fn asked(&self) -> Vec<&'static str> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .copied()
            .filter(|call| *call != "status")
            .collect()
    }
    fn count(&self, call: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| **c == call)
            .count()
    }
}
impl ServiceManager for Manager {
    fn status(&self) -> Result<UnitStatus, ManagerError> {
        self.calls.lock().unwrap().push("status");
        self.status.lock().unwrap().clone()
    }
    fn reload(&self) -> Result<(), ManagerError> {
        self.note("reload");
        Ok(())
    }
    fn start(&self) -> Result<(), ManagerError> {
        self.note("start");
        let result = *self.start.lock().unwrap();
        if result.is_ok()
            && let Some(effect) = self.on_start.lock().unwrap().as_ref()
        {
            effect(&mut self.status.lock().unwrap());
        }
        result
    }
    fn reset_failed(&self) -> Result<(), ManagerError> {
        self.note("reset-failed");
        Ok(())
    }
}

const QUICK: Limits = Limits {
    startup: Duration::from_millis(900),
    first_check: Duration::from_millis(40),
};
struct World {
    account: Account,
    link: Arc<Link>,
    manager: Arc<Manager>,
    bootstrap: AuthorityBootstrap,
}
impl World {
    fn new(
        link: LinkState,
        provision: impl FnOnce(&Account) -> Provision,
        status: Result<UnitStatus, ManagerError>,
    ) -> Self {
        let account = Account::new();
        let link = Link::new(link);
        let manager = Manager::new(status);
        let bootstrap = AuthorityBootstrap::start_with(
            link.clone(),
            provision(&account),
            Ok(account.layout()),
            manager.clone(),
            QUICK,
        );
        Self {
            account,
            link,
            manager,
            bootstrap,
        }
    }
    fn packaged(link: LinkState, status: Result<UnitStatus, ManagerError>) -> Self {
        Self::new(
            link,
            |account| Provision::Packaged(account.package(NEW)),
            status,
        )
    }
    /// Follows the view until `done`, and answers every phase it went through.
    fn until(&self, done: impl Fn(&LifecycleView) -> bool) -> (LifecycleView, Vec<Phase>) {
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut phases = Vec::new();
        let mut after = None;
        loop {
            let view = self
                .bootstrap
                .wait_changed(after, Duration::from_millis(200));
            // Whatever is going on, a view never contradicts itself.
            assert_eq!(
                view.failure.is_some(),
                view.phase == Phase::Failed,
                "{view:?}"
            );
            assert_eq!(view.retry, view.phase == Phase::Failed, "{view:?}");
            assert!(
                view.origin.is_none() || matches!(view.phase, Phase::Ready | Phase::Incompatible),
                "{view:?}"
            );
            assert!(
                !view.update_pending
                    || (view.phase == Phase::Ready && view.origin == Some(Origin::Managed)),
                "{view:?}"
            );
            if phases.last() != Some(&view.phase) {
                phases.push(view.phase);
            }
            if done(&view) {
                return (view, phases);
            }
            assert!(
                Instant::now() < deadline,
                "stuck at {view:?} after {phases:?}"
            );
            after = Some(view.revision);
        }
    }
    fn failed(&self) -> LifecycleView {
        self.until(|v| v.phase == Phase::Failed).0
    }
    fn ready(&self) -> LifecycleView {
        self.until(|v| v.phase == Phase::Ready).0
    }
    fn installed(&self) -> bool {
        self.account.layout().binary().exists() || self.account.layout().unit().exists()
    }
    /// Lets anything that would still happen, happen.
    fn quiet(&self) {
        thread::sleep(Duration::from_millis(150));
    }
}
impl Drop for World {
    fn drop(&mut self) {
        self.link.set(LinkState::Stopped);
        self.bootstrap.shutdown();
    }
}

#[test]
fn an_authority_that_already_answers_is_used_and_nothing_is_installed_or_started() {
    let world = World::packaged(LinkState::Connected, Ok(not_installed()));
    let view = world.ready();
    assert_eq!(
        (view.failure, view.retry, view.update_pending),
        (None, false, false)
    );
    assert_eq!(view.diagnostics.build, BuildKind::Packaged);
    assert_eq!(view.diagnostics.installation, InstallationState::Absent);
    assert_eq!(view.diagnostics.service, ServiceState::NotInstalled);
    world.quiet();
    // It was asked how the unit is, for the diagnostics, and nothing else.
    assert_eq!(world.manager.asked(), Vec::<&str>::new());
    assert!(!world.installed());
    // Once ready nothing is looked at again: no timer reads the unit or the files.
    let looks = world.manager.count("status");
    thread::sleep(Duration::from_millis(400));
    assert_eq!(world.manager.count("status"), looks);

    // The same for an authority that starts answering just as an attempt begins:
    // somebody else brought it up, and it is theirs.
    let world = World::packaged(LinkState::Connecting, Ok(not_installed()));
    *world.link.turns.lock().unwrap() = Some(LinkState::Connected);
    world.link.set(LinkState::Absent);
    world.ready();
    world.quiet();
    assert_eq!(world.manager.asked(), Vec::<&str>::new());
    assert!(!world.installed());
    // Retry has nothing to retry.
    let before = world.bootstrap.view();
    assert_eq!(world.bootstrap.retry(), before);
    world.quiet();
    assert_eq!(world.manager.asked(), Vec::<&str>::new());
}

#[test]
fn with_no_authority_it_installs_starts_and_is_ready_only_when_the_link_connects() {
    let world = World::packaged(LinkState::Connecting, Ok(not_installed()));
    assert_eq!(world.bootstrap.view().phase, Phase::Checking);
    world.quiet();
    // Nothing happens before the link has said that nothing answers.
    assert!(!world.installed());
    assert_eq!(world.manager.asked(), Vec::<&str>::new());

    // The unit runs once started. Nobody connects the link yet.
    world
        .manager
        .on_start(|status| *status = Ok(unit(ActiveState::Active, UnitResult::Success)));
    // The phase the window is shown at the moment each thing is asked of the manager.
    let shown = Arc::new(Mutex::new(Vec::new()));
    let (seen, bootstrap) = (shown.clone(), world.bootstrap.clone());
    world
        .manager
        .watch(move |call| seen.lock().unwrap().push((call, bootstrap.view().phase)));
    world.link.set(LinkState::Absent);
    let (view, phases) = world.until(|v| v.phase == Phase::Connecting);
    assert_eq!(
        *shown.lock().unwrap(),
        [("reload", Phase::Installing), ("start", Phase::Starting)]
    );
    // Whatever a reader happened to see of it, it saw it in this order.
    let order = [
        Phase::Checking,
        Phase::Installing,
        Phase::Starting,
        Phase::Connecting,
    ];
    let mut rest = order.iter();
    assert!(phases.iter().all(|p| rest.any(|o| o == p)), "{phases:?}");
    assert_eq!((view.failure, view.retry, view.origin), (None, false, None));
    assert_eq!(world.manager.asked(), ["reload", "start"]);
    assert_eq!(fs::read(world.account.layout().binary()).unwrap(), NEW);
    // A unit that runs is not an authority that is ready.
    world.quiet();
    assert_eq!(world.bootstrap.view().phase, Phase::Connecting);
    // The link connects: that, and only that, is ready.
    world.link.set(LinkState::Connected);
    let view = world.ready();
    assert_eq!(view.diagnostics.installation, InstallationState::Current);
    assert_eq!(view.diagnostics.service, ServiceState::Running);
    world.quiet();
    assert_eq!(world.manager.asked(), ["reload", "start"]);
}

#[test]
fn a_build_that_carries_no_authority_installs_and_starts_none() {
    let world = World::new(
        LinkState::Absent,
        |_| Provision::Development,
        Ok(not_installed()),
    );
    let view = world.failed();
    assert_eq!(
        (view.failure, view.retry),
        (Some(Failure::NotPackaged), true)
    );
    assert_eq!(view.diagnostics.build, BuildKind::Development);
    world.bootstrap.retry();
    world.until(|v| v.phase == Phase::Checking || v.revision > view.revision);
    assert_eq!(world.failed().failure, Some(Failure::NotPackaged));
    assert_eq!(world.manager.asked(), Vec::<&str>::new());
    assert!(!world.installed());
    // Its developer starts one: it is used.
    world.link.set(LinkState::Connected);
    assert_eq!(world.ready().diagnostics.build, BuildKind::Development);

    // A build that says it is packaged and whose payload is not what it was built with.
    let world = World::new(
        LinkState::Absent,
        |account| {
            account.package(OLD);
            Provision::Packaged(Package::at(
                account
                    .root
                    .join("package/libexec/wyss-console/wyss-control"),
                Digest::of(NEW),
            ))
        },
        Ok(not_installed()),
    );
    assert_eq!(world.failed().failure, Some(Failure::PackageInvalid));
    assert!(!world.installed());
    assert_eq!(world.manager.asked(), Vec::<&str>::new());
    let world = World::new(
        LinkState::Absent,
        |_| Provision::Invalid,
        Ok(not_installed()),
    );
    assert_eq!(world.failed().failure, Some(Failure::PackageInvalid));
}

#[test]
fn without_a_service_manager_nothing_is_written_and_the_failure_is_said() {
    let world = World::packaged(LinkState::Absent, Err(ManagerError::Unavailable));
    let view = world.failed();
    assert_eq!(
        (view.failure, view.retry),
        (Some(Failure::ManagerUnavailable), true)
    );
    assert_eq!(view.diagnostics.service, ServiceState::Unavailable);
    assert!(!world.installed());
    assert_eq!(world.manager.asked(), Vec::<&str>::new());

    // A manager that is there and does not start it.
    let world = World::packaged(LinkState::Absent, Ok(not_installed()));
    *world.manager.start.lock().unwrap() = Err(ManagerError::Refused);
    assert_eq!(world.failed().failure, Some(Failure::StartRefused));
    assert_eq!(world.manager.asked(), ["reload", "start"]);
}

#[test]
fn a_start_that_does_not_end_in_a_connection_ends_within_its_bound() {
    // Started, and the unit ended before anything answered.
    let world = World::packaged(LinkState::Absent, Ok(not_installed()));
    world
        .manager
        .on_start(|status| *status = Ok(unit(ActiveState::Failed, UnitResult::Other)));
    let began = Instant::now();
    assert_eq!(world.failed().failure, Some(Failure::StartupFailed));
    assert!(
        began.elapsed() < QUICK.startup,
        "said when known, not at the limit"
    );
    // It ended as often as its restart limit allows.
    let world = World::packaged(LinkState::Absent, Ok(not_installed()));
    world
        .manager
        .on_start(|status| *status = Ok(unit(ActiveState::Failed, UnitResult::StartLimitHit)));
    assert_eq!(world.failed().failure, Some(Failure::ExitedRepeatedly));
    // The unit runs and nothing answers: the wait is bounded, and nothing is stopped,
    // restarted or started again for it.
    let world = World::packaged(LinkState::Absent, Ok(not_installed()));
    world
        .manager
        .on_start(|status| *status = Ok(unit(ActiveState::Active, UnitResult::Success)));
    let began = Instant::now();
    assert_eq!(world.failed().failure, Some(Failure::SocketUnavailable));
    assert!(began.elapsed() >= QUICK.startup && began.elapsed() < QUICK.startup * 4);
    assert_eq!(world.manager.asked(), ["reload", "start"]);
    // The unit was looked at a bounded number of times while waiting.
    assert!(
        world.manager.count("status") <= 12,
        "{}",
        world.manager.count("status")
    );
    world.quiet();
    assert_eq!(world.manager.count("start"), 1);
    // Retry is the user asking, and still nothing is done to an authority that runs:
    // its files are not written again and it is not started over.
    let installed = inode(&world.account.layout().binary());
    world.bootstrap.retry();
    world.until(|v| v.phase == Phase::Connecting);
    assert_eq!(world.failed().failure, Some(Failure::SocketUnavailable));
    assert_eq!(world.manager.asked(), ["reload", "start"]);
    assert_eq!(inode(&world.account.layout().binary()), installed);
    // The place of the socket is not safely the user's.
    let world = World::packaged(LinkState::Unsafe, Ok(not_installed()));
    assert_eq!(world.failed().failure, Some(Failure::SocketUnsafe));
    assert_eq!(world.manager.asked(), Vec::<&str>::new());
}

#[test]
fn it_starts_the_authority_by_itself_once_and_after_that_only_retry_does() {
    let world = World::packaged(LinkState::Absent, Ok(not_installed()));
    world
        .manager
        .on_start(|status| *status = Ok(unit(ActiveState::Failed, UnitResult::Other)));
    assert_eq!(world.failed().failure, Some(Failure::StartupFailed));
    assert_eq!(world.manager.count("start"), 1);
    // The link keeps saying nothing answers. That is not a reason to start it again, to
    // look at it again, or to say anything else about it.
    let (looks, said) = (world.manager.count("status"), world.bootstrap.view());
    for _ in 0..5 {
        world.link.set(LinkState::Absent);
    }
    world.quiet();
    assert_eq!(world.manager.count("start"), 1);
    assert_eq!(world.manager.count("status"), looks);
    assert_eq!(world.bootstrap.view(), said);
    assert_eq!(said.failure, Some(Failure::StartupFailed));

    // Retry: one attempt, however often it is pressed. A recorded failure is cleared
    // first, because the user asked.
    world
        .manager
        .on_start(|status| *status = Ok(unit(ActiveState::Active, UnitResult::Success)));
    for _ in 0..4 {
        world.bootstrap.retry();
    }
    let (view, _) = world.until(|v| v.phase == Phase::Connecting);
    // What failed before is not said of an attempt that is being made.
    assert_eq!((view.failure, view.retry), (None, false));
    assert_eq!(
        world.manager.asked(),
        ["reload", "start", "reload", "reset-failed", "start"]
    );
    // Retry while an attempt is being made, or once it is ready, does nothing.
    world.bootstrap.retry();
    world.link.set(LinkState::Connected);
    world.ready();
    world.bootstrap.retry();
    world.quiet();
    assert_eq!(world.manager.count("start"), 2);

    // It goes away again. Whether it comes back is the service manager's business: the
    // desktop watches, and starts nothing.
    world
        .manager
        .set(Ok(unit(ActiveState::Activating, UnitResult::Other)));
    world.link.set(LinkState::Absent);
    let (_, phases) = world.until(|v| v.phase == Phase::Connecting);
    assert!(!phases.contains(&Phase::Installing) && !phases.contains(&Phase::Starting));
    world
        .manager
        .set(Ok(unit(ActiveState::Active, UnitResult::Success)));
    world.link.set(LinkState::Connected);
    world.ready();
    // And when the manager gave up on it, or it was stopped, that is what is said.
    world
        .manager
        .set(Ok(unit(ActiveState::Failed, UnitResult::StartLimitHit)));
    world.link.set(LinkState::Absent);
    assert_eq!(world.failed().failure, Some(Failure::ExitedRepeatedly));
    world.link.set(LinkState::Connected);
    world.ready();
    world
        .manager
        .set(Ok(unit(ActiveState::Inactive, UnitResult::Success)));
    world.link.set(LinkState::Absent);
    let view = world.failed();
    assert_eq!((view.failure, view.retry), (Some(Failure::Stopped), true));
    // The same when the desktop looked while it was still being stopped: it was
    // watched, not started, so it did not fail to start.
    world
        .manager
        .set(Ok(unit(ActiveState::Active, UnitResult::Success)));
    world.link.set(LinkState::Connected);
    world.ready();
    world
        .manager
        .set(Ok(unit(ActiveState::Deactivating, UnitResult::Success)));
    world.link.set(LinkState::Absent);
    world.until(|v| v.phase == Phase::Connecting);
    world
        .manager
        .set(Ok(unit(ActiveState::Inactive, UnitResult::Success)));
    assert_eq!(world.failed().failure, Some(Failure::Stopped));
    world.quiet();
    assert_eq!(world.manager.count("start"), 2);
    assert_eq!(world.manager.count("reset-failed"), 1);
}

#[test]
fn an_authority_that_answered_and_went_away_is_never_started_by_the_desktop_itself() {
    // It was running when the desktop came up. Somebody stops it: the manager says it
    // is not running, and that is respected. The desktop has not used a start of its own
    // and still does not start it.
    let world = World::packaged(
        LinkState::Connected,
        Ok(unit(ActiveState::Active, UnitResult::Success)),
    );
    world.ready();
    world
        .manager
        .set(Ok(unit(ActiveState::Inactive, UnitResult::Success)));
    world.link.set(LinkState::Absent);
    let view = world.failed();
    assert_eq!((view.failure, view.retry), (Some(Failure::Stopped), true));
    world.quiet();
    assert_eq!(world.manager.asked(), Vec::<&str>::new());
    assert!(!world.installed());
    // The user asks: now it is installed and started.
    world
        .manager
        .on_start(|status| *status = Ok(unit(ActiveState::Active, UnitResult::Success)));
    world.bootstrap.retry();
    world.until(|v| v.phase == Phase::Connecting);
    assert_eq!(world.manager.asked(), ["reload", "start"]);

    // One attempt at a time, whatever the link does meanwhile. A start that has not
    // returned yet is the attempt; the link flapping under it starts no second one.
    let world = World::packaged(LinkState::Connecting, Ok(not_installed()));
    let (release, held) = std::sync::mpsc::channel::<()>();
    let held = Mutex::new(held);
    world.manager.on_start(move |status| {
        let _ = held.lock().unwrap().recv_timeout(Duration::from_secs(5));
        *status = Ok(unit(ActiveState::Active, UnitResult::Success));
    });
    world.link.set(LinkState::Absent);
    world.until(|v| v.phase == Phase::Starting);
    let looks = world.manager.count("status");
    // The follower sees each of these: it is not the link skipping one that saves it.
    world.link.set(LinkState::Connected);
    thread::sleep(Duration::from_millis(60));
    world.link.set(LinkState::Absent);
    world.quiet();
    assert_eq!(world.bootstrap.view().phase, Phase::Starting);
    assert_eq!(world.manager.count("status"), looks);
    assert_eq!(world.manager.asked(), ["reload", "start"]);
    release.send(()).unwrap();
    world.link.set(LinkState::Connected);
    world.ready();
    assert_eq!(world.manager.asked(), ["reload", "start"]);
}

#[test]
fn a_unit_the_manager_already_has_in_hand_is_neither_reinstalled_nor_started_again() {
    // Another desktop of this user just started it, or the manager is restarting it.
    for active in [
        ActiveState::Activating,
        ActiveState::Active,
        ActiveState::Deactivating,
    ] {
        let world = World::packaged(LinkState::Absent, Ok(unit(active, UnitResult::Success)));
        let (_, phases) = world.until(|v| v.phase == Phase::Connecting);
        assert_eq!(phases, [Phase::Checking, Phase::Connecting], "{active:?}");
        assert!(!world.installed(), "{active:?}");
        world.link.set(LinkState::Connected);
        world.ready();
        assert_eq!(world.manager.asked(), Vec::<&str>::new(), "{active:?}");
    }
    // A unit the manager stopped restarting is not started again without being asked.
    let world = World::packaged(
        LinkState::Absent,
        Ok(unit(ActiveState::Failed, UnitResult::StartLimitHit)),
    );
    assert_eq!(world.failed().failure, Some(Failure::ExitedRepeatedly));
    assert_eq!(world.manager.asked(), Vec::<&str>::new());
    assert!(!world.installed());
    world
        .manager
        .on_start(|status| *status = Ok(unit(ActiveState::Active, UnitResult::Success)));
    world.bootstrap.retry();
    world.until(|v| v.phase == Phase::Connecting);
    assert_eq!(world.manager.asked(), ["reload", "reset-failed", "start"]);
}

#[test]
fn an_installation_that_is_not_safe_or_not_this_products_fails_closed() {
    // A unit of that name somebody else wrote.
    let world = World::new(
        LinkState::Connecting,
        |account| {
            let layout = account.layout();
            install::ensure(&layout, &Payload::of(OLD.to_vec())).unwrap();
            fs::write(layout.unit(), "[Service]\nExecStart=/usr/bin/other\n").unwrap();
            Provision::Packaged(account.package(NEW))
        },
        Ok(unit(ActiveState::Inactive, UnitResult::Success)),
    );
    world.link.set(LinkState::Absent);
    let view = world.failed();
    assert_eq!(view.failure, Some(Failure::InstallationMismatch));
    assert_eq!(view.diagnostics.installation, InstallationState::Mismatch);
    assert_eq!(world.manager.asked(), Vec::<&str>::new());
    assert_eq!(fs::read(world.account.layout().binary()).unwrap(), OLD);

    // A link where the managed binary goes.
    let world = World::new(
        LinkState::Connecting,
        |account| {
            let layout = account.layout();
            install::ensure(&layout, &Payload::of(OLD.to_vec())).unwrap();
            fs::remove_file(layout.binary()).unwrap();
            symlink("/usr/bin/true", layout.binary()).unwrap();
            Provision::Packaged(account.package(NEW))
        },
        Ok(unit(ActiveState::Inactive, UnitResult::Success)),
    );
    world.link.set(LinkState::Absent);
    let view = world.failed();
    assert_eq!(view.failure, Some(Failure::InstallationUnsafe));
    assert_eq!(view.diagnostics.installation, InstallationState::Unsafe);
    assert_eq!(world.manager.asked(), Vec::<&str>::new());

    // No XDG directories to install into.
    let link = Link::new(LinkState::Absent);
    let manager = Manager::new(Ok(not_installed()));
    let account = Account::new();
    let bootstrap = AuthorityBootstrap::start_with(
        link.clone(),
        Provision::Packaged(account.package(NEW)),
        Err(LifecycleError::UnsafePath),
        manager.clone(),
        QUICK,
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while bootstrap.view().phase != Phase::Failed {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(bootstrap.view().failure, Some(Failure::InstallationUnsafe));
    assert_eq!(manager.asked(), Vec::<&str>::new());
    link.set(LinkState::Stopped);
    bootstrap.shutdown();
}

#[test]
fn an_incompatible_authority_is_said_and_nothing_is_done_to_it() {
    let world = World::packaged(
        LinkState::Incompatible,
        Ok(unit(ActiveState::Active, UnitResult::Success)),
    );
    let (view, _) = world.until(|v| v.phase == Phase::Incompatible);
    assert_eq!(
        (view.failure, view.retry, view.update_pending),
        (None, false, false)
    );
    world.bootstrap.retry();
    world.quiet();
    assert_eq!(world.bootstrap.view().phase, Phase::Incompatible);
    assert_eq!(world.manager.asked(), Vec::<&str>::new());
    assert!(!world.installed());
    // The same when it shows up while an attempt is waiting for a connection.
    let world = World::packaged(LinkState::Absent, Ok(not_installed()));
    world
        .manager
        .on_start(|status| *status = Ok(unit(ActiveState::Active, UnitResult::Success)));
    world.until(|v| v.phase == Phase::Connecting);
    world.link.set(LinkState::Incompatible);
    world.until(|v| v.phase == Phase::Incompatible);
    world.quiet();
    assert_eq!(world.manager.asked(), ["reload", "start"]);
}

#[test]
fn a_running_authority_is_never_replaced_and_a_newer_one_waits_for_a_cold_start() {
    // The control socket of this account, listened on by this very process: the kernel
    // names it as the listener, so the test can say who the manager's process is.
    let account = Account::new();
    let layout = account.layout();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(account.root.join("run/wyss-console"))
        .unwrap();
    let _listener = UnixListener::bind(layout.socket()).unwrap();
    install::ensure(&layout, &Payload::of(OLD.to_vec())).unwrap();
    let before = (inode(&layout.binary()), inode(&layout.unit()));
    let managed = UnitStatus {
        main_pid: Some(std::process::id()),
        fragment: Some(layout.unit()),
        ..unit(ActiveState::Active, UnitResult::Success)
    };
    let observe = |status: UnitStatus| {
        let link = Link::new(LinkState::Connected);
        let manager = Manager::new(Ok(status));
        let bootstrap = AuthorityBootstrap::start_with(
            link.clone(),
            Provision::Packaged(account.package(NEW)),
            Ok(layout.clone()),
            manager.clone(),
            QUICK,
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while bootstrap.view().phase != Phase::Ready {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }
        thread::sleep(Duration::from_millis(100));
        let view = bootstrap.view();
        // Whatever is pending, a running authority is asked nothing and loses no file.
        assert_eq!(manager.asked(), Vec::<&str>::new());
        link.set(LinkState::Stopped);
        bootstrap.shutdown();
        view
    };
    // This installation's service, running the authority of an older build.
    let view = observe(managed.clone());
    assert_eq!(view.origin, Some(Origin::Managed));
    assert_eq!(view.diagnostics.installation, InstallationState::Outdated);
    assert!(view.update_pending);
    assert_eq!((inode(&layout.binary()), inode(&layout.unit())), before);
    assert_eq!(fs::read(layout.binary()).unwrap(), OLD);

    // The same socket answered by something this installation's service is not: another
    // process, another unit file, or a unit that is not running. It is used and it is
    // not this product's to update.
    for foreign in [
        UnitStatus {
            main_pid: Some(1),
            ..managed.clone()
        },
        UnitStatus {
            main_pid: None,
            ..managed.clone()
        },
        UnitStatus {
            fragment: Some("/etc/systemd/user/wyss-control.service".into()),
            ..managed.clone()
        },
        UnitStatus {
            fragment: None,
            ..managed.clone()
        },
        UnitStatus {
            active: ActiveState::Inactive,
            ..managed.clone()
        },
        not_installed(),
    ] {
        let view = observe(foreign.clone());
        assert_eq!(view.origin, Some(Origin::External), "{foreign:?}");
        assert!(!view.update_pending, "{foreign:?}");
    }
    assert_eq!((inode(&layout.binary()), inode(&layout.unit())), before);

    // Once nothing runs, the next start installs the newer one first.
    drop(_listener);
    let link = Link::new(LinkState::Absent);
    let manager = Manager::new(Ok(unit(ActiveState::Inactive, UnitResult::Success)));
    let bootstrap = AuthorityBootstrap::start_with(
        link.clone(),
        Provision::Packaged(account.package(NEW)),
        Ok(layout.clone()),
        manager.clone(),
        QUICK,
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while manager.count("start") == 0 {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fs::read(layout.binary()).unwrap(), NEW);
    assert_eq!(manager.asked(), ["reload", "start"]);
    link.set(LinkState::Stopped);
    bootstrap.shutdown();
}

#[test]
fn every_view_of_the_application_shares_one_bootstrap_and_closing_stops_nothing() {
    let world = World::packaged(LinkState::Absent, Ok(not_installed()));
    world
        .manager
        .on_start(|status| *status = Ok(unit(ActiveState::Active, UnitResult::Success)));
    // Four views hold it. There is one state, and one attempt.
    let clones: Vec<AuthorityBootstrap> = (0..4).map(|_| world.bootstrap.clone()).collect();
    world.until(|v| v.phase == Phase::Connecting);
    for clone in &clones {
        assert_eq!(clone.view(), world.bootstrap.view());
        clone.retry();
    }
    world.quiet();
    assert_eq!(world.manager.asked(), ["reload", "start"]);
    world.link.set(LinkState::Connected);
    world.ready();
    // The desktop closes. Nothing was ever asked of the manager but to read, to reload
    // and to start: there is no stop and no restart to ask for.
    world.link.set(LinkState::Stopped);
    world.bootstrap.shutdown();
    assert_eq!(world.manager.asked(), ["reload", "start"]);
    assert_eq!(world.bootstrap.view().phase, Phase::Ready);
    world.bootstrap.retry();
    world.quiet();
    assert_eq!(world.manager.asked(), ["reload", "start"]);
}

#[test]
fn the_view_says_closed_words_and_nothing_of_the_system_underneath() {
    let world = World::packaged(LinkState::Absent, Err(ManagerError::Unavailable));
    let view = serde_json::to_value(world.failed()).unwrap();
    let mut keys: Vec<&str> = view
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "diagnostics",
            "failure",
            "origin",
            "phase",
            "retry",
            "revision",
            "schema_version",
            "update_pending"
        ]
    );
    assert_eq!(view["phase"], "failed");
    assert_eq!(view["failure"], "manager_unavailable");
    assert_eq!(
        view["diagnostics"],
        serde_json::json!({"build": "packaged", "installation": "absent", "service": "unavailable"})
    );
    let text = view.to_string();
    for private in [
        world.account.root.to_str().unwrap(),
        "systemctl",
        "systemd",
        ".service",
        "wyss-control",
        "/",
        "pid",
        "socket",
        "XDG",
    ] {
        assert!(!text.contains(private), "{private} crossed to the window");
    }
}
