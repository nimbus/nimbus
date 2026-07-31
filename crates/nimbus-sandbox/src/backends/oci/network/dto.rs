//! Serialized DTOs exchanged with netavark, gvproxy, and IPAM state.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkAttachmentId, NetworkProviderHandle, NetworkReservationClaim, NetworkResourceGeneration,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::provider_locator::OciAttachmentProviderLocator;

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

/// Attempt-bound observed projection written only after one exact Netavark
/// setup effect returns.
///
/// The provider response itself does not carry Nimbus attachment identity.
/// This strict envelope prevents syntactically valid or cross-attempt JSON
/// from becoming current readiness evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NetavarkStatusProjection {
    pub(super) schema_version: u32,
    pub(super) tenant_id: TenantId,
    pub(super) attachment_id: NetworkAttachmentId,
    pub(super) setup_attempt: NetworkProviderHandle,
    pub(super) assigned_ips: Vec<Ipv4Addr>,
    pub(super) response: Value,
}

impl NetavarkStatusProjection {
    pub(super) const SCHEMA_VERSION: u32 = 1;
}

#[derive(Debug, Serialize)]
pub(super) struct MachinePortForwardRequest {
    /// Exact provider incarnation configured by the gvproxy lifecycle owner.
    pub(super) provider_instance: NetworkProviderHandle,
    /// Monotonic generation of that provider incarnation.
    pub(super) provider_generation: NetworkResourceGeneration,
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub(super) struct IpamAllocation {
    pub(super) segment_id: String,
    pub(super) reservation_claim: NetworkReservationClaim,
    pub(super) provider_locator: OciAttachmentProviderLocator,
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
pub(in crate::backends::oci::network) enum NetavarkProviderOperation {
    /// No Netavark effect has begun for this IPAM generation.
    Reserved,
    /// One exact setup attempt is durable and no provider effect is authorized
    /// yet. A fresh owner may adopt this attempt, but cannot mint a sibling.
    SetupPrepared {
        operation_attempt: NetworkProviderHandle,
    },
    /// The exact setup attempt crossed its final pre-effect fence. A process
    /// loss from this phase is ambiguous and must never rerun setup blindly.
    Provisioning {
        operation_attempt: NetworkProviderHandle,
    },
    /// Setup and its observed status projection completed.
    Ready {
        setup_attempt: NetworkProviderHandle,
    },
    /// One exact teardown attempt is durable and no provider delete effect is
    /// authorized yet. A fresh owner may adopt this attempt.
    TeardownPrepared {
        /// Exact setup generation whose provider effects will be removed.
        setup_attempt: NetworkProviderHandle,
        operation_attempt: NetworkProviderHandle,
    },
    /// Cleanup is durable for a setup attempt that never crossed its provider
    /// pre-effect fence. Namespace and listener compensation may proceed, but
    /// Netavark delete must never be invoked.
    NoEffectTeardownPrepared {
        setup_attempt: NetworkProviderHandle,
        operation_attempt: NetworkProviderHandle,
    },
    /// The exact teardown attempt crossed its final pre-effect fence. A process
    /// loss from this phase requires inspection before any further delete.
    Deleting {
        /// Exact setup generation whose provider effects are being removed.
        setup_attempt: NetworkProviderHandle,
        operation_attempt: NetworkProviderHandle,
    },
    /// Provider absence is confirmed, but removal of observed status is pending.
    DetachedProjectionPending {
        /// Exact setup generation whose provider effects were removed.
        setup_attempt: NetworkProviderHandle,
        operation_attempt: NetworkProviderHandle,
    },
    /// Provider absence and observed-status removal are both confirmed.
    Detached,
}

impl NetavarkProviderOperation {
    pub(super) const fn permits_terminal_ipam_release(&self) -> bool {
        matches!(self, Self::Reserved | Self::Detached)
    }

    pub(super) const fn label(&self) -> &'static str {
        match self {
            Self::Reserved => "reserved",
            Self::SetupPrepared { .. } => "setup_prepared",
            Self::Provisioning { .. } => "provisioning",
            Self::Ready { .. } => "ready",
            Self::TeardownPrepared { .. } => "teardown_prepared",
            Self::NoEffectTeardownPrepared { .. } => "no_effect_teardown_prepared",
            Self::Deleting { .. } => "deleting",
            Self::DetachedProjectionPending { .. } => "detached_projection_pending",
            Self::Detached => "detached",
        }
    }
}
