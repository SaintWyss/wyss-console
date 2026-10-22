//! Preparation has no access to SQLite. The engine owns staging and capacity until this
//! worker proves the preparation thread ended, then proves the actual worker was reaped.
use super::*;
use crate::f8::engine::{Attachment, Containment, StopState, Worker};
use std::{thread::JoinHandle, time::Instant};

pub(super) struct Preparing {
    job: Option<JoinHandle<ControlResult<NativeLocalToolsPlan>>>,
    reference: RuntimeRef,
    worker: Option<Box<dyn Worker>>,
    failed: Option<E>,
    stopped: bool,
    cleanup_uncertain: bool,
}
impl Preparing {
    pub(super) fn start(
        reference: RuntimeRef,
        prepare: impl FnOnce() -> ControlResult<NativeLocalToolsPlan> + Send + 'static,
    ) -> ControlResult<Self> {
        let job = std::thread::Builder::new()
            .name("f8-prepare".into())
            .spawn(prepare)
            .map_err(|_| E::CapacityExceeded)?;
        Ok(Self {
            job: Some(job),
            reference,
            worker: None,
            failed: None,
            stopped: false,
            cleanup_uncertain: false,
        })
    }
    fn advance(&mut self) {
        if !self.job.as_ref().is_some_and(JoinHandle::is_finished) {
            return;
        }
        // Joining only a finished thread never waits on the executable/probe.
        let result = match self.job.take().unwrap().join() {
            Ok(result) => result,
            Err(_) => {
                // A panic may have transferred a probe to the reaper. No completion proof.
                self.failed = Some(E::InternalError);
                self.cleanup_uncertain = true;
                return;
            }
        };
        if self.stopped {
            return;
        } // Drop owned grants; never spawn after cancellation.
        match result
            .and_then(|plan| NativeLocalToolsSpawner::new(plan).spawn(&self.reference, None))
        {
            Ok(spawned) => self.worker = Some(spawned.worker),
            Err(error) => self.failed = Some(error),
        }
    }
}
impl Worker for Preparing {
    fn approval_decisions(&self, item: &str) -> Vec<crate::contracts::control::ApprovalDecision> {
        self.worker
            .as_ref()
            .map_or_else(Vec::new, |w| w.approval_decisions(item))
    }

    fn preflight(&mut self, context: &Attachment) -> ControlResult<()> {
        self.advance();
        self.worker
            .as_mut()
            .ok_or(E::RuntimeOperationInProgress)?
            .preflight(context)
    }
    fn send(&mut self, frame: &[u8]) -> ControlResult<()> {
        self.advance();
        self.worker
            .as_mut()
            .ok_or(E::RuntimeOperationInProgress)?
            .send(frame)
    }
    fn send_critical(&mut self, frame: &[u8]) -> ControlResult<()> {
        self.advance();
        self.worker
            .as_mut()
            .ok_or(E::RuntimeOperationInProgress)?
            .send_critical(frame)
    }
    fn flush(&mut self) -> ControlResult<()> {
        self.advance();
        if let Some(error) = self.failed {
            return Err(error);
        }
        self.worker.as_mut().map_or(Ok(()), |w| w.flush())
    }
    fn inbound(&mut self, max: usize) -> ControlResult<Vec<Vec<u8>>> {
        self.advance();
        if let Some(error) = self.failed {
            return Err(error);
        }
        self.worker
            .as_mut()
            .map_or(Ok(Vec::new()), |w| w.inbound(max))
    }
    fn has_exited(&mut self) -> bool {
        self.advance();
        self.failed.is_some() || self.worker.as_mut().is_some_and(|w| w.has_exited())
    }
    fn begin_stop(&mut self, now: Instant) {
        self.stopped = true;
        self.advance();
        if let Some(worker) = &mut self.worker {
            worker.begin_stop(now);
        }
    }
    fn poll_stop(&mut self, now: Instant) -> StopState {
        self.advance();
        if self.job.is_some() {
            return StopState::Pending;
        }
        if self.cleanup_uncertain {
            return StopState::Stuck;
        }
        self.worker
            .as_mut()
            .map_or(StopState::Reaped, |w| w.poll_stop(now))
    }
    fn containment(&mut self) -> Containment {
        self.advance();
        self.worker
            .as_mut()
            .map_or(Containment::DirectChildOnly, |w| w.containment())
    }
    fn descendants(&self) -> usize {
        self.worker.as_ref().map_or(0, |w| w.descendants())
    }
    fn pid_for_diagnostics(&self) -> Option<i32> {
        self.worker.as_ref().and_then(|w| w.pid_for_diagnostics())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, time::Duration};
    fn reference() -> RuntimeRef {
        RuntimeRef {
            schema: 2,
            authority_instance_id: crate::contracts::WireUuid::try_from(
                uuid::Uuid::new_v4().to_string(),
            )
            .unwrap(),
            managed_session_id: crate::contracts::identities::ManagedSessionId::parse(
                &uuid::Uuid::new_v4().to_string(),
            )
            .unwrap(),
            runtime_instance_id: crate::contracts::identities::AgentRuntimeInstanceId::parse(
                &uuid::Uuid::new_v4().to_string(),
            )
            .unwrap(),
            generation: 1,
        }
    }
    #[test]
    fn cancellation_retains_preparation_until_the_thread_finishes() {
        let (tx, rx) = mpsc::channel();
        let mut worker = Preparing::start(reference(), move || {
            rx.recv().unwrap();
            Err(E::Unsupported)
        })
        .unwrap();
        worker.begin_stop(Instant::now());
        assert_eq!(worker.poll_stop(Instant::now()), StopState::Pending);
        tx.send(()).unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        while worker.poll_stop(Instant::now()) != StopState::Reaped {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn a_preparation_panic_cannot_claim_reap_proof() {
        let mut worker =
            Preparing::start(reference(), || panic!("synthetic preparation failure")).unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        while !worker.has_exited() {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
        worker.begin_stop(Instant::now());
        assert_eq!(worker.poll_stop(Instant::now()), StopState::Stuck);
    }
}
