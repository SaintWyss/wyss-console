//! Native protocol selection for a locally prepared launch. Planning/probing happens before
//! enqueueing a child. Neither client JSON nor protocol negotiation can supply argv or version.
use super::{
    engine::{Attachment, Containment, Spawned, Spawner, StopState, Worker},
    ownership::{ContainmentPolicy, Grace},
    process_worker::DeferredProcessWorker,
    protocol::RuntimeRef,
    provider_adapter::NativeCodexWorker,
    staging::LaunchStaging,
};
use crate::contracts::control::{ControlError as E, ControlResult};
use std::{path::PathBuf, process::Command, time::Instant};

/// Authority-owned resources from its version/configuration discovery. A prepared plan is
/// deliberately not deserializable or available through a Tauri command. Native local-tools
/// remains refused until its namespace/configuration and approval adapter are implemented.
pub struct NativeLaunchPlan {
    pub command: Command,
    pub attachment: Attachment,
    pub catalogue: PathBuf,
    pub observed_version: String,
}
pub struct NativeInferenceSpawner<P> {
    prepare: P,
}
impl<P> NativeInferenceSpawner<P> {
    pub fn new(prepare: P) -> Self {
        Self { prepare }
    }
}
impl<P> Spawner for NativeInferenceSpawner<P>
where
    P: FnMut(&RuntimeRef, Option<&mut LaunchStaging>) -> ControlResult<NativeLaunchPlan>,
{
    fn spawn(
        &mut self,
        reference: &RuntimeRef,
        staging: Option<&mut LaunchStaging>,
    ) -> ControlResult<Spawned> {
        let plan = (self.prepare)(reference, staging)?;
        // Refuse unsupported native contracts before creating a child. This is version
        // selection, never an attempt to fall back to the normalized fixture vocabulary.
        if plan.observed_version != "0.159.0" || plan.attachment.profile != "codex_inference_v1" {
            return Err(E::Unsupported);
        }
        if plan.attachment.revision == 0
            || !plan.catalogue.is_absolute()
            || !plan
                .command
                .get_program()
                .as_encoded_bytes()
                .starts_with(b"/")
        {
            return Err(E::InvalidRequest);
        }
        let worker = DeferredProcessWorker::spawn(
            plan.command,
            Grace::default(),
            ContainmentPolicy::DirectChildOnly,
        )?;
        let worker: Box<dyn Worker> = match NativeCodexWorker::inference_retaining(
            worker,
            reference.managed_session_id.clone(),
            reference.runtime_instance_id.clone(),
            plan.attachment,
            plan.catalogue,
            plan.observed_version,
        ) {
            Ok(native) => Box::new(native),
            // Once a child was enqueued, even initialization refusal must return its owner
            // to the engine. It consumes capacity and staging until the actual owned reap.
            Err((error, worker)) => Box::new(RefusedInitialization { error, worker }),
        };
        Ok(Spawned { worker })
    }
}
struct RefusedInitialization {
    error: E,
    worker: DeferredProcessWorker,
}
impl Worker for RefusedInitialization {
    fn send(&mut self, _: &[u8]) -> ControlResult<()> {
        Err(self.error)
    }
    fn flush(&mut self) -> ControlResult<()> {
        Err(self.error)
    }
    fn inbound(&mut self, _: usize) -> ControlResult<Vec<Vec<u8>>> {
        Err(self.error)
    }
    fn has_exited(&mut self) -> bool {
        self.worker.has_exited()
    }
    fn begin_stop(&mut self, now: Instant) {
        self.worker.begin_stop(now);
    }
    fn poll_stop(&mut self, now: Instant) -> StopState {
        self.worker.poll_stop(now)
    }
    fn containment(&mut self) -> Containment {
        self.worker.containment()
    }
}
