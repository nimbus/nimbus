//! Crash-safe host-port reservation lifecycle.
//!
//! This module owns portable lease identity and durable lifecycle state. It
//! does not bind sockets, probe the host, decide tenant quota, or interpret
//! provider handles. Every operation runs in the one
//! [`LocalNetworkStateStore`] lock and transaction domain, so separately opened
//! handles and separate Nimbus processes cannot publish conflicting authority.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::num::NonZeroU16;
use std::path::Path;

use nimbus_core::TenantId;
use serde::{Deserialize, Serialize};

use crate::{
    LocalNetworkStateStore, NetworkLeaseEpoch, NetworkReservationClaim, NetworkResourceGeneration,
    NetworkResourceId, NetworkStatePartition, NetworkStateStoreError, NetworkStateTransactionError,
    PortLeaseId,
};

mod binding;
mod operation;
mod rebind;
mod request;

pub use binding::{
    PortBindAttempt, PortBindAttemptError, PortBindClaim, PortBindFailure, PortBindFailureKind,
    PortBindingMismatch, PortBindingProvenance, PortBoundEndpoint, PortBoundEndpointError,
    PortLeaseBinding,
};
use operation::PortLeaseOperationError;
pub use operation::{PortLeaseFenceMismatch, PortLeaseOperation};
pub use request::{
    PortAddressFamily, PortBindRealm, PortBindRealmError, PortBindRealmErrorKind, PortBindTarget,
    PortBindTargetError, PortBindingSpec, PortExposure, PortIpv6Overlap, PortIsolatedRealm,
    PortProtocol, PortPublicationIntent, PortRange, PortRangeError, PortRequestMode,
};

/// Durable phase of one host-port lease generation.
///
/// `CleanupPending` is included from the start so NNC3.8 provider
/// reconciliation can retain ambiguous unbind authority without changing the
/// durable wire vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortLeasePhase {
    /// Stable lease identity owns the requested conflict slot.
    Reserved,
    /// A concrete provider binding has been durably adopted but is not public.
    Binding,
    /// The owner may publish or serve through the adopted binding.
    Active,
    /// New publication/use is fenced while the owner drains and unbinds.
    Withdrawing,
    /// Provider deletion is absent or ambiguous; the slot remains fenced.
    CleanupPending,
    /// Confirmed terminal release; a different stable lease may reuse the slot.
    Released,
    /// Confirmed terminal failure with no provider effect or reusable ambiguity.
    Failed,
}

impl PortLeasePhase {
    /// Whether this record no longer fences its requested conflict slot.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Released | Self::Failed)
    }
}

/// Resource-accounting class declared by the admission owner.
///
/// This is durable allocation metadata, not a policy decision. An upper layer
/// decides whether a limit applies and supplies its value; the port authority
/// uses this class only to enforce that caller-supplied decision atomically
/// with reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortLeaseAccounting {
    /// Host-internal infrastructure listener, such as a workload egress PEP.
    HostInternal,
    /// Tenant-visible published endpoint counted by tenant port admission.
    TenantPublished,
}

/// One caller-supplied tenant publication limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantPublishedPortLimit {
    tenant_id: TenantId,
    maximum: usize,
}

impl TenantPublishedPortLimit {
    /// Construct the admission decision to enforce with one reservation batch.
    pub fn new(tenant_id: TenantId, maximum: usize) -> Self {
        Self { tenant_id, maximum }
    }

    /// Tenant whose metered requests are admitted by this decision.
    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Maximum simultaneous non-terminal published leases.
    pub const fn maximum(&self) -> usize {
        self.maximum
    }
}

/// Exact desired generation and monotonic authority epoch for one port lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PortLeaseFence {
    generation: NetworkResourceGeneration,
    lease_epoch: NetworkLeaseEpoch,
}

impl PortLeaseFence {
    /// Construct an immutable port-lease fence.
    pub const fn new(
        generation: NetworkResourceGeneration,
        lease_epoch: NetworkLeaseEpoch,
    ) -> Self {
        Self {
            generation,
            lease_epoch,
        }
    }

    /// Desired connectivity-resource generation.
    pub const fn generation(self) -> NetworkResourceGeneration {
        self.generation
    }

    /// Monotonic authority epoch.
    pub const fn lease_epoch(self) -> NetworkLeaseEpoch {
        self.lease_epoch
    }
}

/// Immutable identity and fence carried by every lease operation.
///
/// The portable [`PortBindingSpec`] carries protocol, address/family overlap,
/// realm, exposure, and exact/range/provider-assigned allocation semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortLeaseRequest {
    lease_id: PortLeaseId,
    owner_id: NetworkResourceId,
    tenant_id: Option<TenantId>,
    generation: NetworkResourceGeneration,
    lease_epoch: NetworkLeaseEpoch,
    accounting: PortLeaseAccounting,
    publication: PortPublicationIntent,
    binding: PortBindingSpec,
}

impl PortLeaseRequest {
    /// Construct one immutable host-port lease request.
    pub fn new(
        lease_id: PortLeaseId,
        owner_id: NetworkResourceId,
        tenant_id: Option<TenantId>,
        fence: PortLeaseFence,
        accounting: PortLeaseAccounting,
        publication: PortPublicationIntent,
        binding: PortBindingSpec,
    ) -> Self {
        Self {
            lease_id,
            owner_id,
            tenant_id,
            generation: fence.generation(),
            lease_epoch: fence.lease_epoch(),
            accounting,
            publication: publication.canonicalized(),
            binding,
        }
    }

    /// Stable reservation identity.
    pub fn lease_id(&self) -> &PortLeaseId {
        &self.lease_id
    }

    /// Stable resource that owns this reservation.
    pub fn owner_id(&self) -> &NetworkResourceId {
        &self.owner_id
    }

    /// Tenant attribution, when the upper admission decision is tenant-scoped.
    pub fn tenant_id(&self) -> Option<&TenantId> {
        self.tenant_id.as_ref()
    }

    /// Desired resource generation fenced by this reservation.
    pub const fn generation(&self) -> NetworkResourceGeneration {
        self.generation
    }

    /// Monotonic lease authority epoch.
    pub const fn lease_epoch(&self) -> NetworkLeaseEpoch {
        self.lease_epoch
    }

    /// Durable resource-accounting class supplied by admission.
    pub const fn accounting(&self) -> PortLeaseAccounting {
        self.accounting
    }

    /// Exact desired host publication, separate from the provider bind target.
    pub fn publication(&self) -> &PortPublicationIntent {
        &self.publication
    }

    /// Portable binding and conflict domain.
    pub fn binding(&self) -> &PortBindingSpec {
        &self.binding
    }
}

/// Durable port-lease record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortLeaseRecord {
    request: PortLeaseRequest,
    reserved_port: Option<NonZeroU16>,
    phase: PortLeasePhase,
    reservation_claim: Option<NetworkReservationClaim>,
    bind_claim: Option<PortBindClaim>,
    adoption_claim: Option<PortBindClaim>,
    binding: Option<PortLeaseBinding>,
    confirmed_stopped_binding: Option<PortLeaseBinding>,
    failure: Option<PortBindFailure>,
}

impl PortLeaseRecord {
    /// Immutable identity/fence that admitted this record.
    pub fn request(&self) -> &PortLeaseRequest {
        &self.request
    }

    /// Numeric port currently fenced by this lease.
    ///
    /// Provider-assigned requests remain `None` until bind adoption.
    pub const fn reserved_port(&self) -> Option<NonZeroU16> {
        self.reserved_port
    }

    /// Current durable lifecycle phase.
    pub const fn phase(&self) -> PortLeasePhase {
        self.phase
    }

    /// Coordinator that alone may compensate this never-bound reservation.
    pub fn reservation_claim(&self) -> Option<&NetworkReservationClaim> {
        self.reservation_claim.as_ref()
    }

