//! The reader, the path rules and the model, against private temporary directories. No
//! test touches a real provider directory (NFR-MNT-03): every path is under a directory
//! of its own, removed at the end.
use super::model::{Bounds, ItemBudget, NotWritable, PathTrust, ReadOnlyReason, SourceClass};
use super::model::{ContentHash, InvalidBounds};
use super::model::{SourceFailure, TooLarge, UnsupportedReason, UntrustedReason, writability};
use super::place::{Facts, hook, is_credential_file, judge_directory, judge_file};
use super::place::{link_target, names};
use super::reader::{ConfigReader, EntryKind, parse_json, settle};
use super::roots::{ApprovedRoots, InvalidRoot};
use nix::{sys::stat::Mode, unistd::mkfifo};
use std::{
    ffi::OsStr,
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// A private directory standing for a home, with a trusted chain of ancestors.
struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        let base = fs::canonicalize(std::env::temp_dir()).unwrap();
        let root = base.join(format!("wyss-cfg-{}", uuid::Uuid::new_v4().simple()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        Self(root)
    }
    fn path(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }
    fn dir(&self, relative: &str, mode: u32) -> PathBuf {
        let path = self.path(relative);
        fs::create_dir_all(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        path
    }
    fn file(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.path(relative);
        fs::write(&path, bytes).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        path
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        // A test may leave a directory it made unreadable; give it back first.
        for entry in walk(&self.0) {
            let _ = fs::set_permissions(&entry, fs::Permissions::from_mode(0o700));
        }
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    let mut found = vec![dir.to_owned()];
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                found.extend(walk(&entry.path()));
            }
        }
    }
    found
}

fn small(file_bytes: u64) -> Bounds {
    Bounds {
        file_bytes,
        ..Bounds::PROPOSED
    }
}

/// A reader whose only approved root is the private directory.
fn reader_with(root: &Root, bounds: Bounds) -> ConfigReader {
    ConfigReader::new(ApprovedRoots::new([root.0.clone()]).unwrap(), bounds).unwrap()
}

fn reader(root: &Root) -> ConfigReader {
    reader_with(root, Bounds::PROPOSED)
}

const ME: u32 = 1000;
fn facts(uid: u32, mode: u32, nlink: u64) -> Facts {
    Facts { uid, mode, nlink }
}

#[test]
fn a_directory_is_judged_by_owner_and_who_can_write_it() {
    let read_only = Ok(PathTrust::ReadOnly(ReadOnlyReason::GroupWritable));
    assert_eq!(
        judge_directory(facts(ME, 0o40700, 2), ME),
        Ok(PathTrust::Trusted)
    );
    assert_eq!(
        judge_directory(facts(0, 0o40755, 2), ME),
        Ok(PathTrust::Trusted)
    );
    // `/tmp`: root's and sticky.
    assert_eq!(
        judge_directory(facts(0, 0o41777, 2), ME),
        Ok(PathTrust::Trusted)
    );
    // The owner's real `~/.claude` is 0775: read, never written.
    assert_eq!(judge_directory(facts(ME, 0o40775, 2), ME), read_only);
    assert_eq!(
        judge_directory(facts(ME, 0o40777, 2), ME),
        Err(UntrustedReason::WritableByOthers)
    );
    assert_eq!(
        judge_directory(facts(ME, 0o41777, 2), ME),
        Err(UntrustedReason::WritableByOthers)
    );
    assert_eq!(
        judge_directory(facts(1001, 0o40700, 2), ME),
        Err(UntrustedReason::ForeignOwner)
    );
}

#[test]
fn a_file_is_judged_by_owner_writers_and_names() {
    assert_eq!(
        judge_file(facts(ME, 0o100600, 1), ME),
        Ok(PathTrust::Trusted)
    );
    assert_eq!(
        judge_file(facts(0, 0o100644, 1), ME),
        Ok(PathTrust::Trusted)
    );
    assert_eq!(
        judge_file(facts(ME, 0o100664, 1), ME),
        Ok(PathTrust::ReadOnly(ReadOnlyReason::GroupWritable))
    );
    assert_eq!(
        judge_file(facts(ME, 0o100600, 2), ME),
        Ok(PathTrust::ReadOnly(ReadOnlyReason::MultipleNames))
    );
    assert_eq!(
        judge_file(facts(ME, 0o100666, 1), ME),
        Err(UntrustedReason::WritableByOthers)
    );
    assert_eq!(
        judge_file(facts(1001, 0o100600, 1), ME),
        Err(UntrustedReason::ForeignOwner)
    );
}

