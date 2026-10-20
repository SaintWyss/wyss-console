//! P1 measurements of the terminal **backend** (architecture 15 §2: M06, M08, M09, M10, M14),
//! on the machine that runs them, with the real PTY, the real service and its real bounds.
//!
//! What this measures, and what it does not. It covers the path a program's output takes from the
//! PTY to the batch the window would read (and the path of a typed byte back), in one process, with
//! no window, no WebKit and no renderer. It is **not** the renderer's cost, not the page's input
//! latency and not the time to a drawn glyph: those need the packaged GUI and an instrument the
//! build does not have (OQ-26/27). Every figure is labelled with this boundary.
//!
//! All of it is `#[ignore]`: run by hand, in release, on a quiet machine:
//!
//! ```sh
//! WYSS_BENCH_OUT=/path/out.json cargo test --release --lib terminal::bench_tests -- \
//!     --ignored --nocapture --test-threads=1
//! ```
//!
//! Corpora are generated from a fixed seed (15 §11.2), in a private temporary file that the test
//! removes by its exact path. Nothing is dropped from a sample list; values beyond three
//! interquartile ranges are only flagged.
use super::*;
use serde_json::{Value, json};
use std::{fs, io::Write as _, path::Path};

// ---- statistics ------------------------------------------------------------------------------

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*: deterministic, so a published interval can be reproduced from its samples.
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    // Linear interpolation between order statistics (the common "type 7" estimate).
    let position = q * (sorted.len() - 1) as f64;
    let (low, high) = (position.floor() as usize, position.ceil() as usize);
    sorted[low] + (sorted[high] - sorted[low]) * (position - low as f64)
}

fn summary(samples: &[f64]) -> Value {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let median = quantile(&sorted, 0.5);
    let (q1, q3) = (quantile(&sorted, 0.25), quantile(&sorted, 0.75));
    let iqr = q3 - q1;
    // 95 % confidence interval of the median, by bootstrap, 10,000 resamples.
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ samples.len() as u64);
    let mut medians: Vec<f64> = (0..10_000)
        .map(|_| {
            let mut resample: Vec<f64> = (0..samples.len())
                .map(|_| samples[(rng.next() % samples.len() as u64) as usize])
                .collect();
            resample.sort_by(f64::total_cmp);
            quantile(&resample, 0.5)
        })
        .collect();
    medians.sort_by(f64::total_cmp);
    let flagged = samples
        .iter()
        .filter(|v| **v < q1 - 3.0 * iqr || **v > q3 + 3.0 * iqr)
        .count();
    json!({
        "n": samples.len(),
        "median": median, "p95": quantile(&sorted, 0.95), "p99": quantile(&sorted, 0.99),
        "min": sorted.first(), "max": sorted.last(), "q1": q1, "q3": q3, "iqr": iqr,
        "median_ci95": [quantile(&medians, 0.025), quantile(&medians, 0.975)],
        "beyond_3_iqr": flagged,
    })
}

// ---- the machine, as it is --------------------------------------------------------------------

fn read(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_default()
}
fn meminfo(key: &str) -> u64 {
    read("/proc/meminfo")
        .lines()
        .find_map(|l| {
            l.strip_prefix(key)?
                .trim_start_matches(':')
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
        .unwrap_or(0)
}
fn conditions() -> Value {
    let running: usize = fs::read_dir("/proc")
        .map(|d| {
            d.flatten()
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .bytes()
                        .all(|b| b.is_ascii_digit())
                })
                .count()
        })
        .unwrap_or(0);
    json!({
        "kernel": read("/proc/sys/kernel/osrelease").trim(),
        "loadavg": read("/proc/loadavg").trim(),
        "mem_available_kib": meminfo("MemAvailable"),
        "swap_used_kib": meminfo("SwapTotal").saturating_sub(meminfo("SwapFree")),
        "processes": running,
        "on_battery": read("/sys/class/power_supply/AC/online").trim() == "0",
        "cpu_model": read("/proc/cpuinfo").lines().find_map(|l| l.strip_prefix("model name")).map(|s| s.trim_start_matches([' ', '\t', ':']).to_owned()),
        "release_build": !cfg!(debug_assertions),
        "commit": option_env!("WYSS_BENCH_COMMIT"),
    })
}

