//! The part of the fixture that owns processes. Only built with `m3-fixture`; no packaged build
//! contains it, and the package check refuses its marker.
use super::{FixtureModel, Reaction};
use crate::{
    adapters::claude::Profile,
    f8::{
        claude::{
            launch::{ClaudeInstall, ClaudeLauncher},
            worker::{ClaudeWorker, Identity},
        },
        engine::{StopState, Worker},
        process_worker::DeferredProcessWorker,
    },
    m3::{
        gui::{FixtureSource, FixtureView},
        report::ProviderReport,
    },
};
use serde::Serialize;
use serde_json::json;
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// Present in a binary only when this module is. The packaging check looks for it.
pub const MARKER: &str = "WYSS_M3_FIXTURE_HOST";
const FIXTURE_SESSION: &str = "0d0d0d0d-1111-4222-8333-444444444444";
const MAX_PROMPT: usize = 8 * 1024;

pub struct FixtureOptions {
    pub sdk_root: PathBuf,
    pub state_dir: PathBuf,
    pub workspace: PathBuf,
    /// The mock provider's script (a JSON array of responses).
    pub script: PathBuf,
}

struct Live {
    worker: ClaudeWorker<DeferredProcessWorker>,
    /// The attachment's workspace, which the binding must name.
    workspace: String,
    stopping: bool,
    /// The phase to show once the stop completes (a failed runtime stays failed).
    keep_phase: bool,
}
struct Inner {
    model: FixtureModel,
    live: Option<Live>,
    generation: u64,
    last_report: Option<ProviderReport>,
}
struct Shared {
    inner: Mutex<Inner>,
    launcher: ClaudeLauncher,
    workspace: String,
    mock: Mutex<Option<Child>>,
    done: AtomicBool,
}

