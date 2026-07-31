//! OCI network layout and configuration.

use std::fs;
use std::net::Ipv4Addr;
use std::path::PathBuf;

use nimbus_core::TenantId;
use nimbus_network::NetworkReservationClaim;
#[cfg(test)]
use nimbus_network::NetworkSegmentId;
use serde::{Deserialize, Serialize};

use crate::artifact_paths;
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

use super::DEFAULT_NETWORK_ID;
use super::ipam::{parse_ipv4_address, parse_ipv4_subnet_and_gateway};
use super::provider_locator::OciAttachmentProviderKind;
#[cfg(test)]
use super::{
    DEFAULT_AARDVARK_DNS_BINARY, DEFAULT_NETAVARK_BINARY, DEFAULT_NETWORK_INTERFACE,
    DEFAULT_NETWORK_NAME, DEFAULT_NETWORK_SUBNET,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OciNetworkLayout {
    /// Backend-local root containing manifests and provider artifacts.
    pub workload_state_root: PathBuf,
    /// Node-local root containing the single network control-plane authority.
    pub network_state_root: PathBuf,
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
    /// Deterministic test layout whose workload and network roots match.
    #[cfg(test)]
    pub(crate) fn under_root(
        state_root: impl Into<PathBuf>,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
    ) -> Self {
        let state_root = state_root.into();
        Self::with_roots(state_root.clone(), state_root, tenant_id, sandbox_id)
    }

    /// Layout with explicit backend-local artifacts and node network authority.
    pub(crate) fn with_roots(
        workload_state_root: impl Into<PathBuf>,
        network_state_root: impl Into<PathBuf>,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
    ) -> Self {
        let workload_state_root = workload_state_root.into();
        let network_root =
            artifact_paths::tenant_root(&workload_state_root, tenant_id).join("networks");
        let run_root = network_root.join("run");
        let netns_root = network_root.join("netns");
        let container_network_dir = network_root.join("containers").join(sandbox_id.as_str());
        Self {
            workload_state_root,
            network_state_root: network_state_root.into(),
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
    /// Stable control-plane segment identity selected for this attachment.
    ///
    /// This is distinct from `network_id`, which is only Netavark's
    /// provider-local network handle.
    pub segment_id: String,
    /// Immutable attachment-generation witness for provider operations.
    ///
    /// This is comparison evidence, not generic cleanup authority. It remains
    /// in the per-attachment manifest after provider adoption so stale setup,
    /// teardown, projection, and confirmed-detach work cannot target a
    /// replacement generation that reuses the same sandbox and segment IDs.
    pub reservation_claim: NetworkReservationClaim,
    /// Sandbox-owned provider family persisted into the IPAM evidence locator.
    pub(super) provider_kind: OciAttachmentProviderKind,
    pub direct_egress: OciNetworkDirectEgress,
    /// Whether netavark starts the in-subnet aardvark-dns stub bound to the
    /// bridge gateway `:53`. Both production host-managed backends disable it:
    /// workloads resolve names through the host PEP, so the bridge resolver is
    /// unreachable dead weight and a residual DNS-exfiltration channel. The
    /// serde default only serves test fixtures and rejects missing production
    /// launch identity elsewhere in the attachment contract.
    #[serde(default = "default_enable_dns")]
    pub enable_dns: bool,
    /// The netavark network id. Per-tenant segments MUST carry a distinct id or
    /// two tenants' bridges alias onto one netavark network (audit M1). Direct
    /// construction gets a deterministic placeholder; placement replaces it
    /// with the selected segment's provider-local identity.
    #[serde(default = "default_network_id")]
    pub network_id: String,
}

impl OciNetworkConfig {
    pub(crate) const fn provider_kind(&self) -> OciAttachmentProviderKind {
        self.provider_kind
    }

    #[cfg(test)]
    pub(crate) fn provider_kind_label(&self) -> &'static str {
        match self.provider_kind {
            OciAttachmentProviderKind::Container => "container",
            OciAttachmentProviderKind::Krun => "krun",
        }
    }
}

pub(super) fn default_enable_dns() -> bool {
    true
}

pub(super) fn default_network_id() -> String {
    DEFAULT_NETWORK_ID.to_owned()
}

#[cfg(test)]
impl Default for OciNetworkConfig {
    fn default() -> Self {
        Self {
            netavark_path: PathBuf::from(DEFAULT_NETAVARK_BINARY),
            aardvark_dns_path: PathBuf::from(DEFAULT_AARDVARK_DNS_BINARY),
            network_name: DEFAULT_NETWORK_NAME.to_owned(),
            network_interface: DEFAULT_NETWORK_INTERFACE.to_owned(),
            network_subnet: DEFAULT_NETWORK_SUBNET.to_owned(),
            segment_id: NetworkSegmentId::generate().as_str().to_owned(),
            reservation_claim: crate::backends::oci::port_lease::new_launch_reservation_claim()
                .expect("test network config claim should validate"),
            provider_kind: OciAttachmentProviderKind::Container,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_config_rejects_a_missing_stable_segment_identity() {
        let config = OciNetworkConfig::default();
        let mut value = serde_json::to_value(&config).expect("network config should serialize");
        value
            .as_object_mut()
            .expect("serialized config should be an object")
            .remove("segment_id");

        let error = serde_json::from_value::<OciNetworkConfig>(value)
            .expect_err("missing segment identity must fail closed");
        assert!(
            error.to_string().contains("segment_id"),
            "deserialization failure must name the required identity: {error}"
        );
    }

    #[test]
    fn network_config_rejects_a_missing_attachment_generation_witness() {
        let config = OciNetworkConfig::default();
        let mut value = serde_json::to_value(&config).expect("network config should serialize");
        value
            .as_object_mut()
            .expect("serialized config should be an object")
            .remove("reservation_claim");

        let error = serde_json::from_value::<OciNetworkConfig>(value)
            .expect_err("missing attachment generation must fail closed");
        assert!(
            error.to_string().contains("reservation_claim"),
            "deserialization failure must name the required generation witness: {error}"
        );
    }

    #[test]
    fn test_defaults_generate_distinct_valid_segment_identities() {
        let first = OciNetworkConfig::default()
            .segment_id
            .parse::<NetworkSegmentId>()
            .expect("first test identity should validate");
        let second = OciNetworkConfig::default()
            .segment_id
            .parse::<NetworkSegmentId>()
            .expect("second test identity should validate");

        assert_ne!(
            first, second,
            "test defaults must not collapse unrelated attachments onto a shared identity"
        );
    }
}
