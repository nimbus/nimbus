//! Typed, rebuildable connectivity observations for the system tenant.
//!
//! These values contain immutable provider evidence only. They cannot read or
//! mutate network authority, bind an address, publish a route, or decide
//! desired state.

use std::collections::BTreeSet;
use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::net::Ipv4Addr;
use std::sync::Arc;

use nimbus_core::{Result, TenantId};
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
    ensure_system_tenant_async, object_fields, query_system_documents_by_eq_async,
    upsert_system_document_async,
};

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
    record_port_listener_documents_async(engine, input, None, input.machine_id.as_deref(), None)
        .await
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
    let service_id = service_document_id(&input.tenant_id, &input.service_name);
    delete_service_connectivity_children_async(engine, &service_id).await?;

    let endpoints = input
        .endpoints
        .iter()
        .map(endpoint_fields)
        .collect::<Vec<_>>();
    upsert_system_document_async(
        engine,
        SystemTable::Services,
        &service_id,
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
        })),
    )
    .await?;

    for endpoint in &input.endpoints {
        let guest_port = endpoint.endpoint.endpoint().guest_port;
        record_port_listener_documents_async(
            engine,
            &endpoint.listener,
            Some(&service_id),
            None,
            guest_port,
        )
        .await?;
        record_connectivity_route_async(engine, &service_id, &input.tenant_id, endpoint).await?;
    }
    Ok(())
}

async fn record_port_listener_documents_async(
    engine: &Arc<Engine>,
    input: &SystemPortListenerObservation,
    service_id: Option<&str>,
    machine_id: Option<&str>,
    guest_port: Option<u16>,
) -> Result<()> {
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
    upsert_system_document_async(
        engine,
        SystemTable::Listeners,
        &listener_document_id(&input.listener_id),
        listener,
    )
    .await?;

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
    upsert_system_document_async(
        engine,
        SystemTable::Ports,
        &port_document_id(&input.port_lease_id),
        port,
    )
    .await
}

async fn record_connectivity_route_async(
    engine: &Arc<Engine>,
    service_id: &str,
    tenant_id: &TenantId,
    input: &SystemPublishedEndpointObservation,
) -> Result<()> {
    let endpoint = input.endpoint.endpoint();
    let listener = &input.listener;
    upsert_system_document_async(
        engine,
        SystemTable::ConnectivityRoutes,
        &connectivity_route_document_id(&input.route_id),
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
        })),
    )
    .await
}

async fn delete_service_connectivity_children_async(
    engine: &Arc<Engine>,
    service_id: &str,
) -> Result<()> {
    let tenant_id = system_tenant_id()?;
    for table in [
        SystemTable::Listeners,
        SystemTable::Ports,
        SystemTable::ConnectivityRoutes,
    ] {
        let table_name = table.table_name()?;
        let documents =
            query_system_documents_by_eq_async(engine, table, [("serviceId", json!(service_id))])
                .await?;
        for document in documents {
            engine
                .delete_document_async(tenant_id.clone(), table_name.clone(), document.id)
                .await?;
        }
    }
    Ok(())
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