/// TS-CFG-09 as F1 has it: no source of any class, in any place, can be written.
#[test]
fn no_source_has_a_write_capability() {
    let trusts = [
        PathTrust::Trusted,
        PathTrust::ReadOnly(ReadOnlyReason::GroupWritable),
        PathTrust::ReadOnly(ReadOnlyReason::MultipleNames),
    ];
    for trust in trusts {
        assert_eq!(writability(SourceClass::System, trust), NotWritable::System);
        assert_eq!(
            writability(SourceClass::ProviderOwned, trust),
            NotWritable::ProviderOwned
        );
        assert_eq!(
            writability(SourceClass::Generated, trust),
            NotWritable::Generated
        );
    }
    for class in [SourceClass::User, SourceClass::Project] {
        assert_eq!(
            writability(class, PathTrust::Trusted),
            NotWritable::NotInThisPhase
        );
        assert_eq!(
            writability(class, PathTrust::ReadOnly(ReadOnlyReason::GroupWritable)),
            NotWritable::SharedPath
        );
    }
}

#[test]
fn only_plain_absolute_paths_are_walked() {
    let not_plain = Err(SourceFailure::UntrustedPath {
        reason: UntrustedReason::NotPlain,
    });
    assert_eq!(names(Path::new("relative/settings.json")), not_plain);
    assert_eq!(names(Path::new("/home/x/../y/settings.json")), not_plain);
    assert_eq!(names(Path::new("/home/x/\nsettings.json")), not_plain);
    assert_eq!(
        names(Path::new("/home/./x/settings.json")).unwrap(),
        ["home", "x", "settings.json"]
    );
}

#[test]
fn a_link_target_is_resolved_against_the_link_directory() {
    let link = Path::new("/home/x/.claude/CLAUDE.md");
    assert_eq!(
        link_target(link, OsStr::new("AGENTS.md")).unwrap(),
        Path::new("/home/x/.claude/AGENTS.md")
    );
    assert_eq!(
        link_target(link, OsStr::new("../../y/./rules.md")).unwrap(),
        Path::new("/home/y/rules.md")
    );
    assert_eq!(
        link_target(link, OsStr::new("/etc/codex/x.toml")).unwrap(),
        Path::new("/etc/codex/x.toml")
    );
    // `..` above the root stays at the root, as the kernel does.
    assert_eq!(
        link_target(link, OsStr::new("../../../../../etc/a")).unwrap(),
        Path::new("/etc/a")
    );
}

#[test]
fn credential_files_are_known_by_name() {
    for name in [
        "auth.json",
        ".credentials.json",
        ".env",
        ".env.local",
        "credentials",
        "credentials.toml",
        "secrets.json",
        ".netrc",
        "id_ed25519",
        "server.pem",
        "Auth.JSON",
        ".npmrc",
        "oauth_tokens.json",
        "keys.kdbx",
    ] {
        assert!(is_credential_file(OsStr::new(name)), "{name}");
    }
    for name in [
        "settings.json",
        "config.toml",
        "hooks.json",
        "AGENTS.md",
        ".claude.json",
        "environment.md",
        // Notes and skills about secrets are configuration; their text is redacted.
        "secrets.md",
        "secret-handling",
        "credentials.txt",
        "token-budget.md",
    ] {
        assert!(!is_credential_file(OsStr::new(name)), "{name}");
    }
}

#[test]
fn a_regular_file_is_read_with_its_hash() {
    let root = Root::new();
    let path = root.file("settings.json", b"{\"hooks\":{}}");
    let read = reader(&root).read_file(&path).unwrap();
    assert_eq!(read.bytes(), b"{\"hooks\":{}}");
    assert_eq!(read.hash(), ContentHash::of(b"{\"hooks\":{}}"));
    assert_eq!(read.trust(), PathTrust::Trusted);
    assert!(read.links().is_empty());
    // A debug line never shows the content.
    assert!(!format!("{read:?}").contains("hooks"));
}

/// TS-CFG-02: missing is a word of its own, and an empty file declares nothing.
#[test]
fn missing_and_empty_sources_are_not_errors_of_the_provider() {
    let root = Root::new();
    assert_eq!(
        reader(&root)
            .read_file(&root.path("settings.json"))
            .unwrap_err(),
        SourceFailure::Missing
    );
    assert_eq!(
        reader(&root)
            .read_file(&root.path("no-such-dir/settings.json"))
            .unwrap_err(),
        SourceFailure::Missing
    );
    let empty = root.file("empty.json", b"");
    let read = reader(&root).read_file(&empty).unwrap();
    assert_eq!(parse_json(read.bytes(), &Bounds::PROPOSED), Ok(None));
    assert_eq!(parse_json(b" \n\t ", &Bounds::PROPOSED), Ok(None));
}

