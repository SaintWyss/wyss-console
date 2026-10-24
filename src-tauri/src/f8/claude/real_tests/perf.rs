//! What the real stack costs, measured offline: one line per run, `PERF {json}` on stderr, which
//! `benchmarks/m3_perf.py` collects and summarizes. Nothing here asserts a budget: the owner
//! accepts budgets from repeated, comparable runs, and this only produces the runs.
//!
//! `WYSS_CLAUDE_SDK_ROOT=<dir> WYSS_PERF_RUNS=7 cargo test --lib real_tests::perf -- --ignored --nocapture`
use super::*;
use std::collections::HashMap;

/// Linux counts CPU time in 1/100 s ticks for `/proc/<pid>/stat`.
const TICKS_PER_SECOND: f64 = 100.0;

#[derive(Default, Clone, Copy)]
struct Tree {
    procs: u64,
    threads: u64,
    fds: u64,
    rss_kib: u64,
    pss_kib: u64,
    ticks: u64,
    switches: u64,
}
impl Tree {
    fn json(self) -> Value {
        json!({"processes": self.procs, "threads": self.threads, "fds": self.fds,
               "rss_kib": self.rss_kib, "pss_kib": self.pss_kib})
    }
}

fn after_comm(pid: u32) -> Option<Vec<String>> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &stat[stat.rfind(')')? + 1..];
    Some(rest.split_whitespace().map(str::to_owned).collect())
}
fn kib(text: &str, key: &str) -> u64 {
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|v| v.split_whitespace().next())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Every descendant of this test process, except the subtrees of `skip` (the mock provider).
fn tree(skip: &[u32]) -> Tree {
    let mut kids: HashMap<u32, Vec<u32>> = HashMap::new();
    for e in fs::read_dir("/proc").unwrap().flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        if let Some(f) = after_comm(pid)
            && let Ok(ppid) = f[1].parse::<u32>()
        {
            kids.entry(ppid).or_default().push(pid);
        }
    }
    let mut out = Tree::default();
    let mut todo: Vec<u32> = kids.get(&std::process::id()).cloned().unwrap_or_default();
    while let Some(pid) = todo.pop() {
        if skip.contains(&pid) {
            continue;
        }
        todo.extend(kids.get(&pid).cloned().unwrap_or_default());
        let Some(f) = after_comm(pid) else { continue };
        let status = fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
        let smaps = fs::read_to_string(format!("/proc/{pid}/smaps_rollup")).unwrap_or_default();
        out.procs += 1;
        out.threads += f[17].parse::<u64>().unwrap_or(0);
        out.fds += fs::read_dir(format!("/proc/{pid}/fd"))
            .map(|d| d.count() as u64)
            .unwrap_or(0);
        out.rss_kib += kib(&status, "VmRSS:");
        out.pss_kib += kib(&smaps, "Pss:");
        out.ticks += f[11].parse::<u64>().unwrap_or(0) + f[12].parse::<u64>().unwrap_or(0);
        out.switches +=
            kib(&status, "voluntary_ctxt_switches:") + kib(&status, "nonvoluntary_ctxt_switches:");
    }
    out
}

/// CPU ticks per process name, for saying which part of the stack an idle cost belongs to.
fn ticks_by_name(skip: &[u32]) -> HashMap<String, u64> {
    let mut kids: HashMap<u32, Vec<u32>> = HashMap::new();
    for e in fs::read_dir("/proc").unwrap().flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        if let Some(f) = after_comm(pid)
            && let Ok(ppid) = f[1].parse::<u32>()
        {
            kids.entry(ppid).or_default().push(pid);
        }
    }
    let mut out: HashMap<String, u64> = HashMap::new();
    let mut todo: Vec<u32> = kids.get(&std::process::id()).cloned().unwrap_or_default();
    while let Some(pid) = todo.pop() {
        if skip.contains(&pid) {
            continue;
        }
        todo.extend(kids.get(&pid).cloned().unwrap_or_default());
        let (Some(f), Ok(name)) = (
            after_comm(pid),
            fs::read_to_string(format!("/proc/{pid}/comm")),
        ) else {
            continue;
        };
        *out.entry(name.trim().to_owned()).or_default() +=
            f[11].parse::<u64>().unwrap_or(0) + f[12].parse::<u64>().unwrap_or(0);
    }
    out
}

