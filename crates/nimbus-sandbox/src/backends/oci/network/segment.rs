//! Per-tenant network segment allocation.
//!
//! Ends the M1 collision: instead of every tenant drawing from one constant
//! subnet on one shared bridge, each tenant is assigned a distinct portable
//! allocation carved from the node's super-net. [`nimbus_network::AllocatedSegment`]
//! owns its global identity, tenant attribution, CIDR, and lease epoch;
//! [`super::OciSegmentRealization`] composes the host-local provider names around
//! it. The allocator is injected HostBridge-style so the single-node
//! implementation here and the future cluster leader — which hands each node a
//! raft-committed, epoch-fenced super-net — satisfy the SAME trait; the sandbox
//! backend consumes it unchanged.
//!
//! Payload state lives in the single checksummed/versioned
//! [`nimbus_network::LocalNetworkStateStore`] authority above `tenant_root`.
//! Assign is fail-closed until a super-net is installed, so the cluster leg
//! cannot start a workload before its committed lease arrives, and a
//! single-node node installs the node-0 slice at startup so the code path is
//! identical.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use nimbus_core::TenantId;
use nimbus_core::net::Cidr;
use nimbus_network::{
    AllocatedSegment, LocalNetworkStateStore, NetworkAttachmentId, NetworkLeaseEpoch,
    NetworkReservationClaim, NetworkSegmentAllocator, NetworkSegmentCleanup,
    NetworkSegmentFinalizeOutcome, NetworkSegmentGrowth, NetworkSegmentId,
    NetworkSegmentQuarantineOutcome, NetworkSegmentReleaseOutcome, NetworkStatePartition,
    NetworkStateTransactionError,
};

use crate::error::{Result, SandboxError};

use super::OciSegmentRealization;

mod cleanup;
mod reservation;

pub(crate) use cleanup::DurableSegmentCleanupAuthority;

/// The default single-node super-net: the node-0 `/16` slice of the cluster pool
/// (`10.0.0.0/8`), so enrolling into a cluster later never re-carves live tenants.
/// The backend configs default `node_network_supernet` to this same value.
#[cfg(test)]
pub(crate) const DEFAULT_NODE_SUPERNET: &str = "10.0.0.0/16";
/// The default per-tenant subnet prefix (`/24` = 253 sandboxes). On-demand block
/// growth for denser packing is MTN6.
pub(crate) const DEFAULT_TENANT_PREFIX: u8 = 24;

/// The node super-net this allocator may carve tenant subnets from. Single-node
/// installs the node-0 slice at epoch 0; the cluster leg installs a raft-committed
/// epoch-fenced lease behind the same type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InstalledSuperNet {
    pub(crate) cidr: Cidr,
    pub(crate) epoch: NetworkLeaseEpoch,
}

/// The maximum number of `/24` blocks (bridges) a single tenant may hold. Each
/// block is 253 sandboxes, so 64 blocks is ~16k sandboxes/tenant — generous —
/// while bounding a runaway tenant's consumption of the node super-net.
const MAX_BLOCKS_PER_TENANT: usize = 64;

/// Durable allocation record for one portable segment plus its host-local
/// provider slot.
///
/// `segment_id` is the global identity. `local_slot` is deliberately separate:
/// it may be reused only after cleanup and exists solely to derive host-local
/// provider names in the sandbox adapter.
#[derive(Clone, Serialize, Deserialize)]
struct SegmentBlock {
    local_slot: u32,
    segment_id: NetworkSegmentId,
}

/// A tenant's allocation: its ordered list of blocks (element 0 is the
/// primary/anchor allocation, with additional blocks appended through
/// compare-and-swap-fenced growth) plus one explicit lifecycle state per
/// attachment across all its blocks. The whole allocation becomes
/// cleanup-pending after the last hold releases and is freed only by an
/// identity-fenced finalization.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
enum SegmentAttachmentState {
    /// Attempt-scoped compensation authority before placement selects a block.
    UnplacedReserved {
        reservation_claim: NetworkReservationClaim,
    },
    /// Attempt-scoped compensation authority bound to the exact IPAM-selected
    /// segment before provider effects.
    Reserved {
        reservation_claim: NetworkReservationClaim,
        segment_id: NetworkSegmentId,
    },
    /// Exact never-realized compensation has started, but adapter-owned IPAM
    /// cleanup has not yet been confirmed.
    ///
    /// The claim remains durable so a cleanup failure or process restart can
    /// retry without letting a foreign coordinator remove the attachment.
    ReservationCleanupPending {
        reservation_claim: NetworkReservationClaim,
        segment_id: Option<NetworkSegmentId>,
    },
    /// Ordinary lifecycle authority after control-plane hold adoption.
    ///
    /// The receipt permits exact acknowledgement-loss replay. It is not
    /// compensation authority and does not assert that provider effects exist.
    Held {
        adoption_receipt: Option<NetworkReservationClaim>,
        segment_id: NetworkSegmentId,
    },
    /// Provider detach is required or ambiguous; the allocation remains fenced.
    CleanupPending {
        adoption_receipt: Option<NetworkReservationClaim>,
        segment_id: NetworkSegmentId,
    },
}

