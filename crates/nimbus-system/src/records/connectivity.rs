//! Typed, rebuildable connectivity observations for the system tenant.
//!
//! These values contain immutable provider evidence only. They cannot read or
//! mutate network authority, bind an address, publish a route, or decide
//! desired state.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::net::Ipv4Addr;
use std::sync::Arc;

use nimbus_core::{
    AtomicWrite, AtomicWriteBatch, Document, DocumentId, DocumentLocator, Error, Filter, FilterOp,
    PrincipalContext, Query, Result, TenantId, WriteKey, WritePrecondition, WriteSetMode,
};
use nimbus_engine::Engine;
use nimbus_machine::MachineSshPortLeaseIdentity;
use nimbus_network::{
    EndpointProtocol, IngressRouteId, ListenerId, NetworkAttachmentHandle, NetworkCondition,
    NetworkConditionKind, NetworkConditionState, NetworkProviderId, NetworkResourceGeneration,
    NetworkResourceId, NetworkResourcePhase, PortBindRealm, PortBindTarget, PortBoundEndpoint,
    PortLeaseId, PortLeaseRequest, PortProtocol, PublishedEndpointHandle,
};
use nimbus_sandbox::{SandboxProvisionNetworkPlan, SandboxSpec};
use serde_json::{Map, Value, json};

use crate::identity::system_tenant_id;
use crate::keys::{
    connectivity_route_document_id, listener_document_id, port_document_id, service_document_id,
};
use crate::schema::SystemTable;

use super::{
    ensure_system_tenant_async, object_fields, unix_time_millis, upsert_system_document_async,
};

const SERVER_LISTENER_PROJECTION_FENCE_ID: &str = "system:server-listener-projection";
const SERVER_LISTENER_PROJECTION_NAME: &str = "server_listener_projection";
const SERVER_LISTENER_INCARNATION_FIELD: &str = "serverIncarnation";

/// Rejected crossed or incomplete observed connectivity evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemConnectivityObservationError {
    EmptyText(&'static str),
    ListenerOwnerMismatch,
    ListenerLeaseMismatch,
    BindingMismatch,
    DuplicateCondition,
    CleanupConditionMismatch,
    EndpointRouteMismatch,
    EndpointAddressMismatch,
    EndpointGenerationMismatch,
    EndpointProtocolMismatch,
    ServiceGenerationZero,
    ServiceOwnerMissing,
    ServicePlanTenantMismatch,
    ServiceAttachmentMismatch,
    ServicePlanCorrelationMismatch,
    ServiceTenantMismatch,
    ServiceGenerationMismatch,
    DuplicateEndpointIdentity,
    DuplicateEndpointName,
    DuplicateRouteIdentity,
    DuplicateListenerIdentity,
    DuplicatePortLeaseIdentity,
}

impl Display for SystemConnectivityObservationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyText(field) => return write!(formatter, "{field} cannot be empty"),
            Self::ListenerOwnerMismatch => {
                "connectivity listener does not own the supplied port lease"
            }
            Self::ListenerLeaseMismatch => {
                "connectivity listener identity does not derive the supplied port lease identity"
            }
            Self::BindingMismatch => {
                "connectivity listener binding does not satisfy its durable lease request"
            }
            Self::DuplicateCondition => {
                "connectivity observation contains a duplicate condition kind"
            }
            Self::CleanupConditionMismatch => {
                "connectivity cleanup phase and cleanup condition do not agree"
            }
            Self::EndpointRouteMismatch => {
                "connectivity route identity does not derive from its endpoint identity"
            }
            Self::EndpointAddressMismatch => {
                "published endpoint address does not equal its observed listener address"
            }
            Self::EndpointGenerationMismatch => {
                "published endpoint generation does not equal its listener generation"
            }
            Self::EndpointProtocolMismatch => {
                "published endpoint protocol is incompatible with its listener transport"
            }
            Self::ServiceGenerationZero => "service source generation must be greater than zero",
            Self::ServiceOwnerMissing => {
                "service connectivity source is not owned by a sandbox service"
            }
            Self::ServicePlanTenantMismatch => {
                "service connectivity source and compiled plan tenants do not agree"
            }
            Self::ServiceAttachmentMismatch => {
                "service attachment does not equal the compiled plan attachment"
            }
            Self::ServicePlanCorrelationMismatch => {
                "service endpoint evidence does not equal its compiled plan correlation"
            }
            Self::ServiceTenantMismatch => {
                "service endpoint lease tenant does not equal the service tenant"
            }
            Self::ServiceGenerationMismatch => {
                "service attachment, endpoint, and listener generations do not agree"
            }
            Self::DuplicateEndpointIdentity => {
                "service connectivity contains a duplicate endpoint identity"
            }
            Self::DuplicateEndpointName => {
                "service connectivity contains a duplicate endpoint name"
            }
            Self::DuplicateRouteIdentity => {
                "service connectivity contains a duplicate route identity"
            }
            Self::DuplicateListenerIdentity => {
                "service connectivity contains a duplicate listener identity"
            }
            Self::DuplicatePortLeaseIdentity => {
                "service connectivity contains a duplicate port lease identity"
            }
        })
    }
}

impl StdError for SystemConnectivityObservationError {}

/// Exact immutable observation of one port-backed physical listener.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemPortListenerObservation {
    adapter: String,
    application_protocol: String,
    listener_id: ListenerId,
    machine_id: Option<String>,
    port_lease_id: PortLeaseId,
    tenant_id: Option<TenantId>,
    generation: NetworkResourceGeneration,
    lease_epoch: nimbus_network::NetworkLeaseEpoch,
    bound_endpoint: PortBoundEndpoint,
    provider_id: NetworkProviderId,
    observed_phase: NetworkResourcePhase,
    conditions: Vec<NetworkCondition>,
    version: Option<String>,
    error: Option<String>,
    lease_request: Option<PortLeaseRequest>,
}

