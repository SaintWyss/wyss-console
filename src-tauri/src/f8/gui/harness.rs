//! A real prepared application on its own thread, for tests that go through the window's
//! service: AF_UNIX socketpair transport, SQLite V8 in memory, an owned fixture process.
//! The fixture speaks the explicitly normalized test protocol; it is not Codex acceptance.
use super::{F8Gui, client::HandleConnector};
use crate::f8::{
    launcher::NativeLocalToolsSpawner,
    provider_adapter::native_process_tests::{Root, local_fixture_plan},
};
use crate::{
    contracts::{WireUuid, control::ControlResult},
    control::{
        F8Transport,
        application::{ApplicationHandle, LocalSelection, PreparedApplication, run_selected},
        f8_fixture,
    },
    f8::{
        authority::Authority,
        engine::Engine,
        migration,
        process_integration_tests::{AUTH, FixtureSpawner, id},
        registry::Registry,
        service::Service,
    },
};
use std::{
    path::PathBuf,
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub(crate) struct Harness {
    pub gui: F8Gui,
    pub handle: ApplicationHandle,
    pub workspace: PathBuf,
    run: Option<JoinHandle<ControlResult<()>>>,
}
impl Harness {
    /// `tools` selects the local-tools fixture; `limit` is the registry's test capacity.
    pub fn start(tools: bool, limit: usize) -> Self {
        let workspace = std::env::temp_dir().join(format!("wyss-f8-gui-{}", id()));
        std::fs::create_dir(&workspace).unwrap();
        let mut source = f8_fixture::v7_with_completed_turn();
        let connection = migration::prepare(&mut source).unwrap().into_connection();
        let authority = WireUuid::try_from(AUTH.to_owned()).unwrap();
        let engine = Engine::new(
            connection,
            authority.clone(),
            Registry::new(authority).unwrap().with_test_limit(limit),
            None,
        )
        .unwrap();
        let (application, handle) =
            PreparedApplication::with_test_transport(F8Transport::new(Service::new(
                Authority::new(engine),
                FixtureSpawner {
                    workspace: workspace.clone(),
                    tools,
                },
            )));
        let run = std::thread::spawn(move || {
            run_selected(LocalSelection::PreparedProtocol2(Box::new(application)))
        });
        Self {
            gui: F8Gui::local(HandleConnector(handle.clone())),
            handle,
            workspace,
            run: Some(run),
        }
    }
    /// Ends the application the way its owner does and answers how it ended.
    pub fn stop(&mut self) -> ControlResult<()> {
        self.gui.shutdown();
        self.handle.shutdown();
        self.run.take().map_or(Ok(()), |run| run.join().unwrap())
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.stop();
        let _ = std::fs::remove_dir_all(&self.workspace);
    }
}

/// Polls `read` until it answers `Some`, failing the test after five seconds.
#[track_caller]
pub(crate) fn eventually<T>(mut read: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(value) = read() {
            return value;
        }
        assert!(Instant::now() < until, "gui fixture deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The same, with the native Codex adapter in front of the schema-derived fixture runtime:
/// a bubblewrap-isolated local-tools launch that lists models and mediates approvals. One
/// runtime generation per harness (the launch plan is consumed by its start).
pub(crate) struct NativeHarness {
    pub gui: F8Gui,
    pub handle: ApplicationHandle,
    pub workspace: String,
    run: Option<JoinHandle<ControlResult<()>>>,
    _root: Root,
}
impl NativeHarness {
    pub fn start(scenario: &str) -> Self {
        let (root, attachment, plan) = local_fixture_plan(scenario);
        let mut source = f8_fixture::v7_with_completed_turn();
        let seed = migration::prepare(&mut source).unwrap().into_connection();
        let path = root.0.join("authority.sqlite3");
        seed.execute("VACUUM INTO ?1", [path.to_str().unwrap()])
            .unwrap();
        drop(seed);
        let connection = rusqlite::Connection::open(&path).unwrap();
        let authority = WireUuid::try_from(AUTH.to_owned()).unwrap();
        let (application, handle) = PreparedApplication::new(
            connection,
            authority,
            None,
            NativeLocalToolsSpawner::new(plan),
        )
        .unwrap();
        let run = std::thread::spawn(move || {
            run_selected(LocalSelection::PreparedProtocol2(Box::new(application)))
        });
        Self {
            gui: F8Gui::local(HandleConnector(handle.clone())),
            handle,
            workspace: attachment.workspace,
            run: Some(run),
            _root: root,
        }
    }
    pub fn stop(&mut self) -> ControlResult<()> {
        self.gui.shutdown();
        self.handle.shutdown();
        self.run.take().map_or(Ok(()), |run| run.join().unwrap())
    }
}
impl Drop for NativeHarness {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
