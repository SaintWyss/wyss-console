//! What the parser helper does to itself before it reads a byte: no new privileges, no core dump,
//! resource ceilings, Landlock (no file access, no TCP, no abstract-socket or signal reach outside
//! its own domain) and a seccomp deny-list (no sockets, no new processes, no exec, no tracing, no
//! mounts, no kernel interfaces a screen parser has no use for).
//!
//! Each layer is applied separately and *reported*; the host says which it requires and the helper
//! refuses to run without them. Nothing here is a claim about what a same-user attacker with
//! kernel exploits can do: these are the documented kernel mechanisms, observed to hold in the
//! probe tests, no more.

use super::ipc::layer;
use nix::libc;

/// Largest address space the helper may map (a screen of 512×512 cells, its history and one
/// repaint need a small fraction of it).
pub const ADDRESS_SPACE: u64 = 384 * 1024 * 1024;
/// Most descriptors it may hold (stdin, stdout, stderr, and room for the runtime).
pub const DESCRIPTORS: u64 = 8;
/// CPU seconds before the kernel ends it, as a last resort; the host's own deadlines come first.
pub const CPU_SECONDS: u64 = 3_600;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Applied {
    pub layers: u8,
    pub landlock_abi: u8,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Unavailable {
    /// The layer(s) required that could not be applied.
    pub missing: u8,
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn no_new_privs() -> bool {
    // SAFETY: prctl with integer arguments only.
    unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) == 0 }
}

fn not_dumpable() -> bool {
    // SAFETY: prctl with integer arguments only.
    unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) == 0 }
}

fn close_inherited() {
    // Everything above stderr: the host marks its descriptors close-on-exec, and this is the second
    // wall. Errors are ignored: an old kernel without close_range leaves the descriptors the host
    // never passed in the first place.
    // SAFETY: a raw syscall with integer arguments.
    unsafe {
        libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32);
    }
}

fn rlimits() -> bool {
    let set = |resource, limit: u64| -> bool {
        let value = libc::rlimit {
            rlim_cur: limit,
            rlim_max: limit,
        };
        // SAFETY: a valid pointer to a local struct.
        unsafe { libc::setrlimit(resource, &value) == 0 }
    };
    set(libc::RLIMIT_CORE, 0)
        && set(libc::RLIMIT_FSIZE, 0)
        && set(libc::RLIMIT_AS, ADDRESS_SPACE)
        && set(libc::RLIMIT_NOFILE, DESCRIPTORS)
        && set(libc::RLIMIT_CPU, CPU_SECONDS)
}

// ---- Landlock ------------------------------------------------------------------------------

const SYS_LANDLOCK_CREATE_RULESET: libc::c_long = 444;
const SYS_LANDLOCK_RESTRICT_SELF: libc::c_long = 446;
const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1;

#[repr(C)]
struct RulesetAttr {
    handled_access_fs: u64,
    handled_access_net: u64,
    scoped: u64,
}

/// The Landlock ABI version of the running kernel, 0 when there is none.
pub fn landlock_abi() -> u8 {
    // SAFETY: the documented version query: null attribute, size 0.
    let v = unsafe {
        libc::syscall(
            SYS_LANDLOCK_CREATE_RULESET,
            std::ptr::null::<RulesetAttr>(),
            0usize,
            LANDLOCK_CREATE_RULESET_VERSION,
        )
    };
    u8::try_from(v).unwrap_or(0)
}

