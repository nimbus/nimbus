use std::collections::BTreeSet;

use nimbus_core::{Cidr, TenantId};

use crate::{NetworkAttachmentId, NetworkLeaseEpoch, NetworkSegmentId};

/// Provider-neutral allocation of one tenant network segment.
///
/// The stable identity is minted independently of the CIDR and of any
/// provider-local allocation slot. The CIDR is an assigned location and may
/// change when a later generation is re-planned; it is never workload or
/// segment identity. Effect-owning adapters compose this value with their own
/// opaque realization handles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllocatedSegment {
    segment_id: NetworkSegmentId,
    tenant_id: TenantId,
    cidr: Cidr,
    lease_epoch: NetworkLeaseEpoch,
}

impl AllocatedSegment {
    /// Compose a durable allocation identity with its assigned location and
    /// fencing context.
    pub fn new(
        segment_id: NetworkSegmentId,
        tenant_id: TenantId,
        cidr: Cidr,
        lease_epoch: NetworkLeaseEpoch,
    ) -> Self {
        Self {
            segment_id,
            tenant_id,
            cidr,
            lease_epoch,
        }
    }

    /// Stable allocation identity, independent of the assigned address range.
    pub fn segment_id(&self) -> &NetworkSegmentId {
        &self.segment_id
    }

    /// Tenant attribution carried by this allocation.
    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Assigned address range. This is location, not identity.
    pub fn cidr(&self) -> Cidr {
        self.cidr
    }

    /// Fencing epoch of the node allocation authority that issued this value.
    pub fn lease_epoch(&self) -> NetworkLeaseEpoch {
        self.lease_epoch
    }
}

/// Result of releasing one attachment hold from a tenant's segment allocation.
///
/// The segment value is an associated adapter type supplied by the allocator
/// implementation. This keeps the lifecycle contract portable while allowing
/// an effect-owning adapter to wrap [`AllocatedSegment`] in its own realization
/// handle without making `nimbus-network` depend on that provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkSegmentReleaseOutcome<Segment> {
    /// The last attachment released, so every segment owned by the tenant is
    /// returned for fenced provider cleanup.
    TenantDrained {
        /// All segment realizations that must be cleaned before allocation
        /// authority may make their locations reusable.
        segments: Vec<Segment>,
    },
    /// At least one other attachment still holds the tenant allocation.
    StillLive,
}

/// Result of compare-and-swap-fenced segment growth.
///
/// A placement coordinator first observes the tenant's complete ordered block
/// set and atomically attempts reservation across it. If every block is full,
/// it may grow only while that observation is still current. A concurrent
/// grower or replacement changes the ordered identity set and forces the caller
/// to rescan instead of appending a redundant block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkSegmentGrowth<Segment> {
    /// This caller appended the next segment block.
    Grown(Segment),
    /// Another caller changed the ordered block set after it was observed.
    ///
    /// The caller must fetch the current blocks and retry reservation.
    ObservationStale,
}

/// Portable lifecycle capability for tenant segment allocation.
///
/// The interface owns allocation/hold/release semantics only. `Segment` and
/// `Error` are associated adapter types so an upper effect-owning crate may
/// compose provider-local realization names and domain errors without a
/// reverse dependency. Attachment holds use [`NetworkAttachmentId`], never a
/// sandbox, workload, address, or provider identifier.
///
/// The trait is object-safe once an adapter fixes both associated types. That
/// lets consumers receive an injected capability instead of reaching through
/// to a concrete single-node or future cluster allocator.
pub trait NetworkSegmentAllocator: Send + Sync {
    /// Segment view returned to the consuming adapter.
    type Segment;
    /// Domain error returned to the consuming adapter.
    type Error;

    /// Idempotently read or assign the tenant's primary segment without taking
    /// an attachment hold.
    fn segment_for(&self, tenant: &TenantId) -> Result<Self::Segment, Self::Error>;

    /// Idempotently read or assign the tenant allocation and return every
    /// segment block in deterministic allocation order.
    ///
    /// Placement must attempt reservation across this complete set before it
    /// requests growth.
    fn segments_for(&self, tenant: &TenantId) -> Result<Vec<Self::Segment>, Self::Error>;

    /// Take an idempotent hold for one stable network attachment.
    fn acquire(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
    ) -> Result<Self::Segment, Self::Error>;

    /// Release one stable attachment hold.
    fn release(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
    ) -> Result<NetworkSegmentReleaseOutcome<Self::Segment>, Self::Error>;

    /// Append another segment block only if the caller's complete-set
    /// observation remains current.
    ///
    /// `observed_segments` is the complete ordered [`Self::segments_for`]
    /// result from the immediately preceding atomic reservation attempt. An
    /// implementation compares stable segment identities and fencing context,
    /// not addresses or provider-local names, so remove-and-recreate ABA
    /// replacement cannot masquerade as the same observation.
    fn grow_block_if_current(
        &self,
        tenant: &TenantId,
        observed_segments: &[Self::Segment],
    ) -> Result<NetworkSegmentGrowth<Self::Segment>, Self::Error>;