impl SystemPortListenerObservation {
    #[expect(
        clippy::too_many_arguments,
        reason = "the projection validates each independent identity and observation dimension"
    )]
    pub fn new(
        adapter: impl Into<String>,
        application_protocol: impl Into<String>,
        listener_id: ListenerId,
        request: PortLeaseRequest,
        bound_endpoint: PortBoundEndpoint,
        provider_id: NetworkProviderId,
        observed_phase: NetworkResourcePhase,
        conditions: impl IntoIterator<Item = NetworkCondition>,
    ) -> std::result::Result<Self, SystemConnectivityObservationError> {
        let adapter = adapter.into();
        let application_protocol = application_protocol.into();
        validate_text(&adapter, "listener adapter")?;
        validate_text(&application_protocol, "listener application protocol")?;
        if request.owner_id() != &NetworkResourceId::from(listener_id.clone()) {
            return Err(SystemConnectivityObservationError::ListenerOwnerMismatch);
        }
        if request.lease_id() != &PortLeaseId::for_listener(&listener_id) {
            return Err(SystemConnectivityObservationError::ListenerLeaseMismatch);
        }
        if !bound_endpoint.satisfies(request.binding()) {
            return Err(SystemConnectivityObservationError::BindingMismatch);
        }
        let conditions = validate_conditions(observed_phase, conditions)?;
        let port_lease_id = request.lease_id().clone();
        let tenant_id = request.tenant_id().cloned();
        let generation = request.generation();
        let lease_epoch = request.lease_epoch();
        Ok(Self {
            adapter,
            application_protocol,
            listener_id,
            machine_id: None,
            port_lease_id,
            tenant_id,
            generation,
            lease_epoch,
            bound_endpoint,
            provider_id,
            observed_phase,
            conditions,
            version: None,
            error: None,
            lease_request: Some(request),
        })
    }

    /// Construct the same observed shape from the machine-owned canonical SSH
    /// lease identity. The CLI still owns the request and gvproxy effects.
    pub fn for_machine_ssh(
        identity: &MachineSshPortLeaseIdentity,
        bound_endpoint: PortBoundEndpoint,
        observed_phase: NetworkResourcePhase,
        conditions: impl IntoIterator<Item = NetworkCondition>,
    ) -> std::result::Result<Self, SystemConnectivityObservationError> {
        if bound_endpoint.protocol() != PortProtocol::Tcp
            || bound_endpoint.realm() != &PortBindRealm::Host
            || bound_endpoint.target() != &PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST)
        {
            return Err(SystemConnectivityObservationError::BindingMismatch);
        }
        let conditions = validate_conditions(observed_phase, conditions)?;
        Ok(Self {
            adapter: "machine".to_owned(),
            application_protocol: "ssh".to_owned(),
            listener_id: identity.listener_id().clone(),
            machine_id: None,
            port_lease_id: identity.port_lease_id().clone(),
            tenant_id: None,
            generation: identity.generation(),
            lease_epoch: identity.lease_epoch(),
            bound_endpoint,
            provider_id: identity.provider_id().clone(),
            observed_phase,
            conditions,
            version: None,
            error: None,
            lease_request: None,
        })
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }

    pub fn with_machine_id(mut self, machine_id: impl Into<String>) -> Self {
        self.machine_id = Some(machine_id.into());
        self
    }

    pub fn listener_id(&self) -> &ListenerId {
        &self.listener_id
    }

    pub fn port_lease_id(&self) -> &PortLeaseId {
        &self.port_lease_id
    }

    pub fn tenant_id(&self) -> Option<&TenantId> {
        self.tenant_id.as_ref()
    }

    pub const fn generation(&self) -> NetworkResourceGeneration {
        self.generation
    }

    pub const fn lease_epoch(&self) -> nimbus_network::NetworkLeaseEpoch {
        self.lease_epoch
    }

    pub fn bound_endpoint(&self) -> &PortBoundEndpoint {
        &self.bound_endpoint
    }

    pub fn provider_id(&self) -> &NetworkProviderId {
        &self.provider_id
    }

    pub const fn observed_phase(&self) -> NetworkResourcePhase {
        self.observed_phase
    }

    pub fn conditions(&self) -> &[NetworkCondition] {
        &self.conditions
    }
}

/// Observed non-port listener, such as a machine API Unix socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemUnixListenerObservation {
    adapter: String,
    protocol: String,
    listener_id: ListenerId,
    machine_id: Option<String>,
    generation: NetworkResourceGeneration,
    provider_id: Option<NetworkProviderId>,
    actual_address: String,
    observed_phase: NetworkResourcePhase,
    conditions: Vec<NetworkCondition>,
    version: Option<String>,
    error: Option<String>,
}

impl SystemUnixListenerObservation {
    #[expect(
        clippy::too_many_arguments,
        reason = "the projection validates each independent identity and observation dimension"
    )]
    pub fn new(
        adapter: impl Into<String>,
        protocol: impl Into<String>,
        listener_id: ListenerId,
        generation: NetworkResourceGeneration,
        provider_id: Option<NetworkProviderId>,
        actual_address: impl Into<String>,
        observed_phase: NetworkResourcePhase,
        conditions: impl IntoIterator<Item = NetworkCondition>,
    ) -> std::result::Result<Self, SystemConnectivityObservationError> {
        let adapter = adapter.into();
        let protocol = protocol.into();
        let actual_address = actual_address.into();
        validate_text(&adapter, "listener adapter")?;
        validate_text(&protocol, "listener protocol")?;
        validate_text(&actual_address, "listener actual address")?;
        Ok(Self {
            adapter,
            protocol,
            listener_id,
            machine_id: None,
            generation,
            provider_id,
            actual_address,
            observed_phase,
            conditions: validate_conditions(observed_phase, conditions)?,
            version: None,
            error: None,
        })
    }

    pub fn with_machine_id(mut self, machine_id: impl Into<String>) -> Self {
        self.machine_id = Some(machine_id.into());
        self
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }

    pub fn listener_id(&self) -> &ListenerId {
        &self.listener_id
    }
}