/// A ruleset that handles every access the ABI knows and grants none: no file may be opened,
/// created, written, executed or listed; no TCP port bound or connected; no abstract unix socket
/// or signal reaches outside the domain.
fn landlock() -> Option<u8> {
    let abi = landlock_abi();
    if abi == 0 {
        return None;
    }
    let mut fs: u64 = (1 << 13) - 1; // EXECUTE .. MAKE_SYM
    if abi >= 2 {
        fs |= 1 << 13; // REFER
    }
    if abi >= 3 {
        fs |= 1 << 14; // TRUNCATE
    }
    if abi >= 5 {
        fs |= 1 << 15; // IOCTL_DEV
    }
    let net: u64 = if abi >= 4 { 0b11 } else { 0 }; // BIND_TCP | CONNECT_TCP
    // Below ABI 6 there is no signal scope; seccomp denies kill/tgkill/pidfd_* instead.
    let scoped: u64 = if abi >= 6 { 0b11 } else { 0 }; // ABSTRACT_UNIX_SOCKET | SIGNAL
    let attr = RulesetAttr {
        handled_access_fs: fs,
        handled_access_net: net,
        scoped,
    };
    let size = match abi {
        1..=3 => 8,
        4 | 5 => 16,
        _ => 24,
    };
    // SAFETY: `attr` is a valid, fully initialised #[repr(C)] struct and `size` never exceeds it.
    let fd = unsafe {
        libc::syscall(
            SYS_LANDLOCK_CREATE_RULESET,
            &attr as *const RulesetAttr,
            size,
            0u32,
        )
    };
    if fd < 0 {
        return None;
    }
    let fd = fd as i32;
    // SAFETY: a ruleset descriptor just created; no_new_privs was set before this is called.
    let ok = unsafe { libc::syscall(SYS_LANDLOCK_RESTRICT_SELF, fd, 0u32) } == 0;
    // SAFETY: closing the descriptor created above.
    unsafe {
        libc::close(fd);
    }
    ok.then_some(abi)
}

// ---- seccomp -------------------------------------------------------------------------------

#[cfg(target_arch = "x86_64")]
const AUDIT_ARCH: u32 = 0xC000_003E;
#[cfg(target_arch = "aarch64")]
const AUDIT_ARCH: u32 = 0xC000_00B7;
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
const AUDIT_ARCH: u32 = 0;

const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
const SECCOMP_RET_ALLOW: u32 = 0x7FFF_0000;
const BPF_LD_W_ABS: u16 = 0x20;
const BPF_JEQ_K: u16 = 0x15;
const BPF_RET_K: u16 = 0x06;
/// Commands that aim a descriptor's asynchronous-I/O signal at a process (same value on x86_64
/// and aarch64).
pub const F_SETOWN: u32 = 8;
pub const F_SETSIG: u32 = 10;
pub const F_SETOWN_EX: u32 = 15;
pub const FIOSETOWN: u32 = 0x8901;

/// What a screen parser never calls. Anything here ends the process.
pub fn denied_syscalls() -> Vec<i64> {
    let mut v: Vec<i64> = vec![
        libc::SYS_socket,
        libc::SYS_socketpair,
        libc::SYS_connect,
        libc::SYS_bind,
        libc::SYS_listen,
        libc::SYS_accept,
        libc::SYS_accept4,
        libc::SYS_execve,
        libc::SYS_execveat,
        libc::SYS_clone,
        libc::SYS_clone3,
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
        libc::SYS_memfd_create,
        // Signals and other processes. Landlock scopes signals only from ABI 6 on; this keeps
        // older kernels covered too, and a parser never signals or inspects anyone.
        libc::SYS_kill,
        libc::SYS_tkill,
        libc::SYS_tgkill,
        libc::SYS_rt_sigqueueinfo,
        libc::SYS_rt_tgsigqueueinfo,
        libc::SYS_pidfd_open,
        libc::SYS_pidfd_send_signal,
        libc::SYS_pidfd_getfd,
    ];
    #[cfg(target_arch = "x86_64")]
    v.extend([libc::SYS_fork, libc::SYS_vfork, libc::SYS_mknod]);
    v
}