    /// Reconcile durable holds against the complete live attachment set.
    fn reconcile_orphans(
        &self,
        live: &BTreeSet<(TenantId, NetworkAttachmentId)>,
    ) -> Result<Vec<Self::Segment>, Self::Error>;

    /// Whether allocation requires an externally committed cluster lease.
    fn requires_cluster_lease(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use super::*;

    fn segment_id(value: &str) -> NetworkSegmentId {
        value.parse().expect("fixture segment id should parse")
    }

    #[test]
    fn allocation_preserves_identity_attribution_location_and_epoch() {
        let id = segment_id("netsegment_01ARZ3NDEKTSV4RRFFQ69G5FAV");
        let tenant = TenantId::new("tenant-a").expect("tenant should parse");
        let cidr = Cidr::parse("10.7.0.0/24").expect("CIDR should parse");
        let allocation =
            AllocatedSegment::new(id.clone(), tenant.clone(), cidr, NetworkLeaseEpoch::new(17));

        assert_eq!(allocation.segment_id(), &id);
        assert_eq!(allocation.tenant_id(), &tenant);
        assert_eq!(allocation.cidr(), cidr);
        assert_eq!(allocation.lease_epoch(), NetworkLeaseEpoch::new(17));
    }

    #[test]
    fn address_is_not_segment_identity() {
        let tenant = TenantId::new("tenant-a").expect("tenant should parse");
        let cidr = Cidr::parse("10.7.0.0/24").expect("CIDR should parse");
        let first = AllocatedSegment::new(
            segment_id("netsegment_01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            tenant.clone(),
            cidr,
            NetworkLeaseEpoch::new(1),
        );
        let replacement = AllocatedSegment::new(
            segment_id("netsegment_01ARZ3NDEKTSV4RRFFQ69G5FAW"),
            tenant,
            cidr,
            NetworkLeaseEpoch::new(2),
        );

        assert_eq!(first.cidr(), replacement.cidr());
        assert_ne!(first.segment_id(), replacement.segment_id());
    }

    struct FixedAllocator {
        segment: AllocatedSegment,
    }

    impl NetworkSegmentAllocator for FixedAllocator {
        type Segment = AllocatedSegment;
        type Error = Infallible;

        fn segment_for(&self, _tenant: &TenantId) -> Result<Self::Segment, Self::Error> {
            Ok(self.segment.clone())
        }

        fn segments_for(&self, _tenant: &TenantId) -> Result<Vec<Self::Segment>, Self::Error> {
            Ok(vec![self.segment.clone()])
        }

        fn acquire(
            &self,
            _tenant: &TenantId,
            _attachment_id: &NetworkAttachmentId,
        ) -> Result<Self::Segment, Self::Error> {
            Ok(self.segment.clone())
        }

        fn release(
            &self,
            _tenant: &TenantId,
            _attachment_id: &NetworkAttachmentId,
        ) -> Result<NetworkSegmentReleaseOutcome<Self::Segment>, Self::Error> {
            Ok(NetworkSegmentReleaseOutcome::TenantDrained {
                segments: vec![self.segment.clone()],
            })
        }

        fn grow_block_if_current(
            &self,
            _tenant: &TenantId,
            _observed_segments: &[Self::Segment],
        ) -> Result<NetworkSegmentGrowth<Self::Segment>, Self::Error> {
            Ok(NetworkSegmentGrowth::Grown(self.segment.clone()))
        }

        fn reconcile_orphans(
            &self,
            _live: &BTreeSet<(TenantId, NetworkAttachmentId)>,
        ) -> Result<Vec<Self::Segment>, Self::Error> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn allocator_contract_is_object_safe_with_adapter_owned_types() {
        let tenant = TenantId::new("tenant-a").expect("tenant should parse");
        let attachment = NetworkAttachmentId::generate();
        let allocator: &dyn NetworkSegmentAllocator<Segment = AllocatedSegment, Error = Infallible> =
            &FixedAllocator {
                segment: AllocatedSegment::new(
                    segment_id("netsegment_01ARZ3NDEKTSV4RRFFQ69G5FAV"),
                    tenant.clone(),
                    Cidr::parse("10.7.0.0/24").expect("CIDR should parse"),
                    NetworkLeaseEpoch::new(1),
                ),
            };

        assert_eq!(
            allocator
                .acquire(&tenant, &attachment)
                .expect("infallible allocator")
                .tenant_id(),
            &tenant
        );
        let observed = allocator
            .segments_for(&tenant)
            .expect("infallible allocator");
        assert_eq!(observed.len(), 1);
        assert!(matches!(
            allocator
                .grow_block_if_current(&tenant, &observed)
                .expect("infallible allocator"),
            NetworkSegmentGrowth::Grown(_)
        ));
        assert!(matches!(
            allocator
                .release(&tenant, &attachment)
                .expect("infallible allocator"),
            NetworkSegmentReleaseOutcome::TenantDrained { segments }
                if segments.len() == 1
        ));
    }
}