/// One stable endpoint/route and its exact physical listener evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemPublishedEndpointObservation {
    route_id: IngressRouteId,
    endpoint: PublishedEndpointHandle,
    listener: SystemPortListenerObservation,
}

impl SystemPublishedEndpointObservation {
    pub fn new(
        route_id: IngressRouteId,
        endpoint: PublishedEndpointHandle,
        listener: SystemPortListenerObservation,
    ) -> std::result::Result<Self, SystemConnectivityObservationError> {
        if route_id != IngressRouteId::for_published_endpoint(endpoint.endpoint_id()) {
            return Err(SystemConnectivityObservationError::EndpointRouteMismatch);
        }
        if endpoint.generation() != listener.generation {
            return Err(SystemConnectivityObservationError::EndpointGenerationMismatch);
        }
        if endpoint.endpoint().address != listener.bound_endpoint.socket_addr() {
            return Err(SystemConnectivityObservationError::EndpointAddressMismatch);
        }
        if listener.bound_endpoint.protocol() != PortProtocol::Tcp
            || !matches!(
                endpoint.endpoint().protocol,
                EndpointProtocol::Tcp | EndpointProtocol::Http | EndpointProtocol::Https
            )
        {
            return Err(SystemConnectivityObservationError::EndpointProtocolMismatch);
        }
        Ok(Self {
            route_id,
            endpoint,
            listener,
        })
    }

    pub fn route_id(&self) -> &IngressRouteId {
        &self.route_id
    }

    pub fn endpoint(&self) -> &PublishedEndpointHandle {
        &self.endpoint
    }

    pub fn listener(&self) -> &SystemPortListenerObservation {
        &self.listener
    }
}

/// Validated service observation converted to system documents in one place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemServiceConnectivityObservation {
    tenant_id: TenantId,
    service_name: String,
    source_generation: u64,
    attachment: NetworkAttachmentHandle,
    attachment_provider_id: NetworkProviderId,
    observed_phase: NetworkResourcePhase,
    conditions: Vec<NetworkCondition>,
    endpoints: Vec<SystemPublishedEndpointObservation>,
}

impl SystemServiceConnectivityObservation {
    #[expect(
        clippy::too_many_arguments,
        reason = "the service projection validates independent source and network dimensions"
    )]
    pub fn new(
        source: &SandboxSpec,
        plan: &SandboxProvisionNetworkPlan,
        source_generation: u64,
        attachment: NetworkAttachmentHandle,
        attachment_provider_id: NetworkProviderId,
        observed_phase: NetworkResourcePhase,
        conditions: impl IntoIterator<Item = NetworkCondition>,
        endpoints: impl IntoIterator<Item = SystemPublishedEndpointObservation>,
    ) -> std::result::Result<Self, SystemConnectivityObservationError> {
        let service_name = source
            .service_name()
            .ok_or(SystemConnectivityObservationError::ServiceOwnerMissing)?
            .to_owned();
        validate_text(&service_name, "service name")?;
        if source_generation == 0 {
            return Err(SystemConnectivityObservationError::ServiceGenerationZero);
        }
        if plan.tenant_id() != &source.tenant_id {
            return Err(SystemConnectivityObservationError::ServicePlanTenantMismatch);
        }
        let source_bindings_match = source.port_bindings.len() == plan.listeners().len()
            && plan.listeners().iter().all(|planned| {
                let mut matching = source
                    .port_bindings
                    .iter()
                    .filter(|binding| binding.name == planned.binding().name);
                matching.next() == Some(planned.binding()) && matching.next().is_none()
            });
        if !source_bindings_match {
            return Err(SystemConnectivityObservationError::ServicePlanCorrelationMismatch);
        }
        if plan.attachment_id() != attachment.attachment_id()
            || plan.generation() != attachment.generation()
        {
            return Err(SystemConnectivityObservationError::ServiceAttachmentMismatch);
        }
        let tenant_id = source.tenant_id.clone();
        let conditions = validate_conditions(observed_phase, conditions)?;
        let mut endpoints = endpoints.into_iter().collect::<Vec<_>>();
        let mut endpoint_ids = BTreeSet::new();
        let mut endpoint_names = BTreeSet::new();
        let mut route_ids = BTreeSet::new();
        let mut listener_ids = BTreeSet::new();
        let mut lease_ids = BTreeSet::new();
        for endpoint in &endpoints {
            if endpoint.listener.tenant_id.as_ref() != Some(&tenant_id) {
                return Err(SystemConnectivityObservationError::ServiceTenantMismatch);
            }
            if endpoint.endpoint.generation() != attachment.generation()
                || endpoint.listener.generation != attachment.generation()
            {
                return Err(SystemConnectivityObservationError::ServiceGenerationMismatch);
            }
            let Some(planned) = plan
                .listeners()
                .iter()
                .find(|planned| planned.listener_id() == &endpoint.listener.listener_id)
            else {
                return Err(SystemConnectivityObservationError::ServicePlanCorrelationMismatch);
            };
            if planned.endpoint_id() != endpoint.endpoint.endpoint_id()
                || endpoint.listener.lease_request.as_ref() != Some(planned.port_lease())
                || planned.binding().name != endpoint.endpoint.endpoint().name
                || planned.binding().protocol != endpoint.endpoint.endpoint().protocol
                || endpoint.listener.application_protocol
                    != endpoint_protocol_label(planned.binding().protocol)
                || Some(planned.binding().guest_port) != endpoint.endpoint.endpoint().guest_port
            {
                return Err(SystemConnectivityObservationError::ServicePlanCorrelationMismatch);
            }
            if !endpoint_ids.insert(endpoint.endpoint.endpoint_id().clone()) {
                return Err(SystemConnectivityObservationError::DuplicateEndpointIdentity);
            }
            if !endpoint_names.insert(endpoint.endpoint.endpoint().name.clone()) {
                return Err(SystemConnectivityObservationError::DuplicateEndpointName);
            }
            if !route_ids.insert(endpoint.route_id.clone()) {
                return Err(SystemConnectivityObservationError::DuplicateRouteIdentity);
            }
            if !listener_ids.insert(endpoint.listener.listener_id.clone()) {
                return Err(SystemConnectivityObservationError::DuplicateListenerIdentity);
            }
            if !lease_ids.insert(endpoint.listener.port_lease_id.clone()) {
                return Err(SystemConnectivityObservationError::DuplicatePortLeaseIdentity);
            }
        }
        endpoints.sort_by(|left, right| left.route_id.cmp(&right.route_id));
        Ok(Self {
            tenant_id,
            service_name,
            source_generation,
            attachment,
            attachment_provider_id,
            observed_phase,
            conditions,
            endpoints,
        })
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    pub const fn source_generation(&self) -> u64 {
        self.source_generation
    }

    pub fn attachment(&self) -> &NetworkAttachmentHandle {
        &self.attachment
    }

    pub fn attachment_provider_id(&self) -> &NetworkProviderId {
        &self.attachment_provider_id
    }

    pub fn endpoints(&self) -> &[SystemPublishedEndpointObservation] {
        &self.endpoints
    }
}

