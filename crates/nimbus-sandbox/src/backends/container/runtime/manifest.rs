//! Container runtime manifest and launch DTOs.

use std::collections::BTreeMap;
use std::ops::RangeInclusive;
use std::path::PathBuf;

use nimbus_network::{NetworkReservationClaim, PortLeaseRequest};
use serde::{Deserialize, Serialize};

use crate::backends::container::bundle::ContainerBundleLayout;
use crate::backends::oci::buildah::{ImageHealthcheck, MountedRootfsSession, OciExposedPort};
use crate::backends::oci::conmon::{OciConmonLaunchPlan, OciConmonLayout};
use crate::backends::oci::egress::EgressProxyAssignment;
use crate::backends::oci::materializer::MaterializedImageRootfs;
use crate::backends::oci::network::{
    OciMachinePortForwarderConfig, OciNetworkConfig, OciNetworkLayout,
};
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;
use crate::instance::{SandboxHandle, SandboxStatus};
use crate::spec::{SandboxPortBinding, SandboxSpec};

use super::ContainerSandboxBackend;
use super::config::{ContainerSandboxBackendConfig, ContainerStartMode};

mod publication;
pub(super) use publication::reconcile_startup_manifest_publications;
#[cfg(test)]
pub(super) use publication::{
    MANIFEST_PUBLICATION_LOCK_FILE, MANIFEST_PUBLICATION_STAGE_FILE,
    establish_durable_manifest_directory_chain_with, publish_with_directory_sync,
};

