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
    fn supports_profile(&self, profile: crate::contracts::control::RuntimeSafetyProfile) -> bool {
        profile == crate::contracts::control::RuntimeSafetyProfile::CodexInferenceV1
    }
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

/// Host-only local-tools plan. It cannot be built from protocol parameters.
#[allow(dead_code)]
pub(crate) struct NativeLocalToolsPlan {
    launch: crate::adapters::codex::probe::isolation::IsolationLaunch,
    proof: crate::adapters::codex::probe::isolation::F8Attestation,
    attachment: Attachment,
    catalogue: PathBuf,
    version: String,
}
#[allow(dead_code)]
impl NativeLocalToolsPlan {
    pub(crate) fn prepare(
        isolation: crate::adapters::codex::probe::isolation::Isolation,
        executable: &std::path::Path,
        user: &std::path::Path,
        cwd: &std::path::Path,
        catalogue: PathBuf,
        observed_version: &str,
    ) -> ControlResult<Self> {
        Self::prepare_with_credential_policy(
            isolation,
            executable,
            user,
            cwd,
            catalogue,
            observed_version,
            true,
        )
    }
    pub(crate) fn prepare_with_credential_policy(
        isolation: crate::adapters::codex::probe::isolation::Isolation,
        executable: &std::path::Path,
        user: &std::path::Path,
        cwd: &std::path::Path,
        catalogue: PathBuf,
        observed_version: &str,
        allow_credential: bool,
    ) -> ControlResult<Self> {
        Self::prepare_inner(
            isolation,
            executable,
            user,
            cwd,
            catalogue,
            observed_version,
            allow_credential,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)] // Preserves the existing launch inputs plus its pinned FD.
    pub(crate) fn prepare_with_workspace_fd(
        isolation: crate::adapters::codex::probe::isolation::Isolation,
        executable: &std::path::Path,
        user: &std::path::Path,
        cwd: &std::path::Path,
        catalogue: PathBuf,
        observed_version: &str,
        allow_credential: bool,
        workspace_fd: std::fs::File,
    ) -> ControlResult<Self> {
        Self::prepare_inner(
            isolation,
            executable,
            user,
            cwd,
            catalogue,
            observed_version,
            allow_credential,
            Some(workspace_fd),
        )
    }

    #[allow(clippy::too_many_arguments)] // Shares one bounded launch constructor across profile modes.
    fn prepare_inner(
        isolation: crate::adapters::codex::probe::isolation::Isolation,
        executable: &std::path::Path,
        user: &std::path::Path,
        cwd: &std::path::Path,
        catalogue: PathBuf,
        observed_version: &str,
        allow_credential: bool,
        workspace_fd: Option<std::fs::File>,
    ) -> ControlResult<Self> {
        if observed_version != "0.159.0" || !catalogue.is_absolute() {
            return Err(E::Unsupported);
        }
        let attachment = Attachment {
            workspace: isolation
                .workspace
                .to_str()
                .ok_or(E::WorkspaceInvalid)?
                .into(),
            profile: "codex_local_tools_v1".into(),
            revision: 1,
        };
        let (launch, mut proof) = match workspace_fd {
            Some(fd) => {
                isolation.prepare_f8_with_workspace_fd(executable, user, cwd, &catalogue, fd)?
            }
            None => isolation.prepare_f8(executable, user, cwd, &catalogue)?,
        };
        // Check the acquired capability, not the pathname observed before preparation.
        if !allow_credential && proof.has_credential() {
            return Err(E::GateDenied);
        }
        proof.verify_version()?;
        Ok(Self {
            launch,
            proof,
            attachment,
            catalogue,
            version: observed_version.into(),
        })
    }
    #[cfg(test)]
    pub(crate) fn fixture_exec_failure(&mut self) {
        self.launch.fixture_exec_failure();
    }
    #[cfg(test)]
    pub(crate) fn fixture_without_namespace(&mut self) {
        self.launch = crate::adapters::codex::probe::isolation::IsolationLaunch::fixture(
            Command::new("/bin/true"),
            Vec::new(),
        );
    }
    #[cfg(test)]
    pub(crate) fn fixture_env(&mut self, key: &str, value: &std::ffi::OsStr) {
        self.launch.fixture_env(key, value);
    }
}
#[allow(dead_code)]
pub(crate) struct NativeLocalToolsSpawner {
    plan: Option<NativeLocalToolsPlan>,
    #[cfg(test)]
    fault: Option<super::provider_adapter::NativeCriticalFault>,
}
#[allow(dead_code)]
impl NativeLocalToolsSpawner {
    pub(crate) fn new(plan: NativeLocalToolsPlan) -> Self {
        Self {
            plan: Some(plan),
            #[cfg(test)]
            fault: None,
        }
    }
    #[cfg(test)]
    pub(crate) fn empty() -> Self {
        Self {
            plan: None,
            fault: None,
        }
    }
    #[cfg(test)]
    pub(crate) fn with_critical_fault(
        mut self,
        fault: Option<super::provider_adapter::NativeCriticalFault>,
    ) -> Self {
        self.fault = fault;
        self
    }
}
impl Spawner for NativeLocalToolsSpawner {
    fn supports_profile(&self, profile: crate::contracts::control::RuntimeSafetyProfile) -> bool {
        profile == crate::contracts::control::RuntimeSafetyProfile::CodexLocalToolsV1
    }
    fn descendant_containment(&self) -> bool {
        true
    }
    fn spawn(
        &mut self,
        reference: &RuntimeRef,
        _staging: Option<&mut LaunchStaging>,
    ) -> ControlResult<Spawned> {
        // This path only consumes already prepared, verified resources. It
        // cannot copy/probe an executable while the sole writer is polling.
        let plan = self.plan.take().ok_or(E::Unsupported)?;
        if !plan.proof.holds() {
            return Err(E::GateDenied);
        }
        let worker = DeferredProcessWorker::spawn_isolated(
            plan.launch,
            Grace::default(),
            ContainmentPolicy::DescendantsContained,
        )?;
        let worker: Box<dyn Worker> = match NativeCodexWorker::local_tools_retaining(
            worker,
            reference.managed_session_id.clone(),
            reference.runtime_instance_id.clone(),
            plan.attachment,
            plan.catalogue,
            plan.version,
            plan.proof,
        ) {
            Ok(native) => {
                #[cfg(test)]
                let mut native = native;
                #[cfg(test)]
                native.fixture_critical_fault(self.fault);
                Box::new(native)
            }
            Err((error, worker)) => Box::new(RefusedInitialization { error, worker }),
        };
        Ok(Spawned { worker })
    }
}
