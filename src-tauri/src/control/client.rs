//! Future desktop bridge depends on this client, never raw provider/socket IO.
use super::{
    framing::{read_frame, write_frame},
    paths::runtime_product,
    server::peer,
    store::uuid,
};
use crate::contracts::{
    WireUuid,
    control::*,
    identities::{ExecutionLeaseId, ManagedSessionId, ManagedTurnId},
};
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
    pub fn turn_start(&mut self, p: TurnStart) -> ControlResult<ManagedTurnRecord> {
        turn(self.call(Operation::TurnStart(p))?)
    }
    pub fn turn_status(&mut self, id: ManagedTurnId) -> ControlResult<ManagedTurnRecord> {
        self.turn_status_projection(id).map(|s| s.turn)
    }
    pub fn turn_status_projection(
        &mut self,
        id: ManagedTurnId,
    ) -> ControlResult<ManagedTurnStatus> {
        match self.call(Operation::TurnStatus(TurnGet {
            managed_turn_id: id,
        }))? {
            ResultValue::TurnStatus(s) => Ok(s),
            _ => Err(ControlError::InvalidRequest),
        }
    }
    pub fn turn_cancel(&mut self, p: TurnCancel) -> ControlResult<ManagedTurnRecord> {
        turn(self.call(Operation::TurnCancel(p))?)
    }
    pub fn turn_events(&mut self, p: TurnEvents) -> ControlResult<TurnStreamPage> {
        match self.call(Operation::TurnEvents(p))? {
            ResultValue::TurnEvents(page) => Ok(page),
            _ => Err(ControlError::InvalidRequest),
        }
    }
    pub fn binding_create(&mut self, p: BindingCreate) -> ControlResult<SessionBinding> {
        binding(self.call(Operation::BindingCreate(p))?)
    }
    pub fn binding_resume(&mut self, p: BindingResume) -> ControlResult<SessionBinding> {
        binding(self.call(Operation::BindingResume(p))?)
    }
    pub fn binding_status(&mut self, id: ManagedSessionId) -> ControlResult<BindingStatus> {
        match self.call(Operation::BindingStatus(Get {
            managed_session_id: id,
        }))? {
            ResultValue::BindingStatus(s) => Ok(s),
            _ => Err(ControlError::InvalidRequest),
        }
    }
    pub fn binding_abandon(&mut self, p: BindingAbandon) -> ControlResult<AbandonedOperation> {
        match self.call(Operation::BindingAbandon(p))? {
            ResultValue::BindingAbandoned(s) => Ok(s),
            _ => Err(ControlError::InvalidRequest),
        }
    }
    pub fn lease_grant(&mut self, p: LeaseGrant) -> ControlResult<ExecutionLease> {
        lease(self.call(Operation::LeaseGrant(p))?)
    }
    pub fn lease_get(&mut self, id: ExecutionLeaseId) -> ControlResult<ExecutionLease> {
        lease(self.call(Operation::LeaseGet(LeaseGet { lease_id: id }))?)
    }
    pub fn lease_active(&mut self, id: ManagedSessionId) -> ControlResult<Option<ExecutionLease>> {
        match self.call(Operation::LeaseActive(LeaseActive { session_id: id }))? {
            ResultValue::ActiveLease(l) => Ok(l),
            _ => Err(ControlError::InvalidRequest),
        }
    }
    pub fn lease_renew(&mut self, p: LeaseRenew) -> ControlResult<ExecutionLease> {
        lease(self.call(Operation::LeaseRenew(p))?)
    }
    pub fn lease_revoke(&mut self, p: LeaseRevoke) -> ControlResult<ExecutionLease> {
        lease(self.call(Operation::LeaseRevoke(p))?)
    }
    /// Informational only. Every future execution operation must evaluate its own internal gate.
    pub fn gate_status(&mut self, p: GateQuery) -> ControlResult<GateStatus> {
        match self.call(Operation::GateStatus(p))? {
            ResultValue::GateStatus(g) => Ok(g),
            _ => Err(ControlError::InvalidRequest),
        }
    }
    pub fn runtime_start(&mut self, p: RuntimeMutation) -> ControlResult<RuntimeControlRecord> {
        runtime_control(self.call(Operation::RuntimeStart(p))?)
    }
    pub fn runtime_stop(&mut self, p: RuntimeMutation) -> ControlResult<RuntimeControlRecord> {
        runtime_control(self.call(Operation::RuntimeStop(p))?)
    }
    pub fn runtime_status(&mut self) -> ControlResult<RuntimeStatus> {
        match self.call(Operation::RuntimeStatus(RuntimeTarget::codex()))? {
            ResultValue::RuntimeStatus(s) => Ok(s),
            _ => Err(ControlError::InvalidRequest),
        }
    }
    pub fn runtime_models(&mut self, p: RuntimeModels) -> ControlResult<ResultValue> {
        match self.call(Operation::RuntimeModels(p))? {
            r @ ResultValue::RuntimeModels { .. } => Ok(r),
            _ => Err(ControlError::InvalidRequest),
        }
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
    let deadline = Instant::now()
        + if matches!(
            request.operation,
            Operation::BindingCreate(_) | Operation::BindingResume(_)
        ) {
            Duration::from_secs(16)
        } else {
            Duration::from_secs(5)
        };
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

fn runtime_control(r: ResultValue) -> ControlResult<RuntimeControlRecord> {
    if let ResultValue::RuntimeControl(r) = r {
        Ok(r)
    } else {
        Err(ControlError::InvalidRequest)
    }
}

fn lease(r: ResultValue) -> ControlResult<ExecutionLease> {
    if let ResultValue::Lease(l) = r {
        Ok(l)
    } else {
        Err(ControlError::InvalidRequest)
    }
}

fn binding(r: ResultValue) -> ControlResult<SessionBinding> {
    if let ResultValue::Binding(b) = r {
        Ok(b)
    } else {
        Err(ControlError::InvalidRequest)
    }
}

fn turn(r: ResultValue) -> ControlResult<ManagedTurnRecord> {
    if let ResultValue::Turn(t) = r {
        Ok(t)
    } else {
        Err(ControlError::InvalidRequest)
    }
}
