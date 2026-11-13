//! The git state of a terminal session's folder, for the session bar: branch, how many changes
//! are not committed, how many commits are not pushed. It only reads. The folder is never one the
//! window names: the desktop asks the host for the session's own folder (`terminal_git_status`).
//!
//! `git status` is run so that it cannot write or wait: `GIT_OPTIONAL_LOCKS=0` and
//! `--no-optional-locks` (no `index.lock`, no index refresh written back), no fsmonitor daemon or
//! hook (`core.fsmonitor=false`, `core.hooksPath=/dev/null`), no pager, no prompt, no lazy
//! fetch, submodules not entered, stdin closed, and a time limit after which its whole process
//! group is killed. A repository whose own configuration names a content filter is not read.
//! These close the known ways for a folder to make `git status` run its commands; a hostile
//! `.git/config` is still git's to interpret, as it is for a shell prompt that shows git.
use serde::Serialize;
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// How long `git status` may run before it is killed and the bar shows nothing.
pub const LIMIT: Duration = Duration::from_secs(2);
/// Most bytes of `git status` read; past it the count of changes is a lower bound.
const MAX_OUTPUT: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GitStatus {
    /// The folder is not inside a git repository.
    NoRepo,
    Repo {
        /// The branch, or `None` when HEAD is detached.
        branch: Option<String>,
        /// Changed, staged, conflicted and untracked paths.
        changes: u32,
        /// Commits not on the upstream; `None` when the branch has no upstream.
        ahead: Option<u32>,
        /// The output was longer than this reads: `changes` is at least this many.
        truncated: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitError {
    /// `git` is not installed or could not start.
    Unavailable,
    /// It ran past `LIMIT` and was killed.
    Timeout,
    /// It failed for another reason (corrupt repository, permissions).
    Failed,
    /// The repository's own configuration names a content filter: status is not run.
    Refused,
}

pub fn status(dir: &Path) -> Result<GitStatus, GitError> {
    status_with(dir, LIMIT, &[])
}

/// `status` with a limit and extra environment, for tests.
pub fn status_with(
    dir: &Path,
    limit: Duration,
    env: &[(&str, &str)],
) -> Result<GitStatus, GitError> {
    status_inner(dir, limit, env, true)
}

/// Every run of git here: the same guards, its own process group, killed whole at the limit.
fn run(
    dir: &Path,
    args: &[&str],
    limit: Duration,
    env: &[(&str, &str)],
) -> Result<(std::process::ExitStatus, Vec<u8>, Vec<u8>), GitError> {
    use std::os::unix::process::CommandExt;
    let mut command = Command::new("git");
    command
        .arg("--no-pager")
        .arg("--no-optional-locks")
        .args(["-c", "core.fsmonitor=false"])
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(["-c", "color.status=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        // A partial clone would otherwise fetch (and write) missing objects.
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_PAGER", "cat")
        .env("PAGER", "cat")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Its own group, so that what git starts is killed with it.
        .process_group(0);
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = command.spawn().map_err(|_| GitError::Unavailable)?;
    let group = nix::unistd::Pid::from_raw(child.id() as i32);
    let mut stdout = child.stdout.take().ok_or(GitError::Failed)?;
    let mut stderr = child.stderr.take().ok_or(GitError::Failed)?;
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = (&mut stdout)
            .take(MAX_OUTPUT as u64)
            .read_to_end(&mut bytes);
        // Drain the rest so git is never blocked on a full pipe.
        let _ = std::io::copy(&mut stdout, &mut std::io::sink());
        bytes
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = (&mut stderr).take(64 * 1024).read_to_end(&mut bytes);
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
        bytes
    });
    let kill = |child: &mut std::process::Child| {
        let _ = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGKILL);
        let _ = child.wait();
    };
    let started = Instant::now();
    let exit = loop {
        match child.try_wait() {
            Ok(Some(exit)) => break exit,
            Ok(None) if started.elapsed() >= limit => {
                kill(&mut child);
                // The whole group is gone, so the pipes close and the readers end.
                let _ = out.join();
                let _ = err.join();
                return Err(GitError::Timeout);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                kill(&mut child);
                let _ = out.join();
                let _ = err.join();
                return Err(GitError::Failed);
            }
        }
    };
    // Anything git left running in its group goes too, before the pipes are read to the end.
    let _ = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGKILL);
    let out = out.join().map_err(|_| GitError::Failed)?;
    let err = err.join().map_err(|_| GitError::Failed)?;
    Ok((exit, out, err))
}

/// Whether the repository (not the owner's own configuration) names a content filter: `git
/// status` runs a `clean` filter on a file whose stat changed, so a folder that brings its own
/// `.git/config` would run its command. The owner's global filters (Git LFS) are kept.
pub fn repository_filters(listing: &[u8]) -> bool {
    String::from_utf8_lossy(listing).lines().any(|line| {
        let scope = line.split('\t').next().unwrap_or("");
        !matches!(scope, "global" | "system")
    })
}

