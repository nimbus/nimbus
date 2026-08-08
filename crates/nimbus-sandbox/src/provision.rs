//! Provider-local idempotency for compute-issued provision attempts.
//!
//! This module deliberately knows nothing about the workload saga. It stores
//! only the provider's stable authority key and complete opaque fences supplied
//! by its adapter. The upper coordinator remains the sole owner of lifecycle
//! order and durable desired state.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::net::IpAddr;
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use nimbus_core::TenantId;
use nimbus_network::{
    ListenerId, NetworkAttachmentId, NetworkPlan, NetworkPlanId, NetworkProviderId,
    NetworkReservationClaim, NetworkResourceGeneration, NetworkResourceId, PortBindRealm,
    PortBindTarget, PortExposure, PortIpv6Overlap, PortLeaseAccounting, PortLeaseId,
    PortLeaseRequest, PortProtocol, PortPublicationIntent, PortRequestMode,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::SandboxPortBinding;

mod activation;
pub(crate) use activation::{
    ProvisionActivationObservationKind, ProvisionActivationRuntimeState,
    classify_provision_activation,
};

const JOURNAL_DIRECTORY: &str = ".nimbus-provision-attempts";
const RECORD_SUFFIX: &str = ".json";
const STAGE_SUFFIX: &str = ".stage";
const LOCK_SUFFIX: &str = ".lock";
const CURRENT_ENVELOPE_VERSION: u32 = 1;
const MAX_IDENTITY_LEN: usize = 256;
const MAX_CANONICAL_SUBJECT_LEN: usize = 64 * 1024;
#[cfg(not(test))]
const LOCK_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const LOCK_TIMEOUT: Duration = Duration::from_millis(250);
const LOCK_RETRY: Duration = Duration::from_millis(10);

/// One exact published-listener reservation supplied by the compiled plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxProvisionListener {
    listener_id: ListenerId,
    binding: SandboxPortBinding,
    port_lease: PortLeaseRequest,
}

impl SandboxProvisionListener {
    pub fn new(
        listener_id: ListenerId,
        binding: SandboxPortBinding,
        port_lease: PortLeaseRequest,
    ) -> Self {
        Self {
            listener_id,
            binding,
            port_lease,
        }
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
pub struct SandboxProvisionNetworkPlan {
    network_plan: NetworkPlan,
    tenant_id: TenantId,
    generation: NetworkResourceGeneration,
    attachment_id: NetworkAttachmentId,
    listeners: Vec<SandboxProvisionListener>,
    dependency_listeners: Vec<SandboxProvisionDependencyListener>,
}

impl SandboxProvisionNetworkPlan {
    pub fn new(
        network_plan: NetworkPlan,
        tenant_id: TenantId,
        generation: NetworkResourceGeneration,
        attachment_id: NetworkAttachmentId,
        listeners: impl IntoIterator<Item = SandboxProvisionListener>,
        dependency_listeners: impl IntoIterator<Item = SandboxProvisionDependencyListener>,
    ) -> Result<Self, SandboxProvisionNetworkPlanError> {
        let plan_id = network_plan.plan_id().clone();
        if network_plan.generation() != generation {
            return Err(SandboxProvisionNetworkPlanError::GenerationMismatch);
        }
        let listeners = listeners.into_iter().collect::<Vec<_>>();
        let dependency_listeners = dependency_listeners.into_iter().collect::<Vec<_>>();
        let mut listener_ids = BTreeSet::new();
        let mut listener_names = BTreeSet::new();
        let mut lease_ids = BTreeSet::new();
        for listener in &listeners {
            validate_provision_listener(&plan_id, &tenant_id, generation, listener)?;
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
                upstream: std::net::SocketAddr::new(assigned_ip, binding.guest_port),
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

/// Provider operation fenced independently within one stable workload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProvisionOperation {
    ReserveNetwork,
    PrepareWorkload,
    AttachNetwork,
    InspectActivationPrerequisites,
    ActivateWorkload,
    InspectWorkloadReadiness,
    PublishIngress,
    ObserveIngress,
}

impl ProviderProvisionOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::ReserveNetwork => "reserve_network",
            Self::PrepareWorkload => "prepare_workload",
            Self::AttachNetwork => "attach_network",
            Self::InspectActivationPrerequisites => "inspect_activation_prerequisites",
            Self::ActivateWorkload => "activate_workload",
            Self::InspectWorkloadReadiness => "inspect_workload_readiness",
            Self::PublishIngress => "publish_ingress",
            Self::ObserveIngress => "observe_ingress",
        }
    }
}

/// Complete opaque fences a provider must authenticate before one effect.
pub struct ProviderProvisionClaimInput {
    pub authority_id: String,
    pub effect_subject: String,
    pub attempt_id: String,
    pub dispatch_epoch: u64,
    pub generation: u64,
    pub desired_digest: String,
    pub source_digest: String,
    pub network_plan_digest: String,
    pub provider_target_digest: String,
    pub operation: ProviderProvisionOperation,
}

/// Validated provider-local claim. No address or allocated port is identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderProvisionClaim {
    authority_id: String,
    effect_subject: String,
    attempt_id: String,
    dispatch_epoch: u64,
    generation: u64,
    desired_digest: String,
    source_digest: String,
    network_plan_digest: String,
    provider_target_digest: String,
    operation: ProviderProvisionOperation,
}

