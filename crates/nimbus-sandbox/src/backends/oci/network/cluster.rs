//! Cluster network-segment allocation (MTN7).
//!
//! In a cluster, each node's per-tenant subnets must be carved from a super-net
//! DISJOINT from every other node's — otherwise two nodes' tenants collide (the
//! cross-node analogue of audit M1). The single-node allocator installs a fixed
//! node-0 slice; the cluster allocator instead consumes a raft-committed,
//! epoch-fenced, TTL-bounded lease and carves the SAME single-node way beneath
//! it, so the leader (sole allocator of node super-nets) guarantees disjointness.
//!
//! WHERE the lease comes from is a SEAM ([`ClusterLeaseProvider`]): the concrete
//! provider reads the openraft-committed lease (the horizontal-scaling lane). This
//! module owns the allocator plus ALL the fencing/admission LOGIC and is tested
//! here against an in-memory provider — a legitimate test double, not a stub: the
//! allocator's behaviour is fully exercised. Fail-closed by construction: no
//! committed lease → no allocation (no config-default fallback); expired lease →
//! the node self-fences; stale epoch → drain and re-carve (via the inner
//! allocator's `ensure_supernet_matches`).
//!
//! Every item here is wired by the horizontal-scaling cluster lane (which supplies
//! the concrete `ClusterLeaseProvider` and calls [`assert_cluster_admission`] at
//! mesh join); until then it is exercised only by this module's tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nimbus_core::TenantId;
use nimbus_core::net::{Cidr, NetworkSegment};

use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

use super::segment::{InstalledSuperNet, SingleNodeSegmentAllocator};
use super::{NetworkSegmentAllocator, ReleaseOutcome};

/// A raft-committed, epoch-fenced, TTL-bounded grant of a node super-net.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SuperNetLease {
    /// The super-net this node may carve per-tenant subnets from. The leader keeps
    /// live leases' super-nets pairwise disjoint.
    pub(crate) super_net: Cidr,
    /// The lease's fencing epoch. A reclamation bumps it; the inner allocator's
    /// persisted state (stamped with the old epoch) then fails closed until drain
    /// and re-carve.
    pub(crate) epoch: u64,
    /// Absolute deadline (wall-clock millis). Past it the node self-fences — the
    /// antidote to a partitioned former owner reusing a reassigned super-net. The
    /// leader must wait > this TTL before reassigning the super-net elsewhere.
    pub(crate) expires_at_millis: u64,
}

/// The seam MTN7 rides on: the source of this node's committed super-net lease.
/// The concrete implementation (horizontal-scaling lane) reads the openraft log;
/// this crate depends only on the trait (the HostBridge pattern), never on raft.
pub(crate) trait ClusterLeaseProvider: Send + Sync {
    /// The current committed lease for THIS node, or `None` when none is committed
    /// / the node is not yet admitted. A partitioned former owner returns its
    /// last-seen lease, whose TTL then expires (self-fencing).
    fn current_lease(&self) -> Option<SuperNetLease>;
    /// Injected wall-clock in millis (for TTL checks and deterministic tests).
    fn now_millis(&self) -> u64;
}

/// Cluster allocator: validates the committed lease, then carves per-tenant
/// subnets exactly the way the single-node allocator does — beneath the LEASED
/// super-net. Behind the SAME [`NetworkSegmentAllocator`] trait, so the OCI
/// backends consume it unchanged.
pub(crate) struct ClusterSegmentAllocator {
    state_root: PathBuf,
    tenant_prefix: u8,
    lease: Arc<dyn ClusterLeaseProvider>,
}

impl ClusterSegmentAllocator {
    pub(crate) fn new(
        state_root: &Path,
        tenant_prefix: u8,
        lease: Arc<dyn ClusterLeaseProvider>,
    ) -> Self {
        Self {
            state_root: state_root.to_path_buf(),
            tenant_prefix,
            lease,
        }
    }