fn report(name: &str, boundary: &str, body: Value) {
    let record = json!({
        "metric": name, "boundary": boundary, "conditions": conditions(), "result": body,
    });
    println!("P1-RESULT {record}");
    if let Some(path) = std::env::var_os("WYSS_BENCH_OUT") {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("the output file opens");
        writeln!(file, "{record}").expect("the record is written");
    }
}

// ---- corpora (15 §11.2), from a fixed seed ---------------------------------------------------

fn corpus_bc01(bytes: usize) -> Vec<u8> {
    let mut rng = Rng(0x0123_4567_89AB_CDEF);
    let mut out = Vec::with_capacity(bytes + 100);
    while out.len() < bytes {
        for _ in 0..79 {
            out.push(b' ' + (rng.next() % 95) as u8);
        }
        out.push(b'\n');
    }
    out.truncate(bytes);
    out
}
/// Colour on every cell: 256-colour and true-colour attributes, a line of 40 cells.
fn corpus_bc02(bytes: usize) -> Vec<u8> {
    let mut rng = Rng(0x2468_ACE0_1357_9BDF);
    let mut out = Vec::with_capacity(bytes + 1024);
    while out.len() < bytes {
        for cell in 0..40 {
            let v = rng.next();
            if cell % 2 == 0 {
                out.extend_from_slice(format!("\x1b[38;5;{}m", v % 256).as_bytes());
            } else {
                out.extend_from_slice(
                    format!(
                        "\x1b[48;2;{};{};{}m",
                        v & 255,
                        (v >> 8) & 255,
                        (v >> 16) & 255
                    )
                    .as_bytes(),
                );
            }
            out.push(b'A' + (v % 26) as u8);
        }
        out.extend_from_slice(b"\x1b[0m\n");
    }
    out.truncate(bytes);
    out
}
/// Wide characters: CJK, combining marks and emoji, whole characters only.
fn corpus_bc03(bytes: usize) -> Vec<u8> {
    let pool = [
        "日", "本", "語", "한", "国", "e\u{301}", "a\u{308}", "😀", "🚀", "界", "A", "z",
    ];
    let mut rng = Rng(0x1357_9BDF_2468_ACE0);
    let mut out = Vec::with_capacity(bytes + 16);
    let mut line = 0;
    while out.len() < bytes {
        out.extend_from_slice(pool[(rng.next() % pool.len() as u64) as usize].as_bytes());
        line += 1;
        if line == 40 {
            out.push(b'\n');
            line = 0;
        }
    }
    // Never cut a character: stop at the last newline that fits.
    let cut = out[..bytes.min(out.len())]
        .iter()
        .rposition(|b| *b == b'\n')
        .map_or(0, |i| i + 1);
    out.truncate(cut);
    out
}
/// Very long lines without newlines: 64 KiB between newlines.
fn corpus_bc04(bytes: usize) -> Vec<u8> {
    let mut rng = Rng(0x0F0F_0F0F_1234_5678);
    let mut out = Vec::with_capacity(bytes + 1);
    while out.len() < bytes {
        for _ in 0..65_535 {
            out.push(b'!' + (rng.next() % 90) as u8);
        }
        out.push(b'\n');
    }
    out.truncate(bytes);
    out
}

// ---- harness -----------------------------------------------------------------------------------

