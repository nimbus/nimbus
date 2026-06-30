//! Container runtime manifest and launch DTOs.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::backends::container::bundle::ContainerBundleLayout;
use crate::backends::oci::buildah::{ImageHealthcheck, MountedRootfsSession, OciExposedPort};
use crate::backends::oci::conmon::{OciConmonLaunchPlan, OciConmonLayout};
use crate::backends::oci::egress::EgressProxyAssignment;
use crate::backends::oci::materializer::MaterializedImageRootfs;
use crate::backends::oci::network::{OciMachinePortForwarderConfig, OciNetworkLayout};
use crate::instance::{SandboxHandle, SandboxStatus};
use crate::spec::SandboxSpec;

use super::config::{ContainerSandboxBackendConfig, ContainerStartMode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContainerStartPlan {
    pub(super) manifest: ContainerSandboxManifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ContainerSandboxManifest {
    pub(super) handle: SandboxHandle,
    pub(super) spec: SandboxSpec,
    pub(super) image_metadata: ContainerImageMetadata,
    pub(super) launch_artifact: Option<ContainerLaunchArtifact>,
    pub(super) bundle_layout: ContainerBundleLayout,
    pub(super) conmon_layout: OciConmonLayout,
    pub(super) network_layout: OciNetworkLayout,
    pub(super) egress_proxy: Option<EgressProxyAssignment>,
    pub(super) conmon_launch: OciConmonLaunchPlan,
    #[serde(default)]
    pub(super) runner_config: ContainerRunnerExecutionConfig,
    pub(super) last_exit_code: Option<i32>,
    #[serde(default)]
    pub(super) restart_count: u32,
    #[serde(default)]
    pub(super) next_restart_at_millis: Option<u64>,
    pub(super) start_mode: ContainerStartMode,
    pub(super) shutdown_requested: bool,
    pub(super) status: SandboxStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ContainerRunnerExecutionConfig {
    pub(super) netavark_path: PathBuf,
    pub(super) aardvark_dns_path: PathBuf,
    pub(super) network_name: String,
    pub(super) network_interface: String,
    pub(super) network_subnet: String,
    pub(super) machine_port_forwarder: Option<OciMachinePortForwarderConfig>,
}

impl ContainerRunnerExecutionConfig {
    pub(super) fn from_backend_config(config: &ContainerSandboxBackendConfig) -> Self {
        Self {
            netavark_path: config.netavark_path.clone(),
            aardvark_dns_path: config.aardvark_dns_path.clone(),
            network_name: config.network_name.clone(),
            network_interface: config.network_interface.clone(),
            network_subnet: config.network_subnet.clone(),
            machine_port_forwarder: config.machine_port_forwarder.clone(),
        }
    }

    pub(super) fn to_backend_config(&self) -> ContainerSandboxBackendConfig {
        ContainerSandboxBackendConfig {
            netavark_path: self.netavark_path.clone(),
            aardvark_dns_path: self.aardvark_dns_path.clone(),
            network_name: self.network_name.clone(),
            network_interface: self.network_interface.clone(),
            network_subnet: self.network_subnet.clone(),
            machine_port_forwarder: self.machine_port_forwarder.clone(),
            ..ContainerSandboxBackendConfig::default()
        }
    }
}

impl Default for ContainerRunnerExecutionConfig {
    fn default() -> Self {
        Self::from_backend_config(&ContainerSandboxBackendConfig::default())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ContainerResolvedLaunchSpec {
    pub(super) spec: SandboxSpec,
    pub(super) image_metadata: ContainerImageMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum ContainerLaunchArtifact {
    MountedRootfs(MountedRootfsSession),
    Rootfs(MaterializedImageRootfs),
}

impl ContainerLaunchArtifact {
    pub(super) fn mount_session_name(&self) -> Option<&str> {
        match self {
            Self::MountedRootfs(session) => Some(session.session_name.as_str()),
            Self::Rootfs(_) => None,
        }
    }

    pub(super) fn uses_mount_session_unshare(&self) -> bool {
        matches!(self, Self::MountedRootfs(_))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct ContainerImageMetadata {
    pub(super) user: Option<String>,
    pub(super) stop_signal: Option<String>,
    pub(super) healthcheck: Option<ImageHealthcheck>,
    pub(super) labels: BTreeMap<String, String>,
    pub(super) exposed_ports: Vec<OciExposedPort>,
}
