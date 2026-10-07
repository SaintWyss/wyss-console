//! Future desktop bridge depends on this client, never raw provider/socket IO.
use super::{
    framing::{read_frame, write_frame},
    paths::runtime_product,
    server::peer,
    store::uuid,
};
use crate::contracts::{WireUuid, control::*, identities::ManagedSessionId};
use nix::{
    errno::Errno,
    sys::socket::{AddressFamily, SockFlag, SockType, UnixAddr, connect, socket},
    unistd::geteuid,
};
use std::os::fd::{AsRawFd, OwnedFd};
use std::{
    env,
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        net::UnixStream,
    },
    path::Path,
    sync::{OnceLock, atomic::AtomicBool},
    time::{Duration, Instant},
};

/// Managed references cannot be replaced with local terminal identities.
/// ```compile_fail
/// use wyss_console::{control::client::ControlClient,contracts::identities::TerminalId};
/// fn wrong(c: &mut ControlClient, id: TerminalId) { c.get(id); }
/// ```
/// ```compile_fail
/// use wyss_console::{control::client::ControlClient,contracts::identities::ConsoleSessionId};
/// fn wrong(c: &mut ControlClient, id: ConsoleSessionId) { c.get(id); }
/// ```
pub struct ControlClient {
    stream: UnixStream,
    instance: WireUuid,
    status: AuthorityStatus,
}
impl ControlClient {
    pub fn connect() -> ControlResult<Self> {
        let root = env::var_os("XDG_RUNTIME_DIR").ok_or(ControlError::UnsafePath)?;
        Self::connect_in_runtime(Path::new(&root))
    }
    /// Validates the same private runtime policy; useful for isolated developer clients.
    pub fn connect_in_runtime(root: &Path) -> ControlResult<Self> {
        let path = runtime_product(root, false)?.join("control.sock");
        let m = std::fs::symlink_metadata(&path).map_err(|_| ControlError::TransportError)?;
        if !m.file_type().is_socket() || m.uid() != geteuid().as_raw() || m.mode() & 0o777 != 0o600
        {
            return Err(ControlError::UnsafePath);
        }
        let fd = socket(
            AddressFamily::Unix,
            SockType::Stream,
            SockFlag::SOCK_NONBLOCK | SockFlag::SOCK_CLOEXEC,
            None,
        )
        .map_err(|_| ControlError::TransportError)?;
        let addr = UnixAddr::new(&path).map_err(|_| ControlError::UnsafePath)?;
        match connect(fd.as_raw_fd(), &addr) {
            Ok(()) => (),
            Err(Errno::EINPROGRESS) => return Err(ControlError::TransportError),
            Err(_) => return Err(ControlError::TransportError),
        }
        let owned: OwnedFd = fd;
        let mut stream = UnixStream::from(owned);
        peer(&stream)?;
        let request = Request {
            request_id: uuid(),
            operation: Operation::Hello(Hello {
                protocol_version: 1,
                client_instance_id: client_instance_id(),
            }),
        };
        let response = exchange(&mut stream, &request)?;
        let instance = response.authority_instance_id.clone();
        let ResultValue::Hello(status) = response.outcome? else {
            return Err(ControlError::InvalidRequest);
        };
        if status.authority_instance_id != instance {
            return Err(ControlError::CorrelationError);
        }
        Ok(Self {
            stream,
            instance,
            status,
        })
    }
    pub fn hello_status(&self) -> &AuthorityStatus {
        &self.status
    }
    fn call(&mut self, op: Operation) -> ControlResult<ResultValue> {
        let request = Request {
            request_id: uuid(),
            operation: op,
        };
        let response = exchange(&mut self.stream, &request)?;
        if response.authority_instance_id != self.instance {
            return Err(ControlError::CorrelationError);
        }
        response.outcome
    }
    pub fn status(&mut self) -> ControlResult<AuthorityStatus> {
        match self.call(Operation::Status(Empty {}))? {
            ResultValue::Status(s) if s.authority_instance_id == self.instance => Ok(s),
            _ => Err(ControlError::InvalidRequest),
        }
    }
    pub fn prepare(&mut self, p: Prepare) -> ControlResult<ManagedSessionRecord> {
        session(self.call(Operation::Prepare(p))?)
    }
    pub fn get(&mut self, id: ManagedSessionId) -> ControlResult<ManagedSessionRecord> {
        session(self.call(Operation::Get(Get {
            managed_session_id: id,
        }))?)
    }
    pub fn close(&mut self, p: Close) -> ControlResult<ManagedSessionRecord> {
        session(self.call(Operation::Close(p))?)
    }
    pub fn list(&mut self, p: List) -> ControlResult<ResultValue> {
        let r = self.call(Operation::List(p))?;
        if matches!(r, ResultValue::Sessions { .. }) {
            Ok(r)
        } else {
            Err(ControlError::InvalidRequest)
        }
    }
    pub fn audit(&mut self, p: AuditRead) -> ControlResult<ResultValue> {
        let r = self.call(Operation::AuditRead(p))?;
        if matches!(r, ResultValue::Audit { .. }) {
            Ok(r)
        } else {
            Err(ControlError::InvalidRequest)
        }
    }
}
fn session(r: ResultValue) -> ControlResult<ManagedSessionRecord> {
    if let ResultValue::Session(s) = r {
        Ok(s)
    } else {
        Err(ControlError::InvalidRequest)
    }
}
pub(super) fn exchange(stream: &mut UnixStream, request: &Request) -> ControlResult<Response> {
    let stop = AtomicBool::new(false);
    let deadline = Instant::now() + Duration::from_secs(5);
    write_frame(stream, &request.encode()?, deadline, &stop)?;
    let bytes = read_frame(stream, deadline, &stop)?.ok_or(ControlError::TransportError)?;
    let response = Response::parse(&bytes)?;
    if response.request_id != request.request_id {
        return Err(ControlError::CorrelationError);
    }
    Ok(response)
}

fn client_instance_id() -> WireUuid {
    static ID: OnceLock<WireUuid> = OnceLock::new();
    ID.get_or_init(uuid).clone()
}