/// Write one exact physical listener and its derived port observation.
pub async fn record_port_listener_observation_async(
    engine: &Arc<Engine>,
    input: &SystemPortListenerObservation,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    publish_port_listener_fenced_async(engine, input.clone()).await
}

/// Claim the durable listener-inventory projection for one server activation.
///
/// A later activation replaces this marker before it submits its inventory.
/// Retried work from an earlier activation must verify the marker inside its
/// mutation transaction and retire without changing the newer inventory.
pub async fn claim_server_listener_projection_async(
    engine: &Arc<Engine>,
    server_incarnation: &str,
) -> Result<()> {
    if server_incarnation.trim().is_empty() || server_incarnation.chars().any(char::is_control) {
        return Err(Error::InvalidInput(
            "server listener projection incarnation cannot be empty or contain control characters"
                .to_owned(),
        ));
    }
    ensure_system_tenant_async(engine).await?;
    let claimed_at = unix_time_millis()?;
    upsert_system_document_async(
        engine,
        SystemTable::SystemStatus,
        SERVER_LISTENER_PROJECTION_FENCE_ID,
        object_fields(json!({
            "name": SERVER_LISTENER_PROJECTION_NAME,
            "version": env!("CARGO_PKG_VERSION"),
            "health": "ok",
            "startedAt": claimed_at,
            "updatedAt": claimed_at,
            "details": {
                "serverIncarnation": server_incarnation,
            },
        })),
    )
    .await
}

/// Replace the process-owned server listener inventory as one projection.
///
/// A server incarnation uses new authority-bound listener IDs. Replacing the
/// complete ownerless set removes observations from an earlier incarnation
/// without touching machine- or service-owned listener evidence.
pub(crate) async fn replace_server_port_listener_observations_async(
    engine: &Arc<Engine>,
    server_incarnation: &str,
    inputs: &[SystemPortListenerObservation],
) -> Result<()> {
    if inputs.is_empty() {
        return Err(Error::InvalidInput(
            "server listener projection cannot replace its inventory with an empty set".to_owned(),
        ));
    }
    let mut listener_ids = BTreeSet::new();
    let mut port_lease_ids = BTreeSet::new();
    for input in inputs {
        if input.machine_id.is_some() || input.tenant_id.is_some() {
            return Err(Error::InvalidInput(
                "server listener projection cannot consume a machine- or tenant-owned observation"
                    .to_owned(),
            ));
        }
        if !listener_ids.insert(input.listener_id.clone()) {
            return Err(Error::InvalidInput(
                "server listener projection repeats a listener identity".to_owned(),
            ));
        }
        if !port_lease_ids.insert(input.port_lease_id.clone()) {
            return Err(Error::InvalidInput(
                "server listener projection repeats a port lease identity".to_owned(),
            ));
        }
    }

    ensure_system_tenant_async(engine).await?;
    let engine = Arc::clone(engine);
    let server_incarnation = server_incarnation.to_owned();
    let inputs = inputs.to_vec();
    tokio::task::spawn_blocking(move || {
        replace_server_port_listener_observations_once(&engine, &server_incarnation, &inputs)
    })
    .await
    .map_err(|error| Error::Internal(format!("server listener projection task failed: {error}")))?
}

/// Write one observed Unix listener without fabricating a host-port lease.
pub async fn record_unix_listener_observation_async(
    engine: &Arc<Engine>,
    input: &SystemUnixListenerObservation,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    let mut fields = object_fields(json!({
        "listenerId": input.listener_id.as_str(),
        "adapter": input.adapter,
        "protocol": input.protocol,
        "generation": input.generation.as_u64().to_string(),
        "actualAddress": input.actual_address,
        "observedPhase": phase_label(input.observed_phase),
        "conditions": condition_fields(&input.conditions),
        "cleanupState": cleanup_state(input.observed_phase, &input.conditions),
    }));
    insert_optional(&mut fields, "machineId", input.machine_id.as_deref());
    insert_optional(
        &mut fields,
        "providerId",
        input.provider_id.as_ref().map(NetworkProviderId::as_str),
    );
    insert_optional(&mut fields, "version", input.version.as_deref());
    insert_optional(&mut fields, "error", input.error.as_deref());
    upsert_system_document_async(
        engine,
        SystemTable::Listeners,
        &listener_document_id(&input.listener_id),
        fields,
    )
    .await
}

