use std::collections::BTreeMap;
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
use crate::backends::capabilities::{
    SandboxAttachmentRegistrationError, SandboxAttachmentRegistrationKind,
    host_managed_attachment_registration,
};
#[cfg(test)]
use crate::backends::conmon::lifecycle::RestartLaunchTestProbe;
#[cfg(test)]
use crate::backends::conmon::lifecycle::detect_runtime_status as detect_conmon_runtime_status;
#[cfg(test)]
use crate::backends::conmon::lifecycle::remove_if_exists;
use crate::backends::conmon::lifecycle::{
    RuntimeStatusProbe, ensure_linux_host, run_status_best_effort, run_status_checked,
    runtime_state,
};
#[cfg(test)]
use crate::backends::conmon::lifecycle::{configured_stop_signal, configured_stop_timeout};
use crate::backends::conmon::spec_resolve::{
    merge_env_overrides, resolve_process_spec, resolve_root_spec, slugify,
};
use crate::backends::oci::buildah::{ImageHealthcheck, OciExposedPort, OciImageLaunchDefaults};
use crate::backends::oci::builder::OciDockerfileBuilder;
use crate::backends::oci::conmon::{
    OciConmonConfig, OciConmonLaunchPlan, OciConmonLayout, build_launch_plan,
};
use crate::backends::oci::egress::{
    EgressProxyAssignment, EgressProxyRegistry, EgressReadinessState, egress_decision_log_root,
    egress_listener_reservation, egress_proxy_assignment, egress_trust_anchor_mount,
    egress_trust_anchor_root,
};
use crate::backends::oci::materializer::{
    MaterializedImageRootfs, OciImageMaterializer, PreparedMaterializedImageLaunch,
};
use crate::backends::oci::network::{
    AttachmentAttachAuthority, AttachmentBackendKind, ConfiguredSegmentAllocator,
    DEFAULT_AARDVARK_DNS_BINARY, DEFAULT_NETAVARK_BINARY, DEFAULT_NETWORK_INTERFACE,
    DEFAULT_NETWORK_NAME, DEFAULT_NETWORK_SUBNET, DEFAULT_TENANT_PREFIX, OciAttachmentAdapter,
    OciAttachmentAuxiliaryListener, OciAttachmentInput, OciAttachmentLifecycle,
    OciAttachmentProviderPaths, OciEgressPinProvider, OciHostManagedAttachmentBackend,
    OciIpamAuthority, OciNetworkConfig, OciNetworkLayout, OciNetworkProcess, OciSegmentAllocator,
    RealOciEgressPinProvider, TerminalNetworkAuthoritySet, TerminalNetworkFinalityEvidence,
    default_network_attachment_id, retire_terminal_container_ipam_release,
};
#[cfg(test)]
use crate::backends::oci::network::{
    HostManagedAttachmentCheckpointTestProbe, OciAttachmentReadinessState,
};
use crate::backends::oci::port_lease::new_launch_reservation_claim;
use crate::backends::oci::port_lifecycle::{
    DEFAULT_MAX_PORTS_PER_TENANT, NetavarkPortLifetimeRegistry, OciPortLeaseCoordinator,
    ReservedLaunchPorts, SandboxLaunchPortPlan,
};
use crate::backends::oci::resource_quota::ResourceQuotaManager;
use crate::backends::readiness_probe::{ReadinessProbeProvider, SocketReadinessProbeProvider};
use crate::error::{Result, SandboxError};
use crate::execution_attempt::SandboxExecutionAttemptId;
use crate::instance::{SandboxHandle, SandboxId, SandboxStatus};
use crate::provision::SandboxProvisionNetworkPlan;
use crate::spec::{
    SandboxOciImageSource, SandboxResourceQuotaPolicy, SandboxRootSpec, SandboxRootfsSpec,
    SandboxSpec, resolve_process_without_image_defaults,
};
use nimbus_network::{
    NetworkAttachmentId, NetworkAttachmentSegmentAssociation, NetworkReservationClaim,
    PublishedEndpoint,
};

mod attachment_teardown;
mod creator;
mod inspection;
mod lifecycle;
mod manifest_publication;
mod network_composition;
mod provision;
mod readiness;
mod restart;
mod teardown;
#[cfg(any(test, feature = "test-hooks"))]
mod test_hooks;
#[cfg(any(test, feature = "test-hooks"))]
pub(in crate::backends) use test_hooks::{
    prepare_network_teardown_fixture, reopen_network_teardown_fixture,
};

