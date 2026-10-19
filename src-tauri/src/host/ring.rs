//! The bounded scrollback a host keeps for one terminal, so that a client that attaches later
//! (a window that was closed and opened again) sees what was printed, and several clients can
//! read the same terminal. Bytes have an absolute offset that only grows; a client asks for
//! "from here", and one that fell behind the bound is told so.
use std::collections::VecDeque;

/// Bytes kept per terminal.
pub const CAPACITY: usize = 512 * 1024;

#[derive(Default)]
pub struct Ring {
    bytes: VecDeque<u8>,
    /// Offset of the first kept byte.
    start: u64,
}

pub struct Read {
    pub data: Vec<u8>,
    /// Where the next read starts.
    pub next: u64,
    /// The client asked for bytes that are no longer kept.
    pub truncated: bool,
}

impl Ring {
    pub fn end(&self) -> u64 {
        self.start + self.bytes.len() as u64
    }

    pub fn push(&mut self, data: &[u8]) {
        self.bytes.extend(data.iter().copied());
        if self.bytes.len() > CAPACITY {
            let drop = self.bytes.len() - CAPACITY;
            self.bytes.drain(..drop);
            self.start += drop as u64;
        }
    }

    /// Up to `max` bytes from offset `from` (an offset beyond the end is the end).
    pub fn read(&self, from: u64, max: usize) -> Read {
        let truncated = from < self.start;
        let from = from.clamp(self.start, self.end());
        let skip = (from - self.start) as usize;
        let data: Vec<u8> = self.bytes.iter().skip(skip).take(max).copied().collect();
        Read {
            next: from + data.len() as u64,
            data,
            truncated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reader_continues_where_it_stopped_and_sees_all_of_it() {
        let mut ring = Ring::default();
        ring.push(b"hello ");
        ring.push(b"world");
        let first = ring.read(0, 4);
        assert_eq!(
            (first.data.as_slice(), first.next, first.truncated),
            (&b"hell"[..], 4, false)
        );
        let rest = ring.read(first.next, 100);
        assert_eq!(rest.data, b"o world");
        assert_eq!(ring.read(rest.next, 10).data, b"");
        assert_eq!(
            ring.read(9999, 10).next,
            ring.end(),
            "past the end is the end"
        );
    }

    #[test]
    fn what_falls_off_the_back_is_gone_and_the_reader_is_told() {
        let mut ring = Ring::default();
        ring.push(&vec![b'a'; CAPACITY]);
        ring.push(b"tail");
        assert_eq!(ring.end(), CAPACITY as u64 + 4);
        let read = ring.read(0, 10);
        assert!(read.truncated);
        assert_eq!(read.next, 4 + 10);
        // Memory stays bounded however much is printed.
        for _ in 0..8 {
            ring.push(&vec![b'b'; CAPACITY]);
        }
        assert!(ring.bytes.len() <= CAPACITY);
        assert!(ring.read(ring.end() - 4, 10).data == b"bbbb");
    }

    #[test]
    fn two_readers_do_not_disturb_each_other() {
        let mut ring = Ring::default();
        ring.push(b"abcdef");
        let a = ring.read(0, 3);
        let b = ring.read(0, 6);
        assert_eq!(a.data, b"abc");
        assert_eq!(b.data, b"abcdef");
    }
}
