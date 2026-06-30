use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use super::bundle::{
    ContainerBundleLayout, ContainerBundleMount, ContainerBundleOptions, write_bundle_config,
};
use crate::backend::{SandboxBackend, SandboxBackendKind, SandboxFuture};
use crate::backends::conmon::lifecycle::{
    RuntimeStatusProbe, configured_stop_signal, configured_stop_timeout,
    detect_runtime_status as detect_conmon_runtime_status, ensure_linux_host, now_millis,
    read_exit_code, read_pid, remove_if_exists, restart_backoff_delay,
    restart_policy_allows_restart, run_status_best_effort, run_status_checked, signal_process,
    spawn_background, wait_for_path, wait_for_runtime_state,
};
use crate::backends::conmon::spec_resolve::{resolve_process_spec, resolve_root_spec, slugify};
use crate::backends::oci::buildah::{
    BuildahCli, ImageHealthcheck, MountedRootfsSession, OciExposedPort, OciImageLaunchDefaults,
};
use crate::backends::oci::builder::OciDockerfileBuilder;
use crate::backends::oci::conmon::{
    OciConmonConfig, OciConmonLaunchPlan, OciConmonLayout, build_launch_plan,
};
use crate::backends::oci::egress::EgressProxyRegistry;
use crate::backends::oci::materializer::{
    MaterializedImageRootfs, OciImageMaterializer, PreparedMaterializedImageLaunch,
};
use crate::backends::oci::network::{
    DEFAULT_AARDVARK_DNS_BINARY, DEFAULT_NETAVARK_BINARY, MachinePortProxy,
    OciMachinePortForwarderConfig, OciNetworkConfig, OciNetworkDirectEgress, OciNetworkLayout,
    bridge_gateway_addr, create_persistent_network_namespace, expose_machine_ports,
    remove_persistent_network_namespace, setup_container_network, start_machine_port_proxies,
    teardown_container_network, unexpose_machine_ports,
};
use crate::backends::oci::port_manager::{DEFAULT_MAX_PORTS_PER_TENANT, PortManager};
use crate::backends::oci::resource_quota::ResourceQuotaManager;
use crate::endpoint::{PublishedEndpoint, PublishedEndpointProtocol};
use crate::error::{Result, SandboxError};
use crate::instance::{SandboxHandle, SandboxId, SandboxStatus};
use crate::spec::{
    SandboxOciImageSource, SandboxResourceQuotaPolicy, SandboxRootSpec, SandboxSpec,
    resolve_process_without_image_defaults,
};
use nimbus_egress::EgressPolicy;

const DEFAULT_RUNTIME_PATH: &str = "crun";
const DEFAULT_CONMON_PATH: &str = "conmon";
const DEFAULT_BUILDAH_PATH: &str = "buildah";
const DEFAULT_PUBLISHED_PORT_START: u16 = 15_000;
const DEFAULT_PUBLISHED_PORT_END: u16 = 16_000;
const DEFAULT_START_TIMEOUT_SECS: u64 = 10;
const DEFAULT_STOP_TIMEOUT_SECS: u64 = 5;
const DEFAULT_READINESS_PROBE_TIMEOUT_MILLIS: u64 = 1_000;
const RUNNER_MANIFEST_POINTER_FILE: &str = ".nimbus-container-manifest";

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
            machine_port_forwarder: None,
            start_mode: ContainerStartMode::Execute,
            log_level: "debug".to_owned(),
            start_timeout: Duration::from_secs(DEFAULT_START_TIMEOUT_SECS),
            stop_timeout: Duration::from_secs(DEFAULT_STOP_TIMEOUT_SECS),
        }
    }
}

#[derive(Clone)]
pub struct ContainerSandboxBackend {
    config: ContainerSandboxBackendConfig,
    egress_proxies: EgressProxyRegistry,
    machine_port_proxies: Arc<Mutex<HashMap<SandboxId, Vec<MachinePortProxy>>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedContainerServiceWorkload {
    pub handle: SandboxHandle,
    pub bundle_dir: PathBuf,
}

impl ContainerSandboxBackend {
    pub fn new(config: ContainerSandboxBackendConfig) -> Self {
        Self {
            config,
            egress_proxies: EgressProxyRegistry::new(),
            machine_port_proxies: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn reload_egress_policy(&self, id: &SandboxId, egress: EgressPolicy) -> Result<()> {
        let compiled = egress
            .compile()
            .map_err(|message| SandboxError::InvalidSpec { message })?;
        let Some(mut manifest) = self.read_manifest(id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: id.as_str().to_owned(),
            });
        };
        if manifest.start_mode != ContainerStartMode::Execute {
            return Err(SandboxError::InvalidSpec {
                message: "container egress live reload requires execute-mode sandbox".to_owned(),
            });
        }
        manifest.spec.egress = compiled.policy().clone();
        self.ensure_egress_proxy_running(&manifest)?;
        self.egress_proxies.reload(id, compiled)?;
        self.write_manifest(&manifest)
    }

