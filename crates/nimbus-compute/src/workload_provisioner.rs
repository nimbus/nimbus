//! Product-level workload provisioning under one compute-owned authority.
//!
//! Callers supply admitted source facts. This seam owns exact composition,
//! durable submission, bounded driving, and retained keyed supervision. It does
//! not expose the coordinator, dispatcher, or driver choreography to products.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nimbus_network::{
    NetworkCapabilityRegistry, NetworkCapabilitySelection, NetworkSovereigntyRequirements,
    NetworkTlsBehavior,
};
use nimbus_sandbox::SandboxSpec;
use nimbus_tenant::TenantIsolationDecision;
use nimbus_workloads::{
    DesiredWorkloadState, NodeIdentity, WorkloadActivationIntent, WorkloadExecutionProviderId,
    WorkloadNetworkForwardingBehavior, WorkloadProvisionSourceGeneration,
    WorkloadProvisionSourceResourceVersion, WorkloadPublicationIntent, WorkloadSagaIntent,
    WorkloadSagaKey, WorkloadSagaStoreError,
};
use thiserror::Error;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::workload_network_plan::WorkloadNetworkEndpointSemanticsInput;
use crate::workload_projection::{
    WorkloadProjectionOrchestrator, WorkloadProjectionSink, WorkloadProjectionState,
};
use crate::workload_provision_composition::{
    WorkloadProvisionCompositionError, WorkloadProvisionCompositionInput,
    WorkloadProvisionSourceSnapshot, compose_workload_provision,
};
use crate::workload_saga::{
    WorkloadProvisionCapabilityRegistry, WorkloadProvisionCompensationError,
    WorkloadProvisionCompensator, WorkloadProvisionDispatcher, WorkloadProvisionDriver,
    WorkloadProvisionRun, WorkloadProvisionRunDisposition, WorkloadProvisionRunError,
    WorkloadProvisionSourceAuthority, WorkloadSagaCoordinator, WorkloadTeardownRunDisposition,
    WorkloadTeardownRuntime,
};

const EMBEDDED_LOCAL_NODE_ID: &str = "embedded-local-node";
const PROVISION_SUPERVISOR_INITIAL_DELAY: Duration = Duration::from_millis(25);
const PROVISION_SUPERVISOR_MAX_DELAY: Duration = Duration::from_secs(1);

/// The canonical identity for Nimbus' in-process local node.
pub fn embedded_local_node_identity() -> NodeIdentity {
    NodeIdentity::new(EMBEDDED_LOCAL_NODE_ID)
        .expect("the canonical embedded local-node identity must remain valid")
}

/// One source-owned standalone or service workload snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadProvisionSource {
    StandaloneSandbox {
        stable_resource_id: String,
        profile: String,
        source_generation: WorkloadProvisionSourceGeneration,
        resource_version: WorkloadProvisionSourceResourceVersion,
        sandbox_spec: SandboxSpec,
    },
    SandboxBackedService {
        service_name: String,
        source_generation: WorkloadProvisionSourceGeneration,
        resource_version: WorkloadProvisionSourceResourceVersion,
        sandbox_spec: SandboxSpec,
    },
}

impl WorkloadProvisionSource {
    fn snapshot(&self) -> WorkloadProvisionSourceSnapshot<'_> {
        match self {
            Self::StandaloneSandbox {
                stable_resource_id,
                profile,
                source_generation,
                resource_version,
                sandbox_spec,
            } => WorkloadProvisionSourceSnapshot::StandaloneSandbox {
                stable_resource_id,
                profile,
                source_generation: *source_generation,
                resource_version,
                sandbox_spec,
            },
            Self::SandboxBackedService {
                service_name,
                source_generation,
                resource_version,
                sandbox_spec,
            } => WorkloadProvisionSourceSnapshot::SandboxBackedService {
                service_name,
                source_generation: *source_generation,
                resource_version,
                sandbox_spec,
            },
        }
    }
}

/// Owned endpoint semantics safe to retain in a tracked provision task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadProvisionEndpointSemantics {
    listener_name: String,
    forwarding: WorkloadNetworkForwardingBehavior,
    tls: NetworkTlsBehavior,
}

impl WorkloadProvisionEndpointSemantics {
    pub(crate) fn new(
        listener_name: impl Into<String>,
        forwarding: WorkloadNetworkForwardingBehavior,
        tls: NetworkTlsBehavior,
    ) -> Self {
        Self {
            listener_name: listener_name.into(),
            forwarding,
            tls,
        }
    }

    pub fn listener_name(&self) -> &str {
        &self.listener_name
    }

    pub const fn forwarding(&self) -> WorkloadNetworkForwardingBehavior {
        self.forwarding
    }

    pub const fn tls(&self) -> NetworkTlsBehavior {
        self.tls
    }

    fn as_input(&self) -> WorkloadNetworkEndpointSemanticsInput<'_> {
        WorkloadNetworkEndpointSemanticsInput::new(&self.listener_name, self.forwarding, self.tls)
    }
}