struct Rig {
    service: TerminalService,
}
impl Rig {
    fn new() -> Self {
        Self {
            service: TerminalService::default(),
        }
    }
    fn open(&self, program: &str, args: &[&str]) -> TerminalInfo {
        self.service
            .create_launch(
                CreateTerminal {
                    size: TerminalSize {
                        rows: 40,
                        cols: 120,
                    },
                    cwd: Some("/".into()),
                },
                Launch {
                    program: PathBuf::from(program),
                    args: args.iter().map(|a| (*a).to_owned()).collect(),
                    env: vec![],
                    remove_env: vec![],
                },
            )
            .expect("a terminal opens")
    }
    /// Drains batches until `marker` is seen at the end of what was read; returns bytes read and
    /// the acknowledgement the next read must carry (the last batch is still held until then).
    fn drain_until(
        &self,
        id: &TerminalId,
        marker: &[u8],
        limit: Duration,
    ) -> (usize, Option<WireUuid>) {
        let deadline = Instant::now() + limit;
        let mut ack: Option<WireUuid> = None;
        let mut total = 0;
        let mut tail: Vec<u8> = Vec::new();
        loop {
            assert!(Instant::now() < deadline, "the marker never came");
            let batch = self.service.read(id, ack.as_ref()).expect("a read");
            ack = batch.batch_id;
            total += batch.data.len();
            tail.extend_from_slice(&batch.data);
            if tail.len() > marker.len() * 4 {
                tail.drain(..tail.len() - marker.len() * 2);
            }
            if tail.windows(marker.len()).any(|w| w == marker) {
                return (total, ack);
            }
            if batch.eof {
                panic!("the program ended before the marker");
            }
            if batch.data.is_empty() {
                let runtime = self.service.runtime(id).expect("the runtime");
                let state = lock(&runtime.shared.data);
                if state.output.len() == 0 {
                    drop(
                        runtime
                            .shared
                            .changed
                            .wait_timeout(state, Duration::from_millis(5))
                            .expect("wait"),
                    );
                }
            }
        }
    }
}

use crate::contracts::WireUuid;

fn temporary(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("wyss-p1-{}-{name}", uuid::Uuid::new_v4().simple()))
}

fn throughput(name: &str, data: &[u8]) {
    let file = temporary(name);
    fs::write(&file, data).expect("the corpus is written");
    let rig = Rig::new();
    // The shell prints the marker after the cat; the byte count includes the terminal's own echo.
    let script = format!("cat {}; printf '\\nP1-DONE-MARKER\\n'", file.display());
    let mut seconds = Vec::new();
    let mut rates = Vec::new();
    for run in 0..11 {
        let terminal = rig.open("/bin/sh", &["-c", &script]);
        let started = Instant::now();
        let (read, _) = rig.drain_until(
            &terminal.terminal_id,
            b"P1-DONE-MARKER",
            Duration::from_secs(300),
        );
        let elapsed = started.elapsed().as_secs_f64();
        let _ = rig.service.close(&terminal.terminal_id);
        // The first run after generation is discarded (15 §3.2), and recorded as such.
        if run > 0 {
            seconds.push(elapsed);
            rates.push(read as f64 / elapsed / (1024.0 * 1024.0));
        }
    }
    fs::remove_file(&file).expect("the corpus is removed by its exact path");
    report(
        name,
        "PTY to the batch the window would read, in one process; no renderer, no webview",
        json!({
            "corpus_bytes": data.len(),
            "runs_discarded": 1,
            "seconds": summary(&seconds),
            "mib_per_second": summary(&rates),
            "raw_seconds": seconds,
        }),
    );
}

