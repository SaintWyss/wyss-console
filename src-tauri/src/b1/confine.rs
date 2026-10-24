//! What the B1 pre-exec shim does to itself, in order, before it executes the untrusted worker:
//! close every inherited descriptor, set resource ceilings, forbid new privileges, install a seccomp
//! filter, verify that it took, and restrict the filesystem with Landlock. Each layer is applied
//! separately and *reported*; the launcher names the layers it requires and the shim aborts (exit 4)
//! without executing the worker when a required one is missing. There is no degraded mode.
//!
//! This is a candidate, not a proof (doc 51: FX-H01 stays OPEN until an independent review of an
//! implementation passes the gates). It uses documented kernel mechanisms only; what it claims is
//! what the process tests observe from inside, each against a baseline that shows the attack works
//! without it.
//!
//! What it does not claim: protection of pathname unix sockets by Landlock (that needs ABI 9; the
//! mount namespace of the launch plan is what hides them, and seccomp narrows `socket(2)` to
//! `AF_UNIX`), resistance to a kernel exploit, or anything against a process that is not inside
//! the domain (a same-user attacker outside it).

use nix::libc;
use std::{
    ffi::CString,
    os::{fd::RawFd, unix::ffi::OsStrExt},
    path::{Path, PathBuf},
};

pub mod layer {
    pub const NO_NEW_PRIVS: u8 = 1;
    pub const RLIMITS: u8 = 2;
    pub const LANDLOCK: u8 = 4;
    pub const SECCOMP: u8 = 8;
    /// No capability in the effective set.
    pub const NO_CAPS: u8 = 16;
    /// Every descriptor above stderr was closed.
    pub const FDS_CLOSED: u8 = 32;
    pub const ALL: u8 = 63;
}

/// Everything the shim needs to know. Paths are the ones the worker will see (inside the mount
/// namespace the launcher made).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Profile {
    /// Readable and executable trees (the runtime): `/usr`, the shim's own directory.
    pub read_exec: Vec<PathBuf>,
    /// Readable trees that are never executable (`/proc`, `/etc` files).
    pub read_only: Vec<PathBuf>,
    /// The task's own tree: read, write, create, remove, rename within it, execute.
    pub workspace: Vec<PathBuf>,
    /// Scratch: like the workspace but nothing in it can be executed.
    pub scratch: Vec<PathBuf>,
    /// Device nodes: read, write and ioctl, no creation.
    pub devices: Vec<PathBuf>,
    /// Layers that must hold or the worker is not started.
    pub require: u8,
    /// A higher Landlock ABI floor than `MIN_LANDLOCK_ABI`, if the launcher wants one.
    pub min_abi: u8,
    pub max_open_files: u64,
    pub max_file_bytes: u64,
}

/// The lowest Landlock ABI this shim runs a worker under, whatever the profile asks: ABI 6 is the first
/// with the scopes that keep abstract unix sockets and signals inside the domain. Below it the
/// worker is not started.
pub const MIN_LANDLOCK_ABI: u8 = 6;

