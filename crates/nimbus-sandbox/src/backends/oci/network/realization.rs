//! OCI provider realization of a portable segment allocation.
//!
//! The control plane owns [`AllocatedSegment`]. This adapter alone turns the
//! allocator's host-local slot into Netavark network, bridge-interface, and
//! display names. Those names are provider handles, never portable identity.

use nimbus_core::Cidr;
use nimbus_network::{AllocatedSegment, NetworkSegmentId};

#[cfg(test)]
use nimbus_core::TenantId;
#[cfg(test)]
use nimbus_network::NetworkLeaseEpoch;

/// A Netavark network identity in the provider's required 64-hex form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NetavarkNetworkId(String);

impl NetavarkNetworkId {
    fn from_local_slot(local_slot: u32) -> Self {
        Self(format!("{local_slot:064x}"))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Sandbox-owned provider handle composed around a portable allocation.
///
/// The local slot is collision-free only inside one allocator authority. That
/// is sufficient for host-local provider names; global identity comes solely
/// from [`AllocatedSegment::segment_id`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OciSegmentRealization {
    allocation: AllocatedSegment,
    network_name: String,
    network_interface: String,
    network_id: NetavarkNetworkId,
}

impl OciSegmentRealization {
    pub(crate) fn from_local_slot(allocation: AllocatedSegment, local_slot: u32) -> Self {
        Self {
            allocation,
            network_name: format!("nimbus-t-{local_slot}"),
            network_interface: format!("nb-{local_slot}"),
            network_id: NetavarkNetworkId::from_local_slot(local_slot),
        }
    }

    pub(crate) fn segment_id(&self) -> &NetworkSegmentId {
        self.allocation.segment_id()
    }

    #[cfg(test)]
    pub(crate) fn tenant_id(&self) -> &TenantId {
        self.allocation.tenant_id()
    }

    pub(crate) fn cidr(&self) -> Cidr {
        self.allocation.cidr()
    }

    #[cfg(test)]
    pub(crate) fn lease_epoch(&self) -> NetworkLeaseEpoch {
        self.allocation.lease_epoch()
    }

    pub(crate) fn network_name(&self) -> &str {
        &self.network_name
    }

    pub(crate) fn network_interface(&self) -> &str {
        &self.network_interface
    }

    pub(crate) fn network_id(&self) -> &NetavarkNetworkId {
        &self.network_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allocation() -> AllocatedSegment {
        AllocatedSegment::new(
            "netsegment_01ARZ3NDEKTSV4RRFFQ69G5FAV"
                .parse()
                .expect("segment id should parse"),
            TenantId::new("tenant-a").expect("tenant should parse"),
            Cidr::parse("10.7.0.0/24").expect("CIDR should parse"),
            NetworkLeaseEpoch::new(9),
        )
    }

    #[test]
    fn realization_keeps_provider_names_outside_the_portable_allocation() {
        let realization = OciSegmentRealization::from_local_slot(allocation(), 42);

        assert_eq!(
            realization.segment_id().as_str(),
            "netsegment_01ARZ3NDEKTSV4RRFFQ69G5FAV"
        );
        assert_eq!(realization.tenant_id().as_str(), "tenant-a");
        assert_eq!(realization.cidr().to_string(), "10.7.0.0/24");
        assert_eq!(realization.lease_epoch(), NetworkLeaseEpoch::new(9));
        assert_eq!(realization.network_name(), "nimbus-t-42");
        assert_eq!(realization.network_interface(), "nb-42");
        assert_eq!(
            realization.network_id().as_str(),
            "000000000000000000000000000000000000000000000000000000000000002a"
        );
    }

    #[test]
    fn largest_local_slot_stays_within_ifnamsiz() {
        let realization = OciSegmentRealization::from_local_slot(allocation(), u32::MAX);

        assert!(
            realization.network_interface().len() <= 15,
            "provider interface {:?} must fit IFNAMSIZ",
            realization.network_interface()
        );
    }
}