/// Resident and proportional memory of this process and of every process below it.
fn tree_pss_kib() -> u64 {
    fn pss(pid: u32) -> u64 {
        read(&format!("/proc/{pid}/smaps_rollup"))
            .lines()
            .find_map(|l| {
                l.strip_prefix("Pss:")?
                    .split_whitespace()
                    .next()?
                    .parse()
                    .ok()
            })
            .unwrap_or(0)
    }
    fn below(pid: u32, found: &mut Vec<u32>) {
        for task in fs::read_dir(format!("/proc/{pid}/task"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let children = read(&task.path().join("children").to_string_lossy());
            for child in children
                .split_whitespace()
                .filter_map(|c| c.parse::<u32>().ok())
            {
                if !found.contains(&child) && found.len() < 256 {
                    found.push(child);
                    below(child, found);
                }
            }
        }
    }
    let me = std::process::id();
    let mut all = vec![me];
    below(me, &mut all);
    all.iter().map(|p| pss(*p)).sum()
}

#[test]
#[ignore = "P1 measurement: run by hand (see the module documentation)"]
fn p1_m09_throughput_bc01_plain_ascii_64_mib() {
    throughput("M09-BC-01", &corpus_bc01(64 * 1024 * 1024));
}
#[test]
#[ignore = "P1 measurement: run by hand (see the module documentation)"]
fn p1_m10_throughput_bc02_dense_colour_16_mib() {
    throughput("M10-BC-02", &corpus_bc02(16 * 1024 * 1024));
}
#[test]
#[ignore = "P1 measurement: run by hand (see the module documentation)"]
fn p1_m10_throughput_bc03_wide_characters_16_mib() {
    throughput("M10-BC-03", &corpus_bc03(16 * 1024 * 1024));
}
#[test]
#[ignore = "P1 measurement: run by hand (see the module documentation)"]
fn p1_m10_throughput_bc04_very_long_lines_8_mib() {
    throughput("M10-BC-04", &corpus_bc04(8 * 1024 * 1024));
}

#[test]
#[ignore = "P1 measurement: run by hand (see the module documentation)"]
fn p1_m08_typed_byte_round_trip_through_the_backend() {
    // `cat` on a raw, non-echoing terminal returns each byte the moment it is read: the time is
    // the backend's, from the write to the byte being readable as output. Three independent
    // sessions (new service, new terminal), 100 keys discarded and 1,000 measured each.
    let mut sessions = Vec::new();
    for _ in 0..3 {
        let rig = Rig::new();
        let terminal = rig.open("/bin/sh", &["-c", "stty raw -echo; printf READY; cat"]);
        let (_, mut ack) = rig.drain_until(&terminal.terminal_id, b"READY", Duration::from_secs(5));
        let mut micros = Vec::new();
        for key in 0..1100 {
            let started = Instant::now();
            rig.service
                .write(&terminal.terminal_id, vec![b'x'])
                .expect("a write");
            let deadline = started + Duration::from_secs(2);
            'seen: loop {
                assert!(Instant::now() < deadline, "the echo never came");
                let batch = rig
                    .service
                    .read(&terminal.terminal_id, ack.as_ref())
                    .expect("a read");
                ack = batch.batch_id;
                if !batch.data.is_empty() {
                    break 'seen;
                }
                let runtime = rig.service.runtime(&terminal.terminal_id).expect("runtime");
                let state = lock(&runtime.shared.data);
                if state.output.len() == 0 {
                    drop(
                        runtime
                            .shared
                            .changed
                            .wait_timeout(state, Duration::from_millis(1))
                            .expect("wait"),
                    );
                }
            }
            if key >= 100 {
                micros.push(started.elapsed().as_secs_f64() * 1e6);
            }
            // Ten keys per second, as the procedure says, without waiting a full 100 ms each time:
            // the pause only has to be long enough that keys are not batched together.
            std::thread::sleep(Duration::from_millis(2));
        }
        let _ = rig.service.close(&terminal.terminal_id);
        sessions.push(micros);
    }
    let all: Vec<f64> = sessions.iter().flatten().copied().collect();
    report(
        "M08-backend",
        "write of one byte to the PTY until it is readable as output; no page, no renderer, no frame",
        json!({
            "unit": "microseconds",
            "sessions": sessions.iter().map(|s| summary(s)).collect::<Vec<_>>(),
            "all": summary(&all),
            "pause_between_keys_ms": 2,
        }),
    );
}

#[test]
#[ignore = "P1 measurement: run by hand (see the module documentation)"]
fn p1_m06_backend_memory_per_idle_terminal() {
    // Proportional memory of this process and its shells with 0, 1, 2, 4, 8 and 16 idle shells.
    // The scrollback of a terminal lives in the page (xterm.js), not here: this is the backend's
    // share only (the PTY, its reader thread, its 64 KiB buffer and the shell process).
    let mut sessions = Vec::new();
    for _ in 0..3 {
        let rig = Rig::new();
        let mut open: Vec<TerminalInfo> = Vec::new();
        let mut ready = 0usize;
        let mut by_count = serde_json::Map::new();
        std::thread::sleep(Duration::from_millis(500));
        by_count.insert("0".into(), json!(tree_pss_kib()));
        for target in [1usize, 2, 4, 8, 16] {
            while open.len() < target {
                open.push(rig.open("/bin/sh", &["-c", "printf READY; exec sleep 3600"]));
            }
            // Only the new ones are read: an earlier one still holds its last batch.
            for t in &open[ready..] {
                rig.drain_until(&t.terminal_id, b"READY", Duration::from_secs(5));
            }
            ready = open.len();
            // Two minutes idle after filling would be canonical (15 §3.2); a short settling pause
            // is used here and said so.
            std::thread::sleep(Duration::from_secs(3));
            by_count.insert(target.to_string(), json!(tree_pss_kib()));
        }
        for t in &open {
            let _ = rig.service.close(&t.terminal_id);
        }
        sessions.push(Value::Object(by_count));
    }
    let per_terminal: Vec<f64> = sessions
        .iter()
        .map(|s| (s["16"].as_u64().unwrap() as f64 - s["0"].as_u64().unwrap() as f64) / 16.0)
        .collect();
    report(
        "M06-backend",
        "PSS of this process plus its shells, kiB; the page's scrollback is not here",
        json!({
            "sessions": sessions,
            "kib_per_terminal_0_to_16": summary(&per_terminal),
            "settling_seconds_per_step": 3,
        }),
    );
}

