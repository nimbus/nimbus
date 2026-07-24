use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use ulid::Ulid;

use super::bundle::{KrunBundleLayout, KrunBundleMount, KrunBundleOptions, write_bundle_config};
use crate::backend::{SandboxBackend, SandboxBackendKind, SandboxFuture};
use crate::backends::conmon::lifecycle::{
    RuntimeStatusProbe, configured_stop_signal, configured_stop_timeout,
    detect_runtime_status as detect_conmon_runtime_status, ensure_linux_host, read_exit_code,
    read_pid, remove_if_exists, restart_backoff_delay, restart_policy_allows_restart,
    run_status_best_effort, run_status_checked, signal_process, spawn_background, wait_for_path,
    wait_for_runtime_state,
};
use crate::backends::conmon::spec_resolve::{
    merge_env_overrides, resolve_process_spec, resolve_root_spec, slugify,
};
use crate::backends::oci::buildah::{
    BuildahCli, ImageHealthcheck, MountedRootfsSession, OciExposedPort, OciImageLaunchDefaults,
};
use crate::backends::oci::builder::OciDockerfileBuilder;
use crate::backends::oci::conmon::{
    OciConmonConfig, OciConmonLaunchPlan, OciConmonLayout, build_launch_plan,
};
use crate::backends::oci::egress::{
    EgressProxyAssignment, EgressProxyRegistry, egress_decision_log_root,
    egress_trust_anchor_mount, egress_trust_anchor_root,
};
use crate::backends::oci::materializer::{
    MaterializedImageRootfs, OciImageMaterializer, PreparedMaterializedImageLaunch,
};
use crate::backends::oci::network::{
    DEFAULT_AARDVARK_DNS_BINARY, DEFAULT_NETAVARK_BINARY, DEFAULT_NETWORK_INTERFACE,
    DEFAULT_NETWORK_NAME, DEFAULT_NETWORK_SUBNET, DEFAULT_TENANT_PREFIX, NetworkSegmentAllocator,
    OciNetworkConfig, OciNetworkDirectEgress, OciNetworkLayout, SingleNodeSegmentAllocator,
    create_persistent_network_namespace, pin_netns_egress_to_own_proxy, place_sandbox_on_block,
    purge_legacy_nimbus0_once, reconcile_network_segment_orphans, release_network_segment_hold,
    remove_persistent_network_namespace, setup_container_network, teardown_container_network,
};
use crate::backends::oci::port_manager::{DEFAULT_MAX_PORTS_PER_TENANT, PortManager};
use crate::backends::oci::resource_quota::ResourceQuotaManager;
use crate::endpoint::{PublishedEndpoint, PublishedEndpointProtocol};
use crate::error::{Result, SandboxError};
use crate::instance::{SandboxHandle, SandboxId, SandboxStatus};
use crate::spec::{
    SandboxOciImageSource, SandboxResourceQuotaPolicy, SandboxRootSpec, SandboxRootfsSpec,
    SandboxSpec, resolve_process_without_image_defaults,
};
use nimbus_core::net::NetworkSegment;

mod lifecycle;
mod readiness;
mod start;

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
    egress_proxies: EgressProxyRegistry,
}

impl KrunSandboxBackend {
    pub fn new(config: KrunSandboxBackendConfig) -> Self {
        // Startup orphan GC: reclaim segment holds whose sandbox netns is gone
        // (best-effort; a fresh node with no persisted state is a no-op).
        if let Ok(allocator) = SingleNodeSegmentAllocator::for_node_supernet(
            &config.state_root,
            &config.node_network_supernet,
            config.node_tenant_subnet_prefix,
        ) {
            let _ = reconcile_network_segment_orphans(&config.state_root, &allocator);
        }
        let egress_proxies = EgressProxyRegistry::with_roots(
            egress_decision_log_root(&config.state_root),
            egress_trust_anchor_root(&config.state_root),
        );
        Self {
            config,
            egress_proxies,
        }
    }

