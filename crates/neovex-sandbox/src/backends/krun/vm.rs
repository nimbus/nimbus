use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;
use ulid::Ulid;

use super::bundle::{KrunBundleLayout, KrunBundleMount, KrunBundleOptions, write_bundle_config};
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
use crate::backends::oci::port_manager::PortManager;
use crate::endpoint::{PublishedEndpoint, PublishedEndpointProtocol};
use crate::error::{Result, SandboxError};
use crate::instance::{SandboxHandle, SandboxId, SandboxStatus};
use crate::process::pid_is_alive;
use crate::spec::{
    SandboxBuildLaunchSpec, SandboxImageLaunchSpec, SandboxImageProcessOverrides,
    SandboxRestartPolicy, SandboxSpec,
};

const DEFAULT_RUNTIME_PATH: &str = "/usr/libexec/neovex/crun";
const DEFAULT_CONMON_PATH: &str = "conmon";
const DEFAULT_BUILDAH_PATH: &str = "buildah";
const DEFAULT_GUEST_USER_HELPER_ROOT: &str = "/usr/libexec/neovex";
const DEFAULT_PUBLISHED_PORT_START: u16 = 15_000;
const DEFAULT_PUBLISHED_PORT_END: u16 = 16_000;
const DEFAULT_START_TIMEOUT_SECS: u64 = 10;
const DEFAULT_STOP_TIMEOUT_SECS: u64 = 5;
const DEFAULT_READINESS_PROBE_TIMEOUT_MILLIS: u64 = 1_000;
const DEFAULT_RESTART_BACKOFF_INITIAL_MILLIS: u64 = 1_000;
const DEFAULT_RESTART_BACKOFF_MAX_MILLIS: u64 = 60_000;
const KRUN_VM_CONFIG_FILENAME: &str = ".krun_vm.json";
const GUEST_USER_HELPER_BINARY_NAME: &str = "neovex-guest-user-switch";
const GUEST_USER_HELPER_GUEST_ROOT: &str = "/.neovex";
const GUEST_USER_HELPER_GUEST_PATH: &str = "/.neovex/neovex-guest-user-switch";
const GUEST_USER_UID_ENV: &str = "NEOVEX_GUEST_UID";
const GUEST_USER_GID_ENV: &str = "NEOVEX_GUEST_GID";
const BYTES_PER_MIB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KrunLaunchMode {
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
    #[cfg(test)]
    pub buildah_launcher_args: Vec<String>,
    pub guest_user_helper_root: PathBuf,
    pub use_buildah_unshare: bool,
    pub published_port_range: RangeInclusive<u16>,
    pub launch_mode: KrunLaunchMode,
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
            launch_mode: KrunLaunchMode::PlanOnly,
            ..Self::default()
        }
    }
}

impl Default for KrunSandboxBackendConfig {
    fn default() -> Self {
        let temp_root = std::env::temp_dir().join("neovex-sandbox");
        Self {
            bundle_root: temp_root.join("bundles"),
            state_root: temp_root.join("state"),
            conmon_path: PathBuf::from(DEFAULT_CONMON_PATH),
            runtime_path: PathBuf::from(DEFAULT_RUNTIME_PATH),
            buildah_path: PathBuf::from(DEFAULT_BUILDAH_PATH),
            #[cfg(test)]
            buildah_launcher_args: Vec::new(),
            guest_user_helper_root: PathBuf::from(DEFAULT_GUEST_USER_HELPER_ROOT),
            use_buildah_unshare: true,
            published_port_range: DEFAULT_PUBLISHED_PORT_START..=DEFAULT_PUBLISHED_PORT_END,
            launch_mode: KrunLaunchMode::Execute,
            log_level: "debug".to_owned(),
            start_timeout: Duration::from_secs(DEFAULT_START_TIMEOUT_SECS),
            stop_timeout: Duration::from_secs(DEFAULT_STOP_TIMEOUT_SECS),
        }
    }
}

#[derive(Debug, Clone)]
pub struct KrunSandboxBackend {
    config: KrunSandboxBackendConfig,
}

impl KrunSandboxBackend {
    pub fn new(config: KrunSandboxBackendConfig) -> Self {
        Self { config }
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

    fn finish_start(&self, launch_plan: KrunLaunchPlan) -> Result<SandboxHandle> {
        let mut manifest = launch_plan.manifest;
        self.materialize_auto_port_bindings(&mut manifest)?;
        self.materialize_krun_vm_config(&manifest)?;
        let launch_plan = KrunLaunchPlan { manifest };

        match self.config.launch_mode {
            KrunLaunchMode::PlanOnly => {
                let mut manifest = launch_plan.manifest.clone();
                manifest.last_exit_code = None;
                manifest.shutdown_requested = false;
                self.write_manifest(&manifest)?;
                Ok(manifest.handle)
            }
            KrunLaunchMode::Execute => self.execute_start(&launch_plan).inspect_err(|_| {
                let _ = self.cleanup_manifest_launch_artifacts(&launch_plan.manifest);
            }),
        }
    }

    fn inspect_sync(&self, id: &SandboxId) -> Result<Option<SandboxHandle>> {
        let Some(mut manifest) = self.read_manifest(id)? else {
            return Ok(None);
        };

        manifest.status = match self.config.launch_mode {
            KrunLaunchMode::PlanOnly => manifest.status,
            KrunLaunchMode::Execute => {
                if self.maybe_restart_after_exit(&mut manifest)? {
                    manifest.status
                } else {
                    self.detect_runtime_status(&manifest)?
                }
            }
        };
        manifest.handle.status = manifest.status;
        manifest.handle.published_endpoints =
            visible_published_endpoints(manifest.launch_mode, &manifest.spec, manifest.status);
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
            KrunLaunchMode::PlanOnly => {
                manifest.shutdown_requested = true;
                manifest.last_exit_code = Some(0);
                manifest.status = SandboxStatus::Stopped;
                manifest.handle.status = SandboxStatus::Stopped;
                self.cleanup_manifest_launch_artifacts(&manifest)?;
                manifest.launch_artifact = None;
                self.write_manifest(&manifest)
            }
            KrunLaunchMode::Execute => self.execute_stop(&mut manifest),
        }
    }

    pub(crate) fn plan_start(&self, spec: &SandboxSpec) -> Result<KrunLaunchPlan> {
        let sandbox_id = next_sandbox_id(&spec.name);
        self.plan_start_with_id(spec, &sandbox_id, None, None)
    }