    fn remove_tenant_artifacts_sync(&self, tenant_id: &nimbus_core::TenantId) -> Result<()> {
        for root in [&self.config.bundle_root, &self.config.state_root] {
            crate::artifact_paths::remove_tenant_root(root, tenant_id).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!(
                        "failed to remove container sandbox tenant artifacts for {} under {}: {error}",
                        tenant_id,
                        root.display()
                    ),
                }
            })?;
        }
        Ok(())
    }

    fn port_manager(&self) -> PortManager {
        PortManager::new(
            &self.config.state_root,
            self.config.published_port_range.clone(),
        )
        .with_max_ports_per_tenant(self.config.max_published_ports_per_tenant)
    }

    fn resource_quota_manager(&self) -> ResourceQuotaManager {
        ResourceQuotaManager::new(
            self.config.state_root.clone(),
            self.config.resource_quota_policy.clone(),
        )
    }

    fn network_config(&self) -> OciNetworkConfig {
        OciNetworkConfig {
            netavark_path: self.config.netavark_path.clone(),
            aardvark_dns_path: self.config.aardvark_dns_path.clone(),
            network_name: self.config.network_name.clone(),
            network_interface: self.config.network_interface.clone(),
            network_subnet: self.config.network_subnet.clone(),
            direct_egress: OciNetworkDirectEgress::Deny,
        }
    }

    fn start_sync(&self, spec: SandboxSpec) -> Result<SandboxHandle> {
        let launch_plan = self.plan_start(&spec)?;
        self.finish_start(launch_plan)
    }

    pub fn prepare_plan_only_service_workload(
        &self,
        spec: SandboxSpec,
    ) -> Result<PreparedContainerServiceWorkload> {
        if self.config.start_mode != ContainerStartMode::PlanOnly {
            return Err(SandboxError::InvalidSpec {
                message: "container service workload materialization requires plan-only mode"
                    .to_owned(),
            });
        }
        if spec.service_name().is_none() {
            return Err(SandboxError::InvalidSpec {
                message:
                    "container service workload materialization requires service owner metadata"
                        .to_owned(),
            });
        }
        let mut launch_plan = self.plan_start(&spec)?;
        self.attach_runner_owned_egress_proxy(&mut launch_plan)?;
        self.write_runner_manifest_pointer(&launch_plan.manifest)?;
        let bundle_dir = launch_plan.manifest.bundle_layout.bundle_dir.clone();
        let handle = self.finish_start(launch_plan)?;
        Ok(PreparedContainerServiceWorkload { handle, bundle_dir })
    }

    pub fn mark_plan_only_service_workload_stopped(
        &self,
        id: &SandboxId,
    ) -> Result<Option<SandboxHandle>> {
        self.update_plan_only_service_workload_status(id, SandboxStatus::Stopped)
    }

    pub fn refresh_plan_only_service_workload_status(
        &self,
        id: &SandboxId,
        status: SandboxStatus,
    ) -> Result<Option<SandboxHandle>> {
        self.update_plan_only_service_workload_status(id, status)
    }

    fn update_plan_only_service_workload_status(
        &self,
        id: &SandboxId,
        status: SandboxStatus,
    ) -> Result<Option<SandboxHandle>> {
        if self.config.start_mode != ContainerStartMode::PlanOnly {
            return Err(SandboxError::OperationFailed {
                message: "container service workload status refresh requires plan-only mode"
                    .to_owned(),
            });
        }
        let Some(mut manifest) = self.read_manifest(id)? else {
            return Ok(None);
        };
        synchronize_handle_status(&mut manifest, status);
        if status == SandboxStatus::Stopped {
            manifest.next_restart_at_millis = None;
            self.cleanup_manifest_launch_artifacts(&manifest)?;
            manifest.launch_artifact = None;
        }
        self.write_manifest(&manifest)?;
        Ok(Some(manifest.handle))
    }

    fn attach_runner_owned_egress_proxy(&self, launch_plan: &mut ContainerStartPlan) -> Result<()> {
        if launch_plan.manifest.egress_proxy.is_some() {
            return Ok(());
        }
        let egress_proxy = self.allocate_egress_proxy(&launch_plan.manifest.spec)?;
        write_bundle_config(
            &launch_plan.manifest.bundle_layout,
            &hostname_for(&launch_plan.manifest.spec),
            &launch_plan.manifest.spec,
            launch_plan.manifest.image_metadata.user.as_deref(),
            Some(launch_plan.manifest.network_layout.netns_path.as_path()),
            &ContainerBundleOptions {
                additional_mounts: container_tenant_volume_mounts(
                    &self.config.state_root,
                    &launch_plan.manifest.spec,
                )?,
                egress_proxy_url: Some(egress_proxy.proxy_url()),
            },
        )?;
        launch_plan.manifest.egress_proxy = Some(egress_proxy);
        Ok(())
    }

    fn write_runner_manifest_pointer(&self, manifest: &ContainerSandboxManifest) -> Result<()> {
        let pointer_path = manifest
            .bundle_layout
            .bundle_dir
            .join(RUNNER_MANIFEST_POINTER_FILE);
        std::fs::write(
            &pointer_path,
            format!("{}\n", manifest.conmon_layout.manifest_path.display()),
        )
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to write container runner manifest pointer {}: {error}",
                pointer_path.display()
            ),
        })
    }

    fn finish_start(&self, launch_plan: ContainerStartPlan) -> Result<SandboxHandle> {
        let mut manifest = launch_plan.manifest;
        match self.config.start_mode {
            ContainerStartMode::PlanOnly => {
                manifest.last_exit_code = None;
                manifest.restart_count = 0;
                manifest.next_restart_at_millis = None;
                manifest.shutdown_requested = false;
                self.write_manifest(&manifest)?;
                Ok(manifest.handle)
            }
            ContainerStartMode::Execute => self.execute_start(&manifest).inspect_err(|_| {
                let _ = self.cleanup_manifest_launch_artifacts(&manifest);
            }),
        }
    }

    fn inspect_sync(&self, id: &SandboxId) -> Result<Option<SandboxHandle>> {
        let Some(mut manifest) = self.read_manifest(id)? else {
            return Ok(None);
        };
        let restarted = self.config.start_mode == ContainerStartMode::Execute
            && self.maybe_restart_after_exit(&mut manifest)?;
        let status = match self.config.start_mode {
            ContainerStartMode::PlanOnly => manifest.status,
            ContainerStartMode::Execute if restarted => manifest.status,
            ContainerStartMode::Execute => self.detect_runtime_status(&manifest)?,
        };
        if self.config.start_mode == ContainerStartMode::Execute
            && !restarted
            && manifest.conmon_layout.exit_status_file.exists()
        {
            manifest.last_exit_code =
                Some(read_exit_code(&manifest.conmon_layout.exit_status_file)?);
            let _ = self.release_execution_artifacts(&mut manifest);
        }
        synchronize_handle_status(&mut manifest, status);
        self.write_manifest(&manifest)?;
        Ok(Some(manifest.handle))
    }

    fn stop_sync(&self, id: &SandboxId) -> Result<()> {
        let Some(mut manifest) = self.read_manifest(id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: id.as_str().to_owned(),
            });
        };

        match self.config.start_mode {
            ContainerStartMode::PlanOnly => {
                manifest.shutdown_requested = true;
                manifest.last_exit_code = Some(0);
                manifest.next_restart_at_millis = None;
                synchronize_handle_status(&mut manifest, SandboxStatus::Stopped);
                self.cleanup_manifest_launch_artifacts(&manifest)?;
                manifest.launch_artifact = None;
                self.write_manifest(&manifest)
            }
            ContainerStartMode::Execute => self.execute_stop(&mut manifest),
        }
    }

    pub(crate) fn plan_start(&self, spec: &SandboxSpec) -> Result<ContainerStartPlan> {
        let sandbox_id = next_sandbox_id(spec.display_name());
        match &spec.root {
            SandboxRootSpec::Rootfs(_) => self.plan_start_with_id(spec, &sandbox_id, None, None),
            SandboxRootSpec::OciImage(image) => {
                self.resource_quota_manager().ensure_launch_quota(spec)?;
                let prepared_launch =
                    self.prepare_oci_image_start(spec, &sandbox_id, &image.source)?;
                self.plan_start_with_materialized_image(spec, &sandbox_id, prepared_launch)
            }
        }
    }

    fn plan_start_with_materialized_image(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        prepared_launch: PreparedMaterializedImageLaunch,
    ) -> Result<ContainerStartPlan> {
        self.plan_start_with_id(
            spec,
            sandbox_id,
            Some(&prepared_launch.launch_defaults),
            Some(ContainerLaunchArtifact::Rootfs(prepared_launch.artifact)),
        )
    }

    fn plan_start_with_id(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        launch_defaults: Option<&OciImageLaunchDefaults>,
        launch_artifact: Option<ContainerLaunchArtifact>,
    ) -> Result<ContainerStartPlan> {
        if spec.backend != SandboxBackendKind::Container {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "container backend cannot lower sandbox spec for backend {:?}",
                    spec.backend
                ),
            });
        }

        let resolved_launch = resolve_start_spec(spec, launch_defaults)?;
        let mut resolved_spec = resolved_launch.spec.clone();
        self.resource_quota_manager()
            .ensure_launch_quota(&resolved_spec)?;
        resolved_spec.port_bindings.extend(
            self.port_manager().allocate_missing_bindings_for_tenant(
                &resolved_spec.tenant_id,
                &resolved_spec.port_bindings,
                &resolved_launch.image_metadata.exposed_ports,
            )?,
        );
        let egress_proxy = (self.config.start_mode == ContainerStartMode::Execute)
            .then(|| self.allocate_egress_proxy(&resolved_spec))
            .transpose()?;
        let network_layout = OciNetworkLayout::new(
            &self.config.state_root,
            &resolved_spec.tenant_id,
            sandbox_id,
        );
        let bundle_layout = ContainerBundleLayout::new(crate::artifact_paths::bundle_dir(
            &self.config.bundle_root,
            &resolved_launch.spec.tenant_id,
            sandbox_id,
        ));
        write_bundle_config(
            &bundle_layout,
            &hostname_for(&resolved_spec),
            &resolved_spec,
            resolved_launch.image_metadata.user.as_deref(),
            Some(network_layout.netns_path.as_path()),
            &ContainerBundleOptions {
                additional_mounts: container_tenant_volume_mounts(
                    &self.config.state_root,
                    &resolved_spec,
                )?,
                egress_proxy_url: egress_proxy
                    .as_ref()
                    .map(ContainerEgressProxyManifest::proxy_url),
            },
        )?;

        let conmon_layout = OciConmonLayout::new_for_tenant(
            &self.config.state_root,
            &resolved_launch.spec.tenant_id,
            sandbox_id,
        );
        conmon_layout
            .ensure_directories()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to create container state directories under {}: {error}",
                    self.config.state_root.display()
                ),
            })?;
        network_layout.ensure_directories()?;

        let conmon_launch = build_launch_plan(
            &OciConmonConfig {
                conmon_path: self.config.conmon_path.clone(),
                runtime_path: self.config.runtime_path.clone(),
                buildah_path: self.config.buildah_path.clone(),
                use_buildah_unshare: launch_artifact
                    .as_ref()
                    .is_some_and(ContainerLaunchArtifact::uses_mount_session_unshare)
                    && self.config.use_buildah_unshare,
                log_level: self.config.log_level.clone(),
                log_size_max_bytes: resolved_spec.resources.log_limit_bytes,
            },
            &conmon_layout,
            sandbox_id,
            resolved_launch.spec.display_name(),
            &bundle_layout.bundle_dir,
            launch_artifact
                .as_ref()
                .and_then(ContainerLaunchArtifact::mount_session_name),
            &[],
        );

        let handle = SandboxHandle::new(
            resolved_spec.tenant_id.clone(),
            sandbox_id.clone(),
            resolved_spec.display_name().to_owned(),
            SandboxBackendKind::Container,
            SandboxStatus::Starting,
            visible_published_endpoints(
                self.config.start_mode,
                &resolved_spec,
                SandboxStatus::Starting,
            ),
        );

        Ok(ContainerStartPlan {
            manifest: ContainerSandboxManifest {
                handle,
                spec: resolved_spec,
                image_metadata: resolved_launch.image_metadata,
                launch_artifact,
                bundle_layout,
                conmon_layout,
                network_layout,
                egress_proxy,
                conmon_launch,
                runner_config: ContainerRunnerExecutionConfig::from_backend_config(&self.config),
                last_exit_code: None,
                restart_count: 0,
                next_restart_at_millis: None,
                start_mode: self.config.start_mode,
                shutdown_requested: false,
                status: SandboxStatus::Starting,
            },
        })
    }

    fn execute_start(&self, manifest: &ContainerSandboxManifest) -> Result<SandboxHandle> {
        let mut manifest = manifest.clone();
        if let Err(error) = self.launch_manifest(&mut manifest, true) {
            let _ = self.release_execution_artifacts(&mut manifest);
            return Err(error);
        }
        Ok(manifest.handle)
    }

    fn maybe_restart_after_exit(&self, manifest: &mut ContainerSandboxManifest) -> Result<bool> {
        match mark_restart_decision_after_exit(manifest, now_millis()?)? {
            ContainerRestartDecision::NotRestarting => Ok(false),
            ContainerRestartDecision::WaitingForBackoff => Ok(true),
            ContainerRestartDecision::RestartNow => {
                self.reset_runtime_for_restart(manifest)?;
                self.launch_manifest(manifest, false)?;
                Ok(true)
            }
        }
    }

    fn launch_manifest(
        &self,
        manifest: &mut ContainerSandboxManifest,
        clear_last_exit_code: bool,
    ) -> Result<()> {
        ensure_linux_host("container")?;
        self.configure_network(manifest)?;
        self.ensure_egress_proxy_running(manifest)?;
        spawn_background(&manifest.conmon_launch.create_command)?;
        let runtime_state = wait_for_runtime_state(
            &manifest.conmon_launch.state_command,
            self.config.start_timeout,
        )?;
        if runtime_state != "running" {
            run_status_checked(&manifest.conmon_launch.start_command)?;
        }

        manifest.shutdown_requested = false;
        manifest.next_restart_at_millis = None;
        if clear_last_exit_code {
            manifest.last_exit_code = None;
        }
        synchronize_handle_status(manifest, SandboxStatus::Starting);
        self.write_manifest(manifest)
    }

    fn reset_runtime_for_restart(&self, manifest: &ContainerSandboxManifest) -> Result<()> {
        let mut errors = Vec::new();
        if let Err(error) = self.stop_egress_proxy(&manifest.handle.id) {
            errors.push(error.to_string());
        }
        if let Err(error) = self.stop_machine_port_proxies(&manifest.handle.id) {
            errors.push(error.to_string());
        }
        if let Err(error) = run_status_checked(&manifest.conmon_launch.delete_command) {
            errors.push(error.to_string());
        }
        if let Err(error) = teardown_container_network(
            &manifest.network_layout,
            &self.network_config(),
            &manifest.handle.id,
            manifest.spec.display_name(),
            &hostname_for(&manifest.spec),
            &manifest.spec.port_bindings,
            self.config.machine_port_forwarder.as_ref(),
        ) {
            errors.push(error.to_string());
        }
        if let Err(error) = remove_persistent_network_namespace(&manifest.network_layout.netns_path)
        {
            errors.push(error.to_string());
        }
        if let Some(forwarder) = self.config.machine_port_forwarder.as_ref() {
            let _ = unexpose_machine_ports(forwarder, &manifest.spec.port_bindings);
        }
        remove_if_exists(&manifest.conmon_layout.exit_status_file)?;
        remove_if_exists(&manifest.conmon_layout.pidfile)?;
        remove_if_exists(&manifest.conmon_layout.conmon_pidfile)?;
        if errors.is_empty() {
            Ok(())
        } else {
            Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to reset container sandbox {} for restart: {}",
                    manifest.handle.id,
                    errors.join("; ")
                ),
            })
        }
    }

    fn execute_stop(&self, manifest: &mut ContainerSandboxManifest) -> Result<()> {
        if manifest.conmon_layout.exit_status_file.exists() {
            manifest.shutdown_requested = true;
            manifest.last_exit_code =
                Some(read_exit_code(&manifest.conmon_layout.exit_status_file)?);
            synchronize_handle_status(manifest, SandboxStatus::Stopped);
            self.release_execution_artifacts(manifest)?;
            return self.write_manifest(manifest);
        }

        manifest.shutdown_requested = true;
        let pid = read_pid(&manifest.conmon_layout.pidfile)?;
        let stop_signal = configured_stop_signal(manifest.image_metadata.stop_signal.as_deref());
        signal_process(&stop_signal, pid)?;
        let stop_timeout = configured_stop_timeout(&manifest.spec, self.config.stop_timeout);
        if !wait_for_path(&manifest.conmon_layout.exit_status_file, stop_timeout) {
            signal_process("KILL", pid)?;
            if !wait_for_path(&manifest.conmon_layout.exit_status_file, stop_timeout) {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "sandbox {} did not write an exit file after TERM/KILL",
                        manifest.handle.id
                    ),
                });
            }
        }

        manifest.last_exit_code = Some(read_exit_code(&manifest.conmon_layout.exit_status_file)?);
        synchronize_handle_status(manifest, SandboxStatus::Stopped);
        self.release_execution_artifacts(manifest)?;
        self.write_manifest(manifest)
    }

    fn detect_runtime_status(&self, manifest: &ContainerSandboxManifest) -> Result<SandboxStatus> {
        detect_conmon_runtime_status(
            RuntimeStatusProbe {
                exit_status_file: &manifest.conmon_layout.exit_status_file,
                state_command: &manifest.conmon_launch.state_command,
                pidfile: &manifest.conmon_layout.pidfile,
                shutdown_requested: manifest.shutdown_requested,
                current_status: manifest.status,
            },
            || {
                self.ensure_egress_proxy_running(manifest)?;
                Ok(running_status(manifest))
            },
        )
    }

    fn prepare_image_launch(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        image_reference: &str,
    ) -> Result<PreparedMaterializedImageLaunch> {
        OciImageMaterializer::for_tenant_sandbox(
            &self.config.state_root,
            &spec.tenant_id,
            sandbox_id,
        )
        .prepare_image_launch(sandbox_id, image_reference, &spec.process)
    }

    fn prepare_built_image_launch(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        image_name: &str,
        dockerfile_path: &Path,
        context_path: &Path,
    ) -> Result<PreparedMaterializedImageLaunch> {
        OciDockerfileBuilder::for_tenant_sandbox(
            &self.config.state_root,
            &spec.tenant_id,
            sandbox_id,
        )
        .prepare_built_image_launch(
            sandbox_id,
            image_name,
            dockerfile_path,
            context_path,
            &spec.process,
        )
    }

    fn prepare_oci_image_start(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        source: &SandboxOciImageSource,
    ) -> Result<PreparedMaterializedImageLaunch> {
        match source {
            SandboxOciImageSource::Reference(reference) => {
                self.prepare_image_launch(spec, sandbox_id, &reference.reference)
            }
            SandboxOciImageSource::Build(build) => self.prepare_built_image_launch(
                spec,
                sandbox_id,
                &build.image_name,
                &build.dockerfile_path,
                &build.context_path,
            ),
        }
    }

    fn cleanup_manifest_launch_artifacts(&self, manifest: &ContainerSandboxManifest) -> Result<()> {
        let Some(artifact) = manifest.launch_artifact.as_ref() else {
            return Ok(());
        };
        match artifact {
            ContainerLaunchArtifact::MountedRootfs(session) => {
                BuildahCli::new(&self.config.buildah_path)
                    .with_unshare(self.config.use_buildah_unshare)
                    .cleanup_rootfs_session(&session.session_name)
            }
            ContainerLaunchArtifact::Rootfs(rootfs) => {
                if !rootfs.rootfs_path.exists() {
                    return Ok(());
                }
                std::fs::remove_dir_all(&rootfs.rootfs_path).map_err(|error| {
                    SandboxError::OperationFailed {
                        message: format!(
                            "failed to remove materialized rootfs {}: {error}",
                            rootfs.rootfs_path.display()
                        ),
                    }
                })
            }
        }
    }

    fn configure_network(&self, manifest: &ContainerSandboxManifest) -> Result<()> {
        create_persistent_network_namespace(&manifest.network_layout.netns_path)?;
        let assigned_ips = match setup_container_network(
            &manifest.network_layout,
            &self.network_config(),
            &manifest.handle.id,
            manifest.spec.display_name(),
            &hostname_for(&manifest.spec),
            &manifest.spec.port_bindings,
            self.config.machine_port_forwarder.as_ref(),
        ) {
            Ok(assigned_ips) => assigned_ips,
            Err(error) => {
                let _ = remove_persistent_network_namespace(&manifest.network_layout.netns_path);
                return Err(error);
            }
        };
        if let Some(forwarder) = self.config.machine_port_forwarder.as_ref() {
            if let Err(error) = self.ensure_machine_port_proxies_running(
                &manifest.handle.id,
                &assigned_ips,
                manifest,
            ) {
                let _ = teardown_container_network(
                    &manifest.network_layout,
                    &self.network_config(),
                    &manifest.handle.id,
                    manifest.spec.display_name(),
                    &hostname_for(&manifest.spec),
                    &manifest.spec.port_bindings,
                    self.config.machine_port_forwarder.as_ref(),
                );
                let _ = remove_persistent_network_namespace(&manifest.network_layout.netns_path);
                return Err(error);
            }
            if let Err(error) = expose_machine_ports(forwarder, &manifest.spec.port_bindings) {
                let _ = self.stop_machine_port_proxies(&manifest.handle.id);
                let _ = teardown_container_network(
                    &manifest.network_layout,
                    &self.network_config(),
                    &manifest.handle.id,
                    manifest.spec.display_name(),
                    &hostname_for(&manifest.spec),
                    &manifest.spec.port_bindings,
                    self.config.machine_port_forwarder.as_ref(),
                );
                let _ = remove_persistent_network_namespace(&manifest.network_layout.netns_path);
                return Err(error);
            }
        }
        Ok(())
    }

    fn allocate_egress_proxy(&self, spec: &SandboxSpec) -> Result<ContainerEgressProxyManifest> {
        let network_config = self.network_config();
        let gateway = bridge_gateway_addr(&network_config)?;
        let port = self
            .port_manager()
            .allocate_internal_host_port(&spec.port_bindings)?;
        Ok(ContainerEgressProxyManifest {
            host: gateway.to_string(),
            port,
        })
    }

    fn ensure_egress_proxy_running(&self, manifest: &ContainerSandboxManifest) -> Result<()> {
        let Some(egress_proxy) = manifest.egress_proxy.as_ref() else {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container sandbox {} has no egress proxy assignment",
                    manifest.handle.id
                ),
            });
        };
        let bind_addr = egress_proxy.bind_addr()?;
        self.egress_proxies
            .ensure_running(&manifest.handle.id, &manifest.spec.egress, bind_addr)
    }

    fn ensure_machine_port_proxies_running(
        &self,
        id: &SandboxId,
        assigned_ips: &[Ipv4Addr],
        manifest: &ContainerSandboxManifest,
    ) -> Result<()> {
        let mut proxies =
            self.machine_port_proxies
                .lock()
                .map_err(|_| SandboxError::OperationFailed {
                    message: "container machine port proxy registry lock is poisoned".to_owned(),
                })?;
        if proxies.contains_key(id) {
            return Ok(());
        }
        proxies.insert(
            id.clone(),
            start_machine_port_proxies(assigned_ips, &manifest.spec.port_bindings)?,
        );
        Ok(())
    }

    fn release_execution_artifacts(&self, manifest: &mut ContainerSandboxManifest) -> Result<()> {
        let mut errors = Vec::new();
        if let Err(error) = self.stop_egress_proxy(&manifest.handle.id) {
            errors.push(error.to_string());
        }
        if let Err(error) = self.stop_machine_port_proxies(&manifest.handle.id) {
            errors.push(error.to_string());
        }
        let _ = run_status_best_effort(&manifest.conmon_launch.delete_command);
        if let Err(error) = teardown_container_network(
            &manifest.network_layout,
            &self.network_config(),
            &manifest.handle.id,
            manifest.spec.display_name(),
            &hostname_for(&manifest.spec),
            &manifest.spec.port_bindings,
            self.config.machine_port_forwarder.as_ref(),
        ) {
            errors.push(error.to_string());
        }
        if let Err(error) = remove_persistent_network_namespace(&manifest.network_layout.netns_path)
        {
            errors.push(error.to_string());
        }
        if let Some(forwarder) = self.config.machine_port_forwarder.as_ref() {
            let _ = unexpose_machine_ports(forwarder, &manifest.spec.port_bindings);
        }
        if let Err(error) = self.cleanup_manifest_launch_artifacts(manifest) {
            errors.push(error.to_string());
        }
        manifest.launch_artifact = None;
        if errors.is_empty() {
            Ok(())
        } else {
            Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to clean up container sandbox {}: {}",
                    manifest.handle.id,
                    errors.join("; ")
                ),
            })
        }
    }

    fn stop_egress_proxy(&self, id: &SandboxId) -> Result<()> {
        self.egress_proxies.stop(id)
    }

    fn stop_machine_port_proxies(&self, id: &SandboxId) -> Result<()> {
        let mut proxies =
            self.machine_port_proxies
                .lock()
                .map_err(|_| SandboxError::OperationFailed {
                    message: "container machine port proxy registry lock is poisoned".to_owned(),
                })?;
        proxies.remove(id);
        Ok(())
    }

    fn read_manifest(&self, id: &SandboxId) -> Result<Option<ContainerSandboxManifest>> {
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
        Ok(Some(manifest))
    }

    fn write_manifest(&self, manifest: &ContainerSandboxManifest) -> Result<()> {
        std::fs::create_dir_all(&manifest.conmon_layout.container_state_dir).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!(
                    "failed to create manifest directory {}: {error}",
                    manifest.conmon_layout.container_state_dir.display()
                ),
            }
        })?;
        let rendered =
            serde_json::to_vec_pretty(manifest).map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to serialize sandbox manifest: {error}"),
            })?;
        std::fs::write(&manifest.conmon_layout.manifest_path, rendered).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!(
                    "failed to write sandbox manifest {}: {error}",
                    manifest.conmon_layout.manifest_path.display()
                ),
            }
        })
    }
}

