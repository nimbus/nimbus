use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use super::bundle::{
    ContainerBundleLayout, ContainerBundleMount, ContainerBundleOptions, write_bundle_config,
};
use crate::backend::{SandboxBackend, SandboxBackendKind, SandboxFuture};
use crate::backends::oci::buildah::{
    BuildahCli, ImageHealthcheck, MountedRootfsSession, OciExposedPort, OciImageLaunchDefaults,
};
use crate::backends::oci::builder::OciDockerfileBuilder;
use crate::backends::oci::command::CommandSpec;
use crate::backends::oci::conmon::{
    OciConmonConfig, OciConmonLaunchPlan, OciConmonLayout, build_launch_plan,
};
use crate::backends::oci::materializer::{
    MaterializedImageRootfs, OciImageMaterializer, PreparedMaterializedImageLaunch,
};
use crate::backends::oci::network::{
    DEFAULT_AARDVARK_DNS_BINARY, DEFAULT_NETAVARK_BINARY, OciMachinePortForwarderConfig,
    OciNetworkConfig, OciNetworkDirectEgress, OciNetworkLayout, bridge_gateway_addr,
    create_persistent_network_namespace, expose_machine_ports, remove_persistent_network_namespace,
    setup_container_network, teardown_container_network, unexpose_machine_ports,
};
use crate::backends::oci::port_manager::{DEFAULT_MAX_PORTS_PER_TENANT, PortManager};
use crate::backends::oci::resource_quota::ResourceQuotaManager;
use crate::egress::SandboxEgressPolicy;
use crate::egress_proxy::{SandboxEgressProxy, SandboxEgressProxyConfig};
use crate::endpoint::{PublishedEndpoint, PublishedEndpointProtocol};
use crate::error::{Result, SandboxError};
use crate::instance::{SandboxHandle, SandboxId, SandboxStatus};
use crate::process::pid_is_alive;
use crate::spec::{
    SandboxBuildLaunchSpec, SandboxImageLaunchSpec, SandboxImageProcessOverrides,
    SandboxResourceQuotaPolicy, SandboxSpec,
};

const DEFAULT_RUNTIME_PATH: &str = "crun";
const DEFAULT_CONMON_PATH: &str = "conmon";
const DEFAULT_BUILDAH_PATH: &str = "buildah";
const DEFAULT_PUBLISHED_PORT_START: u16 = 15_000;
const DEFAULT_PUBLISHED_PORT_END: u16 = 16_000;
const DEFAULT_START_TIMEOUT_SECS: u64 = 10;
const DEFAULT_STOP_TIMEOUT_SECS: u64 = 5;
const DEFAULT_READINESS_PROBE_TIMEOUT_MILLIS: u64 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerLaunchMode {
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
    pub launch_mode: ContainerLaunchMode,
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
            launch_mode: ContainerLaunchMode::PlanOnly,
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
            launch_mode: ContainerLaunchMode::Execute,
            log_level: "debug".to_owned(),
            start_timeout: Duration::from_secs(DEFAULT_START_TIMEOUT_SECS),
            stop_timeout: Duration::from_secs(DEFAULT_STOP_TIMEOUT_SECS),
        }
    }
}

#[derive(Clone)]
pub struct ContainerSandboxBackend {
    config: ContainerSandboxBackendConfig,
    egress_proxies: Arc<Mutex<HashMap<SandboxId, SandboxEgressProxy>>>,
}

