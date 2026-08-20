//! Runtime observations and signal effects used by execution teardown.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::backends::conmon::lifecycle::{
    ExitReceipt, RuntimeStateObservation, read_exit_receipt, runtime_state,
    runtime_state_for_creator_attempt,
};
use crate::backends::conmon::runtime_process::{
    RuntimeProcessIdentity, RuntimeProcessIdentityObservation, RuntimeProcessSignal,
    RuntimeProcessSignalOutcome, capture_runtime_process_identity,
    inspect_runtime_process_identity, signal_authenticated_runtime_process,
};
use crate::{Result, SandboxError};

use super::super::manifest::{ContainerCreatorHandoffState, ContainerSandboxManifest};

/// Narrow substitution seam for the process observations and effects owned by
/// exact Container execution teardown.
pub(in crate::backends::container::runtime) trait ContainerExecutionTeardownRuntime:
    Send + Sync
{
    fn now_unix_millis(&self) -> Result<u64>;

    fn execution_is_terminal(&self, manifest: &ContainerSandboxManifest) -> Result<bool>;

    fn capture_process(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<RuntimeProcessIdentity>;

    fn inspect_process(
        &self,
        manifest: &ContainerSandboxManifest,
        identity: &RuntimeProcessIdentity,
    ) -> Result<RuntimeProcessIdentityObservation>;

    fn signal_process(
        &self,
        manifest: &ContainerSandboxManifest,
        identity: &RuntimeProcessIdentity,
        signal: RuntimeProcessSignal,
    ) -> Result<RuntimeProcessSignalOutcome>;
}

#[derive(Debug, Default)]
pub(in crate::backends::container::runtime) struct HostContainerExecutionTeardownRuntime;

impl ContainerExecutionTeardownRuntime for HostContainerExecutionTeardownRuntime {
    fn now_unix_millis(&self) -> Result<u64> {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!("system clock is before the Unix epoch: {error}"),
            })?;
        u64::try_from(duration.as_millis()).map_err(|_| SandboxError::OperationFailed {
            message: "system time cannot be represented in milliseconds".to_owned(),
        })
    }

    fn execution_is_terminal(&self, manifest: &ContainerSandboxManifest) -> Result<bool> {
        match read_exit_receipt(&manifest.conmon_layout.exit_status_file)? {
            ExitReceipt::Published { .. } => return Ok(true),
            // The receipt exists but carries no code yet, so terminality is not
            // observable here. Answering "not yet" lets the caller reconcile
            // through process inspection, which converges either when conmon
            // finishes publishing or when the process is seen absent.
            ExitReceipt::Unpublished => return Ok(false),
            ExitReceipt::Absent => {}
        }
        match &manifest.creator_handoff {
            ContainerCreatorHandoffState::NotSpawned => Ok(matches!(
                runtime_state(
                    &manifest.conmon_launch.state_command,
                    manifest.handle.id.as_str(),
                )?,
                RuntimeStateObservation::ExplicitlyAbsent
            )),
            ContainerCreatorHandoffState::SpawnIntent { .. }
            | ContainerCreatorHandoffState::Pending { .. } => Ok(false),
            ContainerCreatorHandoffState::Quiesced { proof } => Ok(matches!(
                runtime_state_for_creator_attempt(
                    &manifest.conmon_launch.state_command,
                    manifest.handle.id.as_str(),
                    proof.attempt_id(),
                )?,
                RuntimeStateObservation::ExplicitlyAbsent
            )),
            ContainerCreatorHandoffState::RuntimeObserved { receipt } => Ok(matches!(
                runtime_state_for_creator_attempt(
                    &manifest.conmon_launch.state_command,
                    manifest.handle.id.as_str(),
                    receipt.attempt_id(),
                )?,
                RuntimeStateObservation::ExplicitlyAbsent
            )),
        }
    }

    fn capture_process(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<RuntimeProcessIdentity> {
        capture_runtime_process_identity(
            &manifest.conmon_launch.state_command,
            manifest.handle.id.as_str(),
            current_creator_attempt_id(manifest)?,
            &manifest.conmon_layout.pidfile,
        )
    }

    fn inspect_process(
        &self,
        manifest: &ContainerSandboxManifest,
        identity: &RuntimeProcessIdentity,
    ) -> Result<RuntimeProcessIdentityObservation> {
        inspect_runtime_process_identity(
            identity,
            &manifest.conmon_launch.state_command,
            &manifest.conmon_layout.pidfile,
        )
    }

    fn signal_process(
        &self,
        manifest: &ContainerSandboxManifest,
        identity: &RuntimeProcessIdentity,
        signal: RuntimeProcessSignal,
    ) -> Result<RuntimeProcessSignalOutcome> {
        signal_authenticated_runtime_process(
            identity,
            &manifest.conmon_launch.state_command,
            &manifest.conmon_layout.pidfile,
            signal,
        )
    }
}

fn current_creator_attempt_id(manifest: &ContainerSandboxManifest) -> Result<&str> {
    match &manifest.creator_handoff {
        ContainerCreatorHandoffState::RuntimeObserved { receipt }
        | ContainerCreatorHandoffState::Pending { receipt } => Ok(receipt.attempt_id()),
        ContainerCreatorHandoffState::Quiesced { proof } => Ok(proof.attempt_id()),
        ContainerCreatorHandoffState::SpawnIntent { attempt_id } => Ok(attempt_id),
        ContainerCreatorHandoffState::NotSpawned => Err(SandboxError::OperationFailed {
            message: format!(
                "Container {} has no creator attempt that can authenticate a runtime process",
                manifest.handle.id
            ),
        }),
    }
}
