//! Container runtime manifest and launch DTOs.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;
use std::path::PathBuf;

use nimbus_network::{NetworkReservationClaim, PortLeaseRequest};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::backends::conmon::creator::{CreatorAttemptReceipt, CreatorQuiescenceProof};
use crate::backends::container::bundle::ContainerBundleLayout;
use crate::backends::oci::buildah::{ImageHealthcheck, MountedRootfsSession, OciExposedPort};
use crate::backends::oci::conmon::{OciConmonLaunchPlan, OciConmonLayout};
use crate::backends::oci::egress::{EgressPolicyReloadState, EgressProxyAssignment};
use crate::backends::oci::materializer::MaterializedImageRootfs;
use crate::backends::oci::network::{
    OciMachinePortForwarderConfig, OciNetworkConfig, OciNetworkLayout, TerminalNetworkAuthoritySet,
    TerminalNetworkFinalityEvidence,
};
use crate::error::{Result, SandboxError};
use crate::execution_attempt::{SandboxExecutionAttemptId, SandboxRestartAttemptFence};
use crate::instance::SandboxId;
use crate::instance::{SandboxHandle, SandboxStatus};
use crate::provision::SandboxProvisionNetworkPlan;
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

/// Read and authenticate the one manifest shape that may exist before any
/// network authority is durable: an exact compiler plan plus reservation
/// claim, with no placed attachment, listener lease, bundle, or provider
/// effect. Startup network reconciliation may retain only these paths.
pub(super) fn retained_reservation_pending_manifest_paths(
    config: &ContainerSandboxBackendConfig,
) -> Result<BTreeSet<PathBuf>> {
    let container_state_dirs = crate::artifact_paths::all_container_state_dirs(
        &config.workload_state_root,
    )
    .map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to enumerate reservation-pending manifests under {}: {error}",
            config.workload_state_root.display()
        ),
    })?;
    let mut retained = BTreeSet::new();
    for state_dir in container_state_dirs {
        let path = state_dir.join("manifest.json");
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "failed to read startup container manifest {}: {error}",
                        path.display()
                    ),
                });
            }
        };
        let manifest: ContainerSandboxManifest =
            serde_json::from_slice(&bytes).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "startup container manifest {} is structurally untrusted: {error}",
                    path.display()
                ),
            })?;
        super::provider_context::validate_manifest_execution_context_for_config(config, &manifest)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "startup container manifest {} crossed its execution context: {error}",
                    path.display()
                ),
            })?;

        let (Some(plan), Some(_claim)) = (
            manifest.provision_network_plan.as_ref(),
            manifest.launch_reservation_claim.as_ref(),
        ) else {
            continue;
        };
        if manifest.provision_prepared || manifest.network_config.is_some() {
            continue;
        }
        let exact_claim_only_shape = manifest.launch_artifact.is_none()
            && manifest.egress_proxy.is_none()
            && manifest.port_leases.is_empty()
            && manifest.creator_handoff == ContainerCreatorHandoffState::NotSpawned
            && manifest.restart_transition.is_none()
            && manifest.runner_handoff_id.is_none()
            && !manifest.network_cleanup_complete
            && !manifest.shutdown_requested
            && manifest.status == SandboxStatus::Starting
            && manifest.handle.status == SandboxStatus::Starting
            && manifest.lifecycle_coordinator == ContainerLifecycleCoordinator::DirectBackend
            && manifest.start_mode == config.start_mode
            && plan.tenant_id() == &manifest.spec.tenant_id
            && plan.generation() == plan.network_plan().generation()
            && plan.bindings() == manifest.spec.port_bindings
            && manifest.conmon_layout.manifest_path == path
            && !manifest.bundle_layout.config_path.exists()
            && !manifest.network_layout.netns_path.exists()
            && !manifest.network_layout.status_path.exists();
        if !exact_claim_only_shape {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "startup container manifest {} carries a crossed or effect-bearing reservation-pending shape",
                    path.display()
                ),
            });
        }
        retained.insert(path);
    }
    Ok(retained)
}