/// TS-FS-05: unreadable is not absent, and a file that comes back is read again.
#[test]
fn unreadable_is_distinct_from_missing_and_a_returning_file_is_read() {
    let root = Root::new();
    let path = root.file("settings.json", b"{}");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(
        reader(&root).read_file(&path).unwrap_err(),
        SourceFailure::Unreadable
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(reader(&root).read_file(&path).is_ok());

    let closed = root.dir("closed", 0o700);
    root.file("closed/config.toml", b"x = 1");
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(
        reader(&root)
            .read_file(&closed.join("config.toml"))
            .unwrap_err(),
        SourceFailure::Unreadable
    );
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o700)).unwrap();

    fs::remove_file(&path).unwrap();
    assert_eq!(
        reader(&root).read_file(&path).unwrap_err(),
        SourceFailure::Missing
    );
    root.file("settings.json", b"{\"back\":true}");
    assert_eq!(
        reader(&root).read_file(&path).unwrap().bytes(),
        b"{\"back\":true}"
    );
}

/// TS-CFG-05: the size bound, exceeded by one.
#[test]
fn a_file_one_byte_over_the_bound_is_too_large() {
    let root = Root::new();
    let at = root.file("at.json", &[b' '; 16]);
    let over = root.file("over.json", &[b' '; 17]);
    assert!(reader_with(&root, small(16)).read_file(&at).is_ok());
    assert_eq!(
        reader_with(&root, small(16)).read_file(&over).unwrap_err(),
        SourceFailure::TooLarge {
            bound: TooLarge::File
        }
    );
}

/// TS-CFG-05 and TS-FS-04: link hops at the bound, one past it, and a loop.
#[test]
fn links_are_followed_to_the_bound_and_no_further() {
    let root = Root::new();
    root.file("target.json", b"{}");
    let mut previous = root.path("target.json");
    for hop in 1..=5 {
        let link = root.path(&format!("link{hop}"));
        symlink(&previous, &link).unwrap();
        previous = link;
    }
    let four = reader(&root).read_file(&root.path("link4")).unwrap();
    assert_eq!(four.links().len(), 4);
    assert_eq!(four.links()[0], root.path("link4"));
    assert_eq!(four.bytes(), b"{}");
    let limit = SourceFailure::Unsupported {
        reason: UnsupportedReason::LinkLimit,
    };
    assert_eq!(
        reader(&root).read_file(&root.path("link5")).unwrap_err(),
        limit
    );

    symlink("b", root.path("a")).unwrap();
    symlink("a", root.path("b")).unwrap();
    let started = Instant::now();
    assert_eq!(reader(&root).read_file(&root.path("a")).unwrap_err(), limit);
    assert!(started.elapsed() < Duration::from_secs(1));
}

/// TS-FS-03: a final link is never followed by the open; its target is walked and judged
/// again from the root, and a target in a place others can write is not read.
#[test]
fn a_link_target_in_an_untrusted_place_is_not_read() {
    let root = Root::new();
    let open = root.dir("open", 0o777);
    fs::write(open.join("planted.json"), b"{\"planted\":true}").unwrap();
    symlink(open.join("planted.json"), root.path("settings.json")).unwrap();
    assert_eq!(
        reader(&root)
            .read_file(&root.path("settings.json"))
            .unwrap_err(),
        SourceFailure::UntrustedPath {
            reason: UntrustedReason::WritableByOthers
        }
    );
}

#[test]
fn a_link_on_the_way_or_a_file_on_the_way_is_refused() {
    let root = Root::new();
    root.dir("real", 0o700);
    root.file("real/settings.json", b"{}");
    symlink(root.path("real"), root.path("linked")).unwrap();
    assert_eq!(
        reader(&root)
            .read_file(&root.path("linked/settings.json"))
            .unwrap_err(),
        SourceFailure::UntrustedPath {
            reason: UntrustedReason::AncestorLink
        }
    );
    root.file("plain", b"");
    assert_eq!(
        reader(&root)
            .read_file(&root.path("plain/settings.json"))
            .unwrap_err(),
        SourceFailure::UntrustedPath {
            reason: UntrustedReason::AncestorNotDirectory
        }
    );
}

/// TS-FS-02: a group-writable provider directory is read, and can never be written.
#[test]
fn a_shared_directory_is_read_and_never_writable() {
    let root = Root::new();
    root.dir("dot-claude", 0o775);
    let path = root.file("dot-claude/settings.json", b"{}");
    let read = reader(&root).read_file(&path).unwrap();
    assert_eq!(
        read.trust(),
        PathTrust::ReadOnly(ReadOnlyReason::GroupWritable)
    );
    assert_eq!(
        writability(SourceClass::User, read.trust()),
        NotWritable::SharedPath
    );
}

#[test]
fn a_file_with_a_second_name_is_read_only() {
    let root = Root::new();
    let path = root.file("settings.json", b"{}");
    fs::hard_link(&path, root.path("other-name.json")).unwrap();
    assert_eq!(
        reader(&root).read_file(&path).unwrap().trust(),
        PathTrust::ReadOnly(ReadOnlyReason::MultipleNames)
    );
}