    pub(crate) fn plan_start_from_image(
        &self,
        spec: &SandboxSpec,
        image_reference: &str,
        overrides: &SandboxImageProcessOverrides,
    ) -> Result<KrunLaunchPlan> {
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
    ) -> Result<KrunLaunchPlan> {
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

    pub fn start_from_image(&self, launch: SandboxImageLaunchSpec) -> SandboxFuture<SandboxHandle> {
        let backend = self.clone();
        Box::pin(async move { backend.start_from_image_sync(launch) })
    }

    pub fn start_from_build(&self, launch: SandboxBuildLaunchSpec) -> SandboxFuture<SandboxHandle> {
        let backend = self.clone();
        Box::pin(async move { backend.start_from_build_sync(launch) })
    }

    #[cfg(test)]
    pub(crate) fn plan_start_with_launch_defaults(
        &self,
        spec: &SandboxSpec,
        launch_defaults: Option<&OciImageLaunchDefaults>,
    ) -> Result<KrunLaunchPlan> {
        let sandbox_id = next_sandbox_id(&spec.name);
        self.plan_start_with_id(spec, &sandbox_id, launch_defaults, None)
    }

    fn plan_start_with_materialized_launch(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        prepared_launch: PreparedMaterializedImageLaunch,
    ) -> Result<KrunLaunchPlan> {
        self.plan_start_with_id(
            spec,
            sandbox_id,
            Some(&prepared_launch.launch_defaults),
            Some(KrunLaunchArtifact::Rootfs(prepared_launch.artifact)),
        )
    }

    fn plan_start_with_id(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        launch_defaults: Option<&OciImageLaunchDefaults>,
        launch_artifact: Option<KrunLaunchArtifact>,
    ) -> Result<KrunLaunchPlan> {
        if spec.backend != SandboxBackendKind::Krun {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "krun backend cannot lower sandbox spec for backend {:?}",
                    spec.backend
                ),
            });
        }

        let mut resolved_launch = resolve_launch_spec(spec, launch_defaults);
        apply_guest_user_switch(&mut resolved_launch.spec, &resolved_launch.image_metadata)?;
        let bundle_layout =
            KrunBundleLayout::new(self.config.bundle_root.join(sandbox_id.as_str()));
        write_bundle_config(
            &bundle_layout,
            &hostname_for(&resolved_launch.spec),
            &resolved_launch.spec,
            &KrunBundleOptions {
                additional_mounts: guest_user_switch_mounts(
                    &self.config,
                    &resolved_launch.image_metadata,
                ),
            },
        )?;

        let conmon_layout = OciConmonLayout::new(&self.config.state_root, sandbox_id);
        conmon_layout
            .ensure_directories()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to create krun state directories under {}: {error}",
                    self.config.state_root.display()
                ),
            })?;

        let conmon_launch = build_launch_plan(
            &OciConmonConfig {
                conmon_path: self.config.conmon_path.clone(),
                runtime_path: self.config.runtime_path.clone(),
                buildah_path: self.config.buildah_path.clone(),
                use_buildah_unshare: launch_artifact
                    .as_ref()
                    .is_some_and(KrunLaunchArtifact::uses_mount_session_unshare)
                    && self.config.use_buildah_unshare,
                log_level: self.config.log_level.clone(),
            },
            &conmon_layout,
            sandbox_id,
            &spec.name,
            &bundle_layout.bundle_dir,
            launch_artifact
                .as_ref()
                .and_then(KrunLaunchArtifact::mount_session_name),
            &krun_vm_config_prelude(
                &resolved_launch.spec,
                launch_artifact
                    .as_ref()
                    .is_some_and(KrunLaunchArtifact::uses_mount_session_unshare)
                    && self.config.use_buildah_unshare,
            )?,
        );

        let handle = SandboxHandle::new(
            sandbox_id.clone(),
            resolved_launch.spec.name.clone(),
            SandboxBackendKind::Krun,
            SandboxStatus::Starting,
            visible_published_endpoints(
                self.config.launch_mode,
                &resolved_launch.spec,
                SandboxStatus::Starting,
            ),
        );
        let manifest = KrunSandboxManifest {
            handle,
            spec: resolved_launch.spec,
            image_metadata: resolved_launch.image_metadata,
            launch_artifact,
            bundle_layout,
            conmon_layout,
            conmon_launch,
            last_exit_code: None,
            restart_count: 0,
            next_restart_at_millis: None,
            launch_mode: self.config.launch_mode,
            shutdown_requested: false,
            status: SandboxStatus::Starting,
        };

        Ok(KrunLaunchPlan { manifest })
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

    fn buildah_cli(&self) -> BuildahCli {
        let buildah = BuildahCli::new(self.config.buildah_path.clone());
        #[cfg(test)]
        let buildah = buildah.with_launcher_args(self.config.buildah_launcher_args.clone());
        buildah.with_unshare(self.config.use_buildah_unshare)
    }

    fn cleanup_manifest_launch_artifacts(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        let Some(artifact) = manifest.launch_artifact.as_ref() else {
            return Ok(());
        };
        match artifact {
            KrunLaunchArtifact::MountedRootfs(session) => {
                self.buildah_cli()
                    .cleanup_rootfs_session(&session.session_name)?;
            }
            KrunLaunchArtifact::Rootfs(rootfs) => {
                if !rootfs.rootfs_path.exists() {
                    return Ok(());
                }
                std::fs::remove_dir_all(&rootfs.rootfs_path).map_err(|error| {
                    SandboxError::OperationFailed {
                        message: format!(
                            "failed to remove materialized krun rootfs {}: {error}",
                            rootfs.rootfs_path.display()
                        ),
                    }
                })?;
            }
        }
        Ok(())
    }

    fn materialize_auto_port_bindings(&self, manifest: &mut KrunSandboxManifest) -> Result<()> {
        let auto_bindings = self.port_manager().allocate_missing_bindings(
            &manifest.spec.port_bindings,
            &manifest.image_metadata.exposed_ports,
        )?;
        if auto_bindings.is_empty() {
            return Ok(());
        }

        manifest.spec.port_bindings.extend(auto_bindings);
        manifest.handle.published_endpoints =
            visible_published_endpoints(manifest.launch_mode, &manifest.spec, manifest.status);
        write_bundle_config(
            &manifest.bundle_layout,
            &hostname_for(&manifest.spec),
            &manifest.spec,
            &KrunBundleOptions {
                additional_mounts: guest_user_switch_mounts(&self.config, &manifest.image_metadata),
            },
        )
    }

    fn materialize_krun_vm_config(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        if manifest
            .launch_artifact
            .as_ref()
            .is_some_and(KrunLaunchArtifact::uses_mount_session_unshare)
            && self.config.use_buildah_unshare
        {
            return Ok(());
        }

        let vm_config_path = krun_vm_config_path(&manifest.spec.filesystem.rootfs);
        match desired_krun_vm_config(&manifest.spec)? {
            Some(vm_config) => {
                let rendered = serde_json::to_vec_pretty(&vm_config).map_err(|error| {
                    SandboxError::OperationFailed {
                        message: format!("failed to serialize krun vm config: {error}"),
                    }
                })?;
                std::fs::write(&vm_config_path, rendered).map_err(|error| {
                    SandboxError::OperationFailed {
                        message: format!(
                            "failed to write krun vm config {}: {error}",
                            vm_config_path.display()
                        ),
                    }
                })
            }
            None => {
                if !vm_config_path.exists() {
                    return Ok(());
                }
                std::fs::remove_file(&vm_config_path).map_err(|error| {
                    SandboxError::OperationFailed {
                        message: format!(
                            "failed to remove stale krun vm config {}: {error}",
                            vm_config_path.display()
                        ),
                    }
                })
            }
        }
    }

    fn port_manager(&self) -> PortManager {
        PortManager::new(
            self.config.state_root.clone(),
            self.config.published_port_range.clone(),
        )
    }

    fn execute_start(&self, launch_plan: &KrunLaunchPlan) -> Result<SandboxHandle> {
        ensure_linux_host()?;
        let mut manifest = launch_plan.manifest.clone();
        self.launch_manifest(&mut manifest, true)?;
        Ok(manifest.handle)
    }

    fn execute_stop(&self, manifest: &mut KrunSandboxManifest) -> Result<()> {
        if manifest.conmon_layout.exit_status_file.exists() {
            manifest.shutdown_requested = true;
            manifest.next_restart_at_millis = None;
            manifest.last_exit_code =
                Some(read_exit_code(&manifest.conmon_layout.exit_status_file)?);
            synchronize_handle_status(manifest, SandboxStatus::Stopped);
            return self.write_manifest(manifest);
        }

        manifest.shutdown_requested = true;
        manifest.next_restart_at_millis = None;
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
        self.cleanup_manifest_launch_artifacts(manifest)?;
        manifest.launch_artifact = None;
        self.write_manifest(manifest)
    }

    fn detect_runtime_status(&self, manifest: &KrunSandboxManifest) -> Result<SandboxStatus> {
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

    fn maybe_restart_after_exit(&self, manifest: &mut KrunSandboxManifest) -> Result<bool> {
        if manifest.shutdown_requested || !manifest.conmon_layout.exit_status_file.exists() {
            return Ok(false);
        }

        let exit_code = read_exit_code(&manifest.conmon_layout.exit_status_file)?;
        if !restart_policy_allows_restart(
            manifest.spec.lifecycle.restart_policy,
            exit_code,
            manifest.restart_count,
        ) {
            return Ok(false);
        }

        manifest.last_exit_code = Some(exit_code);
        let now_millis = now_millis()?;
        let next_restart_at_millis = manifest.next_restart_at_millis.get_or_insert_with(|| {
            now_millis.saturating_add(restart_backoff_delay(manifest.restart_count).as_millis() as u64)
        });
        if now_millis < *next_restart_at_millis {
            synchronize_handle_status(manifest, SandboxStatus::Starting);
            return Ok(true);
        }

        manifest.restart_count += 1;
        manifest.next_restart_at_millis = None;
        self.reset_runtime_for_restart(manifest)?;
        self.launch_manifest(manifest, false)?;
        Ok(true)
    }

    fn launch_manifest(
        &self,
        manifest: &mut KrunSandboxManifest,
        clear_last_exit_code: bool,
    ) -> Result<()> {
        ensure_linux_host()?;
        ensure_guest_user_helper_available(&self.config, manifest)?;
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

    fn reset_runtime_for_restart(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        run_status_checked(&manifest.conmon_launch.delete_command)?;
        remove_if_exists(&manifest.conmon_layout.exit_status_file)?;
        remove_if_exists(&manifest.conmon_layout.pidfile)?;
        remove_if_exists(&manifest.conmon_layout.conmon_pidfile)?;
        Ok(())
    }

    fn read_manifest(&self, id: &SandboxId) -> Result<Option<KrunSandboxManifest>> {
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

    fn write_manifest(&self, manifest: &KrunSandboxManifest) -> Result<()> {
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

impl SandboxBackend for KrunSandboxBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Krun
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
pub(crate) struct KrunLaunchPlan {
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
    conmon_launch: OciConmonLaunchPlan,
    last_exit_code: Option<i32>,
    #[serde(default)]
    restart_count: u32,
    #[serde(default)]
    next_restart_at_millis: Option<u64>,
    launch_mode: KrunLaunchMode,
    shutdown_requested: bool,
    status: SandboxStatus,
}

#[derive(Debug, Deserialize)]
struct RuntimeStatePayload {
    status: String,
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

fn ensure_linux_host() -> Result<()> {
    if cfg!(target_os = "linux") {
        return Ok(());
    }

    Err(SandboxError::BackendUnavailable {
        message:
            "krun execution requires a Linux host; use plan-only mode for cross-platform tests"
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
        "neovex-sandbox".to_owned()
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

fn configured_stop_signal(image_metadata: &KrunImageMetadata) -> String {
    image_metadata
        .stop_signal
        .as_deref()
        .map(str::trim)
        .filter(|signal| !signal.is_empty())
        .unwrap_or("TERM")
        .to_owned()
}

fn configured_stop_timeout(spec: &SandboxSpec, config: &KrunSandboxBackendConfig) -> Duration {
    spec.lifecycle.stop_timeout.unwrap_or(config.stop_timeout)
}

fn restart_policy_allows_restart(
    policy: SandboxRestartPolicy,
    exit_code: i32,
    restart_count: u32,
) -> bool {
    match policy {
        SandboxRestartPolicy::Never => false,
        SandboxRestartPolicy::OnFailure { max_restarts } => {
            exit_code != 0 && restart_count < max_restarts
        }
        SandboxRestartPolicy::Always { max_restarts } => restart_count < max_restarts,
    }
}

fn restart_backoff_delay(restart_count: u32) -> Duration {
    let initial = u128::from(DEFAULT_RESTART_BACKOFF_INITIAL_MILLIS);
    let max = u128::from(DEFAULT_RESTART_BACKOFF_MAX_MILLIS);
    let multiplier = 1_u128 << restart_count.min(31);
    let millis = initial.saturating_mul(multiplier).min(max);
    Duration::from_millis(millis as u64)
}

fn now_millis() -> Result<u64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!("system clock is before unix epoch: {error}"),
        })?;
    u64::try_from(elapsed.as_millis()).map_err(|_| SandboxError::OperationFailed {
        message: "system clock milliseconds exceed supported range".to_owned(),
    })
}

fn desired_krun_vm_config(spec: &SandboxSpec) -> Result<Option<KrunVmConfig>> {
    let cpu_count = spec.resources.cpu_count;
    let memory_limit_bytes = spec.resources.memory_limit_bytes;

    match (cpu_count, memory_limit_bytes) {
        (None, _) => Ok(None),
        (Some(_), None) => Err(SandboxError::InvalidSpec {
            message:
                "krun sandbox cpu_count requires memory_limit_bytes so crun can configure /.krun_vm.json"
                    .to_owned(),
        }),
        (Some(0), _) => Err(SandboxError::InvalidSpec {
            message: "krun sandbox cpu_count must be greater than zero".to_owned(),
        }),
        (Some(_), Some(0)) => Err(SandboxError::InvalidSpec {
            message: "krun sandbox memory_limit_bytes must be greater than zero".to_owned(),
        }),
        (Some(cpus), Some(memory_limit_bytes)) => {
            let ram_mib = memory_limit_bytes.div_ceil(BYTES_PER_MIB);
            let ram_mib = u32::try_from(ram_mib).map_err(|_| SandboxError::InvalidSpec {
                message: format!(
                    "krun sandbox memory_limit_bytes {memory_limit_bytes} exceeds the maximum supported MiB range"
                ),
            })?;
            Ok(Some(KrunVmConfig { cpus, ram_mib }))
        }
    }
}

fn krun_vm_config_path(rootfs: &Path) -> PathBuf {
    rootfs.join(KRUN_VM_CONFIG_FILENAME)
}

fn krun_vm_config_prelude(spec: &SandboxSpec, needs_unshare_mount: bool) -> Result<Vec<String>> {
    if !needs_unshare_mount {
        return Ok(Vec::new());
    }

    let vm_config_path = krun_vm_config_path(&spec.filesystem.rootfs);
    let escaped_path = shell_escape(vm_config_path.to_string_lossy().as_ref());
    match desired_krun_vm_config(spec)? {
        Some(vm_config) => {
            let rendered = json!({
                "cpus": vm_config.cpus,
                "ram_mib": vm_config.ram_mib,
            })
            .to_string();
            Ok(vec![format!(
                "printf '%s' {} > {}",
                shell_escape(&rendered),
                escaped_path,
            )])
        }
        None => Ok(vec![format!("rm -f {escaped_path}")]),
    }
}

fn resolve_launch_spec(
    spec: &SandboxSpec,
    launch_defaults: Option<&OciImageLaunchDefaults>,
) -> KrunResolvedLaunchSpec {
    let Some(launch_defaults) = launch_defaults else {
        return KrunResolvedLaunchSpec {
            spec: spec.clone(),
            image_metadata: KrunImageMetadata::default(),
        };
    };

    let mut resolved_spec = spec.clone();
    resolved_spec.filesystem =
        resolve_filesystem_spec(&spec.filesystem, &launch_defaults.filesystem);
    resolved_spec.process = resolve_process_spec(&spec.process, &launch_defaults.process);

    KrunResolvedLaunchSpec {
        spec: resolved_spec,
        image_metadata: KrunImageMetadata {
            user: launch_defaults.user.clone(),
            stop_signal: launch_defaults.stop_signal.clone(),
            healthcheck: launch_defaults.healthcheck.clone(),
            labels: launch_defaults.labels.clone(),
            exposed_ports: launch_defaults.exposed_ports.clone(),
        },
    }
}

fn resolve_filesystem_spec(
    spec: &crate::spec::SandboxFilesystemSpec,
    defaults: &crate::spec::SandboxFilesystemSpec,
) -> crate::spec::SandboxFilesystemSpec {
    if !spec.is_unspecified() {
        return spec.clone();
    }

    let mut resolved = defaults.clone();
    resolved.readonly = resolved.readonly || spec.readonly;
    resolved
}

fn resolve_process_spec(
    spec: &crate::spec::SandboxProcessSpec,
    defaults: &crate::spec::SandboxProcessSpec,
) -> crate::spec::SandboxProcessSpec {
    let mut resolved = defaults.clone();

    if !spec.args.is_empty() {
        resolved.args = spec.args.clone();
    }

    resolved.env = if spec.env.is_empty() || spec.uses_default_env() {
        defaults.env.clone()
    } else {
        merge_env_overrides(&defaults.env, &spec.env)
    };

    if !spec.uses_default_cwd() {
        resolved.cwd = spec.cwd.clone();
    }

    resolved.terminal = spec.terminal || defaults.terminal;
    resolved
}

fn apply_guest_user_switch(
    spec: &mut SandboxSpec,
    image_metadata: &KrunImageMetadata,
) -> Result<()> {
    let Some(target_user) = parse_guest_user(image_metadata.user.as_deref())? else {
        return Ok(());
    };

    if spec
        .process
        .args
        .first()
        .is_none_or(|arg| arg != GUEST_USER_HELPER_GUEST_PATH)
    {
        spec.process
            .args
            .insert(0, GUEST_USER_HELPER_GUEST_PATH.to_owned());
    }

    spec.process.env = merge_env_overrides(
        &spec.process.env,
        &[
            format!("{GUEST_USER_UID_ENV}={}", target_user.uid),
            format!("{GUEST_USER_GID_ENV}={}", target_user.gid),
        ],
    );

    Ok(())
}

fn guest_user_switch_mounts(
    config: &KrunSandboxBackendConfig,
    image_metadata: &KrunImageMetadata,
) -> Vec<KrunBundleMount> {
    if image_metadata
        .user
        .as_deref()
        .map(str::trim)
        .is_none_or(str::is_empty)
    {
        return Vec::new();
    }

    vec![KrunBundleMount {
        destination: GUEST_USER_HELPER_GUEST_ROOT.to_owned(),
        source: config.guest_user_helper_root.clone(),
        options: vec!["rbind".to_owned(), "ro".to_owned()],
    }]
}

fn merge_env_overrides(base: &[String], overrides: &[String]) -> Vec<String> {
    let mut merged = base.to_vec();
    for override_entry in overrides {
        let Some(override_key) = env_key(override_entry) else {
            merged.push(override_entry.clone());
            continue;
        };

        if let Some(index) = merged
            .iter()
            .position(|entry| env_key(entry).is_some_and(|key| key == override_key))
        {
            merged[index] = override_entry.clone();
        } else {
            merged.push(override_entry.clone());
        }
    }
    merged
}

fn env_key(entry: &str) -> Option<&str> {
    let (key, _) = entry.split_once('=')?;
    (!key.is_empty()).then_some(key)
}

fn parse_guest_user(user: Option<&str>) -> Result<Option<GuestUserIds>> {
    let Some(user) = user.map(str::trim).filter(|user| !user.is_empty()) else {
        return Ok(None);
    };

    let (uid, gid) = match user.split_once(':') {
        Some((uid, gid)) => (
            parse_guest_user_id("uid", uid, user)?,
            parse_guest_user_id("gid", gid, user)?,
        ),
        None => (parse_guest_user_id("uid", user, user)?, 0),
    };

    Ok(Some(GuestUserIds { uid, gid }))
}

fn parse_guest_user_id(kind: &str, value: &str, user: &str) -> Result<u32> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|_| SandboxError::InvalidSpec {
            message: format!(
                "krun guest-side user switching requires a numeric image user, got {user:?} with invalid {kind} component {value:?}"
            ),
        })
}

fn running_status(manifest: &KrunSandboxManifest) -> SandboxStatus {
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

fn readiness_probe_target(manifest: &KrunSandboxManifest) -> Option<ReadinessProbeTarget> {
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

fn readiness_probe_timeout(manifest: &KrunSandboxManifest) -> Duration {
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
    launch_mode: KrunLaunchMode,
    spec: &SandboxSpec,
    status: SandboxStatus,
) -> Vec<PublishedEndpoint> {
    let endpoints = published_endpoints(spec);
    if launch_mode == KrunLaunchMode::Execute && status != SandboxStatus::Ready {
        Vec::new()
    } else {
        endpoints
    }
}

fn synchronize_handle_status(manifest: &mut KrunSandboxManifest, status: SandboxStatus) {
    manifest.status = status;
    manifest.handle.status = status;
    manifest.handle.published_endpoints =
        visible_published_endpoints(manifest.launch_mode, &manifest.spec, status);
}

fn shell_escape(s: &str) -> String {
    if s.is_empty() {
        return "''".to_owned();
    }
    if s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'/' || b == b'.')
    {
        return s.to_owned();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn remove_if_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    std::fs::remove_file(path).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to remove stale runtime artifact {}: {error}",
            path.display()
        ),
    })
}

fn ensure_guest_user_helper_available(
    config: &KrunSandboxBackendConfig,
    manifest: &KrunSandboxManifest,
) -> Result<()> {
    if manifest
        .image_metadata
        .user
        .as_deref()
        .map(str::trim)
        .is_none_or(str::is_empty)
    {
        return Ok(());
    }

    let helper_path = config
        .guest_user_helper_root
        .join(GUEST_USER_HELPER_BINARY_NAME);
    if helper_path.is_file() {
        return Ok(());
    }

    Err(SandboxError::OperationFailed {
        message: format!(
            "sandbox {} requires guest-side user switching, but helper {} is missing",
            manifest.handle.id,
            helper_path.display()
        ),
    })
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
    let exit_code =
        exit_status
            .trim()
            .parse::<i32>()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to parse sandbox exit status {}: {error}",
                    path.display()
                ),
            })?;
    Ok(exit_code)
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
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener};
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::Duration;

    use flate2::{Compression, write::GzEncoder};
    use futures::executor::block_on;
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use tar::Builder;
    use tempfile::TempDir;

    use neovex_core::TenantId;

    use super::{
        GUEST_USER_GID_ENV, GUEST_USER_HELPER_GUEST_PATH, GUEST_USER_UID_ENV, GuestUserIds,
        KrunImageMetadata, KrunLaunchMode, KrunSandboxBackend, KrunSandboxBackendConfig,
        KrunSandboxManifest, ReadinessProbeTarget, configured_stop_signal, configured_stop_timeout,
        desired_krun_vm_config, krun_vm_config_path, parse_guest_user, probe_target_ready,
        readiness_probe_target, restart_backoff_delay, restart_policy_allows_restart,
        running_status, slugify, visible_published_endpoints,
    };
    use crate::backend::{SandboxBackend, SandboxBackendKind};
    use crate::backends::oci::buildah::{
        ImageHealthcheck, OciExposedPort, OciExposedPortProtocol, OciImageLaunchDefaults,
    };
    use crate::endpoint::PublishedEndpointProtocol;
    use crate::instance::{SandboxId, SandboxStatus};
    use crate::spec::{
        SandboxBuildLaunchSpec, SandboxFilesystemSpec, SandboxImageLaunchSpec,
        SandboxImageProcessOverrides, SandboxPortBinding, SandboxProcessSpec,
        SandboxResourceLimits, SandboxRestartPolicy, SandboxSpec,
    };

    #[test]
    fn plan_only_backend_lowers_through_generic_trait_surface() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let backend: Box<dyn SandboxBackend> = Box::new(KrunSandboxBackend::new(
            KrunSandboxBackendConfig::plan_only(
                temp_dir.path().join("bundles"),
                temp_dir.path().join("state"),
            ),
        ));
        let spec = sample_spec();

        let handle = block_on(backend.start(spec)).expect("plan-only start should succeed");
        assert_eq!(handle.backend, SandboxBackendKind::Krun);
        assert_eq!(handle.status, crate::instance::SandboxStatus::Starting);
        assert_eq!(handle.published_endpoints.len(), 2);

        let inspected = block_on(backend.inspect(&handle.id))
            .expect("inspect should succeed")
            .expect("plan-only sandbox should persist a manifest");
        assert_eq!(inspected.id, handle.id);

        block_on(backend.stop(&handle.id)).expect("stop should succeed in plan-only mode");
        let stopped = block_on(backend.inspect(&handle.id))
            .expect("inspect after stop should succeed")
            .expect("stopped sandbox should still have a manifest");
        assert_eq!(stopped.status, crate::instance::SandboxStatus::Stopped);
    }

    #[test]
    fn plan_only_backend_lowers_image_launch_through_generic_trait_surface() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let image_reference = sample_registry_image_reference();
        let mut config = KrunSandboxBackendConfig::plan_only(
            temp_dir.path().join("bundles"),
            temp_dir.path().join("state"),
        );
        config.use_buildah_unshare = false;
        let backend: Box<dyn SandboxBackend> = Box::new(KrunSandboxBackend::new(config));

        let handle = block_on(backend.start_from_image(SandboxImageLaunchSpec::new(
            sparse_image_spec("image-trait"),
            &image_reference,
        )))
        .expect("plan-only image-backed start should succeed through the trait");

        assert_eq!(handle.backend, SandboxBackendKind::Krun);
        assert_eq!(handle.status, crate::instance::SandboxStatus::Starting);

        let inspected = block_on(backend.inspect(&handle.id))
            .expect("inspect should succeed")
            .expect("plan-only image-backed sandbox should persist a manifest");
        assert_eq!(inspected.id, handle.id);
    }

    #[test]
    fn plan_only_backend_lowers_build_launch_through_generic_trait_surface() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let workspace = temp_dir.path().join("workspace");
        fs::create_dir_all(&workspace).expect("workspace directory should exist");
        let dockerfile_path = workspace.join("Dockerfile");
        fs::write(&dockerfile_path, "FROM scratch\nCMD [\"/bin/true\"]\n")
            .expect("dockerfile should be written");

        let mut config = KrunSandboxBackendConfig::plan_only(
            temp_dir.path().join("bundles"),
            temp_dir.path().join("state"),
        );
        config.use_buildah_unshare = false;
        let backend: Box<dyn SandboxBackend> = Box::new(KrunSandboxBackend::new(config));

        let handle = block_on(backend.start_from_build(SandboxBuildLaunchSpec::new(
            sparse_image_spec("build-trait"),
            "neovex-api",
            &dockerfile_path,
            &workspace,
        )))
        .expect("plan-only build-backed start should succeed through the trait");

        assert_eq!(handle.backend, SandboxBackendKind::Krun);
        assert_eq!(handle.status, crate::instance::SandboxStatus::Starting);

        let inspected = block_on(backend.inspect(&handle.id))
            .expect("inspect should succeed")
            .expect("plan-only build-backed sandbox should persist a manifest");
        assert_eq!(inspected.id, handle.id);
        let manifest_path = temp_dir
            .path()
            .join("state")
            .join("containers")
            .join(handle.id.as_str())
            .join("manifest.json");
        let manifest = fs::read_to_string(&manifest_path).expect("manifest should be readable");
        assert!(
            manifest.contains("\"Rootfs\""),
            "build-backed plan should persist a materialized rootfs launch artifact: {manifest}"
        );
        assert!(
            !manifest.contains("\"MountedRootfs\""),
            "build-backed plan should no longer depend on mounted buildah rootfs sessions: {manifest}"
        );
    }

    #[test]
    fn plan_start_writes_bundle_and_manifest_under_backend_roots() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
            temp_dir.path().join("bundles"),
            temp_dir.path().join("state"),
        ));
        let spec = sample_spec();

        let handle = block_on(backend.start(spec)).expect("plan-only start should succeed");
        let manifest_dir = temp_dir
            .path()
            .join("state")
            .join("containers")
            .join(handle.id.as_str());
        let manifest_path = manifest_dir.join("manifest.json");
        let bundle_path = temp_dir
            .path()
            .join("bundles")
            .join(handle.id.as_str())
            .join("config.json");

        assert!(manifest_path.exists(), "sandbox manifest should be written");
        assert!(bundle_path.exists(), "bundle config should be written");

        let rendered_bundle =
            fs::read_to_string(bundle_path).expect("bundle config should be readable");
        assert!(
            rendered_bundle.contains("\"krun.port_map\": \"15432:5432,18080:8080\""),
            "bundle config should preserve the host:guest TSI mapping"
        );
    }

    #[test]
    fn plan_only_start_writes_krun_vm_config_for_explicit_resource_limits() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let rootfs = temp_dir.path().join("rootfs");
        fs::create_dir_all(&rootfs).expect("rootfs directory should exist");
        let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
            temp_dir.path().join("bundles"),
            temp_dir.path().join("state"),
        ));
        let spec = sample_spec_with_rootfs(&rootfs).with_resource_limits(
            SandboxResourceLimits::default()
                .with_cpu_count(2)
                .with_memory_limit_bytes(256 * 1024 * 1024),
        );

        let handle = block_on(backend.start(spec)).expect("plan-only start should succeed");
        let vm_config_path = krun_vm_config_path(&rootfs);
        let vm_config =
            fs::read_to_string(&vm_config_path).expect("krun vm config should be materialized");
        let bundle = fs::read_to_string(
            temp_dir
                .path()
                .join("bundles")
                .join(handle.id.as_str())
                .join("config.json"),
        )
        .expect("bundle config should be readable");

        assert!(vm_config.contains("\"cpus\": 2"));
        assert!(vm_config.contains("\"ram_mib\": 256"));
        assert!(bundle.contains("\"limit\": 268435456"));
    }

    #[test]
    fn plan_only_start_removes_stale_krun_vm_config_when_cpu_limit_is_unset() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let rootfs = temp_dir.path().join("rootfs");
        fs::create_dir_all(&rootfs).expect("rootfs directory should exist");
        let stale_vm_config = krun_vm_config_path(&rootfs);
        fs::write(&stale_vm_config, "{\"cpus\":4,\"ram_mib\":512}")
            .expect("stale krun vm config should be seeded");
        let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
            temp_dir.path().join("bundles"),
            temp_dir.path().join("state"),
        ));
        let spec = sample_spec_with_rootfs(&rootfs).with_memory_limit_bytes(256 * 1024 * 1024);

        block_on(backend.start(spec)).expect("plan-only start should succeed");

        assert!(
            !stale_vm_config.exists(),
            "memory-only starts should remove stale krun vm config so crun uses the OCI memory limit path"
        );
    }

    #[test]
    fn slugify_normalizes_operator_facing_names() {
        assert_eq!(slugify("Postgres Primary"), "postgres-primary");
        assert_eq!(slugify("db__1"), "db-1");
        assert_eq!(slugify("api@edge"), "api-edge");
    }

    #[test]
    fn plan_start_with_launch_defaults_materializes_sparse_spec_from_image_defaults() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
            temp_dir.path().join("bundles"),
            temp_dir.path().join("state"),
        ));
        let spec = SandboxSpec::new(
            TenantId::new("tenant").expect("tenant id should be valid"),
            "api",
            SandboxBackendKind::Krun,
            SandboxFilesystemSpec::new(PathBuf::new()),
            SandboxProcessSpec::new(Vec::<String>::new()),
        );

        let launch_plan = backend
            .plan_start_with_launch_defaults(&spec, Some(&sample_launch_defaults()))
            .expect("launch defaults should materialize the sparse spec");

        assert_eq!(
            launch_plan.manifest.spec.filesystem.rootfs,
            PathBuf::from("/image/rootfs")
        );
        assert_eq!(
            launch_plan.manifest.spec.process.args,
            vec![
                GUEST_USER_HELPER_GUEST_PATH.to_owned(),
                "/usr/local/bin/service".to_owned(),
                "serve".to_owned(),
            ]
        );
        assert_eq!(
            launch_plan.manifest.spec.process.env,
            vec![
                "PATH=/usr/local/bin:/usr/bin".to_owned(),
                "SERVICE_MODE=prod".to_owned(),
                format!("{GUEST_USER_UID_ENV}=1000"),
                format!("{GUEST_USER_GID_ENV}=1000"),
            ]
        );
        assert_eq!(
            launch_plan.manifest.spec.process.cwd,
            PathBuf::from("/srv/service")
        );
        assert_eq!(
            launch_plan.manifest.image_metadata.stop_signal,
            Some("SIGTERM".to_owned())
        );
        assert_eq!(
            launch_plan.manifest.image_metadata.exposed_ports,
            vec![
                OciExposedPort {
                    port: 8080,
                    protocol: OciExposedPortProtocol::Tcp,
                    raw: "8080/tcp".to_owned(),
                },
                OciExposedPort {
                    port: 8443,
                    protocol: OciExposedPortProtocol::Tcp,
                    raw: "8443/tcp".to_owned(),
                },
            ]
        );

        let rendered_bundle = fs::read_to_string(&launch_plan.manifest.bundle_layout.config_path)
            .expect("bundle config should be readable");
        assert!(
            rendered_bundle.contains(&format!("\"{GUEST_USER_HELPER_GUEST_PATH}\"")),
            "bundle config should wrap the image-default command with the guest user helper"
        );
        // krun bundles always use root for the VMM process (needs /dev/kvm).
        // The image user is stored in the manifest, not the bundle.
        assert!(
            rendered_bundle.contains("\"uid\": 0"),
            "krun bundle should use root uid for VMM /dev/kvm access"
        );
        assert!(
            rendered_bundle.contains("\"gid\": 0"),
            "krun bundle should use root gid for VMM /dev/kvm access"
        );
        assert!(
            rendered_bundle.contains("\"destination\": \"/.neovex\""),
            "bundle config should mount the guest helper root when image USER is set"
        );
    }

    #[test]
    fn plan_start_with_launch_defaults_preserves_explicit_operator_overrides() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
            temp_dir.path().join("bundles"),
            temp_dir.path().join("state"),
        ));
        let spec = SandboxSpec::new(
            TenantId::new("tenant").expect("tenant id should be valid"),
            "api",
            SandboxBackendKind::Krun,
            SandboxFilesystemSpec::new("/operator/rootfs").read_only(true),
            SandboxProcessSpec::new(["/bin/sh", "-lc", "exec custom-api"])
                .with_env(["PATH=/custom/bin", "APP_MODE=dev"])
                .with_cwd("/workspace"),
        )
        .with_port_binding(SandboxPortBinding::tcp("http", 18080, 8080));

        let launch_plan = backend
            .plan_start_with_launch_defaults(&spec, Some(&sample_launch_defaults()))
            .expect("explicit operator overrides should coexist with image defaults");

        assert_eq!(
            launch_plan.manifest.spec.filesystem.rootfs,
            PathBuf::from("/operator/rootfs")
        );
        assert!(launch_plan.manifest.spec.filesystem.readonly);
        assert_eq!(
            launch_plan.manifest.spec.process.args,
            vec![
                GUEST_USER_HELPER_GUEST_PATH.to_owned(),
                "/bin/sh".to_owned(),
                "-lc".to_owned(),
                "exec custom-api".to_owned(),
            ]
        );
        assert_eq!(
            launch_plan.manifest.spec.process.env,
            vec![
                "PATH=/custom/bin".to_owned(),
                "SERVICE_MODE=prod".to_owned(),
                "APP_MODE=dev".to_owned(),
                format!("{GUEST_USER_UID_ENV}=1000"),
                format!("{GUEST_USER_GID_ENV}=1000"),
            ]
        );
        assert_eq!(
            launch_plan.manifest.spec.process.cwd,
            PathBuf::from("/workspace")
        );
        assert!(!launch_plan.manifest.spec.process.terminal);
        assert_eq!(launch_plan.manifest.spec.port_bindings.len(), 1);
        assert_eq!(
            launch_plan.manifest.image_metadata.healthcheck,
            Some(ImageHealthcheck {
                test: vec![
                    "CMD-SHELL".to_owned(),
                    "curl -f http://localhost/health".to_owned()
                ],
                interval: Some(15_000_000_000),
                timeout: Some(3_000_000_000),
                start_period: Some(20_000_000_000),
                retries: Some(5),
            })
        );
    }

    #[test]
    fn start_from_image_plan_only_persists_and_then_cleans_up_materialized_rootfs() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let image_reference = sample_registry_image_reference();

        let mut config = KrunSandboxBackendConfig::plan_only(
            temp_dir.path().join("bundles"),
            temp_dir.path().join("state"),
        );
        config.use_buildah_unshare = false;

        let backend = KrunSandboxBackend::new(config);
        let spec = SandboxSpec::new(
            TenantId::new("tenant").expect("tenant id should be valid"),
            "image-backed-api",
            SandboxBackendKind::Krun,
            SandboxFilesystemSpec::new(PathBuf::new()),
            SandboxProcessSpec::new(Vec::<String>::new()),
        )
        .with_port_binding(SandboxPortBinding::tcp("http", 18080, 8080));

        let handle = block_on(
            backend.start_from_image(
                SandboxImageLaunchSpec::new(spec, &image_reference)
                    .with_process_overrides(SandboxImageProcessOverrides::default()),
            ),
        )
        .expect("plan-only image-backed start should succeed");

        let manifest_path = temp_dir
            .path()
            .join("state")
            .join("containers")
            .join(handle.id.as_str())
            .join("manifest.json");
        let manifest_before_stop =
            fs::read_to_string(&manifest_path).expect("manifest should be readable before stop");
        assert!(
            manifest_before_stop.contains("\"launch_artifact\""),
            "manifest should retain launch-artifact metadata while running"
        );
        let rootfs_path = temp_dir
            .path()
            .join("state")
            .join("materialized-rootfs")
            .join(handle.id.as_str());
        assert!(
            rootfs_path.exists(),
            "image-backed plan should materialize a rootfs under the krun state root"
        );

        block_on(backend.stop(&handle.id)).expect("plan-only stop should succeed");

        let manifest_after_stop =
            fs::read_to_string(&manifest_path).expect("manifest should be readable after stop");
        assert!(
            manifest_after_stop.contains("\"launch_artifact\": null"),
            "stop should clear launch-artifact metadata after cleanup"
        );
        assert!(
            !rootfs_path.exists(),
            "stop should remove the materialized rootfs after cleanup"
        );
    }

    #[test]
    fn start_from_image_plan_only_skips_krun_vm_config_prelude_for_materialized_rootfs() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let image_reference = sample_registry_image_reference();

        let mut config = KrunSandboxBackendConfig::plan_only(
            temp_dir.path().join("bundles"),
            temp_dir.path().join("state"),
        );
        config.use_buildah_unshare = true;

        let backend = KrunSandboxBackend::new(config);
        let spec = sparse_image_spec("image-with-limits").with_resource_limits(
            SandboxResourceLimits::default()
                .with_cpu_count(2)
                .with_memory_limit_bytes(256 * 1024 * 1024),
        );

        let launch_plan = backend
            .plan_start_from_image(
                &spec,
                &image_reference,
                &SandboxImageProcessOverrides::default(),
            )
            .expect("image-backed plan should succeed");

        let script = launch_plan
            .manifest
            .conmon_launch
            .create_command
            .args
            .join(" ");
        assert!(
            !script.contains(".krun_vm.json"),
            "materialized rootfs launches should write krun vm config directly, not via a buildah unshare prelude: {script}"
        );
    }

    #[test]
    fn start_from_image_plan_only_auto_assigns_exposed_ports_and_reuses_released_ports() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let image_reference = sample_registry_image_reference();

        let mut config = KrunSandboxBackendConfig::plan_only(
            temp_dir.path().join("bundles"),
            temp_dir.path().join("state"),
        );
        config.use_buildah_unshare = false;
        config.published_port_range = 15000..=15001;

        let backend = KrunSandboxBackend::new(config);

        let first = block_on(backend.start_from_image(SandboxImageLaunchSpec::new(
            sparse_image_spec("first"),
            &image_reference,
        )))
        .expect("first plan-only image-backed start should succeed");
        let first_inspected = block_on(backend.inspect(&first.id))
            .expect("inspect should succeed")
            .expect("first sandbox should be persisted");
        assert_eq!(first_inspected.published_endpoints.len(), 1);
        assert_eq!(first_inspected.published_endpoints[0].address.port(), 15000);

        let second = block_on(backend.start_from_image(SandboxImageLaunchSpec::new(
            sparse_image_spec("second"),
            &image_reference,
        )))
        .expect("second plan-only image-backed start should succeed");
        let second_inspected = block_on(backend.inspect(&second.id))
            .expect("inspect should succeed")
            .expect("second sandbox should be persisted");
        assert_eq!(second_inspected.published_endpoints.len(), 1);
        assert_eq!(
            second_inspected.published_endpoints[0].address.port(),
            15001
        );

        block_on(backend.stop(&first.id)).expect("stopping the first sandbox should succeed");

        let third = block_on(backend.start_from_image(SandboxImageLaunchSpec::new(
            sparse_image_spec("third"),
            &image_reference,
        )))
        .expect("third plan-only image-backed start should succeed");
        let third_inspected = block_on(backend.inspect(&third.id))
            .expect("inspect should succeed")
            .expect("third sandbox should be persisted");
        assert_eq!(third_inspected.published_endpoints.len(), 1);
        assert_eq!(third_inspected.published_endpoints[0].address.port(), 15000);

        let third_bundle = fs::read_to_string(
            temp_dir
                .path()
                .join("bundles")
                .join(third.id.as_str())
                .join("config.json"),
        )
        .expect("third bundle config should be readable");
        assert!(
            third_bundle.contains("\"krun.port_map\": \"15000:8080\""),
            "auto-assigned bindings should rewrite the krun port map annotation"
        );
    }

    #[test]
    fn configured_stop_signal_prefers_image_metadata_and_falls_back_to_term() {
        assert_eq!(
            configured_stop_signal(&sample_image_metadata().with_stop_signal("SIGQUIT")),
            "SIGQUIT"
        );
        assert_eq!(
            configured_stop_signal(&sample_image_metadata().with_stop_signal("  ")),
            "TERM"
        );
        assert_eq!(
            configured_stop_signal(&KrunImageMetadata::default()),
            "TERM"
        );
    }

    #[test]
    fn configured_stop_timeout_prefers_sandbox_lifecycle_and_falls_back_to_backend_default() {
        let backend_default = KrunSandboxBackendConfig {
            stop_timeout: Duration::from_secs(5),
            ..KrunSandboxBackendConfig::default()
        };
        assert_eq!(
            configured_stop_timeout(
                &sample_spec().with_stop_timeout(Duration::from_secs(30)),
                &backend_default,
            ),
            Duration::from_secs(30)
        );
        assert_eq!(
            configured_stop_timeout(&sample_spec(), &backend_default),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn parse_guest_user_accepts_numeric_uid_and_uid_gid() {
        assert_eq!(
            parse_guest_user(Some("1234")).expect("uid should parse"),
            Some(GuestUserIds { uid: 1234, gid: 0 })
        );
        assert_eq!(
            parse_guest_user(Some("1234:5678")).expect("uid:gid should parse"),
            Some(GuestUserIds {
                uid: 1234,
                gid: 5678
            })
        );
        assert_eq!(
            parse_guest_user(Some(" ")).expect("blank user should be ignored"),
            None
        );
    }

    #[test]
    fn parse_guest_user_rejects_non_numeric_components() {
        let error = parse_guest_user(Some("postgres:postgres"))
            .expect_err("guest user switching should require numeric ids by this stage");
        assert!(
            error.to_string().contains("requires a numeric image user"),
            "expected actionable numeric-user error, got: {error}"
        );
    }

    #[test]
    fn readiness_probe_target_prefers_http_endpoints() {
        let spec = SandboxSpec::new(
            TenantId::new("tenant").expect("tenant id should be valid"),
            "api",
            SandboxBackendKind::Krun,
            SandboxFilesystemSpec::new("/srv/rootfs"),
            SandboxProcessSpec::new(["/bin/service"]),
        )
        .with_port_bindings([
            SandboxPortBinding::tcp("postgres", 15432, 5432),
            SandboxPortBinding::new("http", PublishedEndpointProtocol::Http, 18080, 8080),
        ]);
        let manifest = sample_manifest(spec, KrunLaunchMode::Execute);

        assert_eq!(
            readiness_probe_target(&manifest),
            Some(ReadinessProbeTarget::Http(SocketAddr::from((
                [127, 0, 0, 1],
                18080
            ))))
        );
    }

    #[test]
    fn probe_target_ready_succeeds_for_http_listener() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener should bind");
        let address = listener
            .local_addr()
            .expect("listener should report local addr");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("listener should accept");
            let mut request = [0_u8; 256];
            let _ = stream.read(&mut request);
            stream
                .write_all(b"HTTP/1.0 204 No Content\r\nContent-Length: 0\r\n\r\n")
                .expect("server should write response");
        });

        assert!(
            probe_target_ready(ReadinessProbeTarget::Http(address), Duration::from_secs(1)),
            "expected HTTP readiness probe to pass against local listener"
        );
        server.join().expect("server thread should join");
    }

    #[test]
    fn running_status_stays_starting_until_probe_passes() {
        let unused_listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener should bind");
        let address = unused_listener
            .local_addr()
            .expect("listener should report local addr");
        drop(unused_listener);

        let spec = SandboxSpec::new(
            TenantId::new("tenant").expect("tenant id should be valid"),
            "tcp-service",
            SandboxBackendKind::Krun,
            SandboxFilesystemSpec::new("/srv/rootfs"),
            SandboxProcessSpec::new(["/bin/service"]),
        )
        .with_port_binding(SandboxPortBinding::tcp("tcp", address.port(), 8080));
        let manifest = sample_manifest(spec, KrunLaunchMode::Execute);

        assert_eq!(running_status(&manifest), SandboxStatus::Starting);
    }

    #[test]
    fn running_status_degrades_ready_sandboxes_to_not_ready_on_probe_failure() {
        let unused_listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener should bind");
        let address = unused_listener
            .local_addr()
            .expect("listener should report local addr");
        drop(unused_listener);

        let spec = SandboxSpec::new(
            TenantId::new("tenant").expect("tenant id should be valid"),
            "http-service",
            SandboxBackendKind::Krun,
            SandboxFilesystemSpec::new("/srv/rootfs"),
            SandboxProcessSpec::new(["/bin/service"]),
        )
        .with_port_binding(SandboxPortBinding::new(
            "http",
            PublishedEndpointProtocol::Http,
            address.port(),
            8080,
        ));
        let mut manifest = sample_manifest(spec, KrunLaunchMode::Execute);
        manifest.status = SandboxStatus::Ready;
        manifest.handle.status = SandboxStatus::Ready;
        manifest.handle.published_endpoints = visible_published_endpoints(
            KrunLaunchMode::Execute,
            &manifest.spec,
            SandboxStatus::Ready,
        );

        assert_eq!(running_status(&manifest), SandboxStatus::NotReady);
    }

    #[test]
    fn running_status_recovers_not_ready_sandboxes_when_probe_returns() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener should bind");
        let address = listener
            .local_addr()
            .expect("listener should report local addr");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("listener should accept");
            let mut request = [0_u8; 256];
            let _ = stream.read(&mut request);
            stream
                .write_all(b"HTTP/1.0 204 No Content\r\nContent-Length: 0\r\n\r\n")
                .expect("server should write response");
        });

        let spec = SandboxSpec::new(
            TenantId::new("tenant").expect("tenant id should be valid"),
            "http-service",
            SandboxBackendKind::Krun,
            SandboxFilesystemSpec::new("/srv/rootfs"),
            SandboxProcessSpec::new(["/bin/service"]),
        )
        .with_port_binding(SandboxPortBinding::new(
            "http",
            PublishedEndpointProtocol::Http,
            address.port(),
            8080,
        ));
        let mut manifest = sample_manifest(spec, KrunLaunchMode::Execute);
        manifest.status = SandboxStatus::NotReady;
        manifest.handle.status = SandboxStatus::NotReady;

        assert_eq!(running_status(&manifest), SandboxStatus::Ready);
        server.join().expect("server thread should join");
    }

    #[test]
    fn detect_runtime_status_marks_stale_pidfiles_as_failed() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
            temp_dir.path().join("bundles"),
            temp_dir.path().join("state"),
        ));
        let mut manifest = backend
            .plan_start_with_id(&sample_spec(), &SandboxId::new("db-01"), None, None)
            .expect("plan should lower")
            .manifest;
        let state_stub = temp_dir.path().join("krun-state");
        fs::write(&state_stub, "#!/bin/sh\nexit 1\n").expect("state stub should write");
        let mut permissions = fs::metadata(&state_stub)
            .expect("state stub metadata should resolve")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&state_stub, permissions)
            .expect("state stub permissions should update");
        manifest.conmon_launch.state_command.program = state_stub;
        fs::write(&manifest.conmon_layout.pidfile, "999999\n").expect("pidfile should write");

        assert_eq!(
            backend
                .detect_runtime_status(&manifest)
                .expect("status should resolve"),
            SandboxStatus::Failed
        );
    }

    #[test]
    fn visible_published_endpoints_hide_execute_mode_endpoints_until_ready() {
        let spec = sample_spec();

        assert!(
            visible_published_endpoints(KrunLaunchMode::Execute, &spec, SandboxStatus::Starting)
                .is_empty(),
            "execute-mode sandboxes should not publish endpoints before readiness succeeds"
        );
        assert_eq!(
            visible_published_endpoints(KrunLaunchMode::Execute, &spec, SandboxStatus::Ready).len(),
            2
        );
        assert!(
            visible_published_endpoints(KrunLaunchMode::Execute, &spec, SandboxStatus::NotReady)
                .is_empty(),
            "execute-mode sandboxes should withdraw endpoints when liveness probes regress"
        );
        assert_eq!(
            visible_published_endpoints(KrunLaunchMode::PlanOnly, &spec, SandboxStatus::Starting)
                .len(),
            2,
            "plan-only starts should retain published endpoints for deterministic tests"
        );
    }

    #[test]
    fn restart_policy_allows_expected_restart_shapes() {
        assert!(
            !restart_policy_allows_restart(SandboxRestartPolicy::Never, 42, 0),
            "never policy should not restart"
        );
        assert!(
            restart_policy_allows_restart(
                SandboxRestartPolicy::OnFailure { max_restarts: 1 },
                42,
                0
            ),
            "on-failure should restart non-zero exits within budget"
        );
        assert!(
            !restart_policy_allows_restart(
                SandboxRestartPolicy::OnFailure { max_restarts: 1 },
                0,
                0
            ),
            "on-failure should not restart clean exits"
        );
        assert!(
            !restart_policy_allows_restart(SandboxRestartPolicy::Always { max_restarts: 1 }, 42, 1),
            "restart budget should cap repeated restarts"
        );
    }

    #[test]
    fn restart_backoff_delay_grows_and_caps() {
        assert_eq!(restart_backoff_delay(0), Duration::from_secs(1));
        assert_eq!(restart_backoff_delay(1), Duration::from_secs(2));
        assert_eq!(restart_backoff_delay(2), Duration::from_secs(4));
        assert_eq!(restart_backoff_delay(6), Duration::from_secs(60));
        assert_eq!(restart_backoff_delay(12), Duration::from_secs(60));
    }

    #[test]
    fn manifest_deserialization_defaults_restart_fields_for_pre_restart_manifests() {
        let manifest: KrunSandboxManifest = serde_json::from_value(json!({
            "handle": {
                "id": "sandbox-01",
                "name": "legacy",
                "backend": "krun",
                "status": "starting",
                "published_endpoints": [],
            },
            "spec": {
                "tenant_id": "tenant",
                "name": "legacy",
                "backend": "krun",
                "filesystem": {
                    "rootfs": "/srv/rootfs",
                    "readonly": false,
                },
                "process": {
                    "args": ["/bin/service"],
                    "env": ["PATH=/usr/bin"],
                    "cwd": "/",
                    "terminal": false,
                },
                "resources": {
                    "cpu_count": null,
                    "memory_limit_bytes": null,
                },
                "port_bindings": [],
            },
            "image_metadata": {},
            "launch_artifact": null,
            "bundle_layout": {
                "bundle_dir": "/tmp/bundle",
                "config_path": "/tmp/bundle/config.json",
            },
            "conmon_layout": {
                "state_root": "/tmp/state",
                "container_state_dir": "/tmp/state/containers/sandbox-01",
                "exit_dir": "/tmp/state/exits",
                "persist_dir": "/tmp/state/persist/sandbox-01",
                "ctr_log": "/tmp/state/containers/sandbox-01/ctr.log",
                "oci_log": "/tmp/state/containers/sandbox-01/oci.log",
                "pidfile": "/tmp/state/containers/sandbox-01/pidfile",
                "conmon_pidfile": "/tmp/state/containers/sandbox-01/conmon.pid",
                "exit_status_file": "/tmp/state/exits/sandbox-01",
                "manifest_path": "/tmp/state/containers/sandbox-01/manifest.json",
            },
            "conmon_launch": {
                "create_command": {
                    "program": "/usr/bin/conmon",
                    "args": [],
                },
                "state_command": {
                    "program": "/usr/libexec/neovex/crun",
                    "args": ["state", "sandbox-01"],
                },
                "start_command": {
                    "program": "/usr/libexec/neovex/crun",
                    "args": ["start", "sandbox-01"],
                },
            },
            "last_exit_code": null,
            "launch_mode": "execute",
            "shutdown_requested": false,
            "status": "starting",
        }))
        .expect("legacy manifest should deserialize with new defaults");

        assert_eq!(manifest.restart_count, 0);
        assert_eq!(
            manifest.spec.lifecycle.restart_policy,
            SandboxRestartPolicy::Never
        );
        assert_eq!(manifest.spec.lifecycle.stop_timeout, None);
        assert!(
            manifest
                .conmon_launch
                .delete_command
                .program
                .as_os_str()
                .is_empty(),
            "legacy manifests should default the delete command instead of failing to deserialize"
        );
    }

    fn sample_spec() -> SandboxSpec {
        sample_spec_with_rootfs(Path::new("/srv/rootfs"))
    }

    fn sample_spec_with_rootfs(rootfs: &Path) -> SandboxSpec {
        SandboxSpec::new(
            TenantId::new("tenant").expect("tenant id should be valid"),
            "postgres-primary",
            SandboxBackendKind::Krun,
            SandboxFilesystemSpec::new(rootfs),
            SandboxProcessSpec::new(["/usr/bin/postgres", "-D", "/var/lib/postgresql/data"])
                .with_env(["PATH=/usr/bin", "PGDATA=/var/lib/postgresql/data"]),
        )
        .with_port_bindings([
            SandboxPortBinding::tcp("postgres", 15432, 5432),
            SandboxPortBinding::tcp("health", 18080, 8080),
        ])
    }

    fn sparse_image_spec(name: &str) -> SandboxSpec {
        SandboxSpec::new(
            TenantId::new("tenant").expect("tenant id should be valid"),
            name,
            SandboxBackendKind::Krun,
            SandboxFilesystemSpec::new(PathBuf::new()),
            SandboxProcessSpec::new(Vec::<String>::new()),
        )
    }

    fn sample_launch_defaults() -> OciImageLaunchDefaults {
        OciImageLaunchDefaults {
            filesystem: SandboxFilesystemSpec::new("/image/rootfs"),
            process: SandboxProcessSpec::new(["/usr/local/bin/service", "serve"])
                .with_env(["PATH=/usr/local/bin:/usr/bin", "SERVICE_MODE=prod"])
                .with_cwd("/srv/service"),
            exposed_ports: vec![
                OciExposedPort {
                    port: 8080,
                    protocol: OciExposedPortProtocol::Tcp,
                    raw: "8080/tcp".to_owned(),
                },
                OciExposedPort {
                    port: 8443,
                    protocol: OciExposedPortProtocol::Tcp,
                    raw: "8443/tcp".to_owned(),
                },
            ],
            user: Some("1000:1000".to_owned()),
            stop_signal: Some("SIGTERM".to_owned()),
            healthcheck: Some(ImageHealthcheck {
                test: vec![
                    "CMD-SHELL".to_owned(),
                    "curl -f http://localhost/health".to_owned(),
                ],
                interval: Some(15_000_000_000),
                timeout: Some(3_000_000_000),
                start_period: Some(20_000_000_000),
                retries: Some(5),
            }),
            labels: BTreeMap::from([("com.example.service".to_owned(), "edge".to_owned())]),
        }
    }

    fn sample_image_metadata() -> KrunImageMetadata {
        KrunImageMetadata::default()
    }

    fn sample_manifest(spec: SandboxSpec, launch_mode: KrunLaunchMode) -> KrunSandboxManifest {
        let endpoints = visible_published_endpoints(launch_mode, &spec, SandboxStatus::Starting);
        KrunSandboxManifest {
            handle: crate::instance::SandboxHandle::new(
                crate::instance::SandboxId::new("sandbox-01"),
                spec.name.clone(),
                SandboxBackendKind::Krun,
                SandboxStatus::Starting,
                endpoints,
            ),
            spec,
            image_metadata: KrunImageMetadata::default(),
            launch_artifact: None,
            bundle_layout: super::KrunBundleLayout::new("/tmp/bundle"),
            conmon_layout: super::OciConmonLayout::new(
                "/tmp/state",
                &crate::instance::SandboxId::new("sandbox-01"),
            ),
            conmon_launch: super::OciConmonLaunchPlan {
                create_command: super::CommandSpec::new("/bin/true"),
                state_command: super::CommandSpec::new("/bin/true"),
                start_command: super::CommandSpec::new("/bin/true"),
                delete_command: super::CommandSpec::new("/bin/true"),
            },
            last_exit_code: None,
            restart_count: 0,
            next_restart_at_millis: None,
            launch_mode,
            shutdown_requested: false,
            status: SandboxStatus::Starting,
        }
    }

    fn sample_registry_image_reference() -> String {
        let listener =
            TcpListener::bind("127.0.0.1:0").expect("fake OCI registry listener should bind");
        let address = listener
            .local_addr()
            .expect("fake OCI registry address should resolve");

        let mut layer_archive = Vec::new();
        {
            let mut encoder = GzEncoder::new(&mut layer_archive, Compression::default());
            {
                let mut tar = Builder::new(&mut encoder);
                let file_contents = b"#!/bin/sh\necho hello from demo\n";
                let mut header = tar::Header::new_gnu();
                header.set_mode(0o755);
                header.set_size(file_contents.len() as u64);
                header.set_cksum();
                tar.append_data(&mut header, "usr/local/bin/demo", &file_contents[..])
                    .expect("fake OCI layer file should append");

                let passwd_contents = b"demo:x:1000:1000:Demo:/workspace:/bin/sh\n";
                let mut passwd_header = tar::Header::new_gnu();
                passwd_header.set_mode(0o644);
                passwd_header.set_size(passwd_contents.len() as u64);
                passwd_header.set_cksum();
                tar.append_data(&mut passwd_header, "etc/passwd", &passwd_contents[..])
                    .expect("fake OCI passwd should append");

                let group_contents = b"demo:x:1000:\n";
                let mut group_header = tar::Header::new_gnu();
                group_header.set_mode(0o644);
                group_header.set_size(group_contents.len() as u64);
                group_header.set_cksum();
                tar.append_data(&mut group_header, "etc/group", &group_contents[..])
                    .expect("fake OCI group should append");
                tar.finish().expect("fake OCI tar archive should finish");
            }
            encoder
                .finish()
                .expect("fake OCI gzip archive should finish");
        }

        let config = serde_json::json!({
            "architecture": "amd64",
            "os": "linux",
            "config": {
                "Entrypoint": ["/usr/local/bin/demo"],
                "Cmd": ["serve"],
                "Env": ["PATH=/usr/local/bin:/usr/bin", "SERVICE_MODE=prod"],
                "User": "demo",
                "WorkingDir": "/workspace",
                "ExposedPorts": {
                    "8080/tcp": {}
                },
                "Labels": {
                    "app": "demo"
                }
            }
        });
        let config_bytes = serde_json::to_vec(&config).expect("fake OCI config should serialize");
        let config_digest = format!("sha256:{:x}", Sha256::digest(&config_bytes));
        let layer_digest = format!("sha256:{:x}", Sha256::digest(&layer_archive));
        let child_manifest = serde_json::json!({
            "schemaVersion": 2,
            "config": {
                "mediaType": "application/vnd.oci.image.config.v1+json",
                "size": config_bytes.len(),
                "digest": config_digest
            },
            "layers": [{
                "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
                "size": layer_archive.len(),
                "digest": layer_digest
            }]
        });
        let child_manifest_bytes =
            serde_json::to_vec(&child_manifest).expect("fake OCI child manifest should serialize");
        let child_manifest_digest = format!("sha256:{:x}", Sha256::digest(&child_manifest_bytes));
        let index_manifest = serde_json::json!({
            "schemaVersion": 2,
            "manifests": [{
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "size": child_manifest_bytes.len(),
                "digest": child_manifest_digest,
                "platform": {
                    "architecture": if cfg!(target_arch = "aarch64") { "aarch64" } else { "x86_64" },
                    "os": "linux"
                }
            }]
        });
        let index_manifest_bytes =
            serde_json::to_vec(&index_manifest).expect("fake OCI index manifest should serialize");

        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.expect("fake OCI registry connection should accept");
                let mut buffer = [0_u8; 4096];
                let read = stream
                    .read(&mut buffer)
                    .expect("fake OCI registry request should read");
                let request = String::from_utf8_lossy(&buffer[..read]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/");

                let (status, body) = match path {
                    "/v2/" => (200, Vec::new()),
                    "/v2/library/demo/manifests/latest" => (200, index_manifest_bytes.clone()),
                    _ if path == format!("/v2/library/demo/manifests/{child_manifest_digest}") => {
                        (200, child_manifest_bytes.clone())
                    }
                    _ if path == format!("/v2/library/demo/blobs/{config_digest}") => {
                        (200, config_bytes.clone())
                    }
                    _ if path == format!("/v2/library/demo/blobs/{layer_digest}") => {
                        (200, layer_archive.clone())
                    }
                    _ => (404, Vec::new()),
                };

                let response = format!(
                    "HTTP/1.1 {status} {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    if status == 200 { "OK" } else { "Not Found" },
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("fake OCI registry response head should write");
                stream
                    .write_all(&body)
                    .expect("fake OCI registry response body should write");
            }
        });

        format!("docker://localhost:{}/library/demo:latest", address.port())
    }

    #[test]
    fn desired_krun_vm_config_requires_memory_when_cpu_count_is_requested() {
        let error = desired_krun_vm_config(
            &sample_spec().with_resource_limits(SandboxResourceLimits::default().with_cpu_count(2)),
        )
        .expect_err("cpu-only krun resource requests should be rejected");

        assert!(
            error
                .to_string()
                .contains("cpu_count requires memory_limit_bytes"),
            "expected actionable validation error, got: {error}"
        );
    }

    trait ImageMetadataTestExt {
        fn with_stop_signal(self, stop_signal: &str) -> Self;
    }

    impl ImageMetadataTestExt for KrunImageMetadata {
        fn with_stop_signal(mut self, stop_signal: &str) -> Self {
            self.stop_signal = Some(stop_signal.to_owned());
            self
        }
    }
}