/// Complete owned input accepted by the product provision seam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadProvisionRequest {
    pub decision: TenantIsolationDecision,
    pub source: WorkloadProvisionSource,
    pub execution_provider_id: WorkloadExecutionProviderId,
    pub endpoint_semantics: Vec<WorkloadProvisionEndpointSemantics>,
    pub activation: WorkloadActivationIntent,
    pub publication: WorkloadPublicationIntent,
}

/// Cooperative cancellation for one caller waiting on retained work.
#[derive(Clone)]
pub struct WorkloadProvisionCancellation {
    signal: watch::Sender<bool>,
}

impl Default for WorkloadProvisionCancellation {
    fn default() -> Self {
        let (signal, _) = watch::channel(false);
        Self { signal }
    }
}

impl WorkloadProvisionCancellation {
    pub fn cancel(&self) {
        self.signal.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.signal.borrow()
    }

    fn subscribe(&self) -> watch::Receiver<bool> {
        self.signal.subscribe()
    }

    pub(crate) async fn cancelled(&self) {
        let mut signal = self.subscribe();
        if *signal.borrow() {
            return;
        }
        while signal.changed().await.is_ok() {
            if *signal.borrow() {
                return;
            }
        }
    }
}

/// Invalid immutable provider-realm configuration.
#[derive(Debug, Error)]
pub enum WorkloadProvisionConfigurationError {
    #[error("provider reports do not contain exact selection {selection}")]
    MissingExactSelection {
        selection: NetworkCapabilitySelection,
    },
}

/// Product-level provision failure.
#[derive(Debug, Error)]
pub enum WorkloadProvisionError {
    #[error("workload provision was cancelled before tracked submission")]
    CancelledBeforeSubmission,
    #[error("workload provision waiter was cancelled after tracked submission")]
    WaiterCancelled,
    #[error("workload provision request is crossed with the tracked request for the same key")]
    CrossedTrackedRequest,
    #[error("workload retirement owns the tracked lifecycle boundary for this key")]
    RetirementInProgress,
    #[error("workload desired-source reservation failed: {0}")]
    SourceReservation(nimbus_core::Error),
    #[error("workload provision composition failed: {0}")]
    Composition(#[from] WorkloadProvisionCompositionError),
    #[error("workload provision drive failed: {0}")]
    Run(#[from] WorkloadProvisionRunError),
    #[error("failed-provision compensation failed: {source}")]
    Compensation {
        source: Arc<WorkloadProvisionCompensationError>,
        failed_run: Box<WorkloadProvisionRun>,
    },
    #[error("tracked workload provision task ended without durable truth")]
    TrackedTaskEnded,
}

/// Product result: portable durable truth plus its separately typed projection state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadProvisionOutcome {
    run: WorkloadProvisionRun,
    durable_record: nimbus_workloads::WorkloadSagaRecord,
    compensation: WorkloadProvisionCompensationState,
    projection: WorkloadProjectionState,
}

/// Durable cleanup state paired with a provision result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadProvisionCompensationState {
    NotRequired,
    Completed,
    Waiting,
    CleanupPending,
}

impl WorkloadProvisionOutcome {
    pub fn run(&self) -> &WorkloadProvisionRun {
        &self.run
    }

    pub fn record(&self) -> &nimbus_workloads::WorkloadSagaRecord {
        &self.durable_record
    }

    pub const fn disposition(&self) -> crate::workload_saga::WorkloadProvisionRunDisposition {
        self.run.disposition()
    }

    pub const fn projection(&self) -> WorkloadProjectionState {
        self.projection
    }

    pub const fn compensation(&self) -> WorkloadProvisionCompensationState {
        self.compensation
    }
}

pub type WorkloadProvisionResult = Result<WorkloadProvisionOutcome, Arc<WorkloadProvisionError>>;

struct InFlightProvision {
    intent: Option<WorkloadSagaIntent>,
    /// First bounded receipt, and later progress, returned to start/resume callers.
    completion: watch::Receiver<Option<WorkloadProvisionResult>>,
    /// Result published only when the retained task stops driving this key.
    settlement: watch::Receiver<Option<WorkloadProvisionResult>>,
    _task: Option<JoinHandle<()>>,
}

#[derive(Clone)]
enum RetainedCompensationWork {
    ResumeTeardown(Box<WorkloadProvisionOutcome>),
    RetryFailedProvisionHandoff(Box<WorkloadProvisionRun>),
}

#[derive(Default)]
struct WorkloadProvisionSupervisor {
    in_flight: BTreeMap<WorkloadSagaKey, InFlightProvision>,
    retiring: BTreeSet<WorkloadSagaKey>,
}

/// Sole product composition of provision facts, persistence, and provider drive.
pub struct WorkloadProvisioner {
    local_node: NodeIdentity,
    provider_reports: NetworkCapabilityRegistry,
    capability_selection: NetworkCapabilitySelection,
    sovereignty: NetworkSovereigntyRequirements,
    coordinator: Arc<WorkloadSagaCoordinator>,
    driver: Arc<WorkloadProvisionDriver>,
    projection: Arc<WorkloadProjectionOrchestrator>,
    compensation: Arc<WorkloadProvisionCompensator>,
    supervisor: Arc<Mutex<WorkloadProvisionSupervisor>>,
    #[cfg(test)]
    test_submission_boundary: Mutex<Option<(Arc<std::sync::Barrier>, Arc<std::sync::Barrier>)>>,
    #[cfg(test)]
    test_wait_boundary: Mutex<Option<Arc<tokio::sync::Semaphore>>>,
    #[cfg(test)]
    test_retirement_claim_boundary: Mutex<Option<Arc<tokio::sync::Semaphore>>>,
}