impl SegmentAttachmentState {
    fn is_cleanup_pending(&self) -> bool {
        matches!(self, Self::CleanupPending { .. })
    }
}

fn require_adoption_receipt(
    attachment_id: &NetworkAttachmentId,
    stored: &Option<NetworkReservationClaim>,
    expected: Option<&NetworkReservationClaim>,
) -> Result<()> {
    if stored.as_ref() == expected {
        Ok(())
    } else {
        Err(SandboxError::OperationFailed {
            message: format!(
                "network attachment {} rejected stale cleanup because its adoption receipt does \
                 not match the current generation",
                attachment_id.as_str()
            ),
        })
    }
}

#[derive(Default, Serialize, Deserialize)]
struct TenantEntry {
    blocks: Vec<SegmentBlock>,
    attachments: BTreeMap<String, SegmentAttachmentState>,
    /// Fences new attachment/growth authority while every remaining hold is
    /// pending, and remains set after the last hold releases until provider
    /// bridge cleanup is identity-fenced and finalized.
    allocation_cleanup_pending: bool,
    /// Exact launch authority that completed a never-realized attachment
    /// compensation and is awaiting identity-fenced allocation finalization.
    ///
    /// This survives acknowledgement loss between returning the cleanup token
    /// and finalizing it. It is legal only after the claimed attachment was
    /// removed as the allocation's last attachment.
    pending_reservation_cleanup_claim: Option<NetworkReservationClaim>,
}

impl TenantEntry {
    /// The tenant's primary/anchor block (element 0). A persisted entry is
    /// never empty once assigned.
    fn primary(&self) -> &SegmentBlock {
        self.blocks
            .first()
            .expect("a persisted tenant entry always has at least its primary block")
    }
}

#[derive(Default, Serialize, Deserialize)]
struct SegmentState {
    /// The super-net (and its fencing epoch) these assignments were carved under.
    /// A mismatch on load is fail-closed: the node must drain + re-carve, never
    /// silently reuse a stale-epoch block (the cluster reclamation-safety hook).
    supernet_cidr: Option<String>,
    supernet_epoch: Option<NetworkLeaseEpoch>,
    /// tenant id → its allocation (index + live-attachment refcount).
    tenants: BTreeMap<String, TenantEntry>,
}

fn validate_segment_state(state: &SegmentState) -> Result<()> {
    let mut local_slots = BTreeSet::new();
    let mut segment_ids = BTreeSet::new();
    for (tenant, entry) in &state.tenants {
        TenantId::new(tenant).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "network segment state contains invalid tenant id {tenant:?}: {error}"
            ),
        })?;
        if entry.blocks.is_empty() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "network segment state contains an empty allocation for tenant {tenant}"
                ),
            });
        }
        for block in &entry.blocks {
            if !local_slots.insert(block.local_slot) {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "network segment state reuses provider-local slot {}",
                        block.local_slot
                    ),
                });
            }
            if !segment_ids.insert(block.segment_id.clone()) {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "network segment state duplicates stable segment id {}",
                        block.segment_id
                    ),
                });
            }
        }
        for (attachment, attachment_state) in &entry.attachments {
            attachment
                .parse::<NetworkAttachmentId>()
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "network segment state contains invalid attachment id {attachment:?}: {error}"
                    ),
                })?;
            let selected_segment = match attachment_state {
                SegmentAttachmentState::UnplacedReserved { .. } => None,
                SegmentAttachmentState::Reserved { segment_id, .. }
                | SegmentAttachmentState::Held { segment_id, .. }
                | SegmentAttachmentState::CleanupPending { segment_id, .. } => Some(segment_id),
                SegmentAttachmentState::ReservationCleanupPending { segment_id, .. } => {
                    segment_id.as_ref()
                }
            };
            if let Some(selected_segment) = selected_segment
                && !entry
                    .blocks
                    .iter()
                    .any(|block| &block.segment_id == selected_segment)
            {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "network attachment {attachment} references unknown segment {selected_segment}"
                    ),
                });
            }
        }
        let every_attachment_cleanup_pending = !entry.attachments.is_empty()
            && entry
                .attachments
                .values()
                .all(SegmentAttachmentState::is_cleanup_pending);
        if entry.allocation_cleanup_pending
            && entry
                .attachments
                .values()
                .any(|state| !state.is_cleanup_pending())
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "network segment allocation for tenant {tenant} is cleanup-pending while a live or reserved attachment remains"
                ),
            });
        }
        if every_attachment_cleanup_pending && !entry.allocation_cleanup_pending {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "network segment allocation for tenant {tenant} failed to fence an all-cleanup-pending attachment set"
                ),
            });
        }
        if entry.pending_reservation_cleanup_claim.is_some()
            && (!entry.allocation_cleanup_pending || !entry.attachments.is_empty())
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "network segment allocation for tenant {tenant} retains reservation cleanup authority outside an empty cleanup-pending allocation"
                ),
            });
        }
    }
    Ok(())
}

