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
    NetworkSegmentAllocator, NetworkSegmentId, NetworkSegmentReleaseOutcome, NetworkStatePartition,
    NetworkStateTransactionError,
};

use crate::error::{Result, SandboxError};

use super::OciSegmentRealization;

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
    pub(crate) epoch: u64,
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
/// primary/anchor allocation; additional blocks are appended on demand by
/// `grow_block`) plus the set of live attachment IDs across all its blocks. The
/// whole allocation is freed only when the last hold releases.
#[derive(Default, Serialize, Deserialize)]
struct TenantEntry {
    blocks: Vec<SegmentBlock>,
    live_attachments: BTreeSet<String>,
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
    supernet_epoch: Option<u64>,
    /// tenant id → its allocation (index + live-attachment refcount).
    tenants: BTreeMap<String, TenantEntry>,
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
            epoch: 0,
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
            Some(InstalledSuperNet { cidr, epoch: 0 }),
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
            NetworkLeaseEpoch::new(supernet.epoch),
        );
        Ok(OciSegmentRealization::from_local_slot(
            allocation,
            block.local_slot,
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
                    "network segment state was carved under super-net epoch {epoch}, not the installed {}; a stale-epoch block must not be reused",
                    supernet.epoch
                ),
            });
        }
        Ok(())
    }

    fn with_state<T>(&self, mutator: impl FnOnce(&mut SegmentState) -> Result<T>) -> Result<T> {
        match self
            .store
            .transaction(&NetworkStatePartition::SegmentAllocations, mutator)
        {
            Ok(result) => Ok(result),
            Err(NetworkStateTransactionError::Operation(error)) => Err(error),
            Err(NetworkStateTransactionError::Store(error)) => Err(network_store_error(error)),
        }
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
                    .is_some_and(|entry| entry.live_attachments.contains(attachment.as_str()))
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
                live_attachments: BTreeSet::new(),
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
        let supernet = self.installed()?.clone();
        let block = self.with_state(|state| self.assign_block(&supernet, state, tenant))?;
        self.segment_at(&supernet, tenant, &block)
    }

    fn acquire(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
    ) -> Result<OciSegmentRealization> {
        let supernet = self.installed()?.clone();
        let block = self.with_state(|state| {
            let block = self.assign_block(&supernet, state, tenant)?;
            state
                .tenants
                .get_mut(tenant.as_str())
                .expect("assign_block inserts the tenant entry")
                .live_attachments
                .insert(attachment_id.as_str().to_owned());
            Ok(block)
        })?;
        self.segment_at(&supernet, tenant, &block)
    }

    fn release(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
    ) -> Result<NetworkSegmentReleaseOutcome<OciSegmentRealization>> {
        let supernet = self.installed()?.clone();
        self.with_state(|state| {
            let Some(entry) = state.tenants.get_mut(tenant.as_str()) else {
                return Ok(NetworkSegmentReleaseOutcome::StillLive);
            };
            entry.live_attachments.remove(attachment_id.as_str());
            if entry.live_attachments.is_empty() {
                let blocks = entry.blocks.clone();
                state.tenants.remove(tenant.as_str());
                // Drain releases EVERY block bridge the tenant grew.
                let mut segments = Vec::with_capacity(blocks.len());
                for block in blocks {
                    segments.push(self.segment_at(&supernet, tenant, &block)?);
                }
                Ok(NetworkSegmentReleaseOutcome::TenantDrained { segments })
            } else {
                Ok(NetworkSegmentReleaseOutcome::StillLive)
            }
        })
    }

    fn grow_block(&self, tenant: &TenantId) -> Result<OciSegmentRealization> {
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
            Ok(block)
        })?;
        self.segment_at(&supernet, tenant, &block)
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
            let mut drained = Vec::new();
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
                for attachment in &entry.live_attachments {
                    attachment.parse::<NetworkAttachmentId>().map_err(|error| {
                        SandboxError::OperationFailed {
                            message: format!(
                                "network segment state contains invalid attachment id: {error}"
                            ),
                        }
                    })?;
                }
                entry
                    .live_attachments
                    .retain(|attachment| live.contains(&(tenant.clone(), attachment.clone())));
                if entry.live_attachments.is_empty() {
                    let blocks = entry.blocks.clone();
                    state.tenants.remove(&tenant);
                    for block in blocks {
                        drained.push(self.segment_at(&supernet, &tenant_id, &block)?);
                    }
                }
            }
            Ok(drained)
        })
    }
}

