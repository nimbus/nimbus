use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use ulid::Ulid;

use super::bundle::{KrunBundleLayout, KrunBundleMount, KrunBundleOptions, write_bundle_config};
use crate::backend::{SandboxBackend, SandboxBackendKind, SandboxFuture};
#[cfg(test)]
use crate::backends::conmon::lifecycle::RestartLaunchTestProbe;
#[cfg(test)]
use crate::backends::conmon::lifecycle::detect_runtime_status as detect_conmon_runtime_status;
use crate::backends::conmon::lifecycle::{
    RuntimeStatusProbe, configured_stop_signal, configured_stop_timeout, ensure_linux_host,
    read_exit_code, read_pid, remove_if_exists, restart_backoff_delay,
    restart_policy_allows_restart, run_status_best_effort, run_status_checked, runtime_state,
    signal_process, wait_for_path,
};
use crate::backends::conmon::spec_resolve::{
    merge_env_overrides, resolve_process_spec, resolve_root_spec, slugify,
};
use crate::backends::oci::buildah::{ImageHealthcheck, OciExposedPort, OciImageLaunchDefaults};
use crate::backends::oci::builder::OciDockerfileBuilder;
use crate::backends::oci::conmon::{
    OciConmonConfig, OciConmonLaunchPlan, OciConmonLayout, build_launch_plan,
};
use crate::backends::oci::egress::{
    EgressProxyAssignment, EgressProxyRegistry, egress_decision_log_root,
    egress_listener_reservation, egress_proxy_assignment, egress_trust_anchor_mount,
    egress_trust_anchor_root,
};
use crate::backends::oci::materializer::{
    MaterializedImageRootfs, OciImageMaterializer, PreparedMaterializedImageLaunch,
};
use crate::backends::oci::network::{
    ConfiguredSegmentAllocator, DEFAULT_AARDVARK_DNS_BINARY, DEFAULT_NETAVARK_BINARY,
    DEFAULT_NETWORK_INTERFACE, DEFAULT_NETWORK_NAME, DEFAULT_NETWORK_SUBNET, DEFAULT_TENANT_PREFIX,
    OciNetworkConfig, OciNetworkDirectEgress, OciNetworkLayout, OciSegmentAllocator,
    OciSegmentRealization, TerminalNetworkAuthoritySet, authenticate_container_network_generation,
    authenticate_container_network_generation_for_cleanup, create_persistent_network_namespace,
    deallocate_container_ips_after_confirmed_detach, default_network_attachment_id,
    pin_netns_egress_to_own_proxy, place_sandbox_on_block, purge_legacy_nimbus0_once,
    quarantine_network_segment_hold, reconcile_startup_network_state, release_network_segment_hold,
    release_reserved_network_launch_after_ports, remove_persistent_network_namespace,
    retire_terminal_container_ipam_release, setup_container_network, teardown_container_network,
};
use crate::backends::oci::port_lease::new_launch_reservation_claim;
use crate::backends::oci::port_manager::{
    DEFAULT_MAX_PORTS_PER_TENANT, LaunchPortBatchState, NetavarkPortLifetimeRegistry, PortManager,
    ReservedLaunchPorts, SandboxLaunchPortPlan,
};
use crate::backends::oci::resource_quota::ResourceQuotaManager;
use crate::error::{Result, SandboxError};
use crate::instance::{SandboxHandle, SandboxId, SandboxStatus};
use crate::spec::{
    SandboxOciImageSource, SandboxResourceQuotaPolicy, SandboxRootSpec, SandboxRootfsSpec,
    SandboxSpec, resolve_process_without_image_defaults,
};
use nimbus_network::{EndpointProtocol, NetworkReservationClaim, PublishedEndpoint};

mod attachment_recovery;
mod creator;
mod lifecycle;
mod manifest_publication;
mod readiness;
mod start;

#[cfg(test)]
use self::lifecycle::KrunLifecycleLockTestProbe;
#[cfg(test)]
use self::readiness::{
    probe_target_ready, readiness_probe_target, running_status, visible_published_endpoints,
};
#[cfg(test)]
use self::start::{desired_krun_vm_config, krun_vm_config_path, parse_guest_user};