    /// The per-node segment allocator, constructed on demand from the state root
    /// (its state is the fs-locked segments.json, so it is stateless to hold).
    fn segment_allocator(&self) -> Result<SingleNodeSegmentAllocator> {
        SingleNodeSegmentAllocator::for_node_supernet(
            &self.config.state_root,
            &self.config.node_network_supernet,
            self.config.node_tenant_subnet_prefix,
        )
    }

    /// Build the OCI network config for a specific resolved block segment. Shared
    /// by the primary-block `network_config` and block-aware `place_sandbox_config`
    /// (MTN6).
    fn config_from_segment(&self, segment: &NetworkSegment) -> OciNetworkConfig {
        OciNetworkConfig {
            netavark_path: self.config.netavark_path.clone(),
            aardvark_dns_path: self.config.aardvark_dns_path.clone(),
            network_name: segment.network_name().to_owned(),
            network_interface: segment.network_interface().to_owned(),
            network_subnet: segment.cidr().to_string(),
            direct_egress: OciNetworkDirectEgress::Deny,
            // The deny-by-default microVM guest resolves names through the host
            // PEP (`HTTP_PROXY`), never a local resolver, so netavark must not
            // start an aardvark-dns stub on the bridge gateway `:53`. That stub
            // is the residual DNS-exfil channel KME5 flagged.
            enable_dns: false,
            network_id: segment.network_id().as_str().to_owned(),
        }
    }

    fn network_config(&self, tenant: &nimbus_core::TenantId) -> Result<OciNetworkConfig> {
        // Per-tenant PRIMARY block: distinct subnet + bridge identity (audit M1).
        let segment = self.segment_allocator()?.segment_for(tenant)?;
        Ok(self.config_from_segment(&segment))
    }

    /// Block-aware placement (MTN6): resolve + reserve the block bridge that will
    /// host `sandbox_id`, growing a new sibling block when the current `/24`s are
    /// full. Fail-closed when the node super-net is exhausted.
    fn place_sandbox_config(
        &self,
        tenant: &nimbus_core::TenantId,
        layout: &OciNetworkLayout,
        sandbox_id: &SandboxId,
    ) -> Result<OciNetworkConfig> {
        place_sandbox_on_block(
            &self.segment_allocator()?,
            tenant,
            layout,
            sandbox_id,
            |segment| self.config_from_segment(segment),
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
        self.materialize_auto_port_bindings(&mut manifest)?;
        self.materialize_krun_vm_config(&manifest)?;
        let launch_plan = KrunStartPlan { manifest };

        match self.config.start_mode {
            KrunStartMode::PlanOnly => {
                let mut manifest = launch_plan.manifest.clone();
                manifest.last_exit_code = None;
                manifest.shutdown_requested = false;
                self.write_manifest(&manifest)?;
                Ok(manifest.handle)
            }
            KrunStartMode::Execute => self.execute_start(&launch_plan).inspect_err(|_| {
                let _ = self.cleanup_manifest_launch_artifacts(&launch_plan.manifest);
            }),
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
    /// The tenant's resolved per-tenant network config, assigned once at
    /// manifest-prepare so setup and teardown reuse the identical bridge without
    /// re-assigning (audit M1 / MTN4 reaper).
    #[serde(default)]
    network_config: OciNetworkConfig,
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
    MountedRootfs(MountedRootfsSession),
    Rootfs(MaterializedImageRootfs),
}

impl KrunLaunchArtifact {
    fn mount_session_name(&self) -> Option<&str> {
        match self {
            Self::MountedRootfs(session) => Some(session.session_name.as_str()),
            Self::Rootfs(_) => None,
        }
    }

    fn uses_mount_session_unshare(&self) -> bool {
        matches!(self, Self::MountedRootfs(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadinessProbeTarget {
    Tcp(SocketAddr),
    Http(SocketAddr),
}

#[cfg(test)]
mod tests;