    /// Exclusive Nimbus-owned provider attempt, while binding is in flight.
    pub fn bind_claim(&self) -> Option<&PortBindClaim> {
        self.bind_claim.as_ref()
    }

    /// Exact provider attempt whose concrete binding was adopted.
    ///
    /// Unlike `bind_claim`, this is historical fencing evidence rather than
    /// live permission to create an effect. It remains durable for as long as
    /// the corresponding provider binding remains recorded.
    pub fn adoption_claim(&self) -> Option<&PortBindClaim> {
        self.adoption_claim.as_ref()
    }

    /// Adopted concrete binding, when one has been recorded.
    pub fn binding(&self) -> Option<&PortLeaseBinding> {
        self.binding.as_ref()
    }

    /// Exact prior binding whose provider absence was durably confirmed.
    ///
    /// This receipt exists only while a same-generation rebind reservation is
    /// retained. It is not a live binding and cannot be manufactured by a
    /// fresh reservation.
    pub fn confirmed_stopped_binding(&self) -> Option<&PortLeaseBinding> {
        self.confirmed_stopped_binding.as_ref()
    }

    /// Durable failed-bind evidence, when the lease terminated before adoption.
    pub fn failure(&self) -> Option<&PortBindFailure> {
        self.failure.as_ref()
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PortLeaseState {
    leases: BTreeMap<PortLeaseId, PortLeaseRecord>,
}

impl PortLeaseState {
    fn reserve_request(
        &mut self,
        request: PortLeaseRequest,
        reservation_claim: Option<&NetworkReservationClaim>,
    ) -> Result<PortLeaseRecord, PortLeaseOperationError> {
        validate_request_accounting(&request)?;
        if let Some(existing) = self.leases.get(request.lease_id()) {
            if existing.request == request {
                if existing.reservation_claim.as_ref() != reservation_claim {
                    return Err(PortLeaseOperationError::ReservationClaimConflict {
                        lease_id: request.lease_id.clone(),
                    });
                }
                return Ok(existing.clone());
            }
            return Err(PortLeaseOperationError::IdentityConflict {
                lease_id: request.lease_id.clone(),
            });
        }

        let reserved_port = self.reserve_port(&request)?;
        let record = PortLeaseRecord {
            request: request.clone(),
            reserved_port,
            phase: PortLeasePhase::Reserved,
            reservation_claim: reservation_claim.cloned(),
            bind_claim: None,
            adoption_claim: None,
            binding: None,
            confirmed_stopped_binding: None,
            failure: None,
        };
        self.leases.insert(request.lease_id.clone(), record.clone());
        Ok(record)
    }

    fn validate(&self) -> Result<(), PortLeaseOperationError> {
        let mut live_ports =
            BTreeMap::<(PortProtocol, NonZeroU16), Vec<(&PortLeaseId, &PortLeaseRecord)>>::new();

        for (lease_id, record) in &self.leases {
            if lease_id != record.request.lease_id() {
                return Err(PortLeaseOperationError::CorruptAuthority {
                    reason: format!(
                        "map key {lease_id} does not match record identity {}",
                        record.request.lease_id()
                    ),
                });
            }
            if let Err(error) = validate_request_accounting(&record.request) {
                let detail = match error {
                    PortLeaseOperationError::TenantAttributionRequired { .. } => {
                        "tenant-published authority requires tenant attribution"
                    }
                    PortLeaseOperationError::InvalidPublicationAccounting { .. } => {
                        "publication intent does not match its accounting class"
                    }
                    _ => unreachable!(
                        "request accounting validation returns only accounting invariants"
                    ),
                };
                return Err(PortLeaseOperationError::CorruptAuthority {
                    reason: format!(
                        "lease {lease_id} has invalid durable publication/accounting authority: \
                         {detail}"
                    ),
                });
            }

            if record.bind_claim.is_some()
                && !matches!(
                    record.phase,
                    PortLeasePhase::Reserved | PortLeasePhase::Binding
                )
            {
                return Err(PortLeaseOperationError::CorruptAuthority {
                    reason: format!(
                        "{:?} lease {lease_id} retains an in-flight bind claim",
                        record.phase
                    ),
                });
            }
            if record.phase == PortLeasePhase::Binding && record.bind_claim.is_none() {
                return Err(PortLeaseOperationError::CorruptAuthority {
                    reason: format!("binding lease {lease_id} has no durable provider bind claim"),
                });
            }
            if record.adoption_claim.is_some() != record.binding.is_some() {
                return Err(PortLeaseOperationError::CorruptAuthority {
                    reason: format!(
                        "{:?} lease {lease_id} must retain an exact adoption claim if and only if \
                         it retains provider binding evidence",
                        record.phase
                    ),
                });
            }
            if record.phase == PortLeasePhase::Binding && record.bind_claim != record.adoption_claim
            {
                return Err(PortLeaseOperationError::CorruptAuthority {
                    reason: format!(
                        "binding lease {lease_id} has different in-flight and adopted provider \
                         attempt identities"
                    ),
                });
            }
            if record.reservation_claim.is_some()
                && !matches!(
                    record.phase,
                    PortLeasePhase::Reserved | PortLeasePhase::Released | PortLeasePhase::Failed
                )
            {
                return Err(PortLeaseOperationError::CorruptAuthority {
                    reason: format!(
                        "{:?} lease {lease_id} retains never-bound coordinator authority",
                        record.phase
                    ),
                });
            }
            if record.reservation_claim.is_some() && record.binding.is_some() {
                return Err(PortLeaseOperationError::CorruptAuthority {
                    reason: format!(
                        "{:?} lease {lease_id} retains never-bound coordinator authority \
                         alongside provider binding evidence",
                        record.phase
                    ),
                });
            }
            if let Some(confirmed_stopped_binding) = &record.confirmed_stopped_binding {
                if record.phase != PortLeasePhase::Reserved {
                    return Err(PortLeaseOperationError::CorruptAuthority {
                        reason: format!(
                            "{:?} lease {lease_id} retains confirmed stopped-binding evidence",
                            record.phase
                        ),
                    });
                }
                if record.reservation_claim.is_some()
                    || record.binding.is_some()
                    || record.failure.is_some()
                {
                    return Err(PortLeaseOperationError::CorruptAuthority {
                        reason: format!(
                            "restart-retained lease {lease_id} combines confirmed stopped-binding \
                             evidence with incompatible lifecycle authority"
                        ),
                    });
                }
                if Some(confirmed_stopped_binding.actual_port()) != record.reserved_port {
                    return Err(PortLeaseOperationError::CorruptAuthority {
                        reason: format!(
                            "restart-retained lease {lease_id} reserves {:?} but records confirmed \
                             stopped binding {}",
                            record.reserved_port,
                            confirmed_stopped_binding.actual_port()
                        ),
                    });
                }
                if let Some(mismatch) = confirmed_stopped_binding.mismatch(record.request.binding())
                {
                    return Err(PortLeaseOperationError::CorruptAuthority {
                        reason: format!(
                            "restart-retained lease {lease_id} has confirmed stopped binding \
                             incompatible with its request: {mismatch}"
                        ),
                    });
                }
            }
            match (
                record.phase,
                record.binding.as_ref(),
                record.failure.as_ref(),
            ) {
                (PortLeasePhase::Reserved, Some(_), _) => {
                    return Err(PortLeaseOperationError::CorruptAuthority {
                        reason: format!("reserved lease {lease_id} has provider binding evidence"),
                    });
                }
                (PortLeasePhase::Reserved, None, Some(_)) => {
                    return Err(PortLeaseOperationError::CorruptAuthority {
                        reason: format!("reserved lease {lease_id} has bind failure evidence"),
                    });
                }
                (PortLeasePhase::Binding | PortLeasePhase::Active, None, _) => {
                    return Err(PortLeaseOperationError::CorruptAuthority {
                        reason: format!("{:?} lease {lease_id} has no binding", record.phase),
                    });
                }
                (PortLeasePhase::Binding | PortLeasePhase::Active, Some(_), Some(_)) => {
                    return Err(PortLeaseOperationError::CorruptAuthority {
                        reason: format!(
                            "{:?} lease {lease_id} has both binding and failure evidence",
                            record.phase
                        ),
                    });
                }
                (PortLeasePhase::Failed, Some(_), _) => {
                    return Err(PortLeaseOperationError::CorruptAuthority {
                        reason: format!(
                            "terminal failed lease {lease_id} retains provider binding evidence"
                        ),
                    });
                }
                (PortLeasePhase::Failed, None, None) => {
                    return Err(PortLeaseOperationError::CorruptAuthority {
                        reason: format!("terminal failed lease {lease_id} has no failure evidence"),
                    });
                }
                (
                    PortLeasePhase::Withdrawing
                    | PortLeasePhase::CleanupPending
                    | PortLeasePhase::Released,
                    _,
                    Some(_),
                ) => {
                    return Err(PortLeaseOperationError::CorruptAuthority {
                        reason: format!(
                            "{:?} lease {lease_id} retains bind failure evidence",
                            record.phase
                        ),
                    });
                }
                _ => {}
            }

            match (record.request.binding.port(), record.reserved_port) {
                (PortRequestMode::ProviderAssigned, None) => {}
                (mode, Some(actual)) if mode.accepts(actual) => {}
                (mode, reserved_port) => {
                    return Err(PortLeaseOperationError::CorruptAuthority {
                        reason: format!(
                            "lease {lease_id} has reserved port {reserved_port:?} incompatible with request {mode:?}"
                        ),
                    });
                }
            }

            if let Some(binding) = &record.binding
                && Some(binding.actual_port()) != record.reserved_port
            {
                return Err(PortLeaseOperationError::CorruptAuthority {
                    reason: format!(
                        "lease {lease_id} reserves {:?} but records binding {}",
                        record.reserved_port,
                        binding.actual_port()
                    ),
                });
            }
            if let Some(binding) = &record.binding
                && let Some(mismatch) = binding.mismatch(record.request.binding())
            {
                return Err(PortLeaseOperationError::CorruptAuthority {
                    reason: format!(
                        "lease {lease_id} has provider binding incompatible with its request: {mismatch}"
                    ),
                });
            }
            if let (Some(binding), Some(claim)) = (&record.binding, &record.adoption_claim)
                && !binding.provider_registration_matches_claim(claim)
            {
                return Err(PortLeaseOperationError::CorruptAuthority {
                    reason: format!(
                        "lease {lease_id} binds provider evidence from a different registration \
                         than its adopted provider attempt"
                    ),
                });
            }
            if let Some(failure) = &record.failure
                && let Some(mismatch) = failure.mismatch(record.request.binding())
            {
                return Err(PortLeaseOperationError::CorruptAuthority {
                    reason: format!(
                        "lease {lease_id} has bind failure incompatible with its request: {mismatch}"
                    ),
                });
            }
            if let Some(failure) = &record.failure {
                let selected_port_matches =
                    match (record.request.binding.port(), record.reserved_port) {
                        (PortRequestMode::ProviderAssigned, None) => failure.attempt().port() == 0,
                        (
                            PortRequestMode::Exact(_) | PortRequestMode::Range(_),
                            Some(reserved_port),
                        ) => failure.attempt().port() == reserved_port.get(),
                        _ => false,
                    };
                if !selected_port_matches {
                    return Err(PortLeaseOperationError::CorruptAuthority {
                        reason: format!(
                            "lease {lease_id} failed bind attempt does not match its selected port"
                        ),
                    });
                }
            }

            if !record.phase.is_terminal()
                && let Some(reserved_port) = record.reserved_port
            {
                let key = (record.request.binding.protocol(), reserved_port);
                let entries = live_ports.entry(key).or_default();
                if let Some((existing_lease_id, _)) = entries.iter().find(|(_, existing)| {
                    record.request.binding.overlaps(&existing.request.binding)
                }) {
                    return Err(PortLeaseOperationError::CorruptAuthority {
                        reason: format!(
                            "overlapping non-terminal leases {existing_lease_id} and {lease_id} both fence {:?} port {reserved_port}",
                            record.request.binding.protocol()
                        ),
                    });
                }
                entries.push((lease_id, record));
            }
        }

        Ok(())
    }

    fn conflicting_record<'a>(
        &'a self,
        request: &PortLeaseRequest,
        port: NonZeroU16,
        exclude: Option<&PortLeaseId>,
    ) -> Option<&'a PortLeaseRecord> {
        self.leases.values().find(|record| {
            !record.phase.is_terminal()
                && record.reserved_port == Some(port)
                && exclude != Some(record.request.lease_id())
                && request.binding.overlaps(&record.request.binding)
        })
    }

