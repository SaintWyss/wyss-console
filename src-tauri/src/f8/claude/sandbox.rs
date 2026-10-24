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
            "--unshare-user",
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
        // `--clearenv` empties what the payload gets, but the namespace's init process is bwrap's
        // own and keeps the environment it started with, which anything inside can read through
        // `/proc/1/environ`. So bwrap itself starts with none (it is run by absolute path).
        c.env_clear();
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
    fn a_user_namespace_is_required_not_merely_tried() {
        // `--unshare-user-try` would carry on without one where the kernel refuses; the launch
        // fails instead, so the isolation a runtime was admitted with cannot silently shrink.
        for net in [true, false] {
            let s = Sandbox::new(net);
            assert!(has(&s, "--unshare-user"));
            assert!(!has(&s, "--unshare-user-try"));
        }
    }

    fn run_inside(net: bool, binds: &[(&Path, &str)], script: &str) -> serde_json::Value {
        let mut s = Sandbox::new(net);
        for (src, dst) in binds {
            s.ro_bind(src, Path::new(dst));
        }
        let out = s
            .command(
                "/usr/bin/python3",
                &["-I".into(), "-c".into(), script.into()],
            )
            .output()
            .expect("bubblewrap runs");
        assert!(
            out.status.success(),
            "the namespace did not start: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).expect("the probe prints one JSON object")
    }

    /// What the namespace the bridge and the CLI live in can and cannot reach, observed from
    /// inside it with real sockets. This is a property of the namespace, not of the CLI's code.
    #[test]
    fn a_network_less_namespace_reaches_nothing_but_the_unix_socket_it_was_given() {
        use std::{
            io::Write,
            net::TcpListener,
            os::{
                linux::net::SocketAddrExt,
                unix::net::{SocketAddr, UnixListener},
            },
        };
        let host_tcp = TcpListener::bind("127.0.0.1:0").unwrap();
        let tcp_port = host_tcp.local_addr().unwrap().port();
        let name = format!("wyss-egress-{}", crate::control::store::uuid().as_str());
        let host_abstract =
            UnixListener::bind_addr(&SocketAddr::from_abstract_name(name.as_bytes()).unwrap())
                .unwrap();
        let dir = std::env::temp_dir().join(format!(
            "wyss-egress-{}",
            crate::control::store::uuid().as_str()
        ));
        std::fs::create_dir(&dir).unwrap();
        let sock = dir.join("b.sock");
        let granted = UnixListener::bind(&sock).unwrap();
        let answer = std::thread::spawn(move || {
            if let Ok((mut c, _)) = granted.accept() {
                let _ = c.write_all(b"ok");
            }
        });
        let probe = format!(
            r#"
import json, socket
def tcp(addr):
    try:
        socket.create_connection(addr, timeout=2).close()
        return "connected"
    except OSError as e:
        return "refused:%s" % e.errno
def unix(addr, read):
    s = socket.socket(socket.AF_UNIX); s.settimeout(2)
    try:
        s.connect(addr)
        return (s.recv(2).decode() if read else "connected")
    except OSError as e:
        return "refused:%s" % e.errno
print(json.dumps({{"host_loopback": tcp(("127.0.0.1", {tcp_port})), "internet": tcp(("1.1.1.1", 443)),
    "host_abstract_socket": unix("\0{name}", False), "granted_socket": unix("/run/wyss/broker/b.sock", True),
    "interfaces": sorted(n for _, n in socket.if_nameindex())}}))
"#
        );
        let dir_ref: &Path = &dir;
        // Without a network: only the socket that was bound in is reachable, and only `lo` exists.
        let closed = run_inside(false, &[(dir_ref, "/run/wyss/broker")], &probe);
        assert_eq!(closed["granted_socket"], "ok", "{closed}");
        for escaped in ["host_loopback", "internet", "host_abstract_socket"] {
            // A real refusal from the kernel (an errno), not a timeout: the route does not exist.
            let said = closed[escaped].as_str().unwrap();
            assert!(
                said.strip_prefix("refused:")
                    .is_some_and(|n| n.parse::<i32>().is_ok()),
                "{escaped}: {closed}"
            );
        }
        assert_eq!(closed["interfaces"], serde_json::json!(["lo"]), "{closed}");
        answer.join().unwrap();
        // The control: the same probe with the host's network does reach the host. If it did not,
        // the assertions above would prove nothing about the namespace.
        let open = run_inside(true, &[], &probe);
        assert_eq!(open["host_loopback"], "connected", "{open}");
        assert_eq!(open["host_abstract_socket"], "connected", "{open}");
        drop((host_tcp, host_abstract));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Found by the real-stack test that looks at the environment of everything below the launcher:
    /// the namespace's init process kept the launcher's whole environment, and anything inside can
    /// read `/proc/1/environ` (same user, same namespace). Tokens in the authority's environment
    /// were readable by the bridge, the CLI and every approved command.
    #[test]
    fn nothing_of_the_launchers_environment_is_readable_from_inside_the_namespace() {
        const CANARY: &str = "WYSS_TEST_CANARY_TOKEN";
        // SAFETY: a variable with a name no other test reads, set before the child exists.
        unsafe { std::env::set_var(CANARY, "must-not-be-seen") };
        let probe = r#"
import glob, json
seen = {}
for path in sorted(glob.glob("/proc/[0-9]*/environ")):
    try:
        data = open(path, "rb").read()
    except OSError:
        continue
    seen[path.split("/")[2]] = sorted(e.split(b"=", 1)[0].decode() for e in data.split(b"\0") if e)
print(json.dumps(seen))
"#;
        for net in [true, false] {
            let all = run_inside(net, &[], probe);
            let all = all.as_object().unwrap();
            assert!(all.len() >= 2, "the init process is visible: {all:?}");
            for (pid, names) in all {
                assert!(
                    !names.to_string().contains(CANARY),
                    "process {pid} inside the namespace can read the launcher's environment"
                );
            }
            // Pid 1 is the namespace's own init: it must have an empty environment.
            assert_eq!(all["1"], serde_json::json!([]), "net={net}");
        }
        unsafe { std::env::remove_var(CANARY) };
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
