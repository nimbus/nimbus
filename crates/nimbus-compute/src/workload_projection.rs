//! Exact read-only provider observation and services-owned workload projection.
//!
//! Projection runs only after portable saga truth reaches `Observed`. Provider
//! evidence stays ephemeral: compute authenticates it against the exact
//! source, execution, plan, listener, lease, generation, and lifetime before a
//! narrow sink may update an observed projection. No observation grants
//! restart, repair, bind, retry, or journal authority.

use std::collections::BTreeSet;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;

use nimbus_network::{
    ListenerId, NetworkPlanDigest, NetworkPlanId, NetworkResourceGeneration, PortAddressFamily,
    PortBindRealm, PortBindingProvenance, PortBoundEndpoint, PortLeaseId, PortLeaseLifetime,
    PortProtocol, PublishedEndpoint, PublishedEndpointId,
};
use nimbus_sandbox::{SandboxHandle, SandboxId, SandboxInspection, SandboxStatus};
use nimbus_services::ServiceManager;
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, WorkloadExecutableIntent, WorkloadExecutionReference,
    WorkloadProvisionSourceEvidence, WorkloadProvisionSourceGeneration,
    WorkloadProvisionSourceIdentity, WorkloadProvisionSourceKind,
    WorkloadProvisionSourceResourceVersion, WorkloadPublicationIntent,
    WorkloadPublicationReference, WorkloadSagaKey, WorkloadSagaPhase, WorkloadSagaRecord,
};

use crate::workload_executable::decode_sandbox_spec;
use crate::workload_saga::{
    WorkloadProvisionCapabilityRegistry, WorkloadProvisionRun, WorkloadProvisionRunDisposition,
};

/// Closed result of one read-only provider observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadProviderObservation<Value> {
    Present(Value),
    Absent,
    InProgress,
    Ambiguous,
}

/// Complete compute-authenticated input for one execution observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadExecutionObservationRequest {
    key: WorkloadSagaKey,
    execution: WorkloadExecutionReference,
    source: WorkloadProvisionSourceEvidence,
    executable: WorkloadExecutableIntent,
}

impl WorkloadExecutionObservationRequest {
    pub(crate) fn for_record(record: &WorkloadSagaRecord) -> Self {
        let intent = record.active_intent();
        Self {
            key: record.key().clone(),
            execution: record.current_execution_reference(),
            source: intent.source().clone(),
            executable: intent.executable().clone(),
        }
    }

    pub fn key(&self) -> &WorkloadSagaKey {
        &self.key
    }

    pub fn execution(&self) -> &WorkloadExecutionReference {
        &self.execution
    }

    pub fn source(&self) -> &WorkloadProvisionSourceEvidence {
        &self.source
    }

    pub fn executable(&self) -> &WorkloadExecutableIntent {
        &self.executable
    }
}

/// One asynchronous execution-observation read.
pub type WorkloadExecutionObservationFuture<'a> =
    Pin<Box<dyn Future<Output = WorkloadProviderObservation<SandboxInspection>> + Send + 'a>>;

/// Read-only observation capability for one exact execution provider.
pub trait WorkloadExecutionObservationCapability: Send + Sync {
    fn observe<'a>(
        &'a self,
        request: &'a WorkloadExecutionObservationRequest,
    ) -> WorkloadExecutionObservationFuture<'a>;
}

/// Provider evidence tying one actual bind to its stable control-plane IDs.
///
/// This value deliberately omits the opaque provider handle. It is ephemeral
/// comparison evidence, not portable desired state or durable effect authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadIngressBindingWitness {
    plan_id: NetworkPlanId,
    plan_digest: NetworkPlanDigest,
    generation: NetworkResourceGeneration,
    listener_id: ListenerId,
    port_lease_id: PortLeaseId,
    lease_lifetime: PortLeaseLifetime,
    binding_lifetime: PortLeaseLifetime,
    bound_endpoint: PortBoundEndpoint,
    provenance: PortBindingProvenance,
}

