//! Provider-local idempotency for compute-issued provision attempts.
//!
//! This module deliberately knows nothing about the workload saga. It stores
//! only the provider's stable authority key and complete opaque fences supplied
//! by its adapter. The upper coordinator remains the sole owner of lifecycle
//! order and durable desired state.

use std::collections::BTreeSet;
use std::net::IpAddr;
use std::num::NonZeroU16;

use nimbus_core::TenantId;
use nimbus_network::{
    ListenerId, NetworkAttachmentHandle, NetworkAttachmentId, NetworkPlan, NetworkPlanId,
    NetworkProviderId, NetworkReservationClaim, NetworkResourceGeneration, NetworkResourceId,
    PortBindRealm, PortBindTarget, PortExposure, PortIpv6Overlap, PortLeaseAccounting, PortLeaseId,
    PortLeaseRequest, PortProtocol, PortPublicationIntent, PortRequestMode, PublishedEndpoint,
    PublishedEndpointHandle, PublishedEndpointId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{SandboxNetworkStatus, SandboxNetworkStatusError, SandboxPortBinding};

mod activation;
pub(crate) use activation::{
    ProvisionActivationObservationKind, ProvisionActivationRuntimeState,
    classify_provision_activation,
};

/// One exact published-listener reservation supplied by the compiled plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxProvisionListener {
    endpoint_id: PublishedEndpointId,
    listener_id: ListenerId,
    binding: SandboxPortBinding,
    port_lease: PortLeaseRequest,
}

/// Canonical compiler mapping between one listener and published endpoint.
///
/// The mapping is retained separately from the provider reservation input so
/// sandbox validation can reject a unique but crossed endpoint identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxProvisionEndpointIdentity {
    listener_id: ListenerId,
    endpoint_id: PublishedEndpointId,
}

impl SandboxProvisionEndpointIdentity {
    pub fn new(listener_id: ListenerId, endpoint_id: PublishedEndpointId) -> Self {
        Self {
            listener_id,
            endpoint_id,
        }
    }

    pub fn listener_id(&self) -> &ListenerId {
        &self.listener_id
    }

    pub fn endpoint_id(&self) -> &PublishedEndpointId {
        &self.endpoint_id
    }
}

impl SandboxProvisionListener {
    pub fn new(
        endpoint_id: PublishedEndpointId,
        listener_id: ListenerId,
        binding: SandboxPortBinding,
        port_lease: PortLeaseRequest,
    ) -> Self {
        Self {
            endpoint_id,
            listener_id,
            binding,
            port_lease,
        }
    }

    pub fn endpoint_id(&self) -> &PublishedEndpointId {
        &self.endpoint_id
    }

    pub fn listener_id(&self) -> &ListenerId {
        &self.listener_id
    }

    pub fn binding(&self) -> &SandboxPortBinding {
        &self.binding
    }

    pub fn port_lease(&self) -> &PortLeaseRequest {
        &self.port_lease
    }
}

/// One exact non-published listener whose provider readiness gates activation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxProvisionDependencyListener {
    listener_id: ListenerId,
    name: String,
    provider_id: NetworkProviderId,
}

impl SandboxProvisionDependencyListener {
    pub fn new(
        listener_id: ListenerId,
        name: impl Into<String>,
        provider_id: NetworkProviderId,
    ) -> Self {
        Self {
            listener_id,
            name: name.into(),
            provider_id,
        }
    }