impl ProviderProvisionClaim {
    pub fn new(input: ProviderProvisionClaimInput) -> Result<Self, ProviderProvisionJournalError> {
        validate_identity("authority ID", &input.authority_id)?;
        validate_identity("attempt ID", &input.attempt_id)?;
        if input.effect_subject.is_empty() || input.effect_subject.len() > MAX_CANONICAL_SUBJECT_LEN
        {
            return Err(ProviderProvisionJournalError::InvalidClaim {
                message: "effect subject must be non-empty and bounded".to_owned(),
            });
        }
        for (label, digest) in [
            ("desired", &input.desired_digest),
            ("source", &input.source_digest),
            ("network plan", &input.network_plan_digest),
            ("provider target", &input.provider_target_digest),
        ] {
            validate_sha256(label, digest)?;
        }
        Ok(Self {
            authority_id: input.authority_id,
            effect_subject: input.effect_subject,
            attempt_id: input.attempt_id,
            dispatch_epoch: input.dispatch_epoch,
            generation: input.generation,
            desired_digest: input.desired_digest,
            source_digest: input.source_digest,
            network_plan_digest: input.network_plan_digest,
            provider_target_digest: input.provider_target_digest,
            operation: input.operation,
        })
    }

    pub fn authority_id(&self) -> &str {
        &self.authority_id
    }

    pub fn effect_subject(&self) -> &str {
        &self.effect_subject
    }

    pub fn attempt_id(&self) -> &str {
        &self.attempt_id
    }

    pub const fn dispatch_epoch(&self) -> u64 {
        self.dispatch_epoch
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn operation(&self) -> ProviderProvisionOperation {
        self.operation
    }

    fn same_attempt_fence(&self, other: &Self) -> bool {
        self.authority_id == other.authority_id
            && self.effect_subject == other.effect_subject
            && self.attempt_id == other.attempt_id
            && self.generation == other.generation
            && self.desired_digest == other.desired_digest
            && self.source_digest == other.source_digest
            && self.network_plan_digest == other.network_plan_digest
            && self.provider_target_digest == other.provider_target_digest
            && self.operation == other.operation
    }

    fn validate(&self) -> Result<(), ProviderProvisionJournalError> {
        Self::new(ProviderProvisionClaimInput {
            authority_id: self.authority_id.clone(),
            effect_subject: self.effect_subject.clone(),
            attempt_id: self.attempt_id.clone(),
            dispatch_epoch: self.dispatch_epoch,
            generation: self.generation,
            desired_digest: self.desired_digest.clone(),
            source_digest: self.source_digest.clone(),
            network_plan_digest: self.network_plan_digest.clone(),
            provider_target_digest: self.provider_target_digest.clone(),
            operation: self.operation,
        })
        .map(|_| ())
    }
}

/// Durable provider observation for one exact attempt and epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProvisionObservationKind {
    Claimed,
    Succeeded,
    DefiniteFailure,
    Absent,
    InProgress,
    Ambiguous,
}

impl ProviderProvisionObservationKind {
    fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::DefiniteFailure | Self::Absent)
    }
}

/// Authenticated current provider observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderProvisionObservation {
    claim: ProviderProvisionClaim,
    kind: ProviderProvisionObservationKind,
    evidence_sha256: Option<String>,
}

