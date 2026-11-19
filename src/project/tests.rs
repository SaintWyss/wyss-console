use super::*;
use std::process::Command;

pub(super) struct Fixture(pub PathBuf);

impl Fixture {
    pub(super) fn new() -> Self {
        let base = std::env::var_os("WYSS_TEST_SCRATCH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".."));
        let root = base.join(format!("panel-fixture-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self(root.canonicalize().unwrap())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn detects_nearest_git_before_manifest_and_handles_nonprojects() {
    let dir = Fixture::new();
    let child = dir.0.join("child");
    fs::create_dir(&child).unwrap();
    assert_eq!(detect_among(child.ancestors().take(2)), None);
    fs::write(child.join("Cargo.toml"), "").unwrap();
    assert_eq!(
        detect_among(child.ancestors().take(2)),
        Some(Project {
            root: child.clone(),
            has_git: false
        })
    );
    fs::create_dir(dir.0.join(".git")).unwrap();
    assert_eq!(
        detect(&child),
        Some(Project {
            root: dir.0.clone(),
            has_git: true
        })
    );
    fs::write(child.join(".git"), "gitdir: elsewhere\n").unwrap();
    assert_eq!(
        detect(&child),
        Some(Project {
            root: child,
            has_git: true
        })
    );
}

#[test]
fn inventories_each_manifest() {
    use super::manifests::parse;
    let cargo = parse(
        "Cargo.toml",
        r#"
[package]
name = "fixture"
version = "1.0"
edition = "2024"
rust-version = "1.92"
license = "MIT"
[dependencies]
gtk = { package = "gtk4", version = "0.11" }
[[bin]]
name = "tool"
"#,
    );
    assert_eq!(cargo.package.as_deref(), Some("fixture"));
    assert_eq!(cargo.edition.len(), 2);
    assert_eq!(cargo.runs[0].1, "cargo run --bin tool");
    assert_eq!(cargo.dependencies[0], ("gtk4".into(), "0.11".into()));

    let node = parse(
        "package.json",
        r#"{"name":"site","version":"0.1","engines":{"node":">=20"},"dependencies":{"next":"^14"},"scripts":{"build":"next build"}}"#,
    );
    assert_eq!(node.runs[0], ("npm run build".into(), "next build".into()));
    assert_eq!(node.edition[0].1, ">=20");
    assert_eq!(node.version.as_deref(), Some("0.1"));

    let py = parse(
        "pyproject.toml",
        "[project]\nname='api'\nrequires-python='>=3.13'\ndependencies=['fastapi>=0.10']\n[tool.ruff]\n",
    );
    assert_eq!(py.dependencies[0], ("fastapi".into(), ">=0.10".into()));
    assert_eq!(py.quality, ["ruff"]);

    let go = parse(
        "go.mod",
        "module example.com/a\ngo 1.24\nrequire (\n github.com/gin-gonic/gin v1\n example.com/ignore v1 // indirect\n)\n",
    );
    assert_eq!(go.package.as_deref(), Some("example.com/a"));
    assert_eq!(
        go.dependencies,
        [("github.com/gin-gonic/gin".into(), "v1".into())]
    );

    let pom = parse(
        "pom.xml",
        "<project><parent><version>wrong</version></parent><artifactId>test</artifactId><version>1.0</version><properties><java.version>21</java.version></properties><dependencies><dependency><groupId>org.springframework</groupId><artifactId>spring-core</artifactId><version>3</version></dependency></dependencies></project>",
    );
    assert_eq!(pom.version.as_deref(), Some("1.0"));
    assert_eq!(
        pom.dependencies[0],
        ("org.springframework:spring-core".into(), "3".into())
    );

    for file in ["build.gradle", "build.gradle.kts"] {
        let gradle = parse(
            file,
            "version = \"2.0\"\nimplementation(\"org.springframework:spring-core:3\")",
        );
        assert_eq!(gradle.version.as_deref(), Some("2.0"));
        assert_eq!(gradle.dependencies, pom.dependencies);
    }
    let make = parse(
        "Makefile",
        &format!(
            "VAR := value\n.PHONY: start\nstart: dep\n\t@true\n{}",
            (0..25).map(|n| format!("t{n}:\n")).collect::<String>()
        ),
    );
    assert_eq!(make.runs.len(), 20);
    assert_eq!(make.runs[0].1, "make start");
    assert_eq!(
        manifests::frameworks(&[cargo, node, py, go, pom]),
        ["gtk4", "next", "fastapi", "spring", "gin"]
    );
}

#[test]
fn origin_hides_credentials_and_query_tokens() {
    assert_eq!(
        clean_origin("https://user:secret@host/owner/repo.git?token=hidden#token").as_deref(),
        Some("host/owner/repo")
    );
    assert_eq!(
        clean_origin("git@host:owner/repo.git").as_deref(),
        Some("host/owner/repo")
    );
    assert_eq!(
        clean_origin("ssh://user@host:22/org/repo.git").as_deref(),
        Some("host/org/repo")
    );
    assert_eq!(clean_origin("file:///home/user/owner/repo"), None);
}

#[test]
fn scan_is_bounded_and_does_not_read_ignored_or_symlinked_files() {
    use std::os::unix::fs::symlink;
    let dir = Fixture::new();
    let root = dir.0.join("project");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("target")).unwrap();
    fs::write(root.join("Makefile"), "test:\n").unwrap();
    fs::write(root.join("src/lib.rs"), "one\ntwo").unwrap();
    fs::write(root.join("target/ignored.py"), "contents").unwrap();
    fs::write(dir.0.join("private.py"), "outside").unwrap();
    symlink(dir.0.join("private.py"), root.join("src/link.py")).unwrap();
    symlink(&dir.0, root.join("external")).unwrap();
    File::create(root.join("src/huge.js"))
        .unwrap()
        .set_len(FILE_LIMIT as u64 + 1)
        .unwrap();
    let project = detect_among(std::iter::once(root.as_path())).unwrap();
    let inv = inventory(&project, READ_LIMIT, || false);
    assert!(inv.limited);
    assert_eq!(inv.languages, [("Rust", 7)]);
    assert_eq!(inv.directories, [("src".into(), 1, 2)]);
    let mut files = Files {
        root: &root,
        remaining: 7,
        limited: false,
    };
    assert_eq!(files.read(Path::new("src/lib.rs")).unwrap().0.len(), 7);
    assert!(files.read(Path::new("Makefile")).is_none());
    assert_eq!(
        bounded(&mut &b"12345"[..], 4).unwrap(),
        (b"1234".to_vec(), true)
    );
}

#[test]
fn markdown_keeps_html_images_and_unsafe_links_literal() {
    let doc = markdown(
        "# Heading\n\n<img src='https://example.org'> [go](https://example.org)\n\n- `code`\n- ![x](https://example.org/img)\n- [x](file:///private)\n\n| A | B |\n|---|---|\n| c | d |\n\n```sh\n<code>\n```",
    );
    assert!(matches!(&doc[0], Block::Heading(_)));
    assert!(
        matches!(&doc[1], Block::Paragraph(p) if p.iter().any(|part| matches!(part, Inline::Text(t) if t.contains("<img"))) && p.iter().any(|part| matches!(part, Inline::Link { .. })))
    );
    assert!(
        matches!(&doc[2], Block::List(l) if l[0] == [Inline::Code("code".into())] && l[1] == [Inline::Text("![x](https://example.org/img)".into())] && l[2] == [Inline::Text("[x](file:///private)".into())])
    );
    assert!(matches!(&doc[3], Block::Table(t) if t.len() == 2 && t[0].len() == 2));
    assert_eq!(doc[4], Block::Code("<code>".into()));
    assert_eq!(
        markdown::commands(
            "## Build and test\n```sh\ncargo test\n```\n## Links\n```sh\nignored\n```"
        ),
        ["cargo test"]
    );
    assert_eq!(
        manifests::workflow(
            "ci.yml",
            "name: Build\non:\n  push:\n    paths: []\n  pull_request:\njobs:\n"
        ),
        ("Build".into(), "push, pull_request".into())
    );
}

fn fixture_git(root: &Path, args: &[&str]) {
    let result = Command::new("/usr/bin/git")
        .env_clear()
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .current_dir(root)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "fixture git {args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn git_reader_never_runs_fsmonitor_or_filters_and_counts_changes() {
    let dir = Fixture::new();
    let root = &dir.0;
    fixture_git(root, &["init", "-q", "-b", "main"]);
    fs::write(root.join("old.rs"), "old\n").unwrap();
    fs::write(root.join("changed.rs"), "old\n").unwrap();
    fs::write(root.join("same.rs"), "same\n").unwrap();
    fs::write(root.join(".gitattributes"), "*.rs filter=trap\n").unwrap();
    fixture_git(root, &["add", "."]);
    fixture_git(root, &["commit", "-qm", "first fixture commit"]);
    fixture_git(root, &["tag", "v1"]);
    fixture_git(root, &["branch", "aaa"]);
    fixture_git(root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    fixture_git(root, &["config", "branch.main.remote", "origin"]);
    fixture_git(root, &["config", "branch.main.merge", "refs/heads/main"]);
    fixture_git(
        root,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
    );
    fixture_git(
        root,
        &[
            "config",
            "remote.origin.url",
            "https://user:secret@host/owner/repo.git",
        ],
    );
    let marker = root.join("trap-ran");
    let script = root.join(".git/trap");
    fs::write(
        &script,
        format!("#!/bin/sh\nprintf ran > '{}'\ncat\n", marker.display()),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    fixture_git(
        root,
        &["config", "core.fsmonitor", script.to_str().unwrap()],
    );
    fixture_git(
        root,
        &["config", "filter.trap.clean", script.to_str().unwrap()],
    );
    fs::write(root.join("new.txt"), "new\n").unwrap();
    fs::write(root.join("changed.rs"), "new contents\n").unwrap();
    fs::write(root.join("same.rs"), "same\n").unwrap();
    fs::remove_file(root.join("old.rs")).unwrap();
    let project = detect(root).unwrap();
    let (summary, _) = summary(&project);
    let result = git(root);
    let _ = inventory(&project, READ_LIMIT, || false);
    assert!(!marker.exists(), "the repository executed its trap");
    let git = result.unwrap();
    assert_eq!(summary.origin.as_deref(), Some("host/owner/repo"));
    assert_eq!((git.modified, git.new, git.deleted), (1, 1, 1));
    assert_eq!(git.upstream, Some((0, 0)));
    assert_eq!(git.commits[0].title, "first fixture commit");
    assert_eq!(git.local, ["main", "aaa"]);
    assert_eq!(git.remote, ["origin/main"]);
    assert_eq!(git.tags, ["v1"]);
}

#[test]
fn git_read_limits_output_and_kills_a_blocked_command() {
    let dir = Fixture::new();
    fixture_git(&dir.0, &["init", "-q", "-b", "main"]);
    let config = dir.0.join(".git/config");
    let mut initial = fs::read_to_string(&config).unwrap();
    initial.push_str("[paneltest]\n  payload = ");
    initial.push_str(&"x".repeat(256 * 1024 + 1));
    initial.push('\n');
    fs::write(&config, initial).unwrap();
    assert_eq!(
        run(&dir.0, &["config", "--local", "--get", "paneltest.payload"]),
        Err(GitError::OutputLimit)
    );
    fs::remove_file(&config).unwrap();
    nix::unistd::mkfifo(&config, Mode::S_IRUSR | Mode::S_IWUSR).unwrap();
    assert_eq!(
        run(&dir.0, &["config", "--local", "--get", "paneltest.payload"]),
        Err(GitError::Timeout)
    );
}
