//! Container backend configuration and defaults.

use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::backends::oci::network::{
    DEFAULT_AARDVARK_DNS_BINARY, DEFAULT_NETAVARK_BINARY, DEFAULT_TENANT_PREFIX,
    OciMachinePortForwarderConfig,
};
use crate::backends::oci::port_manager::DEFAULT_MAX_PORTS_PER_TENANT;
use crate::spec::SandboxResourceQuotaPolicy;

const DEFAULT_RUNTIME_PATH: &str = "crun";
const DEFAULT_CONMON_PATH: &str = "conmon";
const DEFAULT_BUILDAH_PATH: &str = "buildah";
const DEFAULT_PUBLISHED_PORT_START: u16 = 15_000;
const DEFAULT_PUBLISHED_PORT_END: u16 = 16_000;
const DEFAULT_START_TIMEOUT_SECS: u64 = 10;
const DEFAULT_STOP_TIMEOUT_SECS: u64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerStartMode {
    Execute,
    PlanOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerSandboxBackendConfig {
    pub bundle_root: PathBuf,
    pub state_root: PathBuf,
    pub conmon_path: PathBuf,
    pub runtime_path: PathBuf,
    pub buildah_path: PathBuf,
    pub netavark_path: PathBuf,
    pub aardvark_dns_path: PathBuf,
    pub use_buildah_unshare: bool,
    pub published_port_range: RangeInclusive<u16>,
    pub max_published_ports_per_tenant: Option<usize>,
    pub resource_quota_policy: SandboxResourceQuotaPolicy,
    pub network_name: String,
    pub network_interface: String,
    pub network_subnet: String,
    /// The node's network super-net that per-tenant subnets are carved from
    /// (audit M1). Defaults to the node-0 `/16` slice of the cluster pool; the
    /// cluster leg installs a raft-committed slice per node in MTN7.
    pub node_network_supernet: String,
    /// The prefix length of each per-tenant block subnet carved from the
    /// super-net (MTN6). Defaults to `/24` (253 sandboxes/block); a tenant that
    /// exceeds a block grows an additional block bridge on demand. Smaller
    /// prefixes (denser packing) trade address space for more blocks.
    pub node_tenant_subnet_prefix: u8,
    pub machine_port_forwarder: Option<OciMachinePortForwarderConfig>,
    pub start_mode: ContainerStartMode,
    pub log_level: String,
    pub start_timeout: Duration,
    pub stop_timeout: Duration,
}

impl ContainerSandboxBackendConfig {
    pub fn under_root(root: impl Into<PathBuf>) -> Self {
        let mut config = Self::default();
        let root = root.into();
        config.bundle_root = root.join("bundles");
        config.state_root = root.join("state");
        config
    }

    pub fn plan_only(bundle_root: impl Into<PathBuf>, state_root: impl Into<PathBuf>) -> Self {
        Self {
            bundle_root: bundle_root.into(),
            state_root: state_root.into(),
            start_mode: ContainerStartMode::PlanOnly,
            ..Self::default()
        }
    }
}

impl Default for ContainerSandboxBackendConfig {
    fn default() -> Self {
        let temp_root = std::env::temp_dir().join("nimbus-container-sandbox");
        Self {
            bundle_root: temp_root.join("bundles"),
            state_root: temp_root.join("state"),
            conmon_path: PathBuf::from(DEFAULT_CONMON_PATH),
            runtime_path: PathBuf::from(DEFAULT_RUNTIME_PATH),
            buildah_path: PathBuf::from(DEFAULT_BUILDAH_PATH),
            netavark_path: PathBuf::from(DEFAULT_NETAVARK_BINARY),
            aardvark_dns_path: PathBuf::from(DEFAULT_AARDVARK_DNS_BINARY),
            use_buildah_unshare: true,
            published_port_range: DEFAULT_PUBLISHED_PORT_START..=DEFAULT_PUBLISHED_PORT_END,
            max_published_ports_per_tenant: Some(DEFAULT_MAX_PORTS_PER_TENANT),
            resource_quota_policy: SandboxResourceQuotaPolicy::default(),
            network_name: crate::backends::oci::network::DEFAULT_NETWORK_NAME.to_owned(),
            network_interface: crate::backends::oci::network::DEFAULT_NETWORK_INTERFACE.to_owned(),
            network_subnet: crate::backends::oci::network::DEFAULT_NETWORK_SUBNET.to_owned(),
            node_network_supernet: "10.0.0.0/16".to_owned(),
            node_tenant_subnet_prefix: DEFAULT_TENANT_PREFIX,
            machine_port_forwarder: None,
            start_mode: ContainerStartMode::Execute,
            log_level: "debug".to_owned(),
            start_timeout: Duration::from_secs(DEFAULT_START_TIMEOUT_SECS),
            stop_timeout: Duration::from_secs(DEFAULT_STOP_TIMEOUT_SECS),
        }
    }
}