impl ProviderProvisionObservation {
    pub fn claim(&self) -> &ProviderProvisionClaim {
        &self.claim
    }

    pub const fn kind(&self) -> ProviderProvisionObservationKind {
        self.kind
    }

    pub fn evidence_sha256(&self) -> Option<&str> {
        self.evidence_sha256.as_deref()
    }

    fn claimed(claim: ProviderProvisionClaim) -> Self {
        Self {
            claim,
            kind: ProviderProvisionObservationKind::Claimed,
            evidence_sha256: None,
        }
    }

    fn validate(&self) -> Result<(), ProviderProvisionJournalError> {
        self.claim.validate()?;
        match (self.kind, self.evidence_sha256.as_deref()) {
            (ProviderProvisionObservationKind::Claimed, None) => Ok(()),
            (ProviderProvisionObservationKind::Claimed, Some(_)) => {
                Err(ProviderProvisionJournalError::Corrupt {
                    message: "a claimed provider attempt cannot carry outcome evidence".to_owned(),
                })
            }
            (_, Some(evidence)) => validate_sha256("provider evidence", evidence),
            (_, None) => Err(ProviderProvisionJournalError::Corrupt {
                message: "a provider outcome must carry SHA-256 evidence".to_owned(),
            }),
        }
    }
}

/// Result of claiming one provider-local dispatch epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderProvisionClaimDecision {
    ExecuteClaimed(ProviderProvisionObservation),
    AdoptExactAttempt(ProviderProvisionObservation),
}

/// Typed fail-before or durable-store error.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProviderProvisionJournalError {
    #[error("invalid provider provision claim: {message}")]
    InvalidClaim { message: String },
    #[error(
        "provider provision generation {candidate} is stale relative to durable generation {current}"
    )]
    StaleGeneration { current: u64, candidate: u64 },
    #[error(
        "provider provision dispatch epoch {candidate} is stale relative to durable epoch {current}"
    )]
    StaleDispatchEpoch { current: u64, candidate: u64 },
    #[error(
        "provider provision dispatch epoch {candidate} skips durable epoch {current}; only exact +1 after absence is allowed"
    )]
    SkippedDispatchEpoch { current: u64, candidate: u64 },
    #[error("provider provision claim crosses durable authority at the same generation")]
    CrossedClaim,
    #[error("provider provision retry requires exact durable absence at the preceding epoch")]
    RetryWithoutAbsence,
    #[error("a newer provider generation cannot replace an in-progress or ambiguous effect")]
    PriorEffectUnresolved,
    #[error("provider provision journal is corrupt: {message}")]
    Corrupt { message: String },
    #[error("provider provision journal operation failed: {message}")]
    Store { message: String },
}

/// One provider-owned durable attempt journal rooted below its configured state.
#[derive(Debug, Clone)]
pub struct ProviderProvisionAttemptJournal {
    state_root: PathBuf,
    namespace: String,
}

impl ProviderProvisionAttemptJournal {
    /// Open an idempotency journal. Directory effects occur only on first use.
    pub fn open(
        state_root: impl Into<PathBuf>,
        namespace: impl Into<String>,
    ) -> Result<Self, ProviderProvisionJournalError> {
        let namespace = namespace.into();
        validate_identity("provider namespace", &namespace)?;
        let state_root = state_root.into();
        if state_root == Path::new("/") {
            return Err(ProviderProvisionJournalError::InvalidClaim {
                message: "provider journal state root cannot be the filesystem root".to_owned(),
            });
        }
        Ok(Self {
            state_root,
            namespace,
        })
    }

    /// Claim one exact epoch before any provider mutation.
    pub fn claim_dispatch_epoch(
        &self,
        claim: &ProviderProvisionClaim,
    ) -> Result<ProviderProvisionClaimDecision, ProviderProvisionJournalError> {
        claim.validate()?;
        let paths = self.paths(claim);
        self.establish_directory(&paths.directory)?;
        let _guard = lock(&paths.lock)?;
        remove_stale_stage(&paths.stage)?;
        let current = read_if_present(&paths.record)?;
        match current {
            None => self.publish_new_claim(&paths, claim.clone()),
            Some(current) => self.decide_existing(&paths, current, claim),
        }
    }