#[test]
fn what_is_not_a_regular_file_is_unsupported_and_never_hangs() {
    let root = Root::new();
    let not_regular = SourceFailure::Unsupported {
        reason: UnsupportedReason::NotRegularFile,
    };
    let fifo = root.path("settings.json");
    mkfifo(&fifo, Mode::from_bits_truncate(0o600)).unwrap();
    let started = Instant::now();
    assert_eq!(reader(&root).read_file(&fifo).unwrap_err(), not_regular);
    assert!(started.elapsed() < Duration::from_secs(1));

    root.dir("a-directory", 0o700);
    assert_eq!(
        reader(&root)
            .read_file(&root.path("a-directory"))
            .unwrap_err(),
        not_regular
    );
    // A device that never ends: out of the roots it is refused unseen; even approved,
    // it is not a regular file and is never read.
    symlink("/dev/zero", root.path("zero.json")).unwrap();
    assert_eq!(
        reader(&root)
            .read_file(&root.path("zero.json"))
            .unwrap_err(),
        SourceFailure::UntrustedPath {
            reason: UntrustedReason::OutsideRoots
        }
    );
    let with_dev = ConfigReader::new(
        ApprovedRoots::new([root.0.clone(), PathBuf::from("/dev")]).unwrap(),
        Bounds::PROPOSED,
    )
    .unwrap();
    let started = Instant::now();
    assert_eq!(
        with_dev.read_file(&root.path("zero.json")).unwrap_err(),
        not_regular
    );
    assert!(started.elapsed() < Duration::from_secs(1));
}

/// 06 §4.2: a credential file is never opened, also when a link leads to it.
#[test]
fn a_credential_file_is_never_opened() {
    let root = Root::new();
    let auth = root.file("auth.json", b"{\"marker\":\"wyss-marker-auth\"}");
    assert_eq!(
        reader(&root).read_file(&auth).unwrap_err(),
        SourceFailure::CredentialFile
    );
    symlink(&auth, root.path("settings.json")).unwrap();
    assert_eq!(
        reader(&root)
            .read_file(&root.path("settings.json"))
            .unwrap_err(),
        SourceFailure::CredentialFile
    );
}

/// A file that changes on every read is given up after the bound, and one that settles
/// is accepted. The attempt itself is a function, so this needs no race.
#[test]
fn a_file_changing_during_reads_is_retried_then_unstable() {
    let mut calls = 0;
    let result: Result<(), _> = settle(3, || {
        calls += 1;
        Ok(None)
    });
    assert_eq!(result, Err(SourceFailure::Unstable));
    assert_eq!(calls, 3);

    let mut calls = 0;
    let settled = settle(3, || {
        calls += 1;
        Ok((calls == 2).then_some("settled"))
    });
    assert_eq!(settled, Ok("settled"));
    assert_eq!(calls, 2);

    let failed: Result<(), _> = settle(3, || Err(SourceFailure::Missing));
    assert_eq!(failed, Err(SourceFailure::Missing));
}

/// TS-CFG-05: the listing bound, exceeded by one; names come back sorted with kinds.
#[test]
fn a_listing_stops_one_past_its_bound() {
    let root = Root::new();
    let skills = root.dir("skills", 0o700);
    for name in ["c-skill", "a-skill"] {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(skills.join(name))
            .unwrap();
    }
    root.file("skills/b-note.md", b"");
    symlink("a-skill", skills.join("d-link")).unwrap();
    let bounds = Bounds {
        listing: 4,
        ..Bounds::PROPOSED
    };
    let listing = reader_with(&root, bounds).list_directory(&skills).unwrap();
    let seen: Vec<_> = listing
        .entries
        .iter()
        .map(|e| (e.name.to_str().unwrap(), e.kind))
        .collect();
    assert_eq!(
        seen,
        [
            ("a-skill", EntryKind::Directory),
            ("b-note.md", EntryKind::File),
            ("c-skill", EntryKind::Directory),
            ("d-link", EntryKind::Link),
        ]
    );
    root.file("skills/e-one-more.md", b"");
    assert_eq!(
        reader_with(&root, bounds)
            .list_directory(&skills)
            .unwrap_err(),
        SourceFailure::TooLarge {
            bound: TooLarge::Listing
        }
    );
    assert_eq!(
        reader_with(&root, bounds)
            .list_directory(&root.path("skills/b-note.md"))
            .unwrap_err(),
        SourceFailure::Unsupported {
            reason: UnsupportedReason::NotRegularFile
        }
    );
}

/// TS-CFG-03: where parsing stopped, and nothing of what was there.
#[test]
fn a_malformed_document_reports_a_position_and_no_content() {
    let bytes = b"{\n  \"env\": {\"TOKEN\": \"wyss-marker-malformed\" oops}\n}";
    let failure = parse_json(bytes, &Bounds::PROPOSED).unwrap_err();
    assert!(
        matches!(failure, SourceFailure::Malformed { line: 2, .. }),
        "{failure:?}"
    );
    let shown = format!(
        "{failure:?} {failure} {}",
        serde_json::to_string(&failure).unwrap()
    );
    assert!(!shown.contains("wyss-marker"), "{shown}");
    assert!(!shown.contains("oops"), "{shown}");
}

