//! Exact process-local drain and stop state.

use nimbus_workloads::{
    WorkloadFailureEvidence, WorkloadProvisionSourceEvidence, WorkloadTeardownClaim,
    WorkloadTeardownProviderTarget, WorkloadTeardownStep, WorkloadTeardownSuccessEvidence,
};

use super::{DirectProcessBackend, HostBackendObservedState, HostLifecycleStatus};
use crate::host_lifecycle::{
    HostExecutionDrainProvider, HostExecutionStopProvider, HostTeardownExecuteClaim,
    HostTeardownExecuteObservation, HostTeardownFuture, HostTeardownInspectClaim,
    HostTeardownInspectObservation, HostTeardownOperationFence,
};

#[derive(Debug, Default, Clone)]
pub(super) struct DirectProcessTeardownState {
    drain: Option<DirectProcessTeardownOperation>,
    stop: Option<DirectProcessTeardownOperation>,
}

#[derive(Debug, Clone)]
struct DirectProcessTeardownOperation {
    fence: HostTeardownOperationFence,
    evidence: WorkloadTeardownSuccessEvidence,
}

impl DirectProcessTeardownOperation {
    fn for_execute(
        claim: &HostTeardownExecuteClaim,
        evidence: WorkloadTeardownSuccessEvidence,
    ) -> Self {
        Self {
            fence: claim.operation_fence(),
            evidence,
        }
    }

    fn matches_execute(&self, claim: &HostTeardownExecuteClaim) -> bool {
        self.fence.matches_execute(claim)
    }

    fn bind_or_matches_inspect(&mut self, claim: &HostTeardownInspectClaim) -> bool {
        self.fence.bind_or_matches_inspect(claim)
    }
}

impl HostExecutionDrainProvider for DirectProcessBackend {
    fn execute_drain<'a>(
        &'a self,
        claim: HostTeardownExecuteClaim,
    ) -> HostTeardownFuture<'a, HostTeardownExecuteObservation> {
        Box::pin(async move {
            if claim
                .require_step(WorkloadTeardownStep::DrainExecution)
                .is_err()
            {
                return execute_failure(&claim, "crossed_drain_step");
            }
            let mut state = self
                .state
                .lock()
                .expect("direct process backend lock should not be poisoned");
            let Some(record) = state.records.get_mut(claim.execution().execution_id()) else {
                return HostTeardownExecuteObservation::Ambiguous;
            };
            if authenticate_record(record, &claim).is_err() {
                return execute_failure(&claim, "crossed_activation_fence");
            }
            if let Some(operation) = &record.teardown.drain {
                return if operation.matches_execute(&claim) {
                    HostTeardownExecuteObservation::Succeeded(Box::new(operation.evidence.clone()))
                } else {
                    execute_failure(&claim, "crossed_drain_operation")
                };
            }
            let evidence = WorkloadTeardownSuccessEvidence::ExecutionDrained {
                reference: claim.execution().clone(),
                evidence: claim.canonical_evidence("nimbus.node.direct-process.drain.v1"),
            };
            record.teardown.drain = Some(DirectProcessTeardownOperation::for_execute(
                &claim,
                evidence.clone(),
            ));
            record.logs.push(format!(
                "direct-process:{}:drained",
                claim.execution().execution_id().as_str()
            ));
            HostTeardownExecuteObservation::Succeeded(Box::new(evidence))
        })
    }

    fn inspect_drain<'a>(
        &'a self,
        claim: HostTeardownInspectClaim,
    ) -> HostTeardownFuture<'a, HostTeardownInspectObservation> {
        Box::pin(async move {
            if claim
                .require_step(WorkloadTeardownStep::DrainExecution)
                .is_err()
            {
                return inspect_failure(&claim, "crossed_drain_step");
            }
            let mut state = self
                .state
                .lock()
                .expect("direct process backend lock should not be poisoned");
            let Some(record) = state.records.get_mut(claim.execution().execution_id()) else {
                return HostTeardownInspectObservation::Ambiguous;
            };
            if authenticate_record(record, &claim).is_err() {
                return inspect_failure(&claim, "crossed_activation_fence");
            }
            match &mut record.teardown.drain {
                Some(operation) => {
                    if operation.bind_or_matches_inspect(&claim) {
                        HostTeardownInspectObservation::Satisfied(Box::new(
                            operation.evidence.clone(),
                        ))
                    } else {
                        inspect_failure(&claim, "crossed_drain_operation")
                    }
                }
                None if record.status.reason() == super::HostLifecycleStatusReason::Stopped => {
                    HostTeardownInspectObservation::Satisfied(Box::new(
                        WorkloadTeardownSuccessEvidence::ExecutionDrained {
                            reference: claim.execution().clone(),
                            evidence: claim
                                .canonical_evidence("nimbus.node.direct-process.drain.absent.v1"),
                        },
                    ))
                }
                None => HostTeardownInspectObservation::NotCompleted(
                    claim.canonical_evidence("nimbus.node.direct-process.drain.not-completed.v1"),
                ),
            }
        })
    }
}