impl WorkloadIngressBindingWitness {
    #[expect(
        clippy::too_many_arguments,
        reason = "the witness freezes every independent bind-fencing dimension"
    )]
    pub fn new(
        plan_id: NetworkPlanId,
        plan_digest: NetworkPlanDigest,
        generation: NetworkResourceGeneration,
        listener_id: ListenerId,
        port_lease_id: PortLeaseId,
        lease_lifetime: PortLeaseLifetime,
        binding_lifetime: PortLeaseLifetime,
        bound_endpoint: PortBoundEndpoint,
        provenance: PortBindingProvenance,
    ) -> Self {
        Self {
            plan_id,
            plan_digest,
            generation,
            listener_id,
            port_lease_id,
            lease_lifetime,
            binding_lifetime,
            bound_endpoint,
            provenance,
        }
    }

    pub fn plan_id(&self) -> &NetworkPlanId {
        &self.plan_id
    }

    pub const fn plan_digest(&self) -> NetworkPlanDigest {
        self.plan_digest
    }

    pub const fn generation(&self) -> NetworkResourceGeneration {
        self.generation
    }

    pub fn listener_id(&self) -> &ListenerId {
        &self.listener_id
    }

    pub fn port_lease_id(&self) -> &PortLeaseId {
        &self.port_lease_id
    }

    pub const fn lifetime(&self) -> PortLeaseLifetime {
        self.lease_lifetime
    }

    /// Lifetime reported by the concrete live binding owner.
    pub const fn binding_lifetime(&self) -> PortLeaseLifetime {
        self.binding_lifetime
    }

    pub fn bound_endpoint(&self) -> &PortBoundEndpoint {
        &self.bound_endpoint
    }

    pub const fn provenance(&self) -> PortBindingProvenance {
        self.provenance
    }
}

/// One actual reachable endpoint plus the exact bind witness behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadObservedIngressEndpoint {
    endpoint_id: PublishedEndpointId,
    published_address: SocketAddr,
    binding: WorkloadIngressBindingWitness,
}

impl WorkloadObservedIngressEndpoint {
    pub fn new(
        endpoint_id: PublishedEndpointId,
        published_address: SocketAddr,
        binding: WorkloadIngressBindingWitness,
    ) -> Self {
        Self {
            endpoint_id,
            published_address,
            binding,
        }
    }

    pub fn endpoint_id(&self) -> &PublishedEndpointId {
        &self.endpoint_id
    }

    pub const fn published_address(&self) -> SocketAddr {
        self.published_address
    }

    pub fn binding(&self) -> &WorkloadIngressBindingWitness {
        &self.binding
    }
}

/// Complete exact input for the selected ingress provider's read-only query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadIngressObservationRequest {
    key: WorkloadSagaKey,
    execution: WorkloadExecutionReference,
    publication: WorkloadPublicationReference,
    compiled_plan: CompiledWorkloadNetworkPlan,
}

impl WorkloadIngressObservationRequest {
    fn for_record(record: &WorkloadSagaRecord, publication: WorkloadPublicationReference) -> Self {
        let intent = record.active_intent();
        Self {
            key: record.key().clone(),
            execution: record.current_execution_reference(),
            publication,
            compiled_plan: intent.network().compiled_plan().clone(),
        }
    }

    pub fn key(&self) -> &WorkloadSagaKey {
        &self.key
    }

    pub fn execution(&self) -> &WorkloadExecutionReference {
        &self.execution
    }

    pub fn publication(&self) -> &WorkloadPublicationReference {
        &self.publication
    }

    pub fn compiled_plan(&self) -> &CompiledWorkloadNetworkPlan {
        &self.compiled_plan
    }
}

/// One asynchronous ingress-observation read.
pub type WorkloadIngressObservationFuture<'a> = Pin<
    Box<
        dyn Future<Output = WorkloadProviderObservation<Vec<WorkloadObservedIngressEndpoint>>>
            + Send
            + 'a,
    >,
>;

/// Read-only observation capability for one exact ingress provider.
pub trait WorkloadIngressObservationCapability: Send + Sync {
    fn observe<'a>(
        &'a self,
        request: &'a WorkloadIngressObservationRequest,
    ) -> WorkloadIngressObservationFuture<'a>;
}

/// Exact services-owned observed projection after provider evidence closes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadObservedProjection {
    key: WorkloadSagaKey,
    source_identity: WorkloadProvisionSourceIdentity,
    source_generation: WorkloadProvisionSourceGeneration,
    source_resource_version: WorkloadProvisionSourceResourceVersion,
    execution: WorkloadExecutionReference,
    handle: SandboxHandle,
}

