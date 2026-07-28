//! Cluster network-segment allocation (MTN7).
//!
//! In a cluster, each node's per-tenant subnets must be carved from a super-net
//! DISJOINT from every other node's — otherwise two nodes' tenants collide (the
//! cross-node analogue of audit M1). The single-node allocator installs a fixed
//! node-0 slice; the cluster allocator instead consumes a raft-committed,
//! epoch-fenced, TTL-bounded lease and carves the SAME single-node way beneath
//! it. This remains a future-only seam: its wall-expiry safety model is not yet
//! proven, so cluster admission is deliberately blocked below.
//!
//! WHERE the lease comes from is a SEAM ([`ClusterLeaseProvider`]): the concrete
//! provider reads the openraft-committed lease (the horizontal-scaling lane). This
//! module owns the allocator plus ALL the fencing/admission LOGIC and is tested
//! here against an in-memory provider — a legitimate test double, not a stub: the
//! allocator's behaviour is fully exercised. Fail-closed by construction: no
//! committed lease → no allocation (no config-default fallback); locally
//! observed expiry → the node self-fences new create/grow authority while a
//! restricted authority derived from durable state retains inspection and
//! cleanup for old handles; stale epoch → drain and re-carve (via the inner
//! allocator's `ensure_supernet_matches`). Those local checks are not sufficient
//! distributed authority: before promotion, HS5 must either prove a
//! maximum leader/node skew plus observation-delay model whose reassignment
//! grace prevents overlap, or replace wall expiry with a clock-free authority.
//! The resulting epoch must be validated atomically with every protected write.
//!
//! Every item here is wired by the horizontal-scaling cluster lane (which supplies
//! the concrete `ClusterLeaseProvider` and calls [`assert_cluster_admission`] at
//! mesh join); until then it is exercised only by this module's tests.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nimbus_core::TenantId;
use nimbus_core::net::Cidr;
use nimbus_network::{
    NetworkAttachmentId, NetworkLeaseEpoch, NetworkReservationClaim, NetworkSegmentAllocator,
    NetworkSegmentCleanup, NetworkSegmentFinalizeOutcome, NetworkSegmentGrowth,
    NetworkSegmentQuarantineOutcome, NetworkSegmentReleaseOutcome,
};

use crate::error::{Result, SandboxError};

use super::segment::{
    DurableSegmentCleanupAuthority, InstalledSuperNet, SingleNodeSegmentAllocator,
};
use super::{OciSegmentAllocator, OciSegmentRealization};

/// Promotion gate owned by horizontal-scaling HS5.
///
/// This must not become `true` until deterministic tests cover forward/backward
/// node time, delayed committed observation, partition, restart, stale epoch,
/// and concurrent reassignment, and the selected authority rejects stale epochs
/// atomically with protected writes.
const CLUSTER_LEASE_CLOCK_MODEL_PROVEN: bool = false;

/// A raft-committed, epoch-fenced, TTL-bounded grant of a node super-net.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SuperNetLease {
    /// The super-net this node may carve per-tenant subnets from. The leader keeps
    /// live leases' super-nets pairwise disjoint.
    pub(crate) super_net: Cidr,
    /// The lease's fencing epoch. A reclamation bumps it; the inner allocator's
    /// persisted state (stamped with the old epoch) then fails closed until drain
    /// and re-carve.
    pub(crate) epoch: NetworkLeaseEpoch,
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
    fn live_inner(&self) -> Result<SingleNodeSegmentAllocator> {
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
        SingleNodeSegmentAllocator::new(
            &self.state_root,
            Some(InstalledSuperNet {
                cidr: lease.super_net,
                epoch: lease.epoch,
            }),
            self.tenant_prefix,
        )
    }

    /// Open only the inspection/cleanup capability encoded by durable state.
    ///
    /// This path deliberately does not consult the current lease provider:
    /// expiry, partition, or a newly observed epoch must revoke creation but
    /// cannot strand provider effects owned by a previously committed epoch.
    /// The returned type has no assign/acquire/grow methods, so durable cleanup
    /// state cannot be confused with live creation authority.
    fn cleanup_inner(&self) -> Result<Option<DurableSegmentCleanupAuthority>> {
        DurableSegmentCleanupAuthority::open(&self.state_root, self.tenant_prefix)
    }
}

