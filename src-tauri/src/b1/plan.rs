//! The launch plan of a B1 worker: a rootless `bwrap` command that shows the worker a mount
//! namespace of its own, no network, no other process, no capability, and runs the trusted shim
//! first (`confine`), which closes descriptors, applies seccomp and Landlock and only then executes
//! the worker. The plan validates what it is given and refuses, with a closed word, anything that
//! would let a path decide what the worker can see.
//!
//! What the worker sees: the read-only system runtime, its workspace at `/work`, an optional home
//! at `/home/b1` (synthetic credentials live there), an optional broker directory at `/broker`
//! with the one socket of a mediated service, a private `/tmp`, `/proc` of its own process
//! namespace and a minimal `/dev`. Nothing else: no host home, no runtime directory, no D-Bus, no
//! display, no keyring, no container socket, no authority socket.

use super::confine::layer;
use crate::screen::locate::{self, Verified};
use std::{
    ffi::OsString,
    fs::File,
    os::{fd::AsRawFd, unix::fs::MetadataExt, unix::fs::PermissionsExt, unix::process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub const BWRAP: &str = "/usr/bin/bwrap";
pub const WORK: &str = "/work";
pub const HOME: &str = "/home/b1";
pub const BROKER: &str = "/broker";
pub const SHIM: &str = "/b1/shim";
pub const BIN: &str = "/b1/bin";
/// Most memory a private `/tmp` may take.
pub const TMP_BYTES: u64 = 64 * 1024 * 1024;

const SYSTEM_RO: [&str; 7] = [
    "/usr",
    "/etc/ld.so.cache",
    "/etc/ld.so.conf",
    "/etc/ld.so.conf.d",
    "/etc/ssl",
    "/etc/ca-certificates",
    "/etc/localtime",
];

/// Why a plan was refused. Closed words; none carries a path.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum PlanError {
    PathNotPlain,
    NotAPrivateDirectory,
    OverlappingDirectories,
    TooBroad,
    HelperNotTrusted,
    NameNotPlain,
    /// The workspace is not the object the plan was made for.
    WorkspaceChanged,
    Io,
}

impl PlanError {
    pub fn word(self) -> &'static str {
        match self {
            Self::PathNotPlain => "path_not_plain",
            Self::NotAPrivateDirectory => "not_a_private_directory",
            Self::OverlappingDirectories => "overlapping_directories",
            Self::TooBroad => "too_broad",
            Self::HelperNotTrusted => "helper_not_trusted",
            Self::NameNotPlain => "name_not_plain",
            Self::WorkspaceChanged => "workspace_changed",
            Self::Io => "io",
        }
    }
}

#[derive(Debug)]
pub struct Plan {
    /// The task's tree on the host: fresh, owned by this user, mode 0700.
    pub workspace: PathBuf,
    /// A directory of its own for the worker's home (synthetic credentials), same rules.
    pub home: Option<PathBuf>,
    /// A directory that holds exactly the socket of a mediated service.
    pub broker_dir: Option<PathBuf>,
    /// Layers that must hold or the worker does not run.
    pub require: u8,
    /// A Landlock ABI floor above `confine::MIN_LANDLOCK_ABI`; it can never be lower.
    pub min_abi: u8,
    /// The object the workspace must be (device, inode), when the plan was made for a materialized
    /// workspace: a directory swapped in under the same path is refused.
    pub workspace_identity: Option<(u64, u64)>,
}

/// A command ready to spawn, and the descriptors it needs to stay open until it has been.
pub struct Launch {
    pub command: Command,
    _held: Vec<File>,
}

fn me() -> u32 {
    nix::unistd::getuid().as_raw()
}

fn plain(p: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let b = p.as_os_str().as_bytes();
    p.is_absolute()
        && !b.is_empty()
        && !b.contains(&0)
        && !b.contains(&b'\n')
        && !b.contains(&b',')
        && p.components()
            .all(|c| !matches!(c, std::path::Component::ParentDir))
}

