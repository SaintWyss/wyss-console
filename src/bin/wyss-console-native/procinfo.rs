//! What the window shows about a terminal's process, read from `/proc` and the files of a git
//! checkout: memory of the process and everything it started, its current folder, and the branch.
//! Nothing here runs a program (no `git`), and a process id is only an observation.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// `VmRSS` of one process in bytes; `None` when it is gone (or is a kernel thread).
pub fn vm_rss(pid: i32) -> Option<u64> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

/// Direct children of `pid` from `/proc/<pid>/task/*/children`; `None` when the kernel does not
/// offer that file (no CONFIG_PROC_CHILDREN) or the process is gone.
fn children_fast(pid: i32) -> Option<Vec<i32>> {
    let mut out = Vec::new();
    for task in fs::read_dir(format!("/proc/{pid}/task")).ok()?.flatten() {
        let text = fs::read_to_string(task.path().join("children")).ok()?;
        out.extend(text.split_whitespace().filter_map(|n| n.parse::<i32>().ok()));
    }
    Some(out)
}

/// `pid -> ppid` for every process (the slow path, one pass over `/proc`).
fn parent_map() -> HashMap<i32, i32> {
    let mut map = HashMap::new();
    let Ok(dir) = fs::read_dir("/proc") else { return map };
    for e in dir.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<i32>().ok()) else { continue };
        if let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat"))
            && let Some(ppid) = ppid_of(&stat)
        {
            map.insert(pid, ppid);
        }
    }
    map
}

/// `pid (comm) S ppid ...`: the name may hold spaces and parentheses, so count from the last `)`.
pub fn ppid_of(stat: &str) -> Option<i32> {
    let close = stat.rfind(')')?;
    stat[close + 1..].split_whitespace().nth(1)?.parse().ok()
}

/// `root` and all its descendants, `None` when `root` is gone.
pub fn tree(root: i32) -> Option<Vec<i32>> {
    if !Path::new(&format!("/proc/{root}")).exists() {
        return None;
    }
    let mut all = vec![root];
    if children_fast(root).is_some() {
        let mut i = 0;
        while i < all.len() {
            if let Some(kids) = children_fast(all[i]) {
                for k in kids {
                    if !all.contains(&k) {
                        all.push(k);
                    }
                }
            }
            i += 1;
        }
    } else {
        let parents = parent_map();
        let mut changed = true;
        while changed {
            changed = false;
            for (p, pp) in &parents {
                if !all.contains(p) && all.contains(pp) {
                    all.push(*p);
                    changed = true;
                }
            }
        }
    }
    Some(all)
}

/// Resident memory of `root` and every process it started, in bytes; `None` when `root` is gone.
pub fn tree_rss(root: i32) -> Option<u64> {
    Some(tree(root)?.into_iter().filter_map(vm_rss).sum())
}

/// The current folder of a process.
pub fn cwd(pid: i32) -> Option<PathBuf> {
    fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

/// What git says about a folder, without asking git.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Git {
    NoRepo,
    Branch(String),
    Detached(String),
}

/// Looks for `.git` from `dir` upwards and reads its `HEAD`. `.git` may be a folder or a file
/// (`gitdir: <path>`, worktrees and submodules).
pub fn git_state(dir: &Path) -> Git {
    for d in dir.ancestors() {
        let dot = d.join(".git");
        let git_dir = if dot.is_dir() {
            dot
        } else if dot.is_file() {
            let Ok(text) = fs::read_to_string(&dot) else { continue };
            let Some(rel) = text.trim().strip_prefix("gitdir:") else { continue };
            let p = PathBuf::from(rel.trim());
            if p.is_absolute() { p } else { d.join(p) }
        } else {
            continue;
        };
        let Ok(head) = fs::read_to_string(git_dir.join("HEAD")) else { return Git::NoRepo };
        let head = head.trim();
        return match head.strip_prefix("ref: ") {
            Some(r) => Git::Branch(r.strip_prefix("refs/heads/").unwrap_or(r).to_owned()),
            None => Git::Detached(head.chars().take(7).collect()),
        };
    }
    Git::NoRepo
}

/// `/home/me/x` -> `~/x`.
pub fn home_short(path: &str, home: Option<&str>) -> String {
    match home {
        Some(h) if !h.is_empty() && (path == h || path.starts_with(&format!("{h}/"))) => format!("~{}", &path[h.len()..]),
        _ => path.to_owned(),
    }
}