    pub fn listener_id(&self) -> &ListenerId {
        &self.listener_id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn provider_id(&self) -> &NetworkProviderId {
        &self.provider_id
    }
}

/// Provider-neutral exact reservation input for one sandbox provision attempt.
///
/// The compiled-plan owner supplies every stable identity. Sandbox backends
/// may persist and realize this envelope, but cannot derive replacement
/// attachment, listener, or lease identities from a `SandboxId`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SandboxProvisionNetworkPlanWire")]
pub struct SandboxProvisionNetworkPlan {
    network_plan: NetworkPlan,
    tenant_id: TenantId,
    generation: NetworkResourceGeneration,
    attachment_id: NetworkAttachmentId,
    endpoint_identities: Vec<SandboxProvisionEndpointIdentity>,
    listeners: Vec<SandboxProvisionListener>,
    dependency_listeners: Vec<SandboxProvisionDependencyListener>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SandboxProvisionNetworkPlanWire {
    network_plan: NetworkPlan,
    tenant_id: TenantId,
    generation: NetworkResourceGeneration,
    attachment_id: NetworkAttachmentId,
    endpoint_identities: Vec<SandboxProvisionEndpointIdentity>,
    listeners: Vec<SandboxProvisionListener>,
    dependency_listeners: Vec<SandboxProvisionDependencyListener>,
}

impl SandboxProvisionNetworkPlan {
    pub fn new(
        network_plan: NetworkPlan,
        tenant_id: TenantId,
        generation: NetworkResourceGeneration,
        attachment_id: NetworkAttachmentId,
        endpoint_identities: impl IntoIterator<Item = SandboxProvisionEndpointIdentity>,
        listeners: impl IntoIterator<Item = SandboxProvisionListener>,
        dependency_listeners: impl IntoIterator<Item = SandboxProvisionDependencyListener>,
    ) -> Result<Self, SandboxProvisionNetworkPlanError> {
        let plan_id = network_plan.plan_id().clone();
        if network_plan.generation() != generation {
            return Err(SandboxProvisionNetworkPlanError::GenerationMismatch);
        }
        let endpoint_identities = endpoint_identities.into_iter().collect::<Vec<_>>();
        let listeners = listeners.into_iter().collect::<Vec<_>>();
        let dependency_listeners = dependency_listeners.into_iter().collect::<Vec<_>>();
        let mut mapped_listener_ids = BTreeSet::new();
        let mut mapped_endpoint_ids = BTreeSet::new();
        for identity in &endpoint_identities {
            if !mapped_listener_ids.insert(identity.listener_id.clone()) {
                return Err(SandboxProvisionNetworkPlanError::DuplicateEndpointMappingListener);
            }
            if !mapped_endpoint_ids.insert(identity.endpoint_id.clone()) {
                return Err(SandboxProvisionNetworkPlanError::DuplicateEndpoint);
            }
        }
        let mut listener_ids = BTreeSet::new();
        let mut endpoint_ids = BTreeSet::new();
        let mut listener_names = BTreeSet::new();
        let mut lease_ids = BTreeSet::new();
        for listener in &listeners {
            validate_provision_listener(&plan_id, &tenant_id, generation, listener)?;
            let expected_endpoint = endpoint_identities
                .iter()
                .find(|identity| identity.listener_id == listener.listener_id)
                .ok_or(SandboxProvisionNetworkPlanError::EndpointIdentitySetMismatch)?;
            if expected_endpoint.endpoint_id != listener.endpoint_id {
                return Err(SandboxProvisionNetworkPlanError::EndpointIdentityMismatch);
            }
            if !endpoint_ids.insert(listener.endpoint_id.clone()) {
                return Err(SandboxProvisionNetworkPlanError::DuplicateEndpoint);
            }
            if !listener_ids.insert(listener.listener_id.clone()) {
                return Err(SandboxProvisionNetworkPlanError::DuplicateListener);
            }
            if !listener_names.insert(listener.binding.name.clone()) {
                return Err(SandboxProvisionNetworkPlanError::DuplicateListenerName);
            }
            if !lease_ids.insert(listener.port_lease.lease_id().clone()) {
                return Err(SandboxProvisionNetworkPlanError::DuplicatePortLease);
            }
        }
        if endpoint_identities.len() != listeners.len() {
            return Err(SandboxProvisionNetworkPlanError::EndpointIdentitySetMismatch);
        }
        for dependency in &dependency_listeners {
            if dependency.name.is_empty() {
                return Err(SandboxProvisionNetworkPlanError::InvalidDependencyListener);
            }
            if !listener_ids.insert(dependency.listener_id.clone()) {
                return Err(SandboxProvisionNetworkPlanError::DuplicateListener);
            }
            if !listener_names.insert(dependency.name.clone()) {
                return Err(SandboxProvisionNetworkPlanError::DuplicateListenerName);
            }
        }
        Ok(Self {
            network_plan,
            tenant_id,
            generation,
            attachment_id,
            endpoint_identities,
            listeners,
            dependency_listeners,
        })
    }