    fn reserve_port(
        &self,
        request: &PortLeaseRequest,
    ) -> Result<Option<NonZeroU16>, PortLeaseOperationError> {
        match request.binding.port() {
            PortRequestMode::Exact(port) => {
                if let Some(existing) = self.conflicting_record(request, *port, None) {
                    return Err(port_conflict(request, existing, *port));
                }
                Ok(Some(*port))
            }
            PortRequestMode::Range(range) => {
                let occupied = self
                    .leases
                    .values()
                    .filter(|record| {
                        !record.phase.is_terminal()
                            && request.binding.overlaps(&record.request.binding)
                    })
                    .filter_map(|record| record.reserved_port)
                    .collect::<BTreeSet<_>>();
                let reserved_port = range
                    .candidates()
                    .find(|candidate| !occupied.contains(candidate))
                    .ok_or_else(|| PortLeaseOperationError::PortRangeExhausted {
                        requested_lease_id: request.lease_id.clone(),
                        requested_owner_id: request.owner_id.clone(),
                        requested_range: *range,
                    })?;
                Ok(Some(reserved_port))
            }
            PortRequestMode::ProviderAssigned => Ok(None),
        }
    }
}

/// Node-local port lease authority backed by the one network store/lock.
#[derive(Clone, Debug)]
pub struct LocalPortLeaseAuthority {
    store: LocalNetworkStateStore,
}

impl LocalPortLeaseAuthority {
    /// Open the shared node-local authority.
    pub fn open(state_root: impl AsRef<Path>) -> Result<Self, PortLeaseError> {
        let authority = Self {
            store: LocalNetworkStateStore::open(state_root).map_err(PortLeaseError::Store)?,
        };
        authority.load_state()?;
        Ok(authority)
    }

    /// Atomically reserve an exact/range slot or provider-assigned identity.
    ///
    /// Replaying the same immutable unclaimed request returns its existing
    /// record. A coordinator-owned reservation requires the exact coordinator
    /// replay API and claim; generic replay fails closed. Reusing a lease ID
    /// with different identity/fence data also fails closed.
    /// Every non-terminal record with a selected slot fences it, including
    /// `Withdrawing` and `CleanupPending`. Range requests select the lowest
    /// available slot in their complete overlap domain. Provider-assigned
    /// requests acquire a numeric fence only during adoption.
    pub fn reserve(&self, request: PortLeaseRequest) -> Result<PortLeaseRecord, PortLeaseError> {
        self.transaction(|state| state.reserve_request(request, None))
    }

    /// Reserve one request for an attempt-unique launch coordinator.
    ///
    /// Exact replay is idempotent only when it presents the same claim.
    pub fn reserve_for_coordinator(
        &self,
        request: PortLeaseRequest,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        self.transaction(|state| state.reserve_request(request, Some(reservation_claim)))
    }