    /// Validate the committed lease and build a single-node allocator over the
    /// LEASED super-net + epoch. This is the fail-closed cluster-admission gate:
    /// no committed lease (no config-default fallback) OR an expired lease (a
    /// partitioned former owner) both refuse to allocate. Epoch fencing is the
    /// inner allocator's `ensure_supernet_matches`: state carved under an older
    /// epoch fails closed until drain + re-carve.
    fn leased_inner(&self) -> Result<SingleNodeSegmentAllocator> {
        let lease = self
            .lease
            .current_lease()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: "cluster network-segment lease is not committed on this node; refusing \
                          to assign a tenant segment (fail-closed cluster admission, no \
                          config-default super-net fallback)"
                    .to_owned(),
            })?;
        if self.lease.now_millis() >= lease.expires_at_millis {
            return Err(SandboxError::OperationFailed {
                message: "cluster network-segment lease has expired; this node self-fences and \
                          refuses to assign a tenant segment until a fresh lease is committed \
                          (reclamation safety: a partitioned former owner must not reuse a \
                          reassigned super-net)"
                    .to_owned(),
            });
        }
        Ok(SingleNodeSegmentAllocator::new(
            &self.state_root,
            Some(InstalledSuperNet {
                cidr: lease.super_net,
                epoch: lease.epoch,
            }),
            self.tenant_prefix,
        ))
    }
}

impl NetworkSegmentAllocator for ClusterSegmentAllocator {
    fn segment_for(&self, tenant: &TenantId) -> Result<NetworkSegment> {
        self.leased_inner()?.segment_for(tenant)
    }

    fn acquire(&self, tenant: &TenantId, sandbox_id: &SandboxId) -> Result<NetworkSegment> {
        self.leased_inner()?.acquire(tenant, sandbox_id)
    }

    fn release(&self, tenant: &TenantId, sandbox_id: &SandboxId) -> Result<ReleaseOutcome> {
        self.leased_inner()?.release(tenant, sandbox_id)
    }

    fn grow_block(&self, tenant: &TenantId) -> Result<NetworkSegment> {
        self.leased_inner()?.grow_block(tenant)
    }

    fn requires_cluster_lease(&self) -> bool {
        true
    }
}