const DEFAULT_RUNTIME_PATH: &str = "/usr/libexec/nimbus/crun";
const DEFAULT_CONMON_PATH: &str = "conmon";
const DEFAULT_BUILDAH_PATH: &str = "buildah";
const DEFAULT_GUEST_USER_HELPER_ROOT: &str = "/usr/libexec/nimbus";
const DEFAULT_PUBLISHED_PORT_START: u16 = 15_000;
const DEFAULT_PUBLISHED_PORT_END: u16 = 16_000;
const DEFAULT_START_TIMEOUT_SECS: u64 = 10;
const DEFAULT_STOP_TIMEOUT_SECS: u64 = 5;
const DEFAULT_READINESS_PROBE_TIMEOUT_MILLIS: u64 = 1_000;
const KRUN_VM_CONFIG_FILENAME: &str = ".krun_vm.json";
const GUEST_USER_HELPER_BINARY_NAME: &str = "nimbus-guest-user-switch";
const GUEST_USER_HELPER_GUEST_ROOT: &str = "/.nimbus";
const GUEST_USER_HELPER_GUEST_PATH: &str = "/.nimbus/nimbus-guest-user-switch";
const GUEST_USER_UID_ENV: &str = "NIMBUS_GUEST_UID";
const GUEST_USER_GID_ENV: &str = "NIMBUS_GUEST_GID";
const BYTES_PER_MIB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KrunStartMode {
    Execute,
    PlanOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KrunSandboxBackendConfig {
    pub bundle_root: PathBuf,
    pub state_root: PathBuf,
    pub conmon_path: PathBuf,
    pub runtime_path: PathBuf,
    pub buildah_path: PathBuf,
    pub netavark_path: PathBuf,
    pub aardvark_dns_path: PathBuf,
    #[cfg(test)]
    pub buildah_launcher_args: Vec<String>,
    pub guest_user_helper_root: PathBuf,
    pub use_buildah_unshare: bool,
    pub network_name: String,
    pub network_interface: String,
    pub network_subnet: String,
    /// The node's network super-net that per-tenant subnets are carved from
    /// (audit M1). Defaults to the node-0 `/16` slice of the cluster pool; the
    /// cluster leg installs a raft-committed slice per node in MTN7.
    pub node_network_supernet: String,
    /// The prefix length of each per-tenant block subnet (MTN6). Defaults to
    /// `/24` (253 sandboxes/block); a tenant that exceeds a block grows an
    /// additional block bridge on demand.
    pub node_tenant_subnet_prefix: u8,
    pub published_port_range: RangeInclusive<u16>,
    pub max_published_ports_per_tenant: Option<usize>,
    pub resource_quota_policy: SandboxResourceQuotaPolicy,
    pub start_mode: KrunStartMode,
    pub log_level: String,
    pub start_timeout: Duration,
    pub stop_timeout: Duration,
}

impl KrunSandboxBackendConfig {
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
            start_mode: KrunStartMode::PlanOnly,
            ..Self::default()
        }
    }
}

impl Default for KrunSandboxBackendConfig {
    fn default() -> Self {
        let temp_root = std::env::temp_dir().join("nimbus-sandbox");
        Self {
            bundle_root: temp_root.join("bundles"),
            state_root: temp_root.join("state"),
            conmon_path: PathBuf::from(DEFAULT_CONMON_PATH),
            runtime_path: PathBuf::from(DEFAULT_RUNTIME_PATH),
            buildah_path: PathBuf::from(DEFAULT_BUILDAH_PATH),
            netavark_path: PathBuf::from(DEFAULT_NETAVARK_BINARY),
            aardvark_dns_path: PathBuf::from(DEFAULT_AARDVARK_DNS_BINARY),
            #[cfg(test)]
            buildah_launcher_args: Vec::new(),
            guest_user_helper_root: PathBuf::from(DEFAULT_GUEST_USER_HELPER_ROOT),
            use_buildah_unshare: true,
            network_name: DEFAULT_NETWORK_NAME.to_owned(),
            network_interface: DEFAULT_NETWORK_INTERFACE.to_owned(),
            network_subnet: DEFAULT_NETWORK_SUBNET.to_owned(),
            node_network_supernet: "10.0.0.0/16".to_owned(),
            node_tenant_subnet_prefix: DEFAULT_TENANT_PREFIX,
            published_port_range: DEFAULT_PUBLISHED_PORT_START..=DEFAULT_PUBLISHED_PORT_END,
            max_published_ports_per_tenant: Some(DEFAULT_MAX_PORTS_PER_TENANT),
            resource_quota_policy: SandboxResourceQuotaPolicy::default(),
            start_mode: KrunStartMode::Execute,
            log_level: "debug".to_owned(),
            start_timeout: Duration::from_secs(DEFAULT_START_TIMEOUT_SECS),
            stop_timeout: Duration::from_secs(DEFAULT_STOP_TIMEOUT_SECS),
        }
    }
}

