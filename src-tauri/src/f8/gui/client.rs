//! The desktop's typed client of one protocol-2 authority.
//!
//! It speaks exactly what `f8::protocol` parses and decodes: a request is assembled from
//! typed parts and parsed back before it is sent, a response is decoded by the same codec
//! and correlated with the request that asked for it, with the authority that answered the
//! hello and with the identity projections it carries. A response that fails any of that is
//! an error, never data. It speaks protocol 2 only: a protocol-1 authority fails the hello
//! as `protocol_version_unsupported`, and nothing is retried in another version.
//!
//! How a stream to the authority comes to exist is a `Connector`'s business, so that the
//! same client runs against a socket and against an in-process application.
use crate::{
    contracts::{
        WireUuid,
        control::{
            AuthorityStatus, ControlError as E, ControlResult, Hello, Operation, ResultValue,
        },
    },
    control::{
        framing::{read_frame, write_frame},
        paths::runtime_product,
        peer,
        store::uuid,
    },
    f8::protocol::{Projection, Request, Response, Target},
};
use nix::{
    errno::Errno,
    sys::socket::{AddressFamily, SockFlag, SockType, UnixAddr, connect, socket},
    unistd::geteuid,
};
use std::{
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::{
            fs::{FileTypeExt, MetadataExt},
            net::UnixStream,
        },
    },
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

const SOCKET: &str = "control.sock";

/// Where a connection to the authority comes from. Nothing here chooses a protocol.
pub trait Connector: Send + Sync + 'static {
    fn connect(&self) -> ControlResult<UnixStream>;
}

/// The private socket of an authority in one runtime directory, under the same rules the
/// protocol-1 client applies: a socket, this user's, mode 0600, in a private directory.
pub struct SocketConnector {
    root: PathBuf,
}
impl SocketConnector {
    pub fn new(runtime_root: impl Into<PathBuf>) -> Self {
        Self {
            root: runtime_root.into(),
        }
    }
}
impl Connector for SocketConnector {
    fn connect(&self) -> ControlResult<UnixStream> {
        connect_private(&self.root)
    }
}
fn connect_private(root: &Path) -> ControlResult<UnixStream> {
    let path = runtime_product(root, false)?.join(SOCKET);
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| E::TransportError)?;
    if !metadata.file_type().is_socket()
        || metadata.uid() != geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o600
    {
        return Err(E::UnsafePath);
    }
    let fd = socket(
        AddressFamily::Unix,
        SockType::Stream,
        SockFlag::SOCK_NONBLOCK | SockFlag::SOCK_CLOEXEC,
        None,
    )
    .map_err(|_| E::TransportError)?;
    let address = UnixAddr::new(&path).map_err(|_| E::UnsafePath)?;
    match connect(fd.as_raw_fd(), &address) {
        Ok(()) => (),
        Err(Errno::EINPROGRESS) | Err(_) => return Err(E::TransportError),
    }
    let owned: OwnedFd = fd;
    let stream = UnixStream::from(owned);
    peer(&stream)?;
    Ok(stream)
}

/// Why a call did not return an answer, in the one distinction a link needs: whether the
/// connection can still be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallError {
    /// The authority answered, correlated and decoded, and refused. The connection is fine.
    Refused(E),
    /// The request was never sent: it is not one the protocol admits.
    Local(E),
    /// The wire failed, or the peer broke the protocol. Nothing says what it is doing: the
    /// connection must be dropped.
    Broken(E),
}
impl CallError {
    pub fn code(self) -> E {
        match self {
            Self::Refused(e) | Self::Local(e) | Self::Broken(e) => e,
        }
    }
    pub fn broken(self) -> bool {
        matches!(self, Self::Broken(_))
    }
}

/// What an exchange answered: the result and what it says about the runtime it concerns.
#[derive(Debug, Clone)]
pub struct Answer {
    pub value: ResultValue,
    pub projection: Projection,
}

