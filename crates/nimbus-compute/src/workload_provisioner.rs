//! Product-level workload provisioning under one compute-owned authority.
//!
//! Callers supply admitted source facts. This seam owns exact composition,
//! durable submission, bounded driving, and retained keyed supervision. It does
//! not expose the coordinator, dispatcher, or driver choreography to products.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nimbus_network::{
    NetworkCapabilityRegistry, NetworkCapabilitySelection, NetworkSovereigntyRequirements,
    NetworkTlsBehavior,
};
use nimbus_sandbox::SandboxSpec;
use nimbus_tenant::TenantIsolationDecision;
use nimbus_workloads::{
    NodeIdentity, WorkloadActivationIntent, WorkloadExecutionProviderId,
    WorkloadNetworkForwardingBehavior, WorkloadProvisionSourceGeneration,
    WorkloadProvisionSourceResourceVersion, WorkloadPublicationIntent, WorkloadSagaIntent,
    WorkloadSagaKey,
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
    WorkloadProvisionCapabilityRegistry, WorkloadProvisionDispatcher, WorkloadProvisionDriver,
    WorkloadProvisionRun, WorkloadProvisionRunError, WorkloadProvisionSourceAuthority,
    WorkloadSagaCoordinator,
};

const EMBEDDED_LOCAL_NODE_ID: &str = "embedded-local-node";

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
    pub fn new(
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
    #[error("workload desired-source reservation failed: {0}")]
    SourceReservation(nimbus_core::Error),
    #[error("workload provision composition failed: {0}")]
    Composition(#[from] WorkloadProvisionCompositionError),
    #[error("workload provision drive failed: {0}")]
    Run(#[from] WorkloadProvisionRunError),
    #[error("tracked workload provision task ended without durable truth")]
    TrackedTaskEnded,
}

/// Product result: portable durable truth plus its separately typed projection state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadProvisionOutcome {
    run: WorkloadProvisionRun,
    projection: WorkloadProjectionState,
}

impl WorkloadProvisionOutcome {
    pub fn run(&self) -> &WorkloadProvisionRun {
        &self.run
    }

    pub fn record(&self) -> &nimbus_workloads::WorkloadSagaRecord {
        self.run.record()
    }

    pub const fn disposition(&self) -> crate::workload_saga::WorkloadProvisionRunDisposition {
        self.run.disposition()
    }

    pub const fn projection(&self) -> WorkloadProjectionState {
        self.projection
    }
}

pub type WorkloadProvisionResult = Result<WorkloadProvisionOutcome, Arc<WorkloadProvisionError>>;

struct InFlightProvision {
    intent: Option<WorkloadSagaIntent>,
    completion: watch::Receiver<Option<WorkloadProvisionResult>>,
    _task: Option<JoinHandle<()>>,
}

/// Sole product composition of provision facts, persistence, and provider drive.
pub struct WorkloadProvisioner {
    local_node: NodeIdentity,
    provider_reports: NetworkCapabilityRegistry,
    capability_selection: NetworkCapabilitySelection,
    sovereignty: NetworkSovereigntyRequirements,
    driver: Arc<WorkloadProvisionDriver>,
    projection: Arc<WorkloadProjectionOrchestrator>,
    in_flight: Arc<Mutex<BTreeMap<WorkloadSagaKey, InFlightProvision>>>,
    #[cfg(test)]
    test_submission_boundary: Mutex<Option<(Arc<std::sync::Barrier>, Arc<std::sync::Barrier>)>>,
    #[cfg(test)]
    test_wait_boundary: Mutex<Option<Arc<tokio::sync::Semaphore>>>,
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
            driver: Arc::new(WorkloadProvisionDriver::new(coordinator, dispatcher)),
            projection: Arc::new(WorkloadProjectionOrchestrator::new(
                provision_capabilities,
                projection_sink,
            )),
            in_flight: Arc::new(Mutex::new(BTreeMap::new())),
            #[cfg(test)]
            test_submission_boundary: Mutex::new(None),
            #[cfg(test)]
            test_wait_boundary: Mutex::new(None),
        })
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
    /// The callback runs only for the winning new insertion. Exact in-flight
    /// replay reuses the tracked result without repeating source mutation.
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
        let receiver = self.track_resume(key, cancellation)?;
        #[cfg(test)]
        self.notify_test_wait_boundary();
        wait_for_completion(receiver, cancellation).await
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
        let mut in_flight = self
            .in_flight
            .lock()
            .expect("workload provision supervisor lock should not be poisoned");
        if let Some(existing) = in_flight.get(&key) {
            if existing.intent.as_ref() == Some(&intent) {
                return Ok(existing.completion.clone());
            }
            return Err(Arc::new(WorkloadProvisionError::CrossedTrackedRequest));
        }
        reserve_source()
            .map_err(WorkloadProvisionError::SourceReservation)
            .map_err(Arc::new)?;
        let (sender, receiver) = watch::channel(None);
        in_flight.insert(
            key.clone(),
            InFlightProvision {
                intent: Some(intent.clone()),
                completion: receiver.clone(),
                _task: None,
            },
        );
        drop(in_flight);
        drop(cancellation_guard);

        let provisioner = Arc::clone(self);
        let task_key = key.clone();
        let task = tokio::spawn(async move {
            let result = match provisioner
                .driver
                .submit_and_drive(task_key.clone(), intent)
                .await
            {
                Ok(run) => {
                    let projection = provisioner.projection.project(&run).await;
                    Ok(WorkloadProvisionOutcome { run, projection })
                }
                Err(error) => Err(Arc::new(WorkloadProvisionError::Run(error))),
            };
            sender.send_replace(Some(result));
            provisioner
                .in_flight
                .lock()
                .expect("workload provision supervisor lock should not be poisoned")
                .remove(&task_key);
        });
        if let Some(entry) = self
            .in_flight
            .lock()
            .expect("workload provision supervisor lock should not be poisoned")
            .get_mut(&key)
        {
            entry._task = Some(task);
        }
        Ok(receiver)
    }

    fn track_resume(
        self: &Arc<Self>,
        key: WorkloadSagaKey,
        cancellation: &WorkloadProvisionCancellation,
    ) -> Result<watch::Receiver<Option<WorkloadProvisionResult>>, Arc<WorkloadProvisionError>> {
        let cancellation_guard = cancellation.signal.borrow();
        if *cancellation_guard {
            return Err(Arc::new(WorkloadProvisionError::CancelledBeforeSubmission));
        }
        let mut in_flight = self
            .in_flight
            .lock()
            .expect("workload provision supervisor lock should not be poisoned");
        if let Some(existing) = in_flight.get(&key) {
            return Ok(existing.completion.clone());
        }
        let (sender, receiver) = watch::channel(None);
        in_flight.insert(
            key.clone(),
            InFlightProvision {
                intent: None,
                completion: receiver.clone(),
                _task: None,
            },
        );
        drop(in_flight);
        drop(cancellation_guard);

        let provisioner = Arc::clone(self);
        let task_key = key.clone();
        let task = tokio::spawn(async move {
            let result = match provisioner.driver.resume(&task_key).await {
                Ok(run) => {
                    let projection = provisioner.projection.project(&run).await;
                    Ok(WorkloadProvisionOutcome { run, projection })
                }
                Err(error) => Err(Arc::new(WorkloadProvisionError::Run(error))),
            };
            sender.send_replace(Some(result));
            provisioner
                .in_flight
                .lock()
                .expect("workload provision supervisor lock should not be poisoned")
                .remove(&task_key);
        });
        if let Some(entry) = self
            .in_flight
            .lock()
            .expect("workload provision supervisor lock should not be poisoned")
            .get_mut(&key)
        {
            entry._task = Some(task);
        }
        Ok(receiver)
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