pub fn run_prepared_container_service_workload(bundle_dir: impl AsRef<Path>) -> Result<()> {
    let bundle_dir = bundle_dir.as_ref();
    let manifest_path = read_runner_manifest_pointer(bundle_dir)?;
    let mut manifest = read_runner_manifest(&manifest_path)?;
    if manifest.start_mode != ContainerStartMode::PlanOnly {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "container runner expected a prepared plan-only workload manifest, got {:?}",
                manifest.start_mode
            ),
        });
    }
    if manifest.bundle_layout.bundle_dir != bundle_dir {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "container runner bundle {} does not match prepared manifest bundle {}",
                bundle_dir.display(),
                manifest.bundle_layout.bundle_dir.display()
            ),
        });
    }
    let backend = ContainerSandboxBackend::new(manifest.runner_config.to_backend_config());
    backend.launch_manifest(&mut manifest, true)?;
    let exit_code = wait_for_container_runner_exit(&manifest)?;
    if exit_code != 0 {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "container workload {} exited with status {exit_code}",
                manifest.handle.id
            ),
        });
    }
    Ok(())
}

fn wait_for_container_runner_exit(manifest: &ContainerSandboxManifest) -> Result<i32> {
    while !manifest.conmon_layout.exit_status_file.exists() {
        std::thread::sleep(Duration::from_millis(200));
    }
    read_exit_code(&manifest.conmon_layout.exit_status_file)
}

