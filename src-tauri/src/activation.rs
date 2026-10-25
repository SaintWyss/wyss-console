//! Explicit GUI-only activation. The outer process creates a fresh HOME/XDG domain and owns
//! its private bus. The desktop never manages authority services, including in ordinary mode.
use std::{
    ffi::OsString,
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const MODE: &str = "WYSS_GUI_ONLY_ROOT";
fn private(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|s| {
        s.is_dir()
            && s.uid() == nix::unistd::geteuid().as_raw()
            && s.permissions().mode() & 0o077 == 0
    })
}
fn validated(root: &Path) -> bool {
    root.is_absolute()
        && root.parent() == Some(std::env::temp_dir().as_path())
        && root
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("wyss-gui-only-"))
        && fs::canonicalize(root).is_ok_and(|p| p == root)
        && private(root)
        && ["home", "run", "state", "data", "config", "cache"]
            .iter()
            .all(|d| private(&root.join(d)))
}
fn environment(root: &Path, c: &mut Command) {
    for (key, dir) in [
        ("HOME", "home"),
        ("XDG_RUNTIME_DIR", "run"),
        ("XDG_STATE_HOME", "state"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_CACHE_HOME", "cache"),
    ] {
        c.env(key, root.join(dir));
    }
    c.env(
        "DBUS_SESSION_BUS_ADDRESS",
        format!("unix:path={}", root.join("bus").display()),
    );
}
pub fn select(args: &[OsString]) -> Result<bool, &'static str> {
    if let Some(root) = std::env::var_os(MODE) {
        let root = PathBuf::from(root);
        if !(args.is_empty()
            || (args.len() == 1
                && matches!(args[0].to_str(), Some("--wyss-host" | "--wyss-host-stop"))))
            || !validated(&root)
        {
            return Err("invalid GUI-only activation");
        }
        for (key, dir) in [
            ("HOME", "home"),
            ("XDG_RUNTIME_DIR", "run"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
        ] {
            if std::env::var_os(key) != Some(root.join(dir).into_os_string()) {
                return Err("GUI-only isolation mismatch");
            }
        }
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS")
            != Some(format!("unix:path={}", root.join("bus").display()).into())
        {
            return Err("GUI-only bus mismatch");
        }
        return Ok(false);
    }
    if args.first().is_none_or(|s| s != "--gui-only") {
        return Ok(false);
    }
    if args.len() != 1 {
        return Err("--gui-only takes no arguments");
    }
    let root = std::env::temp_dir().join(format!("wyss-gui-only-{}", uuid::Uuid::new_v4()));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .map_err(|_| "GUI-only directory unavailable")?;
    let _cleanup = RootCleanup(root.clone());
    for name in ["home", "run", "state", "data", "config", "cache"] {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(root.join(name))
            .map_err(|_| "GUI-only directory unavailable")?;
    }
    let mut bus = Owned(
        Command::new("/usr/bin/dbus-daemon")
            .args(["--session", "--nofork"])
            .arg(format!(
                "--address=unix:path={}",
                root.join("bus").display()
            ))
            .env_remove(MODE)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "private bus unavailable")?,
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !root.join("bus").exists() {
        if Instant::now() > deadline
            || bus
                .0
                .try_wait()
                .map_err(|_| "private bus unavailable")?
                .is_some()
        {
            let _ = bus.0.kill();
            let _ = bus.0.wait();
            return Err("private bus unavailable");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let exe = std::env::current_exe().map_err(|_| "GUI executable unavailable")?;
    let mut gui = Command::new(&exe);
    environment(&root, &mut gui);
    gui.env(MODE, &root);
    let result = gui.status();
    // Only this invocation's host domain is contacted, never a process found by name/PID.
    let mut stop = Command::new(&exe);
    environment(&root, &mut stop);
    stop.env_remove(MODE)
        .arg("--wyss-host-stop")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Ok(child) = stop.spawn() {
        let mut stop = Owned(child);
        let end = Instant::now() + Duration::from_secs(10);
        while stop.0.try_wait().ok().flatten().is_none() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let _ = bus.0.kill();
    let _ = bus.0.wait();
    let _ = fs::remove_dir_all(&root);
    match result {
        Ok(s) if s.success() => Ok(true),
        _ => Err("GUI-only process ended unsuccessfully"),
    }
}

struct RootCleanup(PathBuf);
impl Drop for RootCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Owned(std::process::Child);
impl Drop for Owned {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