impl ContainerSandboxBackend {
    pub(super) fn read_manifest(&self, id: &SandboxId) -> Result<Option<ContainerSandboxManifest>> {
        let Some(manifest_path) = crate::artifact_paths::manifest_path_for_sandbox_id(
            &self.config.workload_state_root,
            id,
        )
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to find container sandbox manifest for {} under {}: {error}",
                id,
                self.config.workload_state_root.display()
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
        super::runner::ensure_runner_handoff_lock_artifact(manifest)?;
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
        if matches!(
            manifest.status,
            SandboxStatus::Stopped | SandboxStatus::Failed
        ) {
            if !manifest.has_terminal_network_finality() {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "refusing terminal container manifest publication for {} while local \
                         launch or cleanup authority remains: shutdown_requested={}, \
                         status={:?}, handle_status={:?}, network_cleanup_complete={}, \
                         launch_reservation_claim_present={}, launch_artifact_present={}",
                        manifest.handle.id,
                        manifest.shutdown_requested,
                        manifest.status,
                        manifest.handle.status,
                        manifest.network_cleanup_complete,
                        manifest.launch_reservation_claim.is_some(),
                        manifest.launch_artifact.is_some(),
                    ),
                });
            }
            TerminalNetworkAuthoritySet::new(
                self.segment_allocator.as_ref(),
                &self.ipam_authority,
                self.port_lease_coordinator.authority()?,
                TerminalNetworkFinalityEvidence::new(
                    &manifest.spec.tenant_id,
                    &manifest.handle.id,
                    &manifest.network_layout,
                    manifest.network_config.as_ref(),
                    &manifest.port_leases,
                    manifest
                        .egress_proxy
                        .as_ref()
                        .map(|assignment| &assignment.port_lease),
                ),
            )
            .require_released()?;
        }
        let mut rendered =
            serde_json::to_vec_pretty(manifest).map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to serialize sandbox manifest: {error}"),
            })?;
        rendered.push(b'\n');
        publication::publish(
            &self.config.workload_state_root,
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
                &self.ipam_authority,
                &manifest.network_layout,
                &manifest.handle.id,
                &network_config.attachment_id,
                &network_config.reservation_claim,
                network_config.provider_kind(),
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
    /// Exact workload execution incarnation persisted before provider effects.
    pub(super) execution_attempt_id: SandboxExecutionAttemptId,
    pub(super) spec: SandboxSpec,
    /// Durable phase marker separating reservation from workload artifact
    /// materialization. Legacy coarse starts publish this as `true`.
    pub(super) provision_prepared: bool,
    pub(super) image_metadata: ContainerImageMetadata,
    pub(super) launch_artifact: Option<ContainerLaunchArtifact>,
    pub(super) bundle_layout: ContainerBundleLayout,
    pub(super) conmon_layout: OciConmonLayout,
    pub(super) network_layout: OciNetworkLayout,
    /// Complete compiler-issued desired network plan persisted before any
    /// reservation effect. A claim-only crash can resume only when the caller
    /// presents this exact plan again.
    pub(super) provision_network_plan: Option<SandboxProvisionNetworkPlan>,
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
    /// Exact restart fence and provider-local phase retained across process
    /// death. Absence means this workload has not entered a restart command.
    ///
    /// The optional shape is intentional: ordinary initial execution has no
    /// restart authority. Once present, every transition authenticates the
    /// complete source/target/ordinal fence before it may touch provider state.
    #[serde(default)]
    pub(super) restart_transition: Option<ContainerRestartTransition>,
    /// Exact generation of the durable Execute handoff that owns provider
    /// effects for this manifest.
    ///
    /// PlanOnly manifests carry `None`. The winning Execute decision mints and
    /// publishes this identity before `EffectsStarted`, so a substituted
    /// decision record cannot authenticate the manifest merely by recomputing
    /// its other fingerprints.
    #[serde(deserialize_with = "crate::backends::oci::deserialize_required_option")]
    pub(super) runner_handoff_id: Option<RunnerHandoffId>,
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
    /// Monotonic desired policy and provider-attempt generations.
    ///
    /// `Applying` is published before the live PEP is touched and becomes
    /// `Stable` only after exact provider inspection authenticates the same
    /// attempt. It therefore survives acknowledgement loss without treating
    /// the return path as durable truth.
    pub(super) egress_policy_reload: EgressPolicyReloadState,
    pub(super) conmon_launch: OciConmonLaunchPlan,
    pub(super) runner_config: ContainerRunnerExecutionConfig,
    pub(super) last_exit_code: Option<i32>,
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
#[serde(try_from = "String", into = "String")]
pub(super) struct RunnerHandoffId(String);

impl RunnerHandoffId {
    pub(super) fn mint() -> Self {
        Self(Ulid::new().to_string().to_ascii_lowercase())
    }
}

impl TryFrom<String> for RunnerHandoffId {
    type Error = String;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        let parsed = Ulid::from_string(&value)
            .map_err(|error| format!("runner handoff ID must be a valid ULID: {error}"))?;
        let canonical = parsed.to_string().to_ascii_lowercase();
        if value != canonical {
            return Err(format!(
                "runner handoff ID must use canonical lowercase ULID form {canonical}"
            ));
        }
        Ok(Self(value))
    }
}

impl From<RunnerHandoffId> for String {
    fn from(value: RunnerHandoffId) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "phase")]
pub(super) enum ContainerCreatorHandoffState {
    NotSpawned,
    SpawnIntent { attempt_id: String },
    Pending { receipt: CreatorAttemptReceipt },
    Quiesced { proof: CreatorQuiescenceProof },
    RuntimeObserved { receipt: CreatorAttemptReceipt },
}

