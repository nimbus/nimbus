//! Compute projection of the portable workload teardown reducer.

use nimbus_workloads::{
    ProposedWorkloadTeardownTransition, WorkloadSagaRecord, WorkloadSagaStoreError,
};

/// Materialize one workloads-owned teardown proposal without reconstructing
/// its phase or effect rules in compute.
pub(super) fn materialize_teardown_candidate(
    record: &WorkloadSagaRecord,
    proposed: &ProposedWorkloadTeardownTransition,
) -> Result<WorkloadSagaRecord, WorkloadSagaStoreError> {
    match proposed {
        ProposedWorkloadTeardownTransition::Claim {
            attempt,
            provider_target,
        } => Ok(record.claim_teardown((**attempt).clone(), provider_target.clone())?),
        ProposedWorkloadTeardownTransition::ResourceFree { step, .. } => {
            Ok(record.record_resource_free_teardown_step(*step)?)
        }
        ProposedWorkloadTeardownTransition::RecordTerminal => {
            Ok(record.record_terminal_teardown()?)
        }
    }
}

#[cfg(test)]
#[path = "teardown_decision/tests.rs"]
mod tests;
