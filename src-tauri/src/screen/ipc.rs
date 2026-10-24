//! The typed, bounded protocol between the host and a parser helper. Binary frames over the
//! helper's standard input and output: a 4-byte big-endian length, a 1-byte kind, the payload.
//!
//! Strict on both sides: an unknown kind, a short or long payload, a frame over the bound or a
//! field outside its range is an error and ends the conversation; nothing is skipped or guessed.
//! The helper holds no credential and takes no instruction that is not one of these frames.

use std::io::{self, Read, Write};

pub const PROTOCOL: u16 = 1;
/// Largest frame the host may send (bytes of kind + payload).
pub const MAX_REQUEST: usize = 256 * 1024;
/// Largest frame the helper may send: a repaint is bounded by `repaint::MAX_REPAINT`.
pub const MAX_RESPONSE: usize = 5 * 1024 * 1024;
/// Most output bytes in one `Feed`.
pub const MAX_FEED: usize = 64 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum IpcError {
    Io(io::ErrorKind),
    TooLarge,
    Truncated,
    Trailing,
    UnknownKind(u8),
    Invalid(&'static str),
}
impl From<io::Error> for IpcError {
    fn from(e: io::Error) -> Self {
        IpcError::Io(e.kind())
    }
}

/// What the host asks the helper to refuse to run without.
pub mod layer {
    pub const NO_NEW_PRIVS: u8 = 1;
    pub const RLIMITS: u8 = 2;
    pub const LANDLOCK: u8 = 4;
    pub const SECCOMP: u8 = 8;
    pub const ALL: u8 = NO_NEW_PRIVS | RLIMITS | LANDLOCK | SECCOMP;
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Request {
    Hello {
        version: u16,
        rows: u16,
        cols: u16,
        scrollback_lines: u32,
        /// 0 = the window's renderer (xterm.js Unicode 6), 1 = modern.
        width_table: u8,
        /// The isolation layers the helper must have applied, or it exits without a screen.
        require: u8,
    },
    Feed {
        /// Offset in the terminal's output of the first byte (the host's own numbering).
        offset: u64,
        bytes: Vec<u8>,
    },
    Resize {
        rows: u16,
        cols: u16,
    },
    Snapshot {
        scrollback_lines: u32,
    },
    Ping {
        nonce: u64,
    },
    Shutdown,
    /// The host's ring moved past bytes this screen was never given: the next byte it gets is at
    /// `to`. The parser forgets any sequence it was in the middle of and counts the bytes missed.
    Skip {
        to: u64,
    },
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Counters {
    pub clipboard: u64,
    pub hyperlink: u64,
    pub window_op: u64,
    pub query: u64,
    pub oversized: u64,
    pub string: u64,
    pub invalid_utf8: u64,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SnapshotBody {
    /// Output bytes the screen has consumed.
    pub fed: u64,
    pub generation: u64,
    pub rows: u16,
    pub cols: u16,
    pub alt_active: bool,
    pub modes: u32,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub title: String,
    pub counters: Counters,
    pub history_evicted: u64,
    /// Output bytes the screen never saw because the ring moved on (see `Request::Skip`).
    pub skipped: u64,
    pub scrollback_included: u32,
    pub scrollback_omitted: u32,
    pub repaint: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Response {
    Ready {
        version: u16,
        pid: u32,
        /// `layer::*` bits actually in force.
        applied: u8,
        landlock_abi: u8,
    },
    Fed {
        /// Offset after the last byte consumed.
        end: u64,
        generation: u64,
    },
    Snapshot(Box<SnapshotBody>),
    Pong {
        nonce: u64,
    },
    /// The helper will not continue: a closed word for why. Followed by its exit.
    Refused {
        code: Refusal,
    },
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Refusal {
    BadFrame = 1,
    NotReady = 2,
    AlreadyReady = 3,
    Version = 4,
    Size = 5,
    SandboxUnavailable = 6,
    OutOfOrder = 7,
}

impl Refusal {
    pub fn word(self) -> &'static str {
        match self {
            Refusal::BadFrame => "bad_frame",
            Refusal::NotReady => "not_ready",
            Refusal::AlreadyReady => "already_ready",
            Refusal::Version => "version",
            Refusal::Size => "size",
            Refusal::SandboxUnavailable => "sandbox_unavailable",
            Refusal::OutOfOrder => "out_of_order",
        }
    }
    fn from(b: u8) -> Option<Self> {
        Some(match b {
            1 => Refusal::BadFrame,
            2 => Refusal::NotReady,
            3 => Refusal::AlreadyReady,
            4 => Refusal::Version,
            5 => Refusal::Size,
            6 => Refusal::SandboxUnavailable,
            7 => Refusal::OutOfOrder,
            _ => return None,
        })
    }
}

// ---- encoding ------------------------------------------------------------------------------

struct Out(Vec<u8>);
impl Out {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn bytes(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.0.extend_from_slice(v);
    }
}

struct In<'a>(&'a [u8]);
impl<'a> In<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], IpcError> {
        if self.0.len() < n {
            return Err(IpcError::Truncated);
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn u8(&mut self) -> Result<u8, IpcError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, IpcError> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, IpcError> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, IpcError> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn bytes(&mut self, max: usize) -> Result<&'a [u8], IpcError> {
        let n = self.u32()? as usize;
        if n > max {
            return Err(IpcError::TooLarge);
        }
        self.take(n)
    }
    fn done(&self) -> Result<(), IpcError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(IpcError::Trailing)
        }
    }
}

impl Request {
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Out(Vec::new());
        match self {
            Request::Hello {
                version,
                rows,
                cols,
                scrollback_lines,
                width_table,
                require,
            } => {
                o.u8(1);
                o.u16(*version);
                o.u16(*rows);
                o.u16(*cols);
                o.u32(*scrollback_lines);
                o.u8(*width_table);
                o.u8(*require);
            }
            Request::Feed { offset, bytes } => {
                o.u8(2);
                o.u64(*offset);
                o.bytes(bytes);
            }
            Request::Resize { rows, cols } => {
                o.u8(3);
                o.u16(*rows);
                o.u16(*cols);
            }
            Request::Snapshot { scrollback_lines } => {
                o.u8(4);
                o.u32(*scrollback_lines);
            }
            Request::Ping { nonce } => {
                o.u8(5);
                o.u64(*nonce);
            }
            Request::Shutdown => o.u8(6),
            Request::Skip { to } => {
                o.u8(7);
                o.u64(*to);
            }
        }
        o.0
    }

    pub fn decode(frame: &[u8]) -> Result<Request, IpcError> {
        let mut i = In(frame);
        let kind = i.u8()?;
        let r = match kind {
            1 => Request::Hello {
                version: i.u16()?,
                rows: i.u16()?,
                cols: i.u16()?,
                scrollback_lines: i.u32()?,
                width_table: i.u8()?,
                require: i.u8()?,
            },
            2 => {
                let offset = i.u64()?;
                let bytes = i.bytes(MAX_FEED)?.to_vec();
                Request::Feed { offset, bytes }
            }
            3 => Request::Resize {
                rows: i.u16()?,
                cols: i.u16()?,
            },
            4 => Request::Snapshot {
                scrollback_lines: i.u32()?,
            },
            5 => Request::Ping { nonce: i.u64()? },
            6 => Request::Shutdown,
            7 => Request::Skip { to: i.u64()? },
            other => return Err(IpcError::UnknownKind(other)),
        };
        i.done()?;
        Ok(r)
    }
}

