//! Processor and memory of a session's process and everything it started (FR-NAT-07), read
//! from `/proc`. A process id is an observation here and never an identity (05 §5): it is
//! asked about only while the registry says the session runs, and only while the overview is
//! on screen.
use serde::Serialize;
use std::{collections::HashMap, fs};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    /// utime + stime of the whole tree, in clock ticks.
    pub ticks: u64,
    pub rss_bytes: u64,
    pub processes: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Usage {
    /// Percent of one core since the previous sample; `None` for the first.
    pub cpu_percent: Option<f32>,
    pub rss_bytes: u64,
    pub processes: u32,
}

struct Row {
    parent: i32,
    ticks: u64,
    rss_pages: u64,
}

/// `pid (comm) S ppid ...`: the name may hold spaces and parentheses, so fields are counted
/// from the last `)`.
fn row(stat: &str) -> Option<(i32, Row)> {
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let pid: i32 = stat[..open].trim().parse().ok()?;
    let rest: Vec<&str> = stat[close + 1..].split_whitespace().collect();
    // rest[0] is the state; ppid is rest[1]; utime rest[11]; stime rest[12]; rss rest[21].
    let field = |i: usize| rest.get(i)?.parse::<u64>().ok();
    Some((
        pid,
        Row {
            parent: i32::try_from(field(1)?).ok()?,
            ticks: field(11)? + field(12)?,
            rss_pages: field(21)?,
        },
    ))
}

/// The tree rooted at `root`, or `None` when that process is gone.
pub fn tree(root: i32) -> Option<Sample> {
    let mut rows: HashMap<i32, Row> = HashMap::new();
    for entry in fs::read_dir("/proc").ok()?.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<i32>().ok())
        else {
            continue;
        };
        if let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat"))
            && let Some((_, parsed)) = row(&stat)
        {
            rows.insert(pid, parsed);
        }
    }
    rows.get(&root)?;
    let mut in_tree = vec![root];
    let mut changed = true;
    while changed {
        changed = false;
        for (pid, parsed) in &rows {
            if !in_tree.contains(pid) && in_tree.contains(&parsed.parent) {
                in_tree.push(*pid);
                changed = true;
            }
        }
    }
    let page = nix::unistd::sysconf(nix::unistd::SysconfVar::PAGE_SIZE)
        .ok()
        .flatten()
        .unwrap_or(4096) as u64;
    Some(Sample {
        ticks: in_tree
            .iter()
            .filter_map(|p| rows.get(p))
            .map(|r| r.ticks)
            .sum(),
        rss_bytes: in_tree
            .iter()
            .filter_map(|p| rows.get(p))
            .map(|r| r.rss_pages * page)
            .sum(),
        processes: u32::try_from(in_tree.len()).unwrap_or(u32::MAX),
    })
}

/// Percent of one core between two samples `seconds` apart.
pub fn cpu_percent(before: &Sample, now: &Sample, seconds: f32) -> Option<f32> {
    let hz = nix::unistd::sysconf(nix::unistd::SysconfVar::CLK_TCK)
        .ok()
        .flatten()? as f32;
    (seconds > 0.0).then(|| {
        let ticks = now.ticks.saturating_sub(before.ticks) as f32;
        (ticks / hz / seconds * 100.0).max(0.0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stat_line_is_read_from_the_last_parenthesis() {
        // A name with a space and a ")" in it must not shift the fields.
        let stat =
            "123 (we ird) name) S 45 1 1 0 -1 4194304 100 0 0 0 7 5 0 0 20 0 1 0 100 1000 321 0";
        let (pid, parsed) = row(stat).unwrap();
        assert_eq!(
            (pid, parsed.parent, parsed.ticks, parsed.rss_pages),
            (123, 45, 12, 321)
        );
    }

    #[test]
    fn this_process_tree_is_measured_and_a_gone_one_is_not() {
        let me = i32::try_from(std::process::id()).unwrap();
        let mut child = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let sample = tree(me).unwrap();
        assert!(sample.processes >= 2, "the test and its sleeping child");
        assert!(sample.rss_bytes > 0);
        let _ = child.kill();
        let _ = child.wait();
        assert!(tree(i32::MAX - 7).is_none());
    }
}
