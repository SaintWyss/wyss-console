use crate::contracts::{WireUuid, identities::TerminalId};
use serde::{Deserialize, Serialize};

pub const MAX_TERMINALS: usize = 16;
pub const OUTPUT_CAPACITY: usize = 64 * 1024;
pub const OUTPUT_BATCH: usize = 16 * 1024;
pub const INPUT_LIMIT: usize = 4 * 1024;
pub const COMMAND_CAPACITY: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalSize {
    pub rows: u16,
    pub cols: u16,
}

impl TerminalSize {
    pub fn validate(self) -> Result<Self, TerminalError> {
        if !(1..=512).contains(&self.rows) || !(1..=512).contains(&self.cols) {
            return Err(TerminalError::new(ErrorCode::InvalidDimensions));
        }
        Ok(self)
    }

    pub(crate) fn pty(self) -> portable_pty::PtySize {
        portable_pty::PtySize {
            rows: self.rows,
            cols: self.cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateTerminal {
    pub size: TerminalSize,
    pub cwd: Option<String>,
}

/// What a terminal runs: a program, its arguments as separate words, and variables added to
/// the environment. Rust only: the window never sends one (C-01); the desktop binary derives
/// it from a closed catalogue.
#[derive(Debug, Clone)]
pub struct Launch {
    pub program: std::path::PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalTarget {
    pub terminal_id: TerminalId,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteTerminal {
    pub terminal_id: TerminalId,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResizeTerminal {
    pub terminal_id: TerminalId,
    pub size: TerminalSize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadTerminal {
    pub terminal_id: TerminalId,
    pub ack: Option<WireUuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalState {
    Starting,
    Running,
    Exited,
    Closing,
    Closed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessExit {
    pub code: u32,
    pub signal: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalInfo {
    pub terminal_id: TerminalId,
    pub shell: String,
    pub cwd: String,
    pub observed_pid: Option<u32>,
    pub size: TerminalSize,
    pub state: TerminalState,
    pub exit: Option<ProcessExit>,
    pub error: Option<ErrorCode>,
}

/// A single credit-controlled transient byte batch, not an EventEnvelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalOutput {
    pub batch_id: Option<WireUuid>,
    pub data: Vec<u8>,
    pub eof: bool,
    pub truncated: bool,
    pub status: TerminalInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    UnknownTerminal,
    InvalidDimensions,
    InvalidCwd,
    SpawnFailed,
    WriteFailed,
    ResizeFailed,
    AlreadyClosed,
    Busy,
    InvalidInput,
    InvalidAck,
    LimitReached,
    InternalPty,
    OperationTimeout,
    CloseTimeout,
}

#[derive(Debug, Clone, Serialize)]
pub struct TerminalError {
    pub code: ErrorCode,
    /// Allows a caller to distinguish a partial write from no accepted input.
    pub bytes_written: usize,
    #[serde(skip)]
    pub diagnostic: Option<String>,
}

impl TerminalError {
    pub(crate) fn new(code: ErrorCode) -> Self {
        Self {
            code,
            bytes_written: 0,
            diagnostic: None,
        }
    }
    pub(crate) fn context(code: ErrorCode, error: impl std::fmt::Display) -> Self {
        Self {
            code,
            bytes_written: 0,
            diagnostic: Some(error.to_string().chars().take(512).collect()),
        }
    }
    pub(crate) fn partial(mut self, written: usize) -> Self {
        self.bytes_written = written;
        self
    }
}

impl std::fmt::Display for TerminalError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}", self.code)
    }
}
impl std::error::Error for TerminalError {}
