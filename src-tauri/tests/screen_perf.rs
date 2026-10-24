//! M4 measurements of the screen engine and its helper process. Run in release, on purpose:
//!
//!   cargo test --release --test screen_perf -- --ignored --nocapture --test-threads=1
//!
//! Every figure is printed as a JSON line with its raw samples, p50 and p95 over repeated runs;
//! nothing is dropped. They are measurements of this machine at the time, not budgets.

use std::{
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use wyss_console::screen::{
    Limits, Terminal,
    ipc::{self, MAX_REQUEST, MAX_RESPONSE, PROTOCOL, Request, Response, layer},
};

const HELPER: &str = env!("CARGO_BIN_EXE_wyss-screen");

fn corpus() -> Vec<(String, Vec<u8>)> {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/screen");
    let mut out: Vec<(String, Vec<u8>)> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            let n = p.file_name()?.to_str()?.strip_suffix(".bin")?.to_owned();
            Some((n, std::fs::read(&p).unwrap()))
        })
        .collect();
    out.sort();
    out
}

fn pct(sorted: &[f64], q: f64) -> f64 {
    sorted[((sorted.len() as f64 * q) as usize).min(sorted.len() - 1)]
}
fn report(what: &str, unit: &str, mut samples: Vec<f64>, extra: &str) {
    let raw = samples.clone();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "{{\"measure\":\"{what}\",\"unit\":\"{unit}\",\"n\":{},\"p50\":{:.3},\"p95\":{:.3},\"min\":{:.3},\"max\":{:.3},{extra}\"raw\":{:?}}}",
        samples.len(),
        pct(&samples, 0.5),
        pct(&samples, 0.95),
        samples[0],
        samples[samples.len() - 1],
        raw.iter()
            .map(|v| (v * 1000.0).round() / 1000.0)
            .collect::<Vec<_>>()
    );
}

#[test]
#[ignore = "measurement"]
fn parser_throughput_and_repaint_latency() {
    let all: Vec<u8> = corpus().into_iter().flat_map(|(_, b)| b).collect();
    // The corpus is small; it is cycled to about 32 MiB so that a run takes long enough to measure.
    let target = 32 * 1024 * 1024;
    let cycled: Vec<u8> = all.iter().copied().cycle().take(target).collect();
    let mut rates = Vec::new();
    for _ in 0..7 {
        let mut t = Terminal::new(24, 80, Limits::default());
        let started = Instant::now();
        for piece in cycled.chunks(16 * 1024) {
            t.feed(piece);
        }
        rates.push(target as f64 / (1024.0 * 1024.0) / started.elapsed().as_secs_f64());
    }
    report("parser_throughput_corpus_mix", "MiB/s", rates, "");
    // Plain text, the common case of a build log.
    let text: Vec<u8> = b"compiling crate-name v1.2.3 (/some/path/of/a/project) ... ok\r\n"
        .iter()
        .copied()
        .cycle()
        .take(target)
        .collect();
    let mut rates = Vec::new();
    for _ in 0..7 {
        let mut t = Terminal::new(24, 80, Limits::default());
        let started = Instant::now();
        for piece in text.chunks(16 * 1024) {
            t.feed(piece);
        }
        rates.push(target as f64 / (1024.0 * 1024.0) / started.elapsed().as_secs_f64());
    }
    report("parser_throughput_plain_text", "MiB/s", rates, "");

    for (rows, cols, lines) in [(24usize, 80usize, 2000usize), (60, 200, 2000), (24, 80, 0)] {
        let mut t = Terminal::new(rows, cols, Limits::default());
        for n in 0..lines + rows {
            t.feed(format!("\x1b[3{}mline {n} of the output with some text to fill the width of the screen\x1b[0m\r\n", n % 8).as_bytes());
        }
        let mut micros = Vec::new();
        let mut size = 0;
        for _ in 0..30 {
            let started = Instant::now();
            let r = t.repaint(lines);
            micros.push(started.elapsed().as_secs_f64() * 1000.0);
            size = r.bytes.len();
        }
        report(
            &format!("repaint_{rows}x{cols}_history_{lines}"),
            "ms",
            micros,
            &format!("\"bytes\":{size},"),
        );
    }
}

fn spawn() -> (
    std::process::Child,
    std::process::ChildStdin,
    std::process::ChildStdout,
) {
    let mut child = Command::new(HELPER)
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let i = child.stdin.take().unwrap();
    let o = child.stdout.take().unwrap();
    (child, i, o)
}
fn call(i: &mut impl std::io::Write, o: &mut impl Read, r: &Request) -> Response {
    ipc::write_frame(i, &r.encode(), MAX_REQUEST).unwrap();
    Response::decode(&ipc::read_frame(o, MAX_RESPONSE).unwrap().unwrap()).unwrap()
}
fn status_kib(pid: u32, key: &str) -> f64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .unwrap()
        .lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap()
        .parse()
        .unwrap()
}
fn hello() -> Request {
    Request::Hello {
        version: PROTOCOL,
        rows: 24,
        cols: 80,
        scrollback_lines: 2000,
        width_table: 0,
        require: layer::ALL,
    }
}

