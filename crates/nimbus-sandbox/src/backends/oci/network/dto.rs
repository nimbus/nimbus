//! Serialized DTOs exchanged with netavark, gvproxy, and IPAM state.

use std::collections::BTreeMap;

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
    pub(super) allocations: BTreeMap<String, Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) last_assigned_ip: Option<String>,
}
