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
    LocalNetworkStateStore, NetworkLeaseEpoch, NetworkResourceGeneration, NetworkResourceId,
    NetworkStatePartition, NetworkStateStoreError, NetworkStateTransactionError, PortLeaseId,
};

mod binding;
mod request;

pub use binding::{
    PortBindAttempt, PortBindAttemptError, PortBindFailure, PortBindFailureKind,
    PortBindingMismatch, PortBindingProvenance, PortBoundEndpoint, PortBoundEndpointError,
    PortLeaseBinding,
};
pub use request::{
    PortAddressFamily, PortBindRealm, PortBindRealmError, PortBindRealmErrorKind, PortBindTarget,
    PortBindTargetError, PortBindingSpec, PortExposure, PortIpv6Overlap, PortIsolatedRealm,
    PortProtocol, PortRange, PortRangeError, PortRequestMode,
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
    binding: PortBindingSpec,
}

impl PortLeaseRequest {
    /// Construct one immutable host-port lease request.
    pub fn new(
        lease_id: PortLeaseId,
        owner_id: NetworkResourceId,
        tenant_id: Option<TenantId>,
        generation: NetworkResourceGeneration,
        lease_epoch: NetworkLeaseEpoch,
        binding: PortBindingSpec,
    ) -> Self {
        Self {
            lease_id,
            owner_id,
            tenant_id,
            generation,
            lease_epoch,
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
    binding: Option<PortLeaseBinding>,
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

    /// Adopted concrete binding, when one has been recorded.
    pub fn binding(&self) -> Option<&PortLeaseBinding> {
        self.binding.as_ref()
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
    /// Replaying the same immutable request returns its existing record.
    /// Reusing a lease ID with different identity/fence data fails closed.
    /// Every non-terminal record with a selected slot fences it, including
    /// `Withdrawing` and `CleanupPending`. Range requests select the lowest
    /// available slot in their complete overlap domain. Provider-assigned
    /// requests acquire a numeric fence only during adoption.
    pub fn reserve(&self, request: PortLeaseRequest) -> Result<PortLeaseRecord, PortLeaseError> {
        self.transaction(|state| {
            if let Some(existing) = state.leases.get(request.lease_id()) {
                if existing.request == request {
                    return Ok(existing.clone());
                }
                return Err(PortLeaseOperationError::IdentityConflict {
                    lease_id: request.lease_id.clone(),
                });
            }

            let reserved_port = state.reserve_port(&request)?;

            let record = PortLeaseRecord {
                request: request.clone(),
                reserved_port,
                phase: PortLeasePhase::Reserved,
                binding: None,
                failure: None,
            };
            state
                .leases
                .insert(request.lease_id.clone(), record.clone());
            Ok(record)
        })
    }

    /// Durably adopt a concrete provider binding without making it active.
    ///
    /// Exact/range bindings must equal the atomically selected slot.
    /// Provider-assigned adoption atomically checks and records the provider's
    /// actual non-zero port before the lease may activate.
    pub fn adopt(
        &self,
        request: &PortLeaseRequest,
        binding: PortLeaseBinding,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        self.transaction(|state| {
            let existing = exact_record(state, request)?;
            match existing.phase {
                PortLeasePhase::Binding if existing.binding.as_ref() == Some(&binding) => {
                    return Ok(existing.clone());
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
            record.binding = Some(binding);
            Ok(record.clone())
        })
    }

    /// Durably record a confirmed no-effect bind failure without publication.
    ///
    /// The effect-owning adapter may call this only after proving the failed
    /// attempt created no resource requiring cleanup. Ambiguous effects belong
    /// in `CleanupPending` reconciliation. This method itself performs no bind,
    /// close, or provider call. A failed lease is inspectable and cannot
    /// activate.
    pub fn record_bind_failure_without_effect(
        &self,
        request: &PortLeaseRequest,
        failure: PortBindFailure,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        self.transaction(|state| {
            let existing = exact_record(state, request)?;
            match existing.phase {
                PortLeasePhase::Failed if existing.failure.as_ref() == Some(&failure) => {
                    return Ok(existing.clone());
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
            record.failure = Some(failure);
            Ok(record.clone())
        })
    }

    /// Activate a durably adopted binding.
    pub fn activate(&self, request: &PortLeaseRequest) -> Result<PortLeaseRecord, PortLeaseError> {
        self.transaction(|state| {
            let record = exact_record_mut(state, request)?;
            match record.phase {
                PortLeasePhase::Binding if record.binding.is_some() => {
                    record.phase = PortLeasePhase::Active;
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
                PortLeasePhase::Reserved | PortLeasePhase::Binding | PortLeasePhase::Active => {
                    record.phase = PortLeasePhase::Withdrawing;
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

    fn transaction(
        &self,
        operation: impl FnOnce(&mut PortLeaseState) -> Result<PortLeaseRecord, PortLeaseOperationError>,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
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

/// Expected and rejected immutable requests carried by a stale-fence error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortLeaseFenceMismatch {
    expected: PortLeaseRequest,
    candidate: PortLeaseRequest,
}

impl PortLeaseFenceMismatch {
    /// Current durable lease identity and fence.
    pub fn expected(&self) -> &PortLeaseRequest {
        &self.expected
    }

    /// Rejected stale or divergent lease identity and fence.
    pub fn candidate(&self) -> &PortLeaseRequest {
        &self.candidate
    }
}

/// Named operation used by invalid-transition diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortLeaseOperation {
    /// Record a concrete provider binding.
    Adopt,
    /// Record a confirmed no-effect provider bind failure.
    RecordBindFailureWithoutEffect,
    /// Mark an adopted binding active.
    Activate,
    /// Fence new use.
    Withdraw,
    /// Confirm terminal release.
    Release,
}

impl Display for PortLeaseOperation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Adopt => "adopt",
            Self::RecordBindFailureWithoutEffect => "record bind failure without provider effect",
            Self::Activate => "activate",
            Self::Withdraw => "withdraw",
            Self::Release => "release",
        })
    }
}

#[derive(Debug)]
enum PortLeaseOperationError {
    CorruptAuthority {
        reason: String,
    },
    NotFound {
        lease_id: PortLeaseId,
    },
    IdentityConflict {
        lease_id: PortLeaseId,
    },
    PortConflict {
        conflicting_port: NonZeroU16,
        requested_lease_id: PortLeaseId,
        requested_owner_id: NetworkResourceId,
        existing_lease_id: PortLeaseId,
        existing_owner_id: NetworkResourceId,
        existing_phase: PortLeasePhase,
    },
    PortRangeExhausted {
        requested_lease_id: PortLeaseId,
        requested_owner_id: NetworkResourceId,
        requested_range: PortRange,
    },
    StaleFence(Box<PortLeaseFenceMismatch>),
    BindingMismatch {
        lease_id: PortLeaseId,
        mismatch: PortBindingMismatch,
    },
    BindingConflict {
        lease_id: PortLeaseId,
    },
    BindFailureConflict {
        lease_id: PortLeaseId,
    },
    InvalidTransition {
        lease_id: PortLeaseId,
        phase: PortLeasePhase,
        operation: PortLeaseOperation,
    },
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
                 binding {:?} generation {} epoch {}, candidate owner {:?} tenant {:?} binding {:?} \
                 generation {} epoch {}",
                mismatch.expected.lease_id,
                mismatch.expected.owner_id,
                mismatch.expected.tenant_id,
                mismatch.expected.binding,
                mismatch.expected.generation.as_u64(),
                mismatch.expected.lease_epoch.as_u64(),
                mismatch.candidate.owner_id,
                mismatch.candidate.tenant_id,
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
mod tests {
    use std::convert::Infallible;
    use std::sync::{Arc, Barrier};
    use std::thread;

    use crate::{ListenerId, NetworkProviderHandle, NetworkProviderId};

    use super::*;

    const PORT: u16 = 41_473;

    #[test]
    fn lifecycle_is_idempotent_fenced_and_restart_durable() {
        let root = tempfile::tempdir().expect("state root should exist");
        let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
        let request = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 7, 11, PORT);
        let binding = binding(PORT, "provider-binding-a");

        let reserved = authority
            .reserve(request.clone())
            .expect("reservation should commit");
        assert_eq!(reserved.phase(), PortLeasePhase::Reserved);
        assert_eq!(
            authority
                .reserve(request.clone())
                .expect("reservation replay should be idempotent"),
            reserved
        );

        let activation_error = authority
            .activate(&request)
            .expect_err("activation before adoption must fail");
        assert!(matches!(
            activation_error,
            PortLeaseError::InvalidTransition {
                phase: PortLeasePhase::Reserved,
                operation: PortLeaseOperation::Activate,
                ..
            }
        ));

        let adopted = authority
            .adopt(&request, binding.clone())
            .expect("binding should be adopted");
        assert_eq!(adopted.phase(), PortLeasePhase::Binding);
        assert_eq!(adopted.binding(), Some(&binding));
        assert_eq!(
            authority
                .adopt(&request, binding.clone())
                .expect("adoption replay should be idempotent"),
            adopted
        );

        let active = authority
            .activate(&request)
            .expect("adopted binding should activate");
        assert_eq!(active.phase(), PortLeasePhase::Active);
        assert_eq!(
            authority
                .activate(&request)
                .expect("activation replay should be idempotent"),
            active
        );

        let withdrawing = authority
            .withdraw(&request)
            .expect("active lease should withdraw");
        assert_eq!(withdrawing.phase(), PortLeasePhase::Withdrawing);
        assert_eq!(
            authority
                .withdraw(&request)
                .expect("withdraw replay should be idempotent"),
            withdrawing
        );

        let released = authority
            .release(&request)
            .expect("withdrawn lease should release");
        assert_eq!(released.phase(), PortLeasePhase::Released);
        assert_eq!(
            authority
                .release(&request)
                .expect("release replay should be idempotent"),
            released
        );

        drop(authority);
        let restarted =
            LocalPortLeaseAuthority::open(root.path()).expect("authority should restart");
        assert_eq!(
            restarted
                .inspect(request.lease_id())
                .expect("lease should inspect"),
            Some(released)
        );
    }

    #[test]
    fn divergent_identity_and_stale_fence_fail_without_mutation() {
        let root = tempfile::tempdir().expect("state root should exist");
        let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
        let original = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 7, 11, PORT);
        let divergent_owner = request_with_owner(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "01ARZ3NDEKTSV4RRFFQ69G5FAW",
            7,
            11,
            PORT,
        );
        let stale_epoch = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 7, 10, PORT);

        let reserved = authority
            .reserve(original.clone())
            .expect("original reservation should commit");
        assert!(matches!(
            authority.reserve(divergent_owner.clone()),
            Err(PortLeaseError::IdentityConflict { .. })
        ));
        assert!(matches!(
            authority.withdraw(&divergent_owner),
            Err(PortLeaseError::StaleFence(mismatch))
                if mismatch.expected().owner_id() != mismatch.candidate().owner_id()
        ));
        assert!(matches!(
            authority.withdraw(&stale_epoch),
            Err(PortLeaseError::StaleFence(mismatch))
                if mismatch.expected().lease_epoch() == NetworkLeaseEpoch::new(11)
                    && mismatch.candidate().lease_epoch() == NetworkLeaseEpoch::new(10)
        ));
        assert_eq!(
            authority
                .inspect(original.lease_id())
                .expect("original should inspect"),
            Some(reserved)
        );
    }

    #[test]
    fn non_terminal_records_conflict_and_release_permits_new_identity() {
        let root = tempfile::tempdir().expect("state root should exist");
        let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
        let first = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 1, 1, PORT);
        let second = request("01ARZ3NDEKTSV4RRFFQ69G5FAW", 1, 1, PORT);

        authority
            .reserve(first.clone())
            .expect("first reservation should commit");
        let conflict = authority
            .reserve(second.clone())
            .expect_err("second reservation must conflict");
        assert!(matches!(
            conflict,
            PortLeaseError::PortConflict {
                conflicting_port,
                existing_phase: PortLeasePhase::Reserved,
                ..
            } if conflicting_port.get() == PORT
        ));

        authority
            .withdraw(&first)
            .expect("reserved lease should withdraw");
        assert!(
            matches!(
                authority.reserve(second.clone()),
                Err(PortLeaseError::PortConflict {
                    existing_phase: PortLeasePhase::Withdrawing,
                    ..
                })
            ),
            "withdrawal must retain the fence until terminal release"
        );
        authority
            .release(&first)
            .expect("withdrawn lease should release");
        let replacement = authority
            .reserve(second)
            .expect("new stable identity may reserve after release");
        assert_eq!(replacement.phase(), PortLeasePhase::Reserved);
        assert_eq!(authority.list().expect("leases should list").len(), 2);
    }

    #[test]
    fn separately_opened_thread_contenders_choose_exactly_one_winner() {
        let root = Arc::new(tempfile::tempdir().expect("state root should exist"));
        let barrier = Arc::new(Barrier::new(3));
        let mut threads = Vec::new();

        for (payload, owner_payload) in [
            ("01ARZ3NDEKTSV4RRFFQ69G5FAV", "01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            ("01ARZ3NDEKTSV4RRFFQ69G5FAW", "01ARZ3NDEKTSV4RRFFQ69G5FAW"),
        ] {
            let root = Arc::clone(&root);
            let barrier = Arc::clone(&barrier);
            threads.push(thread::spawn(move || {
                let authority = LocalPortLeaseAuthority::open(root.path())
                    .expect("thread authority should open");
                let request = request_with_owner(payload, owner_payload, 3, 5, PORT);
                barrier.wait();
                let reserved = authority.reserve(request.clone())?;
                assert_eq!(reserved.phase(), PortLeasePhase::Reserved);
                authority.adopt(&request, binding(PORT, payload))?;
                authority.activate(&request)
            }));
        }

        barrier.wait();
        let outcomes = threads
            .into_iter()
            .map(|thread| thread.join().expect("contender should not panic"))
            .collect::<Vec<_>>();
        assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            outcomes
                .iter()
                .filter(|result| matches!(result, Err(PortLeaseError::PortConflict { .. })))
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter_map(|result| result.as_ref().ok())
                .filter(|record| record.phase() == PortLeasePhase::Active)
                .count(),
            1
        );
        let authority =
            LocalPortLeaseAuthority::open(root.path()).expect("authority should reopen");
        let records = authority.list().expect("leases should list");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].phase(), PortLeasePhase::Active);
    }

    #[test]
    fn exact_adoption_rejects_a_different_actual_port_and_provider_rewrite() {
        let root = tempfile::tempdir().expect("state root should exist");
        let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
        let request = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 1, 1, PORT);
        authority
            .reserve(request.clone())
            .expect("reservation should commit");

        assert!(matches!(
            authority.adopt(&request, binding(PORT + 1, "wrong-port")),
            Err(PortLeaseError::BindingMismatch {
                mismatch: PortBindingMismatch::Port,
                ..
            })
        ));
        authority
            .adopt(&request, binding(PORT, "provider-binding-a"))
            .expect("matching port should adopt");
        assert!(matches!(
            authority.adopt(&request, binding(PORT, "provider-binding-b")),
            Err(PortLeaseError::BindingConflict { .. })
        ));
    }

    #[test]
    fn bind_failure_is_idempotent_durable_and_cannot_activate() {
        let root = tempfile::tempdir().expect("state root should exist");
        let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
        let request = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 1, 1, PORT);
        let failure = bind_failure(PORT, "bind-attempt-a");
        authority
            .reserve(request.clone())
            .expect("reservation should commit");

        let failed = authority
            .record_bind_failure_without_effect(&request, failure.clone())
            .expect("bind failure should commit");
        assert_eq!(failed.phase(), PortLeasePhase::Failed);
        assert_eq!(failed.failure(), Some(&failure));
        assert_eq!(failed.binding(), None);
        assert_eq!(
            authority
                .record_bind_failure_without_effect(&request, failure.clone())
                .expect("same failed-bind evidence should be idempotent"),
            failed
        );
        assert!(matches!(
            authority.record_bind_failure_without_effect(
                &request,
                bind_failure(PORT, "different-bind-attempt")
            ),
            Err(PortLeaseError::BindFailureConflict { .. })
        ));
        assert!(matches!(
            authority.activate(&request),
            Err(PortLeaseError::InvalidTransition {
                phase: PortLeasePhase::Failed,
                operation: PortLeaseOperation::Activate,
                ..
            })
        ));
        assert!(matches!(
            authority.adopt(&request, binding(PORT, "late-binding")),
            Err(PortLeaseError::InvalidTransition {
                phase: PortLeasePhase::Failed,
                operation: PortLeaseOperation::Adopt,
                ..
            })
        ));

        drop(authority);
        let restarted =
            LocalPortLeaseAuthority::open(root.path()).expect("authority should restart");
        let durable = restarted
            .inspect(request.lease_id())
            .expect("failed lease should inspect")
            .expect("failed lease should remain durable");
        assert_eq!(durable.phase(), PortLeasePhase::Failed);
        assert_eq!(durable.failure(), Some(&failure));
    }

    #[test]
    fn checksum_valid_semantically_corrupt_authority_fails_closed() {
        for (corruption, expected_reason) in [
            (CorruptionFixture::MapKeyMismatch, "does not match"),
            (CorruptionFixture::ActiveWithoutBinding, "has no binding"),
            (CorruptionFixture::BindingPortMismatch, "reserves Some"),
            (
                CorruptionFixture::ExactWithoutReservedPort,
                "incompatible with request",
            ),
            (
                CorruptionFixture::ExactWrongReservedPort,
                "incompatible with request",
            ),
            (CorruptionFixture::FailedWithBinding, "terminal failed"),
            (
                CorruptionFixture::FailedWithoutFailure,
                "has no failure evidence",
            ),
            (
                CorruptionFixture::ReservedWithFailure,
                "has bind failure evidence",
            ),
            (
                CorruptionFixture::FailureEndpointMismatch,
                "bind failure incompatible",
            ),
            (
                CorruptionFixture::RangeFailureSelectedPortMismatch,
                "does not match its selected port",
            ),
            (
                CorruptionFixture::ProviderAssignedFailureWithReservedPort,
                "does not match its selected port",
            ),
            (CorruptionFixture::DuplicateLivePort, "both fence"),
        ] {
            let root = tempfile::tempdir().expect("state root should exist");
            write_corrupt_state(root.path(), corruption);

            let error = LocalPortLeaseAuthority::open(root.path())
                .expect_err("semantic corruption must fail closed during authority startup");
            assert!(
                matches!(
                    &error,
                    PortLeaseError::CorruptAuthority { reason }
                        if reason.contains(expected_reason)
                ),
                "{corruption:?} produced unexpected error: {error}"
            );
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum CorruptionFixture {
        MapKeyMismatch,
        ActiveWithoutBinding,
        BindingPortMismatch,
        ExactWithoutReservedPort,
        ExactWrongReservedPort,
        FailedWithBinding,
        FailedWithoutFailure,
        ReservedWithFailure,
        FailureEndpointMismatch,
        RangeFailureSelectedPortMismatch,
        ProviderAssignedFailureWithReservedPort,
        DuplicateLivePort,
    }

    fn write_corrupt_state(state_root: &Path, corruption: CorruptionFixture) {
        let store = LocalNetworkStateStore::open(state_root).expect("raw store should open");
        store
            .transaction(
                &NetworkStatePartition::PortLeases,
                |state: &mut PortLeaseState| -> Result<(), Infallible> {
                    let first_request = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 1, 1, PORT);
                    let mut first = PortLeaseRecord {
                        request: first_request.clone(),
                        reserved_port: Some(
                            NonZeroU16::new(PORT).expect("fixture port should be non-zero"),
                        ),
                        phase: PortLeasePhase::Reserved,
                        binding: None,
                        failure: None,
                    };

                    match corruption {
                        CorruptionFixture::MapKeyMismatch => {
                            let wrong_key = request("01ARZ3NDEKTSV4RRFFQ69G5FAW", 1, 1, PORT)
                                .lease_id()
                                .clone();
                            state.leases.insert(wrong_key, first);
                        }
                        CorruptionFixture::ActiveWithoutBinding => {
                            first.phase = PortLeasePhase::Active;
                            state.leases.insert(first_request.lease_id().clone(), first);
                        }
                        CorruptionFixture::BindingPortMismatch => {
                            first.phase = PortLeasePhase::Binding;
                            first.binding = Some(binding(PORT + 1, "wrong-port"));
                            state.leases.insert(first_request.lease_id().clone(), first);
                        }
                        CorruptionFixture::ExactWithoutReservedPort => {
                            first.reserved_port = None;
                            state.leases.insert(first_request.lease_id().clone(), first);
                        }
                        CorruptionFixture::ExactWrongReservedPort => {
                            first.reserved_port = NonZeroU16::new(PORT + 1);
                            state.leases.insert(first_request.lease_id().clone(), first);
                        }
                        CorruptionFixture::FailedWithBinding => {
                            first.phase = PortLeasePhase::Failed;
                            first.binding = Some(binding(PORT, "unexpected-provider-effect"));
                            state.leases.insert(first_request.lease_id().clone(), first);
                        }
                        CorruptionFixture::FailedWithoutFailure => {
                            first.phase = PortLeasePhase::Failed;
                            state.leases.insert(first_request.lease_id().clone(), first);
                        }
                        CorruptionFixture::ReservedWithFailure => {
                            first.failure = Some(bind_failure(PORT, "unexpected-failure"));
                            state.leases.insert(first_request.lease_id().clone(), first);
                        }
                        CorruptionFixture::FailureEndpointMismatch => {
                            first.phase = PortLeasePhase::Failed;
                            first.failure = Some(bind_failure(PORT + 1, "wrong-endpoint"));
                            state.leases.insert(first_request.lease_id().clone(), first);
                        }
                        CorruptionFixture::RangeFailureSelectedPortMismatch => {
                            first.request.binding = PortBindingSpec::new(
                                PortProtocol::Tcp,
                                PortBindRealm::Host,
                                PortBindTarget::ipv4_wildcard(),
                                PortExposure::Unknown,
                                PortRequestMode::Range(
                                    PortRange::new(
                                        NonZeroU16::new(PORT)
                                            .expect("fixture port should be non-zero"),
                                        NonZeroU16::new(PORT + 1)
                                            .expect("fixture port should be non-zero"),
                                    )
                                    .expect("fixture range should validate"),
                                ),
                            );
                            first.phase = PortLeasePhase::Failed;
                            first.failure = Some(bind_failure(PORT + 1, "different-selected-port"));
                            state.leases.insert(first_request.lease_id().clone(), first);
                        }
                        CorruptionFixture::ProviderAssignedFailureWithReservedPort => {
                            first.request.binding = PortBindingSpec::new(
                                PortProtocol::Tcp,
                                PortBindRealm::Host,
                                PortBindTarget::ipv4_wildcard(),
                                PortExposure::Unknown,
                                PortRequestMode::ProviderAssigned,
                            );
                            first.phase = PortLeasePhase::Failed;
                            first.failure =
                                Some(provider_assigned_bind_failure("provider-assigned-attempt"));
                            state.leases.insert(first_request.lease_id().clone(), first);
                        }
                        CorruptionFixture::DuplicateLivePort => {
                            let second_request = request("01ARZ3NDEKTSV4RRFFQ69G5FAW", 1, 1, PORT);
                            let second = PortLeaseRecord {
                                request: second_request.clone(),
                                reserved_port: Some(
                                    NonZeroU16::new(PORT).expect("fixture port should be non-zero"),
                                ),
                                phase: PortLeasePhase::Reserved,
                                binding: None,
                                failure: None,
                            };
                            state.leases.insert(first_request.lease_id().clone(), first);
                            state
                                .leases
                                .insert(second_request.lease_id().clone(), second);
                        }
                    }
                    Ok(())
                },
            )
            .expect("checksum-valid corrupt state should be written");
    }

    fn request(payload: &str, generation: u64, epoch: u64, port: u16) -> PortLeaseRequest {
        request_with_owner(payload, payload, generation, epoch, port)
    }

    fn request_with_owner(
        lease_payload: &str,
        owner_payload: &str,
        generation: u64,
        epoch: u64,
        port: u16,
    ) -> PortLeaseRequest {
        let lease_id = format!("netportlease_{lease_payload}")
            .parse()
            .expect("fixture lease id should parse");
        let owner_id: ListenerId = format!("netlistener_{owner_payload}")
            .parse()
            .expect("fixture listener id should parse");
        PortLeaseRequest::new(
            lease_id,
            owner_id.into(),
            Some(TenantId::new("tenant-a").expect("fixture tenant should parse")),
            NetworkResourceGeneration::new(generation),
            NetworkLeaseEpoch::new(epoch),
            PortBindingSpec::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                PortBindTarget::ipv4_wildcard(),
                PortExposure::Unknown,
                PortRequestMode::Exact(NonZeroU16::new(port).expect("fixture port is non-zero")),
            ),
        )
    }

    fn binding(port: u16, opaque: &str) -> PortLeaseBinding {
        let provider_id: NetworkProviderId = "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
            .parse()
            .expect("fixture provider id should parse");
        PortLeaseBinding::new(
            PortBoundEndpoint::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                PortBindTarget::ipv4_wildcard(),
                NonZeroU16::new(port).expect("fixture port is non-zero"),
            )
            .expect("fixture endpoint should validate"),
            PortBindingProvenance::NimbusOwned,
            NetworkProviderHandle::new(provider_id, opaque)
                .expect("fixture provider handle should validate"),
        )
    }

    fn bind_failure(port: u16, opaque: &str) -> PortBindFailure {
        let provider_id: NetworkProviderId = "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
            .parse()
            .expect("fixture provider id should parse");
        PortBindFailure::new(
            PortBindFailureKind::AddrInUse,
            PortBindAttempt::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                PortBindTarget::ipv4_wildcard(),
                port,
            )
            .expect("fixture attempt should validate"),
            NetworkProviderHandle::new(provider_id, opaque)
                .expect("fixture provider attempt should validate"),
        )
    }

    fn provider_assigned_bind_failure(opaque: &str) -> PortBindFailure {
        let provider_id: NetworkProviderId = "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
            .parse()
            .expect("fixture provider id should parse");
        PortBindFailure::new(
            PortBindFailureKind::ResourceExhausted,
            PortBindAttempt::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                PortBindTarget::ipv4_wildcard(),
                0,
            )
            .expect("fixture attempt should validate"),
            NetworkProviderHandle::new(provider_id, opaque)
                .expect("fixture provider attempt should validate"),
        )
    }
}