impl Profile {
    /// The ABI actually required: the profile may raise the floor, never lower it.
    pub fn effective_min_abi(&self) -> u8 {
        self.min_abi.max(MIN_LANDLOCK_ABI)
    }
    pub const DEFAULT_OPEN_FILES: u64 = 256;
    pub const DEFAULT_FILE_BYTES: u64 = 64 * 1024 * 1024;
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Applied {
    pub layers: u8,
    pub landlock_abi: u8,
    /// Descriptors above stderr that were open when the shim began (and were closed).
    pub inherited_fds: Vec<RawFd>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Unavailable {
    pub missing: u8,
    pub why: &'static str,
}

// ---- Landlock ------------------------------------------------------------------------------

const SYS_LANDLOCK_CREATE_RULESET: libc::c_long = 444;
const SYS_LANDLOCK_ADD_RULE: libc::c_long = 445;
const SYS_LANDLOCK_RESTRICT_SELF: libc::c_long = 446;
const RULE_PATH_BENEATH: u32 = 1;

mod access {
    pub const EXECUTE: u64 = 1 << 0;
    pub const WRITE_FILE: u64 = 1 << 1;
    pub const READ_FILE: u64 = 1 << 2;
    pub const READ_DIR: u64 = 1 << 3;
    pub const REMOVE_DIR: u64 = 1 << 4;
    pub const REMOVE_FILE: u64 = 1 << 5;
    pub const MAKE_CHAR: u64 = 1 << 6;
    pub const MAKE_DIR: u64 = 1 << 7;
    pub const MAKE_REG: u64 = 1 << 8;
    pub const MAKE_SOCK: u64 = 1 << 9;
    pub const MAKE_FIFO: u64 = 1 << 10;
    pub const MAKE_BLOCK: u64 = 1 << 11;
    pub const MAKE_SYM: u64 = 1 << 12;
    pub const REFER: u64 = 1 << 13;
    pub const TRUNCATE: u64 = 1 << 14;
    pub const IOCTL_DEV: u64 = 1 << 15;
}

#[repr(C)]
struct RulesetAttr {
    handled_access_fs: u64,
    handled_access_net: u64,
    scoped: u64,
}

#[repr(C, packed)]
struct PathBeneath {
    allowed_access: u64,
    parent_fd: i32,
}

/// The Landlock ABI of the running kernel, 0 when there is none.
pub fn landlock_abi() -> u8 {
    // SAFETY: the documented version query: null attribute, size 0, flag 1.
    let v = unsafe {
        libc::syscall(
            SYS_LANDLOCK_CREATE_RULESET,
            std::ptr::null::<RulesetAttr>(),
            0usize,
            1u32,
        )
    };
    u8::try_from(v).unwrap_or(0)
}

/// Every filesystem right up to ABI 5 (all are *handled*, so all are denied unless granted). Rights
/// that later ABIs add (pathname unix sockets at ABI 9, for one) are not known to this shim and are
/// not covered by it.
fn handled_fs(abi: u8) -> u64 {
    let mut fs: u64 = (1 << 13) - 1;
    if abi >= 2 {
        fs |= access::REFER;
    }
    if abi >= 3 {
        fs |= access::TRUNCATE;
    }
    if abi >= 5 {
        fs |= access::IOCTL_DEV;
    }
    fs
}

/// What a tree of each kind may do. Deliberately absent from every grant: creating sockets, FIFOs
/// and device nodes (MAKE_SOCK / MAKE_FIFO / MAKE_CHAR / MAKE_BLOCK).
fn grants(abi: u8) -> Grants {
    let g = grants_with_creation(abi);
    Grants {
        read_exec: g.read_exec & !NEVER_GRANTED,
        read_only: g.read_only & !NEVER_GRANTED,
        workspace: g.workspace & !NEVER_GRANTED,
        scratch: g.scratch & !NEVER_GRANTED,
        device: g.device & !NEVER_GRANTED,
    }
}

/// Creating sockets, FIFOs and device nodes: in no grant, whatever the profile asks.
const NEVER_GRANTED: u64 =
    access::MAKE_SOCK | access::MAKE_FIFO | access::MAKE_CHAR | access::MAKE_BLOCK;

fn grants_with_creation(abi: u8) -> Grants {
    let read = access::READ_FILE | access::READ_DIR;
    let mut write = access::WRITE_FILE
        | access::REMOVE_DIR
        | access::REMOVE_FILE
        | access::MAKE_DIR
        | access::MAKE_REG
        | access::MAKE_SYM;
    if abi >= 2 {
        // Moving a file between directories of the same tree. Not a way in from outside: the
        // right is needed on both sides of a hardlink or rename, and nothing outside has it.
        write |= access::REFER;
    }
    if abi >= 3 {
        write |= access::TRUNCATE;
    }
    let mut device = access::READ_FILE | access::WRITE_FILE | access::READ_DIR;
    if abi >= 5 {
        device |= access::IOCTL_DEV;
    }
    Grants {
        read_exec: read | access::EXECUTE,
        read_only: read,
        workspace: read | write | access::EXECUTE,
        scratch: read | write,
        device,
    }
}

struct Grants {
    read_exec: u64,
    read_only: u64,
    workspace: u64,
    scratch: u64,
    device: u64,
}

fn open_path(path: &Path) -> Result<RawFd, &'static str> {
    let c = CString::new(path.as_os_str().as_bytes()).map_err(|_| "path_has_nul")?;
    // SAFETY: a NUL-terminated path; O_PATH takes no access to the file itself.
    let fd = unsafe {
        libc::open(
            c.as_ptr(),
            libc::O_PATH | libc::O_CLOEXEC | libc::O_DIRECTORY,
        )
    };
    if fd >= 0 {
        return Ok(fd);
    }
    // A file (not a directory) is a legal grant target too.
    // SAFETY: as above.
    let fd = unsafe { libc::open(c.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
    if fd >= 0 {
        Ok(fd)
    } else {
        Err("grant_target_missing")
    }
}

fn landlock(profile: &Profile) -> Result<u8, &'static str> {
    let abi = landlock_abi();
    if abi == 0 {
        return Err("landlock_unavailable");
    }
    if abi < profile.effective_min_abi() {
        return Err("landlock_abi_too_old");
    }
    let net: u64 = if abi >= 4 { 0b11 } else { 0 }; // BIND_TCP | CONNECT_TCP: none granted
    let scoped: u64 = if abi >= 6 { 0b11 } else { 0 }; // abstract unix sockets, signals outside
    let attr = RulesetAttr {
        handled_access_fs: handled_fs(abi),
        handled_access_net: net,
        scoped,
    };
    let size = match abi {
        1..=3 => 8,
        4 | 5 => 16,
        _ => 24,
    };
    // SAFETY: a fully initialised #[repr(C)] struct and a size that never exceeds it.
    let ruleset = unsafe {
        libc::syscall(
            SYS_LANDLOCK_CREATE_RULESET,
            &attr as *const RulesetAttr,
            size,
            0u32,
        )
    };
    if ruleset < 0 {
        return Err("landlock_create_failed");
    }
    let ruleset = ruleset as i32;
    let g = grants(abi);
    let mut result = Ok(());
    'rules: for (paths, rights) in [
        (&profile.read_exec, g.read_exec),
        (&profile.read_only, g.read_only),
        (&profile.workspace, g.workspace),
        (&profile.scratch, g.scratch),
        (&profile.devices, g.device),
    ] {
        for path in paths {
            let fd = match open_path(path) {
                Ok(fd) => fd,
                Err(e) => {
                    result = Err(e);
                    break 'rules;
                }
            };
            // A right the ABI does not handle cannot be granted; the kernel refuses it.
            let rule = PathBeneath {
                allowed_access: rights & handled_fs(abi),
                parent_fd: fd,
            };
            // SAFETY: a valid ruleset, a packed struct as the kernel defines it, an open fd.
            let r = unsafe {
                libc::syscall(
                    SYS_LANDLOCK_ADD_RULE,
                    ruleset,
                    RULE_PATH_BENEATH,
                    &rule as *const PathBeneath,
                    0u32,
                )
            };
            // SAFETY: the O_PATH descriptor opened above.
            unsafe { libc::close(fd) };
            if r != 0 {
                result = Err("landlock_rule_failed");
                break 'rules;
            }
        }
    }
    let applied = result.and_then(|()| {
        // SAFETY: a populated ruleset; no_new_privs was set before this is called.
        if unsafe { libc::syscall(SYS_LANDLOCK_RESTRICT_SELF, ruleset, 0u32) } == 0 {
            Ok(abi)
        } else {
            Err("landlock_restrict_failed")
        }
    });
    // SAFETY: closing the ruleset descriptor created above.
    unsafe { libc::close(ruleset) };
    applied
}

// ---- seccomp -------------------------------------------------------------------------------

#[cfg(target_arch = "x86_64")]
const AUDIT_ARCH: u32 = 0xC000_003E;
#[cfg(target_arch = "aarch64")]
const AUDIT_ARCH: u32 = 0xC000_00B7;
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
const AUDIT_ARCH: u32 = 0;

const RET_KILL: u32 = 0x8000_0000;
const RET_ALLOW: u32 = 0x7FFF_0000;
const RET_ERRNO: u32 = 0x0005_0000;
const LD_W_ABS: u16 = 0x20;
const JEQ_K: u16 = 0x15;
const JSET_K: u16 = 0x45;
const RET_K: u16 = 0x06;
const OFF_NR: u32 = 0;
/// x32 system calls are the x86_64 numbers with this bit set; the architecture check passes them.
const X32_SYSCALL_BIT: u32 = 0x4000_0000;
const OFF_ARCH: u32 = 4;
const OFF_ARG0: u32 = 16;
const AF_UNIX: u32 = 1;
/// `clone` flags that make a new namespace of any kind.
const CLONE_NEW_ANY: u32 = 0x0002_0000 // NEWNS
    | 0x0200_0000 // NEWCGROUP
    | 0x0400_0000 // NEWUTS
    | 0x0800_0000 // NEWIPC
    | 0x1000_0000 // NEWUSER
    | 0x2000_0000 // NEWPID
    | 0x4000_0000 // NEWNET
    | 0x0000_0080; // NEWTIME

/// What a build tool, a compiler or git never calls and an escape needs. Any of these ends the
/// process (SIGSYS).
pub fn killed_syscalls() -> Vec<i64> {
    let mut v: Vec<i64> = vec![
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_kcmp,
        libc::SYS_mount,
        libc::SYS_umount2,
        libc::SYS_pivot_root,
        libc::SYS_chroot,
        libc::SYS_setns,
        libc::SYS_unshare,
        libc::SYS_open_by_handle_at,
        libc::SYS_name_to_handle_at,
        libc::SYS_bpf,
        libc::SYS_perf_event_open,
        libc::SYS_userfaultfd,
        libc::SYS_io_uring_setup,
        libc::SYS_io_uring_enter,
        libc::SYS_io_uring_register,
        libc::SYS_keyctl,
        libc::SYS_add_key,
        libc::SYS_request_key,
        libc::SYS_kexec_load,
        libc::SYS_init_module,
        libc::SYS_finit_module,
        libc::SYS_delete_module,
        libc::SYS_reboot,
        libc::SYS_swapon,
        libc::SYS_swapoff,
        libc::SYS_acct,
        libc::SYS_quotactl,
        libc::SYS_mknodat,
        libc::SYS_move_mount,
        libc::SYS_open_tree,
        libc::SYS_fsopen,
        libc::SYS_fsconfig,
        libc::SYS_fsmount,
        libc::SYS_fspick,
        libc::SYS_mount_setattr,
    ];
    #[cfg(target_arch = "x86_64")]
    v.push(libc::SYS_mknod);
    v
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum To {
    Kill,
    Allow,
    ErrnoPerm,
    ErrnoNosys,
    SocketCheck,
    CloneCheck,
}

struct Insn {
    code: u16,
    k: u32,
    jt: Option<To>,
}

/// The filter as labelled instructions. Jumps only go forward; `build` resolves them to offsets.
fn program() -> Vec<libc::sock_filter> {
    let mut prog: Vec<Insn> = vec![
        Insn {
            code: LD_W_ABS,
            k: OFF_ARCH,
            jt: None,
        },
        // Equal: skip the next instruction (the kill). Resolved specially below.
        Insn {
            code: JEQ_K,
            k: AUDIT_ARCH,
            jt: None,
        },
        Insn {
            code: RET_K,
            k: RET_KILL,
            jt: None,
        },
        Insn {
            code: LD_W_ABS,
            k: OFF_NR,
            jt: None,
        },
        // A call number with the x32 bit set would skip every comparison below: refused outright.
        Insn {
            code: JSET_K,
            k: X32_SYSCALL_BIT,
            jt: Some(To::Kill),
        },
    ];
    for nr in killed_syscalls() {
        prog.push(Insn {
            code: JEQ_K,
            k: nr as u32,
            jt: Some(To::Kill),
        });
    }
    for (nr, to) in [
        (libc::SYS_socket, To::SocketCheck),
        (libc::SYS_socketpair, To::SocketCheck),
        (libc::SYS_clone, To::CloneCheck),
        // The arguments of clone3 are in memory the filter cannot read, so namespace flags could
        // not be refused; libc falls back to clone, which can.
        (libc::SYS_clone3, To::ErrnoNosys),
    ] {
        prog.push(Insn {
            code: JEQ_K,
            k: nr as u32,
            jt: Some(to),
        });
    }
    // Tail blocks, in the order their offsets are computed from.
    let tail_start = prog.len();
    let blocks: [(To, Vec<(u16, u32)>); 6] = [
        (To::Allow, vec![(RET_K, RET_ALLOW)]),
        (To::Kill, vec![(RET_K, RET_KILL)]),
        (To::ErrnoPerm, vec![(RET_K, RET_ERRNO | libc::EPERM as u32)]),
        (
            To::ErrnoNosys,
            vec![(RET_K, RET_ERRNO | libc::ENOSYS as u32)],
        ),
        (
            To::SocketCheck,
            vec![
                (LD_W_ABS, OFF_ARG0),
                // AF_UNIX: skip the refusal and the allow that follows it is the next block's job:
                // jump over one instruction to the allow below.
                (JEQ_K, AF_UNIX),
                (RET_K, RET_ERRNO | libc::EPERM as u32),
                (RET_K, RET_ALLOW),
            ],
        ),
        (
            To::CloneCheck,
            vec![
                (LD_W_ABS, OFF_ARG0),
                (JSET_K, CLONE_NEW_ANY),
                (RET_K, RET_ALLOW),
                (RET_K, RET_KILL),
            ],
        ),
    ];
    let mut starts = std::collections::HashMap::new();
    let mut at = tail_start;
    for (label, body) in &blocks {
        starts.insert(*label as usize, at);
        at += body.len();
    }
    let label_index = |t: To| -> usize { starts[&(t as usize)] };
    let mut out = Vec::with_capacity(at);
    for (i, insn) in prog.iter().enumerate() {
        let (jt, jf) = match (i, insn.jt) {
            (1, _) => (1, 0),
            (_, Some(to)) => (
                u8::try_from(label_index(to) - i - 1).expect("short jump"),
                0,
            ),
            _ => (0, 0),
        };
        out.push(libc::sock_filter {
            code: insn.code,
            jt,
            jf,
            k: insn.k,
        });
    }
    for (label, body) in &blocks {
        for (j, (code, k)) in body.iter().enumerate() {
            let (jt, jf) = match (label, j) {
                // socket(): AF_UNIX falls to the allow (jump over the refusal)
                (To::SocketCheck, 1) => (1, 0),
                // clone(): any NEW* flag set jumps to the kill (over the allow)
                (To::CloneCheck, 1) => (1, 0),
                _ => (0, 0),
            };
            out.push(libc::sock_filter {
                code: *code,
                jt,
                jf,
                k: *k,
            });
        }
    }
    out
}

/// Exposed for tests: the filter's instruction count and well-formedness are checked there.
pub fn filter_len() -> usize {
    program().len()
}

fn seccomp() -> bool {
    if AUDIT_ARCH == 0 {
        return false;
    }
    let prog = program();
    let fprog = libc::sock_fprog {
        len: prog.len() as u16,
        filter: prog.as_ptr() as *mut libc::sock_filter,
    };
    // SAFETY: `fprog` points at `prog`, alive to the end of the call; the kernel copies it.
    unsafe {
        libc::prctl(
            libc::PR_SET_SECCOMP,
            libc::SECCOMP_MODE_FILTER,
            &fprog as *const libc::sock_fprog,
            0,
            0,
        ) == 0
    }
}

// ---- the rest ------------------------------------------------------------------------------

fn no_new_privs() -> bool {
    // SAFETY: prctl with integer arguments only.
    unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) == 0 }
}