/// One greeted connection. It is used by one thread at a time.
pub struct F8Client {
    stream: UnixStream,
    authority: WireUuid,
    status: AuthorityStatus,
    cancel: Arc<AtomicBool>,
}
impl F8Client {
    /// Greets the authority on `stream` in protocol 2 and checks what it answered.
    pub fn greet(mut stream: UnixStream, cancel: Arc<AtomicBool>) -> ControlResult<Self> {
        stream
            .set_nonblocking(true)
            .map_err(|_| E::TransportError)?;
        let request = Request::new(
            uuid(),
            None,
            Operation::Hello(Hello {
                protocol_version: 2,
                client_instance_id: uuid(),
            }),
            Target::None,
        )?;
        let response = send(&mut stream, &request, &cancel)?;
        let authority = response.authority().clone();
        response.correlate(&request, &authority)?;
        let ResultValue::Hello(status) = response.outcome().clone()? else {
            return Err(E::CorrelationError);
        };
        if status.protocol_version != 2
            || status.storage_schema_version != 8
            || status.authority_instance_id != authority
        {
            return Err(E::ProtocolVersionUnsupported);
        }
        Ok(Self {
            stream,
            authority,
            status,
            cancel,
        })
    }
    pub fn authority(&self) -> &WireUuid {
        &self.authority
    }
    pub fn status(&self) -> &AuthorityStatus {
        &self.status
    }
    /// One request and its correlated answer. A refusal is `Err` with the authority's
    /// closed code; a transport or correlation failure is `Err` too, and the caller must
    /// drop the connection on those: nothing says what the peer is doing.
    pub fn call(&mut self, operation: Operation, target: Target) -> Result<Answer, CallError> {
        self.call_checked(operation, target, None)
    }
    /// A binding create, whose answer is only accepted for the canonical workspace the
    /// caller independently accepted.
    pub fn call_binding_create(
        &mut self,
        operation: Operation,
        target: Target,
        canonical_workspace: &str,
    ) -> Result<Answer, CallError> {
        self.call_checked(operation, target, Some(canonical_workspace))
    }
    fn call_checked(
        &mut self,
        operation: Operation,
        target: Target,
        canonical_workspace: Option<&str>,
    ) -> Result<Answer, CallError> {
        let request = Request::new(uuid(), Some(self.authority.clone()), operation, target)
            .map_err(CallError::Local)?;
        let response = send(&mut self.stream, &request, &self.cancel).map_err(CallError::Broken)?;
        match canonical_workspace {
            Some(workspace) => {
                response.correlate_binding_create(&request, &self.authority, workspace)
            }
            None => response.correlate(&request, &self.authority),
        }
        .map_err(CallError::Broken)?;
        match response.outcome() {
            Ok(value) => Ok(Answer {
                value: value.clone(),
                projection: response.projection().clone(),
            }),
            Err(code) => Err(CallError::Refused(*code)),
        }
    }
}

fn send(
    stream: &mut UnixStream,
    request: &Request,
    cancel: &AtomicBool,
) -> ControlResult<Response> {
    let deadline = Instant::now()
        + match request.operation() {
            // The authority answers within the requested wait; the margin is transport.
            Operation::AuditWatch(p) => {
                Duration::from_millis(p.wait_ms.into()) + Duration::from_secs(5)
            }
            // These reply only after the runtime answered the authority.
            Operation::BindingCreate(_)
            | Operation::BindingResume(_)
            | Operation::ApprovalRespond(_) => Duration::from_secs(16),
            _ => Duration::from_secs(5),
        };
    write_frame(stream, &request.encode()?, deadline, cancel)?;
    let bytes = read_frame(stream, deadline, cancel)?.ok_or(E::TransportError)?;
    Response::parse(&bytes)
}

/// An application in this process, reached through its admission handle. A local
/// connector: it opens no path and no listener, it hands the application one end of a pair.
pub struct HandleConnector(pub crate::control::application::ApplicationHandle);
impl Connector for HandleConnector {
    fn connect(&self) -> ControlResult<UnixStream> {
        let (owned, client) = UnixStream::pair().map_err(|_| E::TransportError)?;
        self.0.attach(owned)?;
        Ok(client)
    }
}