impl HostExecutionStopProvider for DirectProcessBackend {
    fn execute_stop<'a>(
        &'a self,
        claim: HostTeardownExecuteClaim,
    ) -> HostTeardownFuture<'a, HostTeardownExecuteObservation> {
        Box::pin(async move {
            if claim
                .require_step(WorkloadTeardownStep::StopExecution)
                .is_err()
            {
                return execute_failure(&claim, "crossed_stop_step");
            }
            let mut state = self
                .state
                .lock()
                .expect("direct process backend lock should not be poisoned");
            let Some(record) = state.records.get_mut(claim.execution().execution_id()) else {
                return HostTeardownExecuteObservation::Ambiguous;
            };
            if authenticate_record(record, &claim).is_err() {
                return execute_failure(&claim, "crossed_activation_fence");
            }
            if let Some(operation) = &record.teardown.stop {
                return if operation.matches_execute(&claim) {
                    HostTeardownExecuteObservation::Succeeded(Box::new(operation.evidence.clone()))
                } else {
                    execute_failure(&claim, "crossed_stop_operation")
                };
            }
            let evidence = WorkloadTeardownSuccessEvidence::ExecutionStopped {
                reference: claim.execution().clone(),
                evidence: claim.canonical_evidence("nimbus.node.direct-process.stop.v1"),
            };
            if record.status.reason() != super::HostLifecycleStatusReason::Stopped {
                record.status = HostLifecycleStatus::from_provider_state(
                    &record.plan,
                    HostBackendObservedState::Stopped,
                );
                record.logs.push(format!(
                    "direct-process:{}:stopped:{}",
                    claim.execution().execution_id().as_str(),
                    record.process_id
                ));
            }
            record.teardown.stop = Some(DirectProcessTeardownOperation::for_execute(
                &claim,
                evidence.clone(),
            ));
            HostTeardownExecuteObservation::Succeeded(Box::new(evidence))
        })
    }

    fn inspect_stop<'a>(
        &'a self,
        claim: HostTeardownInspectClaim,
    ) -> HostTeardownFuture<'a, HostTeardownInspectObservation> {
        Box::pin(async move {
            if claim
                .require_step(WorkloadTeardownStep::StopExecution)
                .is_err()
            {
                return inspect_failure(&claim, "crossed_stop_step");
            }
            let mut state = self
                .state
                .lock()
                .expect("direct process backend lock should not be poisoned");
            let Some(record) = state.records.get_mut(claim.execution().execution_id()) else {
                return HostTeardownInspectObservation::Ambiguous;
            };
            if authenticate_record(record, &claim).is_err() {
                return inspect_failure(&claim, "crossed_activation_fence");
            }
            match &mut record.teardown.stop {
                Some(operation) => {
                    if operation.bind_or_matches_inspect(&claim) {
                        HostTeardownInspectObservation::Satisfied(Box::new(
                            operation.evidence.clone(),
                        ))
                    } else {
                        inspect_failure(&claim, "crossed_stop_operation")
                    }
                }
                None if record.status.reason() == super::HostLifecycleStatusReason::Stopped => {
                    HostTeardownInspectObservation::Satisfied(Box::new(
                        WorkloadTeardownSuccessEvidence::ExecutionStopped {
                            reference: claim.execution().clone(),
                            evidence: claim
                                .canonical_evidence("nimbus.node.direct-process.stop.absent.v1"),
                        },
                    ))
                }
                None => HostTeardownInspectObservation::NotCompleted(
                    claim.canonical_evidence("nimbus.node.direct-process.stop.not-completed.v1"),
                ),
            }
        })
    }
}

fn authenticate_record(
    record: &super::DirectProcessRecord,
    claim: &impl ExactTeardownClaim,
) -> nimbus_core::Result<()> {
    let fence = record.plan.activation_fence().ok_or_else(|| {
        nimbus_core::Error::PermissionDenied(
            "direct process teardown requires a retained exact activation fence".to_owned(),
        )
    })?;
    fence.authenticate_teardown_execution(
        claim.execution(),
        claim.source(),
        claim.provider_target(),
        claim.portable_claim(),
    )
}

trait ExactTeardownClaim {
    fn portable_claim(&self) -> &WorkloadTeardownClaim;
    fn source(&self) -> &WorkloadProvisionSourceEvidence;
    fn execution(&self) -> &nimbus_workloads::WorkloadExecutionReference;
    fn provider_target(&self) -> &WorkloadTeardownProviderTarget;
}

macro_rules! impl_exact_claim {
    ($claim:ty) => {
        impl ExactTeardownClaim for $claim {
            fn portable_claim(&self) -> &WorkloadTeardownClaim {
                self.portable_claim()
            }
            fn source(&self) -> &WorkloadProvisionSourceEvidence {
                self.source()
            }
            fn execution(&self) -> &nimbus_workloads::WorkloadExecutionReference {
                self.execution()
            }
            fn provider_target(&self) -> &WorkloadTeardownProviderTarget {
                self.provider_target()
            }
        }
    };
}

impl_exact_claim!(HostTeardownExecuteClaim);
impl_exact_claim!(HostTeardownInspectClaim);

fn execute_failure(
    claim: &HostTeardownExecuteClaim,
    code: &'static str,
) -> HostTeardownExecuteObservation {
    HostTeardownExecuteObservation::DefiniteFailure(failure(
        code,
        claim.canonical_evidence("nimbus.node.direct-process.failure.v1"),
    ))
}

fn inspect_failure(
    claim: &HostTeardownInspectClaim,
    code: &'static str,
) -> HostTeardownInspectObservation {
    HostTeardownInspectObservation::DefiniteFailure(failure(
        code,
        claim.canonical_evidence("nimbus.node.direct-process.failure.v1"),
    ))
}

fn failure(
    code: &'static str,
    evidence: nimbus_workloads::WorkloadOwnerEvidenceDigest,
) -> WorkloadFailureEvidence {
    WorkloadFailureEvidence::new(code, evidence)
        .expect("static direct-process teardown failure code should validate")
}