/// Replace one service's rebuildable connectivity observation.
pub async fn record_service_connectivity_observation_async(
    engine: &Arc<Engine>,
    input: &SystemServiceConnectivityObservation,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    publish_service_connectivity_fenced_async(engine, input.clone()).await
}

const MAX_CONNECTIVITY_CONFLICT_ATTEMPTS: usize = 8;

async fn publish_port_listener_fenced_async(
    engine: &Arc<Engine>,
    input: SystemPortListenerObservation,
) -> Result<()> {
    for attempt in 1..=MAX_CONNECTIVITY_CONFLICT_ATTEMPTS {
        let engine = Arc::clone(engine);
        let input = input.clone();
        let result =
            tokio::task::spawn_blocking(move || publish_port_listener_once(&engine, &input))
                .await
                .map_err(|error| {
                    Error::Internal(format!("listener projection task failed: {error}"))
                })?;
        match result {
            Err(error @ Error::Conflict { .. }) if attempt < MAX_CONNECTIVITY_CONFLICT_ATTEMPTS => {
                drop(error);
            }
            Err(error @ Error::Conflict { .. }) => {
                return Err(error.with_conflict_attempts(attempt));
            }
            other => return other,
        }
    }
    unreachable!("the bounded listener projection conflict loop always returns")
}

fn publish_port_listener_once(
    engine: &Arc<Engine>,
    input: &SystemPortListenerObservation,
) -> Result<()> {
    let unit =
        engine.begin_mutation_execution_unit(system_tenant_id()?, PrincipalContext::system())?;
    let listener_table = SystemTable::Listeners.table_name()?;
    let port_table = SystemTable::Ports.table_name()?;
    let listener_id = DocumentId::from_key(listener_document_id(&input.listener_id))?;
    let port_id = DocumentId::from_key(port_document_id(&input.port_lease_id))?;
    let incoming = (input.generation.as_u64(), input.lease_epoch.as_u64());
    if [
        unit.get_document(&listener_table, listener_id.clone())?,
        unit.get_document(&port_table, port_id.clone())?,
    ]
    .into_iter()
    .flatten()
    .map(|document| projection_fence(&document))
    .collect::<Result<Vec<_>>>()?
    .into_iter()
    .any(|current| current > incoming)
    {
        return Ok(());
    }
    let (listener, port) = port_listener_documents(input, None, input.machine_id.as_deref(), None);
    unit.execute_atomic_write_batch(AtomicWriteBatch::new(vec![
        overwrite(listener_table, listener_id, listener),
        overwrite(port_table, port_id, port),
    ])?)?;
    Ok(())
}

fn replace_server_port_listener_observations_once(
    engine: &Arc<Engine>,
    server_incarnation: &str,
    inputs: &[SystemPortListenerObservation],
) -> Result<()> {
    let unit =
        engine.begin_mutation_execution_unit(system_tenant_id()?, PrincipalContext::system())?;
    let status_table = SystemTable::SystemStatus.table_name()?;
    let fence = unit
        .get_document(
            &status_table,
            DocumentId::from_key(SERVER_LISTENER_PROJECTION_FENCE_ID)?,
        )?
        .ok_or_else(|| {
            Error::InvalidInput(
                "server listener projection has no active incarnation claim".to_owned(),
            )
        })?;
    let claimed_incarnation = fence
        .fields
        .get("details")
        .and_then(Value::as_object)
        .and_then(|details| details.get(SERVER_LISTENER_INCARNATION_FIELD))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Error::Serialization(
                "server listener projection claim has no valid server incarnation".to_owned(),
            )
        })?;
    if claimed_incarnation != server_incarnation {
        return Ok(());
    }
    let listener_table = SystemTable::Listeners.table_name()?;
    let port_table = SystemTable::Ports.table_name()?;
    let desired_listener_documents = inputs
        .iter()
        .map(|input| listener_document_id(&input.listener_id))
        .collect::<BTreeSet<_>>();
    let desired_port_documents = inputs
        .iter()
        .map(|input| port_document_id(&input.port_lease_id))
        .collect::<BTreeSet<_>>();
    let mut writes = Vec::new();

    for (table, desired_documents) in [
        (&listener_table, &desired_listener_documents),
        (&port_table, &desired_port_documents),
    ] {
        let documents = unit.query_documents_cancellable(
            &Query {
                table: table.clone(),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            &mut || Ok(()),
        )?;
        for document in documents {
            if is_process_owned_server_listener_document(&document)
                && !desired_documents.contains(document.id.as_str())
            {
                writes.push(AtomicWrite::Delete {
                    key: write_key(table.clone(), document.id),
                    precondition: WritePrecondition::default(),
                    missing_ok: true,
                });
            }
        }
    }

    for input in inputs {
        let (listener, port) = port_listener_documents(input, None, None, None);
        writes.push(overwrite(
            listener_table.clone(),
            DocumentId::from_key(listener_document_id(&input.listener_id))?,
            listener,
        ));
        writes.push(overwrite(
            port_table.clone(),
            DocumentId::from_key(port_document_id(&input.port_lease_id))?,
            port,
        ));
    }
    unit.execute_atomic_write_batch(AtomicWriteBatch::new(writes)?)?;
    Ok(())
}

fn is_process_owned_server_listener_document(document: &Document) -> bool {
    document.fields.get("serviceId").is_none()
        && document.fields.get("machineId").is_none()
        && document.fields.get("tenantId").is_none()
        && document.fields.contains_key("portLeaseId")
}