#[derive(Clone)]
pub struct KrunSandboxBackend {
    config: KrunSandboxBackendConfig,
    segment_allocator: Arc<OciSegmentAllocator>,
    egress_proxies: EgressProxyRegistry,
    netavark_port_lifetimes: NetavarkPortLifetimeRegistry,
    startup_network_reconciliation_error: Option<Arc<str>>,
    #[cfg(test)]
    restart_launch_test_probe: Option<RestartLaunchTestProbe>,
    #[cfg(test)]
    lifecycle_lock_test_probe: Option<KrunLifecycleLockTestProbe>,
    #[cfg(test)]
    effect_barrier_test_probe: Option<KrunEffectBarrierTestProbe>,
    #[cfg(test)]
    terminal_ipam_retirement_failure: Option<Arc<str>>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KrunEffectBarrierFailureStage {
    BeforeWrite,
    AfterRenameBeforeParentSync,
}

#[cfg(test)]
#[derive(Clone)]
struct KrunEffectBarrierTestProbe {
    operation: Arc<str>,
    stage: KrunEffectBarrierFailureStage,
    fired: Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(test)]
impl KrunEffectBarrierTestProbe {
    fn once(operation: impl Into<Arc<str>>, stage: KrunEffectBarrierFailureStage) -> Self {
        Self {
            operation: operation.into(),
            stage,
            fired: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn claim(&self, operation: &str) -> Option<KrunEffectBarrierFailureStage> {
        (self.operation.as_ref() == operation
            && self
                .fired
                .compare_exchange(
                    false,
                    true,
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                )
                .is_ok())
        .then_some(self.stage)
    }
}

impl KrunSandboxBackend {
    pub fn new(config: KrunSandboxBackendConfig) -> Self {
        let segment_allocator: Arc<OciSegmentAllocator> =
            Arc::new(ConfiguredSegmentAllocator::new(
                config.state_root.clone(),
                config.node_network_supernet.clone(),
                config.node_tenant_subnet_prefix,
            ));
        Self::with_segment_allocator(config, segment_allocator)
    }

    pub(crate) fn with_segment_allocator(
        config: KrunSandboxBackendConfig,
        segment_allocator: Arc<OciSegmentAllocator>,
    ) -> Self {
        let startup_network_reconciliation_error =
            reconcile_startup_network_state(&config.state_root, segment_allocator.as_ref())
                .err()
                .map(|error| Arc::<str>::from(error.to_string()));
        let egress_proxies = EgressProxyRegistry::with_roots_and_network_state(
            egress_decision_log_root(&config.state_root),
            egress_trust_anchor_root(&config.state_root),
            &config.state_root,
        );
        Self {
            config,
            segment_allocator,
            egress_proxies,
            netavark_port_lifetimes: NetavarkPortLifetimeRegistry::default(),
            startup_network_reconciliation_error,
            #[cfg(test)]
            restart_launch_test_probe: None,
            #[cfg(test)]
            lifecycle_lock_test_probe: None,
            #[cfg(test)]
            effect_barrier_test_probe: None,
            #[cfg(test)]
            terminal_ipam_retirement_failure: None,
        }
    }

    fn ensure_startup_network_reconciliation_ready(&self) -> Result<()> {
        if let Some(error) = self.startup_network_reconciliation_error.as_ref() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun backend refuses new network work because startup reconciliation did not \
                     complete: {error}"
                ),
            });
        }
        Ok(())
    }

    #[cfg(test)]
    fn with_restart_launch_test_probe(mut self, probe: RestartLaunchTestProbe) -> Self {
        self.restart_launch_test_probe = Some(probe);
        self
    }

    #[cfg(test)]
    fn with_lifecycle_lock_test_probe(mut self, probe: KrunLifecycleLockTestProbe) -> Self {
        self.lifecycle_lock_test_probe = Some(probe);
        self
    }

    #[cfg(test)]
    fn with_effect_barrier_test_probe(mut self, probe: KrunEffectBarrierTestProbe) -> Self {
        self.effect_barrier_test_probe = Some(probe);
        self
    }

    #[cfg(test)]
    fn with_terminal_ipam_retirement_failure(mut self, message: impl Into<Arc<str>>) -> Self {
        self.terminal_ipam_retirement_failure = Some(message.into());
        self
    }

    /// Build the OCI network config for a specific resolved block segment. Shared
    /// by the primary-block `network_config` and block-aware `place_sandbox_config`
    /// (MTN6).
    fn config_from_segment(
        &self,
        segment: &OciSegmentRealization,
        reservation_claim: &NetworkReservationClaim,
    ) -> OciNetworkConfig {
        OciNetworkConfig {
            netavark_path: self.config.netavark_path.clone(),
            aardvark_dns_path: self.config.aardvark_dns_path.clone(),
            network_name: segment.network_name().to_owned(),
            network_interface: segment.network_interface().to_owned(),
            network_subnet: segment.cidr().to_string(),
            segment_id: segment.segment_id().as_str().to_owned(),
            reservation_claim: reservation_claim.clone(),
            direct_egress: OciNetworkDirectEgress::Deny,
            // The deny-by-default microVM guest resolves names through the host
            // PEP (`HTTP_PROXY`), never a local resolver, so netavark must not
            // start an aardvark-dns stub on the bridge gateway `:53`. That stub
            // is the residual DNS-exfil channel KME5 flagged.
            enable_dns: false,
            network_id: segment.network_id().as_str().to_owned(),
        }
    }

    #[cfg(test)]
    fn network_config(&self, tenant: &nimbus_core::TenantId) -> Result<OciNetworkConfig> {
        // Per-tenant PRIMARY block: distinct subnet + bridge identity (audit M1).
        let segment = self.segment_allocator.segment_for(tenant)?;
        let reservation_claim = crate::backends::oci::port_lease::new_launch_reservation_claim()?;
        Ok(self.config_from_segment(&segment, &reservation_claim))
    }

    /// Block-aware placement (MTN6): resolve + reserve the block bridge that will
    /// host `sandbox_id`, growing a new sibling block when the current `/24`s are
    /// full. Fail-closed when the node super-net is exhausted.
    fn place_sandbox_config(
        &self,
        tenant: &nimbus_core::TenantId,
        layout: &OciNetworkLayout,
        sandbox_id: &SandboxId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<OciNetworkConfig> {
        place_sandbox_on_block(
            self.segment_allocator.as_ref(),
            tenant,
            layout,
            sandbox_id,
            reservation_claim,
            |segment, claim| self.config_from_segment(segment, claim),
        )
    }

    fn release_reserved_launch(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        let reservation_claim = manifest.require_reserved_claim()?;
        let manager = self.port_manager();
        release_reserved_network_launch_after_ports(
            self.segment_allocator.as_ref(),
            &manifest.network_layout,
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            reservation_claim,
            manager.release_never_bound_launch_claim(reservation_claim),
        )
    }

    fn release_unpublished_reserved_launch(
        &self,
        manifest: &KrunSandboxManifest,
        reservations: &ReservedLaunchPorts,
    ) -> Result<()> {
        let reservation_claim = manifest.require_reserved_claim()?;
        let manager = self.port_manager();
        release_reserved_network_launch_after_ports(
            self.segment_allocator.as_ref(),
            &manifest.network_layout,
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            reservation_claim,
            manager.release_unpublished_launch_ports(reservations, reservation_claim),
        )
    }

    fn remove_tenant_artifacts_sync(&self, tenant_id: &nimbus_core::TenantId) -> Result<()> {
        for root in [&self.config.bundle_root, &self.config.state_root] {
            crate::artifact_paths::remove_tenant_root(root, tenant_id).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!(
                        "failed to remove krun sandbox tenant artifacts for {} under {}: {error}",
                        tenant_id,
                        root.display()
                    ),
                }
            })?;
        }
        Ok(())
    }

    fn start_sync(&self, spec: SandboxSpec) -> Result<SandboxHandle> {
        let launch_plan = self.plan_start(&spec)?;
        self.finish_start(launch_plan)
    }

    fn finish_start(&self, launch_plan: KrunStartPlan) -> Result<SandboxHandle> {
        let mut manifest = launch_plan.manifest;
        let materialization_lifecycle = (self.config.start_mode == KrunStartMode::Execute)
            .then(|| self.lock_launch_lifecycle(&manifest))
            .transpose()?;
        if materialization_lifecycle.is_some() {
            self.require_current_launch_plan(&manifest)?;
        }
        if let Err(error) = self.materialize_krun_vm_config(&manifest) {
            return match self.config.start_mode {
                KrunStartMode::PlanOnly => Err(error),
                KrunStartMode::Execute => {
                    Err(self.persist_unstarted_launch_failure(&mut manifest, error))
                }
            };
        }
        drop(materialization_lifecycle);
        let launch_plan = KrunStartPlan { manifest };

        match self.config.start_mode {
            KrunStartMode::PlanOnly => {
                let mut manifest = launch_plan.manifest.clone();
                manifest.last_exit_code = None;
                manifest.shutdown_requested = false;
                self.write_manifest(&manifest)?;
                Ok(manifest.handle)
            }
            KrunStartMode::Execute => self.execute_start(&launch_plan),
        }
    }

    fn resource_quota_manager(&self) -> ResourceQuotaManager {
        ResourceQuotaManager::new(
            self.config.state_root.clone(),
            self.config.resource_quota_policy.clone(),
        )
    }
}