impl WorkloadObservedProjection {
    pub fn key(&self) -> &WorkloadSagaKey {
        &self.key
    }

    pub fn source_identity(&self) -> &WorkloadProvisionSourceIdentity {
        &self.source_identity
    }

    pub const fn source_generation(&self) -> WorkloadProvisionSourceGeneration {
        self.source_generation
    }

    pub fn source_resource_version(&self) -> &WorkloadProvisionSourceResourceVersion {
        &self.source_resource_version
    }

    pub fn execution(&self) -> &WorkloadExecutionReference {
        &self.execution
    }

    pub fn handle(&self) -> &SandboxHandle {
        &self.handle
    }
}

/// Projection sink outcome. Unavailability is replayable; a crossed or stale
/// precondition is a durable rejection of this projection candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadProjectionSinkError {
    Unavailable { reason: String },
    Rejected { reason: String },
}

impl WorkloadProjectionSinkError {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self::Unavailable {
            reason: reason.into(),
        }
    }

    pub fn rejected(reason: impl Into<String>) -> Self {
        Self::Rejected {
            reason: reason.into(),
        }
    }

    pub fn reason(&self) -> &str {
        match self {
            Self::Unavailable { reason } | Self::Rejected { reason } => reason,
        }
    }
}

pub type WorkloadProjectionSinkFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), WorkloadProjectionSinkError>> + Send + 'a>>;

/// Narrow downstream projection substitution. Services remains the desired
/// and observed state owner; compute owns only choreography.
pub trait WorkloadProjectionSink: Send + Sync {
    fn project<'a>(
        &'a self,
        projection: &'a WorkloadObservedProjection,
    ) -> WorkloadProjectionSinkFuture<'a>;
}

/// Compute-to-services projection adapter with no reverse dependency.
pub struct ServiceManagerWorkloadProjectionSink {
    manager: Arc<ServiceManager>,
}

impl ServiceManagerWorkloadProjectionSink {
    pub fn new(manager: Arc<ServiceManager>) -> Self {
        Self { manager }
    }
}

impl WorkloadProjectionSink for ServiceManagerWorkloadProjectionSink {
    fn project<'a>(
        &'a self,
        projection: &'a WorkloadObservedProjection,
    ) -> WorkloadProjectionSinkFuture<'a> {
        Box::pin(async move {
            let tenant_id = projection.key().tenant_id();
            let source = projection.source_identity();
            let result = match source.kind() {
                WorkloadProvisionSourceKind::StandaloneSandbox => self
                    .manager
                    .project_sandbox_resource_execution_observation(
                        tenant_id,
                        source.stable_name(),
                        projection.source_generation().as_u64(),
                        projection.source_resource_version().as_str(),
                        projection.execution(),
                        projection.handle().clone(),
                    )
                    .map(|_| ()),
                WorkloadProvisionSourceKind::SandboxBackedService => self
                    .manager
                    .project_service_definition_execution_observation(
                        tenant_id,
                        source.stable_name(),
                        projection.source_generation().as_u64(),
                        projection.source_resource_version().as_str(),
                        projection.execution(),
                        projection.handle().clone(),
                    )
                    .map(|_| ()),
            };
            result.map_err(|error| match error {
                nimbus_core::Error::Cancelled
                | nimbus_core::Error::Overloaded { .. }
                | nimbus_core::Error::Storage { .. }
                | nimbus_core::Error::Transport(_)
                | nimbus_core::Error::Internal(_) => {
                    WorkloadProjectionSinkError::unavailable(error.to_string())
                }
                _ => WorkloadProjectionSinkError::rejected(error.to_string()),
            })
        })
    }
}

/// Why provider evidence is not yet closed enough to project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadProjectionPendingReason {
    ProvisionWaiting,
    ExecutionAbsent,
    ExecutionInProgress,
    ExecutionAmbiguous,
    IngressAbsent,
    IngressInProgress,
    IngressAmbiguous,
    ProjectionSinkUnavailable,
}

