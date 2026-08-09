//! Exact systemd drain and stop state with submission ambiguity fencing.

use std::collections::BTreeMap;

use nimbus_workloads::{
    WorkloadExecutionAttemptId, WorkloadExecutionId, WorkloadFailureEvidence,
    WorkloadProvisionSourceEvidence, WorkloadTeardownAttemptId, WorkloadTeardownClaim,
    WorkloadTeardownProviderTarget, WorkloadTeardownStep, WorkloadTeardownSuccessEvidence,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::teardown_store::SystemdTeardownStore;
use super::{
    SystemdDbusClient, SystemdInspectUnitRequest, SystemdStopUnitRequest,
    SystemdStopUnitSubmission, SystemdTransientUnitBackend, SystemdUnitStatus,
};
use crate::host_lifecycle::{
    HostExecutionDrainProvider, HostExecutionStopProvider, HostTeardownExecuteClaim,
    HostTeardownExecuteObservation, HostTeardownFuture, HostTeardownInspectClaim,
    HostTeardownInspectObservation, HostTeardownOperationFence,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SystemdTeardownState {
    drain: BTreeMap<SystemdTeardownOperationKey, SystemdDrainOperation>,
    stop: BTreeMap<SystemdTeardownOperationKey, SystemdStopOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct SystemdTeardownOperationKey {
    execution_id: WorkloadExecutionId,
    execution_attempt_id: WorkloadExecutionAttemptId,
    teardown_attempt_id: WorkloadTeardownAttemptId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SystemdDrainOperation {
    fence: HostTeardownOperationFence,
    evidence: WorkloadTeardownSuccessEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SystemdStopOperation {
    fence: HostTeardownOperationFence,
    stage: SystemdStopStage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum SystemdStopStage {
    Submitting,
    PreCallFailure {
        _error: String,
    },
    UnknownSubmission {
        _error: String,
    },
    AcceptedJob {
        job_path: String,
        _wait_error: String,
    },
    Terminal {
        evidence: Box<WorkloadTeardownSuccessEvidence>,
    },
    TerminalFailure {
        evidence: WorkloadFailureEvidence,
    },
}

impl SystemdTeardownOperationKey {
    #[cfg(test)]
    pub(super) fn from_parts(
        execution_id: WorkloadExecutionId,
        execution_attempt_id: WorkloadExecutionAttemptId,
        teardown_attempt_id: WorkloadTeardownAttemptId,
    ) -> Self {
        Self {
            execution_id,
            execution_attempt_id,
            teardown_attempt_id,
        }
    }

    fn for_claim(claim: &impl ExactTeardownClaim) -> Self {
        Self {
            execution_id: claim.execution().execution_id().clone(),
            execution_attempt_id: claim.execution().attempt_id().clone(),
            teardown_attempt_id: claim.portable_claim().attempt().attempt_id().clone(),
        }
    }

    fn encode(&self) -> String {
        format!(
            "v1/{}/{}/{}",
            self.execution_id.as_str(),
            self.execution_attempt_id.as_str(),
            self.teardown_attempt_id.as_str()
        )
    }

    fn decode(value: &str) -> nimbus_core::Result<Self> {
        let mut parts = value.split('/');
        if parts.next() != Some("v1") {
            return Err(nimbus_core::Error::InvalidInput(
                "unsupported systemd teardown operation key version".to_owned(),
            ));
        }
        let execution_id = parts
            .next()
            .ok_or_else(|| invalid_operation_key("missing execution ID"))?
            .to_owned()
            .try_into()
            .map_err(|_| invalid_operation_key("invalid execution ID"))?;
        let execution_attempt_id = parts
            .next()
            .ok_or_else(|| invalid_operation_key("missing execution attempt ID"))?
            .to_owned()
            .try_into()
            .map_err(|_| invalid_operation_key("invalid execution attempt ID"))?;
        let teardown_attempt_id = parts
            .next()
            .ok_or_else(|| invalid_operation_key("missing teardown attempt ID"))?
            .to_owned()
            .try_into()
            .map_err(|_| invalid_operation_key("invalid teardown attempt ID"))?;
        if parts.next().is_some() {
            return Err(invalid_operation_key("unexpected trailing component"));
        }
        Ok(Self {
            execution_id,
            execution_attempt_id,
            teardown_attempt_id,
        })
    }
}

impl Serialize for SystemdTeardownOperationKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.encode())
    }
}

impl<'de> Deserialize<'de> for SystemdTeardownOperationKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::decode(&value).map_err(serde::de::Error::custom)
    }
}

fn invalid_operation_key(reason: &'static str) -> nimbus_core::Error {
    nimbus_core::Error::InvalidInput(format!("invalid systemd teardown operation key: {reason}"))
}

impl<C> HostExecutionDrainProvider for SystemdTransientUnitBackend<C>
where
    C: SystemdDbusClient,
{
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
            if self.ensure_capable().is_err() {
                return execute_failure(&claim, "systemd_unavailable");
            }
            let store = match self.teardown_store() {
                Ok(store) => store,
                Err(_) => return execute_failure(&claim, "systemd_teardown_store_unavailable"),
            };
            let observed = match inspect_unit(self, claim.execution().execution_id()).await {
                Ok(observed) => observed,
                Err(()) => return HostTeardownExecuteObservation::Ambiguous,
            };
            if authenticate_observation(&observed, &claim).is_err() {
                return execute_failure(&claim, "crossed_activation_fence");
            }
            if observed
                .current_job()
                .is_some_and(|job| job.job_type() == "stop")
            {
                return HostTeardownExecuteObservation::Ambiguous;
            }
            if observed.current_job().is_some() {
                return HostTeardownExecuteObservation::Ambiguous;
            }
            let key = SystemdTeardownOperationKey::for_claim(&claim);
            match store.transact(|state| {
                if let Some(operation) = state.drain.get(&key) {
                    return Ok(if operation.fence.matches_execute(&claim) {
                        HostTeardownExecuteObservation::Succeeded(Box::new(
                            operation.evidence.clone(),
                        ))
                    } else {
                        execute_failure(&claim, "crossed_drain_operation")
                    });
                }
                let evidence = WorkloadTeardownSuccessEvidence::ExecutionDrained {
                    reference: claim.execution().clone(),
                    evidence: claim.canonical_evidence("nimbus.node.systemd.drain.v1"),
                };
                state.drain.insert(
                    key,
                    SystemdDrainOperation {
                        fence: claim.operation_fence(),
                        evidence: evidence.clone(),
                    },
                );
                Ok(HostTeardownExecuteObservation::Succeeded(Box::new(
                    evidence,
                )))
            }) {
                Ok(observation) => observation,
                Err(_) => execute_failure(&claim, "systemd_teardown_store_unavailable"),
            }
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
            if self.ensure_capable().is_err() {
                return HostTeardownInspectObservation::Ambiguous;
            }
            let store = match self.teardown_store() {
                Ok(store) => store,
                Err(_) => return inspect_failure(&claim, "systemd_teardown_store_unavailable"),
            };
            let observed = match inspect_unit(self, claim.execution().execution_id()).await {
                Ok(observed) => observed,
                Err(()) => return HostTeardownInspectObservation::Ambiguous,
            };
            if authenticate_observation(&observed, &claim).is_err() {
                return inspect_failure(&claim, "crossed_activation_fence");
            }
            if let Some(job) = observed.current_job() {
                return if job.job_type() == "stop" {
                    HostTeardownInspectObservation::InProgress(
                        claim.canonical_evidence("nimbus.node.systemd.drain.stop-job.v1"),
                    )
                } else {
                    HostTeardownInspectObservation::Ambiguous
                };
            }
            if terminal(&observed) {
                return HostTeardownInspectObservation::Satisfied(Box::new(
                    WorkloadTeardownSuccessEvidence::ExecutionDrained {
                        reference: claim.execution().clone(),
                        evidence: claim.canonical_evidence("nimbus.node.systemd.drain.absent.v1"),
                    },
                ));
            }
            let key = SystemdTeardownOperationKey::for_claim(&claim);
            match store.transact(|state| {
                Ok(match state.drain.get_mut(&key) {
                    Some(operation) => {
                        if operation.fence.bind_or_matches_inspect(&claim) {
                            HostTeardownInspectObservation::Satisfied(Box::new(
                                operation.evidence.clone(),
                            ))
                        } else {
                            inspect_failure(&claim, "crossed_drain_operation")
                        }
                    }
                    None => HostTeardownInspectObservation::NotCompleted(
                        claim.canonical_evidence("nimbus.node.systemd.drain.not-completed.v1"),
                    ),
                })
            }) {
                Ok(observation) => observation,
                Err(_) => inspect_failure(&claim, "systemd_teardown_store_unavailable"),
            }
        })
    }
}

