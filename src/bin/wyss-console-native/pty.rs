//! PTY plumbing on std plus the few libc calls std does not expose: open a master with
//! `posix_openpt`, run the child on the slave as session leader with it as controlling terminal,
//! and resize with `TIOCSWINSZ`.

use std::ffi::CStr;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::raw::{c_char, c_int, c_ulong};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const O_RDWR: c_int = 2;
const O_NOCTTY: c_int = 0o400;
const O_CLOEXEC: c_int = 0o2000000;
const TIOCSCTTY: c_ulong = 0x540E;
const TIOCSWINSZ: c_ulong = 0x5414;
const SIGHUP: c_int = 1;

#[repr(C)]
struct WinSize {
    rows: u16,
    cols: u16,
    xpixel: u16,
    ypixel: u16,
}

unsafe extern "C" {
    fn posix_openpt(flags: c_int) -> c_int;
    fn grantpt(fd: c_int) -> c_int;
    fn unlockpt(fd: c_int) -> c_int;
    fn ptsname_r(fd: c_int, buf: *mut c_char, len: usize) -> c_int;
    fn ioctl(fd: c_int, req: c_ulong, ...) -> c_int;
    fn setsid() -> c_int;
    fn kill(pid: c_int, sig: c_int) -> c_int;
}

fn check(rc: c_int) -> io::Result<c_int> {
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(rc)
    }
}

pub struct Pty {
    master: Arc<File>,
    pid: c_int,
    reaped: Arc<AtomicBool>,
}

/// What the reader thread needs: a handle to read output and the child to reap.
pub struct Reader {
    pub file: File,
    pub child: Child,
    reaped: Arc<AtomicBool>,
}

impl Reader {
    /// Waits for the child (the master already reported hang-up) and marks it reaped so `Pty::drop`
    /// does not signal a recycled pid.
    pub fn reap(mut self) {
        let _ = self.child.wait();
        self.reaped.store(true, Ordering::Release);
    }
}

impl Pty {
    /// Spawns `sh -c cmd` when `cmd` is given, else `$SHELL` (falling back to `/bin/sh`).
    pub fn spawn(cols: u16, rows: u16, cell_w: u16, cell_h: u16, cmd: Option<&str>) -> io::Result<(Pty, Reader)> {
        let master_fd = check(unsafe { posix_openpt(O_RDWR | O_NOCTTY | O_CLOEXEC) })?;
        // SAFETY: freshly opened and owned here.
        let master = unsafe { File::from_raw_fd(master_fd) };
        check(unsafe { grantpt(master_fd) })?;
        check(unsafe { unlockpt(master_fd) })?;
        let mut name = [0 as c_char; 128];
        let rc = unsafe { ptsname_r(master_fd, name.as_mut_ptr(), name.len()) };
        if rc != 0 {
            return Err(io::Error::from_raw_os_error(rc));
        }
        let path = unsafe { CStr::from_ptr(name.as_ptr()) }.to_string_lossy().into_owned();
        let ws = WinSize { rows, cols, xpixel: cols.saturating_mul(cell_w), ypixel: rows.saturating_mul(cell_h) };
        check(unsafe { ioctl(master_fd, TIOCSWINSZ, &ws as *const WinSize) })?;

        let slave = OpenOptions::new().read(true).write(true).open(&path)?;
        let mut command = match cmd {
            Some(c) => {
                let mut k = Command::new("/bin/sh");
                k.arg("-c").arg(c);
                k
            }
            None => Command::new(std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".into())),
        };
        command
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::from(slave.try_clone()?))
            .stderr(Stdio::from(slave));
        // SAFETY: only async-signal-safe calls (setsid, ioctl) between fork and exec.
        unsafe {
            command.pre_exec(|| {
                check(setsid())?;
                check(ioctl(0, TIOCSCTTY, 0 as c_int))?;
                Ok(())
            });
        }
        let child = command.spawn()?;
        let reaped = Arc::new(AtomicBool::new(false));
        let pid = child.id() as c_int;
        let reader = Reader { file: master.try_clone()?, child, reaped: reaped.clone() };
        Ok((Pty { master: Arc::new(master), pid, reaped }, reader))
    }

    pub fn pid(&self) -> i32 {
        self.pid
    }

    /// Writes user input to the child (keys, paste, terminal replies).
    pub fn writer(&self) -> Arc<File> {
        self.master.clone()
    }

    pub fn resize(&self, cols: u16, rows: u16, cell_w: u16, cell_h: u16) {
        let ws = WinSize { rows, cols, xpixel: cols.saturating_mul(cell_w), ypixel: rows.saturating_mul(cell_h) };
        unsafe { ioctl(self.master.as_raw_fd(), TIOCSWINSZ, &ws as *const WinSize) };
    }
}

impl Drop for Pty {
    /// Closing the tab hangs up the child's session, like closing a terminal window.
    fn drop(&mut self) {
        if !self.reaped.load(Ordering::Acquire) {
            unsafe { kill(-self.pid, SIGHUP) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn runs_a_command_on_a_pty_and_reports_exit() {
        let (pty, mut reader) = Pty::spawn(100, 30, 8, 16, Some("printf 'cols=%s\\n' \"$(stty size)\"")).unwrap();
        let _keep = pty.writer();
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match reader.file.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => out.extend_from_slice(&buf[..n]),
            }
        }
        reader.reap();
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("cols=30 100"), "{s:?}");
    }
}
