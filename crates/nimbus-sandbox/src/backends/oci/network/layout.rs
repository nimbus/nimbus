//! OCI network layout and configuration.

use std::fs;
use std::net::Ipv4Addr;
use std::path::PathBuf;

use nimbus_core::TenantId;
use serde::{Deserialize, Serialize};

use crate::artifact_paths;
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

use super::ipam::{parse_ipv4_address, parse_ipv4_subnet_and_gateway};
use super::{
    DEFAULT_AARDVARK_DNS_BINARY, DEFAULT_NETAVARK_BINARY, DEFAULT_NETWORK_ID,
    DEFAULT_NETWORK_INTERFACE, DEFAULT_NETWORK_NAME, DEFAULT_NETWORK_SUBNET,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OciNetworkLayout {
    /// Node-local root containing the single network control-plane authority.
    pub state_root: PathBuf,
    /// Typed IPAM partition owner inside that shared authority.
    pub tenant_id: TenantId,
    pub network_root: PathBuf,
    pub run_root: PathBuf,
    pub netns_root: PathBuf,
    pub container_network_dir: PathBuf,
    pub netns_path: PathBuf,
    pub status_path: PathBuf,
}

impl OciNetworkLayout {
    pub(crate) fn new(
        state_root: impl Into<PathBuf>,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
    ) -> Self {
        let state_root = state_root.into();
        let network_root = artifact_paths::tenant_root(&state_root, tenant_id).join("networks");
        let run_root = network_root.join("run");
        let netns_root = network_root.join("netns");
        let container_network_dir = network_root.join("containers").join(sandbox_id.as_str());
        Self {
            state_root,
            tenant_id: tenant_id.clone(),
            status_path: container_network_dir.join("status.json"),
            netns_path: netns_root.join(sandbox_id.as_str()),
            network_root,
            run_root,
            netns_root,
            container_network_dir,
        }
    }

    pub(crate) fn ensure_directories(&self) -> Result<()> {
        for path in [
            &self.run_root,
            &self.netns_root,
            &self.container_network_dir,
        ] {
            fs::create_dir_all(path).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to create OCI network directory {}: {error}",
                    path.display()
                ),
            })?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OciNetworkConfig {
    pub netavark_path: PathBuf,
    pub aardvark_dns_path: PathBuf,
    pub network_name: String,
    pub network_interface: String,
    pub network_subnet: String,
    pub direct_egress: OciNetworkDirectEgress,
    /// Whether netavark starts the in-subnet aardvark-dns stub bound to the
    /// bridge gateway `:53`. The container backend leaves this on so workloads
    /// keep their bridge resolver; the krun microVM backend turns it off
    /// because the deny-by-default guest resolves names through the host PEP
    /// (`HTTP_PROXY`), so the bridge resolver is dead weight and a residual
    /// DNS-exfil channel. Defaults to `true` to preserve container behavior on
    /// (de)serialization of older state.
    #[serde(default = "default_enable_dns")]
    pub enable_dns: bool,
    /// The netavark network id. Per-tenant segments MUST carry a distinct id or
    /// two tenants' bridges alias onto one netavark network (audit M1). Defaults
    /// to the legacy shared id for older state that predates per-tenant segments.
    #[serde(default = "default_network_id")]
    pub network_id: String,
}

pub(super) fn default_enable_dns() -> bool {
    true
}

pub(super) fn default_network_id() -> String {
    DEFAULT_NETWORK_ID.to_owned()
}

impl Default for OciNetworkConfig {
    fn default() -> Self {
        Self {
            netavark_path: PathBuf::from(DEFAULT_NETAVARK_BINARY),
            aardvark_dns_path: PathBuf::from(DEFAULT_AARDVARK_DNS_BINARY),
            network_name: DEFAULT_NETWORK_NAME.to_owned(),
            network_interface: DEFAULT_NETWORK_INTERFACE.to_owned(),
            network_subnet: DEFAULT_NETWORK_SUBNET.to_owned(),
            direct_egress: OciNetworkDirectEgress::Deny,
            enable_dns: default_enable_dns(),
            network_id: default_network_id(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OciNetworkDirectEgress {
    Allow,
    Deny,
}

impl OciNetworkDirectEgress {
    pub(super) fn is_denied(self) -> bool {
        matches!(self, Self::Deny)
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }
}

pub(crate) fn bridge_gateway_addr(config: &OciNetworkConfig) -> Result<Ipv4Addr> {
    let (_, gateway) = parse_ipv4_subnet_and_gateway(&config.network_subnet)?;
    parse_ipv4_address(&gateway)
}
