//! Provider-local activation identity retained beside one physical effect.

use nimbus_core::Result;
use nimbus_workloads::{WorkloadExecutionReference, WorkloadProvisionDispatchClaim};
use serde::Serialize;

use super::restart::HostRestartActivationFence;
#[cfg(any(test, all(target_os = "linux", feature = "systemd-dbus")))]
use super::restart::is_restart_fence;
use super::{HostLifecyclePlan, HostProvisionActivationFence};

/// Closed activation-fence family for initial provision and later attempts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) enum HostActivationFence {
    Provision(HostProvisionActivationFence),
    Restart(HostRestartActivationFence),
}

impl HostActivationFence {
    pub(super) fn from_claim(
        plan: &HostLifecyclePlan,
        claim: &WorkloadProvisionDispatchClaim,
    ) -> Result<Self> {
        HostProvisionActivationFence::from_claim(plan, claim).map(Self::Provision)
    }

    pub(super) fn from_execution(
        execution: &WorkloadExecutionReference,
        claim: &WorkloadProvisionDispatchClaim,
    ) -> Result<Self> {
        HostProvisionActivationFence::from_execution(execution, claim).map(Self::Provision)
    }

    pub(crate) fn journal_fields(&self) -> Vec<String> {
        match self {
            Self::Provision(fence) => fence.journal_fields(),
            Self::Restart(fence) => fence.journal_fields(),
        }
    }

    #[cfg(any(test, all(target_os = "linux", feature = "systemd-dbus")))]
    pub(crate) fn from_log_extra_fields(fields: &[Vec<u8>]) -> Result<Option<Self>> {
        if is_restart_fence(fields) {
            return HostRestartActivationFence::from_log_extra_fields(fields)
                .map(|fence| fence.map(Self::Restart));
        }
        HostProvisionActivationFence::from_log_extra_fields(fields)
            .map(|fence| fence.map(Self::Provision))
    }

    #[cfg(test)]
    pub(super) fn for_test(plan: &HostLifecyclePlan, seed: u8, dispatch_epoch: u64) -> Self {
        Self::Provision(HostProvisionActivationFence::for_test(
            plan,
            seed,
            dispatch_epoch,
        ))
    }
}
