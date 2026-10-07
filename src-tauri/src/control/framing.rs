use crate::contracts::control::{ControlError as E, ControlResult, MAX_FRAME};
use nix::{
    errno::Errno,
    poll::{PollFd, PollFlags, poll},
};
use std::{
    io::{Read, Write},
    os::{fd::AsFd, unix::net::UnixStream},
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};
fn await_io(
    stream: &UnixStream,
    deadline: Instant,
    stop: &AtomicBool,
    writing: bool,
) -> ControlResult<()> {
    if stop.load(Ordering::Acquire) {
        return Err(E::TransportError);
    }
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(E::Timeout);
    }
    let mut fds = [PollFd::new(
        stream.as_fd(),
        if writing {
            PollFlags::POLLOUT
        } else {
            PollFlags::POLLIN
        },
    )];
    match poll(&mut fds, left.as_millis().clamp(1, 50) as u16) {
        Ok(_) | Err(Errno::EINTR) => Ok(()),
        Err(_) => Err(E::TransportError),
    }
}
fn read_exact(
    stream: &mut UnixStream,
    out: &mut [u8],
    deadline: Instant,
    stop: &AtomicBool,
    clean_eof: bool,
) -> ControlResult<bool> {
    let mut offset = 0;
    while offset < out.len() {
        if stop.load(Ordering::Acquire) {
            return Err(E::TransportError);
        }
        if Instant::now() >= deadline {
            return Err(E::Timeout);
        }
        match stream.read(&mut out[offset..]) {
            Ok(0) if clean_eof && offset == 0 => return Ok(false),
            Ok(0) => return Err(E::TruncatedFrame),
            Ok(n) => offset += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                await_io(stream, deadline, stop, false)?
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => (),
            Err(_) => return Err(E::TransportError),
        }
    }
    Ok(true)
}
pub(crate) fn read_frame(
    stream: &mut UnixStream,
    deadline: Instant,
    stop: &AtomicBool,
) -> ControlResult<Option<Vec<u8>>> {
    let mut header = [0; 4];
    if !read_exact(stream, &mut header, deadline, stop, true)? {
        return Ok(None);
    }
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 {
        return Err(E::InvalidRequest);
    }
    if length > MAX_FRAME {
        return Err(E::FrameTooLarge);
    }
    let mut bytes = vec![0; length];
    read_exact(stream, &mut bytes, deadline, stop, false)?;
    Ok(Some(bytes))
}
pub(crate) fn write_frame(
    stream: &mut UnixStream,
    bytes: &[u8],
    deadline: Instant,
    stop: &AtomicBool,
) -> ControlResult<()> {
    if bytes.is_empty() {
        return Err(E::InvalidRequest);
    }
    if bytes.len() > MAX_FRAME {
        return Err(E::FrameTooLarge);
    }
    let header = (bytes.len() as u32).to_be_bytes();
    for part in [&header[..], bytes] {
        let mut offset = 0;
        while offset < part.len() {
            if stop.load(Ordering::Acquire) {
                return Err(E::TransportError);
            }
            if Instant::now() >= deadline {
                return Err(E::Timeout);
            }
            match stream.write(&part[offset..]) {
                Ok(0) => return Err(E::TransportError),
                Ok(n) => offset += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    await_io(stream, deadline, stop, true)?
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => (),
                Err(_) => return Err(E::TransportError),
            }
        }
    }
    Ok(())
}