    pub fn plan_id(&self) -> &NetworkPlanId {
        self.network_plan.plan_id()
    }

    pub fn network_plan(&self) -> &NetworkPlan {
        &self.network_plan
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub const fn generation(&self) -> NetworkResourceGeneration {
        self.generation
    }

    pub fn attachment_id(&self) -> &NetworkAttachmentId {
        &self.attachment_id
    }

    pub fn endpoint_identities(&self) -> &[SandboxProvisionEndpointIdentity] {
        &self.endpoint_identities
    }

    pub fn listeners(&self) -> &[SandboxProvisionListener] {
        &self.listeners
    }

    pub fn dependency_listeners(&self) -> &[SandboxProvisionDependencyListener] {
        &self.dependency_listeners
    }

    pub fn bindings(&self) -> Vec<SandboxPortBinding> {
        self.listeners
            .iter()
            .map(|listener| listener.binding.clone())
            .collect()
    }

    pub fn port_leases(&self) -> Vec<PortLeaseRequest> {
        self.listeners
            .iter()
            .map(|listener| listener.port_lease.clone())
            .collect()
    }

    /// Project exact portable status from one provider-authenticated manifest.
    ///
    /// The observed address remains location only. Listener names, protocols,
    /// and guest ports correlate it to compiler-issued endpoint identity.
    pub fn project_portable_status(
        &self,
        actual_attachment_id: Option<&NetworkAttachmentId>,
        published_endpoints: &[PublishedEndpoint],
    ) -> Result<SandboxNetworkStatus, SandboxProvisionNetworkPlanError> {
        let attachment = match actual_attachment_id {
            Some(actual) if actual == &self.attachment_id => Some(NetworkAttachmentHandle::new(
                self.attachment_id.clone(),
                self.generation,
            )),
            Some(_) => return Err(SandboxProvisionNetworkPlanError::AttachmentIdentityMismatch),
            None => None,
        };
        if !published_endpoints.is_empty() && published_endpoints.len() != self.listeners.len() {
            return Err(SandboxProvisionNetworkPlanError::ListenerSetMismatch);
        }

        let mut portable = Vec::with_capacity(published_endpoints.len());
        for endpoint in published_endpoints {
            let listener = self
                .listeners
                .iter()
                .find(|listener| listener.binding.name == endpoint.name)
                .ok_or(SandboxProvisionNetworkPlanError::ListenerSetMismatch)?;
            if endpoint.protocol != listener.binding.protocol
                || endpoint.guest_port != Some(listener.binding.guest_port)
            {
                return Err(SandboxProvisionNetworkPlanError::EndpointObservationMismatch);
            }
            portable.push(PublishedEndpointHandle::new(
                listener.endpoint_id.clone(),
                self.generation,
                endpoint.clone(),
            ));
        }
        SandboxNetworkStatus::new(attachment, portable).map_err(Into::into)
    }
}

impl TryFrom<SandboxProvisionNetworkPlanWire> for SandboxProvisionNetworkPlan {
    type Error = SandboxProvisionNetworkPlanError;

    fn try_from(wire: SandboxProvisionNetworkPlanWire) -> Result<Self, Self::Error> {
        Self::new(
            wire.network_plan,
            wire.tenant_id,
            wire.generation,
            wire.attachment_id,
            wire.endpoint_identities,
            wire.listeners,
            wire.dependency_listeners,
        )
    }
}

/// One exact host listener routed to an observed private sandbox endpoint.
///
/// The private address is provider route data only. Stable authority remains
/// the listener and lease identity selected by the compiled network plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxProvisionIngressRoute {
    listener_id: ListenerId,
    port_lease: PortLeaseRequest,
    upstream: std::net::SocketAddr,
}

impl SandboxProvisionIngressRoute {
    pub fn listener_id(&self) -> &ListenerId {
        &self.listener_id
    }

    pub fn port_lease(&self) -> &PortLeaseRequest {
        &self.port_lease
    }