/// Why an observed run was rejected before any sink mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadProjectionRejectedReason {
    ProvisionDefiniteFailure,
    DurableRecordNotObserved,
    MissingExecutionObservationCapability,
    MissingIngressObservationCapability,
    InvalidExecutionEvidence,
    InvalidPublicationReference,
    InvalidIngressEvidence,
    WithheldPublicationCarriedEndpoints,
    ProjectionSinkRejected,
}

/// Product-visible projection state paired with durable portable truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadProjectionState {
    Projected,
    Pending(WorkloadProjectionPendingReason),
    Rejected(WorkloadProjectionRejectedReason),
}

/// Compute-owned exact observer and downstream projection choreography.
pub struct WorkloadProjectionOrchestrator {
    capabilities: Arc<WorkloadProvisionCapabilityRegistry>,
    sink: Arc<dyn WorkloadProjectionSink>,
}

impl WorkloadProjectionOrchestrator {
    pub fn new(
        capabilities: Arc<WorkloadProvisionCapabilityRegistry>,
        sink: Arc<dyn WorkloadProjectionSink>,
    ) -> Self {
        Self { capabilities, sink }
    }

    pub async fn project(&self, run: &WorkloadProvisionRun) -> WorkloadProjectionState {
        self.project_record(run.record(), run.disposition()).await
    }

    async fn project_record(
        &self,
        record: &WorkloadSagaRecord,
        disposition: WorkloadProvisionRunDisposition,
    ) -> WorkloadProjectionState {
        match disposition {
            WorkloadProvisionRunDisposition::Waiting => {
                return WorkloadProjectionState::Pending(
                    WorkloadProjectionPendingReason::ProvisionWaiting,
                );
            }
            WorkloadProvisionRunDisposition::DefiniteFailure => {
                return WorkloadProjectionState::Rejected(
                    WorkloadProjectionRejectedReason::ProvisionDefiniteFailure,
                );
            }
            WorkloadProvisionRunDisposition::Observed => {}
        }
        if record.phase() != WorkloadSagaPhase::Observed {
            return WorkloadProjectionState::Rejected(
                WorkloadProjectionRejectedReason::DurableRecordNotObserved,
            );
        }

        let intent = record.active_intent();
        let execution_request = WorkloadExecutionObservationRequest::for_record(record);
        let inspection = match self
            .capabilities
            .observe_execution(intent.source().execution_provider_id(), &execution_request)
            .await
        {
            Ok(WorkloadProviderObservation::Present(inspection)) => inspection,
            Ok(WorkloadProviderObservation::Absent) => {
                return WorkloadProjectionState::Pending(
                    WorkloadProjectionPendingReason::ExecutionAbsent,
                );
            }
            Ok(WorkloadProviderObservation::InProgress) => {
                return WorkloadProjectionState::Pending(
                    WorkloadProjectionPendingReason::ExecutionInProgress,
                );
            }
            Ok(WorkloadProviderObservation::Ambiguous) => {
                return WorkloadProjectionState::Pending(
                    WorkloadProjectionPendingReason::ExecutionAmbiguous,
                );
            }
            Err(_) => {
                return WorkloadProjectionState::Rejected(
                    WorkloadProjectionRejectedReason::MissingExecutionObservationCapability,
                );
            }
        };

        let mut handle = match validate_execution_observation(record, inspection) {
            Ok(handle) => handle,
            Err(reason) => return WorkloadProjectionState::Rejected(reason),
        };
        match intent.publication() {
            WorkloadPublicationIntent::Withheld => {
                if !handle.published_endpoints.is_empty() {
                    return WorkloadProjectionState::Rejected(
                        WorkloadProjectionRejectedReason::WithheldPublicationCarriedEndpoints,
                    );
                }
            }
            WorkloadPublicationIntent::PublishWhenReady => {
                let references = record.phase_detail().references();
                let Some(publication) = references.publication().cloned() else {
                    return WorkloadProjectionState::Rejected(
                        WorkloadProjectionRejectedReason::InvalidPublicationReference,
                    );
                };
                let content = intent.network().compiled_plan().content();
                let Some(selection) = content.capability_selection() else {
                    return WorkloadProjectionState::Rejected(
                        WorkloadProjectionRejectedReason::InvalidPublicationReference,
                    );
                };
                let request = WorkloadIngressObservationRequest::for_record(record, publication);
                let endpoints = match self
                    .capabilities
                    .observe_ingress(selection.ingress_provider_id(), &request)
                    .await
                {
                    Ok(WorkloadProviderObservation::Present(endpoints)) => endpoints,
                    Ok(WorkloadProviderObservation::Absent) => {
                        return WorkloadProjectionState::Pending(
                            WorkloadProjectionPendingReason::IngressAbsent,
                        );
                    }
                    Ok(WorkloadProviderObservation::InProgress) => {
                        return WorkloadProjectionState::Pending(
                            WorkloadProjectionPendingReason::IngressInProgress,
                        );
                    }
                    Ok(WorkloadProviderObservation::Ambiguous) => {
                        return WorkloadProjectionState::Pending(
                            WorkloadProjectionPendingReason::IngressAmbiguous,
                        );
                    }
                    Err(_) => {
                        return WorkloadProjectionState::Rejected(
                            WorkloadProjectionRejectedReason::MissingIngressObservationCapability,
                        );
                    }
                };
                handle.published_endpoints = match validate_ingress_observation(&request, endpoints)
                {
                    Ok(endpoints) => endpoints,
                    Err(reason) => return WorkloadProjectionState::Rejected(reason),
                };
            }
        }

        let projection = WorkloadObservedProjection {
            key: record.key().clone(),
            source_identity: intent.source().source_identity().clone(),
            source_generation: intent.source().source_generation(),
            source_resource_version: intent.source().resource_version().clone(),
            execution: record.current_execution_reference(),
            handle,
        };
        match self.sink.project(&projection).await {
            Ok(()) => WorkloadProjectionState::Projected,
            Err(WorkloadProjectionSinkError::Unavailable { .. }) => {
                WorkloadProjectionState::Pending(
                    WorkloadProjectionPendingReason::ProjectionSinkUnavailable,
                )
            }
            Err(WorkloadProjectionSinkError::Rejected { .. }) => WorkloadProjectionState::Rejected(
                WorkloadProjectionRejectedReason::ProjectionSinkRejected,
            ),
        }
    }
}