fn status_inner(
    dir: &Path,
    limit: Duration,
    env: &[(&str, &str)],
    check_filters: bool,
) -> Result<GitStatus, GitError> {
    if !dir.is_absolute() || !dir.is_dir() {
        return Err(GitError::Failed);
    }
    let started = Instant::now();
    if check_filters {
        let (exit, out, err) = run(
            dir,
            &[
                "config",
                "--show-scope",
                "--name-only",
                "--get-regexp",
                r"^filter\..*\.(clean|smudge|process)$",
            ],
            limit,
            env,
        )?;
        if String::from_utf8_lossy(&err).contains("not a git repository") {
            return Ok(GitStatus::NoRepo);
        }
        // Exit 1: no such key, nothing to refuse. Anything else is read as it is.
        if exit.success() && repository_filters(&out) {
            return Err(GitError::Refused);
        }
        if !exit.success() && exit.code() != Some(1) {
            return Err(GitError::Failed);
        }
    }
    let (exit, out, err) = run(
        dir,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "--untracked-files=normal",
            // A submodule is a repository of its own, with its own configuration.
            "--ignore-submodules=dirty",
        ],
        limit.saturating_sub(started.elapsed()),
        env,
    )?;
    if !exit.success() {
        return if String::from_utf8_lossy(&err).contains("not a git repository") {
            Ok(GitStatus::NoRepo)
        } else {
            Err(GitError::Failed)
        };
    }
    Ok(parse(&out, out.len() >= MAX_OUTPUT))
}