impl<C> HostExecutionStopProvider for SystemdTransientUnitBackend<C>
where
    C: SystemdDbusClient,
{
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
            if self.ensure_capable().is_err() {
                return execute_failure(&claim, "systemd_unavailable");
            }
            let store = match self.teardown_store() {
                Ok(store) => store,
                Err(_) => return execute_failure(&claim, "systemd_teardown_store_unavailable"),
            };
            let initial = match inspect_unit(self, claim.execution().execution_id()).await {
                Ok(observed) => observed,
                Err(()) => return HostTeardownExecuteObservation::Ambiguous,
            };
            if authenticate_observation(&initial, &claim).is_err() {
                return execute_failure(&claim, "crossed_activation_fence");
            }
            if initial.current_job().is_some() {
                return HostTeardownExecuteObservation::Ambiguous;
            }
            if terminal(&initial) {
                return record_terminal_execute(store, &claim);
            }
            let execution_id = claim.execution().execution_id().clone();
            let key = SystemdTeardownOperationKey::for_claim(&claim);
            let existing_observation = match store.transact(|state| {
                if let Some(operation) = state.stop.get_mut(&key) {
                    match &operation.stage {
                        SystemdStopStage::Terminal { evidence } => {
                            if operation.fence.matches_execute(&claim) {
                                Ok(Some(HostTeardownExecuteObservation::Succeeded(
                                    evidence.clone(),
                                )))
                            } else {
                                Ok(Some(execute_failure(&claim, "crossed_stop_operation")))
                            }
                        }
                        SystemdStopStage::TerminalFailure { evidence } => {
                            if operation.fence.matches_execute(&claim) {
                                Ok(Some(HostTeardownExecuteObservation::DefiniteFailure(
                                    evidence.clone(),
                                )))
                            } else {
                                Ok(Some(execute_failure(&claim, "crossed_stop_operation")))
                            }
                        }
                        SystemdStopStage::PreCallFailure { .. } => {
                            if operation.fence.matches_execute(&claim) {
                                Ok(Some(HostTeardownExecuteObservation::Ambiguous))
                            } else if operation.fence.advance_after_not_completed(
                                &claim,
                                "nimbus.node.systemd.stop.pre-call.v1",
                            ) {
                                operation.stage = SystemdStopStage::Submitting;
                                Ok(None)
                            } else {
                                Ok(Some(execute_failure(&claim, "crossed_stop_operation")))
                            }
                        }
                        SystemdStopStage::Submitting
                        | SystemdStopStage::UnknownSubmission { .. }
                        | SystemdStopStage::AcceptedJob { .. } => {
                            if operation.fence.matches_execute(&claim) {
                                Ok(Some(HostTeardownExecuteObservation::Ambiguous))
                            } else {
                                Ok(Some(execute_failure(&claim, "crossed_stop_operation")))
                            }
                        }
                    }
                } else {
                    state.stop.insert(
                        key.clone(),
                        SystemdStopOperation {
                            fence: claim.operation_fence(),
                            stage: SystemdStopStage::Submitting,
                        },
                    );
                    Ok(None)
                }
            }) {
                Ok(observation) => observation,
                Err(_) => {
                    return execute_failure(&claim, "systemd_teardown_store_unavailable");
                }
            };
            if let Some(observation) = existing_observation {
                return observation;
            }
            submit_stop(self, claim, key, execution_id).await
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
            if self.ensure_capable().is_err() {
                return HostTeardownInspectObservation::Ambiguous;
            }
            let store = match self.teardown_store() {
                Ok(store) => store,
                Err(_) => return inspect_failure(&claim, "systemd_teardown_store_unavailable"),
            };
            let observed = match inspect_unit(self, claim.execution().execution_id()).await {
                Ok(observed) => observed,
                Err(()) => return HostTeardownInspectObservation::Ambiguous,
            };
            if authenticate_observation(&observed, &claim).is_err() {
                return inspect_failure(&claim, "crossed_activation_fence");
            }
            if let Some(job) = observed.current_job() {
                if job.job_type() != "stop" {
                    return HostTeardownInspectObservation::Ambiguous;
                }
                let key = SystemdTeardownOperationKey::for_claim(&claim);
                return match store.transact(|state| {
                    if let Some(operation) = state.stop.get_mut(&key) {
                        if !operation.fence.bind_or_matches_inspect(&claim) {
                            return Ok(inspect_failure(&claim, "crossed_stop_operation"));
                        }
                        if matches!(
                            &operation.stage,
                            SystemdStopStage::AcceptedJob { job_path, .. } if job_path != job.path()
                        ) {
                            return Ok(HostTeardownInspectObservation::Ambiguous);
                        }
                    }
                    Ok(HostTeardownInspectObservation::InProgress(
                        claim.canonical_evidence("nimbus.node.systemd.stop.job-in-progress.v1"),
                    ))
                }) {
                    Ok(observation) => observation,
                    Err(_) => HostTeardownInspectObservation::Ambiguous,
                };
            }
            if terminal(&observed) {
                return record_terminal_inspect(store, &claim);
            }
            let key = SystemdTeardownOperationKey::for_claim(&claim);
            match store.transact(|state| {
                let Some(operation) = state.stop.get_mut(&key) else {
                    return Ok(HostTeardownInspectObservation::Ambiguous);
                };
                if !operation.fence.bind_or_matches_inspect(&claim) {
                    return Ok(inspect_failure(&claim, "crossed_stop_operation"));
                }
                Ok(match &operation.stage {
                    SystemdStopStage::PreCallFailure { .. } => {
                        HostTeardownInspectObservation::NotCompleted(
                            claim.canonical_evidence("nimbus.node.systemd.stop.pre-call.v1"),
                        )
                    }
                    SystemdStopStage::Terminal { evidence } => {
                        HostTeardownInspectObservation::Satisfied(evidence.clone())
                    }
                    SystemdStopStage::TerminalFailure { evidence } => {
                        HostTeardownInspectObservation::DefiniteFailure(evidence.clone())
                    }
                    SystemdStopStage::Submitting
                    | SystemdStopStage::UnknownSubmission { .. }
                    | SystemdStopStage::AcceptedJob { .. } => {
                        HostTeardownInspectObservation::Ambiguous
                    }
                })
            }) {
                Ok(observation) => observation,
                Err(_) => HostTeardownInspectObservation::Ambiguous,
            }
        })
    }
}