impl ContainerSandboxBackend {
    pub(super) fn read_manifest(&self, id: &SandboxId) -> Result<Option<ContainerSandboxManifest>> {
        let Some(manifest_path) =
            crate::artifact_paths::manifest_path_for_sandbox_id(&self.config.state_root, id)
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to find container sandbox manifest for {} under {}: {error}",
                        id,
                        self.config.state_root.display()
                    ),
                })?
        else {
            return Ok(None);
        };
        if !manifest_path.exists() {
            return Ok(None);
        }

        let contents =
            std::fs::read(&manifest_path).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to read sandbox manifest {}: {error}",
                    manifest_path.display()
                ),
            })?;
        let manifest =
            serde_json::from_slice(&contents).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to parse sandbox manifest {}: {error}",
                    manifest_path.display()
                ),
            })?;
        self.validate_manifest_execution_context(&manifest)?;
        Ok(Some(manifest))
    }

    pub(super) fn write_manifest(&self, manifest: &ContainerSandboxManifest) -> Result<()> {
        self.ensure_startup_reconciliation_ready()?;
        self.write_existing_workload_manifest(manifest)
    }

    /// Publish an authenticated lifecycle transition for an already-durable
    /// workload while startup reconciliation fences new admission.
    ///
    /// Callers must first read and authenticate the exact manifest under its
    /// lifecycle lock. Provider launch paths use `write_manifest`, whose
    /// startup gate remains mandatory.
    pub(super) fn write_existing_workload_manifest(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<()> {
        self.validate_manifest_execution_context(manifest)?;
        let mut rendered =
            serde_json::to_vec_pretty(manifest).map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to serialize sandbox manifest: {error}"),
            })?;
        rendered.push(b'\n');
        publication::publish(
            &self.config.state_root,
            &manifest.conmon_layout.container_state_dir,
            &manifest.conmon_layout.manifest_path,
            &rendered,
        )
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "sandbox manifest write failed for {}: {error}",
                manifest.handle.id
            ),
        })?;
        self.reconcile_terminal_ipam_retirement(manifest)
    }

    /// Retry the separately fallible retirement that follows durable terminal
    /// publication.
    ///
    /// A manifest write can commit before its IPAM receipt retirement is
    /// acknowledged. Every terminal replay path calls this seam so same-process
    /// retries converge without misreporting the durable manifest as absent.
    pub(super) fn reconcile_terminal_ipam_retirement(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<()> {
        if manifest.has_terminal_network_finality()
            && let Some(network_config) = manifest.network_config.as_ref()
        {
            crate::backends::oci::network::retire_terminal_container_ipam_release(
                &manifest.network_layout,
                &manifest.handle.id,
                &network_config.reservation_claim,
            )?;
        }
        Ok(())
    }
}

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
    /// Exact placed network config for a launch that owns an attachment.
    ///
    /// `None` is either the authority-free PlanOnly state or the durable
    /// claim-only crash cut before the first attachment reservation. Once
    /// placement succeeds, setup and teardown reuse the identical bridge
    /// without reassigning it.
    pub(super) network_config: Option<OciNetworkConfig>,
    /// Durable finality witness for every network-owned cleanup effect.
    ///
    /// Terminal status alone is only an observed projection. This becomes
    /// `true` after provider absence, listener release, IPAM deallocation, and
    /// segment finalization have all succeeded. Only this marker may authorize
    /// retirement of exact terminal IPAM retry evidence.
    pub(super) network_cleanup_complete: bool,
    /// Durable creator-attempt state for the interval between asynchronous
    /// spawn and exact runtime observation.
    ///
    /// `Pending` is intentionally a safe leak after owner death: runtime
    /// absence alone cannot release network authority while a creator might
    /// still materialize it.
    pub(super) creator_handoff: ContainerCreatorHandoffState,
    /// Canonical operator-requested bindings before image-exposed plan previews
    /// are appended. The runner recomputes the exact automatic suffix from this
    /// input and `image_metadata` before it may reserve any listener.
    pub(super) requested_port_bindings: Vec<SandboxPortBinding>,
    pub(super) port_leases: Vec<PortLeaseRequest>,
    /// Attempt-unique capability for compensating this never-bound launch.
    ///
    /// Cleared after initial provider adoption; restart never receives it.
    #[serde(deserialize_with = "crate::backends::oci::deserialize_required_option")]
    pub(super) launch_reservation_claim: Option<NetworkReservationClaim>,
    pub(super) egress_proxy: Option<EgressProxyAssignment>,
    pub(super) conmon_launch: OciConmonLaunchPlan,
    pub(super) runner_config: ContainerRunnerExecutionConfig,
    pub(super) last_exit_code: Option<i32>,
    #[serde(default)]
    pub(super) restart_count: u32,
    #[serde(default)]
    pub(super) next_restart_at_millis: Option<u64>,
    /// Immutable owner of this manifest's workload lifecycle.
    ///
    /// `start_mode` changes from `PlanOnly` to `Execute` when a prepared
    /// service runner takes effect ownership, so it cannot distinguish that
    /// runner from an ordinary direct Execute backend. This discriminator is
    /// set before the first manifest publication and is covered by the runner
    /// handoff fingerprints.
    pub(super) lifecycle_coordinator: ContainerLifecycleCoordinator,
    pub(super) start_mode: ContainerStartMode,
    pub(super) shutdown_requested: bool,
    pub(super) status: SandboxStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ContainerLifecycleCoordinator {
    DirectBackend,
    PreparedServiceRunner,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "phase")]
pub(super) enum ContainerCreatorHandoffState {
    NotSpawned,
    Pending { attempt_id: String },
    Quiesced { attempt_id: String },
    RuntimeObserved { attempt_id: String },
}

impl ContainerCreatorHandoffState {
    pub(super) fn authorizes_runtime_cleanup(&self) -> bool {
        matches!(
            self,
            Self::NotSpawned | Self::Quiesced { .. } | Self::RuntimeObserved { .. }
        )
    }
}

impl ContainerSandboxManifest {
    /// Whether the durable manifest has reached exact terminal network finality.
    ///
    /// `Stopped` and `Failed` are observed projections, not cleanup authority.
    /// Callers may short-circuit terminal reconciliation only after the
    /// canonical status pair agrees and every durable retry capability has
    /// been retired.
    pub(super) fn has_terminal_network_finality(&self) -> bool {
        self.shutdown_requested
            && matches!(self.status, SandboxStatus::Stopped | SandboxStatus::Failed)
            && self.handle.status == self.status
            && self.network_cleanup_complete
            && self.launch_reservation_claim.is_none()
            && self.launch_artifact.is_none()
            && self.next_restart_at_millis.is_none()
    }