    /// Atomically reserve an ordered group of listener requests.
    ///
    /// Every request is evaluated against prior requests in this group and
    /// current node authority under one store transaction. If any request
    /// conflicts, exhausts its range, reuses an identity incorrectly, or
    /// crosses the generic/coordinator claim boundary, none of the new
    /// reservations commit. Replaying an identical unclaimed group returns
    /// the existing records in caller order.
    pub fn reserve_batch(
        &self,
        requests: Vec<PortLeaseRequest>,
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        self.reserve_batch_with_limit(requests, None, None)
    }

    /// Atomically reserve a launch batch for one attempt-unique coordinator.
    ///
    /// A different coordinator replaying the exact requests is rejected while
    /// the original claim remains durable. This prevents one failed replay
    /// from compensating another coordinator's still-valid reservation.
    pub fn reserve_batch_for_coordinator(
        &self,
        requests: Vec<PortLeaseRequest>,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        self.reserve_batch_with_limit(requests, None, Some(reservation_claim))
    }

    /// Atomically reserve a batch while enforcing an upper-layer tenant limit.
    ///
    /// The caller owns the policy decision and supplies its exact tenant and
    /// maximum. The authority counts every non-terminal
    /// [`PortLeaseAccounting::TenantPublished`] record under the same store
    /// transaction that creates new reservations. Exact request replay adds no
    /// usage and remains legal if a later policy lowers the limit below current
    /// live usage.
    pub fn reserve_batch_with_tenant_limit(
        &self,
        requests: Vec<PortLeaseRequest>,
        limit: TenantPublishedPortLimit,
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        self.reserve_batch_with_limit(requests, Some(limit), None)
    }

    /// Reserve a tenant-limited launch batch for one exact coordinator.
    pub fn reserve_batch_with_tenant_limit_for_coordinator(
        &self,
        requests: Vec<PortLeaseRequest>,
        limit: TenantPublishedPortLimit,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        self.reserve_batch_with_limit(requests, Some(limit), Some(reservation_claim))
    }

