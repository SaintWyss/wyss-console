//! Test doubles for the Claude worker: a scripted bridge (the owned inner worker) and a recording
//! executor, shared by the worker's own tests and the authority parity tests.
use super::exec::{ActionExecutor, Approved, Outcome};
use crate::{
    contracts::control::{ControlError as E, ControlResult},
    f8::engine::{Containment, StopState, Worker},
};
use serde_json::Value;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Instant,
};

// ---- doubles ---------------------------------------------------------------------------
#[derive(Default)]
pub(crate) struct BridgeState {
    pub(crate) inbound: VecDeque<Vec<u8>>,
    pub(crate) sent: Vec<Vec<u8>>,
    pub(crate) exited: bool,
    pub(crate) stop: Option<StopState>,
    pub(crate) stopping: bool,
    pub(crate) fail_send: bool,
}
#[derive(Clone, Default)]
pub(crate) struct Bridge(pub(crate) Arc<Mutex<BridgeState>>);
impl Bridge {
    pub(crate) fn push(&self, v: Value) {
        self.0
            .lock()
            .unwrap()
            .inbound
            .push_back(serde_json::to_vec(&v).unwrap());
    }
    pub(crate) fn push_raw(&self, raw: &[u8]) {
        self.0.lock().unwrap().inbound.push_back(raw.to_vec());
    }
    pub(crate) fn sent(&self) -> Vec<Value> {
        self.0
            .lock()
            .unwrap()
            .sent
            .iter()
            .map(|f| serde_json::from_slice(f).unwrap())
            .collect()
    }
    pub(crate) fn sent_of(&self, c: &str) -> Vec<Value> {
        self.sent().into_iter().filter(|v| v["c"] == c).collect()
    }
}
impl Worker for Bridge {
    fn send(&mut self, frame: &[u8]) -> ControlResult<()> {
        let mut s = self.0.lock().unwrap();
        if s.fail_send {
            return Err(E::CapacityExceeded);
        }
        s.sent.push(frame.to_vec());
        Ok(())
    }
    fn inbound(&mut self, n: usize) -> ControlResult<Vec<Vec<u8>>> {
        let mut s = self.0.lock().unwrap();
        let k = n.min(s.inbound.len());
        Ok(s.inbound.drain(..k).collect())
    }
    fn has_exited(&mut self) -> bool {
        self.0.lock().unwrap().exited
    }
    fn begin_stop(&mut self, _: Instant) {
        self.0.lock().unwrap().stopping = true;
    }
    fn poll_stop(&mut self, _: Instant) -> StopState {
        self.0.lock().unwrap().stop.unwrap_or(StopState::Reaped)
    }
    fn containment(&mut self) -> Containment {
        Containment::PidNamespace
    }
}

#[derive(Default)]
pub(crate) struct ExecState {
    pub(crate) dispatched: Vec<Approved>,
    pub(crate) dispatch_error: Option<E>,
    pub(crate) outcome: Option<Outcome>,
    pub(crate) stopped: bool,
    pub(crate) busy: bool,
    /// As the real executor: an outcome ends the action, but the namespace behind it stays
    /// "busy" until `poll_stop` has reaped it.
    pub(crate) reaping: bool,
}
#[derive(Clone, Default)]
pub(crate) struct Exec(pub(crate) Arc<Mutex<ExecState>>);
impl ActionExecutor for Exec {
    fn dispatch(&mut self, a: Approved) -> ControlResult<()> {
        let mut s = self.0.lock().unwrap();
        if let Some(e) = s.dispatch_error {
            return Err(e);
        }
        s.dispatched.push(a);
        s.busy = true;
        Ok(())
    }
    fn poll(&mut self, _: Instant) -> Option<Outcome> {
        let mut s = self.0.lock().unwrap();
        let o = s.outcome.take();
        if o.is_some() {
            s.reaping = true;
        }
        o
    }
    fn busy(&self) -> bool {
        self.0.lock().unwrap().busy
    }
    fn begin_stop(&mut self, _: Instant) {
        self.0.lock().unwrap().stopped = true;
    }
    fn poll_stop(&mut self, _: Instant) -> StopState {
        let mut s = self.0.lock().unwrap();
        if s.reaping {
            s.busy = false;
            s.reaping = false;
        }
        StopState::Reaped
    }
}
