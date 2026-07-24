use nimbus_core::{Cidr, TenantId};

use crate::{NetworkLeaseEpoch, NetworkSegmentId};

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

#[cfg(test)]
mod tests {
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
}