impl OciHostManagedAttachmentBackend for KrunSandboxBackend {
    const ATTACHMENT_BACKEND_KIND: AttachmentBackendKind = AttachmentBackendKind::Krun;
}
mod root_authentication;
mod start;
mod startup_orphan_convergence;

#[cfg(test)]
use self::lifecycle::KrunLifecycleLockTestProbe;
#[cfg(test)]
use self::readiness::{published_endpoints, running_status, visible_published_endpoints};
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
    /// Backend-local manifests, runtime artifacts, trust material, and quotas.
    pub workload_state_root: PathBuf,
    /// Node-local segment, IPAM, and host-port allocation authority.
    pub network_state_root: PathBuf,
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
        config.workload_state_root = root.join("state");
        config.network_state_root = root.join("state");
        config
    }

    pub fn plan_only(
        bundle_root: impl Into<PathBuf>,
        workload_state_root: impl Into<PathBuf>,
    ) -> Self {
        let workload_state_root = workload_state_root.into();
        Self {
            bundle_root: bundle_root.into(),
            network_state_root: workload_state_root.clone(),
            workload_state_root,
            start_mode: KrunStartMode::PlanOnly,
            ..Self::default()
        }
    }

    /// Override the node-local network authority without moving workload state.
    pub fn with_network_state_root(mut self, network_state_root: impl Into<PathBuf>) -> Self {
        self.network_state_root = network_state_root.into();
        self
    }
}