/// Reads `git status --porcelain=v2 --branch`.
pub fn parse(bytes: &[u8], truncated: bool) -> GitStatus {
    let text = String::from_utf8_lossy(bytes);
    let mut branch = None;
    let mut ahead = None;
    let mut changes: u32 = 0;
    for line in text.lines() {
        if let Some(head) = line.strip_prefix("# branch.head ") {
            branch = (head != "(detached)").then(|| head.chars().take(256).collect());
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            ahead = ab
                .split_whitespace()
                .next()
                .and_then(|a| a.strip_prefix('+'))
                .and_then(|a| a.parse().ok());
        } else if ["1 ", "2 ", "u ", "? "].iter().any(|p| line.starts_with(p)) {
            changes = changes.saturating_add(1);
        }
    }
    GitStatus::Repo {
        branch,
        changes,
        ahead,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn temp(name: &str) -> Temp {
        let dir = std::env::temp_dir().join(format!(
            "wyss-git-status-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Temp(dir)
    }
    /// Isolated from the owner's git configuration: no global or system file, a fixed identity.
    const ENV: &[(&str, &str)] = &[
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_AUTHOR_NAME", "t"),
        ("GIT_AUTHOR_EMAIL", "t@example.invalid"),
        ("GIT_COMMITTER_NAME", "t"),
        ("GIT_COMMITTER_EMAIL", "t@example.invalid"),
    ];
    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .envs(ENV.iter().copied())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn a_change_not_committed_and_a_commit_not_pushed_are_counted_without_writing_a_lock() {
        let root = temp("repo");
        let remote = root.0.join("remote.git");
        let work = root.0.join("work");
        std::fs::create_dir_all(&remote).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        git(&remote, &["init", "-q", "--bare", "-b", "main"]);
        git(&work, &["init", "-q", "-b", "main"]);
        std::fs::write(work.join("a.txt"), "one\n").unwrap();
        git(&work, &["add", "a.txt"]);
        git(&work, &["commit", "-qm", "one"]);
        git(
            &work,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&work, &["push", "-q", "-u", "origin", "main"]);

        // Pushed and clean.
        assert_eq!(
            status_with(&work, LIMIT, ENV),
            Ok(GitStatus::Repo {
                branch: Some("main".into()),
                changes: 0,
                ahead: Some(0),
                truncated: false
            })
        );

        // One commit not pushed, one file changed and not committed.
        std::fs::write(work.join("b.txt"), "two\n").unwrap();
        git(&work, &["add", "b.txt"]);
        git(&work, &["commit", "-qm", "two"]);
        std::fs::write(work.join("a.txt"), "changed\n").unwrap();
        // A stale-stat index is what makes `git status` want to refresh and lock it.
        let index = work.join(".git/index");
        let before = std::fs::read(&index).unwrap();
        assert_eq!(
            status_with(&work, LIMIT, ENV),
            Ok(GitStatus::Repo {
                branch: Some("main".into()),
                changes: 1,
                ahead: Some(1),
                truncated: false
            })
        );
        assert!(!work.join(".git/index.lock").exists());
        assert_eq!(
            std::fs::read(&index).unwrap(),
            before,
            "the index is not rewritten"
        );
    }

    /// A repository whose index holds stale stat data for a file whose content did not change:
    /// a plain `git status` refreshes and rewrites that index.
    fn stale_index(name: &str) -> (Temp, PathBuf) {
        let root = temp(name);
        git(&root.0, &["init", "-q", "-b", "main"]);
        std::fs::write(root.0.join("same.txt"), "same\n").unwrap();
        git(&root.0, &["add", "same.txt"]);
        git(&root.0, &["commit", "-qm", "one"]);
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(root.0.join("same.txt"), "same\n").unwrap();
        let index = root.0.join(".git/index");
        (root, index)
    }

    #[test]
    fn a_stale_index_is_not_rewritten_while_plain_git_status_would_rewrite_it() {
        // Control: without the guards git refreshes the index, so the check below can fail.
        let (control, index) = stale_index("control");
        let before = std::fs::read(&index).unwrap();
        git(&control.0, &["status", "--porcelain"]);
        assert_ne!(
            std::fs::read(&index).unwrap(),
            before,
            "plain git status rewrites a stale index"
        );

        let (guarded, index) = stale_index("guarded");
        let before = std::fs::read(&index).unwrap();
        assert!(matches!(
            status_with(&guarded.0, LIMIT, ENV),
            Ok(GitStatus::Repo { changes: 0, .. })
        ));
        assert!(!guarded.0.join(".git/index.lock").exists());
        assert_eq!(std::fs::read(&index).unwrap(), before, "index untouched");
    }

    #[test]
    fn a_clean_filter_set_in_the_repository_is_never_run() {
        let (root, _) = stale_index("filter");
        let marker = root.0.join("ran");
        git(
            &root.0,
            &[
                "config",
                "filter.x.clean",
                &format!("touch {} && cat", marker.display()),
            ],
        );
        std::fs::write(root.0.join(".gitattributes"), "*.txt filter=x\n").unwrap();
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(root.0.join("same.txt"), "same\n").unwrap();
        let _ = std::fs::remove_file(&marker);
        assert!(matches!(
            status_with(&root.0, LIMIT, ENV),
            Err(GitError::Refused)
        ));
        assert!(!marker.exists(), "the repository's filter ran");
    }

    #[test]
    fn a_git_that_runs_past_the_limit_is_killed_with_its_children() {
        let root = temp("slow");
        let bin = root.0.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let pidfile = root.0.join("child.pid");
        let fake = bin.join("git");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\nsleep 30 &\necho $! > '{}'\nsleep 30\n",
                pidfile.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!("{}:/usr/bin:/bin", bin.display());
        let started = Instant::now();
        let got = status_inner(
            &root.0,
            Duration::from_millis(300),
            &[("PATH", path.as_str())],
            false,
        );
        assert_eq!(got, Err(GitError::Timeout));
        assert!(started.elapsed() < Duration::from_secs(3));
        let child = std::fs::read_to_string(&pidfile).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            !Path::new(&format!("/proc/{}", child.trim())).exists(),
            "the grandchild outlived the limit"
        );
    }

    #[test]
    fn a_branch_without_upstream_has_no_ahead_count() {
        let root = temp("local");
        git(&root.0, &["init", "-q", "-b", "solo"]);
        std::fs::write(root.0.join("x"), "x").unwrap();
        assert_eq!(
            status_with(&root.0, LIMIT, ENV),
            Ok(GitStatus::Repo {
                branch: Some("solo".into()),
                changes: 1,
                ahead: None,
                truncated: false
            })
        );
    }

    #[test]
    fn a_folder_outside_any_repository_is_no_repo() {
        let root = temp("plain");
        // GIT_CEILING_DIRECTORIES keeps git from finding a repository above the temp folder.
        let ceiling = root.0.parent().unwrap().to_str().unwrap().to_owned();
        let mut env = ENV.to_vec();
        env.push(("GIT_CEILING_DIRECTORIES", &ceiling));
        assert_eq!(status_with(&root.0, LIMIT, &env), Ok(GitStatus::NoRepo));
    }

    #[test]
    fn a_relative_or_missing_folder_is_refused() {
        assert_eq!(status(Path::new("relative")), Err(GitError::Failed));
        assert_eq!(
            status(Path::new("/nonexistent/wyss/git/status")),
            Err(GitError::Failed)
        );
    }

    #[test]
    fn filters_of_the_repository_are_refused_and_the_owners_own_are_kept() {
        assert!(repository_filters(b"local\tfilter.x.clean\n"));
        assert!(repository_filters(
            b"global\tfilter.lfs.clean\nworktree\tfilter.y.process\n"
        ));
        assert!(repository_filters(b"command\tfilter.z.smudge\n"));
        assert!(!repository_filters(
            b"global\tfilter.lfs.clean\nsystem\tfilter.lfs.process\n"
        ));
        assert!(!repository_filters(b""));
    }

    #[test]
    fn porcelain_lines_are_read_as_branch_ahead_and_changes() {
        let out = b"# branch.oid abc\n# branch.head (detached)\n1 .M N... 1 1 1 a a f\n? new\nu UU N... x\n";
        assert_eq!(
            parse(out, false),
            GitStatus::Repo {
                branch: None,
                changes: 3,
                ahead: None,
                truncated: false
            }
        );
        let out = b"# branch.head main\n# branch.upstream origin/main\n# branch.ab +4 -2\n";
        assert_eq!(
            parse(out, false),
            GitStatus::Repo {
                branch: Some("main".into()),
                changes: 0,
                ahead: Some(4),
                truncated: false
            }
        );
    }
}
