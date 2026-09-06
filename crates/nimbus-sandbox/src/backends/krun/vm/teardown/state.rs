//! Durable Krun execution-teardown progress.

use serde::{Deserialize, Serialize};

use crate::ProviderCommandClaim;
use crate::backends::conmon::runtime_process::RuntimeProcessIdentity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::backends::krun::vm) enum KrunNetworkStopRequirementError {
    NotStopped,
    Crossed,
}

/// Independent execution drain and stop progress retained until network release.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(in crate::backends::krun::vm) struct KrunExecutionTeardownState {
    drain: KrunDrainProgress,
    stop: KrunStopProgress,
}

impl KrunExecutionTeardownState {
    pub(in crate::backends::krun::vm) fn drain(&self) -> &KrunDrainProgress {
        &self.drain
    }

    pub(in crate::backends::krun::vm) fn stop(&self) -> &KrunStopProgress {
        &self.stop
    }

    pub(in crate::backends::krun::vm) fn set_drain(&mut self, progress: KrunDrainProgress) {
        self.drain = progress;
    }

    pub(in crate::backends::krun::vm) fn set_stop(&mut self, progress: KrunStopProgress) {
        self.stop = progress;
    }

    pub(in crate::backends::krun::vm) fn admission_is_open(&self) -> bool {
        matches!(self.drain, KrunDrainProgress::Open)
    }

    pub(in crate::backends::krun::vm) fn require_stopped_for_network(
        &self,
        network_claim: &ProviderCommandClaim,
    ) -> Result<&[u8], KrunNetworkStopRequirementError> {
        let KrunStopProgress::ExecutionStopped { fence, evidence } = &self.stop else {
            return Err(KrunNetworkStopRequirementError::NotStopped);
        };
        if fence.same_lifecycle_fence(network_claim) {
            Ok(evidence)
        } else {
            Err(KrunNetworkStopRequirementError::Crossed)
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "phase", deny_unknown_fields)]
pub(in crate::backends::krun::vm) enum KrunDrainProgress {
    #[default]
    Open,
    BarrierPersisted {
        fence: ProviderCommandClaim,
    },
    Drained {
        fence: ProviderCommandClaim,
        evidence: Vec<u8>,
    },
    /// A pre-activation stop proved under the lifecycle lock that no creator
    /// was admitted. The stop claim closes later execution admission without
    /// fabricating a `DrainExecution` command that the compensation plan did
    /// not issue.
    ExecutionNeverAdmitted {
        fence: ProviderCommandClaim,
        evidence: Vec<u8>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "phase", deny_unknown_fields)]
pub(in crate::backends::krun::vm) enum KrunStopProgress {
    #[default]
    NotRequested,
    IntentPersisted {
        fence: ProviderCommandClaim,
    },
    GracefulSignalMayExist {
        fence: ProviderCommandClaim,
        process: RuntimeProcessIdentity,
        graceful_signal: String,
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