/// Single-node allocator: carves per-tenant subnets from a locally-installed
/// super-net, persisting assignments in the node's one network authority.
pub(crate) struct SingleNodeSegmentAllocator {
    store: LocalNetworkStateStore,
    supernet: Option<InstalledSuperNet>,
    tenant_prefix: u8,
}

/// Deferred single-node adapter used by backend composition roots.
///
/// Backends retain one injected trait object, while each operation opens the
/// shared durable authority and propagates any fail-closed store error. This
/// preserves the existing constructor contract without leaking the concrete
/// allocator or silently accepting an unusable state root.
pub(crate) struct ConfiguredSegmentAllocator {
    state_root: PathBuf,
    supernet: String,
    tenant_prefix: u8,
}

impl ConfiguredSegmentAllocator {
    pub(crate) fn new(state_root: PathBuf, supernet: String, tenant_prefix: u8) -> Self {
        Self {
            state_root,
            supernet,
            tenant_prefix,
        }
    }

    fn inner(&self) -> Result<SingleNodeSegmentAllocator> {
        SingleNodeSegmentAllocator::for_node_supernet(
            &self.state_root,
            &self.supernet,
            self.tenant_prefix,
        )
    }
}

impl SingleNodeSegmentAllocator {
    pub(crate) fn new(
        state_root: &Path,
        supernet: Option<InstalledSuperNet>,
        tenant_prefix: u8,
    ) -> Result<Self> {
        let store = LocalNetworkStateStore::open(state_root).map_err(network_store_error)?;
        Ok(Self {
            store,
            supernet,
            tenant_prefix,
        })
    }

    /// Test-only convenience for a temporary node-0 `/16`, `/24` per tenant.
    ///
    /// Production construction uses [`Self::for_node_supernet`] and propagates
    /// state-root validation errors. Tests intentionally panic if their fresh
    /// temporary local filesystem cannot satisfy that prerequisite.
    #[cfg(test)]
    pub(crate) fn single_node_default(state_root: &Path) -> Self {
        let supernet = InstalledSuperNet {
            cidr: Cidr::parse(DEFAULT_NODE_SUPERNET)
                .expect("the default node super-net constant must be a valid CIDR"),
            epoch: NetworkLeaseEpoch::new(0),
        };
        Self::new(state_root, Some(supernet), DEFAULT_TENANT_PREFIX)
            .expect("temporary local state root should support the network store contract")
    }

    /// Build a single-node allocator carving `/24` tenant subnets from the given
    /// node super-net (the configurable knob a backend passes from its config).
    pub(crate) fn for_node_supernet(
        state_root: &Path,
        supernet: &str,
        tenant_prefix: u8,
    ) -> Result<Self> {
        let cidr = Cidr::parse(supernet).map_err(|error| SandboxError::InvalidSpec {
            message: format!("invalid node network super-net {supernet:?}: {error}"),
        })?;
        Self::new(
            state_root,
            Some(InstalledSuperNet {
                cidr,
                epoch: NetworkLeaseEpoch::new(0),
            }),
            tenant_prefix,
        )
    }

    /// Install (or replace) the node super-net. The cluster leg calls this at
    /// membership-commit with its raft-committed, epoch-fenced lease.
    // Wired by the cluster ClusterSegmentAllocator in MTN7; test-only until then.
    #[allow(dead_code)]
    pub(crate) fn install_supernet(&mut self, supernet: InstalledSuperNet) {
        self.supernet = Some(supernet);
    }

    #[cfg(test)]
    fn state_path(&self) -> &Path {
        self.store.authority_path()
    }

    fn installed(&self) -> Result<&InstalledSuperNet> {
        self.supernet
            .as_ref()
            .ok_or_else(|| SandboxError::OperationFailed {
                message:
                    "network segment pool is unassigned: no super-net is installed on this node \
                     (the cluster lease has not arrived); refusing to assign a tenant segment"
                        .to_owned(),
            })
    }