impl ContainerSandboxBackend {
    pub fn new(config: ContainerSandboxBackendConfig) -> Self {
        Self {
            config,
            egress_proxies: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn reload_egress_policy(&self, id: &SandboxId, egress: SandboxEgressPolicy) -> Result<()> {
        let compiled = egress
            .compile()
            .map_err(|message| SandboxError::InvalidSpec { message })?;
        let Some(mut manifest) = self.read_manifest(id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: id.as_str().to_owned(),
            });
        };
        if manifest.launch_mode != ContainerLaunchMode::Execute {
            return Err(SandboxError::InvalidSpec {
                message: "container egress live reload requires execute-mode sandbox".to_owned(),
            });
        }
        manifest.spec.egress = compiled.policy().clone();
        self.ensure_egress_proxy_running(&manifest)?;
        let proxies = self
            .egress_proxies
            .lock()
            .map_err(|_| SandboxError::OperationFailed {
                message: "container egress proxy registry lock is poisoned".to_owned(),
            })?;
        let proxy = proxies
            .get(id)
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!("container egress proxy for sandbox {id} is not running"),
            })?;
        proxy.reload_policy(compiled)?;
        drop(proxies);
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

    fn start_from_image_sync(&self, launch: SandboxImageLaunchSpec) -> Result<SandboxHandle> {
        let launch_plan = self.plan_start_from_image(
            &launch.spec,
            &launch.image_reference,
            &launch.process_overrides,
        )?;
        self.finish_start(launch_plan)
    }

    fn start_from_build_sync(&self, launch: SandboxBuildLaunchSpec) -> Result<SandboxHandle> {
        let launch_plan = self.plan_start_from_build(
            &launch.spec,
            &launch.image_name,
            &launch.dockerfile_path,
            &launch.context_path,
            &launch.process_overrides,
        )?;
        self.finish_start(launch_plan)
    }

    fn finish_start(&self, launch_plan: ContainerLaunchPlan) -> Result<SandboxHandle> {
        let mut manifest = launch_plan.manifest;
        match self.config.launch_mode {
            ContainerLaunchMode::PlanOnly => {
                manifest.last_exit_code = None;
                manifest.shutdown_requested = false;
                self.write_manifest(&manifest)?;
                Ok(manifest.handle)
            }
            ContainerLaunchMode::Execute => self.execute_start(&manifest).inspect_err(|_| {
                let _ = self.cleanup_manifest_launch_artifacts(&manifest);
            }),
        }
    }

    fn inspect_sync(&self, id: &SandboxId) -> Result<Option<SandboxHandle>> {
        let Some(mut manifest) = self.read_manifest(id)? else {
            return Ok(None);
        };
        let status = match self.config.launch_mode {
            ContainerLaunchMode::PlanOnly => manifest.status,
            ContainerLaunchMode::Execute => self.detect_runtime_status(&manifest)?,
        };
        if self.config.launch_mode == ContainerLaunchMode::Execute
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

        match self.config.launch_mode {
            ContainerLaunchMode::PlanOnly => {
                manifest.shutdown_requested = true;
                manifest.last_exit_code = Some(0);
                synchronize_handle_status(&mut manifest, SandboxStatus::Stopped);
                self.cleanup_manifest_launch_artifacts(&manifest)?;
                manifest.launch_artifact = None;
                self.write_manifest(&manifest)
            }
            ContainerLaunchMode::Execute => self.execute_stop(&mut manifest),
        }
    }

    pub(crate) fn plan_start(&self, spec: &SandboxSpec) -> Result<ContainerLaunchPlan> {
        let sandbox_id = next_sandbox_id(&spec.name);
        self.plan_start_with_id(spec, &sandbox_id, None, None)
    }

    pub(crate) fn plan_start_from_image(
        &self,
        spec: &SandboxSpec,
        image_reference: &str,
        overrides: &SandboxImageProcessOverrides,
    ) -> Result<ContainerLaunchPlan> {
        let sandbox_id = next_sandbox_id(&spec.name);
        self.resource_quota_manager().ensure_launch_quota(spec)?;
        let prepared_launch =
            self.prepare_image_launch(spec, &sandbox_id, image_reference, overrides)?;
        self.plan_start_with_materialized_launch(spec, &sandbox_id, prepared_launch)
    }

    pub(crate) fn plan_start_from_build(
        &self,
        spec: &SandboxSpec,
        image_name: &str,
        dockerfile_path: &Path,
        context_path: &Path,
        overrides: &SandboxImageProcessOverrides,
    ) -> Result<ContainerLaunchPlan> {
        let sandbox_id = next_sandbox_id(&spec.name);
        self.resource_quota_manager().ensure_launch_quota(spec)?;
        let prepared_launch = self.prepare_built_image_launch(
            spec,
            &sandbox_id,
            image_name,
            dockerfile_path,
            context_path,
            overrides,
        )?;
        self.plan_start_with_materialized_launch(spec, &sandbox_id, prepared_launch)
    }

    fn plan_start_with_materialized_launch(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        prepared_launch: PreparedMaterializedImageLaunch,
    ) -> Result<ContainerLaunchPlan> {
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
    ) -> Result<ContainerLaunchPlan> {
        if spec.backend != SandboxBackendKind::Container {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "container backend cannot lower sandbox spec for backend {:?}",
                    spec.backend
                ),
            });
        }

        let resolved_launch = resolve_launch_spec(spec, launch_defaults);
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
        let egress_proxy = (self.config.launch_mode == ContainerLaunchMode::Execute)
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
            &resolved_launch.spec.name,
            &bundle_layout.bundle_dir,
            launch_artifact
                .as_ref()
                .and_then(ContainerLaunchArtifact::mount_session_name),
            &[],
        );

        let handle = SandboxHandle::new(
            resolved_spec.tenant_id.clone(),
            sandbox_id.clone(),
            resolved_spec.name.clone(),
            SandboxBackendKind::Container,
            SandboxStatus::Starting,
            visible_published_endpoints(
                self.config.launch_mode,
                &resolved_spec,
                SandboxStatus::Starting,
            ),
        );

        Ok(ContainerLaunchPlan {
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
                last_exit_code: None,
                launch_mode: self.config.launch_mode,
                shutdown_requested: false,
                status: SandboxStatus::Starting,
            },
        })
    }

    fn execute_start(&self, manifest: &ContainerSandboxManifest) -> Result<SandboxHandle> {
        ensure_linux_host()?;
        let mut manifest = manifest.clone();
        self.configure_network(&manifest)?;
        if let Err(error) = self.ensure_egress_proxy_running(&manifest) {
            let _ = self.release_execution_artifacts(&mut manifest);
            return Err(error);
        }
        if let Err(error) = spawn_background(&manifest.conmon_launch.create_command) {
            let _ = self.release_execution_artifacts(&mut manifest);
            return Err(error);
        }
        let runtime_state = match wait_for_runtime_state(
            &manifest.conmon_launch.state_command,
            self.config.start_timeout,
        ) {
            Ok(state) => state,
            Err(error) => {
                let _ = self.release_execution_artifacts(&mut manifest);
                return Err(error);
            }
        };
        if runtime_state != "running"
            && let Err(error) = run_status_checked(&manifest.conmon_launch.start_command)
        {
            let _ = self.release_execution_artifacts(&mut manifest);
            return Err(error);
        }

        manifest.shutdown_requested = false;
        manifest.last_exit_code = None;
        synchronize_handle_status(&mut manifest, SandboxStatus::Starting);
        self.write_manifest(&manifest)?;
        Ok(manifest.handle)
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
        let stop_signal = configured_stop_signal(&manifest.image_metadata);
        signal_process(&stop_signal, pid)?;
        let stop_timeout = configured_stop_timeout(&manifest.spec, &self.config);
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
        if manifest.conmon_layout.exit_status_file.exists() {
            let exit_code = read_exit_code(&manifest.conmon_layout.exit_status_file)?;
            if manifest.shutdown_requested || exit_code == 0 {
                return Ok(SandboxStatus::Stopped);
            }
            return Ok(SandboxStatus::Failed);
        }

        let runtime_state = runtime_state(&manifest.conmon_launch.state_command)?;
        match runtime_state.as_deref() {
            Some("running") => {
                self.ensure_egress_proxy_running(manifest)?;
                Ok(running_status(manifest))
            }
            Some("created") | Some("creating") => Ok(SandboxStatus::Starting),
            Some("stopped") => Ok(SandboxStatus::Stopped),
            Some("paused") => Ok(SandboxStatus::Stopping),
            Some(_) => Ok(SandboxStatus::Failed),
            None if manifest.conmon_layout.pidfile.exists() => {
                if pid_is_alive(read_pid(&manifest.conmon_layout.pidfile)?) {
                    Ok(SandboxStatus::Starting)
                } else if manifest.shutdown_requested {
                    Ok(SandboxStatus::Stopped)
                } else {
                    Ok(SandboxStatus::Failed)
                }
            }
            None => Ok(manifest.status),
        }
    }

    fn prepare_image_launch(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        image_reference: &str,
        overrides: &SandboxImageProcessOverrides,
    ) -> Result<PreparedMaterializedImageLaunch> {
        OciImageMaterializer::for_tenant_sandbox(
            &self.config.state_root,
            &spec.tenant_id,
            sandbox_id,
        )
        .prepare_image_launch(sandbox_id, image_reference, overrides)
    }

    fn prepare_built_image_launch(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        image_name: &str,
        dockerfile_path: &Path,
        context_path: &Path,
        overrides: &SandboxImageProcessOverrides,
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
            overrides,
        )
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
        if let Some(forwarder) = self.config.machine_port_forwarder.as_ref() {
            expose_machine_ports(forwarder, &manifest.spec.port_bindings)?;
        }
        if let Err(error) = create_persistent_network_namespace(&manifest.network_layout.netns_path)
        {
            if let Some(forwarder) = self.config.machine_port_forwarder.as_ref() {
                let _ = unexpose_machine_ports(forwarder, &manifest.spec.port_bindings);
            }
            return Err(error);
        }
        if let Err(error) = setup_container_network(
            &manifest.network_layout,
            &self.network_config(),
            &manifest.handle.id,
            &manifest.spec.name,
            &hostname_for(&manifest.spec),
            &manifest.spec.port_bindings,
            self.config.machine_port_forwarder.as_ref(),
        ) {
            let _ = remove_persistent_network_namespace(&manifest.network_layout.netns_path);
            if let Some(forwarder) = self.config.machine_port_forwarder.as_ref() {
                let _ = unexpose_machine_ports(forwarder, &manifest.spec.port_bindings);
            }
            return Err(error);
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
        let mut proxies =
            self.egress_proxies
                .lock()
                .map_err(|_| SandboxError::OperationFailed {
                    message: "container egress proxy registry lock is poisoned".to_owned(),
                })?;
        if proxies.contains_key(&manifest.handle.id) {
            return Ok(());
        }
        let policy = manifest
            .spec
            .egress
            .compile()
            .map_err(|message| SandboxError::InvalidSpec { message })?;
        let bind_addr = egress_proxy.bind_addr()?;
        let proxy = SandboxEgressProxy::start(
            SandboxEgressProxyConfig::new(policy).with_bind_addr(bind_addr),
        )?;
        proxies.insert(manifest.handle.id.clone(), proxy);
        Ok(())
    }

    fn release_execution_artifacts(&self, manifest: &mut ContainerSandboxManifest) -> Result<()> {
        let mut errors = Vec::new();
        if let Err(error) = self.stop_egress_proxy(&manifest.handle.id) {
            errors.push(error.to_string());
        }
        let _ = run_status_best_effort(&manifest.conmon_launch.delete_command);
        if let Err(error) = teardown_container_network(
            &manifest.network_layout,
            &self.network_config(),
            &manifest.handle.id,
            &manifest.spec.name,
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
        let mut proxies =
            self.egress_proxies
                .lock()
                .map_err(|_| SandboxError::OperationFailed {
                    message: "container egress proxy registry lock is poisoned".to_owned(),
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
                spec.name,
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

    fn start_from_image(&self, launch: SandboxImageLaunchSpec) -> SandboxFuture<SandboxHandle> {
        let backend = self.clone();
        Box::pin(async move { backend.start_from_image_sync(launch) })
    }

    fn start_from_build(&self, launch: SandboxBuildLaunchSpec) -> SandboxFuture<SandboxHandle> {
        let backend = self.clone();
        Box::pin(async move { backend.start_from_build_sync(launch) })
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

    fn reload_egress_policy(
        &self,
        id: &SandboxId,
        egress: SandboxEgressPolicy,
    ) -> SandboxFuture<()> {
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
pub(crate) struct ContainerLaunchPlan {
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
    last_exit_code: Option<i32>,
    launch_mode: ContainerLaunchMode,
    shutdown_requested: bool,
    status: SandboxStatus,
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

#[derive(Debug, Deserialize)]
struct RuntimeStatePayload {
    status: String,
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

fn ensure_linux_host() -> Result<()> {
    if cfg!(target_os = "linux") {
        return Ok(());
    }

    Err(SandboxError::BackendUnavailable {
        message:
            "container execution requires a Linux host; use plan-only mode for cross-platform tests"
                .to_owned(),
    })
}

fn next_sandbox_id(name: &str) -> SandboxId {
    SandboxId::new(format!(
        "{}-{}",
        slugify(name),
        Ulid::new().to_string().to_ascii_lowercase()
    ))
}

fn hostname_for(spec: &SandboxSpec) -> String {
    let slug = slugify(&spec.name);
    if slug.is_empty() {
        "nimbus-container".to_owned()
    } else {
        slug
    }
}

fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_owned()
}

fn resolve_launch_spec(
    spec: &SandboxSpec,
    launch_defaults: Option<&OciImageLaunchDefaults>,
) -> ContainerResolvedLaunchSpec {
    let Some(launch_defaults) = launch_defaults else {
        return ContainerResolvedLaunchSpec {
            spec: spec.clone(),
            image_metadata: ContainerImageMetadata::default(),
        };
    };

    let mut resolved_spec = spec.clone();
    if resolved_spec.filesystem.is_unspecified() {
        resolved_spec.filesystem = launch_defaults.filesystem.clone();
    }
    resolved_spec.process = resolve_process_spec(&spec.process, &launch_defaults.process);

    ContainerResolvedLaunchSpec {
        spec: resolved_spec,
        image_metadata: ContainerImageMetadata {
            user: launch_defaults.user.clone(),
            stop_signal: launch_defaults.stop_signal.clone(),
            healthcheck: launch_defaults.healthcheck.clone(),
            labels: launch_defaults.labels.clone(),
            exposed_ports: launch_defaults.exposed_ports.clone(),
        },
    }
}

fn resolve_process_spec(
    spec: &crate::spec::SandboxProcessSpec,
    defaults: &crate::spec::SandboxProcessSpec,
) -> crate::spec::SandboxProcessSpec {
    let mut resolved = defaults.clone();
    if !spec.args.is_empty() {
        resolved.args = spec.args.clone();
    }
    if spec.env.is_empty() || spec.uses_default_env() {
        resolved.env = defaults.env.clone();
    } else {
        resolved.env = spec.env.clone();
    }
    if !spec.uses_default_cwd() {
        resolved.cwd = spec.cwd.clone();
    }
    resolved.terminal = spec.terminal || defaults.terminal;
    resolved
}

fn configured_stop_signal(image_metadata: &ContainerImageMetadata) -> String {
    image_metadata
        .stop_signal
        .as_deref()
        .map(str::trim)
        .filter(|signal| !signal.is_empty())
        .unwrap_or("TERM")
        .to_owned()
}

fn configured_stop_timeout(spec: &SandboxSpec, config: &ContainerSandboxBackendConfig) -> Duration {
    spec.lifecycle.stop_timeout.unwrap_or(config.stop_timeout)
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
    launch_mode: ContainerLaunchMode,
    spec: &SandboxSpec,
    status: SandboxStatus,
) -> Vec<PublishedEndpoint> {
    let endpoints = published_endpoints(spec);
    if launch_mode == ContainerLaunchMode::Execute && status != SandboxStatus::Ready {
        Vec::new()
    } else {
        endpoints
    }
}

fn synchronize_handle_status(manifest: &mut ContainerSandboxManifest, status: SandboxStatus) {
    manifest.status = status;
    manifest.handle.status = status;
    manifest.handle.published_endpoints =
        visible_published_endpoints(manifest.launch_mode, &manifest.spec, status);
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

fn spawn_background(command: &CommandSpec) -> Result<()> {
    command
        .as_command()
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to spawn sandbox lifecycle command {}: {error}",
                command.program.display()
            ),
        })?;
    Ok(())
}

fn run_status_checked(command: &CommandSpec) -> Result<()> {
    let output = command
        .as_command()
        .output()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to run sandbox command {}: {error}",
                command.program.display()
            ),
        })?;
    if output.status.success() {
        return Ok(());
    }
    Err(SandboxError::OperationFailed {
        message: format!(
            "sandbox command {} failed: {}",
            command.program.display(),
            render_command_failure(&output.stderr)
        ),
    })
}

fn run_status_best_effort(command: &CommandSpec) -> Result<()> {
    let _ = command
        .as_command()
        .output()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to run sandbox cleanup command {}: {error}",
                command.program.display()
            ),
        })?;
    Ok(())
}

