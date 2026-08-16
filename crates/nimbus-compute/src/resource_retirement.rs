//! Native desired-source retirement through the durable workload saga.
//!
//! Services owns source claims and terminal projections. Compute owns the
//! cross-domain order: source/provision fence, stopped-successor persistence,
//! restart settlement, exact five-capability teardown, and finalization.

use std::sync::Arc;
use std::time::Duration;

use nimbus_core::{Error, TenantId, WorkloadId};
use nimbus_network::NetworkResourceGeneration;
use nimbus_services::{
    SandboxResourceSnapshot, ServiceBackend, ServiceDefinition, ServiceDefinitionSource,
    ServiceManager, WorkloadSourceRetirementClaim, WorkloadSourceRetirementOperation,
};
use nimbus_tenant::TenantIsolationContext;
use nimbus_workloads::{
    DesiredWorkloadState, WorkloadActivationIntent, WorkloadExecutionReference, WorkloadGeneration,
    WorkloadNetworkIntent, WorkloadPhaseDetail, WorkloadPublicationIntent, WorkloadRestartPolicy,
    WorkloadSagaIntent, WorkloadSagaKey, WorkloadSagaPhase, WorkloadSagaRecord,
    WorkloadSagaRevision, WorkloadSagaStoreError,
};
use thiserror::Error;

use crate::state::{ComputeError, ComputeState};
use crate::workload_network_plan::WorkloadNetworkPlanCompiler;
use crate::workload_provisioner::{
    WorkloadProvisionCompensationState, WorkloadProvisionError, WorkloadProvisionOutcome,
    WorkloadProvisioner,
};
use crate::workload_saga::restart_runtime::{WorkloadRestartRuntime, WorkloadRestartSettlement};
use crate::workload_saga::{
    WorkloadProvisionRunDisposition, WorkloadSagaCoordinator, WorkloadSagaIngressError,
    WorkloadTeardownCancellationToken, WorkloadTeardownRunDisposition, WorkloadTeardownRuntime,
    WorkloadTeardownSubmissionError,
};

const SERVICE_RETIREMENT_RETRY_DELAY: Duration = Duration::from_millis(25);

/// Exact native service retirement response facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxServiceRetirementOutcome {
    pub definition: ServiceDefinition,
    pub retired_handle: Option<nimbus_sandbox::SandboxHandle>,
    disposition: WorkloadTeardownDisposition,
    terminal_execution: Option<WorkloadExecutionReference>,
}

impl SandboxServiceRetirementOutcome {
    pub const fn disposition(&self) -> WorkloadTeardownDisposition {
        self.disposition
    }

    pub fn terminal_execution_reference(&self) -> Option<&WorkloadExecutionReference> {
        self.terminal_execution.as_ref()
    }
}

/// Terminal durable classification returned to native retirement callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadTeardownDisposition {
    /// The workload saga reached durable `Recorded` truth.
    Recorded,
    /// The source never created a workload saga or execution effect.
    SourceFinalized,
}