    /// Resolve a local slot to its assigned CIDR, or fail closed if it overflows
    /// the installed pool.
    fn subnet_at(&self, supernet: &InstalledSuperNet, local_slot: u32) -> Result<Cidr> {
        supernet
            .cidr
            .nth_subnet(self.tenant_prefix, u64::from(local_slot))
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "network segment pool exhausted: node super-net {} cannot carve a /{} at local slot {local_slot}; raise the node super-net or the per-tenant prefix",
                    supernet.cidr, self.tenant_prefix
                ),
            })
    }

    /// Compose one durable portable allocation with its sandbox-owned provider
    /// realization.
    fn segment_at(
        &self,
        supernet: &InstalledSuperNet,
        tenant: &TenantId,
        block: &SegmentBlock,
    ) -> Result<OciSegmentRealization> {
        let allocation = AllocatedSegment::new(
            block.segment_id.clone(),
            tenant.clone(),
            self.subnet_at(supernet, block.local_slot)?,
            supernet.epoch,
        );
        Ok(OciSegmentRealization::from_local_slot(
            allocation,
            block.local_slot,
        ))
    }

    fn cleanup_for(
        &self,
        supernet: &InstalledSuperNet,
        tenant: &TenantId,
        entry: &TenantEntry,
    ) -> Result<NetworkSegmentCleanup<OciSegmentRealization>> {
        let mut segments = Vec::with_capacity(entry.blocks.len());
        let mut segment_ids = Vec::with_capacity(entry.blocks.len());
        for block in &entry.blocks {
            segment_ids.push(block.segment_id.clone());
            segments.push(self.segment_at(supernet, tenant, block)?);
        }
        Ok(NetworkSegmentCleanup::new(
            tenant.clone(),
            segment_ids,
            supernet.epoch,
            segments,
        ))
    }

    /// Fail closed if the persisted state was carved under a different super-net
    /// or epoch than the one this allocator has installed.
    fn ensure_supernet_matches(
        &self,
        supernet: &InstalledSuperNet,
        state: &SegmentState,
    ) -> Result<()> {
        if let Some(cidr) = state.supernet_cidr.as_deref()
            && cidr != supernet.cidr.to_string()
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "network segment state was carved under super-net {cidr}, not the installed {}; drain and re-carve before reuse",
                    supernet.cidr
                ),
            });
        }
        if let Some(epoch) = state.supernet_epoch
            && epoch != supernet.epoch
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "network segment state was carved under super-net epoch {}, not the installed {}; a stale-epoch block must not be reused",
                    epoch.as_u64(),
                    supernet.epoch.as_u64()
                ),
            });
        }
        Ok(())
    }

    fn with_state<T>(&self, mutator: impl FnOnce(&mut SegmentState) -> Result<T>) -> Result<T> {
        match self
            .store
            .transaction(&NetworkStatePartition::SegmentAllocations, |state| {
                validate_segment_state(state)?;
                let result = mutator(state)?;
                validate_segment_state(state)?;
                Ok(result)
            }) {
            Ok(result) => Ok(result),
            Err(NetworkStateTransactionError::Operation(error)) => Err(error),
            Err(NetworkStateTransactionError::Store(error)) => Err(network_store_error(error)),
        }
    }

    fn read_state(&self) -> Result<Option<SegmentState>> {
        let state = self
            .store
            .read(&NetworkStatePartition::SegmentAllocations)
            .map_err(network_store_error)?;
        if let Some(state) = state.as_ref() {
            validate_segment_state(state)?;
        }
        Ok(state)
    }

    #[cfg(test)]
    pub(super) fn has_hold(&self, tenant: &str, sandbox: &str) -> bool {
        let attachment =
            NetworkAttachmentId::for_workload_attachment(sandbox, super::DEFAULT_ATTACHMENT_NAME);
        self.store
            .read::<SegmentState>(&NetworkStatePartition::SegmentAllocations)
            .expect("segment authority should read")
            .is_some_and(|state| {
                state
                    .tenants
                    .get(tenant)
                    .is_some_and(|entry| entry.attachments.contains_key(attachment.as_str()))
            })
    }

    #[cfg(test)]
    pub(super) fn has_pending_hold(&self, tenant: &str, sandbox: &str) -> bool {
        let attachment =
            NetworkAttachmentId::for_workload_attachment(sandbox, super::DEFAULT_ATTACHMENT_NAME);
        self.store
            .read::<SegmentState>(&NetworkStatePartition::SegmentAllocations)
            .expect("segment authority should read")
            .is_some_and(|state| {
                state.tenants.get(tenant).is_some_and(|entry| {
                    entry
                        .attachments
                        .get(attachment.as_str())
                        .is_some_and(SegmentAttachmentState::is_cleanup_pending)
                })
            })
    }
}

fn network_store_error(error: impl std::fmt::Display) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!("network segment authority failed: {error}"),
    }
}

impl SingleNodeSegmentAllocator {
    /// The lowest index held by NO tenant's block — the M1 collision guard: it
    /// unions every tenant's whole block list, so a grown tenant's extra blocks
    /// are never re-handed to another tenant. Fail-closed if the index overflows
    /// the node super-net.
    fn next_free_index(&self, supernet: &InstalledSuperNet, state: &SegmentState) -> Result<u32> {
        let used: BTreeSet<u32> = state
            .tenants
            .values()
            .flat_map(|entry| entry.blocks.iter().map(|block| block.local_slot))
            .collect();
        let index = (0u32..)
            .find(|candidate| !used.contains(candidate))
            .expect("the u32 index space cannot be exhausted by a live tenant set");
        // Fail closed BEFORE the caller commits if this index overflows the pool.
        self.subnet_at(supernet, index)?;
        Ok(index)
    }

