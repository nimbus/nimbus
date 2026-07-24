//! Crash-safe host-port reservation lifecycle.
//!
//! This module owns portable lease identity and durable lifecycle state. It
//! does not bind sockets, probe the host, decide tenant quota, or interpret
//! provider handles. Every operation runs in the one
//! [`LocalNetworkStateStore`] lock and transaction domain, so separately opened
//! handles and separate Nimbus processes cannot publish conflicting authority.

use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::num::NonZeroU16;
use std::path::Path;

use nimbus_core::TenantId;
use serde::{Deserialize, Serialize};

use crate::{
    LocalNetworkStateStore, NetworkLeaseEpoch, NetworkProviderHandle, NetworkResourceGeneration,
    NetworkResourceId, NetworkStatePartition, NetworkStateStoreError, NetworkStateTransactionError,
    PortLeaseId,
};

/// Durable phase of one host-port lease generation.
///
/// `CleanupPending` is included from the start so later provider reconciliation
/// can retain ambiguous unbind authority without changing the durable wire
/// vocabulary. NNC3.1 does not manufacture provider cleanup evidence.
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
/// NNC3.1 deliberately admits only an exact non-zero host port. NNC3.2 extends
/// the binding request with protocol, address family, realm, exposure,
/// exact/range/provider-assigned modes, and conservative overlap rules without
/// changing this lifecycle authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortLeaseRequest {
    lease_id: PortLeaseId,
    owner_id: NetworkResourceId,
    tenant_id: Option<TenantId>,
    generation: NetworkResourceGeneration,
    lease_epoch: NetworkLeaseEpoch,
    requested_port: NonZeroU16,
}

impl PortLeaseRequest {
    /// Construct an exact host-port reservation request.
    pub fn new_exact(
        lease_id: PortLeaseId,
        owner_id: NetworkResourceId,
        tenant_id: Option<TenantId>,
        generation: NetworkResourceGeneration,
        lease_epoch: NetworkLeaseEpoch,
        requested_port: NonZeroU16,
    ) -> Self {
        Self {
            lease_id,
            owner_id,
            tenant_id,
            generation,
            lease_epoch,
            requested_port,
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

    /// Exact non-zero host port requested by this NNC3.1 contract.
    pub const fn requested_port(&self) -> NonZeroU16 {
        self.requested_port
    }
}

/// Concrete provider binding adopted into a reserved lease before activation.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortLeaseBinding {
    actual_port: NonZeroU16,
    provider_handle: NetworkProviderHandle,
}

impl PortLeaseBinding {
    /// Record one exact provider binding.
    pub fn new(actual_port: NonZeroU16, provider_handle: NetworkProviderHandle) -> Self {
        Self {
            actual_port,
            provider_handle,
        }
    }

    /// Actual non-zero host port proven by the provider adapter.
    pub const fn actual_port(&self) -> NonZeroU16 {
        self.actual_port
    }

    /// Opaque provider handle used only by the owning adapter.
    pub fn provider_handle(&self) -> &NetworkProviderHandle {
        &self.provider_handle
    }
}

impl fmt::Debug for PortLeaseBinding {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PortLeaseBinding")
            .field("actual_port", &self.actual_port)
            .field("provider_handle", &self.provider_handle)
            .finish()
    }
}

/// Durable port-lease record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortLeaseRecord {
    request: PortLeaseRequest,
    phase: PortLeasePhase,
    binding: Option<PortLeaseBinding>,
}

impl PortLeaseRecord {
    /// Immutable identity/fence that admitted this record.
    pub fn request(&self) -> &PortLeaseRequest {
        &self.request
    }

    /// Current durable lifecycle phase.
    pub const fn phase(&self) -> PortLeasePhase {
        self.phase
    }

    /// Adopted concrete binding, when one has been recorded.
    pub fn binding(&self) -> Option<&PortLeaseBinding> {
        self.binding.as_ref()
    }
}

#[derive(Default, Serialize, Deserialize)]
struct PortLeaseState {
    leases: BTreeMap<PortLeaseId, PortLeaseRecord>,
}