    /// Inspect only when the durable authority is the exact attempt and epoch.
    pub fn adopt_exact_attempt(
        &self,
        claim: &ProviderProvisionClaim,
    ) -> Result<Option<ProviderProvisionObservation>, ProviderProvisionJournalError> {
        claim.validate()?;
        let paths = self.paths(claim);
        if !self.journal_directory_exists(&paths.directory)? {
            return Ok(None);
        }
        let _guard = lock(&paths.lock)?;
        let Some(current) = read_if_present(&paths.record)? else {
            return Ok(None);
        };
        if current.claim == *claim {
            Ok(Some(current))
        } else {
            self.reject_stale_or_crossed(&current.claim, claim)?;
            Err(ProviderProvisionJournalError::CrossedClaim)
        }
    }

    /// Record one exact provider observation after the corresponding effect or inspection.
    pub fn record_observation(
        &self,
        claim: &ProviderProvisionClaim,
        kind: ProviderProvisionObservationKind,
        evidence: &[u8],
    ) -> Result<ProviderProvisionObservation, ProviderProvisionJournalError> {
        if kind == ProviderProvisionObservationKind::Claimed {
            return Err(ProviderProvisionJournalError::InvalidClaim {
                message: "record_observation requires an outcome kind".to_owned(),
            });
        }
        claim.validate()?;
        let paths = self.paths(claim);
        self.establish_directory(&paths.directory)?;
        let _guard = lock(&paths.lock)?;
        remove_stale_stage(&paths.stage)?;
        let current = read_if_present(&paths.record)?.ok_or_else(|| {
            ProviderProvisionJournalError::Store {
                message: "provider outcome has no durable preceding claim".to_owned(),
            }
        })?;
        if current.claim != *claim {
            self.reject_stale_or_crossed(&current.claim, claim)?;
            return Err(ProviderProvisionJournalError::CrossedClaim);
        }
        if current.kind.is_terminal() {
            let expected = evidence_sha256(evidence);
            if current.kind == kind && current.evidence_sha256.as_deref() == Some(&expected) {
                return Ok(current);
            }
            return Err(ProviderProvisionJournalError::CrossedClaim);
        }
        let observation = ProviderProvisionObservation {
            claim: claim.clone(),
            kind,
            evidence_sha256: Some(evidence_sha256(evidence)),
        };
        publish(&paths, &observation)?;
        Ok(observation)
    }

    /// Replace an exact publish success with provider-proven current absence.
    ///
    /// Process-bound ingress can disappear when its owner process dies after
    /// the provider journal recorded success but before compute committed the
    /// result. The provider's lifetime recovery is conclusive no-effect proof;
    /// recording that absence at the same dispatch epoch authorizes the sole
    /// coordinator to retry the same attempt at exactly the next epoch.
    pub fn record_reconciled_absence(
        &self,
        claim: &ProviderProvisionClaim,
        evidence: &[u8],
    ) -> Result<ProviderProvisionObservation, ProviderProvisionJournalError> {
        if claim.operation != ProviderProvisionOperation::PublishIngress {
            return Err(ProviderProvisionJournalError::InvalidClaim {
                message:
                    "only publish-ingress inspection may reconcile a terminal success to absence"
                        .to_owned(),
            });
        }
        claim.validate()?;
        let paths = self.paths(claim);
        self.establish_directory(&paths.directory)?;
        let _guard = lock(&paths.lock)?;
        remove_stale_stage(&paths.stage)?;
        let current = read_if_present(&paths.record)?.ok_or_else(|| {
            ProviderProvisionJournalError::Store {
                message: "provider absence has no durable preceding claim".to_owned(),
            }
        })?;
        if current.claim != *claim {
            self.reject_stale_or_crossed(&current.claim, claim)?;
            return Err(ProviderProvisionJournalError::CrossedClaim);
        }
        if current.kind == ProviderProvisionObservationKind::DefiniteFailure {
            return Err(ProviderProvisionJournalError::CrossedClaim);
        }
        let observation = ProviderProvisionObservation {
            claim: claim.clone(),
            kind: ProviderProvisionObservationKind::Absent,
            evidence_sha256: Some(evidence_sha256(evidence)),
        };
        if current == observation {
            return Ok(current);
        }
        publish(&paths, &observation)?;
        Ok(observation)
    }

