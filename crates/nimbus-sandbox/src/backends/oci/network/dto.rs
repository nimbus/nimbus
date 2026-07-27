//! Serialized DTOs exchanged with netavark, gvproxy, and IPAM state.

use std::collections::BTreeMap;

use nimbus_network::{NetworkProviderHandle, NetworkReservationClaim};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub(super) struct NetavarkRequest {
    pub(super) container_id: String,
    pub(super) container_name: String,
    pub(super) port_mappings: Vec<NetavarkPortMapping>,
    pub(super) networks: BTreeMap<String, NetavarkPerNetworkOptions>,
    pub(super) dns_servers: Vec<String>,
    pub(super) container_hostname: String,
    pub(super) network_info: BTreeMap<String, NetavarkNetwork>,
}

#[derive(Debug, Serialize)]
pub(super) struct NetavarkPortMapping {
    pub(super) host_ip: String,
    pub(super) container_port: u16,
    pub(super) host_port: u16,
    pub(super) range: u16,
    pub(super) protocol: String,
}

#[derive(Debug, Serialize)]
pub(super) struct NetavarkPerNetworkOptions {
    pub(super) interface_name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) static_ips: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct NetavarkNetwork {
    pub(super) name: String,
    pub(super) id: String,
    pub(super) driver: String,
    pub(super) network_interface: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) created: Option<String>,
    pub(super) subnets: Vec<NetavarkSubnet>,
    pub(super) ipv6_enabled: bool,
    pub(super) internal: bool,
    pub(super) dns_enabled: bool,
    pub(super) network_dns_servers: Vec<String>,
    pub(super) labels: BTreeMap<String, String>,
    pub(super) options: BTreeMap<String, String>,
    pub(super) ipam_options: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
pub(super) struct NetavarkSubnet {
    pub(super) subnet: String,
    pub(super) gateway: String,
}

#[derive(Debug, Serialize)]
pub(super) struct MachinePortForwardRequest {
    pub(super) local: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) remote: Option<String>,
    pub(super) protocol: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct NetavarkErrorResponse {
    pub(super) error: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct IpamState {
    pub(super) allocations: BTreeMap<String, IpamAllocation>,
    /// Last terminal generation for an attachment whose provider detach was
    /// confirmed.
    ///
    /// The tombstone is overwritten atomically by the next live allocation.
    /// Retaining it closes the otherwise unauthenticated gap between final
    /// IPAM release and idempotent cleanup replay.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) released_allocations: BTreeMap<String, IpamAllocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) last_assigned_ip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct IpamAllocation {
    pub(super) segment_id: String,
    pub(super) reservation_claim: NetworkReservationClaim,
    pub(super) ips: Vec<String>,
    pub(super) provider_operation: NetavarkProviderOperation,
}

/// Durable Netavark effect ownership for one exact IPAM generation.
///
/// Pending attempts are authority, not observed status. They deliberately
/// survive process death so a successor cannot rerun an ambiguous effect or
/// replace the attachment before evidence-aware reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub(super) enum NetavarkProviderOperation {
    /// No Netavark effect has begun for this IPAM generation.
    Reserved,
    /// Setup may be in flight or may have completed without acknowledgement.
    Provisioning {
        operation_attempt: NetworkProviderHandle,
    },
    /// Setup and its observed status projection completed.
    Ready {
        setup_attempt: NetworkProviderHandle,
    },
    /// Teardown may be in flight or may have completed without acknowledgement.
    Deleting {
        operation_attempt: NetworkProviderHandle,
    },
    /// Provider absence is confirmed, but removal of observed status is pending.
    DetachedProjectionPending {
        operation_attempt: NetworkProviderHandle,
    },
    /// Provider absence and observed-status removal are both confirmed.
    Detached,
}

impl NetavarkProviderOperation {
    pub(super) const fn label(&self) -> &'static str {
        match self {
            Self::Reserved => "reserved",
            Self::Provisioning { .. } => "provisioning",
            Self::Ready { .. } => "ready",
            Self::Deleting { .. } => "deleting",
            Self::DetachedProjectionPending { .. } => "detached_projection_pending",
            Self::Detached => "detached",
        }
    }
}