    pub(super) fn require_lifecycle_coordinator(
        &self,
        expected: ContainerLifecycleCoordinator,
        operation: &str,
    ) -> crate::error::Result<()> {
        if self.lifecycle_coordinator == expected {
            return Ok(());
        }
        Err(crate::error::SandboxError::InvalidSpec {
            message: format!(
                "{operation} requires the {expected:?} lifecycle coordinator for {}; found {:?}",
                self.handle.id, self.lifecycle_coordinator
            ),
        })
    }

    pub(super) fn assign_prepared_service_runner(&mut self) -> crate::error::Result<()> {
        if self.start_mode != ContainerStartMode::PlanOnly
            || self.lifecycle_coordinator != ContainerLifecycleCoordinator::DirectBackend
        {
            return Err(crate::error::SandboxError::OperationFailed {
                message: format!(
                    "container workload {} cannot assign its prepared service runner from \
                     {:?}/{:?}",
                    self.handle.id, self.start_mode, self.lifecycle_coordinator
                ),
            });
        }
        self.lifecycle_coordinator = ContainerLifecycleCoordinator::PreparedServiceRunner;
        Ok(())
    }

    pub(super) fn require_network_config(&self) -> crate::error::Result<&OciNetworkConfig> {
        self.network_config
            .as_ref()
            .ok_or_else(|| crate::error::SandboxError::OperationFailed {
                message: format!(
                    "container workload {} has no reserved network attachment configuration",
                    self.handle.id
                ),
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ContainerRunnerExecutionConfig {
    /// Exact node control-state root that owns the serialized port and segment
    /// authority. A runner must never reconstruct this from process defaults.
    pub(super) state_root: PathBuf,
    /// Exact Buildah binary that owns mounted-rootfs cleanup.
    pub(super) buildah_path: PathBuf,
    /// Whether mounted-rootfs cleanup must execute through `buildah unshare`.
    pub(super) use_buildah_unshare: bool,
    pub(super) netavark_path: PathBuf,
    pub(super) aardvark_dns_path: PathBuf,
    pub(super) network_name: String,
    pub(super) network_interface: String,
    pub(super) network_subnet: String,
    pub(super) node_network_supernet: String,
    pub(super) node_tenant_subnet_prefix: u8,
    pub(super) published_port_range: RangeInclusive<u16>,
    pub(super) max_published_ports_per_tenant: Option<usize>,
    pub(super) machine_port_forwarder: Option<OciMachinePortForwarderConfig>,
}

impl ContainerRunnerExecutionConfig {
    pub(super) fn from_backend_config(config: &ContainerSandboxBackendConfig) -> Self {
        Self {
            state_root: config.state_root.clone(),
            buildah_path: config.buildah_path.clone(),
            use_buildah_unshare: config.use_buildah_unshare,
            netavark_path: config.netavark_path.clone(),
            aardvark_dns_path: config.aardvark_dns_path.clone(),
            network_name: config.network_name.clone(),
            network_interface: config.network_interface.clone(),
            network_subnet: config.network_subnet.clone(),
            node_network_supernet: config.node_network_supernet.clone(),
            node_tenant_subnet_prefix: config.node_tenant_subnet_prefix,
            published_port_range: config.published_port_range.clone(),
            max_published_ports_per_tenant: config.max_published_ports_per_tenant,
            machine_port_forwarder: config.machine_port_forwarder.clone(),
        }
    }

    pub(super) fn to_backend_config(&self) -> ContainerSandboxBackendConfig {
        ContainerSandboxBackendConfig {
            state_root: self.state_root.clone(),
            buildah_path: self.buildah_path.clone(),
            use_buildah_unshare: self.use_buildah_unshare,
            netavark_path: self.netavark_path.clone(),
            aardvark_dns_path: self.aardvark_dns_path.clone(),
            network_name: self.network_name.clone(),
            network_interface: self.network_interface.clone(),
            network_subnet: self.network_subnet.clone(),
            node_network_supernet: self.node_network_supernet.clone(),
            node_tenant_subnet_prefix: self.node_tenant_subnet_prefix,
            published_port_range: self.published_port_range.clone(),
            max_published_ports_per_tenant: self.max_published_ports_per_tenant,
            machine_port_forwarder: self.machine_port_forwarder.clone(),
            ..ContainerSandboxBackendConfig::default()
        }
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