fn read_runner_manifest_pointer(bundle_dir: &Path) -> Result<PathBuf> {
    let pointer_path = bundle_dir.join(RUNNER_MANIFEST_POINTER_FILE);
    let contents =
        std::fs::read_to_string(&pointer_path).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to read container runner manifest pointer {}: {error}",
                pointer_path.display()
            ),
        })?;
    let path = contents.trim();
    if path.is_empty() {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "container runner manifest pointer {} is empty",
                pointer_path.display()
            ),
        });
    }
    Ok(PathBuf::from(path))
}

fn read_runner_manifest(manifest_path: &Path) -> Result<ContainerSandboxManifest> {
    let contents = std::fs::read(manifest_path).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to read container runner manifest {}: {error}",
            manifest_path.display()
        ),
    })?;
    serde_json::from_slice(&contents).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to parse container runner manifest {}: {error}",
            manifest_path.display()
        ),
    })
}

fn container_tenant_volume_mounts(
    state_root: &Path,
    spec: &SandboxSpec,
) -> Result<Vec<ContainerBundleMount>> {
    crate::spec::validate_sandbox_mounts(&spec.mounts)
        .map_err(|message| SandboxError::InvalidSpec { message })?;
    let mut mounts = Vec::new();
    for mount in &spec.mounts {
        let destination = mount.destination.to_string_lossy().into_owned();
        let volume_name = mount
            .tenant_volume_name()
            .ok_or_else(|| SandboxError::InvalidSpec {
                message: "unsupported container sandbox mount source".to_owned(),
            })?;
        let source =
            crate::artifact_paths::tenant_volume_dir(state_root, &spec.tenant_id, volume_name);
        std::fs::create_dir_all(&source).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to create tenant volume {} for sandbox {} under {}: {error}",
                volume_name,
                spec.display_name(),
                source.display()
            ),
        })?;
        mounts.push(ContainerBundleMount {
            destination,
            source,
            options: tenant_volume_mount_options(mount.read_only),
        });
    }
    Ok(mounts)
}

