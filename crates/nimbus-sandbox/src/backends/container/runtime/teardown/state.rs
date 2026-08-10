//! Durable Container execution-teardown progress.

use serde::{Deserialize, Serialize};

use crate::ProviderCommandClaim;
use crate::backends::conmon::runtime_process::RuntimeProcessIdentity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::backends::container::runtime) enum ContainerNetworkStopRequirementError {
    NotStopped,
    Crossed,
}

/// Independent drain and stop progress retained until network release.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(in crate::backends::container::runtime) struct ContainerExecutionTeardownState {
    drain: ContainerDrainProgress,
    stop: ContainerStopProgress,
}

impl ContainerExecutionTeardownState {
    pub(in crate::backends::container::runtime) fn drain(&self) -> &ContainerDrainProgress {
        &self.drain
    }

    pub(in crate::backends::container::runtime) fn stop(&self) -> &ContainerStopProgress {
        &self.stop
    }

    pub(in crate::backends::container::runtime) fn set_drain(
        &mut self,
        progress: ContainerDrainProgress,
    ) {
        self.drain = progress;
    }

    pub(in crate::backends::container::runtime) fn set_stop(
        &mut self,
        progress: ContainerStopProgress,
    ) {
        self.stop = progress;
    }

    pub(in crate::backends::container::runtime) fn admission_is_open(&self) -> bool {
        matches!(self.drain, ContainerDrainProgress::Open)
    }

    pub(in crate::backends::container::runtime) fn require_stopped_for_network(
        &self,
        network_claim: &ProviderCommandClaim,
    ) -> Result<&[u8], ContainerNetworkStopRequirementError> {
        let ContainerStopProgress::ExecutionStopped { fence, evidence } = &self.stop else {
            return Err(ContainerNetworkStopRequirementError::NotStopped);
        };
        if fence.same_lifecycle_fence(network_claim) {
            Ok(evidence)
        } else {
            Err(ContainerNetworkStopRequirementError::Crossed)
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "phase", deny_unknown_fields)]
pub(in crate::backends::container::runtime) enum ContainerDrainProgress {
    #[default]
    Open,
    BarrierPersisted {
        fence: ProviderCommandClaim,
    },
    Drained {
        fence: ProviderCommandClaim,
        evidence: Vec<u8>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "phase", deny_unknown_fields)]
pub(in crate::backends::container::runtime) enum ContainerStopProgress {
    #[default]
    NotRequested,
    IntentPersisted {
        fence: ProviderCommandClaim,
    },
    TermMayExist {
        fence: ProviderCommandClaim,
        process: RuntimeProcessIdentity,
        grace_deadline_unix_millis: u64,
    },
    KillMayExist {
        fence: ProviderCommandClaim,
        process: RuntimeProcessIdentity,
        redelivery_not_before_unix_millis: u64,
    },
    ExecutionStopped {
        fence: ProviderCommandClaim,
        evidence: Vec<u8>,
    },
}