async fn submit_stop<C>(
    backend: &SystemdTransientUnitBackend<C>,
    claim: HostTeardownExecuteClaim,
    key: SystemdTeardownOperationKey,
    execution_id: WorkloadExecutionId,
) -> HostTeardownExecuteObservation
where
    C: SystemdDbusClient,
{
    let store = backend
        .teardown_store()
        .expect("durable store was required before StopUnit submission");
    let request = match SystemdStopUnitRequest::for_execution(execution_id.clone()) {
        Ok(request) => request,
        Err(_) => {
            if let Ok(Some(observation)) = set_stage(
                store,
                &key,
                SystemdStopStage::PreCallFailure {
                    _error: "failed to derive exact Systemd StopUnit request".to_owned(),
                },
            ) {
                return observation;
            }
            return HostTeardownExecuteObservation::Ambiguous;
        }
    };
    let submission = match backend.client.stop_unit_exact(request).await {
        Ok(submission) => submission,
        Err(_) => SystemdStopUnitSubmission::UnknownSubmission {
            error: "systemd exact stop client returned an unclassified error".to_owned(),
        },
    };
    match submission {
        SystemdStopUnitSubmission::PreCallFailure { error } => {
            match set_stage(
                store,
                &key,
                SystemdStopStage::PreCallFailure { _error: error },
            ) {
                Ok(Some(observation)) => observation,
                Ok(None) | Err(_) => HostTeardownExecuteObservation::Ambiguous,
            }
        }
        SystemdStopUnitSubmission::UnknownSubmission { error } => {
            match set_stage(
                store,
                &key,
                SystemdStopStage::UnknownSubmission { _error: error },
            ) {
                Ok(Some(observation)) => observation,
                Ok(None) => reconcile_after_submission(backend, &claim).await,
                Err(_) => HostTeardownExecuteObservation::Ambiguous,
            }
        }
        SystemdStopUnitSubmission::AcceptedJobIncomplete { job_path, error } => {
            match set_stage(
                store,
                &key,
                SystemdStopStage::AcceptedJob {
                    job_path,
                    _wait_error: error,
                },
            ) {
                Ok(Some(observation)) => observation,
                Ok(None) => reconcile_after_submission(backend, &claim).await,
                Err(_) => HostTeardownExecuteObservation::Ambiguous,
            }
        }
        SystemdStopUnitSubmission::Terminal(response) => {
            let status = response.status();
            if authenticate_observation(status, &claim).is_err()
                || status.current_job().is_some()
                || !terminal(status)
            {
                if let Ok(Some(observation)) = set_stage(
                    store,
                    &key,
                    SystemdStopStage::UnknownSubmission {
                        _error: "Systemd StopUnit terminal response was not exact and terminal"
                            .to_owned(),
                    },
                ) {
                    return observation;
                }
                return HostTeardownExecuteObservation::Ambiguous;
            }
            record_terminal_execute(store, &claim)
        }
        SystemdStopUnitSubmission::TerminalFailure { job_path, result } => {
            let evidence_domain = format!("nimbus.node.systemd.failure.v1\0{job_path}\0{result}");
            let evidence = failure(
                "systemd_stop_job_failed",
                claim.canonical_evidence(&evidence_domain),
            );
            match set_stage(
                store,
                &key,
                SystemdStopStage::TerminalFailure {
                    evidence: evidence.clone(),
                },
            ) {
                Ok(Some(observation)) => observation,
                Ok(None) => HostTeardownExecuteObservation::DefiniteFailure(evidence),
                Err(_) => HostTeardownExecuteObservation::Ambiguous,
            }
        }
    }
}