impl Default for KrunSandboxBackendConfig {
    fn default() -> Self {
        let temp_root = std::env::temp_dir().join("nimbus-sandbox");
        Self {
            bundle_root: temp_root.join("bundles"),
            workload_state_root: temp_root.join("state"),
            network_state_root: temp_root.join("state"),
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
    attachment_authority: Option<nimbus_network::LocalNetworkAttachmentAuthority>,
    ipam_authority: OciIpamAuthority,
    port_lease_coordinator: OciPortLeaseCoordinator,
    egress_proxies: EgressProxyRegistry,
    teardown_runtime_provider: Arc<dyn teardown::effects::KrunExecutionTeardownRuntime>,
    egress_pin_provider: Arc<dyn OciEgressPinProvider>,
    readiness_probe_provider: Arc<dyn ReadinessProbeProvider>,
    netavark_port_lifetimes: NetavarkPortLifetimeRegistry,
    _network_process: Option<Arc<OciNetworkProcess>>,
    startup_network_reconciliation_error: Option<Arc<str>>,
    #[cfg(test)]
    restart_launch_test_probe: Option<RestartLaunchTestProbe>,
    #[cfg(test)]
    lifecycle_lock_test_probe: Option<KrunLifecycleLockTestProbe>,
    #[cfg(test)]
    effect_barrier_test_probe: Option<KrunEffectBarrierTestProbe>,
    #[cfg(test)]
    terminal_ipam_retirement_failure: Option<Arc<str>>,
    #[cfg(test)]
    network_teardown_checkpoint_test_probe: Option<HostManagedAttachmentCheckpointTestProbe>,
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
    /// Open this provider's durable provision-attempt idempotency journal.
    pub fn attempt_idempotency_journal(
        &self,
    ) -> std::result::Result<crate::ProviderCommandAttemptJournal, crate::ProviderCommandJournalError>
    {
        crate::ProviderCommandAttemptJournal::open(&self.config.workload_state_root, "krun-runtime")
    }

    /// Report conservative host-managed attachment evidence for this composition.
    ///
    /// This refuses configurations that cannot own the exact local Execute
    /// composition and performs no provider effects or runtime readiness probes.
    pub fn host_managed_attachment_registration(
        &self,
    ) -> std::result::Result<
        nimbus_network::NetworkAttachmentProviderRegistration,
        SandboxAttachmentRegistrationError,
    > {
        host_managed_attachment_registration(
            SandboxAttachmentRegistrationKind::Krun,
            self.config.start_mode == KrunStartMode::Execute,
            false,
            self.startup_network_reconciliation_error.as_ref(),
        )
    }

    pub fn new(config: KrunSandboxBackendConfig) -> Self {
        let segment_allocator: Arc<OciSegmentAllocator> =
            Arc::new(ConfiguredSegmentAllocator::reconstruct_direct(
                &config.network_state_root,
                &config.node_network_supernet,
                config.node_tenant_subnet_prefix,
            ));
        Self::with_segment_allocator(config, segment_allocator)
    }

    pub(crate) fn with_segment_allocator(
        config: KrunSandboxBackendConfig,
        segment_allocator: Arc<OciSegmentAllocator>,
    ) -> Self {
        Self::with_segment_allocator_and_process(config, segment_allocator, None)
    }

    fn with_segment_allocator_and_process(
        config: KrunSandboxBackendConfig,
        segment_allocator: Arc<OciSegmentAllocator>,
        network_process: Option<Arc<OciNetworkProcess>>,
    ) -> Self {
        let attachment_authority = network_process.as_ref().map_or_else(
            || nimbus_network::LocalNetworkAttachmentAuthority::open(&config.network_state_root),
            |process| Ok(process.attachment_authority()),
        );
        let ipam_authority = network_process.as_ref().map_or_else(
            || OciIpamAuthority::reconstruct_direct(&config.network_state_root),
            |process| process.ipam_authority(),
        );
        let port_lease_coordinator = network_process.as_ref().map_or_else(
            || {
                OciPortLeaseCoordinator::reconstruct_direct(
                    &config.network_state_root,
                    config.published_port_range.clone(),
                )
                .with_max_ports_per_tenant(config.max_published_ports_per_tenant)
            },
            |process| {
                process.port_lease_coordinator(
                    config.published_port_range.clone(),
                    config.max_published_ports_per_tenant,
                )
            },
        );
        let attachment_open_error = attachment_authority
            .as_ref()
            .err()
            .map(|error| Arc::<str>::from(error.to_string()));
        let egress_proxies = match network_process.as_ref() {
            Some(process) => process.egress_registry(
                egress_decision_log_root(&config.workload_state_root),
                egress_trust_anchor_root(&config.workload_state_root),
            ),
            None => EgressProxyRegistry::with_roots_and_port_authority(
                egress_decision_log_root(&config.workload_state_root),
                egress_trust_anchor_root(&config.workload_state_root),
                &config.network_state_root,
                port_lease_coordinator.cloned_authority(),
            ),
        };
        let netavark_port_lifetimes = network_process
            .as_ref()
            .map_or_else(NetavarkPortLifetimeRegistry::default, |process| {
                process.netavark_port_lifetimes()
            });
        let mut backend = Self {
            config,
            segment_allocator,
            attachment_authority: attachment_authority.ok(),
            ipam_authority,
            port_lease_coordinator,
            egress_proxies,
            teardown_runtime_provider: Arc::new(
                teardown::effects::HostKrunExecutionTeardownRuntime,
            ),
            egress_pin_provider: Arc::new(RealOciEgressPinProvider),
            readiness_probe_provider: Arc::new(SocketReadinessProbeProvider),
            netavark_port_lifetimes,
            _network_process: network_process,
            startup_network_reconciliation_error: attachment_open_error,
            #[cfg(test)]
            restart_launch_test_probe: None,
            #[cfg(test)]
            lifecycle_lock_test_probe: None,
            #[cfg(test)]
            effect_barrier_test_probe: None,
            #[cfg(test)]
            terminal_ipam_retirement_failure: None,
            #[cfg(test)]
            network_teardown_checkpoint_test_probe: None,
        };
        if backend.startup_network_reconciliation_error.is_none() {
            backend.startup_network_reconciliation_error = backend
                .reconcile_krun_startup_network_state()
                .err()
                .map(|error| Arc::<str>::from(error.to_string()));
        }
        backend
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

    #[cfg(test)]
    fn with_network_teardown_checkpoint_test_probe(
        mut self,
        probe: HostManagedAttachmentCheckpointTestProbe,
    ) -> Self {
        self.network_teardown_checkpoint_test_probe = Some(probe);
        self
    }

    #[cfg(any(test, feature = "test-hooks"))]
    fn with_egress_pin_provider(mut self, provider: Arc<dyn OciEgressPinProvider>) -> Self {
        self.egress_pin_provider = provider;
        self
    }

    #[cfg(test)]
    fn with_readiness_probe_provider(mut self, provider: Arc<dyn ReadinessProbeProvider>) -> Self {
        self.readiness_probe_provider = provider;
        self
    }

    #[cfg(test)]
    fn with_teardown_runtime_provider(
        mut self,
        provider: Arc<dyn teardown::effects::KrunExecutionTeardownRuntime>,
    ) -> Self {
        self.teardown_runtime_provider = provider;
        self
    }

    fn attachment_lifecycle<'a>(
        &'a self,
        ports: &'a OciPortLeaseCoordinator,
    ) -> OciAttachmentLifecycle<'a> {
        OciAttachmentLifecycle::new(
            self.segment_allocator.as_ref(),
            self.attachment_authority.as_ref(),
            &self.ipam_authority,
            ports,
            &self.netavark_port_lifetimes,
        )
    }

    fn mark_attachment_adopting(
        &self,
        manifest: &mut KrunSandboxManifest,
    ) -> Result<NetworkReservationClaim> {
        let claim = manifest.require_reserved_claim()?.clone();
        let config = manifest.require_network_config()?;
        let observation = self.segment_allocator.inspect_attachment_reservation(
            &manifest.spec.tenant_id,
            &config.attachment_id,
            &claim,
        )?;
        if observation.state() != nimbus_network::NetworkAttachmentReservationState::Reserved {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun workload {} cannot persist adoption intent from allocator state {:?}",
                    manifest.handle.id,
                    observation.state()
                ),
            });
        }
        let association = observation
            .association()
            .cloned()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "krun workload {} cannot persist adoption intent without its exact segment association",
                    manifest.handle.id
                ),
            })?;
        manifest.mark_adopting(association)
    }

    fn attachment_adapter<'a>(
        &'a self,
        manifest: &'a KrunSandboxManifest,
        network_config: &'a OciNetworkConfig,
        hostname: &'a str,
    ) -> OciAttachmentAdapter<'a> {
        <Self as OciHostManagedAttachmentBackend>::host_managed_attachment_adapter(
            OciAttachmentInput {
                workload_state_root: &self.config.workload_state_root,
                tenant_id: &manifest.spec.tenant_id,
                sandbox_id: &manifest.handle.id,
                display_name: manifest.spec.display_name(),
                hostname,
                bindings: &manifest.spec.port_bindings,
                leases: &manifest.port_leases,
                auxiliary_listener: manifest.egress_proxy.as_ref().map(|assignment| {
                    OciAttachmentAuxiliaryListener::egress_pep(
                        &assignment.port_lease,
                        &assignment.host,
                        assignment.port,
                    )
                }),
                layout: &manifest.network_layout,
                config: network_config,
                launch_claim: manifest.reservation_claim(),
            },
        )
    }

    fn retained_attachment_adapter<'a>(
        &'a self,
        manifest: &'a KrunSandboxManifest,
        network_config: &'a OciNetworkConfig,
        hostname: &'a str,
    ) -> OciAttachmentAdapter<'a> {
        <Self as OciHostManagedAttachmentBackend>::non_routable_attachment_adapter(
            OciAttachmentInput {
                workload_state_root: &self.config.workload_state_root,
                tenant_id: &manifest.spec.tenant_id,
                sandbox_id: &manifest.handle.id,
                display_name: manifest.spec.display_name(),
                hostname,
                bindings: &manifest.spec.port_bindings,
                leases: &manifest.port_leases,
                auxiliary_listener: manifest.egress_proxy.as_ref().map(|assignment| {
                    OciAttachmentAuxiliaryListener::egress_pep(
                        &assignment.port_lease,
                        &assignment.host,
                        assignment.port,
                    )
                }),
                layout: &manifest.network_layout,
                config: network_config,
                // Provider ownership consumes the launch-authority variant but
                // retained detach still authenticates the immutable complete-plan claim.
                // Server ingress owns publication and must already have made its
                // listener leases terminal before private attachment teardown.
                launch_claim: Some(&network_config.reservation_claim),
            },
        )
    }

    fn non_routable_attachment_adapter<'a>(
        &'a self,
        manifest: &'a KrunSandboxManifest,
        network_config: &'a OciNetworkConfig,
        hostname: &'a str,
    ) -> OciAttachmentAdapter<'a> {
        <Self as OciHostManagedAttachmentBackend>::non_routable_attachment_adapter(
            OciAttachmentInput {
                workload_state_root: &self.config.workload_state_root,
                tenant_id: &manifest.spec.tenant_id,
                sandbox_id: &manifest.handle.id,
                display_name: manifest.spec.display_name(),
                hostname,
                bindings: &manifest.spec.port_bindings,
                leases: &manifest.port_leases,
                auxiliary_listener: manifest.egress_proxy.as_ref().map(|assignment| {
                    OciAttachmentAuxiliaryListener::egress_pep(
                        &assignment.port_lease,
                        &assignment.host,
                        assignment.port,
                    )
                }),
                layout: &manifest.network_layout,
                config: network_config,
                launch_claim: manifest.reservation_claim(),
            },
        )
    }

    #[cfg(test)]
    fn network_config(&self, tenant: &nimbus_core::TenantId) -> Result<OciNetworkConfig> {
        // Per-tenant PRIMARY block: distinct subnet + bridge identity (audit M1).
        let segment = self.segment_allocator.segment_for(tenant)?;
        let reservation_claim = crate::backends::oci::port_lease::new_launch_reservation_claim()?;
        let attachment_id = NetworkAttachmentId::for_workload_attachment(
            tenant.as_str(),
            "krun-network-config-test",
        );
        Ok(OciAttachmentLifecycle::config_from_segment(
            AttachmentBackendKind::Krun,
            self.config.netavark_path.clone(),
            self.config.aardvark_dns_path.clone(),
            &segment,
            &attachment_id,
            &reservation_claim,
        ))
    }

    /// Block-aware placement (MTN6): resolve + reserve the block bridge that will
    /// host `sandbox_id`, growing a new sibling block when the current `/24`s are
    /// full. Fail-closed when the node super-net is exhausted.
    fn place_sandbox_config(
        &self,
        tenant: &nimbus_core::TenantId,
        layout: &OciNetworkLayout,
        sandbox_id: &SandboxId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<OciNetworkConfig> {
        let ports = self.port_lease_coordinator();
        <Self as OciHostManagedAttachmentBackend>::reserve_attachment_config(
            &self.attachment_lifecycle(&ports),
            tenant,
            layout,
            sandbox_id,
            attachment_id,
            reservation_claim,
            OciAttachmentProviderPaths::new(
                self.config.netavark_path.clone(),
                self.config.aardvark_dns_path.clone(),
            ),
        )
    }

    fn release_reserved_launch(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        let reservation_claim = manifest.require_reserved_claim()?;
        let ports = self.port_lease_coordinator();
        let port_compensation = ports.release_never_bound_launch_claim(reservation_claim);
        let fallback_attachment_id = manifest.provision_network_plan.as_ref().map_or_else(
            || default_network_attachment_id(&manifest.handle.id),
            |plan| plan.attachment_id().clone(),
        );
        let attachment_id = manifest
            .network_config
            .as_ref()
            .map_or(&fallback_attachment_id, |config| &config.attachment_id);
        self.attachment_lifecycle(&ports).release_reserved(
            AttachmentBackendKind::Krun,
            crate::backends::oci::network::ReservedNetworkLaunchIdentity::new(
                &manifest.network_layout,
                &manifest.spec.tenant_id,
                &manifest.handle.id,
                attachment_id,
                reservation_claim,
            ),
            port_compensation,
        )
    }

    fn release_unpublished_reserved_launch(
        &self,
        manifest: &KrunSandboxManifest,
        reservations: &ReservedLaunchPorts,
    ) -> Result<()> {
        let reservation_claim = manifest.require_reserved_claim()?;
        let attachment_id = &manifest.require_network_config()?.attachment_id;
        let ports = self.port_lease_coordinator();
        let port_compensation =
            ports.release_unpublished_launch_ports(reservations, reservation_claim);
        self.attachment_lifecycle(&ports).release_reserved(
            AttachmentBackendKind::Krun,
            crate::backends::oci::network::ReservedNetworkLaunchIdentity::new(
                &manifest.network_layout,
                &manifest.spec.tenant_id,
                &manifest.handle.id,
                attachment_id,
                reservation_claim,
            ),
            port_compensation,
        )
    }

    fn remove_tenant_artifacts_sync(&self, tenant_id: &nimbus_core::TenantId) -> Result<()> {
        for root in [&self.config.bundle_root, &self.config.workload_state_root] {
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

    fn resource_quota_manager(&self) -> ResourceQuotaManager {
        ResourceQuotaManager::new(
            self.config.workload_state_root.clone(),
            self.config.resource_quota_policy.clone(),
        )
    }
}

impl SandboxBackend for KrunSandboxBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Krun
    }

    fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<crate::SandboxInspection>> {
        let backend = self.clone();
        let sandbox_id = id.clone();
        Box::pin(async move { backend.inspect_sync(&sandbox_id) })
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
    /// Exact workload execution incarnation persisted before provider effects.
    execution_attempt_id: SandboxExecutionAttemptId,
    spec: SandboxSpec,
    image_metadata: KrunImageMetadata,
    launch_artifact: Option<KrunLaunchArtifact>,
    /// Whether workload-owned artifacts and the exact bundle config are
    /// durably materialized for the reserved provision envelope.
    #[serde(default)]
    provision_prepared: bool,
    bundle_layout: KrunBundleLayout,
    conmon_layout: OciConmonLayout,
    network_layout: OciNetworkLayout,
    /// Complete compiler-issued desired network plan persisted before the
    /// first attachment reservation. This preserves the exact attachment ID
    /// when placement returns an ambiguous failure before publishing a config.
    provision_network_plan: Option<SandboxProvisionNetworkPlan>,
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
    /// Exact execution-only drain and stop progress. Network authority remains
    /// retained until the later release owner completes.
    execution_teardown: teardown::state::KrunExecutionTeardownState,
    /// Strict provider-local attachment detach and release progress.
    ///
    /// This records effect boundaries and compound detached evidence. The
    /// provider command journal remains the sole command-result authority.
    network_teardown: crate::backends::oci::network::HostManagedAttachmentTeardownState,
    egress_proxy: Option<EgressProxyAssignment>,
    conmon_launch: OciConmonLaunchPlan,
    last_exit_code: Option<i32>,
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
        association: NetworkAttachmentSegmentAssociation,
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
    fn require_execution_admission_open(&self, operation: &str) -> Result<()> {
        if self.execution_teardown.admission_is_open() {
            return Ok(());
        }
        Err(SandboxError::OperationFailed {
            message: format!(
                "{operation} for {} is fenced by durable execution drain progress {:?}",
                self.handle.id,
                self.execution_teardown.drain(),
            ),
        })
    }

    fn require_execution_attempt(
        &self,
        expected: &SandboxExecutionAttemptId,
        operation: &str,
    ) -> Result<()> {
        if &self.execution_attempt_id == expected {
            return Ok(());
        }
        Err(SandboxError::InvalidSpec {
            message: format!(
                "{operation} for {} crossed execution attempt {}; durable attempt is {}",
                self.handle.id, expected, self.execution_attempt_id
            ),
        })
    }

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
            | KrunLaunchAuthority::Adopting {
                reservation_claim, ..
            }
            | KrunLaunchAuthority::Adopted { reservation_claim } => Some(reservation_claim),
            KrunLaunchAuthority::PlanOnly
            | KrunLaunchAuthority::ProviderOwned
            | KrunLaunchAuthority::Released => None,
        }
    }

    fn require_reserved_claim(&self) -> Result<&NetworkReservationClaim> {
        match &self.launch_authority {
            KrunLaunchAuthority::Reserved { reservation_claim }
            | KrunLaunchAuthority::Adopting {
                reservation_claim, ..
            } => Ok(reservation_claim),
            phase => Err(SandboxError::OperationFailed {
                message: format!(
                    "krun workload {} requires reserved launch authority before provider adoption, \
                     got {phase:?}",
                    self.handle.id
                ),
            }),
        }
    }

    fn adopting_association(&self) -> Option<&NetworkAttachmentSegmentAssociation> {
        match &self.launch_authority {
            KrunLaunchAuthority::Adopting { association, .. } => Some(association),
            _ => None,
        }
    }

    fn mark_adopted(&mut self) -> Result<NetworkReservationClaim> {
        let claim = self.require_reserved_claim()?.clone();
        self.launch_authority = KrunLaunchAuthority::Adopted {
            reservation_claim: claim.clone(),
        };
        Ok(claim)
    }

    fn mark_adopting(
        &mut self,
        association: NetworkAttachmentSegmentAssociation,
    ) -> Result<NetworkReservationClaim> {
        let claim = self.require_reserved_claim()?.clone();
        let config = self.require_network_config()?;
        if association.reservation_claim() != &claim
            || association.segment_id().as_str() != config.segment_id
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun workload {} cannot persist crossed attachment adoption identity",
                    self.handle.id
                ),
            });
        }
        self.launch_authority = KrunLaunchAuthority::Adopting {
            reservation_claim: claim.clone(),
            association,
        };
        Ok(claim)
    }

    fn permits_provider_teardown(&self) -> bool {
        matches!(
            self.launch_authority,
            KrunLaunchAuthority::Adopted { .. } | KrunLaunchAuthority::ProviderOwned
        )
    }

    fn permits_exact_network_teardown(&self) -> bool {
        self.permits_provider_teardown()
            || matches!(
                self.launch_authority,
                KrunLaunchAuthority::Reserved { .. }
                    | KrunLaunchAuthority::Adopting { .. }
                    | KrunLaunchAuthority::Released
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

#[cfg(test)]
mod tests;