impl Response {
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Out(Vec::new());
        match self {
            Response::Ready {
                version,
                pid,
                applied,
                landlock_abi,
            } => {
                o.u8(0x81);
                o.u16(*version);
                o.u32(*pid);
                o.u8(*applied);
                o.u8(*landlock_abi);
            }
            Response::Fed { end, generation } => {
                o.u8(0x82);
                o.u64(*end);
                o.u64(*generation);
            }
            Response::Snapshot(s) => {
                o.u8(0x83);
                o.u64(s.fed);
                o.u64(s.generation);
                o.u16(s.rows);
                o.u16(s.cols);
                o.u8(u8::from(s.alt_active));
                o.u32(s.modes);
                o.u16(s.cursor_row);
                o.u16(s.cursor_col);
                o.bytes(s.title.as_bytes());
                for c in [
                    s.counters.clipboard,
                    s.counters.hyperlink,
                    s.counters.window_op,
                    s.counters.query,
                    s.counters.oversized,
                    s.counters.string,
                    s.counters.invalid_utf8,
                ] {
                    o.u64(c);
                }
                o.u64(s.history_evicted);
                o.u64(s.skipped);
                o.u32(s.scrollback_included);
                o.u32(s.scrollback_omitted);
                o.bytes(&s.repaint);
            }
            Response::Pong { nonce } => {
                o.u8(0x84);
                o.u64(*nonce);
            }
            Response::Refused { code } => {
                o.u8(0x85);
                o.u8(*code as u8);
            }
        }
        o.0
    }

    pub fn decode(frame: &[u8]) -> Result<Response, IpcError> {
        let mut i = In(frame);
        let kind = i.u8()?;
        let r = match kind {
            0x81 => Response::Ready {
                version: i.u16()?,
                pid: i.u32()?,
                applied: i.u8()?,
                landlock_abi: i.u8()?,
            },
            0x82 => Response::Fed {
                end: i.u64()?,
                generation: i.u64()?,
            },
            0x83 => {
                let fed = i.u64()?;
                let generation = i.u64()?;
                let rows = i.u16()?;
                let cols = i.u16()?;
                let alt_active = match i.u8()? {
                    0 => false,
                    1 => true,
                    _ => return Err(IpcError::Invalid("alt")),
                };
                let modes = i.u32()?;
                let cursor_row = i.u16()?;
                let cursor_col = i.u16()?;
                let title = String::from_utf8(i.bytes(1024)?.to_vec())
                    .map_err(|_| IpcError::Invalid("title"))?;
                let counters = Counters {
                    clipboard: i.u64()?,
                    hyperlink: i.u64()?,
                    window_op: i.u64()?,
                    query: i.u64()?,
                    oversized: i.u64()?,
                    string: i.u64()?,
                    invalid_utf8: i.u64()?,
                };
                let history_evicted = i.u64()?;
                let skipped = i.u64()?;
                let scrollback_included = i.u32()?;
                let scrollback_omitted = i.u32()?;
                let repaint = i.bytes(MAX_RESPONSE)?.to_vec();
                Response::Snapshot(Box::new(SnapshotBody {
                    fed,
                    generation,
                    rows,
                    cols,
                    alt_active,
                    modes,
                    cursor_row,
                    cursor_col,
                    title,
                    counters,
                    history_evicted,
                    skipped,
                    scrollback_included,
                    scrollback_omitted,
                    repaint,
                }))
            }
            0x84 => Response::Pong { nonce: i.u64()? },
            0x85 => Response::Refused {
                code: Refusal::from(i.u8()?).ok_or(IpcError::Invalid("refusal"))?,
            },
            other => return Err(IpcError::UnknownKind(other)),
        };
        i.done()?;
        Ok(r)
    }
}