async fn inspect_unit<C>(
    backend: &SystemdTransientUnitBackend<C>,
    execution_id: &WorkloadExecutionId,
) -> Result<SystemdUnitStatus, ()>
where
    C: SystemdDbusClient,
{
    let request = SystemdInspectUnitRequest::for_execution(execution_id.clone()).map_err(|_| ())?;
    backend.client.inspect_unit(request).await.map_err(|_| ())
}

fn authenticate_observation(
    observed: &SystemdUnitStatus,
    claim: &impl ExactTeardownClaim,
) -> nimbus_core::Result<()> {
    let expected =
        SystemdInspectUnitRequest::for_execution(claim.execution().execution_id().clone())?;
    if observed.execution_id() != claim.execution().execution_id()
        || observed.unit_name() != expected.unit_name()
    {
        return Err(nimbus_core::Error::PermissionDenied(
            "systemd teardown observation is crossed with the exact execution".to_owned(),
        ));
    }
    if observed.is_absent() {
        return Ok(());
    }
    observed
        .activation_fence()
        .ok_or_else(|| {
            nimbus_core::Error::PermissionDenied(
                "systemd teardown observation has no retained activation fence".to_owned(),
            )
        })?
        .authenticate_teardown_execution(
            claim.execution(),
            claim.source(),
            claim.provider_target(),
            claim.portable_claim(),
        )
}