#[test]
#[ignore = "P1 measurement: run by hand (see the module documentation); takes about 80 seconds"]
fn p1_m14_runaway_output_stays_bounded_and_a_stalled_reader_blocks_the_program() {
    // 1. `yes` for sixty seconds with a reader that keeps up: memory must stay flat.
    let rig = Rig::new();
    let terminal = rig.open("/bin/sh", &["-c", "exec yes"]);
    let mut ack: Option<WireUuid> = None;
    let started = Instant::now();
    let mut series = Vec::new();
    let mut bytes = 0usize;
    let mut next_sample = Instant::now();
    while started.elapsed() < Duration::from_secs(60) {
        let batch = rig
            .service
            .read(&terminal.terminal_id, ack.as_ref())
            .expect("a read");
        ack = batch.batch_id;
        bytes += batch.data.len();
        if Instant::now() >= next_sample {
            series.push(tree_pss_kib());
            next_sample += Duration::from_secs(1);
        }
        if batch.data.is_empty() {
            std::thread::sleep(Duration::from_micros(200));
        }
    }
    let _ = rig.service.close(&terminal.terminal_id);
    // 2. The same with nobody reading for ten seconds: the buffer fills, the program blocks, memory
    //    does not grow, and nothing is dropped (the buffer's own bound, PP6).
    let rig = Rig::new();
    let terminal = rig.open("/bin/sh", &["-c", "exec yes"]);
    std::thread::sleep(Duration::from_secs(2));
    let before = tree_pss_kib();
    std::thread::sleep(Duration::from_secs(8));
    let after = tree_pss_kib();
    let (buffered, truncated) = {
        let runtime = rig.service.runtime(&terminal.terminal_id).expect("runtime");
        let state = lock(&runtime.shared.data);
        (state.output.len(), state.output.truncated)
    };
    let _ = rig.service.close(&terminal.terminal_id);
    let first = *series.first().unwrap_or(&0) as f64;
    let slope = if series.len() > 10 {
        // Least-squares slope in kiB per minute over the series.
        let n = series.len() as f64;
        let xs: Vec<f64> = (0..series.len()).map(|i| i as f64).collect();
        let (mx, my) = (
            xs.iter().sum::<f64>() / n,
            series.iter().map(|v| *v as f64).sum::<f64>() / n,
        );
        let num: f64 = xs
            .iter()
            .zip(&series)
            .map(|(x, y)| (x - mx) * (*y as f64 - my))
            .sum();
        let den: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
        num / den * 60.0
    } else {
        f64::NAN
    };
    report(
        "M14-backend",
        "`yes` for sixty seconds, drained, then ten seconds not drained; PSS of process plus children, kiB",
        json!({
            "drained_seconds": 60,
            "drained_mib_read": bytes as f64 / 1048576.0,
            "drained_mib_per_second": bytes as f64 / 1048576.0 / 60.0,
            "pss_first_sample_kib": first,
            "pss_max_kib": series.iter().max(),
            "pss_slope_kib_per_minute": slope,
            "undrained": {
                "pss_before_kib": before, "pss_after_kib": after,
                "buffered_bytes": buffered, "output_capacity": OUTPUT_CAPACITY, "truncated": truncated,
            },
        }),
    );
    assert!(buffered <= OUTPUT_CAPACITY);
    assert!(
        !truncated,
        "output was dropped instead of the program being slowed"
    );
    let _ = Path::new("/");
}