impl NetworkSegmentAllocator for ConfiguredSegmentAllocator {
    type Segment = OciSegmentRealization;
    type Error = SandboxError;

    fn segment_for(&self, tenant: &TenantId) -> Result<Self::Segment> {
        self.inner()?.segment_for(tenant)
    }

    fn acquire(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
    ) -> Result<Self::Segment> {
        self.inner()?.acquire(tenant, attachment_id)
    }

    fn release(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
    ) -> Result<NetworkSegmentReleaseOutcome<Self::Segment>> {
        self.inner()?.release(tenant, attachment_id)
    }

    fn grow_block(&self, tenant: &TenantId) -> Result<Self::Segment> {
        self.inner()?.grow_block(tenant)
    }

    fn reconcile_orphans(
        &self,
        live: &BTreeSet<(TenantId, NetworkAttachmentId)>,
    ) -> Result<Vec<Self::Segment>> {
        self.inner()?.reconcile_orphans(live)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn tenant(id: &str) -> TenantId {
        TenantId::new(id).expect("tenant id should parse")
    }

    fn attachment(id: &str) -> NetworkAttachmentId {
        NetworkAttachmentId::for_workload_attachment(id, super::super::DEFAULT_ATTACHMENT_NAME)
    }

    #[test]
    fn two_tenants_get_distinct_subnets_bridges_and_ids() {
        let dir = tempdir().expect("temp dir");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let a = allocator
            .segment_for(&tenant("tenant-a"))
            .expect("assign a");
        let b = allocator
            .segment_for(&tenant("tenant-b"))
            .expect("assign b");

        assert_eq!(a.cidr().to_string(), "10.0.0.0/24");
        assert_eq!(b.cidr().to_string(), "10.0.1.0/24");
        assert!(
            !a.cidr().overlaps(&b.cidr()),
            "tenant subnets must not overlap"
        );
        assert_ne!(a.network_interface(), b.network_interface());
        assert_ne!(a.network_id().as_str(), b.network_id().as_str());
    }

    #[test]
    fn assign_is_idempotent_per_tenant() {
        let dir = tempdir().expect("temp dir");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let first = allocator.segment_for(&tenant("tenant-a")).expect("assign");
        let again = allocator
            .segment_for(&tenant("tenant-a"))
            .expect("re-assign");
        assert_eq!(first.cidr(), again.cidr());
        assert_eq!(first.segment_id(), again.segment_id());
        assert_eq!(first.network_id().as_str(), again.network_id().as_str());
    }

    #[test]
    fn node_supernets_mint_distinct_stable_segment_ids_at_the_same_local_slot() {
        let node_a_root = tempdir().expect("node A temp dir");
        let node_b_root = tempdir().expect("node B temp dir");
        let node_a = SingleNodeSegmentAllocator::for_node_supernet(
            node_a_root.path(),
            "10.10.0.0/16",
            DEFAULT_TENANT_PREFIX,
        )
        .expect("node A allocator");
        let node_b = SingleNodeSegmentAllocator::for_node_supernet(
            node_b_root.path(),
            "10.20.0.0/16",
            DEFAULT_TENANT_PREFIX,
        )
        .expect("node B allocator");

        let segment_a = node_a
            .segment_for(&tenant("tenant-shared"))
            .expect("node A segment");
        let segment_b = node_b
            .segment_for(&tenant("tenant-shared"))
            .expect("node B segment");

        assert_eq!(segment_a.network_interface(), "nb-0");
        assert_eq!(segment_b.network_interface(), "nb-0");
        assert_eq!(segment_a.cidr().to_string(), "10.10.0.0/24");
        assert_eq!(segment_b.cidr().to_string(), "10.20.0.0/24");
        assert_ne!(
            segment_a.segment_id(),
            segment_b.segment_id(),
            "global segment identity must not alias merely because two nodes use local slot zero"
        );

        let restarted_a = SingleNodeSegmentAllocator::for_node_supernet(
            node_a_root.path(),
            "10.10.0.0/16",
            DEFAULT_TENANT_PREFIX,
        )
        .expect("restarted node A allocator")
        .segment_for(&tenant("tenant-shared"))
        .expect("restarted node A segment");
        let restarted_b = SingleNodeSegmentAllocator::for_node_supernet(
            node_b_root.path(),
            "10.20.0.0/16",
            DEFAULT_TENANT_PREFIX,
        )
        .expect("restarted node B allocator")
        .segment_for(&tenant("tenant-shared"))
        .expect("restarted node B segment");

        assert_eq!(restarted_a.segment_id(), segment_a.segment_id());
        assert_eq!(restarted_b.segment_id(), segment_b.segment_id());
        assert_eq!(restarted_a.tenant_id(), &tenant("tenant-shared"));
        assert_eq!(restarted_b.tenant_id(), &tenant("tenant-shared"));
        assert_eq!(restarted_a.lease_epoch(), NetworkLeaseEpoch::new(0));
        assert_eq!(restarted_b.lease_epoch(), NetworkLeaseEpoch::new(0));
    }

    #[test]
    #[ignore = "NNC0.9 explicit allocation-scale characterization"]
    fn durable_segment_assignment_scale_baseline() {
        const SAMPLE_COUNT: usize = 21;

        for existing_tenants in [0usize, 64, 256, 1_024] {
            let dir = tempdir().expect("temp dir");
            let allocator = SingleNodeSegmentAllocator::for_node_supernet(
                dir.path(),
                "10.0.0.0/8",
                DEFAULT_TENANT_PREFIX,
            )
            .expect("baseline allocator should accept the node super-net");

            let seed_started = std::time::Instant::now();
            for index in 0..existing_tenants {
                allocator
                    .segment_for(&tenant(&format!("baseline-seed-{index:04}")))
                    .expect("baseline seed assignment should fit the super-net");
            }
            let seed_elapsed_ns = seed_started.elapsed().as_nanos();

            let mut samples_ns = Vec::with_capacity(SAMPLE_COUNT);
            for sample in 0..SAMPLE_COUNT {
                let expected_index = existing_tenants + sample;
                let expected_cidr = Cidr::parse("10.0.0.0/8")
                    .expect("baseline super-net should parse")
                    .nth_subnet(
                        DEFAULT_TENANT_PREFIX,
                        u64::try_from(expected_index).expect("baseline index fits u64"),
                    )
                    .expect("baseline subnet should fit");
                let started = std::time::Instant::now();
                let segment = allocator
                    .segment_for(&tenant(&format!("baseline-sample-{sample:02}")))
                    .expect("baseline sample assignment should fit the super-net");
                samples_ns.push(started.elapsed().as_nanos());
                assert_eq!(
                    segment.cidr(),
                    expected_cidr,
                    "durable allocation must remain lowest-free and collision-free at scale"
                );
            }
            samples_ns.sort_unstable();
            let p95_index = (SAMPLE_COUNT * 95).div_ceil(100) - 1;

            println!(
                "NNC0.9 segment-allocation-baseline existing_tenants={existing_tenants} seed_total_ns={seed_elapsed_ns} samples={SAMPLE_COUNT} median_ns={} p95_ns={}",
                samples_ns[SAMPLE_COUNT / 2],
                samples_ns[p95_index]
            );
        }
    }

    #[test]
    fn refcount_frees_the_index_only_after_the_last_sandbox_releases() {
        let dir = tempdir().expect("temp dir");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let a = tenant("tenant-a");

        // Two sandboxes of tenant-a hold the same segment.
        let s1 = allocator
            .acquire(&a, &attachment("sb-1"))
            .expect("acquire sb-1");
        let s2 = allocator
            .acquire(&a, &attachment("sb-2"))
            .expect("acquire sb-2");
        assert_eq!(s1.cidr().to_string(), "10.0.0.0/24");
        assert_eq!(s1.cidr(), s2.cidr(), "same tenant shares one segment");

        // Releasing one leaves the tenant live — the bridge stays, index held.
        assert!(matches!(
            allocator
                .release(&a, &attachment("sb-1"))
                .expect("release sb-1"),
            NetworkSegmentReleaseOutcome::StillLive
        ));
        // A fresh tenant does NOT get tenant-a's still-held index.
        let b = allocator
            .acquire(&tenant("tenant-b"), &attachment("sb-b"))
            .expect("acquire b");
        assert_eq!(b.cidr().to_string(), "10.0.1.0/24");

        // Releasing the LAST sandbox drains the tenant and frees the index.
        assert!(matches!(
            allocator
                .release(&a, &attachment("sb-2"))
                .expect("release sb-2"),
            NetworkSegmentReleaseOutcome::TenantDrained { .. }
        ));
        // The freed lowest index (10.0.0.0/24) is handed to the next new tenant.
        let c = allocator
            .acquire(&tenant("tenant-c"), &attachment("sb-c"))
            .expect("acquire c");
        assert_eq!(c.cidr().to_string(), "10.0.0.0/24");
        assert_ne!(
            c.segment_id(),
            s1.segment_id(),
            "reusing a cleaned local slot must mint a new global allocation identity"
        );

        // Releasing an unknown sandbox is idempotent.
        assert!(matches!(
            allocator
                .release(&tenant("nobody"), &attachment("ghost"))
                .expect("release ghost"),
            NetworkSegmentReleaseOutcome::StillLive
        ));
    }

    #[test]
    fn exhaustion_fails_closed() {
        let dir = tempdir().expect("temp dir");
        // A /30 super-net carved into /30 tenant subnets holds exactly one tenant.
        let supernet = InstalledSuperNet {
            cidr: Cidr::parse("10.9.0.0/30").unwrap(),
            epoch: 0,
        };
        let allocator = SingleNodeSegmentAllocator::new(dir.path(), Some(supernet), 30)
            .expect("local network store should open");
        allocator
            .segment_for(&tenant("t0"))
            .expect("first tenant fits");
        let error = allocator
            .segment_for(&tenant("t1"))
            .expect_err("second tenant must not fit a single-child super-net");
        assert!(
            format!("{error}").contains("pool exhausted"),
            "exhaustion must fail closed, got: {error}"
        );
    }

    #[test]
    fn assign_fails_closed_until_a_supernet_is_installed() {
        let dir = tempdir().expect("temp dir");
        let mut allocator =
            SingleNodeSegmentAllocator::new(dir.path(), None, DEFAULT_TENANT_PREFIX)
                .expect("local network store should open");
        let error = allocator
            .segment_for(&tenant("tenant-a"))
            .expect_err("no super-net installed must fail closed");
        assert!(
            format!("{error}").contains("unassigned"),
            "must fail closed as unassigned, got: {error}"
        );
        allocator.install_supernet(InstalledSuperNet {
            cidr: Cidr::parse(DEFAULT_NODE_SUPERNET).unwrap(),
            epoch: 0,
        });
        let seg = allocator
            .segment_for(&tenant("tenant-a"))
            .expect("assign after install");
        assert_eq!(seg.cidr().to_string(), "10.0.0.0/24");
    }

    #[test]
    fn a_stale_epoch_carve_fails_closed_on_load() {
        let dir = tempdir().expect("temp dir");
        let epoch0 = SingleNodeSegmentAllocator::new(
            dir.path(),
            Some(InstalledSuperNet {
                cidr: Cidr::parse(DEFAULT_NODE_SUPERNET).unwrap(),
                epoch: 0,
            }),
            DEFAULT_TENANT_PREFIX,
        )
        .expect("epoch 0 store should open");
        epoch0
            .segment_for(&tenant("tenant-a"))
            .expect("carve at epoch 0");
        // A later allocator with a bumped epoch must refuse the stale state.
        let epoch1 = SingleNodeSegmentAllocator::new(
            dir.path(),
            Some(InstalledSuperNet {
                cidr: Cidr::parse(DEFAULT_NODE_SUPERNET).unwrap(),
                epoch: 1,
            }),
            DEFAULT_TENANT_PREFIX,
        )
        .expect("epoch 1 store should open");
        let error = epoch1
            .segment_for(&tenant("tenant-b"))
            .expect_err("stale-epoch state must fail closed");
        assert!(
            format!("{error}").contains("epoch"),
            "must fail closed on epoch mismatch, got: {error}"
        );
    }

    #[test]
    fn concurrent_acquire_release_across_threads_stays_consistent_under_the_lock() {
        use std::sync::Arc;
        use std::thread;

        let dir = tempdir().expect("temp dir");
        let root = Arc::new(dir.path().to_path_buf());
        // 8 threads, each a distinct tenant, contending on the shared on-disk
        // shared network authority: acquire a sole sandbox then
        // release it, which must drain the tenant.
        let handles: Vec<_> = (0..8u32)
            .map(|i| {
                let root = Arc::clone(&root);
                thread::spawn(move || {
                    let allocator = SingleNodeSegmentAllocator::single_node_default(&root);
                    let tenant = tenant(&format!("t-{i}"));
                    let attachment = attachment(&format!("sb-{i}"));
                    let segment = allocator.acquire(&tenant, &attachment).expect("acquire");
                    assert!(segment.cidr().to_string().starts_with("10.0."));
                    matches!(
                        allocator.release(&tenant, &attachment).expect("release"),
                        NetworkSegmentReleaseOutcome::TenantDrained { .. }
                    )
                })
            })
            .collect();
        for handle in handles {
            assert!(handle.join().expect("thread should not panic"));
        }
        // Every tenant drained, so the freed lowest index is reused: the next new
        // tenant gets 10.0.0.0/24 (no leaked reservations under contention).
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let segment = allocator
            .acquire(&tenant("after"), &attachment("sb"))
            .expect("acquire after drain");
        assert_eq!(segment.cidr().to_string(), "10.0.0.0/24");
    }

    #[test]
    fn reconcile_orphans_prunes_leaked_holds_and_drains_empty_tenants() {
        let dir = tempdir().expect("temp dir");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        // tenant-a holds two sandboxes; sb-1 will be a crash-leaked hold, sb-2 live.
        allocator
            .acquire(&tenant("tenant-a"), &attachment("sb-1"))
            .expect("acquire a/1");
        allocator
            .acquire(&tenant("tenant-a"), &attachment("sb-2"))
            .expect("acquire a/2");
        // tenant-b holds one sandbox, fully crash-leaked (nothing live).
        let b = allocator
            .acquire(&tenant("tenant-b"), &attachment("sb-b"))
            .expect("acquire b");
        assert_eq!(b.cidr().to_string(), "10.0.1.0/24");

        // Only tenant-a/sb-2 is actually live at startup.
        let mut live = BTreeSet::new();
        live.insert((tenant("tenant-a"), attachment("sb-2")));
        let drained = allocator.reconcile_orphans(&live).expect("reconcile");

        // tenant-b fully orphaned -> drained (its bridge must be reaped), segment returned.
        assert_eq!(drained.len(), 1, "only the fully-orphaned tenant drains");
        assert_eq!(drained[0].cidr().to_string(), "10.0.1.0/24");
        // tenant-a still holds sb-2, so its index 0 is retained; the freed index 1
        // is what the next new tenant reuses.
        let c = allocator
            .acquire(&tenant("tenant-c"), &attachment("sb-c"))
            .expect("acquire c");
        assert_eq!(c.cidr().to_string(), "10.0.1.0/24");
        // tenant-a's still-live sandbox keeps its original segment.
        let a = allocator
            .acquire(&tenant("tenant-a"), &attachment("sb-2"))
            .expect("re-acquire a/2");
        assert_eq!(a.cidr().to_string(), "10.0.0.0/24");
    }

    #[test]
    fn grow_block_appends_a_distinct_block_and_never_collides_across_tenants() {
        let dir = tempdir().expect("temp dir");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let a = tenant("tenant-a");
        // tenant-a's primary block = index 0 (10.0.0.0/24).
        let a0 = allocator
            .acquire(&a, &attachment("sb-a"))
            .expect("acquire a");
        assert_eq!(a0.cidr().to_string(), "10.0.0.0/24");
        // Grow tenant-a: a second, distinct block/bridge at index 1.
        let a1 = allocator.grow_block(&a).expect("grow a");
        assert_eq!(a1.cidr().to_string(), "10.0.1.0/24");
        assert_ne!(a0.network_interface(), a1.network_interface());
        assert_ne!(a0.network_id().as_str(), a1.network_id().as_str());
        // The M1 guard: a DIFFERENT tenant must NEVER be handed tenant-a's grown
        // index 1 — the unioned lowest-free scan skips it to index 2.
        let b = allocator
            .acquire(&tenant("tenant-b"), &attachment("sb-b"))
            .expect("acquire b");
        assert_eq!(b.cidr().to_string(), "10.0.2.0/24");
    }

    #[test]
    fn draining_a_multi_block_tenant_returns_every_block_to_reap() {
        let dir = tempdir().expect("temp dir");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let a = tenant("tenant-a");
        allocator.acquire(&a, &attachment("sb-1")).expect("acquire");
        allocator.grow_block(&a).expect("grow to block 1");
        allocator.grow_block(&a).expect("grow to block 2");

        // The sole sandbox's release drains the tenant and returns ALL 3 block
        // bridges so the caller reaps every one.
        let NetworkSegmentReleaseOutcome::TenantDrained { segments } =
            allocator.release(&a, &attachment("sb-1")).expect("release")
        else {
            panic!("expected the last release to drain the tenant");
        };
        let subnets: Vec<String> = segments.iter().map(|s| s.cidr().to_string()).collect();
        assert_eq!(subnets, ["10.0.0.0/24", "10.0.1.0/24", "10.0.2.0/24"]);
        // All 3 indices are freed, so the next new tenant reuses index 0.
        let c = allocator
            .acquire(&tenant("tenant-c"), &attachment("sb-c"))
            .expect("acquire c");
        assert_eq!(c.cidr().to_string(), "10.0.0.0/24");
    }

    #[test]
    fn grow_block_fails_closed_at_pool_exhaustion() {
        let dir = tempdir().expect("temp dir");
        // A /24 super-net carved into /24 blocks holds exactly ONE block.
        let supernet = InstalledSuperNet {
            cidr: Cidr::parse("10.9.0.0/24").unwrap(),
            epoch: 0,
        };
        let allocator = SingleNodeSegmentAllocator::new(dir.path(), Some(supernet), 24)
            .expect("local network store should open");
        let t = tenant("t0");
        allocator
            .acquire(&t, &attachment("sb"))
            .expect("primary block fits");
        let error = allocator
            .grow_block(&t)
            .expect_err("a second block must not fit a single-child super-net");
        assert!(
            format!("{error}").contains("pool exhausted"),
            "grow must fail closed on exhaustion, got: {error}"
        );
    }

    #[test]
    fn torn_segment_state_error_must_name_the_authority_path() {
        let dir = tempdir().expect("temp dir");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        allocator
            .acquire(&tenant("tenant-original"), &attachment("sandbox-original"))
            .expect("original segment should allocate");
        let state_path = allocator.state_path();
        fs::write(state_path, b"{").expect("torn state should be installed");

        let error = match SingleNodeSegmentAllocator::for_node_supernet(
            dir.path(),
            DEFAULT_NODE_SUPERNET,
            DEFAULT_TENANT_PREFIX,
        ) {
            Ok(_) => panic!("torn segment authority must fail closed during startup"),
            Err(error) => error,
        };
        let rendered = error.to_string();
        assert!(
            rendered.contains("network authority state") && rendered.contains("corrupt"),
            "the failure must reach the checksummed authority boundary: {rendered}"
        );
        assert!(
            rendered.contains(&state_path.display().to_string()),
            "a corruption diagnostic must name the affected authority path: {rendered}"
        );
    }

    #[test]
    fn semantically_valid_segment_state_corruption_must_not_reissue_a_live_segment() {
        let dir = tempdir().expect("temp dir");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let original = allocator
            .acquire(&tenant("tenant-original"), &attachment("sandbox-original"))
            .expect("original segment should allocate");
        let state_path = allocator.state_path();
        let mut envelope: serde_json::Value =
            serde_json::from_slice(&fs::read(state_path).expect("authority should read"))
                .expect("authority envelope should parse");
        envelope["body"]["records"]["segment-allocations"]["tenants"] = serde_json::json!({});
        fs::write(
            state_path,
            serde_json::to_vec_pretty(&envelope).expect("tampered envelope should render"),
        )
        .expect("semantically corrupt state should be installed without updating its checksum");

        let error = match SingleNodeSegmentAllocator::for_node_supernet(
            dir.path(),
            DEFAULT_NODE_SUPERNET,
            DEFAULT_TENANT_PREFIX,
        ) {
            Ok(restarted) => {
                let replacement = restarted.acquire(
                    &tenant("tenant-replacement"),
                    &attachment("sandbox-replacement"),
                );
                if let Ok(segment) = &replacement {
                    assert_eq!(
                        segment.cidr(),
                        original.cidr(),
                        "unchecked corruption would expose the audited live-segment reuse"
                    );
                }
                replacement.expect_err(
                    "semantically valid corruption must fail closed instead of reissuing a live segment",
                )
            }
            Err(error) => error,
        };
        let rendered = error.to_string();
        assert!(
            ["checksum", "corrupt", "integrity", "version"]
                .iter()
                .any(|needle| rendered.to_ascii_lowercase().contains(needle)),
            "the store must reject corruption with a named integrity error: {rendered}"
        );
    }
}