fn terminal(observed: &SystemdUnitStatus) -> bool {
    observed.is_absent() || matches!(observed.active_state(), "inactive" | "failed")
}

/// Persist a submission-stage result without replacing an observation that
/// has already reached a terminal receipt. The terminal stages are absorbing
/// even when inspection races the original StopUnit submitter.
pub(super) fn set_stage(
    store: &SystemdTeardownStore,
    key: &SystemdTeardownOperationKey,
    stage: SystemdStopStage,
) -> nimbus_core::Result<Option<HostTeardownExecuteObservation>> {
    store.transact(|state| {
        let operation = state.stop.get_mut(key).ok_or_else(|| {
            nimbus_core::Error::InvalidInput(
                "systemd teardown stage has no prepared operation".to_owned(),
            )
        })?;
        match &operation.stage {
            SystemdStopStage::Terminal { evidence } => Ok(Some(
                HostTeardownExecuteObservation::Succeeded(evidence.clone()),
            )),
            SystemdStopStage::TerminalFailure { evidence } => Ok(Some(
                HostTeardownExecuteObservation::DefiniteFailure(evidence.clone()),
            )),
            SystemdStopStage::Submitting => {
                operation.stage = stage;
                Ok(None)
            }
            SystemdStopStage::PreCallFailure { .. }
            | SystemdStopStage::UnknownSubmission { .. }
            | SystemdStopStage::AcceptedJob { .. } => Err(nimbus_core::Error::InvalidInput(
                "systemd submission result did not follow the submitting stage".to_owned(),
            )),
        }
    })
}

async fn reconcile_after_submission<C>(
    backend: &SystemdTransientUnitBackend<C>,
    claim: &HostTeardownExecuteClaim,
) -> HostTeardownExecuteObservation
where
    C: SystemdDbusClient,
{
    let Ok(observed) = inspect_unit(backend, claim.execution().execution_id()).await else {
        return HostTeardownExecuteObservation::Ambiguous;
    };
    if authenticate_observation(&observed, claim).is_err() {
        return execute_failure(claim, "crossed_activation_fence");
    }
    if observed.current_job().is_some() {
        return HostTeardownExecuteObservation::Ambiguous;
    }
    if terminal(&observed) {
        record_terminal_execute(
            backend
                .teardown_store()
                .expect("durable store was required before submission reconciliation"),
            claim,
        )
    } else {
        HostTeardownExecuteObservation::Ambiguous
    }
}