async fn publish_service_connectivity_fenced_async(
    engine: &Arc<Engine>,
    input: SystemServiceConnectivityObservation,
) -> Result<()> {
    for attempt in 1..=MAX_CONNECTIVITY_CONFLICT_ATTEMPTS {
        let engine = Arc::clone(engine);
        let input = input.clone();
        let result =
            tokio::task::spawn_blocking(move || publish_service_connectivity_once(&engine, &input))
                .await
                .map_err(|error| {
                    Error::Internal(format!("service projection task failed: {error}"))
                })?;
        match result {
            Err(error @ Error::Conflict { .. }) if attempt < MAX_CONNECTIVITY_CONFLICT_ATTEMPTS => {
                drop(error);
            }
            Err(error @ Error::Conflict { .. }) => {
                return Err(error.with_conflict_attempts(attempt));
            }
            other => return other,
        }
    }
    unreachable!("the bounded service projection conflict loop always returns")
}

fn publish_service_connectivity_once(
    engine: &Arc<Engine>,
    input: &SystemServiceConnectivityObservation,
) -> Result<()> {
    let service_id = service_document_id(&input.tenant_id, &input.service_name);
    let unit =
        engine.begin_mutation_execution_unit(system_tenant_id()?, PrincipalContext::system())?;
    let service_table = SystemTable::Services.table_name()?;
    let service_document_id = DocumentId::from_key(service_id.clone())?;
    let current = unit.get_document(&service_table, service_document_id.clone())?;
    let incoming_fence = (
        input.source_generation,
        input.attachment.generation().as_u64(),
    );
    let service_fields = service_document(input);
    if let Some(current) = current.as_ref() {
        let current_fence = (
            required_string_u64(current, "sourceGeneration")?,
            required_string_u64(current, "generation")?,
        );
        if current_fence > incoming_fence {
            return Ok(());
        }
        if current_fence == incoming_fence {
            let current_endpoints = current.fields.get("endpoints");
            let incoming_endpoints = service_fields.get("endpoints");
            if endpoint_membership(current_endpoints)? != endpoint_membership(incoming_endpoints)? {
                return Ok(());
            }
            let incoming_listener_fences = endpoint_listener_fences(incoming_endpoints)?;
            if endpoint_listener_fences(current_endpoints)?.iter().any(
                |(listener_id, current_fence)| {
                    incoming_listener_fences
                        .get(listener_id)
                        .is_none_or(|incoming_fence| current_fence > incoming_fence)
                },
            ) {
                return Ok(());
            }
        }
    }

    let child_tables = [
        SystemTable::Listeners,
        SystemTable::Ports,
        SystemTable::ConnectivityRoutes,
    ];
    let mut existing_children = Vec::new();
    for table in child_tables {
        let table_name = table.table_name()?;
        let documents = unit.query_documents_cancellable(
            &Query {
                table: table_name.clone(),
                filters: vec![Filter {
                    field: "serviceId".to_owned(),
                    op: FilterOp::Eq,
                    value: json!(service_id),
                }],
                order: None,
                limit: None,
            },
            &mut || Ok(()),
        )?;
        existing_children.extend(
            documents
                .into_iter()
                .map(|document| (table_name.clone(), document)),
        );
    }
    let incoming_listener_fences = input
        .endpoints
        .iter()
        .map(|endpoint| {
            (
                endpoint.listener.listener_id.as_str(),
                (
                    endpoint.listener.generation.as_u64(),
                    endpoint.listener.lease_epoch.as_u64(),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    if current.is_some() {
        for (_, child) in &existing_children {
            let Some(listener_id) = child.fields.get("listenerId").and_then(Value::as_str) else {
                continue;
            };
            let Some(incoming) = incoming_listener_fences.get(listener_id) else {
                continue;
            };
            if projection_fence(child)? > *incoming {
                return Ok(());
            }
        }
    }

    let mut writes = Vec::with_capacity(1 + existing_children.len() + input.endpoints.len() * 3);
    writes.push(overwrite(
        service_table,
        service_document_id,
        service_fields,
    ));
    for (table, document) in existing_children {
        writes.push(AtomicWrite::Delete {
            key: write_key(table, document.id),
            precondition: WritePrecondition::default(),
            missing_ok: true,
        });
    }
    for endpoint in &input.endpoints {
        let (listener, port) = port_listener_documents(
            &endpoint.listener,
            Some(&service_id),
            None,
            endpoint.endpoint.endpoint().guest_port,
        );
        writes.push(overwrite(
            SystemTable::Listeners.table_name()?,
            DocumentId::from_key(listener_document_id(&endpoint.listener.listener_id))?,
            listener,
        ));
        writes.push(overwrite(
            SystemTable::Ports.table_name()?,
            DocumentId::from_key(port_document_id(&endpoint.listener.port_lease_id))?,
            port,
        ));
        writes.push(overwrite(
            SystemTable::ConnectivityRoutes.table_name()?,
            DocumentId::from_key(connectivity_route_document_id(&endpoint.route_id))?,
            connectivity_route_document(&service_id, &input.tenant_id, endpoint),
        ));
    }
    unit.execute_atomic_write_batch(AtomicWriteBatch::new(writes)?)?;
    Ok(())
}

fn port_listener_documents(
    input: &SystemPortListenerObservation,
    service_id: Option<&str>,
    machine_id: Option<&str>,
    guest_port: Option<u16>,
) -> (Map<String, Value>, Map<String, Value>) {
    let actual_address = input.bound_endpoint.socket_addr();
    let mut listener = object_fields(json!({
        "listenerId": input.listener_id.as_str(),
        "portLeaseId": input.port_lease_id.as_str(),
        "adapter": input.adapter,
        "protocol": input.application_protocol,
        "generation": input.generation.as_u64().to_string(),
        "leaseEpoch": input.lease_epoch.as_u64().to_string(),
        "providerId": input.provider_id.as_str(),
        "actualAddress": actual_address.to_string(),
        "observedPhase": phase_label(input.observed_phase),
        "conditions": condition_fields(&input.conditions),
        "cleanupState": cleanup_state(input.observed_phase, &input.conditions),
    }));
    insert_optional(&mut listener, "serviceId", service_id);
    insert_optional(&mut listener, "machineId", machine_id);
    insert_optional(
        &mut listener,
        "tenantId",
        input.tenant_id.as_ref().map(TenantId::as_str),
    );
    insert_optional(&mut listener, "version", input.version.as_deref());
    insert_optional(&mut listener, "error", input.error.as_deref());
    let mut port = object_fields(json!({
        "portLeaseId": input.port_lease_id.as_str(),
        "listenerId": input.listener_id.as_str(),
        "generation": input.generation.as_u64().to_string(),
        "leaseEpoch": input.lease_epoch.as_u64().to_string(),
        "providerId": input.provider_id.as_str(),
        "actualAddress": actual_address.to_string(),
        "hostPort": actual_address.port(),
        "protocol": port_protocol_label(input.bound_endpoint.protocol()),
        "observedPhase": phase_label(input.observed_phase),
        "conditions": condition_fields(&input.conditions),
        "cleanupState": cleanup_state(input.observed_phase, &input.conditions),
    }));
    insert_optional(&mut port, "serviceId", service_id);
    insert_optional(&mut port, "machineId", machine_id);
    insert_optional(
        &mut port,
        "tenantId",
        input.tenant_id.as_ref().map(TenantId::as_str),
    );
    if let Some(guest_port) = guest_port {
        port.insert("guestPort".to_owned(), json!(guest_port));
    }
    (listener, port)
}

fn connectivity_route_document(
    service_id: &str,
    tenant_id: &TenantId,
    input: &SystemPublishedEndpointObservation,
) -> Map<String, Value> {
    let endpoint = input.endpoint.endpoint();
    let listener = &input.listener;
    object_fields(json!({
        "routeId": input.route_id.as_str(),
        "serviceId": service_id,
        "tenantId": tenant_id.as_str(),
        "endpointId": input.endpoint.endpoint_id().as_str(),
        "listenerId": listener.listener_id.as_str(),
        "portLeaseId": listener.port_lease_id.as_str(),
        "generation": listener.generation.as_u64().to_string(),
        "leaseEpoch": listener.lease_epoch.as_u64().to_string(),
        "providerId": listener.provider_id.as_str(),
        "protocol": endpoint_protocol_label(endpoint.protocol),
        "actualAddress": endpoint.address.to_string(),
        "observedPhase": phase_label(listener.observed_phase),
        "conditions": condition_fields(&listener.conditions),
        "cleanupState": cleanup_state(listener.observed_phase, &listener.conditions),
    }))
}

fn service_document(input: &SystemServiceConnectivityObservation) -> Map<String, Value> {
    let endpoints = input
        .endpoints
        .iter()
        .map(endpoint_fields)
        .collect::<Vec<_>>();
    object_fields(json!({
        "tenantId": input.tenant_id.as_str(),
        "name": input.service_name,
        "kind": "sandbox",
        "sourceGeneration": input.source_generation.to_string(),
        "attachmentId": input.attachment.attachment_id().as_str(),
        "generation": input.attachment.generation().as_u64().to_string(),
        "attachmentProviderId": input.attachment_provider_id.as_str(),
        "observedPhase": phase_label(input.observed_phase),
        "endpoints": endpoints,
        "conditions": condition_fields(&input.conditions),
        "cleanupState": cleanup_state(input.observed_phase, &input.conditions),
    }))
}

fn projection_fence(document: &Document) -> Result<(u64, u64)> {
    Ok((
        required_string_u64(document, "generation")?,
        required_string_u64(document, "leaseEpoch")?,
    ))
}

fn required_string_u64(document: &Document, field: &str) -> Result<u64> {
    document
        .fields
        .get(field)
        .and_then(Value::as_str)
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| {
            Error::Serialization(format!(
                "connectivity projection {} is missing string-u64 field {field}",
                document.id
            ))
        })
}

fn endpoint_membership(
    endpoints: Option<&Value>,
) -> Result<BTreeSet<(String, String, String, String)>> {
    endpoints
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Serialization("service projection is missing endpoints".to_owned()))?
        .iter()
        .map(|endpoint| {
            let object = endpoint.as_object().ok_or_else(|| {
                Error::Serialization("service endpoint projection is not an object".to_owned())
            })?;
            let field = |name: &str| {
                object
                    .get(name)
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        Error::Serialization(format!(
                            "service endpoint projection is missing {name}"
                        ))
                    })
            };
            Ok((
                field("routeId")?,
                field("endpointId")?,
                field("listenerId")?,
                field("portLeaseId")?,
            ))
        })
        .collect()
}