impl WorkloadProvisioner {
    #[expect(
        clippy::too_many_arguments,
        reason = "constructor freezes every independent authority"
    )]
    pub fn new(
        local_node: NodeIdentity,
        provider_reports: NetworkCapabilityRegistry,
        capability_selection: NetworkCapabilitySelection,
        sovereignty: NetworkSovereigntyRequirements,
        coordinator: Arc<WorkloadSagaCoordinator>,
        teardown_runtime: Arc<WorkloadTeardownRuntime>,
        source_authority: Arc<dyn WorkloadProvisionSourceAuthority>,
        provision_capabilities: WorkloadProvisionCapabilityRegistry,
        projection_sink: Arc<dyn WorkloadProjectionSink>,
    ) -> Result<Self, WorkloadProvisionConfigurationError> {
        if !provider_reports
            .selections()
            .any(|candidate| candidate == &capability_selection)
        {
            return Err(WorkloadProvisionConfigurationError::MissingExactSelection {
                selection: capability_selection,
            });
        }
        let provision_capabilities = Arc::new(provision_capabilities);
        let dispatcher = Arc::new(WorkloadProvisionDispatcher::new(
            source_authority,
            provider_reports.clone(),
            Arc::clone(&provision_capabilities),
        ));
        Ok(Self {
            local_node,
            provider_reports,
            capability_selection,
            sovereignty,
            coordinator: Arc::clone(&coordinator),
            driver: Arc::new(WorkloadProvisionDriver::new(
                Arc::clone(&coordinator),
                dispatcher,
            )),
            projection: Arc::new(WorkloadProjectionOrchestrator::new(
                provision_capabilities,
                projection_sink,
            )),
            compensation: Arc::new(WorkloadProvisionCompensator::new(
                Arc::clone(&coordinator),
                teardown_runtime,
            )),
            supervisor: Arc::new(Mutex::new(WorkloadProvisionSupervisor::default())),
            #[cfg(test)]
            test_submission_boundary: Mutex::new(None),
            #[cfg(test)]
            test_wait_boundary: Mutex::new(None),
            #[cfg(test)]
            test_retirement_claim_boundary: Mutex::new(None),
        })
    }

    async fn finalize_run(&self, run: WorkloadProvisionRun) -> WorkloadProvisionResult {
        let (durable_record, compensation) = match run.disposition() {
            WorkloadProvisionRunDisposition::DefiniteFailure => {
                let teardown = match self
                    .compensation
                    .compensate_definite_provision_failure(run.record())
                    .await
                {
                    Ok(teardown) => teardown,
                    Err(source) => {
                        return Err(Arc::new(WorkloadProvisionError::Compensation {
                            source: Arc::new(source),
                            failed_run: Box::new(run),
                        }));
                    }
                };
                let state = match teardown.disposition() {
                    WorkloadTeardownRunDisposition::Completed => {
                        WorkloadProvisionCompensationState::Completed
                    }
                    WorkloadTeardownRunDisposition::Waiting => {
                        WorkloadProvisionCompensationState::Waiting
                    }
                    WorkloadTeardownRunDisposition::CleanupPending => {
                        WorkloadProvisionCompensationState::CleanupPending
                    }
                };
                (teardown.record().clone(), state)
            }
            WorkloadProvisionRunDisposition::Observed
            | WorkloadProvisionRunDisposition::Waiting
            | WorkloadProvisionRunDisposition::SuccessorSettlementReady
            | WorkloadProvisionRunDisposition::SuccessorSettlementCommitted => (
                run.record().clone(),
                WorkloadProvisionCompensationState::NotRequired,
            ),
        };
        let projection = self.projection.project(&run).await;
        Ok(WorkloadProvisionOutcome {
            run,
            durable_record,
            compensation,
            projection,
        })
    }

    fn retain_after_result(result: &WorkloadProvisionResult) -> bool {
        match result {
            Ok(outcome) => matches!(
                outcome.compensation(),
                WorkloadProvisionCompensationState::Waiting
                    | WorkloadProvisionCompensationState::CleanupPending
            ),
            Err(error) => matches!(error.as_ref(), WorkloadProvisionError::Compensation { .. }),
        }
    }

    fn retained_compensation_work(
        result: &WorkloadProvisionResult,
    ) -> Option<RetainedCompensationWork> {
        match result {
            Ok(outcome)
                if outcome.compensation() == WorkloadProvisionCompensationState::Waiting =>
            {
                Some(RetainedCompensationWork::ResumeTeardown(Box::new(
                    outcome.clone(),
                )))
            }
            Err(error) => match error.as_ref() {
                WorkloadProvisionError::Compensation { failed_run, .. } => Some(
                    RetainedCompensationWork::RetryFailedProvisionHandoff(failed_run.clone()),
                ),
                _ => None,
            },
            _ => None,
        }
    }

    fn parked_retained_result(result: &WorkloadProvisionResult) -> bool {
        Self::retain_after_result(result) && Self::retained_compensation_work(result).is_none()
    }

    async fn resume_compensation(
        &self,
        prior: WorkloadProvisionOutcome,
    ) -> WorkloadProvisionResult {
        let failed_run = prior.run.clone();
        let teardown = self
            .compensation
            .resume(prior.record().key().clone())
            .await
            .map_err(|source| {
                Arc::new(WorkloadProvisionError::Compensation {
                    source: Arc::new(source),
                    failed_run: Box::new(failed_run),
                })
            })?;
        let compensation = match teardown.disposition() {
            WorkloadTeardownRunDisposition::Completed => {
                WorkloadProvisionCompensationState::Completed
            }
            WorkloadTeardownRunDisposition::Waiting => WorkloadProvisionCompensationState::Waiting,
            WorkloadTeardownRunDisposition::CleanupPending => {
                WorkloadProvisionCompensationState::CleanupPending
            }
        };
        Ok(WorkloadProvisionOutcome {
            run: prior.run,
            durable_record: teardown.record().clone(),
            compensation,
            projection: prior.projection,
        })
    }

    fn publish_tracked_result(
        &self,
        key: &WorkloadSagaKey,
        sender: &watch::Sender<Option<WorkloadProvisionResult>>,
        settlement_sender: &watch::Sender<Option<WorkloadProvisionResult>>,
        result: WorkloadProvisionResult,
    ) {
        let retain = Self::retain_after_result(&result);
        let mut supervisor = self
            .supervisor
            .lock()
            .expect("workload provision supervisor lock should not be poisoned");
        sender.send_replace(Some(result.clone()));
        settlement_sender.send_replace(Some(result));
        if retain {
            if let Some(entry) = supervisor.in_flight.get_mut(key) {
                entry._task = None;
            }
        } else {
            supervisor.in_flight.remove(key);
        }
    }

    fn requires_retained_supervision(result: &WorkloadProvisionResult) -> bool {
        let Ok(outcome) = result else {
            return false;
        };
        if outcome.compensation() != WorkloadProvisionCompensationState::NotRequired {
            return false;
        }
        match outcome.disposition() {
            WorkloadProvisionRunDisposition::Waiting => {
                !(outcome.record().phase() == nimbus_workloads::WorkloadSagaPhase::NetworkAttached
                    && outcome.record().active_intent().activation()
                        == WorkloadActivationIntent::PrepareOnly)
            }
            WorkloadProvisionRunDisposition::Observed => {
                matches!(outcome.projection(), WorkloadProjectionState::Pending(_))
            }
            WorkloadProvisionRunDisposition::SuccessorSettlementReady
            | WorkloadProvisionRunDisposition::SuccessorSettlementCommitted
            | WorkloadProvisionRunDisposition::DefiniteFailure => false,
        }
    }

    fn retirement_requested(&self, key: &WorkloadSagaKey) -> bool {
        self.supervisor
            .lock()
            .expect("workload provision supervisor lock should not be poisoned")
            .retiring
            .contains(key)
    }

    async fn supervise_result(
        &self,
        key: &WorkloadSagaKey,
        sender: &watch::Sender<Option<WorkloadProvisionResult>>,
        settlement_sender: &watch::Sender<Option<WorkloadProvisionResult>>,
        mut result: WorkloadProvisionResult,
    ) {
        let mut delay = PROVISION_SUPERVISOR_INITIAL_DELAY;
        loop {
            if !Self::requires_retained_supervision(&result) || self.retirement_requested(key) {
                self.publish_tracked_result(key, sender, settlement_sender, result);
                return;
            }

            // The first bounded receipt returns truthful pending state to the
            // caller. The retained task continues exact read-only inspection;
            // GET remains side-effect-free and a second POST is unnecessary.
            sender.send_replace(Some(result.clone()));
            tokio::time::sleep(delay).await;
            delay = delay.saturating_mul(2).min(PROVISION_SUPERVISOR_MAX_DELAY);
            if self.retirement_requested(key) {
                self.publish_tracked_result(key, sender, settlement_sender, result);
                return;
            }

            result = match self.driver.resume(key).await {
                Ok(run) => self.finalize_run(run).await,
                Err(error) => Err(Arc::new(WorkloadProvisionError::Run(error))),
            };
        }
    }

    pub fn local_node(&self) -> &NodeIdentity {
        &self.local_node
    }

    pub fn provider_reports(&self) -> &NetworkCapabilityRegistry {
        &self.provider_reports
    }

    pub fn capability_selection(&self) -> &NetworkCapabilitySelection {
        &self.capability_selection
    }

    pub fn sovereignty(&self) -> &NetworkSovereigntyRequirements {
        &self.sovereignty
    }

    /// Select the workload lifecycle generation independently from the
    /// services-owned source generation. Exact running replay keeps the active
    /// generation; a changed or stopped source advances durable saga truth.
    pub async fn lifecycle_generation_for_start(
        &self,
        key: &WorkloadSagaKey,
        source_generation: WorkloadProvisionSourceGeneration,
        resource_version: &WorkloadProvisionSourceResourceVersion,
    ) -> Result<u64, nimbus_core::Error> {
        let Some(record) = self
            .coordinator
            .load(key)
            .await
            .map_err(map_saga_load_error)?
        else {
            return Ok(source_generation.as_u64());
        };
        if record.successor_intent().is_some() {
            return Err(nimbus_core::Error::conflict(
                "workload retirement successor is already durable; retry start after retirement reaches Recorded"
                    .to_owned(),
            ));
        }
        let active = record.active_intent();
        let exact_source = active.source().source_generation() == source_generation
            && active.source().resource_version() == resource_version;
        if exact_source && active.desired_state() == DesiredWorkloadState::Running {
            return Ok(active.generation().as_u64());
        }
        let next = active.generation().checked_next().ok_or_else(|| {
            nimbus_core::Error::PreconditionFailed(
                "workload lifecycle generation overflow prevents a later start".to_owned(),
            )
        })?;
        Ok(next.as_u64())
    }

    /// Compose, durably submit, and drive one exact generation.
    pub async fn provision(
        self: &Arc<Self>,
        request: WorkloadProvisionRequest,
        cancellation: &WorkloadProvisionCancellation,
    ) -> WorkloadProvisionResult {
        self.provision_with_source_reservation(request, cancellation, || Ok(()))
            .await
    }

    /// Compose and submit one exact generation while linearizing its
    /// synchronous desired-source reservation with cancellation and keyed
    /// in-flight insertion.
    ///
    /// The callback revalidates source authority for every attempt while the
    /// keyed lock is held. Exact in-flight replay reuses the tracked result;
    /// the callback must therefore be idempotent for the same source facts.
    pub async fn provision_with_source_reservation<Reserve>(
        self: &Arc<Self>,
        request: WorkloadProvisionRequest,
        cancellation: &WorkloadProvisionCancellation,
        reserve_source: Reserve,
    ) -> WorkloadProvisionResult
    where
        Reserve: FnOnce() -> Result<(), nimbus_core::Error> + Send,
    {
        if cancellation.is_cancelled() {
            return Err(Arc::new(WorkloadProvisionError::CancelledBeforeSubmission));
        }
        let endpoint_semantics = request
            .endpoint_semantics
            .iter()
            .map(WorkloadProvisionEndpointSemantics::as_input)
            .collect::<Vec<_>>();
        let composed = compose_workload_provision(WorkloadProvisionCompositionInput {
            decision: &request.decision,
            local_node: &self.local_node,
            source: request.source.snapshot(),
            execution_provider_id: &request.execution_provider_id,
            capability_selection: &self.capability_selection,
            capability_registry: &self.provider_reports,
            sovereignty: self.sovereignty.clone(),
            endpoint_semantics: &endpoint_semantics,
            activation: request.activation,
            publication: request.publication,
        })
        .map_err(|error| Arc::new(WorkloadProvisionError::Composition(error)))?;
        if cancellation.is_cancelled() {
            return Err(Arc::new(WorkloadProvisionError::CancelledBeforeSubmission));
        }
        let (key, intent) = composed.into_parts();
        let receiver = self.track_submission(key, intent, cancellation, reserve_source)?;
        #[cfg(test)]
        self.notify_test_wait_boundary();
        wait_for_completion(receiver, cancellation).await
    }

    /// Resume only durable truth under the same retained provider realm.
    pub async fn resume(
        self: &Arc<Self>,
        key: WorkloadSagaKey,
        cancellation: &WorkloadProvisionCancellation,
    ) -> WorkloadProvisionResult {
        if cancellation.is_cancelled() {
            return Err(Arc::new(WorkloadProvisionError::CancelledBeforeSubmission));
        }
        let receiver = self.track_resume(key, cancellation, false, false)?;
        #[cfg(test)]
        self.notify_test_wait_boundary();
        wait_for_completion(receiver, cancellation).await
    }

    /// Resume one exact process-bound publication after this composition
    /// owner reopened the durable roots but before startup recovery completes.
    pub(crate) async fn resume_owner_reopened_publication(
        self: &Arc<Self>,
        key: WorkloadSagaKey,
        cancellation: &WorkloadProvisionCancellation,
    ) -> WorkloadProvisionResult {
        if cancellation.is_cancelled() {
            return Err(Arc::new(WorkloadProvisionError::CancelledBeforeSubmission));
        }
        let receiver = self.track_resume(key, cancellation, false, true)?;
        #[cfg(test)]
        self.notify_test_wait_boundary();
        wait_for_completion(receiver, cancellation).await
    }

    /// Acquire one services-owned source claim while holding the same
    /// insertion lock as provision, then join the exact retained provision if
    /// one exists. The source claim remains owned by services if this waiter is
    /// cancelled or the retained work reports an error.
    pub async fn claim_retirement_and_join<Claim, Claimed>(
        self: &Arc<Self>,
        key: &WorkloadSagaKey,
        claim_source: Claim,
    ) -> Result<(Claimed, Option<WorkloadProvisionOutcome>), Arc<WorkloadProvisionError>>
    where
        Claim: FnOnce() -> Result<Claimed, nimbus_core::Error> + Send,
    {
        let (claimed, completion) = {
            let mut supervisor = self
                .supervisor
                .lock()
                .expect("workload provision supervisor lock should not be poisoned");
            let claimed = claim_source()
                .map_err(WorkloadProvisionError::SourceReservation)
                .map_err(Arc::new)?;
            supervisor.retiring.insert(key.clone());
            #[cfg(test)]
            self.notify_test_retirement_claim_boundary();
            let completion = supervisor
                .in_flight
                .get(key)
                .map(|entry| entry.settlement.clone());
            (claimed, completion)
        };
        let Some(completion) = completion else {
            return Ok((claimed, None));
        };
        let cancellation = WorkloadProvisionCancellation::default();
        wait_for_completion(completion, &cancellation)
            .await
            .map(|outcome| (claimed, Some(outcome)))
    }

    /// Fence one exact key under an already installed tenant-wide source
    /// barrier and join retained provisioning without reopening services.
    pub(crate) async fn claim_tenant_retirement_and_join(
        self: &Arc<Self>,
        key: &WorkloadSagaKey,
    ) -> Result<Option<WorkloadProvisionOutcome>, Arc<WorkloadProvisionError>> {
        let completion = {
            let mut supervisor = self
                .supervisor
                .lock()
                .expect("workload provision supervisor lock should not be poisoned");
            supervisor.retiring.insert(key.clone());
            supervisor
                .in_flight
                .get(key)
                .map(|entry| entry.settlement.clone())
        };
        let Some(completion) = completion else {
            return Ok(None);
        };
        let cancellation = WorkloadProvisionCancellation::default();
        wait_for_completion(completion, &cancellation)
            .await
            .map(Some)
    }

    /// Fence every source key captured by one tenant-retirement snapshot under
    /// the submission lock, then join all retained provisions. The services
    /// barrier rejects reservations after the snapshot; this batch fence
    /// captures reservations that won immediately before it.
    pub(crate) async fn claim_tenant_retirements_and_join(
        self: &Arc<Self>,
        keys: &[WorkloadSagaKey],
    ) -> Result<(), Arc<WorkloadProvisionError>> {
        let completions = {
            let mut supervisor = self
                .supervisor
                .lock()
                .expect("workload provision supervisor lock should not be poisoned");
            let mut completions = Vec::new();
            for key in keys {
                supervisor.retiring.insert(key.clone());
                if let Some(entry) = supervisor.in_flight.get(key) {
                    completions.push(entry.settlement.clone());
                }
            }
            #[cfg(test)]
            self.notify_test_retirement_claim_boundary();
            completions
        };

        let cancellation = WorkloadProvisionCancellation::default();
        let mut first_error = None;
        for completion in completions {
            if let Err(error) = wait_for_completion(completion, &cancellation).await
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Resume durable provision truth only as part of an already-fenced
    /// retirement. Public resume and new submissions remain rejected.
    pub(crate) async fn resume_for_retirement(
        self: &Arc<Self>,
        key: WorkloadSagaKey,
    ) -> WorkloadProvisionResult {
        let cancellation = WorkloadProvisionCancellation::default();
        let receiver = self.track_resume(key, &cancellation, true, false)?;
        wait_for_completion(receiver, &cancellation).await
    }

    /// Release the compute-local retirement fence after services completes
    /// the exact terminal source mutation.
    pub(crate) fn release_retirement_fence(&self, key: &WorkloadSagaKey) {
        self.supervisor
            .lock()
            .expect("workload provision supervisor lock should not be poisoned")
            .retiring
            .remove(key);
    }

    fn track_submission<Reserve>(
        self: &Arc<Self>,
        key: WorkloadSagaKey,
        intent: WorkloadSagaIntent,
        cancellation: &WorkloadProvisionCancellation,
        reserve_source: Reserve,
    ) -> Result<watch::Receiver<Option<WorkloadProvisionResult>>, Arc<WorkloadProvisionError>>
    where
        Reserve: FnOnce() -> Result<(), nimbus_core::Error> + Send,
    {
        // The guard and keyed insertion form the submission linearization
        // boundary. `cancel()` either changes the value before this read (so
        // no tracked entry is created), or waits for the guard to drop after
        // insertion (so cancellation affects only this caller's wait).
        let cancellation_guard = cancellation.signal.borrow();
        if *cancellation_guard {
            return Err(Arc::new(WorkloadProvisionError::CancelledBeforeSubmission));
        }
        #[cfg(test)]
        self.pause_at_test_submission_boundary();
        let mut supervisor = self
            .supervisor
            .lock()
            .expect("workload provision supervisor lock should not be poisoned");
        if supervisor.retiring.contains(&key) {
            return Err(Arc::new(WorkloadProvisionError::RetirementInProgress));
        }
        if let Some(existing) = supervisor.in_flight.get(&key) {
            if existing.intent.as_ref() != Some(&intent) {
                return Err(Arc::new(WorkloadProvisionError::CrossedTrackedRequest));
            }
            if existing
                .completion
                .borrow()
                .as_ref()
                .is_some_and(Self::parked_retained_result)
            {
                return Ok(existing.completion.clone());
            }
        }
        reserve_source()
            .map_err(WorkloadProvisionError::SourceReservation)
            .map_err(Arc::new)?;
        if let Some(existing) = supervisor.in_flight.get_mut(&key) {
            if existing.intent.as_ref() == Some(&intent) {
                let retry = existing
                    .completion
                    .borrow()
                    .as_ref()
                    .and_then(Self::retained_compensation_work);
                if let Some(retry) = retry {
                    let (sender, receiver) = watch::channel(None);
                    let (settlement_sender, settlement) = watch::channel(None);
                    existing.completion = receiver.clone();
                    existing.settlement = settlement;
                    existing._task = None;
                    drop(supervisor);
                    drop(cancellation_guard);
                    self.spawn_retained_compensation_task(key, retry, sender, settlement_sender);
                    return Ok(receiver);
                }
                return Ok(existing.completion.clone());
            }
            return Err(Arc::new(WorkloadProvisionError::CrossedTrackedRequest));
        }
        let (sender, receiver) = watch::channel(None);
        let (settlement_sender, settlement) = watch::channel(None);
        supervisor.in_flight.insert(
            key.clone(),
            InFlightProvision {
                intent: Some(intent.clone()),
                completion: receiver.clone(),
                settlement,
                _task: None,
            },
        );
        drop(supervisor);
        drop(cancellation_guard);

        self.spawn_submission_task(key, intent, sender, settlement_sender);
        Ok(receiver)
    }

    fn track_resume(
        self: &Arc<Self>,
        key: WorkloadSagaKey,
        cancellation: &WorkloadProvisionCancellation,
        allow_retirement: bool,
        owner_reopened_publication: bool,
    ) -> Result<watch::Receiver<Option<WorkloadProvisionResult>>, Arc<WorkloadProvisionError>> {
        let cancellation_guard = cancellation.signal.borrow();
        if *cancellation_guard {
            return Err(Arc::new(WorkloadProvisionError::CancelledBeforeSubmission));
        }
        let mut supervisor = self
            .supervisor
            .lock()
            .expect("workload provision supervisor lock should not be poisoned");
        if supervisor.retiring.contains(&key) && !allow_retirement {
            return Err(Arc::new(WorkloadProvisionError::RetirementInProgress));
        }
        if let Some(existing) = supervisor.in_flight.get_mut(&key) {
            let retry = existing
                .completion
                .borrow()
                .as_ref()
                .and_then(Self::retained_compensation_work);
            if let Some(retry) = retry {
                let (sender, receiver) = watch::channel(None);
                let (settlement_sender, settlement) = watch::channel(None);
                existing.completion = receiver.clone();
                existing.settlement = settlement;
                existing._task = None;
                drop(supervisor);
                drop(cancellation_guard);
                self.spawn_retained_compensation_task(key, retry, sender, settlement_sender);
                return Ok(receiver);
            }
            return Ok(existing.completion.clone());
        }
        let (sender, receiver) = watch::channel(None);
        let (settlement_sender, settlement) = watch::channel(None);
        supervisor.in_flight.insert(
            key.clone(),
            InFlightProvision {
                intent: None,
                completion: receiver.clone(),
                settlement,
                _task: None,
            },
        );
        drop(supervisor);
        drop(cancellation_guard);

        self.spawn_resume_task(
            key,
            sender,
            settlement_sender,
            owner_reopened_publication,
            !allow_retirement,
        );
        Ok(receiver)
    }

    fn spawn_submission_task(
        self: &Arc<Self>,
        key: WorkloadSagaKey,
        intent: WorkloadSagaIntent,
        sender: watch::Sender<Option<WorkloadProvisionResult>>,
        settlement_sender: watch::Sender<Option<WorkloadProvisionResult>>,
    ) {
        let provisioner = Arc::clone(self);
        let task_key = key.clone();
        let task = tokio::spawn(async move {
            let result = match provisioner
                .driver
                .submit_and_drive(task_key.clone(), intent)
                .await
            {
                Ok(run) => provisioner.finalize_run(run).await,
                Err(error) => Err(Arc::new(WorkloadProvisionError::Run(error))),
            };
            provisioner
                .supervise_result(&task_key, &sender, &settlement_sender, result)
                .await;
        });
        self.install_tracked_task(&key, task);
    }

    fn spawn_resume_task(
        self: &Arc<Self>,
        key: WorkloadSagaKey,
        sender: watch::Sender<Option<WorkloadProvisionResult>>,
        settlement_sender: watch::Sender<Option<WorkloadProvisionResult>>,
        owner_reopened_publication: bool,
        supervise_waiting: bool,
    ) {
        let provisioner = Arc::clone(self);
        let task_key = key.clone();
        let task = tokio::spawn(async move {
            let resumed = if owner_reopened_publication {
                provisioner
                    .driver
                    .resume_owner_reopened_publication(&task_key)
                    .await
            } else {
                provisioner.driver.resume(&task_key).await
            };
            let result = match resumed {
                Ok(run) => provisioner.finalize_run(run).await,
                Err(error) => Err(Arc::new(WorkloadProvisionError::Run(error))),
            };
            if supervise_waiting {
                provisioner
                    .supervise_result(&task_key, &sender, &settlement_sender, result)
                    .await;
            } else {
                provisioner.publish_tracked_result(&task_key, &sender, &settlement_sender, result);
            }
        });
        self.install_tracked_task(&key, task);
    }

    fn spawn_retained_compensation_task(
        self: &Arc<Self>,
        key: WorkloadSagaKey,
        work: RetainedCompensationWork,
        sender: watch::Sender<Option<WorkloadProvisionResult>>,
        settlement_sender: watch::Sender<Option<WorkloadProvisionResult>>,
    ) {
        let provisioner = Arc::clone(self);
        let task_key = key.clone();
        let task = tokio::spawn(async move {
            let result = match work {
                RetainedCompensationWork::ResumeTeardown(prior) => {
                    provisioner.resume_compensation(*prior).await
                }
                RetainedCompensationWork::RetryFailedProvisionHandoff(run) => {
                    provisioner.finalize_run(*run).await
                }
            };
            provisioner.publish_tracked_result(&task_key, &sender, &settlement_sender, result);
        });
        self.install_tracked_task(&key, task);
    }

    fn install_tracked_task(&self, key: &WorkloadSagaKey, task: JoinHandle<()>) {
        if let Some(entry) = self
            .supervisor
            .lock()
            .expect("workload provision supervisor lock should not be poisoned")
            .in_flight
            .get_mut(key)
        {
            entry._task = Some(task);
        }
    }

    #[cfg(test)]
    fn install_test_submission_boundary(
        &self,
        entered: Arc<std::sync::Barrier>,
        release: Arc<std::sync::Barrier>,
    ) {
        *self
            .test_submission_boundary
            .lock()
            .expect("test submission-boundary lock should not be poisoned") =
            Some((entered, release));
    }

    #[cfg(test)]
    fn pause_at_test_submission_boundary(&self) {
        let boundary = self
            .test_submission_boundary
            .lock()
            .expect("test submission-boundary lock should not be poisoned")
            .clone();
        if let Some((entered, release)) = boundary {
            entered.wait();
            release.wait();
        }
    }

    #[cfg(test)]
    fn install_test_wait_boundary(&self, entered: Arc<tokio::sync::Semaphore>) {
        *self
            .test_wait_boundary
            .lock()
            .expect("test wait-boundary lock should not be poisoned") = Some(entered);
    }

    #[cfg(test)]
    fn notify_test_wait_boundary(&self) {
        if let Some(entered) = self
            .test_wait_boundary
            .lock()
            .expect("test wait-boundary lock should not be poisoned")
            .as_ref()
        {
            entered.add_permits(1);
        }
    }

    #[cfg(test)]
    pub(crate) fn install_test_retirement_claim_boundary(
        &self,
        entered: Arc<tokio::sync::Semaphore>,
    ) {
        *self
            .test_retirement_claim_boundary
            .lock()
            .expect("test retirement-claim boundary lock should not be poisoned") = Some(entered);
    }

    #[cfg(test)]
    fn notify_test_retirement_claim_boundary(&self) {
        if let Some(entered) = self
            .test_retirement_claim_boundary
            .lock()
            .expect("test retirement-claim boundary lock should not be poisoned")
            .as_ref()
        {
            entered.add_permits(1);
        }
    }

    #[cfg(test)]
    pub(crate) fn has_tracked_submission(&self, key: &WorkloadSagaKey) -> bool {
        self.supervisor
            .lock()
            .expect("workload provision supervisor lock should not be poisoned")
            .in_flight
            .contains_key(key)
    }

    #[cfg(test)]
    pub(crate) fn has_running_tracked_task(&self, key: &WorkloadSagaKey) -> bool {
        self.supervisor
            .lock()
            .expect("workload provision supervisor lock should not be poisoned")
            .in_flight
            .get(key)
            .and_then(|entry| entry._task.as_ref())
            .is_some_and(|task| !task.is_finished())
    }
}

fn map_saga_load_error(error: WorkloadSagaStoreError) -> nimbus_core::Error {
    nimbus_core::Error::Internal(format!(
        "failed to load durable workload lifecycle generation: {error}"
    ))
}

async fn wait_for_completion(
    mut receiver: watch::Receiver<Option<WorkloadProvisionResult>>,
    cancellation: &WorkloadProvisionCancellation,
) -> WorkloadProvisionResult {
    let mut cancellation_signal = cancellation.subscribe();
    loop {
        if let Some(result) = receiver.borrow().clone() {
            return result;
        }
        if *cancellation_signal.borrow() {
            return Err(Arc::new(WorkloadProvisionError::WaiterCancelled));
        }
        tokio::select! {
            changed = cancellation_signal.changed() => {
                if changed.is_err() || *cancellation_signal.borrow() {
                    return Err(Arc::new(WorkloadProvisionError::WaiterCancelled));
                }
            }
            changed = receiver.changed() => {
                if changed.is_err() {
                    return Err(Arc::new(WorkloadProvisionError::TrackedTaskEnded));
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "workload_provisioner/tests.rs"]
mod tests;