/// Fail-closed cluster admission: a node that has joined the cluster mesh MUST use
/// a lease-gated allocator ([`ClusterSegmentAllocator`]), never the single-node
/// allocator whose config-default super-net could overlap another node's range.
/// The cluster startup path calls this after mesh join.
pub(crate) fn assert_cluster_admission(
    mesh_joined: bool,
    allocator: &dyn NetworkSegmentAllocator,
) -> Result<()> {
    if mesh_joined && !allocator.requires_cluster_lease() {
        return Err(SandboxError::OperationFailed {
            message: "node has joined the cluster mesh but is still using the single-node segment \
                      allocator (config-default super-net); refusing to start — a mesh-joined \
                      node must use the lease-gated ClusterSegmentAllocator"
                .to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn tenant(id: &str) -> TenantId {
        TenantId::new(id).expect("tenant id should parse")
    }

    fn sandbox(id: &str) -> SandboxId {
        SandboxId::new(id)
    }

    fn lease(super_net: &str, epoch: u64, expires_at_millis: u64) -> SuperNetLease {
        SuperNetLease {
            super_net: Cidr::parse(super_net).expect("valid CIDR"),
            epoch,
            expires_at_millis,
        }
    }

    struct FakeLeaseProvider {
        lease: Option<SuperNetLease>,
        now: u64,
    }

    impl ClusterLeaseProvider for FakeLeaseProvider {
        fn current_lease(&self) -> Option<SuperNetLease> {
            self.lease.clone()
        }
        fn now_millis(&self) -> u64 {
            self.now
        }
    }

    fn node(state_root: &Path, lease: Option<SuperNetLease>, now: u64) -> ClusterSegmentAllocator {
        ClusterSegmentAllocator::new(state_root, 24, Arc::new(FakeLeaseProvider { lease, now }))
    }

    /// The MTN7 verifier invariant, ∀ live i≠j super_net_i ∩ super_net_j = ∅:
    /// two nodes holding DISJOINT leases carve disjoint per-tenant subnets, so the
    /// SAME tenant on both nodes never collides (the cross-node M1 fix).
    #[test]
    fn two_nodes_with_disjoint_leases_carve_disjoint_tenant_subnets() {
        let dir_a = tempdir().expect("temp dir");
        let dir_b = tempdir().expect("temp dir");
        let node_a = node(dir_a.path(), Some(lease("10.10.0.0/16", 1, 10_000)), 0);
        let node_b = node(dir_b.path(), Some(lease("10.20.0.0/16", 1, 10_000)), 0);

        let seg_a = node_a.acquire(&tenant("t"), &sandbox("s")).expect("node A");
        let seg_b = node_b.acquire(&tenant("t"), &sandbox("s")).expect("node B");

        assert_eq!(seg_a.cidr().to_string(), "10.10.0.0/24");
        assert_eq!(seg_b.cidr().to_string(), "10.20.0.0/24");
        assert!(
            !seg_a.cidr().overlaps(&seg_b.cidr()),
            "cross-node tenant subnets must be disjoint"
        );
    }

    /// Fail-closed cluster admission: no committed lease → no allocation (NO
    /// config-default super-net fallback like the single-node allocator has).
    #[test]
    fn no_committed_lease_fails_closed() {
        let dir = tempdir().expect("temp dir");
        let node = node(dir.path(), None, 0);
        let error = node
            .acquire(&tenant("t"), &sandbox("s"))
            .expect_err("a node with no committed lease must fail closed");
        assert!(format!("{error}").contains("not committed"), "got: {error}");
    }

    /// Self-fencing: once the lease TTL has elapsed the node refuses to allocate,
    /// so a partitioned former owner cannot reuse a super-net the leader may have
    /// reassigned.
    #[test]
    fn expired_lease_self_fences() {
        let dir = tempdir().expect("temp dir");
        // now == expiry: the lease is no longer valid.
        let node = node(dir.path(), Some(lease("10.10.0.0/16", 1, 5_000)), 5_000);
        let error = node
            .acquire(&tenant("t"), &sandbox("s"))
            .expect_err("an expired lease must self-fence");
        let text = format!("{error}");
        assert!(
            text.contains("expired") && text.contains("self-fences"),
            "got: {text}"
        );
    }

    /// Reclamation safety: after a super-net is reassigned with a bumped epoch, a
    /// node whose persisted state was carved under the OLD epoch fails closed until
    /// it drains + re-carves — it cannot silently reuse stale-epoch assignments.
    #[test]
    fn reclaimed_supernet_new_epoch_fails_closed_until_recarve() {
        let dir = tempdir().expect("temp dir");
        // Carve under epoch 1 (stamps segments.json with epoch 1).
        let epoch1 = node(dir.path(), Some(lease("10.10.0.0/16", 1, 10_000)), 0);
        epoch1
            .acquire(&tenant("t"), &sandbox("s"))
            .expect("epoch-1 carve succeeds");
        // SAME super-net, epoch 2 (a reclamation) over the SAME state: fail closed.
        let epoch2 = node(dir.path(), Some(lease("10.10.0.0/16", 2, 10_000)), 0);
        let error = epoch2
            .acquire(&tenant("t2"), &sandbox("s2"))
            .expect_err("stale-epoch state must fail closed");
        assert!(
            format!("{error}").contains("epoch") || format!("{error}").contains("drain"),
            "got: {error}"
        );
    }

    /// The startup admission assertion: a mesh-joined node on the single-node
    /// allocator is refused; the cluster allocator (or a non-mesh node) is fine.
    #[test]
    fn mesh_joined_node_on_single_node_allocator_is_refused() {
        let dir = tempdir().expect("temp dir");
        let single = SingleNodeSegmentAllocator::single_node_default(dir.path());
        assert!(
            assert_cluster_admission(true, &single).is_err(),
            "a mesh-joined node on the single-node allocator must be refused"
        );
        assert!(
            assert_cluster_admission(false, &single).is_ok(),
            "a non-mesh node may use the single-node allocator"
        );
        let cluster = node(dir.path(), None, 0);
        assert!(
            assert_cluster_admission(true, &cluster).is_ok(),
            "a mesh-joined node on the cluster allocator is admitted"
        );
    }
}