fn endpoint_listener_fences(endpoints: Option<&Value>) -> Result<BTreeMap<String, (u64, u64)>> {
    endpoints
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Serialization("service projection is missing endpoints".to_owned()))?
        .iter()
        .map(|endpoint| {
            let object = endpoint.as_object().ok_or_else(|| {
                Error::Serialization("service endpoint projection is not an object".to_owned())
            })?;
            let string_field = |name: &str| {
                object.get(name).and_then(Value::as_str).ok_or_else(|| {
                    Error::Serialization(format!("service endpoint projection is missing {name}"))
                })
            };
            let u64_field = |name: &str| {
                string_field(name)?.parse::<u64>().map_err(|_| {
                    Error::Serialization(format!("service endpoint projection has invalid {name}"))
                })
            };
            Ok((
                string_field("listenerId")?.to_owned(),
                (
                    u64_field("listenerGeneration")?,
                    u64_field("listenerLeaseEpoch")?,
                ),
            ))
        })
        .collect()
}

fn overwrite(
    table: nimbus_core::TableName,
    document_id: DocumentId,
    document: Map<String, Value>,
) -> AtomicWrite {
    AtomicWrite::Set {
        key: write_key(table, document_id),
        document,
        typed_fields: Default::default(),
        mode: WriteSetMode::Overwrite,
        precondition: WritePrecondition::default(),
        transforms: Vec::new(),
    }
}