    fn publish_new_claim(
        &self,
        paths: &JournalPaths,
        claim: ProviderProvisionClaim,
    ) -> Result<ProviderProvisionClaimDecision, ProviderProvisionJournalError> {
        let observation = ProviderProvisionObservation::claimed(claim);
        publish(paths, &observation)?;
        Ok(ProviderProvisionClaimDecision::ExecuteClaimed(observation))
    }

    fn decide_existing(
        &self,
        paths: &JournalPaths,
        current: ProviderProvisionObservation,
        candidate: &ProviderProvisionClaim,
    ) -> Result<ProviderProvisionClaimDecision, ProviderProvisionJournalError> {
        if candidate.generation < current.claim.generation {
            return Err(ProviderProvisionJournalError::StaleGeneration {
                current: current.claim.generation,
                candidate: candidate.generation,
            });
        }
        if candidate.generation > current.claim.generation {
            if !matches!(
                current.kind,
                ProviderProvisionObservationKind::Absent
                    | ProviderProvisionObservationKind::DefiniteFailure
            ) {
                return Err(ProviderProvisionJournalError::PriorEffectUnresolved);
            }
            return self.publish_new_claim(paths, candidate.clone());
        }
        if !candidate.same_attempt_fence(&current.claim) {
            return Err(ProviderProvisionJournalError::CrossedClaim);
        }
        if candidate.dispatch_epoch < current.claim.dispatch_epoch {
            return Err(Self::reject_stale_dispatch_epoch(
                current.claim.dispatch_epoch,
                candidate.dispatch_epoch,
            ));
        }
        if candidate.dispatch_epoch == current.claim.dispatch_epoch {
            return Ok(ProviderProvisionClaimDecision::AdoptExactAttempt(current));
        }
        let expected = current.claim.dispatch_epoch.checked_add(1).ok_or(
            ProviderProvisionJournalError::SkippedDispatchEpoch {
                current: current.claim.dispatch_epoch,
                candidate: candidate.dispatch_epoch,
            },
        )?;
        if candidate.dispatch_epoch != expected {
            return Err(ProviderProvisionJournalError::SkippedDispatchEpoch {
                current: current.claim.dispatch_epoch,
                candidate: candidate.dispatch_epoch,
            });
        }
        if current.kind != ProviderProvisionObservationKind::Absent {
            return Err(ProviderProvisionJournalError::RetryWithoutAbsence);
        }
        self.publish_new_claim(paths, candidate.clone())
    }

    fn reject_stale_or_crossed(
        &self,
        current: &ProviderProvisionClaim,
        candidate: &ProviderProvisionClaim,
    ) -> Result<(), ProviderProvisionJournalError> {
        if candidate.generation < current.generation {
            return Err(ProviderProvisionJournalError::StaleGeneration {
                current: current.generation,
                candidate: candidate.generation,
            });
        }
        if candidate.generation == current.generation
            && candidate.same_attempt_fence(current)
            && candidate.dispatch_epoch < current.dispatch_epoch
        {
            return Err(Self::reject_stale_dispatch_epoch(
                current.dispatch_epoch,
                candidate.dispatch_epoch,
            ));
        }
        Err(ProviderProvisionJournalError::CrossedClaim)
    }

    fn reject_stale_dispatch_epoch(current: u64, candidate: u64) -> ProviderProvisionJournalError {
        ProviderProvisionJournalError::StaleDispatchEpoch { current, candidate }
    }

    fn establish_directory(&self, directory: &Path) -> Result<(), ProviderProvisionJournalError> {
        crate::backends::oci::durable_directory::establish_durable_directory_chain_with(
            &self.state_root,
            directory,
            "provider provision attempt journal",
            sync_directory,
        )
        .map_err(|error| ProviderProvisionJournalError::Store {
            message: error.to_string(),
        })
    }