/// A directory held open: the object that was checked is the object that is used.
pub(crate) struct Pinned {
    pub fd: std::os::fd::OwnedFd,
    /// The real path of that very object, read from the kernel through the descriptor.
    pub real: PathBuf,
    pub dev: u64,
    pub ino: u64,
}

/// A directory that is the worker's to have: opened without following a link, a real directory,
/// ours, private, and not a place that holds the user's own state or the machine's. The checks are
/// made on the open descriptor (and on the path the kernel reports for it), so a path component
/// swapped after this returns changes nothing: the descriptor is what is handed to `bwrap`.
pub(crate) fn pin_directory(p: &Path) -> Result<Pinned, PlanError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    if !plain(p) {
        return Err(PlanError::PathNotPlain);
    }
    let c = std::ffi::CString::new(p.as_os_str().as_encoded_bytes())
        .map_err(|_| PlanError::PathNotPlain)?;
    // SAFETY: a NUL-terminated path and integer flags.
    let raw = unsafe {
        nix::libc::open(
            c.as_ptr(),
            nix::libc::O_RDONLY
                | nix::libc::O_DIRECTORY
                | nix::libc::O_NOFOLLOW
                | nix::libc::O_CLOEXEC,
        )
    };
    if raw < 0 {
        return Err(PlanError::NotAPrivateDirectory);
    }
    // SAFETY: a descriptor just opened and owned by nobody else.
    let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(raw) };
    let meta = File::from(fd.try_clone().map_err(|_| PlanError::Io)?)
        .metadata()
        .map_err(|_| PlanError::Io)?;
    if !meta.is_dir() || meta.uid() != me() || meta.permissions().mode() & 0o077 != 0 {
        return Err(PlanError::NotAPrivateDirectory);
    }
    let real = std::fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd()))
        .map_err(|_| PlanError::Io)?;
    if real != p {
        // A path that goes through a link names a different place than it reads as.
        return Err(PlanError::PathNotPlain);
    }
    // Never a place that holds the user's own state or the machine's.
    let mut forbidden: Vec<PathBuf> = vec![PathBuf::from("/")];
    for var in [
        "HOME",
        "XDG_RUNTIME_DIR",
        "XDG_STATE_HOME",
        "XDG_CONFIG_HOME",
    ] {
        if let Some(v) = std::env::var_os(var) {
            forbidden.push(PathBuf::from(v));
        }
    }
    for dir in ["/proc", "/sys", "/dev", "/run", "/etc", "/usr", "/var"] {
        forbidden.push(PathBuf::from(dir));
    }
    for f in &forbidden {
        // Equal to, or an ancestor of, a place that must not be shown.
        if f.starts_with(&real) && !f.as_os_str().is_empty() {
            return Err(PlanError::TooBroad);
        }
    }
    for dir in ["/proc", "/sys", "/dev", "/etc", "/usr"] {
        if real.starts_with(dir) {
            return Err(PlanError::TooBroad);
        }
    }
    Ok(Pinned {
        fd,
        real,
        dev: meta.dev(),
        ino: meta.ino(),
    })
}

