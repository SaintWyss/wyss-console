//! Protocol client used by the isolated host tests.
use super::{
    protocol::{Answer, HostError, Request, read_frame, write_frame},
};
use serde_json::Value;
use std::{
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

const CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// The request never left whole: the host cannot have acted on it.
pub const HOST_UNAVAILABLE: &str = "host_unavailable";
/// The whole request was written and no answer came back: the host may have acted on it, once,
/// or not at all. Nothing here can tell which, and protocol 1 has no receipt to ask for.
pub const HOST_OUTCOME_UNKNOWN: &str = "host_outcome_unknown";

pub struct Remote {
    path: PathBuf,
}

impl Remote {
    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// One request, one answer. A failure before the whole frame was written is
    /// `host_unavailable`: the host reads a complete length-prefixed frame before it acts, so a
    /// partial one does nothing. A failure after that is `host_outcome_unknown`.
    pub fn call(&self, op: &str, params: Value) -> Result<Value, HostError> {
        let unsent = |_: std::io::Error| HostError::new(HOST_UNAVAILABLE);
        let mut stream = UnixStream::connect(&self.path).map_err(unsent)?;
        stream
            .set_read_timeout(Some(CALL_TIMEOUT))
            .map_err(unsent)?;
        stream
            .set_write_timeout(Some(CALL_TIMEOUT))
            .map_err(unsent)?;
        write_frame(
            &mut stream,
            &Request {
                op: op.to_owned(),
                params,
            },
        )
        .map_err(unsent)?;
        match read_frame::<Answer>(&mut stream).map_err(|_| HostError::new(HOST_OUTCOME_UNKNOWN))? {
            Answer::Ok(value) => Ok(value),
            Answer::Err(error) => Err(error),
        }
    }
}