    pub const fn upstream(&self) -> std::net::SocketAddr {
        self.upstream
    }
}

/// Backend-authenticated route input for one deferred ingress publication.
///
/// The launch reservation claim is provider evidence needed to hand each
/// already-reserved lease to its real listener owner. It does not replace the
/// stable tenant, plan, attachment, listener, lease, or generation identities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxProvisionIngressTargets {
    tenant_id: TenantId,
    plan_id: NetworkPlanId,
    generation: NetworkResourceGeneration,
    attachment_id: NetworkAttachmentId,
    reservation_claim: NetworkReservationClaim,
    routes: Vec<SandboxProvisionIngressRoute>,
}

/// Effect-free classification of one private ingress route set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxProvisionIngressTargetObservation {
    /// No durable workload or attachment exists for the exact command.
    Absent { evidence: Vec<u8> },
    /// Durable provider state exists but is not yet safe to route.
    InProgress { evidence: Vec<u8> },
    /// The exact private attachment is ready and yields authenticated routes.
    Ready {
        targets: SandboxProvisionIngressTargets,
        evidence: Vec<u8>,
    },
}

impl SandboxProvisionIngressTargets {
    /// Construct exact route inputs from provider-authenticated private state.
    ///
    /// Provider crates may use this validation boundary to expose private
    /// routes without granting the publication owner access to attachment
    /// mutation. The assigned address remains route data; stable authority is
    /// carried by the plan, attachment, listener, lease, and generation IDs.
    pub fn from_private_attachment(
        plan: &SandboxProvisionNetworkPlan,
        actual_spec: &crate::SandboxSpec,
        actual_network_plan: &NetworkPlan,
        actual_attachment_id: &NetworkAttachmentId,
        reservation_claim: NetworkReservationClaim,
        assigned_ip: IpAddr,
    ) -> Result<Self, SandboxProvisionNetworkPlanError> {
        Self::from_private_attachment_with_upstream_port(
            plan,
            actual_spec,
            actual_network_plan,
            actual_attachment_id,
            reservation_claim,
            assigned_ip,
            |binding| binding.guest_port,
        )
    }

