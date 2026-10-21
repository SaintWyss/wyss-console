//! A local application for looking at the window: a real prepared protocol-2 application on
//! its own thread, its durable image in a directory the caller names, and the fixture child
//! of `fixture` for a runtime. **Development only**: it is compiled with the `f8-dev` feature,
//! which no build of the installed desktop enables. It touches nothing outside the directory
//! it is given.
use super::{
    fixture::FixtureSpawner,
    gui::{F8Gui, HandleConnector},
    migration,
};
use crate::{
    contracts::{WireUuid, control::ControlResult},
    control::application::{ApplicationHandle, LocalSelection, PreparedApplication, run_selected},
};
use rusqlite::Connection;
use std::{
    path::{Path, PathBuf},
    thread::JoinHandle,
};

/// A running local application and the window's service for it.
pub struct DevApplication {
    pub gui: F8Gui,
    handle: ApplicationHandle,
    run: Option<JoinHandle<ControlResult<()>>>,
    pub workspace: PathBuf,
}
impl DevApplication {
    /// Opens (creating when absent) the durable image in `dir`, a private directory.
    pub fn start(dir: &Path) -> ControlResult<Self> {
        let image = dir.join("authority.sqlite3");
        if !image.exists() {
            migration::fresh()?.write_new_image(&image)?;
        }
        let workspace = dir.join("workspace");
        std::fs::create_dir_all(&workspace)
            .map_err(|_| crate::contracts::control::ControlError::UnsafePath)?;
        let connection = Connection::open(&image)
            .map_err(|_| crate::contracts::control::ControlError::StorageUnavailable)?;
        let authority = WireUuid::try_from(uuid::Uuid::new_v4().to_string())
            .map_err(|_| crate::contracts::control::ControlError::InternalError)?;
        let (application, handle) = PreparedApplication::new(
            connection,
            authority,
            None,
            FixtureSpawner::new(workspace.clone()),
        )?;
        let run = std::thread::spawn(move || {
            run_selected(LocalSelection::PreparedProtocol2(Box::new(application)))
        });
        Ok(Self {
            gui: F8Gui::local(HandleConnector(handle.clone())),
            handle,
            run: Some(run),
            workspace,
        })
    }
    /// Ends the application the way its owner does.
    pub fn stop(&mut self) -> ControlResult<()> {
        self.gui.shutdown();
        self.handle.shutdown();
        self.run
            .take()
            .map_or(Ok(()), |run| run.join().unwrap_or(Ok(())))
    }
}
impl Drop for DevApplication {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