fn tenant_volume_mount_options(read_only: bool) -> Vec<String> {
    vec![
        "rbind".to_owned(),
        if read_only { "ro" } else { "rw" }.to_owned(),
        "nosuid".to_owned(),
        "nodev".to_owned(),
    ]
}

impl SandboxBackend for ContainerSandboxBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
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

    fn reload_egress_policy(&self, id: &SandboxId, egress: EgressPolicy) -> SandboxFuture<()> {
        let backend = self.clone();
        let sandbox_id = id.clone();
        Box::pin(async move {
            ContainerSandboxBackend::reload_egress_policy(&backend, &sandbox_id, egress)
        })
    }

    fn remove_tenant_artifacts(&self, tenant_id: nimbus_core::TenantId) -> SandboxFuture<()> {
        let backend = self.clone();
        Box::pin(async move { backend.remove_tenant_artifacts_sync(&tenant_id) })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContainerStartPlan {
    manifest: ContainerSandboxManifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ContainerSandboxManifest {
    handle: SandboxHandle,
    spec: SandboxSpec,
    image_metadata: ContainerImageMetadata,
    launch_artifact: Option<ContainerLaunchArtifact>,
    bundle_layout: ContainerBundleLayout,
    conmon_layout: OciConmonLayout,
    network_layout: OciNetworkLayout,
    egress_proxy: Option<ContainerEgressProxyManifest>,
    conmon_launch: OciConmonLaunchPlan,
    #[serde(default)]
    runner_config: ContainerRunnerExecutionConfig,
    last_exit_code: Option<i32>,
    #[serde(default)]
    restart_count: u32,
    #[serde(default)]
    next_restart_at_millis: Option<u64>,
    start_mode: ContainerStartMode,
    shutdown_requested: bool,
    status: SandboxStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ContainerRunnerExecutionConfig {
    netavark_path: PathBuf,
    aardvark_dns_path: PathBuf,
    network_name: String,
    network_interface: String,
    network_subnet: String,
    machine_port_forwarder: Option<OciMachinePortForwarderConfig>,
}

impl ContainerRunnerExecutionConfig {
    fn from_backend_config(config: &ContainerSandboxBackendConfig) -> Self {
        Self {
            netavark_path: config.netavark_path.clone(),
            aardvark_dns_path: config.aardvark_dns_path.clone(),
            network_name: config.network_name.clone(),
            network_interface: config.network_interface.clone(),
            network_subnet: config.network_subnet.clone(),
            machine_port_forwarder: config.machine_port_forwarder.clone(),
        }
    }

    fn to_backend_config(&self) -> ContainerSandboxBackendConfig {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ContainerEgressProxyManifest {
    host: String,
    port: u16,
}

impl ContainerEgressProxyManifest {
    fn proxy_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    fn bind_addr(&self) -> Result<SocketAddr> {
        let host = self
            .host
            .parse::<IpAddr>()
            .map_err(|_| SandboxError::InvalidSpec {
                message: format!(
                    "container egress proxy host {:?} must be an IP address",
                    self.host
                ),
            })?;
        Ok(SocketAddr::new(host, self.port))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContainerResolvedLaunchSpec {
    spec: SandboxSpec,
    image_metadata: ContainerImageMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum ContainerLaunchArtifact {
    MountedRootfs(MountedRootfsSession),
    Rootfs(MaterializedImageRootfs),
}

impl ContainerLaunchArtifact {
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct ContainerImageMetadata {
    user: Option<String>,
    stop_signal: Option<String>,
    healthcheck: Option<ImageHealthcheck>,
    labels: BTreeMap<String, String>,
    exposed_ports: Vec<OciExposedPort>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadinessProbeTarget {
    Tcp(SocketAddr),
    Http(SocketAddr),
}

fn next_sandbox_id(name: &str) -> SandboxId {
    SandboxId::new(format!(
        "{}-{}",
        slugify(name),
        Ulid::new().to_string().to_ascii_lowercase()
    ))
}

fn hostname_for(spec: &SandboxSpec) -> String {
    let slug = slugify(spec.display_name());
    if slug.is_empty() {
        "nimbus-container".to_owned()
    } else {
        slug
    }
}

fn resolve_start_spec(
    spec: &SandboxSpec,
    launch_defaults: Option<&OciImageLaunchDefaults>,
) -> Result<ContainerResolvedLaunchSpec> {
    let Some(launch_defaults) = launch_defaults else {
        let mut resolved_spec = spec.clone();
        resolved_spec.process = resolve_process_without_image_defaults(&spec.process)?;
        let process_user = resolved_spec.process.user.clone();
        return Ok(ContainerResolvedLaunchSpec {
            spec: resolved_spec,
            image_metadata: ContainerImageMetadata {
                user: process_user,
                ..ContainerImageMetadata::default()
            },
        });
    };

    let mut resolved_spec = spec.clone();
    resolved_spec.root = resolve_root_spec(&spec.root, &launch_defaults.rootfs);
    resolved_spec.process = resolve_process_spec(&spec.process, &launch_defaults.process);

    Ok(ContainerResolvedLaunchSpec {
        spec: resolved_spec,
        image_metadata: ContainerImageMetadata {
            user: launch_defaults.user.clone(),
            stop_signal: launch_defaults.stop_signal.clone(),
            healthcheck: launch_defaults.healthcheck.clone(),
            labels: launch_defaults.labels.clone(),
            exposed_ports: launch_defaults.exposed_ports.clone(),
        },
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContainerRestartDecision {
    NotRestarting,
    WaitingForBackoff,
    RestartNow,
}

fn mark_restart_decision_after_exit(
    manifest: &mut ContainerSandboxManifest,
    now_millis: u64,
) -> Result<ContainerRestartDecision> {
    if manifest.shutdown_requested || !manifest.conmon_layout.exit_status_file.exists() {
        return Ok(ContainerRestartDecision::NotRestarting);
    }

    let exit_code = read_exit_code(&manifest.conmon_layout.exit_status_file)?;
    if !restart_policy_allows_restart(
        manifest.spec.lifecycle.restart_policy,
        exit_code,
        manifest.restart_count,
    ) {
        return Ok(ContainerRestartDecision::NotRestarting);
    }

    manifest.last_exit_code = Some(exit_code);
    let next_restart_at_millis = manifest.next_restart_at_millis.get_or_insert_with(|| {
        now_millis.saturating_add(restart_backoff_delay(manifest.restart_count).as_millis() as u64)
    });
    if now_millis < *next_restart_at_millis {
        synchronize_handle_status(manifest, SandboxStatus::Starting);
        return Ok(ContainerRestartDecision::WaitingForBackoff);
    }

    manifest.restart_count += 1;
    manifest.next_restart_at_millis = None;
    synchronize_handle_status(manifest, SandboxStatus::Starting);
    Ok(ContainerRestartDecision::RestartNow)
}

fn running_status(manifest: &ContainerSandboxManifest) -> SandboxStatus {
    match readiness_probe_target(manifest) {
        Some(target) if probe_target_ready(target, readiness_probe_timeout(manifest)) => {
            SandboxStatus::Ready
        }
        Some(_)
            if matches!(
                manifest.status,
                SandboxStatus::Ready | SandboxStatus::NotReady
            ) =>
        {
            SandboxStatus::NotReady
        }
        Some(_) => SandboxStatus::Starting,
        None => SandboxStatus::Ready,
    }
}

fn readiness_probe_target(manifest: &ContainerSandboxManifest) -> Option<ReadinessProbeTarget> {
    let endpoints = published_endpoints(&manifest.spec);
    endpoints
        .iter()
        .find_map(|endpoint| match endpoint.protocol {
            PublishedEndpointProtocol::Http => Some(ReadinessProbeTarget::Http(endpoint.address)),
            PublishedEndpointProtocol::Https => Some(ReadinessProbeTarget::Tcp(endpoint.address)),
            PublishedEndpointProtocol::Tcp => None,
        })
        .or_else(|| {
            endpoints
                .iter()
                .find_map(|endpoint| match endpoint.protocol {
                    PublishedEndpointProtocol::Tcp | PublishedEndpointProtocol::Https => {
                        Some(ReadinessProbeTarget::Tcp(endpoint.address))
                    }
                    PublishedEndpointProtocol::Http => None,
                })
        })
}

fn readiness_probe_timeout(manifest: &ContainerSandboxManifest) -> Duration {
    manifest
        .image_metadata
        .healthcheck
        .as_ref()
        .and_then(|healthcheck| healthcheck.timeout)
        .map(Duration::from_nanos)
        .unwrap_or_else(|| Duration::from_millis(DEFAULT_READINESS_PROBE_TIMEOUT_MILLIS))
}

fn probe_target_ready(target: ReadinessProbeTarget, timeout: Duration) -> bool {
    match target {
        ReadinessProbeTarget::Tcp(address) => TcpStream::connect_timeout(&address, timeout).is_ok(),
        ReadinessProbeTarget::Http(address) => probe_http_ready(address, timeout),
    }
}

fn probe_http_ready(address: SocketAddr, timeout: Duration) -> bool {
    let Ok(mut stream) = TcpStream::connect_timeout(&address, timeout) else {
        return false;
    };
    if stream.set_read_timeout(Some(timeout)).is_err() {
        return false;
    }
    if stream
        .write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut response = [0_u8; 256];
    match stream.read(&mut response) {
        Ok(read) if read > 0 => String::from_utf8_lossy(&response[..read]).starts_with("HTTP/"),
        _ => false,
    }
}

fn visible_published_endpoints(
    start_mode: ContainerStartMode,
    spec: &SandboxSpec,
    status: SandboxStatus,
) -> Vec<PublishedEndpoint> {
    let endpoints = published_endpoints(spec);
    if start_mode == ContainerStartMode::Execute && status != SandboxStatus::Ready {
        Vec::new()
    } else {
        endpoints
    }
}

fn synchronize_handle_status(manifest: &mut ContainerSandboxManifest, status: SandboxStatus) {
    manifest.status = status;
    manifest.handle.status = status;
    manifest.handle.published_endpoints =
        visible_published_endpoints(manifest.start_mode, &manifest.spec, status);
}

fn published_endpoints(spec: &SandboxSpec) -> Vec<PublishedEndpoint> {
    spec.port_bindings
        .iter()
        .map(|port_binding| {
            PublishedEndpoint::new(
                port_binding.name.clone(),
                port_binding.protocol,
                port_binding.host_socket_addr(),
            )
            .with_guest_port(port_binding.guest_port)
        })
        .collect()
}

#[cfg(test)]
mod tests;