    /// Get or assign the tenant's primary block under a held state lock,
    /// fail-closed on a stale super-net or pool exhaustion.
    fn assign_block(
        &self,
        supernet: &InstalledSuperNet,
        state: &mut SegmentState,
        tenant: &TenantId,
    ) -> Result<SegmentBlock> {
        self.ensure_supernet_matches(supernet, state)?;
        if let Some(entry) = state.tenants.get(tenant.as_str()) {
            if entry.allocation_cleanup_pending {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "network segment allocation for tenant {} is cleanup-pending; refusing reuse until provider deletion is confirmed",
                        tenant.as_str()
                    ),
                });
            }
            return Ok(entry.primary().clone());
        }
        let block = SegmentBlock {
            local_slot: self.next_free_index(supernet, state)?,
            segment_id: NetworkSegmentId::generate(),
        };
        state.tenants.insert(
            tenant.as_str().to_owned(),
            TenantEntry {
                blocks: vec![block.clone()],
                attachments: BTreeMap::new(),
                allocation_cleanup_pending: false,
                pending_reservation_cleanup_claim: None,
            },
        );
        state.supernet_cidr = Some(supernet.cidr.to_string());
        state.supernet_epoch = Some(supernet.epoch);
        Ok(block)
    }
}

impl NetworkSegmentAllocator for SingleNodeSegmentAllocator {
    type Segment = OciSegmentRealization;
    type Error = SandboxError;

