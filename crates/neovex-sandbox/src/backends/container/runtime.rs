use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use super::bundle::{ContainerBundleLayout, write_bundle_config};
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
    OciNetworkConfig, OciNetworkLayout, create_persistent_network_namespace, expose_machine_ports,
    remove_persistent_network_namespace, setup_container_network, teardown_container_network,
    unexpose_machine_ports,
};
use crate::backends::oci::port_manager::PortManager;
use crate::endpoint::{PublishedEndpoint, PublishedEndpointProtocol};
use crate::error::{Result, SandboxError};
use crate::instance::{SandboxHandle, SandboxId, SandboxStatus};
use crate::process::pid_is_alive;
use crate::spec::{
    SandboxBuildLaunchSpec, SandboxImageLaunchSpec, SandboxImageProcessOverrides, SandboxSpec,
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
        let temp_root = std::env::temp_dir().join("neovex-container-sandbox");
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

#[derive(Debug, Clone)]
pub struct ContainerSandboxBackend {
    config: ContainerSandboxBackendConfig,
}

impl ContainerSandboxBackend {
    pub fn new(config: ContainerSandboxBackendConfig) -> Self {
        Self { config }
    }

    fn port_manager(&self) -> PortManager {
        PortManager::new(
            &self.config.state_root,
            self.config.published_port_range.clone(),
        )
    }

    fn network_config(&self) -> OciNetworkConfig {
        OciNetworkConfig {
            netavark_path: self.config.netavark_path.clone(),
            aardvark_dns_path: self.config.aardvark_dns_path.clone(),
            network_name: self.config.network_name.clone(),
            network_interface: self.config.network_interface.clone(),
            network_subnet: self.config.network_subnet.clone(),
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
        let prepared_launch = self.prepare_image_launch(&sandbox_id, image_reference, overrides)?;
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
        let prepared_launch = self.prepare_built_image_launch(
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
        resolved_spec
            .port_bindings
            .extend(self.port_manager().allocate_missing_bindings(
                &resolved_spec.port_bindings,
                &resolved_launch.image_metadata.exposed_ports,
            )?);
        let network_layout = OciNetworkLayout::new(&self.config.state_root, sandbox_id);
        let bundle_layout =
            ContainerBundleLayout::new(self.config.bundle_root.join(sandbox_id.as_str()));
        write_bundle_config(
            &bundle_layout,
            &hostname_for(&resolved_spec),
            &resolved_spec,
            resolved_launch.image_metadata.user.as_deref(),
            Some(network_layout.netns_path.as_path()),
        )?;

        let conmon_layout = OciConmonLayout::new(&self.config.state_root, sandbox_id);
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
            Some("running") => Ok(running_status(manifest)),
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
        sandbox_id: &SandboxId,
        image_reference: &str,
        overrides: &SandboxImageProcessOverrides,
    ) -> Result<PreparedMaterializedImageLaunch> {
        OciImageMaterializer::under_state_root(&self.config.state_root).prepare_image_launch(
            sandbox_id,
            image_reference,
            overrides,
        )
    }

    fn prepare_built_image_launch(
        &self,
        sandbox_id: &SandboxId,
        image_name: &str,
        dockerfile_path: &Path,
        context_path: &Path,
        overrides: &SandboxImageProcessOverrides,
    ) -> Result<PreparedMaterializedImageLaunch> {
        OciDockerfileBuilder::under_state_root(&self.config.state_root).prepare_built_image_launch(
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

    fn release_execution_artifacts(&self, manifest: &mut ContainerSandboxManifest) -> Result<()> {
        let mut errors = Vec::new();
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

    fn read_manifest(&self, id: &SandboxId) -> Result<Option<ContainerSandboxManifest>> {
        let manifest_path = self
            .config
            .state_root
            .join("containers")
            .join(id.as_str())
            .join("manifest.json");
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
    conmon_launch: OciConmonLaunchPlan,
    last_exit_code: Option<i32>,
    launch_mode: ContainerLaunchMode,
    shutdown_requested: bool,
    status: SandboxStatus,
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
        "neovex-container".to_owned()
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
mod tests {
    use std::collections::BTreeMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::thread;

    use neovex_core::TenantId;
    use tempfile::TempDir;

    use super::{
        ContainerLaunchArtifact, ContainerLaunchMode, ContainerSandboxBackend,
        ContainerSandboxBackendConfig,
    };
    use crate::backend::SandboxBackendKind;
    use crate::backends::oci::buildah::{
        OciExposedPort, OciExposedPortProtocol, OciImageLaunchDefaults,
    };
    use crate::backends::oci::materializer::MaterializedImageRootfs;
    use crate::backends::oci::network::OciMachinePortForwarderConfig;
    use crate::instance::{SandboxId, SandboxStatus};
    use crate::spec::{SandboxFilesystemSpec, SandboxPortBinding, SandboxProcessSpec, SandboxSpec};

    fn sample_spec() -> SandboxSpec {
        SandboxSpec::new(
            TenantId::new("svc-demo").expect("tenant should parse"),
            "db",
            SandboxBackendKind::Container,
            SandboxFilesystemSpec::new(PathBuf::from("/tmp/rootfs")),
            SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
        )
    }

    #[test]
    fn plan_only_backend_persists_a_container_manifest() {
        let temp_dir = TempDir::new().expect("tempdir should build");
        let backend = ContainerSandboxBackend::new(ContainerSandboxBackendConfig {
            launch_mode: ContainerLaunchMode::PlanOnly,
            ..ContainerSandboxBackendConfig::under_root(temp_dir.path())
        });

        let handle = backend
            .start_sync(sample_spec().with_port_binding(SandboxPortBinding::tcp("db", 5432, 5432)))
            .expect("container plan should start");

        assert_eq!(handle.backend, SandboxBackendKind::Container);
        let manifest_path = temp_dir
            .path()
            .join("state")
            .join("containers")
            .join(handle.id.as_str())
            .join("manifest.json");
        assert!(manifest_path.is_file(), "manifest should be written");
    }

    #[test]
    fn plan_only_backend_auto_assigns_exposed_ports_from_published_range() {
        let temp_dir = TempDir::new().expect("tempdir should build");
        let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
        config.launch_mode = ContainerLaunchMode::PlanOnly;
        config.published_port_range = 15000..=15001;
        let backend = ContainerSandboxBackend::new(config);

        let launch_defaults = OciImageLaunchDefaults {
            filesystem: SandboxFilesystemSpec::new(PathBuf::from("/tmp/rootfs")),
            process: SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
            exposed_ports: vec![OciExposedPort {
                port: 8080,
                protocol: OciExposedPortProtocol::Tcp,
                raw: "8080/tcp".to_owned(),
            }],
            user: None,
            stop_signal: None,
            healthcheck: None,
            labels: BTreeMap::new(),
        };

        let plan = backend
            .plan_start_with_id(
                &sample_spec(),
                &SandboxId::new("db-01"),
                Some(&launch_defaults),
                None,
            )
            .expect("plan should lower image-exposed ports");

        assert_eq!(plan.manifest.spec.port_bindings.len(), 1);
        let binding = &plan.manifest.spec.port_bindings[0];
        assert_eq!(binding.name, "tcp-8080");
        assert_eq!(binding.host_port, 15000);
        assert_eq!(binding.guest_port, 8080);
    }

    #[test]
    fn image_backed_plan_uses_direct_conmon_launch_for_materialized_rootfs() {
        let temp_dir = TempDir::new().expect("tempdir should build");
        let backend = ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(
            temp_dir.path(),
        ));
        let rootfs_path = temp_dir.path().join("materialized-rootfs");
        let launch_defaults = OciImageLaunchDefaults {
            filesystem: SandboxFilesystemSpec::new(rootfs_path.clone()),
            process: SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
            exposed_ports: Vec::new(),
            user: None,
            stop_signal: None,
            healthcheck: None,
            labels: BTreeMap::new(),
        };

        let plan = backend
            .plan_start_with_id(
                &sample_spec(),
                &SandboxId::new("db-01"),
                Some(&launch_defaults),
                Some(ContainerLaunchArtifact::Rootfs(MaterializedImageRootfs {
                    image_reference: "docker.io/library/demo:latest".to_owned(),
                    rootfs_path,
                })),
            )
            .expect("image-backed plan should lower");

        assert_eq!(
            plan.manifest.conmon_launch.create_command.program,
            PathBuf::from("conmon")
        );
        assert_eq!(
            plan.manifest.conmon_launch.start_command.program,
            PathBuf::from("crun")
        );
        assert!(
            plan.manifest
                .conmon_launch
                .create_command
                .args
                .first()
                .map(String::as_str)
                != Some("unshare"),
            "materialized rootfs launches should not be wrapped in buildah unshare"
        );
    }

    #[test]
    fn detect_runtime_status_marks_stale_pidfiles_as_failed() {
        let temp_dir = TempDir::new().expect("tempdir should build");
        let backend = ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(
            temp_dir.path(),
        ));
        let mut manifest = backend
            .plan_start_with_id(&sample_spec(), &SandboxId::new("db-01"), None, None)
            .expect("plan should lower")
            .manifest;
        let state_stub = temp_dir.path().join("crun-state");
        std::fs::write(&state_stub, "#!/bin/sh\nexit 1\n").expect("state stub should write");
        let mut permissions = std::fs::metadata(&state_stub)
            .expect("state stub metadata should resolve")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&state_stub, permissions)
            .expect("state stub permissions should update");
        manifest.conmon_launch.state_command.program = state_stub;
        std::fs::write(&manifest.conmon_layout.pidfile, "999999\n").expect("pidfile should write");

        assert_eq!(
            backend
                .detect_runtime_status(&manifest)
                .expect("status should resolve"),
            SandboxStatus::Failed
        );
    }

    #[test]
    fn release_execution_artifacts_ignores_machine_forwarder_unexpose_failures() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let port = listener
            .local_addr()
            .expect("listener address should resolve")
            .port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("connection should arrive");
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer);
            stream
                .write_all(
                    b"HTTP/1.0 500 Internal Server Error\r\nContent-Length: 16\r\n\r\nproxy not found",
                )
                .expect("response should write");
        });

        let temp_dir = TempDir::new().expect("tempdir should build");
        let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
        config.machine_port_forwarder = Some(OciMachinePortForwarderConfig {
            host: "127.0.0.1".to_owned(),
            port,
            path_prefix: "/services/forwarder".to_owned(),
        });
        let backend = ContainerSandboxBackend::new(config);
        let mut manifest = backend
            .plan_start_with_id(
                &sample_spec().with_port_binding(SandboxPortBinding::tcp("db", 5432, 5432)),
                &SandboxId::new("db-01"),
                None,
                None,
            )
            .expect("plan should lower")
            .manifest;

        backend
            .release_execution_artifacts(&mut manifest)
            .expect("cleanup should ignore unexpose failures");
        server.join().expect("server thread should join");
    }
}