fn nested(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

impl Plan {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            workspace,
            home: None,
            broker_dir: None,
            require: layer::ALL,
            min_abi: super::confine::MIN_LANDLOCK_ABI,
            workspace_identity: None,
        }
    }

    /// A plan for the tree `materialize` built, bound to that very directory.
    pub fn for_workspace(ws: &super::workspace::Workspace) -> Self {
        let mut plan = Self::new(ws.path().to_path_buf());
        plan.workspace_identity = Some(ws.identity());
        plan
    }

    /// The command line. `shim` and every file in `programs` are verified the way the parser helper
    /// is (`screen::locate`) and their *bytes are copied from the verified descriptor* into the
    /// worker's namespace, so what runs is what was checked. `programs` are named, not pathed: each
    /// is `/b1/bin/<name>`.
    pub fn launch(
        &self,
        shim: &Path,
        programs: &[(&str, &Path)],
        worker: &str,
        args: &[OsString],
        receipt_fd: Option<&File>,
    ) -> Result<Launch, PlanError> {
        let workspace = pin_directory(&self.workspace)?;
        if self
            .workspace_identity
            .is_some_and(|id| id != (workspace.dev, workspace.ino))
        {
            return Err(PlanError::WorkspaceChanged);
        }
        let home = self.home.as_deref().map(pin_directory).transpose()?;
        let broker = self.broker_dir.as_deref().map(pin_directory).transpose()?;
        let mut dirs = vec![&workspace.real];
        dirs.extend(home.iter().map(|h| &h.real));
        dirs.extend(broker.iter().map(|b| &b.real));
        for (i, a) in dirs.iter().enumerate() {
            for b in &dirs[i + 1..] {
                if nested(a, b) {
                    return Err(PlanError::OverlappingDirectories);
                }
            }
        }
        if !worker.starts_with('/') || worker.contains('\n') || worker.contains('\0') {
            return Err(PlanError::PathNotPlain);
        }
        let me = me();
        let shim: Verified = locate::verify(shim, me).map_err(|_| PlanError::HelperNotTrusted)?;
        let mut files: Vec<(String, Verified)> = Vec::new();
        for (name, path) in programs {
            let ok = !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
            if !ok {
                return Err(PlanError::NameNotPlain);
            }
            files.push((
                (*name).to_owned(),
                locate::verify(path, me).map_err(|_| PlanError::HelperNotTrusted)?,
            ));
        }

        let mut held: Vec<File> = Vec::new();
        let mut a: Vec<OsString> = Vec::new();
        let mut push = |items: &[&str]| a.extend(items.iter().map(OsString::from));
        push(&[
            // Named one by one: `--unshare-all` only *tries* the user namespace, and
            // `--disable-userns` needs it to be certain.
            "--unshare-user",
            "--unshare-ipc",
            "--unshare-pid",
            "--unshare-net",
            "--unshare-uts",
            "--unshare-cgroup-try",
            "--disable-userns",
            "--die-with-parent",
            "--new-session",
            "--clearenv",
            "--cap-drop",
            "ALL",
            "--hostname",
            "wyss-b1",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
        ]);
        let tmp_size = TMP_BYTES.to_string();
        push(&["--size", &tmp_size, "--tmpfs", "/tmp"]);
        for lib in ["bin", "sbin", "lib", "lib64"] {
            let p = PathBuf::from("/").join(lib);
            if let Ok(target) = std::fs::read_link(&p) {
                a.push("--symlink".into());
                a.push(target.into_os_string());
                a.push(p.into_os_string());
            }
        }
        for p in SYSTEM_RO {
            if Path::new(p).exists() {
                a.extend(["--ro-bind".into(), p.into(), p.into()]);
            }
        }
        // No identity database is bound: the worker does not learn the host's account names.
        // Programs: copied from the verified descriptors.
        let mut data: Vec<(String, File)> = Vec::new();
        data.push((SHIM.to_owned(), shim.reopen().map_err(|_| PlanError::Io)?));
        for (name, v) in &files {
            data.push((
                format!("{BIN}/{name}"),
                v.reopen().map_err(|_| PlanError::Io)?,
            ));
        }
        for (dest, file) in data {
            a.extend([
                "--perms".into(),
                "0755".into(),
                "--ro-bind-data".into(),
                file.as_raw_fd().to_string().into(),
                dest.into(),
            ]);
            held.push(file);
        }
        // The directories are bound from the descriptors that were checked, not from their paths.
        for (pinned, dest) in [
            (Some(&workspace), WORK),
            (home.as_ref(), HOME),
            (broker.as_ref(), BROKER),
        ] {
            if let Some(p) = pinned {
                let file = File::from(p.fd.try_clone().map_err(|_| PlanError::Io)?);
                a.extend([
                    "--bind-fd".into(),
                    file.as_raw_fd().to_string().into(),
                    dest.into(),
                ]);
                held.push(file);
            }
        }
        let home_value = if home.is_some() { HOME } else { "/tmp" };
        for (k, v) in [
            ("HOME", home_value),
            ("PATH", "/usr/bin:/bin"),
            ("TMPDIR", "/tmp"),
            ("LANG", "C.UTF-8"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_TERMINAL_PROMPT", "0"),
        ] {
            a.extend(["--setenv".into(), k.into(), v.into()]);
        }
        a.extend(["--chdir".into(), WORK.into(), "--".into(), SHIM.into()]);

        // The shim's own arguments: what it may touch, and what must hold.
        let shim_args: Vec<OsString> = {
            let mut s: Vec<OsString> = Vec::new();
            for p in ["/usr", "/b1"] {
                s.extend(["--read-exec".into(), p.into()]);
            }
            for p in ["/proc", "/etc"] {
                s.extend(["--read-only".into(), p.into()]);
            }
            s.extend(["--workspace".into(), WORK.into()]);
            s.extend(["--scratch".into(), "/tmp".into()]);
            if home.is_some() {
                s.extend(["--scratch".into(), HOME.into()]);
            }
            s.extend(["--device".into(), "/dev".into()]);
            s.extend(["--require".into(), self.require.to_string().into()]);
            s.extend(["--min-abi".into(), self.min_abi.to_string().into()]);
            if let Some(f) = receipt_fd {
                s.extend(["--receipt-fd".into(), f.as_raw_fd().to_string().into()]);
            }
            s.push("--".into());
            s.push(worker.into());
            s.extend(args.iter().cloned());
            s
        };
        a.extend(shim_args);

        let mut command = Command::new(BWRAP);
        // bwrap's own environment is what its init process keeps and what the worker could read
        // through /proc/1/environ: it starts with none.
        command
            .env_clear()
            .args(&a)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // The copies bwrap reads, and the receipt pipe, must survive the exec into bwrap.
        let mut inherit: Vec<i32> = held.iter().map(|f| f.as_raw_fd()).collect();
        if let Some(f) = receipt_fd {
            inherit.push(f.as_raw_fd());
        }
        // SAFETY: only fcntl (async-signal-safe) between fork and exec.
        unsafe {
            command.pre_exec(move || {
                for fd in &inherit {
                    let flags = nix::libc::fcntl(*fd, nix::libc::F_GETFD);
                    if flags < 0
                        || nix::libc::fcntl(*fd, nix::libc::F_SETFD, flags & !nix::libc::FD_CLOEXEC)
                            < 0
                    {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        Ok(Launch {
            command,
            _held: held,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn scratch(name: &str) -> PathBuf {
        let base = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join("b1-plan-tests");
        let _ = std::fs::create_dir_all(&base);
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700)).unwrap();
        let dir = base.join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::canonicalize(&dir).unwrap()
    }

    fn tool(dir: &Path, name: &str, mode: u32) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, b"#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        p
    }

    fn args(c: &Command) -> Vec<String> {
        c.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn a_plan_shows_the_worker_nothing_but_what_it_names() {
        let d = scratch("shape");
        let ws = d.join("ws");
        std::fs::create_dir(&ws).unwrap();
        std::fs::set_permissions(&ws, std::fs::Permissions::from_mode(0o700)).unwrap();
        let shim = tool(&d, "shim", 0o755);
        let plan = Plan::new(ws.clone());
        let launch = plan.launch(&shim, &[], "/usr/bin/true", &[], None).unwrap();
        let a = args(&launch.command);
        for flag in [
            // Named one by one: `--unshare-all` only *tries* the user namespace, and
            // `--disable-userns` needs it to be certain.
            "--unshare-user",
            "--unshare-ipc",
            "--unshare-pid",
            "--unshare-net",
            "--unshare-uts",
            "--unshare-cgroup-try",
            "--disable-userns",
            "--die-with-parent",
            "--new-session",
            "--clearenv",
        ] {
            assert!(a.iter().any(|x| x == flag), "{flag}");
        }
        // The network is never shared back.
        assert!(!a.iter().any(|x| x == "--share-net"));
        // Every bind of the host's tree is the system runtime (read-only) or the task's own.
        let mut i = 0;
        while i < a.len() {
            if a[i] == "--bind" {
                assert_eq!(
                    a[i + 1],
                    ws.to_string_lossy(),
                    "only the workspace is bound writable"
                );
            }
            if a[i] == "--ro-bind" {
                assert!(SYSTEM_RO.contains(&a[i + 1].as_str()), "{}", a[i + 1]);
            }
            i += 1;
        }
        // No home, runtime directory, bus, display or container socket is named anywhere.
        let joined = a.join(" ");
        for banned in [
            "/run/user",
            "/var/run",
            "docker",
            "dbus",
            ".ssh",
            ".claude",
            ".codex",
            ".config",
        ] {
            assert!(!joined.contains(banned), "{banned}");
        }
        assert!(
            launch.command.get_envs().next().is_none(),
            "bwrap starts with no environment"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_workspace_that_is_the_users_own_state_or_a_link_or_open_to_others_is_refused() {
        let d = scratch("refuse");
        let shim = tool(&d, "shim", 0o755);
        let try_ws = |p: PathBuf| {
            Plan::new(p)
                .launch(&shim, &[], "/usr/bin/true", &[], None)
                .err()
        };
        // Not private.
        let open = d.join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(try_ws(open), Some(PlanError::NotAPrivateDirectory));
        // A link.
        let real = d.join("real");
        std::fs::create_dir(&real).unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o700)).unwrap();
        let link = d.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(try_ws(link), Some(PlanError::NotAPrivateDirectory));
        // A relative path, one with a parent component, one with a newline or a comma.
        assert_eq!(
            try_ws(PathBuf::from("relative")),
            Some(PlanError::PathNotPlain)
        );
        assert_eq!(
            try_ws(d.join("..").join("x")),
            Some(PlanError::PathNotPlain)
        );
        // The root, the system, and the user's home (an ancestor of what must not be shown).
        assert!(matches!(
            try_ws(PathBuf::from("/")),
            Some(PlanError::NotAPrivateDirectory | PlanError::TooBroad)
        ));
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            if std::fs::metadata(&home)
                .map(|m| m.permissions().mode() & 0o077 == 0)
                .unwrap_or(false)
            {
                assert_eq!(try_ws(home), Some(PlanError::TooBroad));
            }
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_directories_of_one_worker_never_contain_each_other() {
        let d = scratch("nest");
        let shim = tool(&d, "shim", 0o755);
        let outer = d.join("outer");
        std::fs::create_dir(&outer).unwrap();
        std::fs::set_permissions(&outer, std::fs::Permissions::from_mode(0o700)).unwrap();
        let inner = outer.join("inner");
        std::fs::create_dir(&inner).unwrap();
        std::fs::set_permissions(&inner, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut plan = Plan::new(outer);
        plan.home = Some(inner);
        assert_eq!(
            plan.launch(&shim, &[], "/usr/bin/true", &[], None).err(),
            Some(PlanError::OverlappingDirectories)
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_shim_or_program_that_is_not_trusted_or_not_plainly_named_is_refused() {
        let d = scratch("trust");
        let ws = d.join("ws");
        std::fs::create_dir(&ws).unwrap();
        std::fs::set_permissions(&ws, std::fs::Permissions::from_mode(0o700)).unwrap();
        let plan = Plan::new(ws);
        let weak = tool(&d, "weak", 0o775);
        assert_eq!(
            plan.launch(&weak, &[], "/usr/bin/true", &[], None).err(),
            Some(PlanError::HelperNotTrusted)
        );
        let good = tool(&d, "good", 0o755);
        assert_eq!(
            plan.launch(&good, &[("../x", &good)], "/usr/bin/true", &[], None)
                .err(),
            Some(PlanError::NameNotPlain)
        );
        assert_eq!(
            plan.launch(&good, &[("ok", &weak)], "/usr/bin/true", &[], None)
                .err(),
            Some(PlanError::HelperNotTrusted)
        );
        assert_eq!(
            plan.launch(&good, &[], "relative/worker", &[], None).err(),
            Some(PlanError::PathNotPlain)
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