fn ms(t: Instant) -> f64 {
    (t.elapsed().as_secs_f64() * 1000.0 * 10.0).round() / 10.0
}

fn run_once(profile: Profile) -> Value {
    let mock = Mock::start(mock_text("a short scripted answer for the measurement"));
    let skip = [mock.child.id()];
    let state = dir("state");
    let t_start = Instant::now();
    let mut r = start(&mock, profile, SESSION, &state, |_| ());
    r.ready();
    let ready = ms(t_start);
    let t = Instant::now();
    r.bind(SESSION);
    let bind = ms(t);
    let at_ready = tree(&skip);
    let t = Instant::now();
    r.turn("t1", "measure");
    r.pump(|n| n.iter().any(|x| x["kind"] == "content"), 60)
        .unwrap();
    let first_content = ms(t);
    r.pump(|n| n.iter().any(|x| x["kind"] == "turn_completed"), 60)
        .unwrap();
    let turn = ms(t);
    let after_turn = tree(&skip);
    // Idle: the stack is up, nothing is asked of it.
    let idle_from = tree(&skip);
    let names_from = ticks_by_name(&skip);
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5) {
        // What a host does while idle: look for anything the runtime has to say.
        let _ = r.w.flush();
        let _ = r.w.inbound(16);
        std::thread::sleep(Duration::from_millis(200));
    }
    let idle_to = tree(&skip);
    let names_to = ticks_by_name(&skip);
    let idle_s = t.elapsed().as_secs_f64();
    let by_name: serde_json::Map<String, Value> = names_to
        .iter()
        .map(|(n, to)| {
            let d = to.saturating_sub(names_from.get(n).copied().unwrap_or(0));
            (
                n.clone(),
                json!((d as f64 / TICKS_PER_SECOND / idle_s * 1000.0).round() / 10.0),
            )
        })
        .collect();
    let t = Instant::now();
    r.stop();
    let stop = ms(t);
    let left = tree(&skip).procs;
    json!({
        "profile": profile.attachment_name(),
        "ready_ms": ready, "bind_ms": bind, "first_content_ms": first_content, "turn_ms": turn,
        "stop_ms": stop, "left_after_stop": left,
        "at_ready": at_ready.json(), "after_turn": after_turn.json(),
        "idle": {
            "seconds": (idle_s * 10.0).round() / 10.0,
            "cpu_percent": ((idle_to.ticks.saturating_sub(idle_from.ticks)) as f64
                / TICKS_PER_SECOND / idle_s * 1000.0).round() / 10.0,
            "cpu_percent_by_process": by_name,
            "context_switches_per_s": ((idle_to.switches.saturating_sub(idle_from.switches)) as f64
                / idle_s * 10.0).round() / 10.0,
        },
    })
}

/// A turn that is still streaming is interrupted; the time is from the request to the notice.
fn cancel_once() -> Value {
    let text = "slow slow slow slow slow slow slow slow slow slow slow slow slow slow slow slow";
    let mock = Mock::start(json!([{"text": text, "delay": 0.3}]));
    let state = dir("state");
    let mut r = start(&mock, Profile::Inference, SESSION, &state, |_| ());
    r.ready();
    r.bind(SESSION);
    r.turn("t1", "stream");
    r.pump(|n| n.iter().any(|x| x["kind"] == "content"), 60)
        .unwrap();
    let t = Instant::now();
    r.send(json!({"method":"turn/interrupt"})).unwrap();
    r.pump(|n| n.iter().any(|x| x["kind"] == "turn_interrupted"), 30)
        .unwrap();
    let cancel = ms(t);
    r.stop();
    json!({"cancel_ms": cancel})
}

#[test]
#[ignore = "needs WYSS_CLAUDE_SDK_ROOT; measurements, not assertions"]
fn measure_the_real_stack() {
    let runs: usize = std::env::var("WYSS_PERF_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(7);
    for i in 0..runs {
        for profile in [Profile::Inference, Profile::LocalTools] {
            let mut v = run_once(profile);
            v["run"] = json!(i);
            eprintln!("PERF {v}");
        }
        let mut c = cancel_once();
        c["run"] = json!(i);
        eprintln!("PERF {c}");
    }
}