impl PortLeaseState {
    fn validate(&self) -> Result<(), PortLeaseOperationError> {
        let mut live_ports = BTreeMap::<NonZeroU16, PortLeaseId>::new();

        for (lease_id, record) in &self.leases {
            if lease_id != record.request.lease_id() {
                return Err(PortLeaseOperationError::CorruptAuthority {
                    reason: format!(
                        "map key {lease_id} does not match record identity {}",
                        record.request.lease_id()
                    ),
                });
            }

            match (record.phase, record.binding.as_ref()) {
                (PortLeasePhase::Reserved, Some(_)) => {
                    return Err(PortLeaseOperationError::CorruptAuthority {
                        reason: format!("reserved lease {lease_id} has provider binding evidence"),
                    });
                }
                (PortLeasePhase::Binding | PortLeasePhase::Active, None) => {
                    return Err(PortLeaseOperationError::CorruptAuthority {
                        reason: format!("{:?} lease {lease_id} has no binding", record.phase),
                    });
                }
                (PortLeasePhase::Failed, Some(_)) => {
                    return Err(PortLeaseOperationError::CorruptAuthority {
                        reason: format!(
                            "terminal failed lease {lease_id} retains provider binding evidence"
                        ),
                    });
                }
                _ => {}
            }

            if let Some(binding) = &record.binding
                && binding.actual_port != record.request.requested_port
            {
                return Err(PortLeaseOperationError::CorruptAuthority {
                    reason: format!(
                        "lease {lease_id} requested exact port {} but records binding {}",
                        record.request.requested_port, binding.actual_port
                    ),
                });
            }

            if !record.phase.is_terminal()
                && let Some(existing_lease_id) =
                    live_ports.insert(record.request.requested_port, lease_id.clone())
            {
                return Err(PortLeaseOperationError::CorruptAuthority {
                    reason: format!(
                        "non-terminal leases {existing_lease_id} and {lease_id} both fence port {}",
                        record.request.requested_port
                    ),
                });
            }
        }

        Ok(())
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

    /// Atomically reserve one exact host port.
    ///
    /// Replaying the same immutable request returns its existing record.
    /// Reusing a lease ID with different identity/fence data fails closed.
    /// Every non-terminal record fences the requested port, including
    /// `Withdrawing` and `CleanupPending`.
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

            if let Some(existing) = state.leases.values().find(|record| {
                !record.phase.is_terminal()
                    && record.request.requested_port == request.requested_port
            }) {
                return Err(PortLeaseOperationError::PortConflict {
                    requested_port: request.requested_port,
                    requested_lease_id: request.lease_id.clone(),
                    requested_owner_id: request.owner_id.clone(),
                    existing_lease_id: existing.request.lease_id.clone(),
                    existing_owner_id: existing.request.owner_id.clone(),
                    existing_phase: existing.phase,
                });
            }

            let record = PortLeaseRecord {
                request: request.clone(),
                phase: PortLeasePhase::Reserved,
                binding: None,
            };
            state
                .leases
                .insert(request.lease_id.clone(), record.clone());
            Ok(record)
        })
    }

    /// Durably adopt a concrete provider binding without making it active.
    ///
    /// For the exact-only NNC3.1 contract, the actual port must equal the
    /// requested port. Provider-assigned mode is introduced with its conflict
    /// rules in NNC3.2.
    pub fn adopt(
        &self,
        request: &PortLeaseRequest,
        binding: PortLeaseBinding,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        self.transaction(|state| {
            let record = exact_record_mut(state, request)?;
            if binding.actual_port != request.requested_port {
                return Err(PortLeaseOperationError::BindingPortMismatch {
                    lease_id: request.lease_id.clone(),
                    requested_port: request.requested_port,
                    actual_port: binding.actual_port,
                });
            }
            match record.phase {
                PortLeasePhase::Reserved => {
                    record.phase = PortLeasePhase::Binding;
                    record.binding = Some(binding);
                }
                PortLeasePhase::Binding if record.binding.as_ref() == Some(&binding) => {}
                PortLeasePhase::Binding => {
                    return Err(PortLeaseOperationError::BindingConflict {
                        lease_id: request.lease_id.clone(),
                    });
                }
                phase => {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id.clone(),
                        phase,
                        operation: PortLeaseOperation::Adopt,
                    });
                }
            }
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

fn exact_record_mut<'a>(
    state: &'a mut PortLeaseState,
    request: &PortLeaseRequest,
) -> Result<&'a mut PortLeaseRecord, PortLeaseOperationError> {
    let record = state.leases.get_mut(request.lease_id()).ok_or_else(|| {
        PortLeaseOperationError::NotFound {
            lease_id: request.lease_id.clone(),
        }
    })?;
    if record.request != *request {
        return Err(PortLeaseOperationError::StaleFence(Box::new(
            PortLeaseFenceMismatch {
                expected: record.request.clone(),
                candidate: request.clone(),
            },
        )));
    }
    Ok(record)
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
        requested_port: NonZeroU16,
        requested_lease_id: PortLeaseId,
        requested_owner_id: NetworkResourceId,
        existing_lease_id: PortLeaseId,
        existing_owner_id: NetworkResourceId,
        existing_phase: PortLeasePhase,
    },
    StaleFence(Box<PortLeaseFenceMismatch>),
    BindingPortMismatch {
        lease_id: PortLeaseId,
        requested_port: NonZeroU16,
        actual_port: NonZeroU16,
    },
    BindingConflict {
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
        requested_port: NonZeroU16,
        requested_lease_id: PortLeaseId,
        requested_owner_id: NetworkResourceId,
        existing_lease_id: PortLeaseId,
        existing_owner_id: NetworkResourceId,
        existing_phase: PortLeasePhase,
    },
    /// A generation/epoch/owner request did not match durable authority.
    StaleFence(Box<PortLeaseFenceMismatch>),
    /// Exact reservation and concrete adopted port disagree.
    BindingPortMismatch {
        lease_id: PortLeaseId,
        requested_port: NonZeroU16,
        actual_port: NonZeroU16,
    },
    /// A second adoption supplied different provider evidence.
    BindingConflict { lease_id: PortLeaseId },
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
                requested_port,
                requested_lease_id,
                requested_owner_id,
                existing_lease_id,
                existing_owner_id,
                existing_phase,
            } => Self::PortConflict {
                requested_port,
                requested_lease_id,
                requested_owner_id,
                existing_lease_id,
                existing_owner_id,
                existing_phase,
            },
            PortLeaseOperationError::StaleFence(mismatch) => Self::StaleFence(mismatch),
            PortLeaseOperationError::BindingPortMismatch {
                lease_id,
                requested_port,
                actual_port,
            } => Self::BindingPortMismatch {
                lease_id,
                requested_port,
                actual_port,
            },
            PortLeaseOperationError::BindingConflict { lease_id } => {
                Self::BindingConflict { lease_id }
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
                requested_port,
                requested_lease_id,
                requested_owner_id,
                existing_lease_id,
                existing_owner_id,
                existing_phase,
            } => write!(
                formatter,
                "port {} requested by lease {} owner {:?} conflicts with lease {} owner {:?} in \
                 phase {:?}",
                requested_port,
                requested_lease_id,
                requested_owner_id,
                existing_lease_id,
                existing_owner_id,
                existing_phase
            ),
            Self::StaleFence(mismatch) => write!(
                formatter,
                "port lease {} rejected stale or divergent fence: expected owner {:?} tenant {:?} \
                 port {} generation {} epoch {}, candidate owner {:?} tenant {:?} port {} \
                 generation {} epoch {}",
                mismatch.expected.lease_id,
                mismatch.expected.owner_id,
                mismatch.expected.tenant_id,
                mismatch.expected.requested_port,
                mismatch.expected.generation.as_u64(),
                mismatch.expected.lease_epoch.as_u64(),
                mismatch.candidate.owner_id,
                mismatch.candidate.tenant_id,
                mismatch.candidate.requested_port,
                mismatch.candidate.generation.as_u64(),
                mismatch.candidate.lease_epoch.as_u64()
            ),
            Self::BindingPortMismatch {
                lease_id,
                requested_port,
                actual_port,
            } => write!(
                formatter,
                "port lease {lease_id} requested exact port {requested_port} but provider reported \
                 {actual_port}"
            ),
            Self::BindingConflict { lease_id } => write!(
                formatter,
                "port lease {lease_id} already has different adopted provider evidence"
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
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::sync::{Arc, Barrier};
    use std::thread;

    use crate::{ListenerId, NetworkProviderId};

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
                requested_port,
                existing_phase: PortLeasePhase::Reserved,
                ..
            } if requested_port.get() == PORT
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
            Err(PortLeaseError::BindingPortMismatch { .. })
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
    fn checksum_valid_semantically_corrupt_authority_fails_closed() {
        for (corruption, expected_reason) in [
            (CorruptionFixture::MapKeyMismatch, "does not match"),
            (CorruptionFixture::ActiveWithoutBinding, "has no binding"),
            (
                CorruptionFixture::BindingPortMismatch,
                "requested exact port",
            ),
            (CorruptionFixture::FailedWithBinding, "terminal failed"),
            (CorruptionFixture::DuplicateLivePort, "both fence port"),
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
        FailedWithBinding,
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
                        phase: PortLeasePhase::Reserved,
                        binding: None,
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
                        CorruptionFixture::FailedWithBinding => {
                            first.phase = PortLeasePhase::Failed;
                            first.binding = Some(binding(PORT, "unexpected-provider-effect"));
                            state.leases.insert(first_request.lease_id().clone(), first);
                        }
                        CorruptionFixture::DuplicateLivePort => {
                            let second_request = request("01ARZ3NDEKTSV4RRFFQ69G5FAW", 1, 1, PORT);
                            let second = PortLeaseRecord {
                                request: second_request.clone(),
                                phase: PortLeasePhase::Reserved,
                                binding: None,
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
        PortLeaseRequest::new_exact(
            lease_id,
            owner_id.into(),
            Some(TenantId::new("tenant-a").expect("fixture tenant should parse")),
            NetworkResourceGeneration::new(generation),
            NetworkLeaseEpoch::new(epoch),
            NonZeroU16::new(port).expect("fixture port is non-zero"),
        )
    }

    fn binding(port: u16, opaque: &str) -> PortLeaseBinding {
        let provider_id: NetworkProviderId = "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
            .parse()
            .expect("fixture provider id should parse");
        PortLeaseBinding::new(
            NonZeroU16::new(port).expect("fixture port is non-zero"),
            NetworkProviderHandle::new(provider_id, opaque)
                .expect("fixture provider handle should validate"),
        )
    }
}