    pub(crate) fn from_private_attachment_with_upstream_port(
        plan: &SandboxProvisionNetworkPlan,
        actual_spec: &crate::SandboxSpec,
        actual_network_plan: &NetworkPlan,
        actual_attachment_id: &NetworkAttachmentId,
        reservation_claim: NetworkReservationClaim,
        assigned_ip: IpAddr,
        upstream_port: impl Fn(&crate::SandboxPortBinding) -> u16,
    ) -> Result<Self, SandboxProvisionNetworkPlanError> {
        if actual_spec.tenant_id != *plan.tenant_id() {
            return Err(SandboxProvisionNetworkPlanError::TenantMismatch);
        }
        if actual_network_plan != plan.network_plan() {
            return Err(SandboxProvisionNetworkPlanError::DurablePlanMismatch);
        }
        if actual_attachment_id != plan.attachment_id() {
            return Err(SandboxProvisionNetworkPlanError::AttachmentIdentityMismatch);
        }
        if actual_spec.port_bindings.len() != plan.listeners().len() {
            return Err(SandboxProvisionNetworkPlanError::ListenerSetMismatch);
        }
        let mut routes = Vec::with_capacity(plan.listeners().len());
        for listener in plan.listeners() {
            let binding = actual_spec
                .port_bindings
                .iter()
                .find(|binding| binding.name == listener.binding().name)
                .ok_or(SandboxProvisionNetworkPlanError::ListenerSetMismatch)?;
            if binding.protocol != listener.binding().protocol
                || binding.host_address != listener.binding().host_address
                || binding.host_port != listener.binding().host_port
                || binding.guest_port != listener.binding().guest_port
            {
                return Err(SandboxProvisionNetworkPlanError::ListenerSetMismatch);
            }
            routes.push(SandboxProvisionIngressRoute {
                listener_id: listener.listener_id().clone(),
                port_lease: listener.port_lease().clone(),
                upstream: std::net::SocketAddr::new(assigned_ip, upstream_port(binding)),
            });
        }
        Ok(Self {
            tenant_id: plan.tenant_id().clone(),
            plan_id: plan.plan_id().clone(),
            generation: plan.generation(),
            attachment_id: plan.attachment_id().clone(),
            reservation_claim,
            routes,
        })
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn plan_id(&self) -> &NetworkPlanId {
        &self.plan_id
    }

    pub const fn generation(&self) -> NetworkResourceGeneration {
        self.generation
    }

    pub fn attachment_id(&self) -> &NetworkAttachmentId {
        &self.attachment_id
    }

    pub fn reservation_claim(&self) -> &NetworkReservationClaim {
        &self.reservation_claim
    }

    pub fn routes(&self) -> &[SandboxProvisionIngressRoute] {
        &self.routes
    }
}

fn validate_provision_listener(
    plan_id: &NetworkPlanId,
    tenant_id: &TenantId,
    generation: NetworkResourceGeneration,
    listener: &SandboxProvisionListener,
) -> Result<(), SandboxProvisionNetworkPlanError> {
    let request = &listener.port_lease;
    if request.lease_id() != &PortLeaseId::for_listener(&listener.listener_id) {
        return Err(SandboxProvisionNetworkPlanError::LeaseIdentityMismatch);
    }
    let expected_owner: NetworkResourceId = listener.listener_id.clone().into();
    if request.owner_id() != &expected_owner {
        return Err(SandboxProvisionNetworkPlanError::ListenerOwnerMismatch);
    }
    if request.plan_id() != Some(plan_id) {
        return Err(SandboxProvisionNetworkPlanError::PlanIdentityMismatch);
    }
    if request.tenant_id() != Some(tenant_id) {
        return Err(SandboxProvisionNetworkPlanError::TenantMismatch);
    }
    if request.generation() != generation {
        return Err(SandboxProvisionNetworkPlanError::GenerationMismatch);
    }
    if request.accounting() != PortLeaseAccounting::TenantPublished {
        return Err(SandboxProvisionNetworkPlanError::AccountingMismatch);
    }
    if request.publication() != &PortPublicationIntent::host(listener.binding.host_address) {
        return Err(SandboxProvisionNetworkPlanError::PublicationMismatch);
    }
    if request.binding().protocol() != PortProtocol::Tcp {
        return Err(SandboxProvisionNetworkPlanError::ProtocolMismatch);
    }
    if request.binding().realm() != &PortBindRealm::Host {
        return Err(SandboxProvisionNetworkPlanError::BindRealmMismatch);
    }
    let (expected_target, expected_exposure) = published_scope(listener.binding.host_address)?;
    if request.binding().target() != &expected_target {
        return Err(SandboxProvisionNetworkPlanError::BindTargetMismatch);
    }
    if request.binding().exposure() != expected_exposure {
        return Err(SandboxProvisionNetworkPlanError::ExposureMismatch);
    }
    match (
        NonZeroU16::new(listener.binding.host_port),
        request.binding().port(),
    ) {
        (Some(expected), PortRequestMode::Exact(candidate)) if expected == *candidate => {}
        (None, PortRequestMode::ProviderAssigned) => {}
        _ => return Err(SandboxProvisionNetworkPlanError::PortRequestMismatch),
    }
    if listener.binding.name.is_empty() || listener.binding.guest_port == 0 {
        return Err(SandboxProvisionNetworkPlanError::InvalidBinding);
    }
    Ok(())
}

fn published_scope(
    address: IpAddr,
) -> Result<(PortBindTarget, PortExposure), SandboxProvisionNetworkPlanError> {
    let address = match address {
        IpAddr::V6(address) => address
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(address)),
        IpAddr::V4(_) => address,
    };
    let target = match address {
        IpAddr::V4(address) if address.is_unspecified() => PortBindTarget::ipv4_wildcard(),
        IpAddr::V4(address) => PortBindTarget::ipv4_specific(address),
        IpAddr::V6(address) if address.is_unspecified() => {
            PortBindTarget::ipv6_wildcard(PortIpv6Overlap::Unknown)
        }
        IpAddr::V6(address) => PortBindTarget::ipv6_specific(address, PortIpv6Overlap::Unknown)
            .map_err(|_| SandboxProvisionNetworkPlanError::BindTargetMismatch)?,
    };
    let exposure = match address {
        IpAddr::V4(address) if address.is_loopback() => PortExposure::Loopback,
        IpAddr::V4(address) if address.is_private() || address.is_link_local() => {
            PortExposure::Private
        }
        IpAddr::V6(address) if address.is_loopback() => PortExposure::Loopback,
        IpAddr::V6(address) if address.is_unique_local() || address.is_unicast_link_local() => {
            PortExposure::Private
        }
        IpAddr::V4(_) | IpAddr::V6(_) => PortExposure::Public,
    };
    Ok((target, exposure))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SandboxProvisionNetworkPlanError {
    #[error("sandbox provision plan contains a duplicate published-endpoint identity")]
    DuplicateEndpoint,
    #[error("sandbox provision plan maps one listener to multiple endpoint identities")]
    DuplicateEndpointMappingListener,
    #[error("sandbox provision endpoint identities do not cover the exact listener set")]
    EndpointIdentitySetMismatch,
    #[error("sandbox provision listener carries a crossed published-endpoint identity")]
    EndpointIdentityMismatch,
    #[error("sandbox provision plan contains a duplicate listener identity")]
    DuplicateListener,
    #[error("sandbox provision plan contains a duplicate listener name")]
    DuplicateListenerName,
    #[error("sandbox provision plan contains a duplicate port-lease identity")]
    DuplicatePortLease,
    #[error("sandbox provision listener lease identity does not match its listener")]
    LeaseIdentityMismatch,
    #[error("sandbox provision listener owner does not match its listener identity")]
    ListenerOwnerMismatch,
    #[error("sandbox provision listener belongs to a different network plan")]
    PlanIdentityMismatch,
    #[error("sandbox provision listener belongs to a different tenant")]
    TenantMismatch,
    #[error("sandbox provision listener belongs to a different generation")]
    GenerationMismatch,
    #[error("sandbox provision listener is not tenant-published authority")]
    AccountingMismatch,
    #[error("sandbox provision listener publication does not match its binding")]
    PublicationMismatch,
    #[error("sandbox provision listener uses a non-TCP host lease")]
    ProtocolMismatch,
    #[error("sandbox provision listener lease does not belong to the host bind realm")]
    BindRealmMismatch,
    #[error("sandbox provision listener lease target does not match its published host address")]
    BindTargetMismatch,
    #[error("sandbox provision listener lease exposure does not match its published host address")]
    ExposureMismatch,
    #[error("sandbox provision listener port request does not match its binding")]
    PortRequestMismatch,
    #[error("sandbox provision listener binding is incomplete")]
    InvalidBinding,
    #[error("sandbox provision dependency listener is incomplete")]
    InvalidDependencyListener,
    #[error("sandbox provision manifest carries a different durable network plan")]
    DurablePlanMismatch,
    #[error("sandbox provision manifest carries a different attachment identity")]
    AttachmentIdentityMismatch,
    #[error("sandbox provision manifest listener set differs from the compiled plan")]
    ListenerSetMismatch,
    #[error("sandbox provision endpoint observation differs from the compiled plan")]
    EndpointObservationMismatch,
    #[error(transparent)]
    InvalidPortableStatus(#[from] SandboxNetworkStatusError),
}

/// Read-only or effect-result evidence emitted by one narrow sandbox phase.
///
/// The bytes are provider-owned evidence. They are hashed by the upper adapter
/// before portable saga state is committed, and never become workload identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxProvisionPhaseObservation {
    /// The exact phase is durably complete.
    Succeeded { evidence: Vec<u8> },
    /// Exact provider inspection proves that the phase effect is absent.
    Absent { evidence: Vec<u8> },
    /// Exact provider work or readiness has not reached a terminal observation.
    InProgress { evidence: Vec<u8> },
    /// Provider state cannot be classified safely from current evidence.
    Ambiguous { evidence: Vec<u8> },
}

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
#[path = "provision/tests.rs"]
mod tests;