fn write_key(table: nimbus_core::TableName, document_id: DocumentId) -> WriteKey {
    DocumentLocator {
        table,
        id: document_id,
    }
    .into()
}

fn endpoint_fields(input: &SystemPublishedEndpointObservation) -> Value {
    let endpoint = input.endpoint.endpoint();
    let listener = &input.listener;
    json!({
        "routeId": input.route_id.as_str(),
        "endpointId": input.endpoint.endpoint_id().as_str(),
        "listenerId": listener.listener_id.as_str(),
        "portLeaseId": listener.port_lease_id.as_str(),
        "generation": input.endpoint.generation().as_u64().to_string(),
        "listenerGeneration": listener.generation.as_u64().to_string(),
        "listenerLeaseEpoch": listener.lease_epoch.as_u64().to_string(),
        "providerId": listener.provider_id.as_str(),
        "name": endpoint.name,
        "protocol": endpoint_protocol_label(endpoint.protocol),
        "actualAddress": endpoint.address.to_string(),
        "guestPort": endpoint.guest_port,
        "conditions": condition_fields(&listener.conditions),
        "cleanupState": cleanup_state(listener.observed_phase, &listener.conditions),
    })
}

fn validate_text(
    value: &str,
    field: &'static str,
) -> std::result::Result<(), SystemConnectivityObservationError> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        Err(SystemConnectivityObservationError::EmptyText(field))
    } else {
        Ok(())
    }
}

fn validate_conditions(
    phase: NetworkResourcePhase,
    conditions: impl IntoIterator<Item = NetworkCondition>,
) -> std::result::Result<Vec<NetworkCondition>, SystemConnectivityObservationError> {
    let mut conditions = conditions.into_iter().collect::<Vec<_>>();
    conditions.sort_by_key(|condition| condition.kind());
    if conditions
        .windows(2)
        .any(|pair| pair[0].kind() == pair[1].kind())
    {
        return Err(SystemConnectivityObservationError::DuplicateCondition);
    }
    let cleanup = conditions
        .iter()
        .find(|condition| condition.kind() == NetworkConditionKind::CleanupPending)
        .map(|condition| condition.state());
    if (phase == NetworkResourcePhase::CleanupPending
        && cleanup == Some(NetworkConditionState::False))
        || (phase != NetworkResourcePhase::CleanupPending
            && cleanup == Some(NetworkConditionState::True))
    {
        return Err(SystemConnectivityObservationError::CleanupConditionMismatch);
    }
    Ok(conditions)
}

fn condition_fields(conditions: &[NetworkCondition]) -> Vec<Value> {
    conditions
        .iter()
        .map(|condition| {
            json!({
                "kind": condition_kind_label(condition.kind()),
                "state": condition_state_label(condition.state()),
            })
        })
        .collect()
}

fn cleanup_state(phase: NetworkResourcePhase, conditions: &[NetworkCondition]) -> &'static str {
    match conditions
        .iter()
        .find(|condition| condition.kind() == NetworkConditionKind::CleanupPending)
        .map(|condition| condition.state())
    {
        Some(NetworkConditionState::True) => "pending",
        Some(NetworkConditionState::Unknown) => "unknown",
        Some(NetworkConditionState::False) => "clear",
        None if phase == NetworkResourcePhase::CleanupPending => "unknown",
        None => "clear",
    }
}

fn phase_label(phase: NetworkResourcePhase) -> &'static str {
    match phase {
        NetworkResourcePhase::Reserved => "reserved",
        NetworkResourcePhase::Provisioning => "provisioning",
        NetworkResourcePhase::Ready => "ready",
        NetworkResourcePhase::Publishing => "publishing",
        NetworkResourcePhase::Active => "active",
        NetworkResourcePhase::Withdrawing => "withdrawing",
        NetworkResourcePhase::Draining => "draining",
        NetworkResourcePhase::Deleting => "deleting",
        NetworkResourcePhase::CleanupPending => "cleanup_pending",
        NetworkResourcePhase::Released => "released",
        NetworkResourcePhase::Failed => "failed",
    }
}

fn condition_kind_label(kind: NetworkConditionKind) -> &'static str {
    match kind {
        NetworkConditionKind::Ready => "ready",
        NetworkConditionKind::Published => "published",
        NetworkConditionKind::Degraded => "degraded",
        NetworkConditionKind::CleanupPending => "cleanup_pending",
    }
}

fn condition_state_label(state: NetworkConditionState) -> &'static str {
    match state {
        NetworkConditionState::True => "true",
        NetworkConditionState::False => "false",
        NetworkConditionState::Unknown => "unknown",
    }
}

fn endpoint_protocol_label(protocol: EndpointProtocol) -> &'static str {
    match protocol {
        EndpointProtocol::Tcp => "tcp",
        EndpointProtocol::Http => "http",
        EndpointProtocol::Https => "https",
    }
}

fn port_protocol_label(protocol: PortProtocol) -> &'static str {
    match protocol {
        PortProtocol::Tcp => "tcp",
        PortProtocol::Udp => "udp",
    }
}

fn insert_optional(fields: &mut Map<String, Value>, name: &str, value: Option<&str>) {
    if let Some(value) = value {
        fields.insert(name.to_owned(), json!(value));
    }
}
