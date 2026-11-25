//! Read-only provider readings. Only numerical usage is kept in the subscription cache.
use super::{catalogue::AgentId, store};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};
use uuid::Uuid;

const STALE_AFTER: u64 = 15 * 60;
const CHUNK: usize = 128 * 1024;
const MAX_LINE: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub used: f64,
    pub resets_at: Option<u64>,
    pub at: u64,
}

impl Window {
    pub fn stale(self, now: u64) -> bool {
        now.saturating_sub(self.at) >= STALE_AFTER || self.resets_at.is_some_and(|at| at <= now)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub five_hours: Option<Window>,
    pub weekly: Option<Window>,
}

impl Plan {
    fn merge(&mut self, new: Self) {
        for (old, new) in [
            (&mut self.five_hours, new.five_hours),
            (&mut self.weekly, new.weekly),
        ] {
            if let Some(new) = new
                && old.is_none_or(|old| {
                    old.at <= new.at && (old.used != new.used || old.resets_at != new.resets_at)
                })
            {
                *old = Some(new);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    pub plan: Plan,
    pub context: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Normal,
    Warning,
    Danger,
}

pub fn context_level(used: f64) -> Level {
    if used >= 75.0 {
        Level::Danger
    } else if used >= 50.0 {
        Level::Warning
    } else {
        Level::Normal
    }
}

fn percent(value: &Value) -> Option<f64> {
    value.as_f64().filter(|v| (0.0..=100.0).contains(v))
}

fn window(value: &Value, used: &str, at: u64) -> Option<Window> {
    Some(Window {
        used: percent(&value[used])?,
        resets_at: value["resets_at"].as_u64().filter(|v| *v > 0),
        at,
    })
}

pub(super) fn claude_reading(raw: &[u8], expected: Uuid, at: u64) -> Option<Reading> {
    let data: Value = serde_json::from_slice(raw).ok()?;
    if data["session_id"].as_str()?.parse::<Uuid>().ok()? != expected {
        return None;
    }
    Some(Reading {
        plan: Plan {
            five_hours: window(&data["rate_limits"]["five_hour"], "used_percentage", at),
            weekly: window(&data["rate_limits"]["seven_day"], "used_percentage", at),
        },
        context: percent(&data["context_window"]["used_percentage"]),
    })
}

/// Codex rollouts use RFC 3339 UTC. Round-trip the calendar fields to reject impossible dates.
fn codex_time(value: &Value) -> Option<u64> {
    let s = value.as_str()?.strip_suffix('Z')?;
    let (whole, fraction) = s.split_once('.').unwrap_or((s, "0"));
    if fraction.is_empty() || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let b = whole.as_bytes();
    if b.len() != 19
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let field = |start, end| {
        let bytes = b.get(start..end)?;
        if !bytes.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(bytes).ok()?.parse::<i32>().ok()
    };
    // SAFETY: tm is a C struct of integer and pointer fields, with zero-valid storage.
    let mut tm: nix::libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = field(0, 4)? - 1900;
    tm.tm_mon = field(5, 7)? - 1;
    tm.tm_mday = field(8, 10)?;
    tm.tm_hour = field(11, 13)?;
    tm.tm_min = field(14, 16)?;
    tm.tm_sec = field(17, 19)?;
    let before = calendar(&tm);
    // SAFETY: tm is initialized and the function is given exclusive access for normalization.
    let result = unsafe { nix::libc::timegm(&mut tm) };
    (calendar(&tm) == before).then_some(result)?.try_into().ok()
}

fn calendar(tm: &nix::libc::tm) -> [i32; 6] {
    [
        tm.tm_year, tm.tm_mon, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec,
    ]
}

fn codex_reading(data: &CountEnvelope) -> Option<Reading> {
    let event = &data.payload;
    if event.kind != "token_count" {
        return None;
    }
    let mut result = Reading::default();
    let info = &event.info;
    let last = &info["last_token_usage"];
    if let (Some(used), Some(total)) = (
        last["total_tokens"].as_u64(),
        info["model_context_window"]
            .as_u64()
            .filter(|total| *total > 0),
    ) && used <= total
        && last["input_tokens"].as_u64().is_some_and(|n| n > 0)
    {
        result.context = Some(100.0 * used as f64 / total as f64);
    }
    let rates = &event.rate_limits;
    if rates["limit_id"] == "codex"
        && let Some(at) = codex_time(&data.timestamp)
    {
        for slot in [&rates["primary"], &rates["secondary"]] {
            match slot["window_minutes"].as_u64() {
                Some(300) => result.plan.five_hours = window(slot, "used_percent", at),
                Some(10080) => result.plan.weekly = window(slot, "used_percent", at),
                _ => {}
            }
        }
        if rates["primary"]["window_minutes"] == rates["secondary"]["window_minutes"] {
            result.plan = Plan::default();
        }
    }
    Some(result)
}

#[derive(Default)]
struct Tail {
    identity: Option<(u64, u64)>,
    offset: u64,
    line: Vec<u8>,
    skipping: bool,
}

impl Tail {
    fn read(&mut self, file: &mut File, mut record: impl FnMut(&[u8])) -> io::Result<()> {
        let meta = file.metadata()?;
        let identity = (meta.dev(), meta.ino());
        if self.identity != Some(identity) || self.offset > meta.len() {
            *self = Self {
                identity: Some(identity),
                ..Self::default()
            };
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = Vec::new();
        file.take(CHUNK as u64).read_to_end(&mut bytes)?;
        self.offset += bytes.len() as u64;
        for part in bytes.split_inclusive(|&b| b == b'\n') {
            let completed = part.ends_with(b"\n");
            if !self.skipping && self.line.len() + part.len() <= MAX_LINE {
                self.line.extend_from_slice(part);
                if completed {
                    record(&self.line);
                }
            } else {
                self.skipping = true;
                self.line.clear();
            }
            if completed {
                self.skipping = false;
                self.line.clear();
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct RolloutKind {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct RolloutHeader {
    payload: RolloutIdentity,
}

#[derive(Deserialize)]
struct RolloutIdentity {
    id: String,
}

#[derive(Deserialize)]
struct CountEnvelope {
    #[serde(default)]
    timestamp: Value,
    payload: CountEvent,
}

#[derive(Deserialize)]
struct CountEvent {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    info: Value,
    #[serde(default)]
    rate_limits: Value,
}

#[derive(Default)]
struct Rollout {
    tail: Tail,
    thread: Option<Uuid>,
    reading: Reading,
}

impl Rollout {
    fn read(&mut self, mut file: File, expected: Uuid) -> io::Result<Reading> {
        let meta = file.metadata()?;
        if self.tail.identity != Some((meta.dev(), meta.ino())) || self.tail.offset > meta.len() {
            self.thread = None;
            self.reading = Reading::default();
        }
        self.tail.read(&mut file, |line| {
            // Skip prompt and output fields during deserialization. Only usage fields and
            // the header identity are retained.
            let Ok(envelope) = serde_json::from_slice::<RolloutKind>(line) else {
                return;
            };
            match envelope.kind.as_str() {
                "session_meta" => {
                    self.thread = serde_json::from_slice::<RolloutHeader>(line)
                        .ok()
                        .and_then(|header| header.payload.id.parse().ok());
                }
                "event_msg" if self.thread == Some(expected) => {
                    if let Ok(data) = serde_json::from_slice::<CountEnvelope>(line)
                        && let Some(reading) = codex_reading(&data)
                    {
                        self.reading.context = reading.context;
                        self.reading.plan.merge(reading.plan);
                    }
                }
                _ => {}
            }
        })?;
        Ok(self.reading)
    }
}

fn rollout_name(file: &Path, sessions: &Path) -> Option<Uuid> {
    let rest = file.strip_prefix(sessions).ok()?;
    let parts: Vec<_> = rest.iter().map(|p| p.to_str()).collect::<Option<_>>()?;
    let [year, month, day, name] = parts.as_slice() else {
        return None;
    };
    if ![(year, 4), (month, 2), (day, 2)]
        .iter()
        .all(|(s, n)| s.len() == *n && s.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let suffix = name.strip_prefix("rollout-")?.strip_suffix(".jsonl")?;
    let thread = suffix.get(20..56)?.parse().ok()?;
    let remainder = suffix.get(56..)?;
    (suffix.as_bytes().get(19) == Some(&b'-')
        && (remainder.is_empty()
            || remainder
                .strip_prefix('_')
                .is_some_and(|s| s.parse::<Uuid>().is_ok())))
    .then_some(thread)
}

/// Only an open, append-mode rollout of the PTY's process may bind it to a provider thread.
fn open_rollout(pid: u32, home: &Path, executable: &Path) -> Option<(File, Uuid)> {
    let sessions = home.join("sessions").canonicalize().ok()?;
    let proc = PathBuf::from(format!("/proc/{pid}"));
    if fs::read_link(proc.join("exe")).ok()? != executable {
        return None;
    }
    let mut match_file: Option<(File, Uuid)> = None;
    for entry in fs::read_dir(proc.join("fd")).ok()?.take(256).flatten() {
        let Ok(path) = fs::read_link(entry.path()) else {
            continue;
        };
        let Some(thread) = rollout_name(&path, &sessions) else {
            continue;
        };
        let Ok(info) = fs::read_to_string(proc.join("fdinfo").join(entry.file_name())) else {
            continue;
        };
        let flags = info
            .lines()
            .find_map(|l| l.strip_prefix("flags:\t"))
            .and_then(|s| i32::from_str_radix(s, 8).ok())?;
        if flags & nix::libc::O_APPEND == 0 || flags & nix::libc::O_ACCMODE == nix::libc::O_RDONLY {
            continue;
        }
        let opened = OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(&path)
            .ok()?;
        let m = opened.metadata().ok()?;
        let fd = fs::metadata(entry.path()).ok()?;
        if !m.is_file()
            || m.uid() != nix::unistd::geteuid().as_raw()
            || (m.dev(), m.ino()) != (fd.dev(), fd.ino())
        {
            return None;
        }
        if let Some((previous, prev_thread)) = &match_file {
            let previous = previous.metadata().ok()?;
            if *prev_thread != thread || (previous.dev(), previous.ino()) != (m.dev(), m.ino()) {
                return None;
            }
        }
        match_file = Some((opened, thread));
    }
    match_file
}

/// Newest `rollout-*.jsonl` under the latest `YYYY/MM/DD` folder.
// ponytail: only the newest day folder; right after midnight the previous day's open session is
// skipped until Codex writes the next rollout.
fn newest_rollout(sessions: &Path) -> Option<PathBuf> {
    let mut dir = sessions.to_path_buf();
    for _ in 0..3 {
        dir = fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).max()?;
    }
    fs::read_dir(&dir)
        .ok()?
        .flatten()
        .filter(|e| rollout_name(&e.path(), sessions).is_some())
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max()
        .map(|(_, p)| p)
}

/// Last rate-limit snapshot in the final `CHUNK` bytes of a rollout.
fn last_codex_plan(file: &Path) -> Option<Plan> {
    let mut f = OpenOptions::new().read(true).custom_flags(nix::libc::O_NOFOLLOW).open(file).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(CHUNK as u64))).ok()?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).ok()?;
    bytes
        .split(|&b| b == b'\n')
        .rev()
        .filter_map(|line| serde_json::from_slice::<CountEnvelope>(line).ok())
        .filter_map(|data| codex_reading(&data))
        .map(|r| r.plan)
        .find(|p| p.five_hours.is_some() || p.weekly.is_some())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Plans {
    pub claude: Plan,
    pub codex: Plan,
}

pub(super) struct Usage {
    path: PathBuf,
    saved: Plans,
    plans: Plans,
    rollouts: HashMap<String, Rollout>,
    contexts: HashMap<String, Option<f64>>,
}

pub fn cached_plans(path: &Path) -> Plans {
    store::read_bounded(path, 4096)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

impl Usage {
    pub(super) fn new(base: &Path) -> Self {
        let path = base.join("usage.json");
        let saved = cached_plans(&path);
        Self {
            path,
            saved,
            plans: saved,
            rollouts: HashMap::new(),
            contexts: HashMap::new(),
        }
    }

    pub(super) fn observe(
        &mut self,
        id: &str,
        provider: AgentId,
        dir: &Path,
        codex: Option<(u32, &(PathBuf, PathBuf))>,
    ) {
        let reading = match provider {
            AgentId::ClaudeCode => {
                let path = dir.join("usage.json");
                store::read_bounded(&path, 4096)
                    .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            }
            AgentId::Codex => codex.and_then(|(pid, (home, executable))| {
                if let Some((file, thread)) = open_rollout(pid, home, executable) {
                    self.rollouts
                        .entry(id.to_owned())
                        .or_default()
                        .read(file, thread)
                        .ok()
                } else {
                    // Missing or ambiguous ownership invalidates this session's current context.
                    Some(Reading::default())
                }
            }),
            _ => None,
        };
        let Some(reading) = reading else { return };
        self.contexts.insert(id.to_owned(), reading.context);
        match provider {
            AgentId::ClaudeCode => self.plans.claude.merge(reading.plan),
            AgentId::Codex => self.plans.codex.merge(reading.plan),
            _ => {}
        }
    }

    /// Account-wide Codex limits from the newest rollout on disk, even when Codex runs
    /// outside the Console. Context stays bound to each PTY session.
    pub(super) fn observe_codex_account(&mut self, home: &Path) {
        if let Some(plan) = newest_rollout(&home.join("sessions")).and_then(|f| last_codex_plan(&f)) {
            self.plans.codex.merge(plan);
        }
    }

    pub(super) fn context(&self, id: &str) -> Option<f64> {
        self.contexts.get(id).copied().flatten()
    }

    pub(super) fn forget(&mut self, id: &str) {
        self.contexts.remove(id);
        self.rollouts.remove(id);
    }

    pub(super) fn finished(&mut self, id: &str) {
        self.rollouts.remove(id);
    }

    pub(super) fn retain(&mut self, known: impl Fn(&str) -> bool) {
        self.contexts.retain(|id, _| known(id));
    }

    pub(super) fn plans(&mut self) -> Plans {
        if self.plans != self.saved
            && serde_json::to_vec(&self.plans)
                .is_ok_and(|bytes| store::write_atomic(&self.path, &bytes).is_ok())
        {
            self.saved = self.plans;
        }
        self.plans
    }
}

#[cfg(test)]
mod tests;