fn filter() -> Vec<libc::sock_filter> {
    let stmt = |code: u16, k: u32| libc::sock_filter {
        code,
        jt: 0,
        jf: 0,
        k,
    };
    let denied = denied_syscalls();
    let n = denied.len();
    let mut prog = vec![
        stmt(BPF_LD_W_ABS, 4), // seccomp_data.arch
        libc::sock_filter {
            code: BPF_JEQ_K,
            jt: 1,
            jf: 0,
            k: AUDIT_ARCH,
        },
        stmt(BPF_RET_K, SECCOMP_RET_KILL_PROCESS),
        stmt(BPF_LD_W_ABS, 0), // seccomp_data.nr
    ];
    // x32 shares AUDIT_ARCH_X86_64, but its syscall numbers carry bit 30. Native-number
    // comparisons below cannot constrain that ABI. Refuse it before the deny-list.
    #[cfg(target_arch = "x86_64")]
    prog.extend([
        libc::sock_filter {
            code: 0x45, // BPF_JMP | BPF_JSET | BPF_K
            jt: 0,
            jf: 1,
            k: 0x4000_0000,
        },
        stmt(BPF_RET_K, SECCOMP_RET_KILL_PROCESS),
    ]);
    // After the deny-list: `fcntl`/`ioctl` are allowed except the commands that point a pipe's
    // asynchronous-I/O signal at another process (F_SETOWN, F_SETOWN_EX, F_SETSIG, FIOSETOWN).
    // Landlock's signal scope does not cover that path, and below ABI 6 nothing else does.
    let owner_cmds = |nr: i64, cmds: &[u32]| -> Vec<libc::sock_filter> {
        // check nr; if it matches, load the 2nd argument (low word) and kill on any listed command.
        let mut block = vec![libc::sock_filter {
            code: BPF_JEQ_K,
            jt: 0,
            jf: (cmds.len() + 1) as u8,
            k: nr as u32,
        }];
        block.push(stmt(BPF_LD_W_ABS, 16 + 8)); // seccomp_data.args[1], low word
        for (j, cmd) in cmds.iter().enumerate() {
            block.push(libc::sock_filter {
                code: BPF_JEQ_K,
                jt: (cmds.len() - j) as u8 + 1, // patched below to reach the kill
                jf: 0,
                k: *cmd,
            });
        }
        // Reload the syscall number for whatever check comes next.
        block.push(stmt(BPF_LD_W_ABS, 0));
        block
    };
    let fcntl_block = owner_cmds(
        libc::SYS_fcntl,
        &[F_SETOWN, F_SETSIG, F_SETOWN_EX],
    );
    let ioctl_block = owner_cmds(libc::SYS_ioctl, &[FIOSETOWN]);
    let m = fcntl_block.len() + ioctl_block.len();
    let first = prog.len();
    for (i, nr) in denied.iter().enumerate() {
        // The kill is the last instruction: after this one, the rest of the list, the owner
        // checks and the allow.
        let to_kill = (n - i + m) as u8;
        prog.push(libc::sock_filter {
            code: BPF_JEQ_K,
            jt: to_kill,
            jf: 0,
            k: *nr as u32,
        });
    }
    for mut block in [fcntl_block, ioctl_block] {
        // Distance from each command test to the final kill instruction (after the allow).
        let kill_at = first + n + m + 1;
        let base = prog.len();
        for (j, ins) in block.iter_mut().enumerate() {
            if ins.code == BPF_JEQ_K && ins.jt != 0 {
                ins.jt = (kill_at - (base + j + 1)) as u8;
            }
        }
        prog.extend(block);
    }
    prog.push(stmt(BPF_RET_K, SECCOMP_RET_ALLOW));
    prog.push(stmt(BPF_RET_K, SECCOMP_RET_KILL_PROCESS));
    prog
}