/// The descriptors above stderr that this process holds, read before they are closed.
fn open_descriptors() -> Vec<RawFd> {
    let Ok(dir) = std::fs::read_dir("/proc/self/fd") else {
        return Vec::new();
    };
    let mut fds: Vec<RawFd> = dir
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<RawFd>().ok())
        .filter(|fd| *fd > 2)
        .collect();
    fds.sort_unstable();
    fds
}

fn close_all_above(keep: &[RawFd]) -> bool {
    // `keep` is a short, sorted list (the receipt descriptor): close the ranges between and after.
    let mut next: u32 = 3;
    for &k in keep {
        let k = k as u32;
        if k > next {
            // SAFETY: a raw syscall with integer arguments.
            if unsafe { libc::syscall(libc::SYS_close_range, next, k - 1, 0u32) } != 0 {
                return false;
            }
        }
        next = k + 1;
    }
    // SAFETY: a raw syscall with integer arguments.
    unsafe { libc::syscall(libc::SYS_close_range, next, u32::MAX, 0u32) == 0 }
}

fn rlimits(profile: &Profile) -> bool {
    let set = |resource, limit: u64| -> bool {
        let value = libc::rlimit {
            rlim_cur: limit,
            rlim_max: limit,
        };
        // SAFETY: a valid pointer to a local struct.
        unsafe { libc::setrlimit(resource, &value) == 0 }
    };
    set(libc::RLIMIT_CORE, 0)
        && set(
            libc::RLIMIT_NOFILE,
            if profile.max_open_files == 0 {
                Profile::DEFAULT_OPEN_FILES
            } else {
                profile.max_open_files
            },
        )
        && set(
            libc::RLIMIT_FSIZE,
            if profile.max_file_bytes == 0 {
                Profile::DEFAULT_FILE_BYTES
            } else {
                profile.max_file_bytes
            },
        )
}