/// TS-CFG-05: the depth bound, exceeded by one; brackets inside strings do not count.
#[test]
fn nesting_one_past_the_depth_bound_is_refused_before_parsing() {
    let bounds = Bounds {
        depth: 4,
        ..Bounds::PROPOSED
    };
    assert!(parse_json(b"[[[[1]]]]", &bounds).unwrap().is_some());
    assert_eq!(
        parse_json(b"[[[[[1]]]]]", &bounds),
        Err(SourceFailure::TooLarge {
            bound: TooLarge::Depth
        })
    );
    assert!(
        parse_json(br#"{"a":"[[[[[[[[\"]]"}"#, &bounds)
            .unwrap()
            .is_some()
    );
    // Far past serde_json's own limit: refused by the bound, without recursion.
    let deep = "[".repeat(100_000);
    assert_eq!(
        parse_json(deep.as_bytes(), &bounds),
        Err(SourceFailure::TooLarge {
            bound: TooLarge::Depth
        })
    );
}

/// TS-CFG-05: the item bound, exceeded by one.
#[test]
fn the_item_budget_stops_at_its_bound() {
    let bounds = Bounds {
        items: 3,
        ..Bounds::PROPOSED
    };
    let mut budget = ItemBudget::new(&bounds);
    for _ in 0..3 {
        budget.take().unwrap();
    }
    assert_eq!(
        budget.take(),
        Err(SourceFailure::TooLarge {
            bound: TooLarge::Items
        })
    );
}

#[test]
fn failures_serialise_as_closed_words() {
    let json = |f: SourceFailure| serde_json::to_string(&f).unwrap();
    assert_eq!(json(SourceFailure::Missing), r#"{"failure":"missing"}"#);
    assert_eq!(
        json(SourceFailure::Malformed { line: 3, column: 7 }),
        r#"{"failure":"malformed","line":3,"column":7}"#
    );
    assert_eq!(
        json(SourceFailure::UntrustedPath {
            reason: UntrustedReason::AncestorLink
        }),
        r#"{"failure":"untrusted_path","reason":"ancestor_link"}"#
    );
    assert_eq!(SourceFailure::Unstable.as_str(), "source_unstable");
    assert_eq!(
        serde_json::to_string(&PathTrust::ReadOnly(ReadOnlyReason::GroupWritable)).unwrap(),
        r#"{"trust":"read_only","reason":"group_writable"}"#
    );
}

/// TS-SEC-03: the configuration module lives in the desktop binary and nowhere else; it
/// names neither the control module, the authority nor the window toolkit, and none of
/// them names it. The built authority binary is searched in `tests/authority_boundary.rs`.
#[test]
fn configuration_and_control_do_not_reference_each_other() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let texts = |dir: &Path| -> Vec<(PathBuf, String)> {
        walk_files(dir)
            .into_iter()
            .map(|path| {
                let text = fs::read_to_string(&path).unwrap();
                (path, text)
            })
            .collect()
    };
    for (path, text) in texts(&src.join("agentconfig")) {
        // Spelled in two halves, so that this file does not find itself.
        let forbidden_names = [
            concat!("crate::", "control"),
            concat!("crate::", "authority"),
            concat!("crate::", "contracts"),
            concat!("super::", "control"),
            concat!("wyss_", "control"),
            concat!("wyss_", "console"),
            concat!("tau", "ri"),
        ];
        for forbidden in forbidden_names {
            assert!(
                !text.contains(forbidden),
                "{} names {forbidden}",
                path.display()
            );
        }
    }
    let mut other = texts(&src.join("control"));
    other.extend(texts(&src.join("authority")));
    other.push((
        src.join("bin/wyss-control.rs"),
        fs::read_to_string(src.join("bin/wyss-control.rs")).unwrap(),
    ));
    other.push((
        src.join("lib.rs"),
        fs::read_to_string(src.join("lib.rs")).unwrap(),
    ));
    assert!(other.len() > 10);
    for (path, text) in other {
        assert!(
            !text.contains("agentconfig"),
            "{} names agentconfig",
            path.display()
        );
    }
    let desktop = fs::read_to_string(src.join("main.rs")).unwrap();
    assert!(desktop.contains("mod agentconfig;"));
}

fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
    found
}

// ---- approved roots, links that leave them, and the race the descriptors close -------