// ---- framing -------------------------------------------------------------------------------

pub fn write_frame(w: &mut impl Write, body: &[u8], max: usize) -> Result<(), IpcError> {
    if body.is_empty() || body.len() > max {
        return Err(IpcError::TooLarge);
    }
    w.write_all(&(body.len() as u32).to_be_bytes())?;
    w.write_all(body)?;
    w.flush()?;
    Ok(())
}

/// Reads one frame. `Ok(None)` is a clean end of input at a frame boundary; an end in the middle
/// of a frame is `Truncated`.
pub fn read_frame(r: &mut impl Read, max: usize) -> Result<Option<Vec<u8>>, IpcError> {
    let mut head = [0u8; 4];
    let mut got = 0;
    while got < 4 {
        match r.read(&mut head[got..]) {
            Ok(0) if got == 0 => return Ok(None),
            Ok(0) => return Err(IpcError::Truncated),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    let len = u32::from_be_bytes(head) as usize;
    if len == 0 || len > max {
        return Err(IpcError::TooLarge);
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).map_err(|e| {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            IpcError::Truncated
        } else {
            e.into()
        }
    })?;
    Ok(Some(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello() -> Request {
        Request::Hello {
            version: PROTOCOL,
            rows: 24,
            cols: 80,
            scrollback_lines: 2000,
            width_table: 0,
            require: layer::ALL,
        }
    }

    #[test]
    fn every_request_and_response_round_trips() {
        for r in [
            hello(),
            Request::Feed {
                offset: 1 << 40,
                bytes: b"abc\x1b[0m".to_vec(),
            },
            Request::Resize { rows: 5, cols: 7 },
            Request::Snapshot {
                scrollback_lines: 99,
            },
            Request::Ping { nonce: 7 },
            Request::Shutdown,
            Request::Skip { to: 1 << 33 },
        ] {
            assert_eq!(Request::decode(&r.encode()).unwrap(), r);
        }
        let snap = SnapshotBody {
            fed: 5,
            generation: 2,
            rows: 24,
            cols: 80,
            alt_active: true,
            modes: 0x80c,
            cursor_row: 3,
            cursor_col: 4,
            title: "t".into(),
            counters: Counters {
                clipboard: 1,
                hyperlink: 2,
                window_op: 3,
                query: 4,
                oversized: 5,
                string: 6,
                invalid_utf8: 7,
            },
            history_evicted: 9,
            skipped: 12,
            scrollback_included: 10,
            scrollback_omitted: 11,
            repaint: vec![1, 2, 3],
        };
        for r in [
            Response::Ready {
                version: 1,
                pid: 42,
                applied: layer::ALL,
                landlock_abi: 6,
            },
            Response::Fed {
                end: 10,
                generation: 1,
            },
            Response::Snapshot(Box::new(snap)),
            Response::Pong { nonce: 9 },
            Response::Refused {
                code: Refusal::Version,
            },
        ] {
            assert_eq!(Response::decode(&r.encode()).unwrap(), r);
        }
    }

    #[test]
    fn nothing_odd_decodes() {
        // Unknown kinds, short and long payloads, an oversized feed and a bad refusal.
        assert_eq!(Request::decode(&[9]), Err(IpcError::UnknownKind(9)));
        assert_eq!(Request::decode(&[]), Err(IpcError::Truncated));
        let mut long = Request::Shutdown.encode();
        long.push(0);
        assert_eq!(Request::decode(&long), Err(IpcError::Trailing));
        let mut short = hello().encode();
        short.pop();
        assert_eq!(Request::decode(&short), Err(IpcError::Truncated));
        let mut feed = vec![2];
        feed.extend_from_slice(&0u64.to_be_bytes());
        feed.extend_from_slice(&((MAX_FEED + 1) as u32).to_be_bytes());
        feed.extend(std::iter::repeat_n(0, MAX_FEED + 1));
        assert_eq!(Request::decode(&feed), Err(IpcError::TooLarge));
        assert_eq!(
            Response::decode(&[0x85, 99]),
            Err(IpcError::Invalid("refusal"))
        );
        assert_eq!(Response::decode(&[0x80]), Err(IpcError::UnknownKind(0x80)));
    }

    #[test]
    fn frames_are_length_prefixed_bounded_and_strict_about_the_end() {
        let mut wire = Vec::new();
        write_frame(&mut wire, &hello().encode(), MAX_REQUEST).unwrap();
        let mut cursor = wire.as_slice();
        let body = read_frame(&mut cursor, MAX_REQUEST).unwrap().unwrap();
        assert_eq!(Request::decode(&body).unwrap(), hello());
        assert_eq!(read_frame(&mut cursor, MAX_REQUEST), Ok(None));
        // An end in the middle is not an end of conversation.
        let cut = &wire[..wire.len() - 1];
        assert_eq!(
            read_frame(&mut &cut[..], MAX_REQUEST),
            Err(IpcError::Truncated)
        );
        assert_eq!(
            read_frame(&mut &wire[..2], MAX_REQUEST),
            Err(IpcError::Truncated)
        );
        // A length over the bound or zero is refused before anything is allocated.
        let lie = (MAX_REQUEST as u32 + 1).to_be_bytes();
        assert_eq!(
            read_frame(&mut &lie[..], MAX_REQUEST),
            Err(IpcError::TooLarge)
        );
        assert_eq!(
            read_frame(&mut &[0u8, 0, 0, 0][..], MAX_REQUEST),
            Err(IpcError::TooLarge)
        );
        assert!(write_frame(&mut Vec::new(), &vec![1; MAX_REQUEST + 1], MAX_REQUEST).is_err());
    }
}