fn validate_execution_observation(
    record: &WorkloadSagaRecord,
    inspection: SandboxInspection,
) -> Result<SandboxHandle, WorkloadProjectionRejectedReason> {
    let intent = record.active_intent();
    let execution = record.current_execution_reference();
    let expected_attempt =
        nimbus_sandbox::SandboxExecutionAttemptId::new(execution.attempt_id().to_string())
            .map_err(|_| WorkloadProjectionRejectedReason::InvalidExecutionEvidence)?;
    if !matches!(
        &inspection.execution_attempt,
        nimbus_sandbox::SandboxExecutionAttemptObservation::Exact(observed)
            if observed == &expected_attempt
    ) {
        return Err(WorkloadProjectionRejectedReason::InvalidExecutionEvidence);
    }
    let spec = decode_sandbox_spec(intent.executable())
        .map_err(|_| WorkloadProjectionRejectedReason::InvalidExecutionEvidence)?;
    let handle = inspection.handle;
    if handle.tenant_id != *record.key().tenant_id()
        || handle.id != SandboxId::new(execution.execution_id().as_str())
        || handle.name != spec.display_name()
        || handle.backend != spec.backend
        || (intent.activation() == nimbus_workloads::WorkloadActivationIntent::ActivateWhenAttached
            && handle.status != SandboxStatus::Ready)
    {
        return Err(WorkloadProjectionRejectedReason::InvalidExecutionEvidence);
    }
    Ok(handle)
}