    fn segment_for(&self, tenant: &TenantId) -> Result<OciSegmentRealization> {
        self.segments_for(tenant)?
            .into_iter()
            .next()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "network segment authority returned no primary block for tenant {}",
                    tenant.as_str()
                ),
            })
    }

    fn segments_for(&self, tenant: &TenantId) -> Result<Vec<OciSegmentRealization>> {
        let supernet = self.installed()?.clone();
        let blocks = self.with_state(|state| {
            self.assign_block(&supernet, state, tenant)?;
            Ok(state
                .tenants
                .get(tenant.as_str())
                .expect("assign_block inserts the tenant entry")
                .blocks
                .clone())
        })?;
        blocks
            .iter()
            .map(|block| self.segment_at(&supernet, tenant, block))
            .collect()
    }

    fn inspect_segments(&self, tenant: &TenantId) -> Result<Option<Vec<OciSegmentRealization>>> {
        let supernet = self.installed()?.clone();
        let Some(state) = self.read_state()? else {
            return Ok(None);
        };
        self.ensure_supernet_matches(&supernet, &state)?;
        let Some(entry) = state.tenants.get(tenant.as_str()) else {
            return Ok(None);
        };
        if entry.blocks.is_empty() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "network segment authority contains an empty allocation for tenant {}",
                    tenant.as_str()
                ),
            });
        }
        entry
            .blocks
            .iter()
            .map(|block| self.segment_at(&supernet, tenant, block))
            .collect::<Result<Vec<_>>>()
            .map(Some)
    }

    fn reserve_attachment_for_coordinator(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<()> {
        self.reserve_attachment_for_coordinator_inner(tenant, attachment_id, reservation_claim)
    }

    fn bind_reserved_attachment_to_segment(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        segment_id: &NetworkSegmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<OciSegmentRealization> {
        self.bind_reserved_attachment_to_segment_inner(
            tenant,
            attachment_id,
            segment_id,
            reservation_claim,
        )
    }

    fn adopt_reserved_attachment(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<OciSegmentRealization> {
        self.adopt_reserved_attachment_inner(tenant, attachment_id, reservation_claim)
    }

    fn release_reserved_attachment_without_effect(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<NetworkSegmentReleaseOutcome<OciSegmentRealization>> {
        self.release_reserved_attachment_without_effect_inner(
            tenant,
            attachment_id,
            reservation_claim,
        )
    }

    fn finalize_reserved_attachment_without_effect(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<NetworkSegmentReleaseOutcome<OciSegmentRealization>> {
        self.finalize_reserved_attachment_without_effect_inner(
            tenant,
            attachment_id,
            reservation_claim,
        )
    }

    fn acquire(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
    ) -> Result<OciSegmentRealization> {
        let supernet = self.installed()?.clone();
        let block = self.with_state(|state| {
            let block = self.assign_block(&supernet, state, tenant)?;
            let entry = state
                .tenants
                .get_mut(tenant.as_str())
                .expect("assign_block inserts the tenant entry");
            match entry.attachments.get(attachment_id.as_str()) {
                Some(
                    SegmentAttachmentState::UnplacedReserved { .. }
                    | SegmentAttachmentState::Reserved { .. },
                ) => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} has an unadopted launch reservation; direct acquire is forbidden",
                            attachment_id.as_str()
                        ),
                    });
                }
                Some(SegmentAttachmentState::ReservationCleanupPending { .. }) => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} has exact reservation cleanup pending; refusing reacquire until IPAM deletion is confirmed",
                            attachment_id.as_str()
                        ),
                    });
                }
                Some(SegmentAttachmentState::CleanupPending { .. }) => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} is cleanup-pending; refusing reacquire until provider deletion is confirmed",
                            attachment_id.as_str()
                        ),
                    });
                }
                Some(SegmentAttachmentState::Held { segment_id, .. }) => {
                    return entry
                        .blocks
                        .iter()
                        .find(|held_block| held_block.segment_id == *segment_id)
                        .cloned()
                        .ok_or_else(|| SandboxError::OperationFailed {
                            message: format!(
                                "network attachment {} references missing selected segment {segment_id}",
                                attachment_id.as_str()
                            ),
                        });
                }
                None => {}
            }
            let segment_id = block.segment_id.clone();
            entry.attachments.insert(
                attachment_id.as_str().to_owned(),
                SegmentAttachmentState::Held {
                    adoption_receipt: None,
                    segment_id,
                },
            );
            Ok(block)
        })?;
        self.segment_at(&supernet, tenant, &block)
    }

    fn quarantine(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        expected_adoption_receipt: Option<&NetworkReservationClaim>,
    ) -> Result<NetworkSegmentQuarantineOutcome> {
        let supernet = self.installed()?.clone();
        self.with_state(|state| {
            self.ensure_supernet_matches(&supernet, state)?;
            let Some(entry) = state.tenants.get_mut(tenant.as_str()) else {
                return Ok(NetworkSegmentQuarantineOutcome::AlreadyReleased);
            };
            if entry.attachments.is_empty() && entry.allocation_cleanup_pending {
                return Ok(NetworkSegmentQuarantineOutcome::CleanupPending);
            }
            match entry.attachments.get(attachment_id.as_str()).cloned() {
                Some(
                    SegmentAttachmentState::UnplacedReserved { .. }
                    | SegmentAttachmentState::Reserved { .. },
                ) => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} has an unadopted launch reservation; generic quarantine is forbidden",
                            attachment_id.as_str()
                        ),
                    });
                }
                Some(SegmentAttachmentState::ReservationCleanupPending { .. }) => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} awaits exact reservation IPAM cleanup; generic quarantine is forbidden",
                            attachment_id.as_str()
                        ),
                    });
                }
                Some(SegmentAttachmentState::Held {
                    adoption_receipt,
                    segment_id,
                }) => {
                    require_adoption_receipt(
                        attachment_id,
                        &adoption_receipt,
                        expected_adoption_receipt,
                    )?;
                    entry.attachments.insert(
                        attachment_id.as_str().to_owned(),
                        SegmentAttachmentState::CleanupPending {
                            adoption_receipt,
                            segment_id,
                        },
                    );
                    if entry
                        .attachments
                        .values()
                        .all(SegmentAttachmentState::is_cleanup_pending)
                    {
                        entry.allocation_cleanup_pending = true;
                    }
                    return Ok(NetworkSegmentQuarantineOutcome::CleanupPending);
                }
                Some(SegmentAttachmentState::CleanupPending {
                    adoption_receipt, ..
                }) => {
                    require_adoption_receipt(
                        attachment_id,
                        &adoption_receipt,
                        expected_adoption_receipt,
                    )?;
                    return Ok(NetworkSegmentQuarantineOutcome::CleanupPending);
                }
                None => {}
            }
            if entry.attachments.is_empty() {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "network segment allocation for tenant {} has no attachment ownership; refusing quarantine for {}",
                        tenant.as_str(),
                        attachment_id.as_str()
                    ),
                });
            }
            Ok(NetworkSegmentQuarantineOutcome::AlreadyReleased)
        })
    }

    fn release(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        expected_adoption_receipt: Option<&NetworkReservationClaim>,
    ) -> Result<NetworkSegmentReleaseOutcome<OciSegmentRealization>> {
        let supernet = self.installed()?.clone();
        self.with_state(|state| {
            self.ensure_supernet_matches(&supernet, state)?;
            let Some(entry) = state.tenants.get_mut(tenant.as_str()) else {
                return Ok(NetworkSegmentReleaseOutcome::AlreadyReleased);
            };
            if entry.attachments.is_empty() && entry.allocation_cleanup_pending {
                if entry.pending_reservation_cleanup_claim.is_some() {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "network segment allocation for tenant {} awaits exact launch-reservation cleanup retry",
                            tenant.as_str()
                        ),
                    });
                }
                return self
                    .cleanup_for(&supernet, tenant, entry)
                    .map(NetworkSegmentReleaseOutcome::CleanupPending);
            }
            match entry.attachments.get(attachment_id.as_str()) {
                Some(
                    SegmentAttachmentState::UnplacedReserved { .. }
                    | SegmentAttachmentState::Reserved { .. },
                ) => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} has an unadopted launch reservation; generic release is forbidden",
                            attachment_id.as_str()
                        ),
                    });
                }
                Some(SegmentAttachmentState::ReservationCleanupPending { .. }) => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} awaits exact reservation IPAM cleanup; generic release is forbidden",
                            attachment_id.as_str()
                        ),
                    });
                }
                Some(SegmentAttachmentState::Held { .. }) => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} must be durably quarantined before release",
                            attachment_id.as_str()
                        ),
                    });
                }
                Some(SegmentAttachmentState::CleanupPending {
                    adoption_receipt, ..
                }) => {
                    require_adoption_receipt(
                        attachment_id,
                        adoption_receipt,
                        expected_adoption_receipt,
                    )?;
                }
                None => return Ok(NetworkSegmentReleaseOutcome::AlreadyReleased),
            }
            entry.attachments.remove(attachment_id.as_str());
            if !entry.attachments.is_empty() {
                entry.allocation_cleanup_pending = entry
                    .attachments
                    .values()
                    .all(SegmentAttachmentState::is_cleanup_pending);
                return Ok(NetworkSegmentReleaseOutcome::StillLive);
            }
            entry.allocation_cleanup_pending = true;
            self.cleanup_for(&supernet, tenant, entry)
                .map(NetworkSegmentReleaseOutcome::CleanupPending)
        })
    }

    fn finalize_release(
        &self,
        cleanup: &NetworkSegmentCleanup<OciSegmentRealization>,
    ) -> Result<NetworkSegmentFinalizeOutcome> {
        let supernet = self.installed()?.clone();
        self.with_state(|state| {
            self.ensure_supernet_matches(&supernet, state)?;
            let Some(entry) = state.tenants.get(cleanup.tenant_id().as_str()) else {
                return Ok(NetworkSegmentFinalizeOutcome::AlreadyReleased);
            };
            if !entry.allocation_cleanup_pending || !entry.attachments.is_empty() {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "network segment allocation for tenant {} is not ready for final release",
                        cleanup.tenant_id().as_str()
                    ),
                });
            }
            if cleanup.lease_epoch() != supernet.epoch
                || entry.blocks.len() != cleanup.segment_ids().len()
                || !entry
                    .blocks
                    .iter()
                    .zip(cleanup.segment_ids())
                    .all(|(block, segment_id)| &block.segment_id == segment_id)
            {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "stale network segment cleanup proof for tenant {}; allocation identity or lease epoch changed",
                        cleanup.tenant_id().as_str()
                    ),
                });
            }
            state.tenants.remove(cleanup.tenant_id().as_str());
            Ok(NetworkSegmentFinalizeOutcome::Released)
        })
    }

    fn grow_block_if_current(
        &self,
        tenant: &TenantId,
        observed_segments: &[OciSegmentRealization],
    ) -> Result<NetworkSegmentGrowth<OciSegmentRealization>> {
        let supernet = self.installed()?.clone();
        let block = self.with_state(|state| {
            self.ensure_supernet_matches(&supernet, state)?;
            let entry = state
                .tenants
                .get(tenant.as_str())
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "cannot grow a block for tenant {} before its primary block is assigned",
                        tenant.as_str()
                    ),
                })?;
            if entry.allocation_cleanup_pending {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "network segment allocation for tenant {} is cleanup-pending; refusing growth until provider deletion is confirmed",
                        tenant.as_str()
                    ),
                });
            }
            let observation_is_current = entry.blocks.len() == observed_segments.len()
                && entry
                    .blocks
                    .iter()
                    .zip(observed_segments)
                    .all(|(block, observed)| &block.segment_id == observed.segment_id());
            if !observation_is_current {
                return Ok(None);
            }
            if entry.blocks.len() >= MAX_BLOCKS_PER_TENANT {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "tenant {} already holds the maximum of {MAX_BLOCKS_PER_TENANT} network blocks; \
                         raise the per-tenant subnet prefix or MAX_BLOCKS_PER_TENANT",
                        tenant.as_str()
                    ),
                });
            }
            // Fail-closed on pool exhaustion happens inside next_free_index.
            let block = SegmentBlock {
                local_slot: self.next_free_index(&supernet, state)?,
                segment_id: NetworkSegmentId::generate(),
            };
            state
                .tenants
                .get_mut(tenant.as_str())
                .expect("tenant entry checked above")
                .blocks
                .push(block.clone());
            Ok(Some(block))
        })?;
        match block {
            Some(block) => self
                .segment_at(&supernet, tenant, &block)
                .map(NetworkSegmentGrowth::Grown),
            None => Ok(NetworkSegmentGrowth::ObservationStale),
        }
    }

    fn reconcile_orphans(
        &self,
        live: &BTreeSet<(TenantId, NetworkAttachmentId)>,
    ) -> Result<Vec<OciSegmentRealization>> {
        let supernet = self.installed()?.clone();
        self.with_state(|state| {
            let live: BTreeSet<(String, String)> = live
                .iter()
                .map(|(tenant, attachment)| {
                    (tenant.as_str().to_owned(), attachment.as_str().to_owned())
                })
                .collect();
            let mut quarantined = Vec::new();
            let tenants: Vec<String> = state.tenants.keys().cloned().collect();
            for tenant in tenants {
                let tenant_id =
                    TenantId::new(&tenant).map_err(|error| SandboxError::OperationFailed {
                        message: format!(
                            "network segment state contains invalid tenant id {tenant:?}: {error}"
                        ),
                    })?;
                let entry = state
                    .tenants
                    .get_mut(&tenant)
                    .expect("tenant key came from the same map");
                for attachment in entry.attachments.keys() {
                    attachment.parse::<NetworkAttachmentId>().map_err(|error| {
                        SandboxError::OperationFailed {
                            message: format!(
                                "network segment state contains invalid attachment id: {error}"
                            ),
                        }
                    })?;
                }
                let orphaned: Vec<String> = entry
                    .attachments
                    .iter()
                    .filter(|(attachment, attachment_state)| {
                        !matches!(
                            attachment_state,
                            SegmentAttachmentState::UnplacedReserved { .. }
                                | SegmentAttachmentState::Reserved { .. }
                                | SegmentAttachmentState::ReservationCleanupPending { .. }
                        ) && !live.contains(&(tenant.clone(), (*attachment).clone()))
                    })
                    .map(|(attachment, _)| attachment.clone())
                    .collect();
                for attachment in orphaned {
                    let attachment_state = entry
                        .attachments
                        .get_mut(&attachment)
                        .expect("orphaned attachment came from the same map");
                    if let SegmentAttachmentState::Held {
                        adoption_receipt,
                        segment_id,
                    } = attachment_state
                    {
                        *attachment_state = SegmentAttachmentState::CleanupPending {
                            adoption_receipt: adoption_receipt.clone(),
                            segment_id: segment_id.clone(),
                        };
                    }
                }
                if entry.attachments.is_empty()
                    || entry
                        .attachments
                        .values()
                        .all(SegmentAttachmentState::is_cleanup_pending)
                {
                    entry.allocation_cleanup_pending = true;
                    quarantined.extend(
                        self.cleanup_for(&supernet, &tenant_id, entry)?
                            .segments()
                            .iter()
                            .cloned(),
                    );
                }
            }
            Ok(quarantined)
        })
    }
}