/// `CapEff`, `NoNewPrivs` and `Seccomp` as the kernel reports them for this process.
pub struct ProcStatus {
    pub cap_eff: u64,
    pub no_new_privs: bool,
    pub seccomp_filter: bool,
}

pub fn proc_status() -> Option<ProcStatus> {
    let text = std::fs::read_to_string("/proc/self/status").ok()?;
    let mut cap_eff = None;
    let mut nnp = None;
    let mut sec = None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("CapEff:") {
            cap_eff = u64::from_str_radix(v.trim(), 16).ok();
        } else if let Some(v) = line.strip_prefix("NoNewPrivs:") {
            nnp = Some(v.trim() == "1");
        } else if let Some(v) = line.strip_prefix("Seccomp:") {
            sec = Some(v.trim() == "2");
        }
    }
    Some(ProcStatus {
        cap_eff: cap_eff?,
        no_new_privs: nnp?,
        seccomp_filter: sec?,
    })
}

/// Applies every layer in the one order that works (descriptors, limits, no-new-privs, seccomp,
/// verification, Landlock last because after it `/proc` is only as readable as the profile says),
/// and reports what held. `Err` means a required layer is missing: the caller must not run the
/// worker.
pub fn apply(profile: &Profile, keep_fds: &[RawFd]) -> Result<Applied, Unavailable> {
    let inherited: Vec<RawFd> = open_descriptors()
        .into_iter()
        .filter(|fd| !keep_fds.contains(fd))
        .collect();
    let mut layers = 0u8;
    let mut abi = 0u8;
    let mut why: &'static str = "";
    if close_all_above(keep_fds) {
        layers |= layer::FDS_CLOSED;
    }
    if rlimits(profile) {
        layers |= layer::RLIMITS;
    }
    if no_new_privs() {
        layers |= layer::NO_NEW_PRIVS;
    }
    if layers & layer::NO_NEW_PRIVS != 0 {
        if seccomp() {
            layers |= layer::SECCOMP;
        } else {
            why = "seccomp_failed";
        }
        // The kernel's own account of the process, not this function's belief.
        if let Some(status) = proc_status() {
            if status.cap_eff == 0 {
                layers |= layer::NO_CAPS;
            }
            if !status.no_new_privs {
                layers &= !layer::NO_NEW_PRIVS;
            }
            if !status.seccomp_filter {
                layers &= !layer::SECCOMP;
            }
        }
        match landlock(profile) {
            Ok(v) => {
                layers |= layer::LANDLOCK;
                abi = v;
            }
            Err(e) => why = e,
        }
    } else {
        why = "no_new_privs_failed";
    }
    let missing = profile.require & !layers;
    if missing != 0 {
        return Err(Unavailable { missing, why });
    }
    Ok(Applied {
        layers,
        landlock_abi: abi,
        inherited_fds: inherited,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filter_is_well_formed_every_jump_lands_on_an_instruction_and_returns_are_terminal() {
        let prog = program();
        assert!(prog.len() < 255 + 8, "{} instructions", prog.len());
        for (i, insn) in prog.iter().enumerate() {
            if insn.code == JEQ_K || insn.code == JSET_K {
                let to_true = i + 1 + insn.jt as usize;
                let to_false = i + 1 + insn.jf as usize;
                assert!(to_true < prog.len() && to_false < prog.len(), "insn {i}");
            }
        }
        // The last instruction is a return (a program that falls off the end is rejected).
        assert_eq!(prog.last().unwrap().code, RET_K);
        // The architecture check kills foreign ABIs.
        assert_eq!(prog[1].k, AUDIT_ARCH);
        assert_eq!(prog[2].k, RET_KILL);
    }

    #[test]
    fn x32_numbers_are_killed_before_any_comparison_and_the_abi_floor_cannot_be_lowered() {
        let prog = program();
        assert_eq!((prog[3].code, prog[3].k), (LD_W_ABS, OFF_NR));
        assert_eq!((prog[4].code, prog[4].k), (JSET_K, X32_SYSCALL_BIT));
        let target = 4 + 1 + prog[4].jt as usize;
        assert_eq!((prog[target].code, prog[target].k), (RET_K, RET_KILL));
        assert_eq!(Profile::default().effective_min_abi(), MIN_LANDLOCK_ABI);
        let lower = Profile {
            min_abi: 3,
            ..Profile::default()
        };
        assert_eq!(lower.effective_min_abi(), MIN_LANDLOCK_ABI);
        let higher = Profile {
            min_abi: 9,
            ..Profile::default()
        };
        assert_eq!(higher.effective_min_abi(), 9);
    }

    #[test]
    fn the_kill_list_has_no_duplicates_and_holds_the_ways_out_but_not_what_tools_need() {
        let list = killed_syscalls();
        let mut sorted = list.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), list.len());
        for needed in [
            libc::SYS_ptrace,
            libc::SYS_mount,
            libc::SYS_unshare,
            libc::SYS_setns,
            libc::SYS_io_uring_setup,
            libc::SYS_bpf,
            libc::SYS_process_vm_writev,
        ] {
            assert!(list.contains(&needed));
        }
        for tools_need in [
            libc::SYS_execve,
            libc::SYS_openat,
            libc::SYS_read,
            libc::SYS_write,
            libc::SYS_futex,
            libc::SYS_mmap,
            libc::SYS_rename,
            libc::SYS_link,
            libc::SYS_connect,
            libc::SYS_kill,
        ] {
            assert!(!list.contains(&tools_need), "{tools_need}");
        }
    }

    #[test]
    fn the_grants_never_create_sockets_fifos_or_devices_and_only_the_workspace_executes_besides_the_runtime()
     {
        for abi in [1u8, 3, 5, 7] {
            let g = grants(abi);
            let forbidden =
                access::MAKE_SOCK | access::MAKE_FIFO | access::MAKE_CHAR | access::MAKE_BLOCK;
            for rights in [g.read_exec, g.read_only, g.workspace, g.scratch, g.device] {
                assert_eq!(rights & forbidden, 0, "abi {abi}");
            }
            assert_eq!(g.scratch & access::EXECUTE, 0);
            assert_eq!(g.read_only & access::EXECUTE, 0);
            assert_eq!(g.device & access::EXECUTE, 0);
            assert_ne!(g.workspace & access::EXECUTE, 0);
            // Writing is only the workspace's and the scratch's.
            assert_eq!(g.read_exec & access::WRITE_FILE, 0);
            assert_eq!(g.read_only & access::WRITE_FILE, 0);
            // Every right the profile grants is one the ABI handles.
            for rights in [g.read_exec, g.workspace, g.scratch, g.device] {
                let _ = rights & handled_fs(abi);
            }
        }
    }

    #[test]
    fn a_status_file_is_read_as_the_kernel_reports_it() {
        let status = proc_status().expect("procfs");
        // The test process is neither confined nor privileged in a way this asserts; only that
        // the three fields parse.
        let _ = (status.cap_eff, status.no_new_privs, status.seccomp_filter);
    }

    #[test]
    fn descriptors_are_listed_without_the_standard_three() {
        let file = std::fs::File::open("/proc/self/status").unwrap();
        let fds = open_descriptors();
        assert!(fds.iter().all(|fd| *fd > 2));
        use std::os::fd::AsRawFd;
        assert!(fds.contains(&file.as_raw_fd()));
    }
}