/// CTL-02, TS-FS-01: only the closed list is reachable, and a place outside it is not
/// even looked at.
#[test]
fn a_path_outside_the_approved_roots_is_refused_before_it_is_looked_at() {
    let root = Root::new();
    let other = Root::new();
    let planted = other.file("settings.json", b"{\"marker\":\"wyss-marker-outside\"}");
    let outside = SourceFailure::UntrustedPath {
        reason: UntrustedReason::OutsideRoots,
    };
    assert_eq!(reader(&root).read_file(&planted).unwrap_err(), outside);
    // A place that does not exist is refused the same way: the answer is not an oracle
    // for what exists outside the list.
    assert_eq!(
        reader(&root)
            .read_file(Path::new("/etc/wyss-console-no-such/settings.json"))
            .unwrap_err(),
        outside
    );
    assert_eq!(reader(&root).list_directory(&other.0).unwrap_err(), outside);
    // The root itself is inside, and a name that only starts like it is not.
    assert!(reader(&root).list_directory(&root.0).is_ok());
    let sibling = PathBuf::from(format!("{}-evil", root.0.display()));
    assert_eq!(
        reader(&root)
            .read_file(&sibling.join("settings.json"))
            .unwrap_err(),
        outside
    );
}

#[test]
fn roots_are_plain_absolute_and_never_the_whole_file_system() {
    for bad in ["/", "relative/dir", "/home/../etc", "/home/u\n/x", ""] {
        assert_eq!(
            ApprovedRoots::new([PathBuf::from(bad)]),
            Err(InvalidRoot),
            "{bad:?}"
        );
    }
    let many = (0..65).map(|n| PathBuf::from(format!("/srv/root{n}")));
    assert_eq!(ApprovedRoots::new(many), Err(InvalidRoot));
    let roots = ApprovedRoots::new(
        ["/home/u/.claude", "/home/u//.claude/", "/etc/codex"].map(PathBuf::from),
    )
    .unwrap();
    assert_eq!(
        roots.holding(Path::new("/home/u/.claude/settings.json")),
        Some(Path::new("/home/u/.claude"))
    );
    assert_eq!(
        roots.holding(Path::new("/home/u/.claude-x/settings.json")),
        None
    );
    assert_eq!(roots.holding(Path::new("/home/u")), None);
    // Two roots, one inside the other: the nearest holds it.
    let nested = ApprovedRoots::new(["/home/u", "/home/u/p"].map(PathBuf::from)).unwrap();
    assert_eq!(
        nested.holding(Path::new("/home/u/p/a.json")),
        Some(Path::new("/home/u/p"))
    );
}

/// TS-FS-03 with links the owner really has: inside the roots they are followed, with
/// every hop checked; out of them, or to a credential, they are not.
#[test]
fn a_link_is_followed_only_while_it_stays_inside_the_roots() {
    let root = Root::new();
    let other = Root::new();
    root.file("AGENTS.md", b"rules");
    symlink("AGENTS.md", root.path("CLAUDE.md")).unwrap();
    let inside = reader(&root).read_file(&root.path("CLAUDE.md")).unwrap();
    assert_eq!(inside.bytes(), b"rules");
    assert_eq!(inside.links().len(), 1);

    let outside = other.file("elsewhere.md", b"wyss-marker-leak");
    symlink(&outside, root.path("absolute.md")).unwrap();
    symlink(
        format!(
            "../{}/elsewhere.md",
            other.0.file_name().unwrap().to_str().unwrap()
        ),
        root.path("relative.md"),
    )
    .unwrap();
    for name in ["absolute.md", "relative.md"] {
        assert_eq!(
            reader(&root).read_file(&root.path(name)).unwrap_err(),
            SourceFailure::UntrustedPath {
                reason: UntrustedReason::OutsideRoots
            },
            "{name}"
        );
    }
    // Approving the second root is what lets the same link through: the list decides.
    let both = ConfigReader::new(
        ApprovedRoots::new([root.0.clone(), other.0.clone()]).unwrap(),
        Bounds::PROPOSED,
    )
    .unwrap();
    assert_eq!(
        both.read_file(&root.path("absolute.md")).unwrap().bytes(),
        b"wyss-marker-leak"
    );

    // A skills folder that is a link to somewhere else is listed only if that is approved.
    let skills = other.dir("skills", 0o700);
    symlink(&skills, root.path("skills")).unwrap();
    assert!(reader(&root).list_directory(&root.path("skills")).is_err());
    assert!(both.list_directory(&root.path("skills")).is_ok());
}