fn record_terminal_execute(
    store: &SystemdTeardownStore,
    claim: &HostTeardownExecuteClaim,
) -> HostTeardownExecuteObservation {
    let evidence = WorkloadTeardownSuccessEvidence::ExecutionStopped {
        reference: claim.execution().clone(),
        evidence: claim.canonical_evidence("nimbus.node.systemd.stop.v1"),
    };
    let key = SystemdTeardownOperationKey::for_claim(claim);
    store
        .transact(|state| {
            if let Some(operation) = state.stop.get_mut(&key) {
                let authorized = operation.fence.matches_execute(claim)
                    || (matches!(&operation.stage, SystemdStopStage::PreCallFailure { .. })
                        && operation.fence.advance_after_not_completed(
                            claim,
                            "nimbus.node.systemd.stop.pre-call.v1",
                        ));
                if !authorized {
                    return Ok(execute_failure(claim, "crossed_stop_operation"));
                }
                match &operation.stage {
                    SystemdStopStage::Terminal { evidence } => {
                        return Ok(HostTeardownExecuteObservation::Succeeded(evidence.clone()));
                    }
                    SystemdStopStage::TerminalFailure { evidence } => {
                        return Ok(HostTeardownExecuteObservation::DefiniteFailure(
                            evidence.clone(),
                        ));
                    }
                    SystemdStopStage::Submitting
                    | SystemdStopStage::PreCallFailure { .. }
                    | SystemdStopStage::UnknownSubmission { .. }
                    | SystemdStopStage::AcceptedJob { .. } => {}
                }
            }
            state.stop.insert(
                key,
                SystemdStopOperation {
                    fence: claim.operation_fence(),
                    stage: SystemdStopStage::Terminal {
                        evidence: Box::new(evidence.clone()),
                    },
                },
            );
            Ok(HostTeardownExecuteObservation::Succeeded(Box::new(
                evidence,
            )))
        })
        .unwrap_or(HostTeardownExecuteObservation::Ambiguous)
}

fn record_terminal_inspect(
    store: &SystemdTeardownStore,
    claim: &HostTeardownInspectClaim,
) -> HostTeardownInspectObservation {
    let evidence = WorkloadTeardownSuccessEvidence::ExecutionStopped {
        reference: claim.execution().clone(),
        evidence: claim.canonical_evidence("nimbus.node.systemd.stop.absent.v1"),
    };
    let key = SystemdTeardownOperationKey::for_claim(claim);
    store
        .transact(|state| {
            let Some(operation) = state.stop.get_mut(&key) else {
                return Ok(HostTeardownInspectObservation::Ambiguous);
            };
            if !operation.fence.bind_or_matches_inspect(claim) {
                return Ok(inspect_failure(claim, "crossed_stop_operation"));
            }
            match &operation.stage {
                SystemdStopStage::Terminal { evidence } => {
                    return Ok(HostTeardownInspectObservation::Satisfied(evidence.clone()));
                }
                SystemdStopStage::TerminalFailure { evidence } => {
                    return Ok(HostTeardownInspectObservation::DefiniteFailure(
                        evidence.clone(),
                    ));
                }
                SystemdStopStage::Submitting
                | SystemdStopStage::PreCallFailure { .. }
                | SystemdStopStage::UnknownSubmission { .. }
                | SystemdStopStage::AcceptedJob { .. } => {}
            }
            operation.stage = SystemdStopStage::Terminal {
                evidence: Box::new(evidence.clone()),
            };
            Ok(HostTeardownInspectObservation::Satisfied(Box::new(
                evidence,
            )))
        })
        .unwrap_or(HostTeardownInspectObservation::Ambiguous)
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
        claim.canonical_evidence("nimbus.node.systemd.failure.v1"),
    ))
}

fn inspect_failure(
    claim: &HostTeardownInspectClaim,
    code: &'static str,
) -> HostTeardownInspectObservation {
    HostTeardownInspectObservation::DefiniteFailure(failure(
        code,
        claim.canonical_evidence("nimbus.node.systemd.failure.v1"),
    ))
}

fn failure(
    code: &'static str,
    evidence: nimbus_workloads::WorkloadOwnerEvidenceDigest,
) -> WorkloadFailureEvidence {
    WorkloadFailureEvidence::new(code, evidence)
        .expect("static systemd teardown failure code should validate")
}