    /// Atomically verify a coordinator's complete still-never-bound batch.
    ///
    /// Effect-owning adapters use this as a preflight before any provider
    /// setup that precedes individual bind claims. It authenticates exact
    /// request identity and the attempt-unique compensation capability without
    /// mutating desired or durable state.
    pub fn verify_reserved_batch_for_coordinator(
        &self,
        requests: &[PortLeaseRequest],
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        self.transaction(|state| {
            let mut distinct = BTreeMap::<PortLeaseId, &PortLeaseRequest>::new();
            for request in requests {
                if let Some(previous) = distinct.insert(request.lease_id().clone(), request)
                    && previous != request
                {
                    return Err(PortLeaseOperationError::IdentityConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                let record = exact_record(state, request)?;
                if record.reservation_claim.as_ref() != Some(reservation_claim) {
                    return Err(PortLeaseOperationError::ReservationClaimConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                if record.bind_claim.is_some() {
                    return Err(PortLeaseOperationError::BindClaimConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                if record.phase != PortLeasePhase::Reserved {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase: record.phase,
                        operation: PortLeaseOperation::VerifyReservationClaim,
                    });
                }
            }
            requests
                .iter()
                .map(|request| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Atomically release a batch that is still proven never bound.
    ///
    /// This is the compensation path for a coordinator that reserved a
    /// complete launch batch but failed before handing any request to an effect
    /// provider. Every distinct request is authenticated and every record must
    /// still be `Reserved`, already `Released` by an identical retry, or
    /// terminal `Failed` with durable proof that the provider created no
    /// effect before any record changes. If a concurrent actor has adopted
    /// even one binding, the complete compensation fails without mutation.
    pub fn release_reserved_batch_without_effect(
        &self,
        requests: &[PortLeaseRequest],
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        self.transaction(|state| {
            for request in requests {
                let record = exact_record(state, request)?;
                if record.reservation_claim.as_ref() != Some(reservation_claim) {
                    return Err(PortLeaseOperationError::ReservationClaimConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                if record.bind_claim.is_some() {
                    return Err(PortLeaseOperationError::BindClaimConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                if !matches!(
                    record.phase,
                    PortLeasePhase::Reserved | PortLeasePhase::Released | PortLeasePhase::Failed
                ) {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase: record.phase,
                        operation: PortLeaseOperation::ReleaseReservedWithoutEffect,
                    });
                }
            }
            let distinct = requests
                .iter()
                .map(|request| (request.lease_id().clone(), request))
                .collect::<BTreeMap<_, _>>();
            for request in distinct.into_values() {
                let record = exact_record_mut(state, request)?;
                if record.phase == PortLeasePhase::Reserved {
                    record.phase = PortLeasePhase::Released;
                }
            }
            requests
                .iter()
                .map(|request| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Atomically claim one Nimbus-owned provider bind attempt.
    ///
    /// The claim is durable before the effect-owning adapter binds. A
    /// concurrent replay of the same stable request must acquire a distinct
    /// claim and is rejected without mutating the winner's authority.
    pub fn claim_bind(
        &self,
        request: &PortLeaseRequest,
        reservation_claim: Option<&NetworkReservationClaim>,
        claim: PortBindClaim,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        self.claim_bind_batch(&[(request.clone(), claim)], reservation_claim)
            .map(|mut records| {
                records
                    .pop()
                    .expect("one bind claim must return one durable record")
            })
    }

    /// Atomically claim a complete provider listener batch before any bind.
    ///
    /// Prevalidation covers every exact request and claim before mutation, so
    /// one process cannot own a partial machine-listener batch.
    pub fn claim_bind_batch(
        &self,
        claims: &[(PortLeaseRequest, PortBindClaim)],
        reservation_claim: Option<&NetworkReservationClaim>,
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        self.transaction(|state| {
            let mut distinct = BTreeMap::<PortLeaseId, (&PortLeaseRequest, &PortBindClaim)>::new();
            for (request, claim) in claims {
                if let Some((existing_request, existing_claim)) =
                    distinct.insert(request.lease_id().clone(), (request, claim))
                    && (existing_request != request || existing_claim != claim)
                {
                    return Err(PortLeaseOperationError::BindClaimConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                let record = exact_record(state, request)?;
                require_reservation_claim(record, reservation_claim)?;
                if record.phase != PortLeasePhase::Reserved {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase: record.phase,
                        operation: PortLeaseOperation::ClaimBind,
                    });
                }
                if record
                    .bind_claim
                    .as_ref()
                    .is_some_and(|existing| existing != claim)
                {
                    return Err(PortLeaseOperationError::BindClaimConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
            }
            for (request, claim) in distinct.into_values() {
                let record = exact_record_mut(state, request)?;
                if record.bind_claim.is_none() {
                    record.bind_claim = Some(claim.clone());
                }
            }
            claims
                .iter()
                .map(|(request, _)| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Relinquish exact claimed attempts after the adapter proved no effect.
    ///
    /// A terminal failed member is accepted only when its failure carries the
    /// same provider-attempt identity. This lets a failed batch retire its
    /// untouched claimed siblings without weakening another attempt's fence.
    pub fn abandon_bind_claims_without_effect(
        &self,
        claims: &[(PortLeaseRequest, PortBindClaim)],
        reservation_claim: Option<&NetworkReservationClaim>,
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        self.transaction(|state| {
            let mut distinct = BTreeMap::<PortLeaseId, (&PortLeaseRequest, &PortBindClaim)>::new();
            for (request, claim) in claims {
                if let Some((existing_request, existing_claim)) =
                    distinct.insert(request.lease_id().clone(), (request, claim))
                    && (existing_request != request || existing_claim != claim)
                {
                    return Err(PortLeaseOperationError::BindClaimConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                let record = exact_record(state, request)?;
                require_reservation_claim(record, reservation_claim)?;
                match record.phase {
                    PortLeasePhase::Reserved
                        if record
                            .bind_claim
                            .as_ref()
                            .is_none_or(|current| current == claim) => {}
                    PortLeasePhase::Failed
                        if record.failure.as_ref().is_some_and(|failure| {
                            failure.provider_attempt() == claim.provider_attempt()
                        }) => {}
                    phase => {
                        return Err(if phase == PortLeasePhase::Reserved {
                            PortLeaseOperationError::BindClaimConflict {
                                lease_id: request.lease_id().clone(),
                            }
                        } else {
                            PortLeaseOperationError::InvalidTransition {
                                lease_id: request.lease_id().clone(),
                                phase,
                                operation: PortLeaseOperation::AbandonBindClaimWithoutEffect,
                            }
                        });
                    }
                }
            }
            for (request, claim) in distinct.into_values() {
                let record = exact_record_mut(state, request)?;
                if record.bind_claim.as_ref() == Some(claim) {
                    record.bind_claim = None;
                }
            }
            claims
                .iter()
                .map(|(request, _)| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Adopt a concrete binding owned by the exact durable bind claim.
    ///
    /// Exact/range bindings must equal the atomically selected slot.
    /// Provider-assigned adoption atomically checks and records the provider's
    /// actual non-zero port before the lease may activate.
    pub fn adopt_claimed(
        &self,
        request: &PortLeaseRequest,
        reservation_claim: Option<&NetworkReservationClaim>,
        claim: &PortBindClaim,
        binding: PortLeaseBinding,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        self.adopt_with_claim(request, reservation_claim, claim, binding)
    }

    /// Atomically adopt and activate a complete claimed listener batch.
    ///
    /// The effect owner must already hold every concrete listener inertly.
    /// Every request, claim, selected port, and binding is prevalidated before
    /// mutation, so a launch cannot expose partial durable activation or lose
    /// cleanup authority between per-listener transactions.
    pub fn adopt_claimed_and_activate_batch(
        &self,
        bindings: &[(PortLeaseRequest, PortBindClaim, PortLeaseBinding)],
        reservation_claim: Option<&NetworkReservationClaim>,
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        self.transaction(|state| {
            let mut distinct = BTreeMap::<
                PortLeaseId,
                (&PortLeaseRequest, &PortBindClaim, &PortLeaseBinding),
            >::new();
            for (request, claim, binding) in bindings {
                if let Some((existing_request, existing_claim, existing_binding)) =
                    distinct.insert(request.lease_id().clone(), (request, claim, binding))
                    && (existing_request != request
                        || existing_claim != claim
                        || existing_binding != binding)
                {
                    return Err(PortLeaseOperationError::BindingConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                let record = exact_record(state, request)?;
                if !binding.provider_registration_matches_claim(claim) {
                    return Err(PortLeaseOperationError::BindClaimConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                if let Some(mismatch) = binding.mismatch(request.binding()) {
                    return Err(PortLeaseOperationError::BindingMismatch {
                        lease_id: request.lease_id().clone(),
                        mismatch,
                    });
                }
                if let Some(reserved_port) = record.reserved_port {
                    if binding.actual_port() != reserved_port {
                        return Err(PortLeaseOperationError::BindingMismatch {
                            lease_id: request.lease_id().clone(),
                            mismatch: PortBindingMismatch::Port,
                        });
                    }
                } else if !matches!(request.binding.port(), PortRequestMode::ProviderAssigned) {
                    return Err(PortLeaseOperationError::CorruptAuthority {
                        reason: format!(
                            "non-provider-assigned lease {} has no reserved port",
                            request.lease_id
                        ),
                    });
                }
                match record.phase {
                    PortLeasePhase::Reserved if record.bind_claim.as_ref() == Some(claim) => {
                        require_reservation_claim(record, reservation_claim)?;
                    }
                    PortLeasePhase::Reserved => {
                        return Err(PortLeaseOperationError::BindClaimConflict {
                            lease_id: request.lease_id().clone(),
                        });
                    }
                    PortLeasePhase::Active
                        if record.bind_claim.is_none()
                            && record.adoption_claim.as_ref() == Some(claim)
                            && record.binding.as_ref() == Some(binding) => {}
                    PortLeasePhase::Active => {
                        return Err(PortLeaseOperationError::BindClaimConflict {
                            lease_id: request.lease_id().clone(),
                        });
                    }
                    phase => {
                        return Err(PortLeaseOperationError::InvalidTransition {
                            lease_id: request.lease_id().clone(),
                            phase,
                            operation: PortLeaseOperation::Adopt,
                        });
                    }
                }
                if record.phase == PortLeasePhase::Reserved
                    && let Some(conflict) = state.conflicting_record(
                        request,
                        binding.actual_port(),
                        Some(request.lease_id()),
                    )
                {
                    return Err(port_conflict(request, conflict, binding.actual_port()));
                }
            }

            let prospective = distinct.values().copied().collect::<Vec<_>>();
            for (index, (request, _, binding)) in prospective.iter().enumerate() {
                for (existing_request, _, existing_binding) in &prospective[..index] {
                    if binding.actual_port() == existing_binding.actual_port()
                        && request.binding().overlaps(existing_request.binding())
                    {
                        let existing = exact_record(state, existing_request)?;
                        return Err(port_conflict(request, existing, binding.actual_port()));
                    }
                }
            }

            for (request, claim, binding) in distinct.into_values() {
                let record = exact_record_mut(state, request)?;
                if record.phase == PortLeasePhase::Reserved {
                    record.reserved_port = Some(binding.actual_port());
                    record.phase = PortLeasePhase::Active;
                    record.reservation_claim = None;
                    record.bind_claim = None;
                    record.adoption_claim = Some(claim.clone());
                    record.binding = Some(binding.clone());
                    record.confirmed_stopped_binding = None;
                    record.failure = None;
                }
            }
            bindings
                .iter()
                .map(|(request, _, _)| exact_record(state, request).cloned())
                .collect()
        })
    }

    fn adopt_with_claim(
        &self,
        request: &PortLeaseRequest,
        reservation_claim: Option<&NetworkReservationClaim>,
        claim: &PortBindClaim,
        binding: PortLeaseBinding,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        self.transaction(|state| {
            let existing = exact_record(state, request)?;
            if !binding.provider_registration_matches_claim(claim) {
                return Err(PortLeaseOperationError::BindClaimConflict {
                    lease_id: request.lease_id().clone(),
                });
            }
            match existing.phase {
                PortLeasePhase::Binding if existing.binding.as_ref() == Some(&binding) => {
                    if existing.bind_claim.as_ref() == Some(claim) {
                        return Ok(existing.clone());
                    }
                    return Err(PortLeaseOperationError::BindClaimConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                PortLeasePhase::Binding => {
                    return Err(PortLeaseOperationError::BindingConflict {
                        lease_id: request.lease_id.clone(),
                    });
                }
                PortLeasePhase::Reserved => {}
                phase => {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id.clone(),
                        phase,
                        operation: PortLeaseOperation::Adopt,
                    });
                }
            }
            require_reservation_claim(existing, reservation_claim)?;
            if existing.bind_claim.as_ref() != Some(claim) {
                return Err(PortLeaseOperationError::BindClaimConflict {
                    lease_id: request.lease_id().clone(),
                });
            }

            if let Some(mismatch) = binding.mismatch(request.binding()) {
                return Err(PortLeaseOperationError::BindingMismatch {
                    lease_id: request.lease_id.clone(),
                    mismatch,
                });
            }

            if let Some(reserved_port) = existing.reserved_port {
                if binding.actual_port() != reserved_port {
                    return Err(PortLeaseOperationError::BindingMismatch {
                        lease_id: request.lease_id.clone(),
                        mismatch: PortBindingMismatch::Port,
                    });
                }
            } else if !matches!(request.binding.port(), PortRequestMode::ProviderAssigned) {
                return Err(PortLeaseOperationError::CorruptAuthority {
                    reason: format!(
                        "non-provider-assigned lease {} has no reserved port",
                        request.lease_id
                    ),
                });
            }

            if let Some(conflict) =
                state.conflicting_record(request, binding.actual_port(), Some(request.lease_id()))
            {
                return Err(port_conflict(request, conflict, binding.actual_port()));
            }

            let record = exact_record_mut(state, request)?;
            record.reserved_port = Some(binding.actual_port());
            record.phase = PortLeasePhase::Binding;
            record.reservation_claim = None;
            record.adoption_claim = Some(claim.clone());
            record.binding = Some(binding);
            record.confirmed_stopped_binding = None;
            Ok(record.clone())
        })
    }

    /// Record a no-effect bind failure owned by the exact durable claim.
    ///
    /// The effect-owning adapter may call this only after proving the failed
    /// attempt created no resource requiring cleanup. Ambiguous effects belong
    /// in `CleanupPending` reconciliation. This method itself performs no bind,
    /// close, or provider call. A failed lease is inspectable and cannot
    /// activate.
    pub fn record_claimed_bind_failure_without_effect(
        &self,
        request: &PortLeaseRequest,
        reservation_claim: Option<&NetworkReservationClaim>,
        claim: &PortBindClaim,
        failure: PortBindFailure,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        self.record_bind_failure_with_claim(request, reservation_claim, claim, failure)
    }

    fn record_bind_failure_with_claim(
        &self,
        request: &PortLeaseRequest,
        reservation_claim: Option<&NetworkReservationClaim>,
        claim: &PortBindClaim,
        failure: PortBindFailure,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        self.transaction(|state| {
            let existing = exact_record(state, request)?;
            match existing.phase {
                PortLeasePhase::Failed if existing.failure.as_ref() == Some(&failure) => {
                    require_reservation_claim(existing, reservation_claim)?;
                    if failure.provider_attempt() == claim.provider_attempt() {
                        return Ok(existing.clone());
                    }
                    return Err(PortLeaseOperationError::BindClaimConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                PortLeasePhase::Failed => {
                    return Err(PortLeaseOperationError::BindFailureConflict {
                        lease_id: request.lease_id.clone(),
                    });
                }
                PortLeasePhase::Reserved => {}
                phase => {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id.clone(),
                        phase,
                        operation: PortLeaseOperation::RecordBindFailureWithoutEffect,
                    });
                }
            }
            require_reservation_claim(existing, reservation_claim)?;
            if existing.bind_claim.as_ref() != Some(claim)
                || failure.provider_attempt() != claim.provider_attempt()
            {
                return Err(PortLeaseOperationError::BindClaimConflict {
                    lease_id: request.lease_id().clone(),
                });
            }

            if let Some(mismatch) = failure.mismatch(request.binding()) {
                return Err(PortLeaseOperationError::BindingMismatch {
                    lease_id: request.lease_id.clone(),
                    mismatch,
                });
            }
            if let Some(reserved_port) = existing.reserved_port
                && failure.attempt().port() != reserved_port.get()
            {
                return Err(PortLeaseOperationError::BindingMismatch {
                    lease_id: request.lease_id.clone(),
                    mismatch: PortBindingMismatch::Port,
                });
            }

            let record = exact_record_mut(state, request)?;
            record.phase = PortLeasePhase::Failed;
            record.bind_claim = None;
            record.adoption_claim = None;
            record.confirmed_stopped_binding = None;
            record.failure = Some(failure);
            Ok(record.clone())
        })
    }

    /// Activate a durably adopted binding owned by the exact provider attempt.
    pub fn activate_claimed(
        &self,
        request: &PortLeaseRequest,
        claim: &PortBindClaim,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        self.transaction(|state| {
            let record = exact_record_mut(state, request)?;
            match record.phase {
                PortLeasePhase::Binding
                    if record.binding.is_some()
                        && record.bind_claim.as_ref() == Some(claim)
                        && record.adoption_claim.as_ref() == Some(claim) =>
                {
                    record.phase = PortLeasePhase::Active;
                    record.reservation_claim = None;
                    record.bind_claim = None;
                }
                PortLeasePhase::Binding | PortLeasePhase::Active
                    if record.adoption_claim.as_ref() != Some(claim) =>
                {
                    return Err(PortLeaseOperationError::BindClaimConflict {
                        lease_id: request.lease_id.clone(),
                    });
                }
                PortLeasePhase::Active => {}
                phase => {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id.clone(),
                        phase,
                        operation: PortLeaseOperation::Activate,
                    });
                }
            }
            Ok(record.clone())
        })
    }

    /// Fence new use and enter withdrawal.
    pub fn withdraw(&self, request: &PortLeaseRequest) -> Result<PortLeaseRecord, PortLeaseError> {
        self.transaction(|state| {
            let record = exact_record_mut(state, request)?;
            match record.phase {
                PortLeasePhase::Reserved if record.reservation_claim.is_some() => {
                    return Err(PortLeaseOperationError::ReservationClaimConflict {
                        lease_id: request.lease_id.clone(),
                    });
                }
                PortLeasePhase::Reserved if record.bind_claim.is_some() => {
                    return Err(PortLeaseOperationError::BindClaimConflict {
                        lease_id: request.lease_id.clone(),
                    });
                }
                PortLeasePhase::Reserved | PortLeasePhase::Binding | PortLeasePhase::Active => {
                    record.phase = PortLeasePhase::Withdrawing;
                    record.reservation_claim = None;
                    record.bind_claim = None;
                    record.confirmed_stopped_binding = None;
                }
                PortLeasePhase::Withdrawing => {}
                phase => {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id.clone(),
                        phase,
                        operation: PortLeaseOperation::Withdraw,
                    });
                }
            }
            Ok(record.clone())
        })
    }

    /// Confirm terminal release after the effect owner has completed unbind.
    ///
    /// This method records authority only; it performs no provider effect.
    /// NNC3.8 adds explicit ambiguous-unbind/cleanup-pending reconciliation
    /// before any production effect owner is allowed to rely on reuse.
    pub fn release(&self, request: &PortLeaseRequest) -> Result<PortLeaseRecord, PortLeaseError> {
        self.transaction(|state| {
            let record = exact_record_mut(state, request)?;
            match record.phase {
                PortLeasePhase::Withdrawing => {
                    record.phase = PortLeasePhase::Released;
                    record.reservation_claim = None;
                    record.confirmed_stopped_binding = None;
                }
                PortLeasePhase::Released => {}
                phase => {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id.clone(),
                        phase,
                        operation: PortLeaseOperation::Release,
                    });
                }
            }
            Ok(record.clone())
        })
    }

    /// Inspect one durable lease by stable identity.
    pub fn inspect(
        &self,
        lease_id: &PortLeaseId,
    ) -> Result<Option<PortLeaseRecord>, PortLeaseError> {
        let state = self.load_state()?;
        Ok(state.leases.get(lease_id).cloned())
    }

    /// List durable leases in stable ID order.
    pub fn list(&self) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let state = self.load_state()?;
        Ok(state.leases.into_values().collect())
    }

    fn load_state(&self) -> Result<PortLeaseState, PortLeaseError> {
        let state: PortLeaseState = self
            .store
            .read(&NetworkStatePartition::PortLeases)
            .map_err(PortLeaseError::Store)?
            .unwrap_or_default();
        state.validate().map_err(PortLeaseError::from)?;
        Ok(state)
    }

    fn transaction<Output>(
        &self,
        operation: impl FnOnce(&mut PortLeaseState) -> Result<Output, PortLeaseOperationError>,
    ) -> Result<Output, PortLeaseError> {
        self.store
            .transaction(
                &NetworkStatePartition::PortLeases,
                |state: &mut PortLeaseState| {
                    state.validate()?;
                    let record = operation(state)?;
                    state.validate()?;
                    Ok(record)
                },
            )
            .map_err(PortLeaseError::from_transaction)
    }

    fn reserve_batch_with_limit(
        &self,
        requests: Vec<PortLeaseRequest>,
        limit: Option<TenantPublishedPortLimit>,
        reservation_claim: Option<&NetworkReservationClaim>,
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        self.transaction(|state| {
            let mut distinct = BTreeMap::<PortLeaseId, &PortLeaseRequest>::new();
            for request in &requests {
                validate_request_accounting(request)?;
                if let Some(previous) = distinct.insert(request.lease_id().clone(), request)
                    && previous != request
                {
                    return Err(PortLeaseOperationError::IdentityConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                if let Some(existing) = state.leases.get(request.lease_id())
                    && existing.request() != request
                {
                    return Err(PortLeaseOperationError::IdentityConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
            }

            if let Some(limit) = &limit {
                for request in distinct
                    .values()
                    .copied()
                    .filter(|request| request.accounting() == PortLeaseAccounting::TenantPublished)
                {
                    let actual_tenant_id = request.tenant_id().ok_or_else(|| {
                        PortLeaseOperationError::TenantAttributionRequired {
                            lease_id: request.lease_id().clone(),
                        }
                    })?;
                    if actual_tenant_id != limit.tenant_id() {
                        return Err(PortLeaseOperationError::TenantLimitScopeMismatch {
                            expected_tenant_id: limit.tenant_id().clone(),
                            request_lease_id: request.lease_id().clone(),
                            actual_tenant_id: actual_tenant_id.clone(),
                        });
                    }
                }
                let current_live = state
                    .leases
                    .values()
                    .filter(|record| {
                        !record.phase().is_terminal()
                            && record.request().accounting() == PortLeaseAccounting::TenantPublished
                            && record.request().tenant_id() == Some(limit.tenant_id())
                    })
                    .count();
                let additional = distinct
                    .values()
                    .filter(|request| {
                        request.accounting() == PortLeaseAccounting::TenantPublished
                            && request.tenant_id() == Some(limit.tenant_id())
                            && !state.leases.contains_key(request.lease_id())
                    })
                    .count();
                if additional > limit.maximum().saturating_sub(current_live) {
                    return Err(PortLeaseOperationError::TenantPublishedPortLimitExceeded {
                        tenant_id: limit.tenant_id().clone(),
                        current_live,
                        additional,
                        maximum: limit.maximum(),
                    });
                }
            }

            requests
                .into_iter()
                .map(|request| state.reserve_request(request, reservation_claim))
                .collect()
        })
    }
}

fn validate_request_accounting(request: &PortLeaseRequest) -> Result<(), PortLeaseOperationError> {
    match (request.accounting(), request.publication()) {
        (PortLeaseAccounting::TenantPublished, PortPublicationIntent::Host { .. }) => {
            if request.tenant_id().is_none() {
                return Err(PortLeaseOperationError::TenantAttributionRequired {
                    lease_id: request.lease_id().clone(),
                });
            }
        }
        (PortLeaseAccounting::HostInternal, PortPublicationIntent::Unpublished) => {}
        _ => {
            return Err(PortLeaseOperationError::InvalidPublicationAccounting {
                lease_id: request.lease_id().clone(),
            });
        }
    }
    Ok(())
}

fn require_reservation_claim(
    record: &PortLeaseRecord,
    supplied: Option<&NetworkReservationClaim>,
) -> Result<(), PortLeaseOperationError> {
    if record.reservation_claim.as_ref() == supplied {
        Ok(())
    } else {
        Err(PortLeaseOperationError::ReservationClaimConflict {
            lease_id: record.request.lease_id().clone(),
        })
    }
}

fn exact_record<'a>(
    state: &'a PortLeaseState,
    request: &PortLeaseRequest,
) -> Result<&'a PortLeaseRecord, PortLeaseOperationError> {
    let record =
        state
            .leases
            .get(request.lease_id())
            .ok_or_else(|| PortLeaseOperationError::NotFound {
                lease_id: request.lease_id.clone(),
            })?;
    verify_exact_request(record, request)?;
    Ok(record)
}

fn exact_record_mut<'a>(
    state: &'a mut PortLeaseState,
    request: &PortLeaseRequest,
) -> Result<&'a mut PortLeaseRecord, PortLeaseOperationError> {
    let record = state.leases.get_mut(request.lease_id()).ok_or_else(|| {
        PortLeaseOperationError::NotFound {
            lease_id: request.lease_id.clone(),
        }
    })?;
    verify_exact_request(record, request)?;
    Ok(record)
}

fn verify_exact_request(
    record: &PortLeaseRecord,
    request: &PortLeaseRequest,
) -> Result<(), PortLeaseOperationError> {
    if record.request != *request {
        return Err(PortLeaseOperationError::StaleFence(Box::new(
            PortLeaseFenceMismatch {
                expected: record.request.clone(),
                candidate: request.clone(),
            },
        )));
    }
    Ok(())
}

fn port_conflict(
    request: &PortLeaseRequest,
    existing: &PortLeaseRecord,
    conflicting_port: NonZeroU16,
) -> PortLeaseOperationError {
    PortLeaseOperationError::PortConflict {
        conflicting_port,
        requested_lease_id: request.lease_id.clone(),
        requested_owner_id: request.owner_id.clone(),
        existing_lease_id: existing.request.lease_id.clone(),
        existing_owner_id: existing.request.owner_id.clone(),
        existing_phase: existing.phase,
    }
}

/// Durable authority or lifecycle rejection.
#[derive(Debug)]
pub enum PortLeaseError {
    /// The shared store could not be safely read or committed.
    Store(NetworkStateStoreError),
    /// The checksum-valid durable authority violates lease invariants.
    CorruptAuthority { reason: String },
    /// No durable lease has this stable identity.
    NotFound { lease_id: PortLeaseId },
    /// One lease ID was reused with different immutable reservation identity.
    IdentityConflict { lease_id: PortLeaseId },
    /// A tenant-published request omitted tenant attribution.
    TenantAttributionRequired { lease_id: PortLeaseId },
    /// Publication intent does not match the request's accounting class.
    InvalidPublicationAccounting { lease_id: PortLeaseId },
    /// A metered request fell outside the caller-supplied tenant decision.
    TenantLimitScopeMismatch {
        expected_tenant_id: TenantId,
        request_lease_id: PortLeaseId,
        actual_tenant_id: TenantId,
    },
    /// New published requests would exceed the atomic tenant limit.
    TenantPublishedPortLimitExceeded {
        tenant_id: TenantId,
        current_live: usize,
        additional: usize,
        maximum: usize,
    },
    /// Another non-terminal lease already fences the requested host port.
    PortConflict {
        conflicting_port: NonZeroU16,
        requested_lease_id: PortLeaseId,
        requested_owner_id: NetworkResourceId,
        existing_lease_id: PortLeaseId,
        existing_owner_id: NetworkResourceId,
        existing_phase: PortLeasePhase,
    },
    /// Every port in an overlapping requested range is fenced.
    PortRangeExhausted {
        requested_lease_id: PortLeaseId,
        requested_owner_id: NetworkResourceId,
        requested_range: PortRange,
    },
    /// A generation/epoch/owner request did not match durable authority.
    StaleFence(Box<PortLeaseFenceMismatch>),
    /// Concrete provider evidence does not satisfy the durable request.
    BindingMismatch {
        lease_id: PortLeaseId,
        mismatch: PortBindingMismatch,
    },
    /// A second adoption supplied different provider evidence.
    BindingConflict { lease_id: PortLeaseId },
    /// A second failed-bind report supplied different provider evidence.
    BindFailureConflict { lease_id: PortLeaseId },
    /// Another attempt owns the durable pre-bind claim.
    BindClaimConflict { lease_id: PortLeaseId },
    /// Another coordinator owns never-bound compensation authority.
    ReservationClaimConflict { lease_id: PortLeaseId },
    /// The requested operation is not legal from the durable phase.
    InvalidTransition {
        lease_id: PortLeaseId,
        phase: PortLeasePhase,
        operation: PortLeaseOperation,
    },
}

impl PortLeaseError {
    fn from_transaction(error: NetworkStateTransactionError<PortLeaseOperationError>) -> Self {
        match error {
            NetworkStateTransactionError::Store(error) => Self::Store(error),
            NetworkStateTransactionError::Operation(error) => error.into(),
        }
    }
}

impl From<PortLeaseOperationError> for PortLeaseError {
    fn from(error: PortLeaseOperationError) -> Self {
        match error {
            PortLeaseOperationError::CorruptAuthority { reason } => {
                Self::CorruptAuthority { reason }
            }
            PortLeaseOperationError::NotFound { lease_id } => Self::NotFound { lease_id },
            PortLeaseOperationError::IdentityConflict { lease_id } => {
                Self::IdentityConflict { lease_id }
            }
            PortLeaseOperationError::TenantAttributionRequired { lease_id } => {
                Self::TenantAttributionRequired { lease_id }
            }
            PortLeaseOperationError::InvalidPublicationAccounting { lease_id } => {
                Self::InvalidPublicationAccounting { lease_id }
            }
            PortLeaseOperationError::TenantLimitScopeMismatch {
                expected_tenant_id,
                request_lease_id,
                actual_tenant_id,
            } => Self::TenantLimitScopeMismatch {
                expected_tenant_id,
                request_lease_id,
                actual_tenant_id,
            },
            PortLeaseOperationError::TenantPublishedPortLimitExceeded {
                tenant_id,
                current_live,
                additional,
                maximum,
            } => Self::TenantPublishedPortLimitExceeded {
                tenant_id,
                current_live,
                additional,
                maximum,
            },
            PortLeaseOperationError::PortConflict {
                conflicting_port,
                requested_lease_id,
                requested_owner_id,
                existing_lease_id,
                existing_owner_id,
                existing_phase,
            } => Self::PortConflict {
                conflicting_port,
                requested_lease_id,
                requested_owner_id,
                existing_lease_id,
                existing_owner_id,
                existing_phase,
            },
            PortLeaseOperationError::PortRangeExhausted {
                requested_lease_id,
                requested_owner_id,
                requested_range,
            } => Self::PortRangeExhausted {
                requested_lease_id,
                requested_owner_id,
                requested_range,
            },
            PortLeaseOperationError::StaleFence(mismatch) => Self::StaleFence(mismatch),
            PortLeaseOperationError::BindingMismatch { lease_id, mismatch } => {
                Self::BindingMismatch { lease_id, mismatch }
            }
            PortLeaseOperationError::BindingConflict { lease_id } => {
                Self::BindingConflict { lease_id }
            }
            PortLeaseOperationError::BindFailureConflict { lease_id } => {
                Self::BindFailureConflict { lease_id }
            }
            PortLeaseOperationError::BindClaimConflict { lease_id } => {
                Self::BindClaimConflict { lease_id }
            }
            PortLeaseOperationError::ReservationClaimConflict { lease_id } => {
                Self::ReservationClaimConflict { lease_id }
            }
            PortLeaseOperationError::InvalidTransition {
                lease_id,
                phase,
                operation,
            } => Self::InvalidTransition {
                lease_id,
                phase,
                operation,
            },
        }
    }
}

impl Display for PortLeaseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => Display::fmt(error, formatter),
            Self::CorruptAuthority { reason } => {
                write!(formatter, "port lease authority is corrupt: {reason}")
            }
            Self::NotFound { lease_id } => {
                write!(formatter, "port lease {lease_id} does not exist")
            }
            Self::IdentityConflict { lease_id } => write!(
                formatter,
                "port lease {lease_id} was reused with different immutable reservation identity"
            ),
            Self::TenantAttributionRequired { lease_id } => write!(
                formatter,
                "tenant-published port lease {lease_id} requires tenant attribution"
            ),
            Self::InvalidPublicationAccounting { lease_id } => write!(
                formatter,
                "port lease {lease_id} publication intent does not match its accounting class"
            ),
            Self::TenantLimitScopeMismatch {
                expected_tenant_id,
                request_lease_id,
                actual_tenant_id,
            } => write!(
                formatter,
                "tenant-published port lease {request_lease_id} belongs to tenant \
                 {actual_tenant_id}, outside the supplied limit for tenant {expected_tenant_id}"
            ),
            Self::TenantPublishedPortLimitExceeded {
                tenant_id,
                current_live,
                additional,
                maximum,
            } => write!(
                formatter,
                "published port quota exceeded for tenant {tenant_id}: {current_live} live plus \
                 {additional} new leases exceeds limit {maximum}"
            ),
            Self::PortConflict {
                conflicting_port,
                requested_lease_id,
                requested_owner_id,
                existing_lease_id,
                existing_owner_id,
                existing_phase,
            } => write!(
                formatter,
                "port {} requested by lease {} owner {:?} conflicts with lease {} owner {:?} in \
                 phase {:?}",
                conflicting_port,
                requested_lease_id,
                requested_owner_id,
                existing_lease_id,
                existing_owner_id,
                existing_phase
            ),
            Self::PortRangeExhausted {
                requested_lease_id,
                requested_owner_id,
                requested_range,
            } => write!(
                formatter,
                "port range {}..={} requested by lease {} owner {:?} has no free slot in its \
                 overlap domain",
                requested_range.start(),
                requested_range.end(),
                requested_lease_id,
                requested_owner_id
            ),
            Self::StaleFence(mismatch) => write!(
                formatter,
                "port lease {} rejected stale or divergent fence: expected owner {:?} tenant {:?} \
                 accounting {:?} publication {:?} binding {:?} generation {} epoch {}, candidate \
                 owner {:?} tenant {:?} accounting {:?} publication {:?} binding {:?} generation \
                 {} epoch {}",
                mismatch.expected.lease_id,
                mismatch.expected.owner_id,
                mismatch.expected.tenant_id,
                mismatch.expected.accounting,
                mismatch.expected.publication,
                mismatch.expected.binding,
                mismatch.expected.generation.as_u64(),
                mismatch.expected.lease_epoch.as_u64(),
                mismatch.candidate.owner_id,
                mismatch.candidate.tenant_id,
                mismatch.candidate.accounting,
                mismatch.candidate.publication,
                mismatch.candidate.binding,
                mismatch.candidate.generation.as_u64(),
                mismatch.candidate.lease_epoch.as_u64()
            ),
            Self::BindingMismatch { lease_id, mismatch } => {
                write!(
                    formatter,
                    "port lease {lease_id} rejected provider evidence: {mismatch}"
                )
            }
            Self::BindingConflict { lease_id } => write!(
                formatter,
                "port lease {lease_id} already has different adopted provider evidence"
            ),
            Self::BindFailureConflict { lease_id } => write!(
                formatter,
                "port lease {lease_id} already has different failed-bind evidence"
            ),
            Self::BindClaimConflict { lease_id } => write!(
                formatter,
                "port lease {lease_id} is owned by a different in-flight provider bind attempt"
            ),
            Self::ReservationClaimConflict { lease_id } => write!(
                formatter,
                "port lease {lease_id} is owned by a different launch reservation coordinator"
            ),
            Self::InvalidTransition {
                lease_id,
                phase,
                operation,
            } => write!(
                formatter,
                "port lease {lease_id} cannot {operation} from phase {phase:?}"
            ),
        }
    }
}

impl StdError for PortLeaseError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::BindingMismatch { mismatch, .. } => Some(mismatch),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "port_lease/tests.rs"]
mod tests;