#[test]
fn credentials_below_a_root_are_never_opened_or_listed() {
    let root = Root::new();
    root.dir(".ssh", 0o700);
    root.file(".ssh/config", b"wyss-marker-ssh");
    root.dir("secrets", 0o700);
    root.file("secrets/anything.json", b"{}");
    for path in [".ssh/config", "secrets/anything.json"] {
        assert_eq!(
            reader(&root).read_file(&root.path(path)).unwrap_err(),
            SourceFailure::CredentialFile,
            "{path}"
        );
    }
    assert_eq!(
        reader(&root)
            .list_directory(&root.path(".ssh"))
            .unwrap_err(),
        SourceFailure::CredentialFile
    );
    // A link in an ordinary place that leads into one is not followed into it.
    symlink(root.path(".ssh/config"), root.path("settings.json")).unwrap();
    assert_eq!(
        reader(&root)
            .read_file(&root.path("settings.json"))
            .unwrap_err(),
        SourceFailure::CredentialFile
    );
    // The names above the root do not count: a project that lives in a folder called
    // `secrets` is still a project.
    let outer = Root::new();
    outer.dir("secrets/app", 0o700);
    let file = outer.file("secrets/app/settings.json", b"{}");
    let app = ConfigReader::new(
        ApprovedRoots::new([outer.path("secrets/app")]).unwrap(),
        Bounds::PROPOSED,
    )
    .unwrap();
    assert!(app.read_file(&file).is_ok());
}

/// TS-FS-03: a regular file swapped for a link after the walk and before the open is not
/// followed by the open. The link is then judged like any link: out of the roots, it is
/// refused and its target is never read.
#[test]
fn a_file_swapped_for_a_link_before_the_open_does_not_leak_its_target() {
    let root = Root::new();
    let other = Root::new();
    let secret = other.file("elsewhere.json", b"{\"marker\":\"wyss-marker-race\"}");
    let path = root.file("settings.json", b"{\"honest\":true}");
    let swap = (path.clone(), secret.clone());
    hook::set_before_final_open(move |reached| {
        if reached == swap.0 {
            fs::remove_file(&swap.0).unwrap();
            symlink(&swap.1, &swap.0).unwrap();
        }
    });
    let result = reader(&root).read_file(&path);
    hook::clear();
    assert_eq!(
        result.unwrap_err(),
        SourceFailure::UntrustedPath {
            reason: UntrustedReason::OutsideRoots
        }
    );
}

/// TS-FS-03: a directory swapped for a link after it was opened is not walked again: the
/// descriptor already held is the one that is read.
#[test]
fn a_directory_swapped_for_a_link_after_it_was_opened_is_not_followed() {
    let root = Root::new();
    let other = Root::new();
    root.dir("home", 0o700);
    let honest = root.file("home/settings.json", b"{\"honest\":true}");
    other.file("settings.json", b"{\"marker\":\"wyss-marker-swap\"}");
    let (home, away, target) = (root.path("home"), root.path("away"), other.0.clone());
    let watched = honest.clone();
    hook::set_before_final_open(move |reached| {
        if reached == watched {
            fs::rename(&home, &away).unwrap();
            symlink(&target, &home).unwrap();
        }
    });
    let result = reader(&root).read_file(&honest);
    hook::clear();
    // Control: the swap really happened, so reading the honest bytes is the descriptor's
    // doing and not an unexercised test.
    assert!(root.path("away/settings.json").exists());
    assert!(
        fs::symlink_metadata(root.path("home"))
            .unwrap()
            .is_symlink()
    );
    assert_eq!(result.unwrap().bytes(), b"{\"honest\":true}");
}

/// Legitimate shared configuration is read, as a file and as a folder, and flagged so
/// that nothing later writes it; only a place anyone can write is refused.
#[test]
fn group_writable_configuration_is_read_but_never_writable_and_world_writable_is_refused() {
    let root = Root::new();
    root.dir("shared", 0o775);
    let file = root.file("shared/settings.json", b"{}");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o664)).unwrap();
    let read = reader(&root).read_file(&file).unwrap();
    assert_eq!(
        read.trust(),
        PathTrust::ReadOnly(ReadOnlyReason::GroupWritable)
    );
    assert_eq!(
        writability(SourceClass::User, read.trust()),
        NotWritable::SharedPath
    );
    let listing = reader(&root).list_directory(&root.path("shared")).unwrap();
    assert_eq!(
        listing.trust,
        PathTrust::ReadOnly(ReadOnlyReason::GroupWritable)
    );

    fs::set_permissions(&file, fs::Permissions::from_mode(0o666)).unwrap();
    assert_eq!(
        reader(&root).read_file(&file).unwrap_err(),
        SourceFailure::UntrustedPath {
            reason: UntrustedReason::WritableByOthers
        }
    );
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(root.path("shared"), fs::Permissions::from_mode(0o777)).unwrap();
    assert_eq!(
        reader(&root).read_file(&file).unwrap_err(),
        SourceFailure::UntrustedPath {
            reason: UntrustedReason::WritableByOthers
        }
    );
}

// ---- bounds, and arithmetic at their edges -------------------------------------------