fn validate_ingress_observation(
    request: &WorkloadIngressObservationRequest,
    observations: Vec<WorkloadObservedIngressEndpoint>,
) -> Result<Vec<PublishedEndpoint>, WorkloadProjectionRejectedReason> {
    let publication = request.publication();
    let plan = request.compiled_plan();
    let content = plan.content();
    if observations.len() != publication.endpoints().len() || observations.is_empty() {
        return Err(WorkloadProjectionRejectedReason::InvalidIngressEvidence);
    }

    let expected_endpoint_ids = publication
        .endpoints()
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let compiled_endpoint_ids = content
        .listeners()
        .iter()
        .map(|listener| listener.endpoint_id().clone())
        .collect::<BTreeSet<_>>();
    if expected_endpoint_ids.len() != publication.endpoints().len()
        || compiled_endpoint_ids.len() != content.listeners().len()
        || expected_endpoint_ids != compiled_endpoint_ids
    {
        return Err(WorkloadProjectionRejectedReason::InvalidIngressEvidence);
    }

    let mut endpoint_ids = BTreeSet::new();
    let mut listener_ids = BTreeSet::new();
    let mut lease_ids = BTreeSet::new();
    let mut published = Vec::with_capacity(observations.len());
    for observation in observations {
        if !endpoint_ids.insert(observation.endpoint_id.clone())
            || !listener_ids.insert(observation.binding.listener_id.clone())
            || !lease_ids.insert(observation.binding.port_lease_id.clone())
            || !expected_endpoint_ids.contains(&observation.endpoint_id)
        {
            return Err(WorkloadProjectionRejectedReason::InvalidIngressEvidence);
        }
        let Some(blueprint) = content
            .listeners()
            .iter()
            .find(|candidate| candidate.endpoint_id() == &observation.endpoint_id)
        else {
            return Err(WorkloadProjectionRejectedReason::InvalidIngressEvidence);
        };
        let binding = &observation.binding;
        if binding.plan_id() != plan.plan().plan_id()
            || binding.plan_digest() != plan.plan().digest()
            || binding.generation() != content.identity().generation()
            || binding.listener_id() != blueprint.listener_id()
            || binding.port_lease_id() != blueprint.port_lease_id()
            || binding.bound_endpoint().protocol() != PortProtocol::Tcp
            || binding.bound_endpoint().realm() != &PortBindRealm::Host
            || binding.lifetime() != binding.binding_lifetime()
            || binding.bound_endpoint().port().get() != observation.published_address.port()
            || !binding_target_matches(
                blueprint.desired_host_address(),
                binding.bound_endpoint(),
                observation.published_address,
            )
            || !binding_provenance_matches(blueprint.port_request(), binding.provenance())
        {
            return Err(WorkloadProjectionRejectedReason::InvalidIngressEvidence);
        }
        match blueprint.port_request().exact_port() {
            Some(expected) if expected.get() != observation.published_address.port() => {
                return Err(WorkloadProjectionRejectedReason::InvalidIngressEvidence);
            }
            Some(_) | None => {}
        }
        let mut endpoint = PublishedEndpoint::new(
            blueprint.name(),
            blueprint.protocol(),
            observation.published_address,
        );
        if let Some(guest_port) = blueprint.guest_port() {
            endpoint = endpoint.with_guest_port(guest_port);
        }
        published.push(endpoint);
    }
    if endpoint_ids != compiled_endpoint_ids {
        return Err(WorkloadProjectionRejectedReason::InvalidIngressEvidence);
    }
    published.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(published)
}

fn binding_target_matches(
    desired: IpAddr,
    bound: &PortBoundEndpoint,
    published: SocketAddr,
) -> bool {
    if published.port() == 0 || published.ip().is_unspecified() {
        return false;
    }
    let target = bound.target();
    if desired.is_unspecified() {
        let expected_family = match desired {
            IpAddr::V4(_) => PortAddressFamily::Ipv4,
            IpAddr::V6(_) => PortAddressFamily::Ipv6,
        };
        return target.is_wildcard()
            && target.family() == Some(expected_family)
            && published.is_ipv4() == matches!(expected_family, PortAddressFamily::Ipv4);
    }
    target.specific_address() == Some(desired) && published.ip() == desired
}

fn binding_provenance_matches(
    request: nimbus_workloads::WorkloadNetworkPortRequestMode,
    provenance: PortBindingProvenance,
) -> bool {
    match request {
        nimbus_workloads::WorkloadNetworkPortRequestMode::Exact { .. } => matches!(
            provenance,
            PortBindingProvenance::NimbusOwned | PortBindingProvenance::ExternallyOwned
        ),
        nimbus_workloads::WorkloadNetworkPortRequestMode::ProviderAssigned => {
            provenance == PortBindingProvenance::ProviderAssigned
        }
    }
}

#[cfg(test)]
#[path = "workload_projection/tests.rs"]
mod tests;