    fn journal_directory_exists(
        &self,
        directory: &Path,
    ) -> Result<bool, ProviderProvisionJournalError> {
        let journal_directory = self.state_root.join(JOURNAL_DIRECTORY);
        let namespace_directory =
            journal_directory.join(format!("{:x}", Sha256::digest(self.namespace.as_bytes())));
        debug_assert_eq!(directory, namespace_directory);
        for component in [
            self.state_root.as_path(),
            journal_directory.as_path(),
            namespace_directory.as_path(),
        ] {
            match fs::symlink_metadata(component) {
                Ok(metadata) if metadata.file_type().is_dir() => {}
                Ok(_) => {
                    return Err(ProviderProvisionJournalError::Corrupt {
                        message: format!(
                            "provider journal directory component {} is not a real directory",
                            component.display()
                        ),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(error) => {
                    return Err(ProviderProvisionJournalError::Store {
                        message: format!(
                            "failed to inspect provider journal directory {}: {error}",
                            component.display()
                        ),
                    });
                }
            }
        }
        Ok(true)
    }

    fn paths(&self, claim: &ProviderProvisionClaim) -> JournalPaths {
        let directory = self
            .state_root
            .join(JOURNAL_DIRECTORY)
            .join(format!("{:x}", Sha256::digest(self.namespace.as_bytes())));
        let key = stream_key(&self.namespace, claim);
        JournalPaths {
            record: directory.join(format!("{key}{RECORD_SUFFIX}")),
            stage: directory.join(format!("{key}{STAGE_SUFFIX}")),
            lock: directory.join(format!("{key}{LOCK_SUFFIX}")),
            directory,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct JournalEnvelope {
    version: u32,
    observation_sha256: String,
    observation: ProviderProvisionObservation,
}

impl JournalEnvelope {
    fn new(
        observation: ProviderProvisionObservation,
    ) -> Result<Self, ProviderProvisionJournalError> {
        observation.validate()?;
        Ok(Self {
            version: CURRENT_ENVELOPE_VERSION,
            observation_sha256: observation_sha256(&observation)?,
            observation,
        })
    }

    fn authenticate(
        self,
        path: &Path,
    ) -> Result<ProviderProvisionObservation, ProviderProvisionJournalError> {
        if self.version != CURRENT_ENVELOPE_VERSION
            || self.observation_sha256 != observation_sha256(&self.observation)?
        {
            return Err(ProviderProvisionJournalError::Corrupt {
                message: format!(
                    "{} has an unsupported version or failed SHA-256 authentication",
                    path.display()
                ),
            });
        }
        self.observation.validate()?;
        Ok(self.observation)
    }
}

struct JournalPaths {
    directory: PathBuf,
    record: PathBuf,
    stage: PathBuf,
    lock: PathBuf,
}

fn publish(
    paths: &JournalPaths,
    observation: &ProviderProvisionObservation,
) -> Result<(), ProviderProvisionJournalError> {
    let envelope = JournalEnvelope::new(observation.clone())?;
    let mut bytes = serde_json::to_vec_pretty(&envelope).map_err(|error| {
        ProviderProvisionJournalError::Store {
            message: format!("failed to encode provider provision observation: {error}"),
        }
    })?;
    bytes.push(b'\n');
    let result = (|| {
        let mut stage = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&paths.stage)
            .map_err(|error| ProviderProvisionJournalError::Store {
                message: format!(
                    "failed to create journal stage {}: {error}",
                    paths.stage.display()
                ),
            })?;
        stage
            .write_all(&bytes)
            .and_then(|()| stage.sync_all())
            .map_err(|error| ProviderProvisionJournalError::Store {
                message: format!(
                    "failed to durably write journal stage {}: {error}",
                    paths.stage.display()
                ),
            })?;
        fs::rename(&paths.stage, &paths.record).map_err(|error| {
            ProviderProvisionJournalError::Store {
                message: format!(
                    "failed to atomically publish journal {}: {error}",
                    paths.record.display()
                ),
            }
        })?;
        sync_directory(&paths.directory).map_err(|error| ProviderProvisionJournalError::Store {
            message: format!(
                "journal {} reached its commit point but directory sync failed; outcome is ambiguous: {error}",
                paths.record.display()
            ),
        })
    })();
    match (result, remove_stale_stage(&paths.stage)) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(primary), Ok(())) => Err(primary),
        (Ok(()), Err(cleanup)) => Err(cleanup),
        (Err(primary), Err(cleanup)) => Err(ProviderProvisionJournalError::Store {
            message: format!("{primary}; staged journal cleanup also failed: {cleanup}"),
        }),
    }
}

fn read_if_present(
    path: &Path,
) -> Result<Option<ProviderProvisionObservation>, ProviderProvisionJournalError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            Err(ProviderProvisionJournalError::Corrupt {
                message: format!("journal entry {} is not a regular file", path.display()),
            })
        }
        Ok(_) => {
            let bytes = fs::read(path).map_err(|error| ProviderProvisionJournalError::Store {
                message: format!("failed to read journal {}: {error}", path.display()),
            })?;
            let envelope: JournalEnvelope = serde_json::from_slice(&bytes).map_err(|error| {
                ProviderProvisionJournalError::Corrupt {
                    message: format!("failed to parse strict journal {}: {error}", path.display()),
                }
            })?;
            envelope.authenticate(path).map(Some)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ProviderProvisionJournalError::Store {
            message: format!("failed to inspect journal {}: {error}", path.display()),
        }),
    }
}