impl NetworkSegmentAllocator for ConfiguredSegmentAllocator {
    type Segment = OciSegmentRealization;
    type Error = SandboxError;

    fn segment_for(&self, tenant: &TenantId) -> Result<Self::Segment> {
        self.inner()?.segment_for(tenant)
    }

    fn segments_for(&self, tenant: &TenantId) -> Result<Vec<Self::Segment>> {
        self.inner()?.segments_for(tenant)
    }

    fn inspect_segments(&self, tenant: &TenantId) -> Result<Option<Vec<Self::Segment>>> {
        self.inner()?.inspect_segments(tenant)
    }

    fn reserve_attachment_for_coordinator(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<()> {
        self.inner()?
            .reserve_attachment_for_coordinator(tenant, attachment_id, reservation_claim)
    }

    fn bind_reserved_attachment_to_segment(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        segment_id: &NetworkSegmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<Self::Segment> {
        self.inner()?.bind_reserved_attachment_to_segment(
            tenant,
            attachment_id,
            segment_id,
            reservation_claim,
        )
    }

    fn adopt_reserved_attachment(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<Self::Segment> {
        self.inner()?
            .adopt_reserved_attachment(tenant, attachment_id, reservation_claim)
    }

    fn release_reserved_attachment_without_effect(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<NetworkSegmentReleaseOutcome<Self::Segment>> {
        self.inner()?.release_reserved_attachment_without_effect(
            tenant,
            attachment_id,
            reservation_claim,
        )
    }

    fn finalize_reserved_attachment_without_effect(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<NetworkSegmentReleaseOutcome<Self::Segment>> {
        self.inner()?.finalize_reserved_attachment_without_effect(
            tenant,
            attachment_id,
            reservation_claim,
        )
    }

    fn acquire(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
    ) -> Result<Self::Segment> {
        self.inner()?.acquire(tenant, attachment_id)
    }

    fn quarantine(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        expected_adoption_receipt: Option<&NetworkReservationClaim>,
    ) -> Result<NetworkSegmentQuarantineOutcome> {
        self.inner()?
            .quarantine(tenant, attachment_id, expected_adoption_receipt)
    }

    fn release(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        expected_adoption_receipt: Option<&NetworkReservationClaim>,
    ) -> Result<NetworkSegmentReleaseOutcome<Self::Segment>> {
        self.inner()?
            .release(tenant, attachment_id, expected_adoption_receipt)
    }

    fn finalize_release(
        &self,
        cleanup: &NetworkSegmentCleanup<Self::Segment>,
    ) -> Result<NetworkSegmentFinalizeOutcome> {
        self.inner()?.finalize_release(cleanup)
    }

    fn grow_block_if_current(
        &self,
        tenant: &TenantId,
        observed_segments: &[Self::Segment],
    ) -> Result<NetworkSegmentGrowth<Self::Segment>> {
        self.inner()?
            .grow_block_if_current(tenant, observed_segments)
    }

    fn reconcile_orphans(
        &self,
        live: &BTreeSet<(TenantId, NetworkAttachmentId)>,
    ) -> Result<Vec<Self::Segment>> {
        self.inner()?.reconcile_orphans(live)
    }
}

#[cfg(test)]
#[path = "segment/tests.rs"]
mod tests;