fn seccomp() -> bool {
    if AUDIT_ARCH == 0 {
        return false;
    }
    let prog = filter();
    let fprog = libc::sock_fprog {
        len: prog.len() as u16,
        filter: prog.as_ptr() as *mut libc::sock_filter,
    };
    // SAFETY: `fprog` points at `prog`, which lives to the end of this call; the kernel copies it.
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

/// Applies the layers in `want` (a `layer::*` mask) in the one order that works: seccomp last,
/// because it forbids nothing the earlier steps need but everything they would need to be undone.
/// `required` layers that did not take make this an error; the others are reported as not applied.
pub fn apply(want: u8, required: u8) -> Result<Applied, Unavailable> {
    let mut layers = 0u8;
    let mut abi = 0u8;
    if want & layer::NO_NEW_PRIVS != 0 && no_new_privs() {
        layers |= layer::NO_NEW_PRIVS;
    }
    if want & layer::NO_NEW_PRIVS != 0 {
        let _ = not_dumpable();
    }
    close_inherited();
    if want & layer::RLIMITS != 0 && rlimits() {
        layers |= layer::RLIMITS;
    }
    // Landlock and seccomp need no-new-privs first.
    if layers & layer::NO_NEW_PRIVS != 0 || want & layer::NO_NEW_PRIVS == 0 {
        if want & layer::LANDLOCK != 0
            && let Some(v) = landlock()
        {
            layers |= layer::LANDLOCK;
            abi = v;
        }
        if want & layer::SECCOMP != 0 && seccomp() {
            layers |= layer::SECCOMP;
        }
    }
    let missing = required & !layers;
    if missing != 0 {
        return Err(Unavailable { missing });
    }
    let _ = errno();
    Ok(Applied {
        layers,
        landlock_abi: abi,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filter_is_well_formed_and_every_denied_call_jumps_to_the_kill() {
        let prog = filter();
        let n = denied_syscalls().len();
        let start = if cfg!(target_arch = "x86_64") { 6 } else { 4 };
        // After the deny-list: the fcntl block (check, load, 3 commands, reload) and the ioctl
        // block (check, load, 1 command, reload), then allow and kill.
        let owner = 6 + 4;
        assert_eq!(prog.len(), start + n + owner + 2);
        let kill = prog.len() - 1;
        for (i, insn) in prog[start..start + n].iter().enumerate() {
            assert_eq!(start + i + 1 + usize::from(insn.jt), kill);
            assert_eq!(insn.code, BPF_JEQ_K);
        }
        // Every command test in the owner blocks also lands on the kill.
        let cmds = [F_SETOWN, F_SETSIG, F_SETOWN_EX, FIOSETOWN];
        let tests: Vec<usize> = (start + n..start + n + owner)
            .filter(|&i| cmds.contains(&prog[i].k) && prog[i].code == BPF_JEQ_K)
            .collect();
        assert_eq!(tests.len(), cmds.len());
        for i in tests {
            assert_eq!(i + 1 + usize::from(prog[i].jt), kill);
        }
        assert_eq!(prog[kill].k, SECCOMP_RET_KILL_PROCESS);
        assert_eq!(prog[kill - 1].k, SECCOMP_RET_ALLOW);
        assert!(n < 250, "a conditional jump reaches 255 instructions");
    }

    #[test]
    fn the_deny_list_has_no_duplicates_and_holds_the_ways_out() {
        let d = denied_syscalls();
        let mut sorted = d.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), d.len());
        for needed in [
            libc::SYS_socket,
            libc::SYS_connect,
            libc::SYS_execve,
            libc::SYS_ptrace,
            libc::SYS_clone3,
            libc::SYS_kill,
            libc::SYS_tgkill,
            libc::SYS_pidfd_send_signal,
            libc::SYS_process_vm_writev,
        ] {
            assert!(d.contains(&needed));
        }
        // What the parser itself needs is not denied.
        for allowed in [
            libc::SYS_read,
            libc::SYS_write,
            libc::SYS_mmap,
            libc::SYS_exit_group,
            libc::SYS_futex,
        ] {
            assert!(!d.contains(&allowed));
        }
    }
}
