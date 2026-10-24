//! The bubblewrap command line that both Claude launches use: the supervised bridge and the
//! executor of an approved command. The launcher is the root-owned `/usr/bin/bwrap`, always with
//! `--unshare-pid` and `--die-with-parent`, which is what `ownership::ContainmentPolicy::
//! DescendantsContained` accepts and observes. Nothing of the user's home, credentials, D-Bus,
//! display or session is bound: whatever a launch needs is named here, one `bind` at a time.
use std::{
    ffi::OsString,
    os::unix::ffi::OsStringExt,
    path::{Path, PathBuf},
    process::Command,
};

pub const BWRAP: &str = "/usr/bin/bwrap";
const SYSTEM_RO: [&str; 8] = [
    "/usr",
    "/etc/ld.so.cache",
    "/etc/ld.so.conf",
    "/etc/ld.so.conf.d",
    "/etc/ssl",
    "/etc/ca-certificates",
    "/etc/nsswitch.conf",
    "/etc/localtime",
];
/// Names the identity databases a process needs to resolve its own user.
const IDENTITY_RO: [&str; 2] = ["/etc/passwd", "/etc/group"];
/// DNS configuration, bound only for a launch that has a network.
const NETWORK_RO: [&str; 2] = ["/etc/resolv.conf", "/etc/hosts"];

pub struct Sandbox {
    args: Vec<OsString>,
}

impl Sandbox {
    /// `network`: whether the process gets the host's network (the bridge needs the provider's
    /// API; an approved command does not).
    pub fn new(network: bool) -> Self {
        let mut s = Self { args: Vec::new() };
        for a in [
            "--unshare-pid",
            "--unshare-ipc",
            "--unshare-uts",
            "--unshare-user-try",
            "--die-with-parent",
            "--new-session",
            "--clearenv",
        ] {
            s.args.push(a.into());
        }
        if !network {
            s.args.push("--unshare-net".into());
        }
        for a in ["--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp"] {
            s.args.push(a.into());
        }
        for lib in ["bin", "sbin", "lib", "lib64"] {
            let p = PathBuf::from("/").join(lib);
            if let Ok(target) = std::fs::read_link(&p) {
                s.args.push("--symlink".into());
                s.args.push(target.into_os_string());
                s.args.push(p.into_os_string());
            }
        }
        for p in SYSTEM_RO.iter().chain(IDENTITY_RO.iter()) {
            s.ro_bind_if_present(Path::new(p));
        }
        if network {
            for p in NETWORK_RO {
                s.ro_bind_if_present(Path::new(p));
            }
        }
        s
    }
    fn ro_bind_if_present(&mut self, p: &Path) {
        if p.exists() {
            self.ro_bind(p, p);
        }
    }
    pub fn ro_bind(&mut self, src: &Path, dst: &Path) -> &mut Self {
        self.args
            .extend(["--ro-bind".into(), src.into(), dst.into()]);
        self
    }
    pub fn bind(&mut self, src: &Path, dst: &Path) -> &mut Self {
        self.args.extend(["--bind".into(), src.into(), dst.into()]);
        self
    }
    pub fn setenv(&mut self, k: &str, v: &str) -> &mut Self {
        self.args.extend(["--setenv".into(), k.into(), v.into()]);
        self
    }
    pub fn chdir(&mut self, p: &Path) -> &mut Self {
        self.args.extend(["--chdir".into(), p.into()]);
        self
    }
    /// Arguments so far, for tests and for the launch record.
    pub fn args(&self) -> &[OsString] {
        &self.args
    }
    pub fn command(&self, program: &str, program_args: &[OsString]) -> Command {
        let mut c = Command::new(BWRAP);
        c.args(&self.args).arg("--").arg(program).args(program_args);
        c
    }
}

/// A path as bytes that `bwrap` will receive unchanged; refuses anything it could misparse.
pub fn path_is_plain(p: &Path) -> bool {
    let b = p.as_os_str().to_owned().into_vec();
    p.is_absolute() && !b.is_empty() && !b.contains(&0) && !b.contains(&b'\n')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(s: &Sandbox, flag: &str) -> bool {
        s.args().iter().any(|a| a == flag)
    }

    #[test]
    fn every_launch_is_a_contained_namespace() {
        for net in [true, false] {
            let s = Sandbox::new(net);
            for f in [
                "--unshare-pid",
                "--die-with-parent",
                "--new-session",
                "--clearenv",
            ] {
                assert!(has(&s, f), "{f}");
            }
            assert_eq!(has(&s, "--unshare-net"), !net);
            assert!(!has(&s, "--share-pid"));
        }
    }

    #[test]
    fn only_named_binds_exist_and_never_the_home() {
        let mut s = Sandbox::new(false);
        s.ro_bind(Path::new("/opt/x"), Path::new("/opt/x"));
        let joined = s
            .args()
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(!joined.contains("/home"), "{joined}");
        assert!(joined.contains("--ro-bind /opt/x /opt/x"));
        // Network files only when the launch has a network.
        assert!(!joined.contains("resolv.conf"));
        assert!(
            Sandbox::new(true)
                .args()
                .iter()
                .any(|a| a == "/etc/resolv.conf")
                || !Path::new("/etc/resolv.conf").exists()
        );
    }

    #[test]
    fn the_command_is_the_root_owned_launcher_with_a_payload_separator() {
        let s = Sandbox::new(false);
        let c = s.command("/usr/bin/true", &[]);
        assert_eq!(c.get_program(), BWRAP);
        let args: Vec<_> = c.get_args().collect();
        assert_eq!(args[args.len() - 2], "--");
        assert_eq!(args[args.len() - 1], "/usr/bin/true");
    }

    #[test]
    fn plain_paths_only() {
        assert!(path_is_plain(Path::new("/a/b")));
        assert!(!path_is_plain(Path::new("rel")));
        assert!(!path_is_plain(Path::new("/a\nb")));
    }
}