#[derive(Clone)]
pub struct FixtureHost {
    shared: Arc<Shared>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Ack {
    pub ok: bool,
    pub error: Option<&'static str>,
}
impl Ack {
    fn ok() -> Self {
        Self {
            ok: true,
            error: None,
        }
    }
    fn no(error: &'static str) -> Self {
        Self {
            ok: false,
            error: Some(error),
        }
    }
}

impl FixtureHost {
    pub fn start(opts: FixtureOptions) -> Result<Self, String> {
        let mock_log = opts.state_dir.join("mock-requests.jsonl");
        let probe = Path::new(env!("CARGO_MANIFEST_DIR")).join("claude-bridge/probe/serve_mock.py");
        let mut mock = Command::new("python3")
            .arg("-I")
            .arg(probe)
            .arg(&opts.script)
            .arg(&mock_log)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("mock provider: {e}"))?;
        let mut line = String::new();
        BufReader::new(mock.stdout.as_mut().ok_or("mock stdout")?)
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        let port: u16 = line
            .trim()
            .strip_prefix("PORT ")
            .and_then(|p| p.parse().ok())
            .ok_or("the mock provider did not report a port")?;
        let mut install = ClaudeInstall::new(opts.sdk_root);
        install.upstream = Some(format!("http://127.0.0.1:{port}"));
        // An administrator's policy would be refused; a development machine has none.
        let runtime = opts.state_dir.join("runtime");
        fs::create_dir_all(&runtime).map_err(|e| e.to_string())?;
        let launcher = ClaudeLauncher::prepare(&install, &runtime, &opts.state_dir)
            .map_err(|e| format!("installation refused: {}", e.code()))?;
        let host = Self {
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner {
                    model: FixtureModel::new(),
                    live: None,
                    generation: 0,
                    last_report: None,
                }),
                launcher,
                workspace: opts.workspace.to_string_lossy().into_owned(),
                mock: Mutex::new(Some(mock)),
                done: AtomicBool::new(false),
            }),
        };
        let shared = host.shared.clone();
        std::thread::spawn(move || {
            while !shared.done.load(Ordering::Acquire) {
                Self::tick(&shared);
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        Ok(host)
    }

    fn lock(shared: &Shared) -> std::sync::MutexGuard<'_, Inner> {
        shared.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn fail(inner: &mut Inner) {
        let Some(live) = inner.live.as_mut() else {
            return;
        };
        let report = live.worker.provider_report();
        match report.as_ref().and_then(|r| r.refusal) {
            Some(code) => inner.model.refused(code),
            None => {
                let why = report
                    .as_ref()
                    .and_then(|r| r.failure.clone())
                    .unwrap_or_else(|| "the runtime stopped responding".into());
                inner.model.failed(&why)
            }
        }
        inner.last_report = report;
        live.worker.begin_stop(Instant::now());
        live.stopping = true;
        live.keep_phase = true;
    }

    fn tick(shared: &Shared) {
        let mut g = Self::lock(shared);
        let now = Instant::now();
        let Some(live) = g.live.as_mut() else { return };
        if live.stopping {
            if live.worker.poll_stop(now) == StopState::Reaped {
                let keep = live.keep_phase;
                let report = live.worker.provider_report();
                g.live = None;
                if !keep {
                    g.model.stopped();
                    g.last_report = report;
                }
            }
            return;
        }
        if live.worker.flush().is_err() {
            Self::fail(&mut g);
            return;
        }
        let frames = match live.worker.inbound(16) {
            Ok(f) => f,
            Err(_) => {
                Self::fail(&mut g);
                return;
            }
        };
        let mut bind = false;
        for f in frames {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&f) {
                bind |= g.model.apply(&v) == Reaction::Bind;
            }
        }
        if bind {
            let workspace = g
                .live
                .as_ref()
                .map(|l| l.workspace.clone())
                .unwrap_or_default();
            let frame = json!({"method":"thread/start","operation_id":crate::control::store::uuid().as_str(),"workspace":workspace});
            let sent = g.live.as_mut().map(|l| {
                l.worker
                    .send(&serde_json::to_vec(&frame).unwrap_or_default())
            });
            if sent.is_some_and(|r| r.is_err()) {
                Self::fail(&mut g);
                return;
            }
        }
        if g.live.as_mut().is_some_and(|l| l.worker.has_exited()) {
            Self::fail(&mut g);
        }
    }

    pub fn start_runtime(&self, profile: &str) -> Ack {
        let profile = match Profile::from_attachment(profile) {
            Some(p) => p,
            None => return Ack::no("unknown_profile"),
        };
        let mut g = Self::lock(&self.shared);
        if g.live.is_some() {
            return Ack::no("already_running");
        }
        g.generation += 1;
        let identity = Identity {
            managed_session: FIXTURE_SESSION.into(),
            runtime_instance: crate::control::store::uuid().as_str().to_owned(),
            generation: g.generation,
        };
        let conversation = crate::control::store::uuid().as_str().to_owned();
        let workspace = (profile == Profile::LocalTools).then_some(self.shared.workspace.as_str());
        match self
            .shared
            .launcher
            .start(profile, workspace, &identity, &conversation, None, None)
        {
            Ok(worker) => {
                g.model.starting(profile.attachment_name());
                g.last_report = None;
                let bound = workspace
                    .map(str::to_owned)
                    .unwrap_or_else(|| crate::f8::claude::launch::SANDBOX_CWD.to_owned());
                g.live = Some(Live {
                    worker,
                    workspace: bound,
                    stopping: false,
                    keep_phase: false,
                });
                Ack::ok()
            }
            Err(_) => {
                g.model.failed("the runtime could not be started");
                Ack::no("start_refused")
            }
        }
    }

    pub fn stop_runtime(&self) -> Ack {
        let mut g = Self::lock(&self.shared);
        match g.live.as_mut() {
            Some(l) if !l.stopping => {
                l.worker.begin_stop(Instant::now());
                l.stopping = true;
                Ack::ok()
            }
            _ => Ack::no("not_running"),
        }
    }

    pub fn prompt(&self, text: &str) -> Ack {
        if text.is_empty() || text.len() > MAX_PROMPT {
            return Ack::no("invalid_prompt");
        }
        let mut g = Self::lock(&self.shared);
        if g.model.phase() != "ready" {
            return Ack::no("not_ready");
        }
        let turn = crate::control::store::uuid().as_str().to_owned();
        let frame = serde_json::to_vec(&json!({"method":"turn/start","turn":turn,"input":text}))
            .unwrap_or_default();
        match g.live.as_mut().map(|l| l.worker.send(&frame)) {
            Some(Ok(())) => {
                g.model.prompt(&turn, text);
                Ack::ok()
            }
            Some(Err(_)) => Ack::no("turn_refused"),
            None => Ack::no("not_running"),
        }
    }

    pub fn interrupt(&self) -> Ack {
        let mut g = Self::lock(&self.shared);
        match g
            .live
            .as_mut()
            .map(|l| l.worker.send(br#"{"method":"turn/interrupt"}"#))
        {
            Some(Ok(())) => Ack::ok(),
            Some(Err(_)) => Ack::no("no_active_turn"),
            None => Ack::no("not_running"),
        }
    }

    pub fn decide(&self, item: &str, decision: &str) -> Ack {
        if !matches!(decision, "accept" | "decline" | "cancel") {
            return Ack::no("invalid_decision");
        }
        let mut g = Self::lock(&self.shared);
        let report = g.live.as_ref().and_then(|l| l.worker.provider_report());
        let view = g.model.view(report);
        if !view.approvals.iter().any(|a| {
            a.item == item
                && a.decisions.contains(&match decision {
                    "accept" => "accept",
                    "decline" => "decline",
                    _ => "cancel",
                })
        }) {
            return Ack::no("not_pending");
        }
        let frame = format!("approval {item} {decision}");
        match g
            .live
            .as_mut()
            .map(|l| l.worker.send_critical(frame.as_bytes()))
        {
            Some(Ok(())) => Ack::ok(),
            Some(Err(_)) => Ack::no("decision_refused"),
            None => Ack::no("not_running"),
        }
    }

    /// Ends the runtime and the mock provider; the application calls it on exit.
    pub fn shutdown(&self) {
        let mut g = Self::lock(&self.shared);
        if let Some(l) = g.live.as_mut() {
            l.worker.begin_stop(Instant::now());
            l.stopping = true;
        }
        drop(g);
        let end = Instant::now() + Duration::from_secs(8);
        while Instant::now() < end && Self::lock(&self.shared).live.is_some() {
            std::thread::sleep(Duration::from_millis(25));
        }
        self.shared.done.store(true, Ordering::Release);
        if let Some(mut m) = self
            .shared
            .mock
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            drop(m.stdin.take());
            let _ = m.wait();
        }
    }
}

impl FixtureSource for FixtureHost {
    fn view(&self) -> FixtureView {
        let g = Self::lock(&self.shared);
        let report = g
            .live
            .as_ref()
            .and_then(|l| l.worker.provider_report())
            .or_else(|| g.last_report.clone());
        g.model.view(report)
    }
}

// ---- Tauri commands ----------------------------------------------------------------------------

#[tauri::command]
pub fn m3_fixture_start(host: tauri::State<'_, FixtureHost>, profile: String) -> Ack {
    host.start_runtime(&profile)
}
#[tauri::command]
pub fn m3_fixture_stop(host: tauri::State<'_, FixtureHost>) -> Ack {
    host.stop_runtime()
}
#[tauri::command]
pub fn m3_fixture_prompt(host: tauri::State<'_, FixtureHost>, text: String) -> Ack {
    host.prompt(&text)
}
#[tauri::command]
pub fn m3_fixture_interrupt(host: tauri::State<'_, FixtureHost>) -> Ack {
    host.interrupt()
}
#[tauri::command]
pub fn m3_fixture_decide(
    host: tauri::State<'_, FixtureHost>,
    item: String,
    decision: String,
) -> Ack {
    host.decide(&item, &decision)
}

/// The commands as a desktop extension.
/// The grant of the five fixture commands to the main window, made when this binary starts: the
/// package's own capabilities (`capabilities/*.json`) name none of them, so no other build has it.
fn grant() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    use tauri::{Manager, ipc::CapabilityBuilder};
    tauri::plugin::Builder::new("m3-fixture-grant")
        .setup(|app, _| {
            let mut capability = CapabilityBuilder::new("m3-fixture").window("main");
            for permission in [
                "allow-m3-fixture-start",
                "allow-m3-fixture-stop",
                "allow-m3-fixture-prompt",
                "allow-m3-fixture-interrupt",
                "allow-m3-fixture-decide",
            ] {
                capability = capability.permission(permission);
            }
            app.add_capability(capability)?;
            Ok(())
        })
        .build()
}

pub fn extension(host: FixtureHost) -> crate::Extension {
    let on_exit = host.clone();
    crate::Extension {
        commands: &[
            "m3_fixture_start",
            "m3_fixture_stop",
            "m3_fixture_prompt",
            "m3_fixture_interrupt",
            "m3_fixture_decide",
        ],
        handler: Box::new(tauri::generate_handler![
            m3_fixture_start,
            m3_fixture_stop,
            m3_fixture_prompt,
            m3_fixture_interrupt,
            m3_fixture_decide
        ]),
        install: Box::new(move |b| b.manage(host).plugin(grant())),
        on_exit: Some(Box::new(move |_| on_exit.shutdown())),
    }
}