/// Failure before native retirement can report truthful terminal state.
#[derive(Debug, Error)]
pub enum ComputeResourceRetirementError {
    #[error("retirement source policy failed: {0}")]
    Source(#[from] Error),
    #[error("workload provision settlement failed: {0}")]
    Provision(Arc<WorkloadProvisionError>),
    #[error("workload provision settlement remains pending")]
    ProvisionSettlementPending,
    #[error("workload saga failed: {0}")]
    Saga(#[from] WorkloadSagaStoreError),
    #[error("workload saga ingress failed: {0}")]
    Ingress(#[from] WorkloadSagaIngressError),
    #[error("workload teardown submission failed: {0}")]
    Teardown(#[from] WorkloadTeardownSubmissionError),
    #[error("workload restart settlement failed: {0}")]
    Restart(String),
    #[error("workload restart settlement remains pending")]
    RestartSettlementPending,
    #[error("workload teardown returned {0:?}; terminal source state was not changed")]
    TeardownPending(WorkloadTeardownRunDisposition),
    #[error("workload lifecycle generation overflow prevents retirement")]
    GenerationOverflow,
    #[error("a provider observation exists without durable workload saga truth")]
    ObservationWithoutSaga,
    #[error("joined workload provision completed without durable workload saga truth")]
    ProvisionWithoutSaga,
    #[error("recorded teardown did not retain the exact stopped successor")]
    InvalidRecordedSuccessor,
}

impl ComputeResourceRetirementError {
    pub fn into_compute_error(self) -> ComputeError {
        match self {
            Self::Source(error) => ComputeError::from(error),
            other => ComputeError::from(Error::Internal(other.to_string())),
        }
    }
}

/// One complete native source owner paired with the sole compute lifecycle
/// authorities. Construction is possible only for exact managed composition.
#[derive(Clone)]
pub struct ComputeResourceRetirer {
    services: Arc<ServiceManager>,
    provisioner: Arc<WorkloadProvisioner>,
    coordinator: Arc<WorkloadSagaCoordinator>,
    restart_runtime: Arc<WorkloadRestartRuntime>,
    teardown_runtime: Arc<WorkloadTeardownRuntime>,
}

impl ComputeResourceRetirer {
    pub(crate) fn new(
        services: Arc<ServiceManager>,
        provisioner: Arc<WorkloadProvisioner>,
        coordinator: Arc<WorkloadSagaCoordinator>,
        restart_runtime: Arc<WorkloadRestartRuntime>,
        teardown_runtime: Arc<WorkloadTeardownRuntime>,
    ) -> Self {
        Self {
            services,
            provisioner,
            coordinator,
            restart_runtime,
            teardown_runtime,
        }
    }

    pub async fn submit_service_teardown(
        &self,
        context: &TenantIsolationContext,
        service_name: &str,
    ) -> Result<SandboxServiceRetirementOutcome, ComputeResourceRetirementError> {
        self.submit_service_teardown_once(
            context,
            service_name,
            &WorkloadTeardownCancellationToken::new(),
        )
        .await
    }

    /// Retire one service and retain foreground ownership while durable
    /// provision, restart, or teardown work reports a safe pending state.
    ///
    /// Every retry reopens exact source and saga truth through the existing
    /// one-shot path. Definite failures and `CleanupPending` remain terminal;
    /// caller cancellation detaches only this waiter and leaves durable work
    /// available for a later exact replay.
    pub async fn submit_service_teardown_until_terminal(
        &self,
        context: &TenantIsolationContext,
        service_name: &str,
        cancellation: &WorkloadTeardownCancellationToken,
    ) -> Result<SandboxServiceRetirementOutcome, ComputeResourceRetirementError> {
        loop {
            if cancellation.is_cancelled() {
                return Err(cancelled_retirement());
            }
            match self
                .submit_service_teardown_once(context, service_name, cancellation)
                .await
            {
                Ok(outcome) => return Ok(outcome),
                Err(error) if foreground_retirement_can_retry(&error) => {}
                Err(error) => return Err(error),
            }
            tokio::select! {
                () = cancellation.cancelled() => return Err(cancelled_retirement()),
                () = tokio::time::sleep(SERVICE_RETIREMENT_RETRY_DELAY) => {}
            }
        }
    }

    async fn submit_service_teardown_once(
        &self,
        context: &TenantIsolationContext,
        service_name: &str,
        cancellation: &WorkloadTeardownCancellationToken,
    ) -> Result<SandboxServiceRetirementOutcome, ComputeResourceRetirementError> {
        let prepared = self
            .services
            .prepare_sandbox_service_provision_source(context.tenant_id(), service_name)?;
        let definition = prepared.definition().clone();
        let key = workload_key(context.tenant_id(), service_name)?;
        let services = Arc::clone(&self.services);
        let tenant_id = context.tenant_id().clone();
        let source_name = service_name.to_owned();
        let source_version = definition.resource_version.clone();
        let source_generation = definition.generation;
        let (claim, joined_provision) = self
            .fence_and_join_inflight_provision(&key, move || {
                services.claim_service_definition_retirement(
                    &tenant_id,
                    &source_name,
                    source_generation,
                    &source_version,
                    WorkloadSourceRetirementOperation::Stop,
                    initial_retirement_fence_generation(),
                    initial_retirement_fence_revision(),
                )
            })
            .await?;
        let Some(loaded) = self.coordinator.load(&key).await? else {
            if self
                .services
                .service_definition_observation_for_tenant(context.tenant_id(), service_name)
                .is_some()
            {
                return Err(ComputeResourceRetirementError::ObservationWithoutSaga);
            }
            if joined_provision.is_some() {
                return Err(ComputeResourceRetirementError::ProvisionWithoutSaga);
            }
            self.services.finalize_unstarted_source_stop(&claim)?;
            self.provisioner.release_retirement_fence(&key);
            return Ok(SandboxServiceRetirementOutcome {
                definition,
                retired_handle: None,
                disposition: WorkloadTeardownDisposition::SourceFinalized,
                terminal_execution: None,
            });
        };
        authenticate_record_source(&loaded, &definition.resource_version, definition.generation)?;
        if let Err(error) = preflight_stopped_successor_generation(&loaded) {
            self.release_unadvanced_retirement_claim(&key, &claim)?;
            return Err(error);
        }
        let (claim, run) = self
            .drive_recorded_teardown(&key, loaded, claim, joined_provision, cancellation)
            .await?;
        authenticate_recorded_stop(&run)?;
        let terminal_execution = recorded_terminal_execution(&run)?;
        let retired_handle = self
            .services
            .project_recorded_service_teardown(&claim, &run)?;
        self.provisioner.release_retirement_fence(&key);
        Ok(SandboxServiceRetirementOutcome {
            definition,
            retired_handle,
            disposition: WorkloadTeardownDisposition::Recorded,
            terminal_execution,
        })
    }

    pub async fn submit_sandbox_teardown(
        &self,
        context: &TenantIsolationContext,
        sandbox_id: &str,
    ) -> Result<SandboxResourceSnapshot, ComputeResourceRetirementError> {
        let snapshot = self
            .services
            .sandbox_resource_snapshot_for_tenant(context.tenant_id(), sandbox_id)?
            .ok_or_else(|| {
                Error::NotFound(format!(
                    "sandbox `{sandbox_id}` was not found for tenant `{}`",
                    context.tenant_id()
                ))
            })?;
        let key = workload_key(context.tenant_id(), sandbox_id)?;
        let services = Arc::clone(&self.services);
        let tenant_id = context.tenant_id().clone();
        let source_id = sandbox_id.to_owned();
        let source_version = snapshot.source.resource_version.clone();
        let source_generation = snapshot.source.generation;
        let (claim, joined_provision) = self
            .fence_and_join_inflight_provision(&key, move || {
                services.claim_standalone_sandbox_retirement(
                    &tenant_id,
                    &source_id,
                    source_generation,
                    &source_version,
                    initial_retirement_fence_generation(),
                    initial_retirement_fence_revision(),
                )
            })
            .await?;
        let snapshot = self
            .services
            .sandbox_resource_snapshot_for_tenant(context.tenant_id(), sandbox_id)?
            .ok_or_else(|| {
                Error::NotFound(format!(
                    "sandbox `{sandbox_id}` was removed while retirement held its source claim"
                ))
            })?;
        let Some(loaded) = self.coordinator.load(&key).await? else {
            if snapshot.observation.is_some() {
                return Err(ComputeResourceRetirementError::ObservationWithoutSaga);
            }
            if joined_provision.is_some() {
                return Err(ComputeResourceRetirementError::ProvisionWithoutSaga);
            }
            self.services.finalize_unstarted_source_stop(&claim)?;
            self.provisioner.release_retirement_fence(&key);
            return Ok(snapshot);
        };
        authenticate_record_source(
            &loaded,
            &snapshot.source.resource_version,
            snapshot.source.generation,
        )?;
        if let Err(error) = preflight_stopped_successor_generation(&loaded) {
            self.release_unadvanced_retirement_claim(&key, &claim)?;
            return Err(error);
        }
        let cancellation = WorkloadTeardownCancellationToken::new();
        let (claim, run) = self
            .drive_recorded_teardown(&key, loaded, claim, joined_provision, &cancellation)
            .await?;
        authenticate_recorded_stop(&run)?;
        let snapshot = self
            .services
            .project_recorded_sandbox_teardown(&claim, &run)
            .map_err(ComputeResourceRetirementError::from)?;
        self.provisioner.release_retirement_fence(&key);
        Ok(snapshot)
    }

    pub async fn submit_definition_teardown(
        &self,
        context: &TenantIsolationContext,
        service_name: &str,
        expected_generation: u64,
        force: bool,
    ) -> Result<ServiceDefinition, ComputeResourceRetirementError> {
        let definition = self
            .services
            .service_definition_for_tenant(context.tenant_id(), service_name)
            .ok_or_else(|| {
                Error::NotFound(format!(
                    "service `{service_name}` was not found for tenant `{}`",
                    context.tenant_id()
                ))
            })?;
        if definition.source != ServiceDefinitionSource::Dynamic {
            return Err(Error::conflict(format!(
                "service `{service_name}` for tenant `{}` is static and cannot be deleted through dynamic service definition routes",
                context.tenant_id()
            ))
            .into());
        }
        if definition.generation != expected_generation {
            return Err(Error::PreconditionFailed(format!(
                "service `{service_name}` has generation {}, but delete expected {expected_generation}",
                definition.generation
            ))
            .into());
        }
        if !matches!(definition.backend, ServiceBackend::Sandbox(_)) {
            return self
                .services
                .finalize_unmanaged_service_definition_deletion(
                    context.tenant_id(),
                    service_name,
                    expected_generation,
                    force,
                )
                .map_err(Into::into);
        }
        let key = workload_key(context.tenant_id(), service_name)?;
        let services = Arc::clone(&self.services);
        let tenant_id = context.tenant_id().clone();
        let source_name = service_name.to_owned();
        let source_version = definition.resource_version.clone();
        let (claim, joined_provision) = self
            .fence_and_join_inflight_provision(&key, move || {
                services.claim_service_definition_retirement(
                    &tenant_id,
                    &source_name,
                    expected_generation,
                    &source_version,
                    WorkloadSourceRetirementOperation::DeleteDefinition { force },
                    initial_retirement_fence_generation(),
                    initial_retirement_fence_revision(),
                )
            })
            .await?;
        let Some(loaded) = self.coordinator.load(&key).await? else {
            if self
                .services
                .service_definition_observation_for_tenant(context.tenant_id(), service_name)
                .is_some()
            {
                return Err(ComputeResourceRetirementError::ObservationWithoutSaga);
            }
            if joined_provision.is_some() {
                return Err(ComputeResourceRetirementError::ProvisionWithoutSaga);
            }
            let definition = self
                .services
                .finalize_unstarted_service_definition_deletion(&claim)?;
            self.provisioner.release_retirement_fence(&key);
            return Ok(definition);
        };
        authenticate_record_source(&loaded, &definition.resource_version, definition.generation)?;
        if let Err(error) = preflight_stopped_successor_generation(&loaded) {
            self.release_unadvanced_retirement_claim(&key, &claim)?;
            return Err(error);
        }
        let cancellation = WorkloadTeardownCancellationToken::new();
        let (claim, run) = self
            .drive_recorded_teardown(&key, loaded, claim, joined_provision, &cancellation)
            .await?;
        authenticate_recorded_stop(&run)?;
        let definition = self
            .services
            .finalize_service_definition_after_recorded(&claim, &run)
            .map_err(ComputeResourceRetirementError::from)?;
        self.provisioner.release_retirement_fence(&key);
        Ok(definition)
    }

    pub(crate) async fn submit_tenant_record_teardown(
        &self,
        loaded: WorkloadSagaRecord,
    ) -> Result<WorkloadSagaRecord, ComputeResourceRetirementError> {
        let key = loaded.key().clone();
        let joined_provision = self
            .provisioner
            .claim_tenant_retirement_and_join(&key)
            .await
            .map_err(ComputeResourceRetirementError::Provision)?;
        let loaded = self
            .coordinator
            .load(&key)
            .await?
            .ok_or(ComputeResourceRetirementError::ProvisionWithoutSaga)?;
        if loaded.phase() == WorkloadSagaPhase::Recorded
            && loaded.active_intent().desired_state() == DesiredWorkloadState::Stopped
            && loaded.successor_intent().is_none()
        {
            self.provisioner.release_retirement_fence(&key);
            return Ok(loaded);
        }
        preflight_stopped_successor_generation(&loaded)?;
        let (record, _) = self.persist_stopped_successor(&key, loaded).await?;
        self.retire_late_provision_result(&key, record.phase(), joined_provision)
            .await?;
        self.settle_issued_restart_before_native_teardown(&key)
            .await?;
        let cancellation = WorkloadTeardownCancellationToken::default();
        let run = self
            .teardown_runtime
            .submit(key.clone(), &cancellation)
            .await?;
        if run.disposition() != WorkloadTeardownRunDisposition::Completed
            || run.record().phase() != WorkloadSagaPhase::Recorded
        {
            return Err(ComputeResourceRetirementError::TeardownPending(
                run.disposition(),
            ));
        }
        let recorded = self
            .confirm_recorded_stopped_successor(run.record())
            .await?;
        self.provisioner.release_retirement_fence(&key);
        Ok(recorded)
    }

    pub(crate) async fn fence_tenant_sources_and_join(
        &self,
        keys: &[WorkloadSagaKey],
    ) -> Result<(), ComputeResourceRetirementError> {
        self.provisioner
            .claim_tenant_retirements_and_join(keys)
            .await
            .map_err(ComputeResourceRetirementError::Provision)
    }

    pub(crate) fn release_tenant_source_fences(&self, keys: &[WorkloadSagaKey]) {
        for key in keys {
            self.provisioner.release_retirement_fence(key);
        }
    }

    async fn drive_recorded_teardown(
        &self,
        key: &WorkloadSagaKey,
        loaded: WorkloadSagaRecord,
        claim: WorkloadSourceRetirementClaim,
        joined_provision: Option<WorkloadProvisionOutcome>,
        cancellation: &WorkloadTeardownCancellationToken,
    ) -> Result<(WorkloadSourceRetirementClaim, WorkloadSagaRecord), ComputeResourceRetirementError>
    {
        let claim = self.services.advance_source_retirement_claim_saga_fence(
            &claim,
            retirement_fence_generation(&loaded),
            loaded.revision(),
        )?;
        let (record, successor_generation) = self.persist_stopped_successor(key, loaded).await?;
        let claim = self.services.advance_source_retirement_claim_saga_fence(
            &claim,
            successor_generation,
            record.revision(),
        )?;
        self.retire_late_provision_result(key, record.phase(), joined_provision)
            .await?;
        self.settle_issued_restart_before_native_teardown(key)
            .await?;
        let run = self
            .teardown_runtime
            .submit(key.clone(), cancellation)
            .await?;
        if run.disposition() != WorkloadTeardownRunDisposition::Completed
            || run.record().phase() != WorkloadSagaPhase::Recorded
        {
            return Err(ComputeResourceRetirementError::TeardownPending(
                run.disposition(),
            ));
        }
        let recorded = self
            .confirm_recorded_stopped_successor(run.record())
            .await?;
        let claim = self.services.advance_source_retirement_claim_saga_fence(
            &claim,
            recorded.active_intent().generation(),
            recorded.revision(),
        )?;
        Ok((claim, recorded))
    }

    fn release_unadvanced_retirement_claim(
        &self,
        key: &WorkloadSagaKey,
        claim: &WorkloadSourceRetirementClaim,
    ) -> Result<(), ComputeResourceRetirementError> {
        self.services
            .release_unadvanced_source_retirement_claim(claim)?;
        self.provisioner.release_retirement_fence(key);
        Ok(())
    }

    async fn confirm_recorded_stopped_successor(
        &self,
        recorded: &WorkloadSagaRecord,
    ) -> Result<WorkloadSagaRecord, ComputeResourceRetirementError> {
        if recorded.phase() != WorkloadSagaPhase::Recorded {
            return Err(ComputeResourceRetirementError::InvalidRecordedSuccessor);
        }
        if recorded.active_intent().desired_state() == DesiredWorkloadState::Stopped
            && recorded.successor_intent().is_none()
        {
            return Ok(recorded.clone());
        }
        if !recorded
            .successor_intent()
            .is_some_and(|successor| successor.desired_state() == DesiredWorkloadState::Stopped)
        {
            return Err(ComputeResourceRetirementError::InvalidRecordedSuccessor);
        }
        self.coordinator
            .promote_recorded_successor(recorded)
            .await
            .map_err(Into::into)
    }

    async fn fence_and_join_inflight_provision<Claim>(
        &self,
        key: &WorkloadSagaKey,
        claim_source: Claim,
    ) -> Result<
        (
            WorkloadSourceRetirementClaim,
            Option<WorkloadProvisionOutcome>,
        ),
        ComputeResourceRetirementError,
    >
    where
        Claim: FnOnce() -> Result<WorkloadSourceRetirementClaim, Error> + Send,
    {
        self.provisioner
            .claim_retirement_and_join(key, claim_source)
            .await
            .map_err(|error| match error.as_ref() {
                crate::workload_provisioner::WorkloadProvisionError::SourceReservation(source) => {
                    ComputeResourceRetirementError::Source(source.clone())
                }
                _ => ComputeResourceRetirementError::Provision(error),
            })
    }

    async fn retire_late_provision_result(
        &self,
        key: &WorkloadSagaKey,
        phase: WorkloadSagaPhase,
        joined: Option<WorkloadProvisionOutcome>,
    ) -> Result<(), ComputeResourceRetirementError> {
        let outcome = match joined {
            Some(outcome) => Some(outcome),
            None if phase.is_provision() => Some(
                self.provisioner
                    .resume_for_retirement(key.clone())
                    .await
                    .map_err(ComputeResourceRetirementError::Provision)?,
            ),
            None => None,
        };
        if let Some(outcome) = outcome.as_ref() {
            match outcome.compensation() {
                WorkloadProvisionCompensationState::NotRequired
                | WorkloadProvisionCompensationState::Completed => {}
                WorkloadProvisionCompensationState::Waiting => {
                    return Err(ComputeResourceRetirementError::TeardownPending(
                        WorkloadTeardownRunDisposition::Waiting,
                    ));
                }
                WorkloadProvisionCompensationState::CleanupPending => {
                    return Err(ComputeResourceRetirementError::TeardownPending(
                        WorkloadTeardownRunDisposition::CleanupPending,
                    ));
                }
            }
            match outcome.disposition() {
                WorkloadProvisionRunDisposition::Waiting => {
                    return Err(ComputeResourceRetirementError::ProvisionSettlementPending);
                }
                WorkloadProvisionRunDisposition::SuccessorSettlementReady => {
                    self.coordinator
                        .commit_provision_settlement_teardown(outcome.record())
                        .await?;
                }
                WorkloadProvisionRunDisposition::SuccessorSettlementCommitted => {}
                WorkloadProvisionRunDisposition::Observed
                | WorkloadProvisionRunDisposition::DefiniteFailure => {}
            }
        }
        Ok(())
    }

    async fn settle_issued_restart_before_native_teardown(
        &self,
        key: &WorkloadSagaKey,
    ) -> Result<(), ComputeResourceRetirementError> {
        match self
            .restart_runtime
            .settle_for_teardown(key)
            .await
            .map_err(ComputeResourceRetirementError::Restart)?
        {
            WorkloadRestartSettlement::Settled => Ok(()),
            WorkloadRestartSettlement::Pending => {
                Err(ComputeResourceRetirementError::RestartSettlementPending)
            }
        }
    }

    async fn persist_stopped_successor(
        &self,
        key: &WorkloadSagaKey,
        record: WorkloadSagaRecord,
    ) -> Result<(WorkloadSagaRecord, WorkloadGeneration), ComputeResourceRetirementError> {
        if record.active_intent().desired_state() == DesiredWorkloadState::Stopped
            && record.successor_intent().is_none()
        {
            return Ok((record.clone(), record.active_intent().generation()));
        }
        if let Some(successor) = record.successor_intent()
            && successor.desired_state() == DesiredWorkloadState::Stopped
        {
            let generation = successor.generation();
            return Ok((record, generation));
        }
        let base = record.successor_intent().unwrap_or(record.active_intent());
        let generation = base
            .generation()
            .checked_next()
            .ok_or(ComputeResourceRetirementError::GenerationOverflow)?;
        let successor = stopped_successor(base, generation)?;
        let confirmed = self
            .coordinator
            .submit_intent(key.clone(), successor)
            .await?;
        Ok((confirmed.record().clone(), generation))
    }
}

fn recorded_terminal_execution(
    record: &WorkloadSagaRecord,
) -> Result<Option<WorkloadExecutionReference>, ComputeResourceRetirementError> {
    let WorkloadPhaseDetail::Recorded(detail) = record.phase_detail() else {
        return Err(ComputeResourceRetirementError::InvalidRecordedSuccessor);
    };
    Ok(detail.terminal_execution_reference().cloned())
}

impl ComputeState {
    /// Resolve the complete managed native retirement facade. Missing exact
    /// teardown composition fails here before services or saga reads.
    pub fn resource_retirer(&self) -> Result<ComputeResourceRetirer, ComputeError> {
        let teardown_runtime = self.workload_teardown_runtime().ok_or_else(|| {
            ComputeError::not_found(
                "native workload retirement requires exact teardown composition",
            )
        })?;
        let services = self.service_manager().ok_or_else(|| {
            ComputeError::not_found("native workload retirement requires a services source owner")
        })?;
        let provisioner = self.workload_provisioner().ok_or_else(|| {
            ComputeError::not_found("native workload retirement requires managed compute")
        })?;
        let coordinator = self.workload_saga_coordinator().ok_or_else(|| {
            ComputeError::not_found("native workload retirement requires a durable saga store")
        })?;
        let restart_runtime = self.workload_restart_runtime().ok_or_else(|| {
            ComputeError::not_found("native workload retirement requires restart settlement")
        })?;
        Ok(ComputeResourceRetirer::new(
            services,
            provisioner,
            coordinator,
            restart_runtime,
            teardown_runtime,
        ))
    }
}

fn foreground_retirement_can_retry(error: &ComputeResourceRetirementError) -> bool {
    matches!(
        error,
        ComputeResourceRetirementError::ProvisionSettlementPending
            | ComputeResourceRetirementError::RestartSettlementPending
            | ComputeResourceRetirementError::TeardownPending(
                WorkloadTeardownRunDisposition::Waiting
            )
    )
}

fn cancelled_retirement() -> ComputeResourceRetirementError {
    ComputeResourceRetirementError::Teardown(WorkloadTeardownSubmissionError::Cancelled)
}

fn workload_key(tenant_id: &TenantId, stable_id: &str) -> Result<WorkloadSagaKey, Error> {
    Ok(WorkloadSagaKey::new(
        tenant_id.clone(),
        WorkloadId::new(stable_id)?,
    ))
}

/// A source claim is installed before the first saga read. Zero is the
/// lower-bound fence for both monotonic saga coordinates; compute advances the
/// claim to observed durable truth before any provider effect or projection.
const fn initial_retirement_fence_generation() -> WorkloadGeneration {
    WorkloadGeneration::new(0)
}

const fn initial_retirement_fence_revision() -> WorkloadSagaRevision {
    WorkloadSagaRevision::new(0)
}

fn authenticate_record_source(
    record: &WorkloadSagaRecord,
    resource_version: &str,
    source_generation: u64,
) -> Result<(), ComputeResourceRetirementError> {
    let source = record
        .successor_intent()
        .unwrap_or(record.active_intent())
        .source();
    if source.source_generation().as_u64() != source_generation
        || source.resource_version().as_str() != resource_version
    {
        return Err(Error::PreconditionFailed(
            "durable workload source is crossed with current services source".to_owned(),
        )
        .into());
    }
    Ok(())
}

fn preflight_stopped_successor_generation(
    record: &WorkloadSagaRecord,
) -> Result<(), ComputeResourceRetirementError> {
    if record.active_intent().desired_state() == DesiredWorkloadState::Stopped
        && record.successor_intent().is_none()
        || record
            .successor_intent()
            .is_some_and(|successor| successor.desired_state() == DesiredWorkloadState::Stopped)
    {
        return Ok(());
    }
    let base = record.successor_intent().unwrap_or(record.active_intent());
    base.generation()
        .checked_next()
        .map(|_| ())
        .ok_or(ComputeResourceRetirementError::GenerationOverflow)
}

fn retirement_fence_generation(record: &WorkloadSagaRecord) -> WorkloadGeneration {
    record
        .successor_intent()
        .filter(|successor| successor.desired_state() == DesiredWorkloadState::Stopped)
        .map_or_else(
            || record.active_intent().generation(),
            WorkloadSagaIntent::generation,
        )
}

fn authenticate_recorded_stop(
    record: &WorkloadSagaRecord,
) -> Result<(), ComputeResourceRetirementError> {
    if record.phase() == WorkloadSagaPhase::Recorded
        && record.active_intent().desired_state() == DesiredWorkloadState::Stopped
        && record.successor_intent().is_none()
    {
        Ok(())
    } else {
        Err(ComputeResourceRetirementError::InvalidRecordedSuccessor)
    }
}

fn stopped_successor(
    base: &WorkloadSagaIntent,
    generation: WorkloadGeneration,
) -> Result<WorkloadSagaIntent, ComputeResourceRetirementError> {
    let compiled = WorkloadNetworkPlanCompiler
        .compile_terminal_empty_successor(
            base.network().compiled_plan(),
            NetworkResourceGeneration::new(generation.as_u64()),
        )
        .map_err(|error| Error::InvalidInput(error.to_string()))?;
    let network = WorkloadNetworkIntent::new(compiled);
    WorkloadSagaIntent::new_with_restart_policy(
        base.kind(),
        DesiredWorkloadState::Stopped,
        generation,
        base.executable().clone(),
        base.source().clone(),
        WorkloadRestartPolicy::Never,
        network,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
        base.admission().clone(),
    )
    .map_err(|error| Error::InvalidInput(error.to_string()).into())
}

#[cfg(test)]
#[path = "resource_retirement/tests/mod.rs"]
mod tests;
