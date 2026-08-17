//! Exact systemd drain and stop state with submission ambiguity fencing.

use std::collections::BTreeMap;

use nimbus_workloads::{
    WorkloadExecutionAttemptId, WorkloadExecutionId, WorkloadFailureEvidence,
    WorkloadOwnerEvidenceDigest, WorkloadProvisionSourceEvidence, WorkloadTeardownAttemptId,
    WorkloadTeardownClaim, WorkloadTeardownProviderTarget, WorkloadTeardownStep,
    WorkloadTeardownSuccessEvidence,
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
    activation: BTreeMap<WorkloadExecutionId, SystemdActivationAdmission>,
    drain: BTreeMap<WorkloadExecutionId, SystemdDrainOperation>,
    stop: BTreeMap<SystemdTeardownOperationKey, SystemdStopOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SystemdActivationAdmission {
    request_digest: WorkloadOwnerEvidenceDigest,
    stage: SystemdActivationAdmissionStage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SystemdActivationAdmissionStage {
    Submitting,
    Settled,
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

impl SystemdTeardownState {
    #[cfg(test)]
    pub(super) fn closed_drain_count(&self) -> usize {
        self.drain.len()
    }

    pub(super) fn begin_activation(
        &mut self,
        execution_id: &WorkloadExecutionId,
        request_digest: WorkloadOwnerEvidenceDigest,
    ) -> nimbus_core::Result<()> {
        if self.drain.contains_key(execution_id) {
            return Err(nimbus_core::Error::PermissionDenied(
                "systemd activation admission is closed by the durable execution drain barrier"
                    .to_owned(),
            ));
        }
        match self.activation.get_mut(execution_id) {
            Some(admission)
                if admission.stage == SystemdActivationAdmissionStage::Submitting
                    && admission.request_digest != request_digest =>
            {
                return Err(nimbus_core::Error::AlreadyExists(
                    "a different exact systemd activation submission is unresolved".to_owned(),
                ));
            }
            Some(admission) => {
                admission.request_digest = request_digest;
                admission.stage = SystemdActivationAdmissionStage::Submitting;
            }
            None => {
                self.activation.insert(
                    execution_id.clone(),
                    SystemdActivationAdmission {
                        request_digest,
                        stage: SystemdActivationAdmissionStage::Submitting,
                    },
                );
            }
        }
        Ok(())
    }

    pub(super) fn settle_activation(
        &mut self,
        execution_id: &WorkloadExecutionId,
        request_digest: WorkloadOwnerEvidenceDigest,
    ) -> nimbus_core::Result<()> {
        let admission = self.activation.get_mut(execution_id).ok_or_else(|| {
            nimbus_core::Error::InvalidInput(
                "systemd activation settlement has no durable admission".to_owned(),
            )
        })?;
        if admission.request_digest != request_digest {
            return Err(nimbus_core::Error::PermissionDenied(
                "systemd activation settlement is crossed with its durable admission".to_owned(),
            ));
        }
        admission.stage = SystemdActivationAdmissionStage::Settled;
        Ok(())
    }

    fn activation_is_unresolved(&self, execution_id: &WorkloadExecutionId) -> bool {
        self.activation
            .get(execution_id)
            .is_some_and(|admission| admission.stage == SystemdActivationAdmissionStage::Submitting)
    }

    fn settle_observed_activation(&mut self, execution_id: &WorkloadExecutionId) {
        if let Some(admission) = self.activation.get_mut(execution_id) {
            admission.stage = SystemdActivationAdmissionStage::Settled;
        }
    }
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
            let mut state = match store.lock_state() {
                Ok(state) => state,
                Err(_) => return HostTeardownExecuteObservation::Ambiguous,
            };
            let execution_id = claim.execution().execution_id();
            if let Some(operation) = state.state().drain.get(execution_id) {
                let evidence = operation.evidence.clone();
                if operation.fence.matches_execute(&claim) {
                    return HostTeardownExecuteObservation::Succeeded(Box::new(evidence));
                }
                let adopted =
                    state
                        .state_mut()
                        .drain
                        .get_mut(execution_id)
                        .is_some_and(|operation| {
                            operation.fence.advance_after_parent_inspection(&claim)
                        });
                if !adopted {
                    return execute_failure(&claim, "crossed_drain_barrier");
                }
                if state.checkpoint().is_err() {
                    return HostTeardownExecuteObservation::Ambiguous;
                }
                return HostTeardownExecuteObservation::Succeeded(Box::new(evidence));
            }
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
            if state.state().activation_is_unresolved(execution_id) {
                if observed.is_absent() {
                    return HostTeardownExecuteObservation::Ambiguous;
                }
                state.state_mut().settle_observed_activation(execution_id);
            }
            let evidence = WorkloadTeardownSuccessEvidence::ExecutionDrained {
                reference: claim.execution().clone(),
                evidence: claim.canonical_evidence("nimbus.node.systemd.drain.v2"),
            };
            state.state_mut().drain.insert(
                execution_id.clone(),
                SystemdDrainOperation {
                    fence: claim.operation_fence(),
                    evidence: evidence.clone(),
                },
            );
            if state.checkpoint().is_err() {
                return HostTeardownExecuteObservation::Ambiguous;
            }
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
            if self.ensure_capable().is_err() {
                return HostTeardownInspectObservation::Ambiguous;
            }
            let store = match self.teardown_store() {
                Ok(store) => store,
                Err(_) => return inspect_failure(&claim, "systemd_teardown_store_unavailable"),
            };
            let state = match store.lock_state() {
                Ok(state) => state,
                Err(_) => return HostTeardownInspectObservation::Ambiguous,
            };
            let observed = match inspect_unit(self, claim.execution().execution_id()).await {
                Ok(observed) => observed,
                Err(()) => return HostTeardownInspectObservation::Ambiguous,
            };
            if authenticate_observation(&observed, &claim).is_err() {
                return inspect_failure(&claim, "crossed_activation_fence");
            }
            let execution_id = claim.execution().execution_id();
            if let Some(operation) = state.state().drain.get(execution_id) {
                return if operation.fence.matches_inspect(&claim) {
                    HostTeardownInspectObservation::Satisfied(Box::new(operation.evidence.clone()))
                } else {
                    inspect_failure(&claim, "crossed_drain_barrier")
                };
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
            if state.state().activation_is_unresolved(execution_id) {
                return HostTeardownInspectObservation::InProgress(
                    claim.canonical_evidence("nimbus.node.systemd.drain.activation-unresolved.v1"),
                );
            }
            HostTeardownInspectObservation::NotCompleted(
                claim.canonical_evidence("nimbus.node.systemd.drain.not-completed.v2"),
            )
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
            let drain_barrier = match store.lock_state() {
                Ok(state) => state,
                Err(_) => return HostTeardownExecuteObservation::Ambiguous,
            };
            if !closed_drain_authenticates(drain_barrier.state(), &claim) {
                return execute_failure(&claim, "systemd_drain_barrier_required");
            }
            drop(drain_barrier);
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
                            if operation.fence.matches_execute(&claim)
                                || operation.fence.advance_after_parent_inspection(&claim)
                            {
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
                            } else if operation.fence.advance_after_parent_inspection(&claim) {
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
                Err(_) => return HostTeardownExecuteObservation::Ambiguous,
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
            let state = match store.lock_state() {
                Ok(state) => state,
                Err(_) => return HostTeardownInspectObservation::Ambiguous,
            };
            if !closed_drain_authenticates(state.state(), &claim) {
                return inspect_failure(&claim, "systemd_drain_barrier_required");
            }
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
                if let Some(operation) = state.state().stop.get(&key) {
                    if !operation.fence.matches_inspect(&claim) {
                        return inspect_failure(&claim, "crossed_stop_operation");
                    }
                    if matches!(
                        &operation.stage,
                        SystemdStopStage::AcceptedJob { job_path, .. } if job_path != job.path()
                    ) {
                        return HostTeardownInspectObservation::Ambiguous;
                    }
                }
                return HostTeardownInspectObservation::InProgress(
                    claim.canonical_evidence("nimbus.node.systemd.stop.job-in-progress.v1"),
                );
            }
            let key = SystemdTeardownOperationKey::for_claim(&claim);
            let Some(operation) = state.state().stop.get(&key) else {
                return HostTeardownInspectObservation::Ambiguous;
            };
            if !operation.fence.matches_inspect(&claim) {
                return inspect_failure(&claim, "crossed_stop_operation");
            }
            match &operation.stage {
                SystemdStopStage::Terminal { evidence } => {
                    HostTeardownInspectObservation::Satisfied(evidence.clone())
                }
                SystemdStopStage::TerminalFailure { evidence } => {
                    HostTeardownInspectObservation::DefiniteFailure(evidence.clone())
                }
                SystemdStopStage::Submitting
                | SystemdStopStage::PreCallFailure { .. }
                | SystemdStopStage::UnknownSubmission { .. }
                | SystemdStopStage::AcceptedJob { .. }
                    if terminal(&observed) =>
                {
                    HostTeardownInspectObservation::Satisfied(Box::new(
                        WorkloadTeardownSuccessEvidence::ExecutionStopped {
                            reference: claim.execution().clone(),
                            evidence: claim
                                .canonical_evidence("nimbus.node.systemd.stop.absent.v1"),
                        },
                    ))
                }
                SystemdStopStage::PreCallFailure { .. } => {
                    HostTeardownInspectObservation::NotCompleted(
                        claim.canonical_evidence("nimbus.node.systemd.stop.pre-call.v1"),
                    )
                }
                SystemdStopStage::Submitting
                | SystemdStopStage::UnknownSubmission { .. }
                | SystemdStopStage::AcceptedJob { .. } => HostTeardownInspectObservation::Ambiguous,
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

fn closed_drain_authenticates(
    state: &SystemdTeardownState,
    claim: &impl ExactTeardownClaim,
) -> bool {
    let Some(receipt) = claim
        .prior_receipt_prefix()
        .receipt_for(WorkloadTeardownStep::DrainExecution)
    else {
        return false;
    };
    state
        .drain
        .get(claim.execution().execution_id())
        .is_some_and(|operation| operation.fence.matches_prior_receipt(receipt))
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
                        && operation.fence.advance_after_parent_inspection(claim))
                    || (matches!(&operation.stage, SystemdStopStage::Terminal { .. })
                        && operation.fence.advance_after_parent_inspection(claim));
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

trait ExactTeardownClaim {
    fn portable_claim(&self) -> &WorkloadTeardownClaim;
    fn source(&self) -> &WorkloadProvisionSourceEvidence;
    fn execution(&self) -> &nimbus_workloads::WorkloadExecutionReference;
    fn provider_target(&self) -> &WorkloadTeardownProviderTarget;
    fn prior_receipt_prefix(&self) -> &nimbus_workloads::WorkloadTeardownReceiptPrefix;
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
            fn prior_receipt_prefix(&self) -> &nimbus_workloads::WorkloadTeardownReceiptPrefix {
                self.prior_receipt_prefix()
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