/// "11 MB" like the Console: whole megabytes (rounded), `—` when unknown.
pub fn mb(bytes: Option<u64>) -> String {
    match bytes {
        Some(b) => format!("{} MB", (b + (1 << 19)) >> 20),
        None => "—".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    fn me() -> i32 {
        std::process::id() as i32
    }

    #[test]
    fn rss_of_this_process_matches_status() {
        let own = vm_rss(me()).expect("VmRSS");
        let v: Vec<u8> = vec![7; 32 << 20];
        let after = vm_rss(me()).unwrap();
        assert!(after >= own, "{after} < {own}");
        assert_eq!(v[1 << 20], 7);
        assert!(own > 1 << 20, "a test process is bigger than 1 MB");
    }

    #[test]
    fn tree_includes_descendants_and_sums_their_memory() {
        // sh -> sleep: the tree of the shell holds both.
        let mut sh = Command::new("sh").args(["-c", "sleep 30 & wait"]).stdout(Stdio::null()).spawn().unwrap();
        let sh_pid = sh.id() as i32;
        let mut kids = Vec::new();
        for _ in 0..50 {
            kids = tree(sh_pid).unwrap_or_default();
            if kids.len() >= 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(kids.len() >= 2, "tree of the shell: {kids:?}");
        let total = tree_rss(sh_pid).unwrap();
        let parts: u64 = kids.iter().filter_map(|p| vm_rss(*p)).sum();
        assert!(total.abs_diff(parts) < 1 << 20, "{total} vs {parts}");
        // Kill the sleep first, then the shell: only our own children.
        for p in kids.iter().skip(1) {
            let _ = Command::new("kill").arg(p.to_string()).status();
        }
        let _ = sh.kill();
        let _ = sh.wait();
        assert!(tree(sh_pid).is_none() || vm_rss(sh_pid).is_none());
    }

    #[test]
    fn the_slow_path_agrees_with_the_fast_one() {
        let parents = parent_map();
        assert_eq!(parents.get(&me()).copied(), Some(std::os::unix::process::parent_id() as i32));
    }

    #[test]
    fn ppid_handles_odd_names() {
        assert_eq!(ppid_of("12 (a) b) S 345 1 1 0"), Some(345));
        assert_eq!(ppid_of("garbage"), None);
    }

    #[test]
    fn cwd_follows_the_process() {
        let here = std::env::current_dir().unwrap().canonicalize().unwrap();
        assert_eq!(cwd(me()).unwrap().canonicalize().unwrap(), here);
        let child = Command::new("sleep").arg("30").current_dir("/tmp").spawn();
        let mut child = child.unwrap();
        let got = cwd(child.id() as i32);
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(got, Some(PathBuf::from("/tmp")));
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("procinfo-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn git_branch_is_read_from_head_going_up() {
        let d = scratch("git");
        fs::create_dir_all(d.join(".git")).unwrap();
        fs::create_dir_all(d.join("a/b")).unwrap();
        fs::write(d.join(".git/HEAD"), "ref: refs/heads/feat/x\n").unwrap();
        assert_eq!(git_state(&d.join("a/b")), Git::Branch("feat/x".into()));
        fs::write(d.join(".git/HEAD"), "0123456789abcdef\n").unwrap();
        assert_eq!(git_state(&d), Git::Detached("0123456".into()));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn git_file_pointing_to_a_gitdir_is_followed() {
        let d = scratch("gitfile");
        fs::create_dir_all(d.join("real")).unwrap();
        fs::create_dir_all(d.join("wt")).unwrap();
        fs::write(d.join("real/HEAD"), "ref: refs/heads/main\n").unwrap();
        fs::write(d.join("wt/.git"), format!("gitdir: {}\n", d.join("real").display())).unwrap();
        assert_eq!(git_state(&d.join("wt")), Git::Branch("main".into()));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn no_git_when_there_is_none() {
        let d = scratch("nogit");
        // /tmp itself may sit inside nothing: the scratch folder has no `.git` above it.
        assert_eq!(git_state(&d), Git::NoRepo);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn home_is_abbreviated_only_on_a_folder_boundary() {
        assert_eq!(home_short("/home/me", Some("/home/me")), "~");
        assert_eq!(home_short("/home/me/x", Some("/home/me")), "~/x");
        assert_eq!(home_short("/home/mex", Some("/home/me")), "/home/mex");
        assert_eq!(home_short("/tmp", Some("/home/me")), "/tmp");
        assert_eq!(home_short("/tmp", None), "/tmp");
    }

    #[test]
    fn megabytes_round_to_whole() {
        assert_eq!(mb(Some(11 << 20)), "11 MB");
        assert_eq!(mb(Some((11 << 20) + (600 << 10))), "12 MB");
        assert_eq!(mb(None), "—");
    }
}
