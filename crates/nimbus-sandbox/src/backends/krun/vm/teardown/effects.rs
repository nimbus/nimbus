//! Runtime observations and signal effects used by exact Krun execution teardown.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::backends::conmon::lifecycle::{
    RuntimeStateObservation, runtime_state_for_creator_attempt,
};
use crate::backends::conmon::runtime_process::{
    RuntimeProcessIdentity, RuntimeProcessIdentityObservation, RuntimeProcessSignal,
    RuntimeProcessSignalOutcome, capture_runtime_process_identity,
    inspect_runtime_process_identity, signal_authenticated_runtime_process,
};
use crate::{Result, SandboxError};

use super::super::{KrunCreatorHandoffState, KrunSandboxManifest};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::backends::krun::vm) enum KrunExecutionTerminalObservation {
    NotObserved,
    /// The exact creator-qualified provider state is terminal but the runtime
    /// object remains available for later network and artifact cleanup.
    ExactStopped,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the exact-receipt state is exercised through the teardown runtime substitution seam"
        )
    )]
    ExactExit {
        exit_code: i32,
    },
    ExplicitAbsence,
}

/// Narrow substitution seam for Krun process observations and signal effects.
pub(in crate::backends::krun::vm) trait KrunExecutionTeardownRuntime:
    Send + Sync
{
    fn now_unix_millis(&self) -> Result<u64>;

    /// Observe terminality only from an exit receipt paired with exact
    /// creator absence, or from creator-authenticated explicit absence alone.
    fn observe_execution_terminal(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<KrunExecutionTerminalObservation>;

    fn capture_process(&self, manifest: &KrunSandboxManifest) -> Result<RuntimeProcessIdentity>;

    fn inspect_process(
        &self,
        manifest: &KrunSandboxManifest,
        identity: &RuntimeProcessIdentity,
    ) -> Result<RuntimeProcessIdentityObservation>;

    fn signal_process(
        &self,
        manifest: &KrunSandboxManifest,
        identity: &RuntimeProcessIdentity,
        signal: RuntimeProcessSignal,
    ) -> Result<RuntimeProcessSignalOutcome>;
}

#[derive(Debug, Default)]
pub(in crate::backends::krun::vm) struct HostKrunExecutionTeardownRuntime;

impl KrunExecutionTeardownRuntime for HostKrunExecutionTeardownRuntime {
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

    fn observe_execution_terminal(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<KrunExecutionTerminalObservation> {
        let provider_state = match &manifest.creator_handoff {
            KrunCreatorHandoffState::Quiesced { proof } => Some(runtime_state_for_creator_attempt(
                &manifest.conmon_launch.state_command,
                manifest.handle.id.as_str(),
                proof.attempt_id(),
            )?),
            KrunCreatorHandoffState::RuntimeObserved { receipt } => {
                Some(runtime_state_for_creator_attempt(
                    &manifest.conmon_launch.state_command,
                    manifest.handle.id.as_str(),
                    receipt.attempt_id(),
                )?)
            }
            KrunCreatorHandoffState::NotSpawned
            | KrunCreatorHandoffState::SpawnIntent { .. }
            | KrunCreatorHandoffState::Pending { .. } => None,
        };
        // Krun's legacy integer exit-status path is not attempt-qualified. It
        // cannot prove that a receipt belongs to the current execution. Treat
        // exact current-creator provider state as the authority and leave the
        // exit code unset; a future attempt-qualified receipt may return
        // ExactExit.
        provider_state.map_or_else(
            || Ok(KrunExecutionTerminalObservation::NotObserved),
            terminal_observation,
        )
    }

    fn capture_process(&self, manifest: &KrunSandboxManifest) -> Result<RuntimeProcessIdentity> {
        capture_runtime_process_identity(
            &manifest.conmon_launch.state_command,
            manifest.handle.id.as_str(),
            current_creator_attempt_id(manifest)?,
            &manifest.conmon_layout.pidfile,
        )
    }

    fn inspect_process(
        &self,
        manifest: &KrunSandboxManifest,
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
        manifest: &KrunSandboxManifest,
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

fn terminal_observation(
    provider_state: RuntimeStateObservation,
) -> Result<KrunExecutionTerminalObservation> {
    match provider_state {
        RuntimeStateObservation::ExplicitlyAbsent => {
            Ok(KrunExecutionTerminalObservation::ExplicitAbsence)
        }
        RuntimeStateObservation::Present(status) if status == "stopped" => {
            Ok(KrunExecutionTerminalObservation::ExactStopped)
        }
        RuntimeStateObservation::Present(_) => Ok(KrunExecutionTerminalObservation::NotObserved),
    }
}

fn current_creator_attempt_id(manifest: &KrunSandboxManifest) -> Result<&str> {
    match &manifest.creator_handoff {
        KrunCreatorHandoffState::RuntimeObserved { receipt }
        | KrunCreatorHandoffState::Pending { receipt } => Ok(receipt.attempt_id()),
        KrunCreatorHandoffState::Quiesced { proof } => Ok(proof.attempt_id()),
        KrunCreatorHandoffState::SpawnIntent { attempt_id } => Ok(attempt_id),
        KrunCreatorHandoffState::NotSpawned => Err(SandboxError::OperationFailed {
            message: format!(
                "Krun {} has no creator attempt that can authenticate a runtime process",
                manifest.handle.id
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_creator_qualified_stopped_state_is_terminal_without_claiming_absence() {
        assert_eq!(
            terminal_observation(RuntimeStateObservation::Present("stopped".to_owned()))
                .expect("stopped state should classify"),
            KrunExecutionTerminalObservation::ExactStopped
        );
        assert_eq!(
            terminal_observation(RuntimeStateObservation::Present("running".to_owned()))
                .expect("running state should classify"),
            KrunExecutionTerminalObservation::NotObserved
        );
        assert_eq!(
            terminal_observation(RuntimeStateObservation::ExplicitlyAbsent)
                .expect("absence should classify"),
            KrunExecutionTerminalObservation::ExplicitAbsence
        );
    }
}