fn lock(path: &Path) -> Result<JournalGuard, ProviderProvisionJournalError> {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && !metadata.file_type().is_file()
    {
        return Err(ProviderProvisionJournalError::Corrupt {
            message: format!("journal lock {} is not a regular file", path.display()),
        });
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|error| ProviderProvisionJournalError::Store {
            message: format!("failed to open journal lock {}: {error}", path.display()),
        })?;
    if !file
        .metadata()
        .map_err(|error| ProviderProvisionJournalError::Store {
            message: format!("failed to inspect journal lock {}: {error}", path.display()),
        })?
        .is_file()
    {
        return Err(ProviderProvisionJournalError::Corrupt {
            message: format!("journal lock {} is not a regular file", path.display()),
        });
    }
    let deadline = Instant::now() + LOCK_TIMEOUT;
    loop {
        match FileExt::try_lock_exclusive(&file) {
            Ok(()) => return Ok(JournalGuard { _file: file }),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(ProviderProvisionJournalError::Store {
                        message: format!("timed out acquiring journal lock {}", path.display()),
                    });
                }
                thread::sleep(LOCK_RETRY);
            }
            Err(error) => {
                return Err(ProviderProvisionJournalError::Store {
                    message: format!("failed to acquire journal lock {}: {error}", path.display()),
                });
            }
        }
    }
}

fn remove_stale_stage(path: &Path) -> Result<(), ProviderProvisionJournalError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            Err(ProviderProvisionJournalError::Corrupt {
                message: format!("journal stage {} is not a regular file", path.display()),
            })
        }
        Ok(_) => fs::remove_file(path).map_err(|error| ProviderProvisionJournalError::Store {
            message: format!(
                "failed to remove stale journal stage {}: {error}",
                path.display()
            ),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ProviderProvisionJournalError::Store {
            message: format!(
                "failed to inspect journal stage {}: {error}",
                path.display()
            ),
        }),
    }
}

fn validate_identity(label: &str, value: &str) -> Result<(), ProviderProvisionJournalError> {
    if value.is_empty()
        || value.len() > MAX_IDENTITY_LEN
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        return Err(ProviderProvisionJournalError::InvalidClaim {
            message: format!("{label} must be a bounded portable identity"),
        });
    }
    Ok(())
}

fn validate_sha256(label: &str, value: &str) -> Result<(), ProviderProvisionJournalError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ProviderProvisionJournalError::InvalidClaim {
            message: format!("{label} digest must be canonical lowercase SHA-256"),
        });
    }
    Ok(())
}

fn stream_key(namespace: &str, claim: &ProviderProvisionClaim) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"nimbus.sandbox.provider-provision.stream.v1\0");
    for component in [namespace, claim.authority_id(), claim.operation().as_str()] {
        hasher.update(
            u64::try_from(component.len())
                .expect("a Rust string length fits u64 on supported targets")
                .to_be_bytes(),
        );
        hasher.update(component.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn evidence_sha256(evidence: &[u8]) -> String {
    format!("{:x}", Sha256::digest(evidence))
}

fn observation_sha256(
    observation: &ProviderProvisionObservation,
) -> Result<String, ProviderProvisionJournalError> {
    let bytes =
        serde_json::to_vec(observation).map_err(|error| ProviderProvisionJournalError::Store {
            message: format!("failed to authenticate provider observation: {error}"),
        })?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn sync_directory(path: &Path) -> std::io::Result<()> {
    File::open(path)?.sync_all()
}

#[derive(Debug)]
struct JournalGuard {
    _file: File,
}

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
#[path = "provision/tests.rs"]
mod tests;