/// Durable provider-local progress for one exact coordinator-issued restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "phase", deny_unknown_fields)]
pub(super) enum ContainerRestartTransition {
    /// The source runtime and its creator are proven absent. The source
    /// execution attempt remains the manifest owner at this phase.
    SourceQuiesced {
        fence: SandboxRestartAttemptFence,
        creator_quiescence: CreatorQuiescenceProof,
    },
    /// The target attempt owns the manifest, but stale source receipts may
    /// still need idempotent removal before activation is admitted.
    TargetPreparing {
        fence: SandboxRestartAttemptFence,
        creator_quiescence: CreatorQuiescenceProof,
    },
    /// The target attempt is durable and all source runtime receipts are
    /// retired. No target creator has been launched by this phase.
    TargetPrepared {
        fence: SandboxRestartAttemptFence,
        creator_quiescence: CreatorQuiescenceProof,
    },
    /// The target reuses the exact retained private attachment and PEP. This
    /// does not imply ingress publication.
    RetainedNetworkAttached {
        fence: SandboxRestartAttemptFence,
        creator_quiescence: CreatorQuiescenceProof,
    },
}

impl ContainerRestartTransition {
    pub(super) fn fence(&self) -> &SandboxRestartAttemptFence {
        match self {
            Self::SourceQuiesced { fence, .. }
            | Self::TargetPreparing { fence, .. }
            | Self::TargetPrepared { fence, .. }
            | Self::RetainedNetworkAttached { fence, .. } => fence,
        }
    }

    pub(super) fn creator_quiescence(&self) -> &CreatorQuiescenceProof {
        match self {
            Self::SourceQuiesced {
                creator_quiescence, ..
            }
            | Self::TargetPreparing {
                creator_quiescence, ..
            }
            | Self::TargetPrepared {
                creator_quiescence, ..
            }
            | Self::RetainedNetworkAttached {
                creator_quiescence, ..
            } => creator_quiescence,
        }
    }

    pub(super) fn target_is_prepared(&self) -> bool {
        matches!(
            self,
            Self::TargetPrepared { .. } | Self::RetainedNetworkAttached { .. }
        )
    }
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
    pub(super) fn require_execution_attempt(
        &self,
        expected: &SandboxExecutionAttemptId,
        operation: &str,
    ) -> crate::error::Result<()> {
        if &self.execution_attempt_id == expected {
            return Ok(());
        }
        Err(crate::error::SandboxError::InvalidSpec {
            message: format!(
                "{operation} for {} crossed execution attempt {}; durable attempt is {}",
                self.handle.id, expected, self.execution_attempt_id
            ),
        })
    }

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
#[serde(rename_all = "snake_case")]
pub(super) enum ContainerNetworkPublicationMode {
    HostManaged,
    MachineForwarded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ContainerRunnerExecutionConfig {
    /// Exact backend-local root that owns workload manifests and provider
    /// artifacts. A runner must never reconstruct this from process defaults.
    pub(super) workload_state_root: PathBuf,
    /// Exact node-local root that owns serialized segment, IPAM, and port
    /// authority. A runner must never reconstruct this from workload state.
    pub(super) network_state_root: PathBuf,
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
    /// Immutable desired publication mode, separate from provider authority.
    ///
    /// A missing or corrupted machine forwarder must fail closed rather than
    /// silently reinterpret the attachment as host-managed. This is required
    /// even when the desired publication set is empty.
    pub(super) network_publication_mode: ContainerNetworkPublicationMode,
    pub(super) machine_port_forwarder: Option<OciMachinePortForwarderConfig>,
}

impl ContainerRunnerExecutionConfig {
    pub(super) fn from_backend_config(config: &ContainerSandboxBackendConfig) -> Self {
        Self {
            workload_state_root: config.workload_state_root.clone(),
            network_state_root: config.network_state_root.clone(),
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
            network_publication_mode: if config.machine_port_forwarder.is_some() {
                ContainerNetworkPublicationMode::MachineForwarded
            } else {
                ContainerNetworkPublicationMode::HostManaged
            },
            machine_port_forwarder: config.machine_port_forwarder.clone(),
        }
    }

    pub(super) fn validated_machine_port_forwarder(
        &self,
        sandbox_id: &crate::instance::SandboxId,
    ) -> crate::error::Result<Option<&OciMachinePortForwarderConfig>> {
        match (
            &self.network_publication_mode,
            self.machine_port_forwarder.as_ref(),
        ) {
            (ContainerNetworkPublicationMode::HostManaged, None) => Ok(None),
            (ContainerNetworkPublicationMode::MachineForwarded, Some(forwarder)) => {
                Ok(Some(forwarder))
            }
            (ContainerNetworkPublicationMode::HostManaged, Some(_)) => {
                Err(crate::error::SandboxError::InvalidSpec {
                    message: format!(
                        "container sandbox {sandbox_id} declares host-managed publication but \
                         carries machine forwarder authority"
                    ),
                })
            }
            (ContainerNetworkPublicationMode::MachineForwarded, None) => {
                Err(crate::error::SandboxError::InvalidSpec {
                    message: format!(
                        "container sandbox {sandbox_id} declares machine-forwarded publication \
                         but has no machine forwarder authority"
                    ),
                })
            }
        }
    }

    pub(super) fn to_backend_config(&self) -> ContainerSandboxBackendConfig {
        ContainerSandboxBackendConfig {
            workload_state_root: self.workload_state_root.clone(),
            network_state_root: self.network_state_root.clone(),
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
