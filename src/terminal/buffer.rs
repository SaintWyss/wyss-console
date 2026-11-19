use super::contracts::*;
use crate::contracts::WireUuid;
use std::collections::VecDeque;

/// Fixed logical capacity. Only one delivered batch may remain unacknowledged.
pub(crate) struct OutputBuffer {
    bytes: VecDeque<u8>,
    pending: Option<WireUuid>,
    pub eof: bool,
    pub truncated: bool,
}

impl OutputBuffer {
    pub fn new() -> Self {
        Self {
            bytes: VecDeque::with_capacity(OUTPUT_CAPACITY),
            pending: None,
            eof: false,
            truncated: false,
        }
    }
    pub fn remaining(&self) -> usize {
        OUTPUT_CAPACITY - self.bytes.len()
    }
    pub fn append(&mut self, data: &[u8]) {
        assert!(data.len() <= self.remaining());
        self.bytes.extend(data);
    }
    pub fn take(
        &mut self,
        ack: Option<&WireUuid>,
    ) -> Result<(Option<WireUuid>, Vec<u8>), TerminalError> {
        if self.pending.as_ref() != ack {
            return Err(TerminalError::new(ErrorCode::InvalidAck));
        }
        self.pending = None;
        let count = self.bytes.len().min(OUTPUT_BATCH);
        let data = self.bytes.drain(..count).collect();
        if count > 0 {
            self.pending = Some(
                WireUuid::try_from(uuid::Uuid::new_v4().to_string())
                    .map_err(|error| TerminalError::context(ErrorCode::InternalPty, error))?,
            );
        }
        Ok((self.pending.clone(), data))
    }
    pub fn discard(&mut self) {
        self.truncated |= !self.bytes.is_empty() || self.pending.is_some();
        self.bytes.clear();
        self.pending = None;
        self.eof = true;
    }
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }
}