#[test]
fn bounds_refuse_zero_and_anything_past_their_ceiling() {
    let ok = |file_bytes, items, elements, depth, hops, listing, attempts| {
        Bounds::new(file_bytes, items, elements, depth, hops, listing, attempts)
    };
    assert!(ok(1, 1, 1, 1, 0, 1, 1).is_ok());
    assert!(ok(64 << 20, 1 << 20, 1 << 24, 100, 40, 1 << 16, 10).is_ok());
    for (name, bounds) in [
        ("file zero", ok(0, 1, 1, 1, 0, 1, 1)),
        ("file max", ok(u64::MAX, 1, 1, 1, 0, 1, 1)),
        ("file over", ok((64 << 20) + 1, 1, 1, 1, 0, 1, 1)),
        ("items zero", ok(1, 0, 1, 1, 0, 1, 1)),
        ("items max", ok(1, usize::MAX, 1, 1, 0, 1, 1)),
        ("elements max", ok(1, 1, usize::MAX, 1, 0, 1, 1)),
        ("depth over", ok(1, 1, 1, 101, 0, 1, 1)),
        ("hops over", ok(1, 1, 1, 1, 41, 1, 1)),
        ("listing max", ok(1, 1, 1, 1, 0, usize::MAX, 1)),
        ("attempts zero", ok(1, 1, 1, 1, 0, 1, 0)),
        ("attempts over", ok(1, 1, 1, 1, 0, 1, 11)),
    ] {
        assert_eq!(bounds, Err(InvalidBounds), "{name}");
    }
    assert_eq!(Bounds::PROPOSED.validated(), Ok(Bounds::PROPOSED));
    let roots = ApprovedRoots::new([PathBuf::from("/srv/x")]).unwrap();
    let unbounded = Bounds {
        file_bytes: u64::MAX,
        ..Bounds::PROPOSED
    };
    assert!(ConfigReader::new(roots, unbounded).is_err());
}

/// A value that skipped `new` cannot overflow the reader: it saturates.
#[test]
fn a_bound_at_the_edge_of_the_integers_does_not_overflow() {
    let root = Root::new();
    let file = root.file("settings.json", b"{}");
    let handle = fs::File::open(&file).unwrap();
    assert_eq!(super::reader::read_bytes_for_test(&handle, u64::MAX), b"{}");
    assert_eq!(
        super::reader::read_bytes_for_test(&handle, u64::MAX - 1),
        b"{}"
    );
    let edge = Bounds {
        elements: usize::MAX,
        depth: usize::MAX,
        ..Bounds::PROPOSED
    };
    assert_eq!(
        parse_json(b"[[[[1]]]]", &edge).unwrap(),
        Some(serde_json::json!([[[[1]]]]))
    );
    let mut budget = ItemBudget::new(&Bounds {
        items: 0,
        ..Bounds::PROPOSED
    });
    assert!(budget.take().is_err());
    let none: Result<(), _> = settle(0, || Ok(Some(())));
    assert_eq!(none, Err(SourceFailure::Unstable));
}

/// The read buffer's own edge: a file of exactly one chunk and one byte more.
#[test]
fn files_at_the_chunk_edge_are_read_whole_and_one_over_the_bound_is_refused() {
    let root = Root::new();
    for size in [65_535_usize, 65_536, 65_537, 131_072] {
        let path = root.file(&format!("f{size}.json"), &vec![b' '; size]);
        let at = reader_with(&root, small(size as u64))
            .read_file(&path)
            .unwrap();
        assert_eq!(at.bytes().len(), size);
        assert_eq!(
            reader_with(&root, small(size as u64 - 1))
                .read_file(&path)
                .unwrap_err(),
            SourceFailure::TooLarge {
                bound: TooLarge::File
            },
            "{size}"
        );
    }
    // A file that reports no size at all is still stopped by the bound.
    symlink("/proc/self/environ", root.path("environ.json")).unwrap();
    assert!(reader(&root).read_file(&root.path("environ.json")).is_err());
}

/// TS-CFG-05: the element bound, exceeded by one, and a document built to exhaust memory
/// is refused before it is parsed.
#[test]
fn json_with_one_element_past_the_bound_is_refused() {
    // root, `[`, and a comma per element after the first: `[1,2,3]` is 4.
    let bounds = Bounds {
        elements: 4,
        ..Bounds::PROPOSED
    };
    assert!(parse_json(b"[1,2,3]", &bounds).unwrap().is_some());
    let too_many = Err(SourceFailure::TooLarge {
        bound: TooLarge::Elements,
    });
    assert_eq!(parse_json(b"[1,2,3,4]", &bounds), too_many);
    // `{"a":1}` is root, `{`, `:` = 3; a comma inside a string does not count.
    assert!(
        parse_json(br#"{"a":"1,2,3,4,5,6"}"#, &bounds)
            .unwrap()
            .is_some()
    );
    let wide = format!("[{}0]", "0,".repeat(300_000));
    assert_eq!(parse_json(wide.as_bytes(), &Bounds::PROPOSED), too_many);
    let keys = format!("{{{}}}", r#""k":0,"#.repeat(200_000));
    assert_eq!(parse_json(keys.as_bytes(), &Bounds::PROPOSED), too_many);
}