#[test]
#[ignore = "measurement"]
fn helper_startup_memory_round_trip_and_throughput() {
    // Start to ready, with every isolation layer applied.
    let mut ready = Vec::new();
    let mut idle_rss = Vec::new();
    for _ in 0..30 {
        let started = Instant::now();
        let (mut c, mut i, mut o) = spawn();
        let r = call(&mut i, &mut o, &hello());
        ready.push(started.elapsed().as_secs_f64() * 1000.0);
        assert!(matches!(r, Response::Ready { .. }));
        idle_rss.push(status_kib(c.id(), "VmRSS:") / 1024.0);
        let _ = call(&mut i, &mut o, &Request::Ping { nonce: 1 });
        drop(i);
        let _ = c.wait();
    }
    report("helper_start_to_ready", "ms", ready, "");
    report("helper_idle_rss", "MiB", idle_rss, "");

    // One helper, loaded: a screen with a full history, then round trips and a flood.
    let (mut c, mut i, mut o) = spawn();
    call(&mut i, &mut o, &hello());
    let mut offset = 0u64;
    let line = b"\x1b[32mgreen\x1b[0m output line with ordinary text that fills about eighty columns of a row\r\n";
    for _ in 0..3000 {
        let mut chunk = Vec::new();
        for _ in 0..20 {
            chunk.extend_from_slice(line);
        }
        call(
            &mut i,
            &mut o,
            &Request::Feed {
                offset,
                bytes: chunk.clone(),
            },
        );
        offset += chunk.len() as u64;
    }
    report(
        "helper_loaded_rss",
        "MiB",
        vec![status_kib(c.id(), "VmRSS:") / 1024.0],
        "\"history_lines\":2000,",
    );
    let mut rtt = Vec::new();
    for n in 0..300u64 {
        let started = Instant::now();
        let r = call(&mut i, &mut o, &Request::Ping { nonce: n });
        rtt.push(started.elapsed().as_secs_f64() * 1000.0);
        assert!(matches!(r, Response::Pong { .. }));
    }
    report("ipc_ping_round_trip", "ms", rtt, "");
    let mut feed_rtt = Vec::new();
    for _ in 0..300 {
        let bytes = line.to_vec();
        let started = Instant::now();
        call(
            &mut i,
            &mut o,
            &Request::Feed {
                offset,
                bytes: bytes.clone(),
            },
        );
        offset += bytes.len() as u64;
        feed_rtt.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    report("ipc_feed_1_line_round_trip", "ms", feed_rtt, "");
    let mut snap = Vec::new();
    let mut size = 0;
    for _ in 0..30 {
        let started = Instant::now();
        let Response::Snapshot(s) = call(
            &mut i,
            &mut o,
            &Request::Snapshot {
                scrollback_lines: 500,
            },
        ) else {
            panic!()
        };
        snap.push(started.elapsed().as_secs_f64() * 1000.0);
        size = s.repaint.len();
    }
    report(
        "ipc_snapshot_round_trip_500_lines",
        "ms",
        snap,
        &format!("\"bytes\":{size},"),
    );
    let mut rates = Vec::new();
    let big: Vec<u8> = line.iter().copied().cycle().take(ipc::MAX_FEED).collect();
    for _ in 0..7 {
        let started = Instant::now();
        let frames = 256;
        for _ in 0..frames {
            call(
                &mut i,
                &mut o,
                &Request::Feed {
                    offset,
                    bytes: big.clone(),
                },
            );
            offset += big.len() as u64;
        }
        rates.push(
            (frames * big.len()) as f64 / (1024.0 * 1024.0) / started.elapsed().as_secs_f64(),
        );
    }
    report("ipc_feed_throughput_64KiB_frames", "MiB/s", rates, "");
    report(
        "helper_peak_rss_after_flood",
        "MiB",
        vec![status_kib(c.id(), "VmHWM:") / 1024.0],
        "",
    );
    let fds = std::fs::read_dir(format!("/proc/{}/fd", c.id()))
        .map(|d| d.count())
        .unwrap_or(0);
    let threads = std::fs::read_dir(format!("/proc/{}/task", c.id()))
        .unwrap()
        .count();
    println!(
        "{{\"measure\":\"helper_descriptors_and_threads\",\"descriptors\":{fds},\"threads\":{threads}}}"
    );
    // Idle: a helper that is waiting for the next frame uses no processor.
    let stat = |pid: u32| -> u64 {
        let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
        let f: Vec<&str> = s.rsplit_once(')').unwrap().1.split_whitespace().collect();
        f[11].parse::<u64>().unwrap() + f[12].parse::<u64>().unwrap()
    };
    let before = stat(c.id());
    let switches = |pid: u32| -> u64 {
        std::fs::read_to_string(format!("/proc/{pid}/status"))
            .unwrap()
            .lines()
            .filter(|l| l.starts_with("voluntary_ctxt_switches"))
            .map(|l| l.split_whitespace().nth(1).unwrap().parse::<u64>().unwrap())
            .sum()
    };
    let s0 = switches(c.id());
    std::thread::sleep(Duration::from_secs(10));
    println!(
        "{{\"measure\":\"helper_idle_10s\",\"cpu_ticks\":{},\"voluntary_context_switches\":{}}}",
        stat(c.id()) - before,
        switches(c.id()) - s0
    );
    drop(i);
    let _ = c.wait();
}