impl SandboxBackend for KrunSandboxBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Krun
    }

    fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle> {
        let backend = self.clone();
        Box::pin(async move { backend.start_sync(spec) })
    }

    fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>> {
        let backend = self.clone();
        let sandbox_id = id.clone();
        Box::pin(async move { backend.inspect_sync(&sandbox_id) })
    }

    fn stop(&self, id: &SandboxId) -> SandboxFuture<()> {
        let backend = self.clone();
        let sandbox_id = id.clone();
        Box::pin(async move { backend.stop_sync(&sandbox_id) })
    }

    fn remove_tenant_artifacts(&self, tenant_id: nimbus_core::TenantId) -> SandboxFuture<()> {
        let backend = self.clone();
        Box::pin(async move { backend.remove_tenant_artifacts_sync(&tenant_id) })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KrunStartPlan {
    manifest: KrunSandboxManifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct KrunSandboxManifest {
    handle: SandboxHandle,
    spec: SandboxSpec,
    image_metadata: KrunImageMetadata,
    launch_artifact: Option<KrunLaunchArtifact>,
    bundle_layout: KrunBundleLayout,
    conmon_layout: OciConmonLayout,
    network_layout: OciNetworkLayout,
    /// Exact placed network config for a launch that owns an attachment.
    ///
    /// `None` is the authority-free PlanOnly state. Execute preparation must
    /// reserve an attachment before setting `Some`, after which setup and
    /// teardown reuse the identical bridge without reassigning.
    network_config: Option<OciNetworkConfig>,
    port_leases: Vec<nimbus_network::PortLeaseRequest>,
    /// Durable authority phase separating exact pre-effect compensation from
    /// authenticated provider-owned teardown.
    launch_authority: KrunLaunchAuthority,
    /// Exact asynchronous creator-attempt state.
    ///
    /// Runtime absence cannot authorize provider or network cleanup while this
    /// remains `Pending`: the creator may still materialize the runtime.
    creator_handoff: KrunCreatorHandoffState,
    /// Durable progress for compensation after provider launch has failed.
    ///
    /// This state is deliberately separate from observed runtime status and
    /// from `launch_authority`: it records which cleanup effects have been
    /// confirmed so a fresh process can resume without falling back to the
    /// ordinary PID-driven stop path.
    provider_failure_cleanup: KrunProviderFailureCleanupState,
    egress_proxy: Option<EgressProxyAssignment>,
    conmon_launch: OciConmonLaunchPlan,
    last_exit_code: Option<i32>,
    #[serde(default)]
    restart_count: u32,
    #[serde(default)]
    next_restart_at_millis: Option<u64>,
    start_mode: KrunStartMode,
    shutdown_requested: bool,
    status: SandboxStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
enum KrunLaunchAuthority {
    PlanOnly,
    Reserved {
        reservation_claim: NetworkReservationClaim,
    },
    Adopting {
        reservation_claim: NetworkReservationClaim,
    },
    Adopted {
        reservation_claim: NetworkReservationClaim,
    },
    ProviderOwned,
    Released,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
enum KrunCreatorHandoffState {
    NotSpawned,
    SpawnIntent {
        attempt_id: String,
    },
    Pending {
        receipt: crate::backends::conmon::creator::CreatorAttemptReceipt,
    },
    Quiesced {
        proof: crate::backends::conmon::creator::CreatorQuiescenceProof,
    },
    RuntimeObserved {
        receipt: crate::backends::conmon::creator::CreatorAttemptReceipt,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
enum KrunProviderFailureCleanupState {
    Inactive,
    Requested,
    RuntimeAbsent {
        proof: KrunRuntimeAbsenceProof,
    },
    NetworkReleased {
        runtime_absence: KrunRuntimeAbsenceProof,
    },
    ArtifactsReleased {
        runtime_absence: KrunRuntimeAbsenceProof,
    },
}

impl KrunProviderFailureCleanupState {
    fn is_active(&self) -> bool {
        !matches!(self, Self::Inactive)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum KrunRuntimeAbsenceProof {
    NeverSpawned,
    ObservedAbsent { attempt_id: String },
}

impl KrunCreatorHandoffState {
    fn authorizes_provider_cleanup(&self) -> bool {
        matches!(
            self,
            Self::NotSpawned | Self::Quiesced { .. } | Self::RuntimeObserved { .. }
        )
    }
}

impl KrunSandboxManifest {
    fn has_terminal_network_finality(&self) -> bool {
        let launch_authority_released = match self.start_mode {
            KrunStartMode::PlanOnly => {
                self.launch_authority == KrunLaunchAuthority::PlanOnly
                    && self.network_config.is_none()
                    && self.port_leases.is_empty()
                    && self.egress_proxy.is_none()
            }
            KrunStartMode::Execute => self.launch_authority == KrunLaunchAuthority::Released,
        };
        matches!(self.status, SandboxStatus::Stopped | SandboxStatus::Failed)
            && self.shutdown_requested
            && self.handle.status == self.status
            && self.launch_artifact.is_none()
            && !self.provider_failure_cleanup.is_active()
            && self.creator_handoff.authorizes_provider_cleanup()
            && self.next_restart_at_millis.is_none()
            && launch_authority_released
    }

    fn require_network_config(&self) -> Result<&OciNetworkConfig> {
        self.network_config
            .as_ref()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "krun workload {} has no reserved network attachment configuration",
                    self.handle.id
                ),
            })
    }

    fn reservation_claim(&self) -> Option<&NetworkReservationClaim> {
        match &self.launch_authority {
            KrunLaunchAuthority::Reserved { reservation_claim }
            | KrunLaunchAuthority::Adopting { reservation_claim }
            | KrunLaunchAuthority::Adopted { reservation_claim } => Some(reservation_claim),
            KrunLaunchAuthority::PlanOnly
            | KrunLaunchAuthority::ProviderOwned
            | KrunLaunchAuthority::Released => None,
        }
    }

    fn require_reserved_claim(&self) -> Result<&NetworkReservationClaim> {
        match &self.launch_authority {
            KrunLaunchAuthority::Reserved { reservation_claim }
            | KrunLaunchAuthority::Adopting { reservation_claim } => Ok(reservation_claim),
            phase => Err(SandboxError::OperationFailed {
                message: format!(
                    "krun workload {} requires reserved launch authority before provider adoption, \
                     got {phase:?}",
                    self.handle.id
                ),
            }),
        }
    }

    fn mark_adopted(&mut self) -> Result<NetworkReservationClaim> {
        let claim = self.require_reserved_claim()?.clone();
        self.launch_authority = KrunLaunchAuthority::Adopted {
            reservation_claim: claim.clone(),
        };
        Ok(claim)
    }

    fn mark_adopting(&mut self) -> Result<NetworkReservationClaim> {
        let claim = self.require_reserved_claim()?.clone();
        self.launch_authority = KrunLaunchAuthority::Adopting {
            reservation_claim: claim.clone(),
        };
        Ok(claim)
    }

    fn permits_provider_teardown(&self) -> bool {
        matches!(
            self.launch_authority,
            KrunLaunchAuthority::Adopted { .. } | KrunLaunchAuthority::ProviderOwned
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KrunResolvedLaunchSpec {
    spec: SandboxSpec,
    image_metadata: KrunImageMetadata,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct KrunImageMetadata {
    user: Option<String>,
    stop_signal: Option<String>,
    healthcheck: Option<ImageHealthcheck>,
    labels: BTreeMap<String, String>,
    exposed_ports: Vec<OciExposedPort>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct KrunVmConfig {
    cpus: u8,
    ram_mib: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GuestUserIds {
    uid: u32,
    gid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum KrunLaunchArtifact {
    Rootfs(MaterializedImageRootfs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadinessProbeTarget {
    Tcp(SocketAddr),
    Http(SocketAddr),
}

#[cfg(test)]
mod tests;