fn runtime_state(command: &CommandSpec) -> Result<Option<String>> {
    let output = command
        .as_command()
        .output()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to run runtime state command {}: {error}",
                command.program.display()
            ),
        })?;
    if !output.status.success() {
        return Ok(None);
    }
    let payload: RuntimeStatePayload =
        serde_json::from_slice(&output.stdout).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to parse runtime state JSON: {error}"),
        })?;
    Ok(Some(payload.status))
}

fn wait_for_runtime_state(command: &CommandSpec, timeout: Duration) -> Result<String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(status) = runtime_state(command)?
            && (status == "created" || status == "running")
        {
            return Ok(status);
        }
        thread::sleep(Duration::from_millis(200));
    }
    Err(SandboxError::OperationFailed {
        message: format!(
            "sandbox runtime did not reach created state before timeout via {}",
            command.program.display()
        ),
    })
}

fn signal_process(signal: &str, pid: u32) -> Result<()> {
    let status = std::process::Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .status()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to signal sandbox process {pid} with {signal}: {error}"),
        })?;
    if status.success() {
        return Ok(());
    }
    Err(SandboxError::OperationFailed {
        message: format!("kill -{signal} {pid} returned non-zero status {status}"),
    })
}

fn read_pid(path: &Path) -> Result<u32> {
    let pid = std::fs::read_to_string(path).map_err(|error| SandboxError::OperationFailed {
        message: format!("failed to read sandbox pidfile {}: {error}", path.display()),
    })?;
    pid.trim()
        .parse::<u32>()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to parse sandbox pid from {}: {error}",
                path.display()
            ),
        })
}

fn wait_for_path(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(200));
    }
    path.exists()
}

fn read_exit_code(path: &Path) -> Result<i32> {
    let exit_status =
        std::fs::read_to_string(path).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to read sandbox exit status {}: {error}",
                path.display()
            ),
        })?;
    exit_status
        .trim()
        .parse::<i32>()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to parse sandbox exit status {}: {error}",
                path.display()
            ),
        })
}

fn render_command_failure(stderr: &[u8]) -> String {
    let rendered = String::from_utf8_lossy(stderr).trim().to_owned();
    if rendered.is_empty() {
        "stderr was empty".to_owned()
    } else {
        rendered
    }
}

#[cfg(test)]
mod tests;