impl NetworkSegmentAllocator for ClusterSegmentAllocator {
    type Segment = OciSegmentRealization;
    type Error = SandboxError;

    fn segment_for(&self, tenant: &TenantId) -> Result<OciSegmentRealization> {
        self.live_inner()?.segment_for(tenant)
    }

    fn segments_for(&self, tenant: &TenantId) -> Result<Vec<OciSegmentRealization>> {
        self.live_inner()?.segments_for(tenant)
    }

    fn inspect_segments(&self, tenant: &TenantId) -> Result<Option<Vec<OciSegmentRealization>>> {
        match self.cleanup_inner()? {
            Some(cleanup) => cleanup.inspect_segments(tenant),
            None => Ok(None),
        }
    }

    fn inspect_attachment_reservation(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<nimbus_network::NetworkAttachmentReservationState> {
        match self.cleanup_inner()? {
            Some(cleanup) => {
                cleanup.inspect_attachment_reservation(tenant, attachment_id, reservation_claim)
            }
            None => Ok(nimbus_network::NetworkAttachmentReservationState::Absent),
        }
    }

    fn reserve_attachment_for_coordinator(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<()> {
        self.live_inner()?.reserve_attachment_for_coordinator(
            tenant,
            attachment_id,
            reservation_claim,
        )
    }

    fn bind_reserved_attachment_to_segment(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        segment_id: &nimbus_network::NetworkSegmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<OciSegmentRealization> {
        self.live_inner()?.bind_reserved_attachment_to_segment(
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
        self.live_inner()?
            .adopt_reserved_attachment(tenant, attachment_id, reservation_claim)
    }

    fn release_reserved_attachment_without_effect(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<NetworkSegmentReleaseOutcome<OciSegmentRealization>> {
        match self.cleanup_inner()? {
            Some(cleanup) => cleanup.release_reserved_attachment_without_effect(
                tenant,
                attachment_id,
                reservation_claim,
            ),
            None => Ok(NetworkSegmentReleaseOutcome::AlreadyReleased),
        }
    }

    fn finalize_reserved_attachment_without_effect(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<NetworkSegmentReleaseOutcome<OciSegmentRealization>> {
        match self.cleanup_inner()? {
            Some(cleanup) => cleanup.finalize_reserved_attachment_without_effect(
                tenant,
                attachment_id,
                reservation_claim,
            ),
            None => Ok(NetworkSegmentReleaseOutcome::AlreadyReleased),
        }
    }

    fn acquire(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
    ) -> Result<OciSegmentRealization> {
        self.live_inner()?.acquire(tenant, attachment_id)
    }

    fn quarantine(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        expected_adoption_receipt: Option<&NetworkReservationClaim>,
    ) -> Result<NetworkSegmentQuarantineOutcome> {
        match self.cleanup_inner()? {
            Some(cleanup) => cleanup.quarantine(tenant, attachment_id, expected_adoption_receipt),
            None => Ok(NetworkSegmentQuarantineOutcome::AlreadyReleased),
        }
    }

    fn release(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        expected_adoption_receipt: Option<&NetworkReservationClaim>,
    ) -> Result<NetworkSegmentReleaseOutcome<OciSegmentRealization>> {
        match self.cleanup_inner()? {
            Some(cleanup) => cleanup.release(tenant, attachment_id, expected_adoption_receipt),
            None => Ok(NetworkSegmentReleaseOutcome::AlreadyReleased),
        }
    }

    fn finalize_release(
        &self,
        cleanup: &NetworkSegmentCleanup<OciSegmentRealization>,
    ) -> Result<NetworkSegmentFinalizeOutcome> {
        match self.cleanup_inner()? {
            Some(authority) => authority.finalize_release(cleanup),
            None => Ok(NetworkSegmentFinalizeOutcome::AlreadyReleased),
        }
    }

    fn grow_block_if_current(
        &self,
        tenant: &TenantId,
        observed_segments: &[OciSegmentRealization],
    ) -> Result<NetworkSegmentGrowth<OciSegmentRealization>> {
        self.live_inner()?
            .grow_block_if_current(tenant, observed_segments)
    }

    fn reconcile_orphans(
        &self,
        live: &BTreeSet<(TenantId, NetworkAttachmentId)>,
    ) -> Result<Vec<OciSegmentRealization>> {
        match self.cleanup_inner()? {
            Some(cleanup) => cleanup.reconcile_orphans(live),
            None => Ok(Vec::new()),
        }
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
    allocator: &OciSegmentAllocator,
) -> Result<()> {
    if mesh_joined {
        if !allocator.requires_cluster_lease() {
            return Err(SandboxError::OperationFailed {
                message: "node has joined the cluster mesh but is still using the single-node \
                          segment allocator (config-default super-net); refusing to start — a \
                          mesh-joined node must use the lease-gated ClusterSegmentAllocator"
                    .to_owned(),
            });
        }
        if !CLUSTER_LEASE_CLOCK_MODEL_PROVEN {
            return Err(SandboxError::OperationFailed {
                message: "cluster network-segment admission remains blocked until \
                          horizontal-scaling HS5 proves the leader/node clock-skew and \
                          observation-delay contract (or adopts clock-free authority) and \
                          atomically fences stale epochs on protected writes"
                    .to_owned(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use nimbus_network::{LocalNetworkStateStore, NetworkProviderHandle, NetworkProviderId};
    use tempfile::tempdir;

    fn tenant(id: &str) -> TenantId {
        TenantId::new(id).expect("tenant id should parse")
    }

    fn attachment(id: &str) -> NetworkAttachmentId {
        NetworkAttachmentId::for_workload_attachment(id, super::super::DEFAULT_ATTACHMENT_NAME)
    }

    fn reservation_claim(attempt: &str) -> NetworkReservationClaim {
        let provider = NetworkProviderId::for_registration_key(
            "nimbus-sandbox.network-launch-coordinator.test",
        );
        NetworkReservationClaim::new(
            NetworkProviderHandle::new(provider, format!("attempt:{attempt}"))
                .expect("claim fixture should validate"),
        )
    }

    fn lease(super_net: &str, epoch: NetworkLeaseEpoch, expires_at_millis: u64) -> SuperNetLease {
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
        let node_a = node(
            dir_a.path(),
            Some(lease("10.10.0.0/16", NetworkLeaseEpoch::new(1), 10_000)),
            0,
        );
        let node_b = node(
            dir_b.path(),
            Some(lease("10.20.0.0/16", NetworkLeaseEpoch::new(1), 10_000)),
            0,
        );

        let seg_a = node_a
            .acquire(&tenant("t"), &attachment("s"))
            .expect("node A");
        let seg_b = node_b
            .acquire(&tenant("t"), &attachment("s"))
            .expect("node B");

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
            .acquire(&tenant("t"), &attachment("s"))
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
        let node = node(
            dir.path(),
            Some(lease("10.10.0.0/16", NetworkLeaseEpoch::new(1), 5_000)),
            5_000,
        );
        let error = node
            .acquire(&tenant("t"), &attachment("s"))
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
        // Carve under epoch 1 (stamps the shared network authority with epoch 1).
        let epoch1 = node(
            dir.path(),
            Some(lease("10.10.0.0/16", NetworkLeaseEpoch::new(1), 10_000)),
            0,
        );
        epoch1
            .acquire(&tenant("t"), &attachment("s"))
            .expect("epoch-1 carve succeeds");
        // SAME super-net, epoch 2 (a reclamation) over the SAME state: fail closed.
        let epoch2 = node(
            dir.path(),
            Some(lease("10.10.0.0/16", NetworkLeaseEpoch::new(2), 10_000)),
            0,
        );
        let error = epoch2
            .acquire(&tenant("t2"), &attachment("s2"))
            .expect_err("stale-epoch state must fail closed");
        assert!(
            format!("{error}").contains("epoch") || format!("{error}").contains("drain"),
            "got: {error}"
        );
    }

    /// The startup admission assertion refuses both a single-node allocator and
    /// the future cluster allocator while its clock-skew proof gate is open.
    #[test]
    fn cluster_mode_remains_blocked_until_clock_skew_contract_is_proven() {
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
        let error = assert_cluster_admission(true, &cluster)
            .expect_err("the future cluster allocator remains blocked before HS5 proof");
        let message = error.to_string();
        assert!(message.contains("clock-skew"), "got: {message}");
        assert!(message.contains("stale epochs"), "got: {message}");
    }

    struct MutableClockLeaseProvider {
        lease: Mutex<Option<SuperNetLease>>,
        now: AtomicU64,
    }

    impl ClusterLeaseProvider for MutableClockLeaseProvider {
        fn current_lease(&self) -> Option<SuperNetLease> {
            self.lease
                .lock()
                .expect("mutable lease provider lock should not be poisoned")
                .clone()
        }

        fn now_millis(&self) -> u64 {
            self.now.load(Ordering::SeqCst)
        }
    }

    #[test]
    fn expired_lease_must_fence_creation_but_allow_cleanup_of_a_durable_hold() {
        let dir = tempdir().expect("temp dir");
        let provider = Arc::new(MutableClockLeaseProvider {
            lease: Mutex::new(Some(lease(
                "10.10.0.0/16",
                NetworkLeaseEpoch::new(7),
                5_000,
            ))),
            now: AtomicU64::new(0),
        });
        let allocator = ClusterSegmentAllocator::new(dir.path(), 24, provider.clone());
        let original_tenant = tenant("tenant-original");
        let original_attachment = attachment("sandbox-original");
        let original = allocator
            .acquire(&original_tenant, &original_attachment)
            .expect("the durable hold should be created under the live lease");
        let authority_path = LocalNetworkStateStore::authority_path_for(dir.path());
        let before_expiry =
            fs::read(&authority_path).expect("durable authority should exist after acquire");

        provider.now.store(5_000, Ordering::SeqCst);
        let inspected = allocator
            .inspect_segments(&original_tenant)
            .expect("expiry must retain non-creating inspection")
            .expect("the durable old allocation must remain inspectable");
        assert_eq!(inspected, vec![original.clone()]);

        let create_error = allocator
            .acquire(
                &tenant("tenant-replacement"),
                &attachment("sandbox-replacement"),
            )
            .expect_err("lease expiry must continue to fence new creation");
        assert!(
            create_error.to_string().contains("expired"),
            "the creation refusal must be the lease-expiry boundary: {create_error}"
        );
        let primary_assign_error = allocator
            .segment_for(&original_tenant)
            .expect_err("primary assigning lookup must require live authority");
        assert!(
            primary_assign_error.to_string().contains("expired"),
            "primary assigning lookup must remain behind the expiry fence: {primary_assign_error}"
        );
        let assigning_read_error = allocator
            .segments_for(&original_tenant)
            .expect_err("even an existing assigning lookup must require live authority");
        assert!(
            assigning_read_error.to_string().contains("expired"),
            "assign-capable lookup must remain behind the expiry fence: {assigning_read_error}"
        );
        let grow_error = allocator
            .grow_block_if_current(&original_tenant, &inspected)
            .expect_err("lease expiry must fence allocation growth");
        assert!(
            grow_error.to_string().contains("expired"),
            "growth refusal must remain the lease-expiry boundary: {grow_error}"
        );
        assert_eq!(
            fs::read(&authority_path)
                .expect("rejected create/grow and inspection must preserve authority"),
            before_expiry,
            "inspection and rejected creation/growth must not mutate durable authority"
        );

        assert_eq!(
            allocator
                .quarantine(&original_tenant, &original_attachment, None)
                .expect("expiry must retain durable quarantine authority"),
            NetworkSegmentQuarantineOutcome::CleanupPending
        );
        *provider
            .lease
            .lock()
            .expect("mutable lease provider lock should not be poisoned") =
            Some(lease("10.20.0.0/16", NetworkLeaseEpoch::new(8), 10_000));
        assert_eq!(
            allocator
                .inspect_segments(&original_tenant)
                .expect("a new reported epoch must not hide durable old cleanup state"),
            Some(vec![original.clone()])
        );
        let reassigned_create_error = allocator
            .acquire(
                &tenant("tenant-new-epoch"),
                &attachment("sandbox-new-epoch"),
            )
            .expect_err("a new lease must not overwrite durable old-epoch state");
        assert!(
            reassigned_create_error.to_string().contains("super-net")
                || reassigned_create_error.to_string().contains("epoch"),
            "new-epoch creation must fail on the durable old fence: {reassigned_create_error}"
        );
        *provider
            .lease
            .lock()
            .expect("mutable lease provider lock should not be poisoned") = None;
        let restarted = ClusterSegmentAllocator::new(dir.path(), 24, provider);
        assert_eq!(
            restarted
                .inspect_segments(&original_tenant)
                .expect("restart without a reported lease must inspect durable old state"),
            Some(vec![original]),
            "cleanup authority must come from the durable fenced handle, not an in-memory lease"
        );

        let NetworkSegmentReleaseOutcome::CleanupPending(cleanup) = restarted
            .release(&original_tenant, &original_attachment, None)
            .expect("confirmed detach must release the durable old hold")
        else {
            panic!("the last durable old hold must enter allocation cleanup");
        };
        assert_eq!(cleanup.lease_epoch(), NetworkLeaseEpoch::new(7));
        assert_eq!(
            restarted
                .finalize_release(&cleanup)
                .expect("provider cleanup proof must finalize the old allocation"),
            NetworkSegmentFinalizeOutcome::Released
        );
        assert_eq!(
            restarted
                .finalize_release(&cleanup)
                .expect("repeated finalization must be idempotent"),
            NetworkSegmentFinalizeOutcome::AlreadyReleased
        );
        assert_eq!(
            restarted
                .inspect_segments(&original_tenant)
                .expect("released state remains inspectable as absent"),
            None
        );
        assert_eq!(
            restarted
                .quarantine(&original_tenant, &original_attachment, None)
                .expect("repeated quarantine must be idempotent"),
            NetworkSegmentQuarantineOutcome::AlreadyReleased
        );
        assert_eq!(
            restarted
                .release(&original_tenant, &original_attachment, None)
                .expect("repeated hold release must be idempotent"),
            NetworkSegmentReleaseOutcome::AlreadyReleased
        );
        let no_lease_create_error = restarted
            .acquire(
                &tenant("tenant-still-fenced"),
                &attachment("sandbox-still-fenced"),
            )
            .expect_err("cleanup must not manufacture new create authority");
        assert!(
            no_lease_create_error.to_string().contains("not committed"),
            "new creation must remain fenced without a committed lease: {no_lease_create_error}"
        );
    }

    #[test]
    fn expired_lease_fences_claim_adoption_but_retains_exact_compensation() {
        let dir = tempdir().expect("state root");
        let provider = Arc::new(MutableClockLeaseProvider {
            lease: Mutex::new(Some(lease(
                "10.30.0.0/16",
                NetworkLeaseEpoch::new(11),
                5_000,
            ))),
            now: AtomicU64::new(0),
        });
        let allocator = ClusterSegmentAllocator::new(dir.path(), 24, provider.clone());
        let tenant = tenant("tenant-claimed-expiry");
        let attachment = attachment("sandbox-claimed-expiry");
        let claim = reservation_claim("cluster-expiry");
        allocator
            .reserve_attachment_for_coordinator(&tenant, &attachment, &claim)
            .expect("live lease should permit claimed reservation");

        provider.now.store(5_000, Ordering::SeqCst);
        let adoption_error = allocator
            .adopt_reserved_attachment(&tenant, &attachment, &claim)
            .expect_err("expired create authority must fence hold adoption");
        assert!(
            adoption_error.to_string().contains("expired"),
            "adoption must fail at the live-lease boundary: {adoption_error}"
        );

        assert_eq!(
            allocator
                .release_reserved_attachment_without_effect(&tenant, &attachment, &claim)
                .expect("durable exact compensation must fence IPAM through lease expiry"),
            NetworkSegmentReleaseOutcome::AttachmentCleanupPending
        );
        let cleanup = match allocator
            .finalize_reserved_attachment_without_effect(&tenant, &attachment, &claim)
            .expect("durable IPAM confirmation must survive lease expiry")
        {
            NetworkSegmentReleaseOutcome::CleanupPending(cleanup) => cleanup,
            outcome => panic!("last claimed finalization should return cleanup, got {outcome:?}"),
        };
        assert_eq!(cleanup.lease_epoch(), NetworkLeaseEpoch::new(11));
        assert_eq!(
            allocator
                .finalize_release(&cleanup)
                .expect("durable exact cleanup should finalize"),
            NetworkSegmentFinalizeOutcome::Released
        );
    }
}
