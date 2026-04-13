use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use super::buildah::{
    BuildahCli, BuildahContainer, ImageHealthcheck, OciExposedPort, OciImageLaunchDefaults,
    OciProcessOverrides, PreparedImageLaunch,
};
use super::bundle::{KrunBundleLayout, KrunBundleOptions, write_bundle_config};
use super::command::CommandSpec;
use super::conmon::{KrunConmonConfig, KrunConmonLaunchPlan, KrunConmonLayout, build_launch_plan};
use crate::backend::{SandboxBackend, SandboxBackendKind, SandboxFuture};
use crate::endpoint::PublishedEndpoint;
use crate::error::{Result, SandboxError};
use crate::instance::{SandboxHandle, SandboxId, SandboxStatus};
use crate::spec::SandboxSpec;

const DEFAULT_RUNTIME_PATH: &str = "/usr/libexec/neovex/crun";
const DEFAULT_CONMON_PATH: &str = "conmon";
const DEFAULT_BUILDAH_PATH: &str = "buildah";
const DEFAULT_START_TIMEOUT_SECS: u64 = 10;
const DEFAULT_STOP_TIMEOUT_SECS: u64 = 5;

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
    pub use_buildah_unshare: bool,
    pub launch_mode: KrunLaunchMode,
    pub log_level: String,
    pub start_timeout: Duration,
    pub stop_timeout: Duration,
}

impl KrunSandboxBackendConfig {
    pub fn plan_only(bundle_root: impl Into<PathBuf>, state_root: impl Into<PathBuf>) -> Self {
        let mut config = Self::default();
        config.bundle_root = bundle_root.into();
        config.state_root = state_root.into();
        config.launch_mode = KrunLaunchMode::PlanOnly;
        config
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
            use_buildah_unshare: true,
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

    fn start_from_image_sync(
        &self,
        spec: SandboxSpec,
        image_reference: String,
        overrides: OciProcessOverrides,
    ) -> Result<SandboxHandle> {
        let launch_plan = self.plan_start_from_image(&spec, &image_reference, &overrides)?;
        self.finish_start(launch_plan)
    }

    fn start_from_build_sync(
        &self,
        spec: SandboxSpec,
        image_name: String,
        dockerfile_path: PathBuf,
        context_path: PathBuf,
        overrides: OciProcessOverrides,
    ) -> Result<SandboxHandle> {
        let launch_plan = self.plan_start_from_build(
            &spec,
            &image_name,
            &dockerfile_path,
            &context_path,
            &overrides,
        )?;
        self.finish_start(launch_plan)
    }

    fn finish_start(&self, launch_plan: KrunLaunchPlan) -> Result<SandboxHandle> {
        match self.config.launch_mode {
            KrunLaunchMode::PlanOnly => {
                let mut manifest = launch_plan.manifest.clone();
                manifest.last_exit_code = None;
                manifest.shutdown_requested = false;
                self.write_manifest(&manifest)?;
                Ok(manifest.handle)
            }
            KrunLaunchMode::Execute => self.execute_start(&launch_plan).inspect_err(|_| {
                let _ = self.cleanup_manifest_buildah_artifacts(&launch_plan.manifest);
            }),
        }
    }

    fn inspect_sync(&self, id: &SandboxId) -> Result<Option<SandboxHandle>> {
        let Some(mut manifest) = self.read_manifest(id)? else {
            return Ok(None);
        };

        manifest.status = match self.config.launch_mode {
            KrunLaunchMode::PlanOnly => manifest.status,
            KrunLaunchMode::Execute => self.detect_runtime_status(&manifest)?,
        };
        manifest.handle.status = manifest.status;
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
                self.cleanup_manifest_buildah_artifacts(&manifest)?;
                manifest.buildah_container = None;
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
        overrides: &OciProcessOverrides,
    ) -> Result<KrunLaunchPlan> {
        let sandbox_id = next_sandbox_id(&spec.name);
        let prepared_launch = self.prepare_image_launch(&sandbox_id, image_reference, overrides)?;
        self.plan_start_with_prepared_launch(spec, &sandbox_id, prepared_launch)
    }

    pub(crate) fn plan_start_from_build(
        &self,
        spec: &SandboxSpec,
        image_name: &str,
        dockerfile_path: &Path,
        context_path: &Path,
        overrides: &OciProcessOverrides,
    ) -> Result<KrunLaunchPlan> {
        let sandbox_id = next_sandbox_id(&spec.name);
        let prepared_launch = self.prepare_built_image_launch(
            &sandbox_id,
            image_name,
            dockerfile_path,
            context_path,
            overrides,
        )?;
        self.plan_start_with_prepared_launch(spec, &sandbox_id, prepared_launch)
    }

    pub fn start_from_image(
        &self,
        spec: SandboxSpec,
        image_reference: String,
        overrides: OciProcessOverrides,
    ) -> SandboxFuture<SandboxHandle> {
        let backend = self.clone();
        Box::pin(async move { backend.start_from_image_sync(spec, image_reference, overrides) })
    }

    pub fn start_from_build(
        &self,
        spec: SandboxSpec,
        image_name: String,
        dockerfile_path: PathBuf,
        context_path: PathBuf,
        overrides: OciProcessOverrides,
    ) -> SandboxFuture<SandboxHandle> {
        let backend = self.clone();
        Box::pin(async move {
            backend.start_from_build_sync(
                spec,
                image_name,
                dockerfile_path,
                context_path,
                overrides,
            )
        })
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

    fn plan_start_with_prepared_launch(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        prepared_launch: PreparedImageLaunch,
    ) -> Result<KrunLaunchPlan> {
        self.plan_start_with_id(
            spec,
            sandbox_id,
            Some(&prepared_launch.launch_defaults),
            Some(prepared_launch.container),
        )
    }

    fn plan_start_with_id(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        launch_defaults: Option<&OciImageLaunchDefaults>,
        buildah_container: Option<BuildahContainer>,
    ) -> Result<KrunLaunchPlan> {
        if spec.backend != SandboxBackendKind::Krun {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "krun backend cannot lower sandbox spec for backend {:?}",
                    spec.backend
                ),
            });
        }

        let resolved_launch = resolve_launch_spec(spec, launch_defaults);
        let bundle_layout =
            KrunBundleLayout::new(self.config.bundle_root.join(sandbox_id.as_str()));
        write_bundle_config(
            &bundle_layout,
            &hostname_for(&resolved_launch.spec),
            &resolved_launch.spec,
            &KrunBundleOptions {
                process_user: resolved_launch.image_metadata.user.clone(),
            },
        )?;

        let conmon_layout = KrunConmonLayout::new(&self.config.state_root, &sandbox_id);
        conmon_layout
            .ensure_directories()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to create krun state directories under {}: {error}",
                    self.config.state_root.display()
                ),
            })?;

        let conmon_launch = build_launch_plan(
            &KrunConmonConfig {
                conmon_path: self.config.conmon_path.clone(),
                runtime_path: self.config.runtime_path.clone(),
                buildah_path: self.config.buildah_path.clone(),
                use_buildah_unshare: self.config.use_buildah_unshare,
                log_level: self.config.log_level.clone(),
            },
            &conmon_layout,
            &sandbox_id,
            &spec.name,
            &bundle_layout.bundle_dir,
            buildah_container
                .as_ref()
                .map(|c| c.container_name.as_str()),
        );

        let handle = SandboxHandle::new(
            sandbox_id.clone(),
            resolved_launch.spec.name.clone(),
            SandboxBackendKind::Krun,
            SandboxStatus::Starting,
            published_endpoints(&resolved_launch.spec),
        );
        let manifest = KrunSandboxManifest {
            handle,
            spec: resolved_launch.spec,
            image_metadata: resolved_launch.image_metadata,
            buildah_container,
            bundle_layout,
            conmon_layout,
            conmon_launch,
            last_exit_code: None,
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
        overrides: &OciProcessOverrides,
    ) -> Result<PreparedImageLaunch> {
        self.buildah_cli().prepare_image_launch(
            &buildah_container_name(sandbox_id),
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
        overrides: &OciProcessOverrides,
    ) -> Result<PreparedImageLaunch> {
        self.buildah_cli().prepare_built_image_launch(
            image_name,
            &buildah_container_name(sandbox_id),
            dockerfile_path,
            context_path,
            overrides,
        )
    }

    fn buildah_cli(&self) -> BuildahCli {
        BuildahCli::new(self.config.buildah_path.clone())
            .with_unshare(self.config.use_buildah_unshare)
    }

    fn cleanup_manifest_buildah_artifacts(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        if let Some(container) = &manifest.buildah_container {
            self.buildah_cli()
                .cleanup_container(&container.container_name)?;
        }
        Ok(())
    }

    fn execute_start(&self, launch_plan: &KrunLaunchPlan) -> Result<SandboxHandle> {
        ensure_linux_host()?;
        spawn_background(&launch_plan.manifest.conmon_launch.create_command)?;
        let runtime_state = wait_for_runtime_state(
            &launch_plan.manifest.conmon_launch.state_command,
            self.config.start_timeout,
        )?;
        if runtime_state != "running" {
            run_status_checked(&launch_plan.manifest.conmon_launch.start_command)?;
        }

        let mut manifest = launch_plan.manifest.clone();
        manifest.shutdown_requested = false;
        manifest.last_exit_code = None;
        manifest.status = SandboxStatus::Starting;
        manifest.handle.status = SandboxStatus::Starting;
        self.write_manifest(&manifest)?;
        Ok(manifest.handle)
    }

    fn execute_stop(&self, manifest: &mut KrunSandboxManifest) -> Result<()> {
        if manifest.conmon_layout.exit_status_file.exists() {
            manifest.shutdown_requested = true;
            manifest.last_exit_code =
                Some(read_exit_code(&manifest.conmon_layout.exit_status_file)?);
            manifest.status = SandboxStatus::Stopped;
            manifest.handle.status = manifest.status;
            return self.write_manifest(manifest);
        }

        manifest.shutdown_requested = true;
        let pid = read_pid(&manifest.conmon_layout.pidfile)?;
        let stop_signal = configured_stop_signal(&manifest.image_metadata);
        signal_process(&stop_signal, pid)?;
        if !wait_for_path(
            &manifest.conmon_layout.exit_status_file,
            self.config.stop_timeout,
        ) {
            signal_process("KILL", pid)?;
            if !wait_for_path(
                &manifest.conmon_layout.exit_status_file,
                self.config.stop_timeout,
            ) {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "sandbox {} did not write an exit file after TERM/KILL",
                        manifest.handle.id
                    ),
                });
            }
        }

        manifest.last_exit_code = Some(read_exit_code(&manifest.conmon_layout.exit_status_file)?);
        manifest.status = SandboxStatus::Stopped;
        manifest.handle.status = manifest.status;
        self.cleanup_manifest_buildah_artifacts(manifest)?;
        manifest.buildah_container = None;
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
            Some("running") => Ok(SandboxStatus::Ready),
            Some("created") | Some("creating") => Ok(SandboxStatus::Starting),
            Some("stopped") => Ok(SandboxStatus::Stopped),
            Some("paused") => Ok(SandboxStatus::Stopping),
            Some(_) => Ok(SandboxStatus::Failed),
            None if manifest.conmon_layout.pidfile.exists() => Ok(SandboxStatus::Starting),
            None => Ok(manifest.status),
        }
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
    buildah_container: Option<BuildahContainer>,
    bundle_layout: KrunBundleLayout,
    conmon_layout: KrunConmonLayout,
    conmon_launch: KrunConmonLaunchPlan,
    last_exit_code: Option<i32>,
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
struct KrunImageMetadata {
    user: Option<String>,
    stop_signal: Option<String>,
    healthcheck: Option<ImageHealthcheck>,
    labels: BTreeMap<String, String>,
    exposed_ports: Vec<OciExposedPort>,
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
        } else if (character == '-' || character == '_') && !slug.ends_with('-') {
            slug.push('-');
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_owned()
}

fn buildah_container_name(sandbox_id: &SandboxId) -> String {
    format!("{}-image", sandbox_id.as_str())
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
        .to_command()
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
        .to_command()
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
        .to_command()
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
        if let Some(status) = runtime_state(command)? {
            if status == "created" || status == "running" {
                return Ok(status);
            }
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
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use futures::executor::block_on;
    use serde_json::json;
    use tempfile::TempDir;

    use neovex_core::TenantId;

    use super::{
        KrunImageMetadata, KrunSandboxBackend, KrunSandboxBackendConfig, configured_stop_signal,
        slugify,
    };
    use crate::backend::{SandboxBackend, SandboxBackendKind};
    use crate::backends::krun::buildah::{
        ImageHealthcheck, OciExposedPort, OciExposedPortProtocol, OciImageLaunchDefaults,
        OciProcessOverrides,
    };
    use crate::spec::{SandboxFilesystemSpec, SandboxPortBinding, SandboxProcessSpec, SandboxSpec};

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
            vec!["/usr/local/bin/service".to_owned(), "serve".to_owned()]
        );
        assert_eq!(
            launch_plan.manifest.spec.process.env,
            vec![
                "PATH=/usr/local/bin:/usr/bin".to_owned(),
                "SERVICE_MODE=prod".to_owned(),
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
            rendered_bundle.contains("\"/usr/local/bin/service\""),
            "bundle config should use the image-default command when the generic spec is sparse"
        );
        assert!(
            rendered_bundle.contains("\"uid\": 1000"),
            "bundle config should lower the image user uid into process.user"
        );
        assert!(
            rendered_bundle.contains("\"gid\": 1000"),
            "bundle config should lower the image user gid into process.user"
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
    fn start_from_image_plan_only_persists_and_then_cleans_up_buildah_artifacts() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let (buildah_path, log_path) = write_fake_buildah_script(&temp_dir);

        let mut config = KrunSandboxBackendConfig::plan_only(
            temp_dir.path().join("bundles"),
            temp_dir.path().join("state"),
        );
        config.buildah_path = buildah_path;
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

        let handle = block_on(backend.start_from_image(
            spec,
            "postgres:16".to_owned(),
            OciProcessOverrides::default(),
        ))
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
            manifest_before_stop.contains("\"buildah_container\""),
            "manifest should retain buildah container metadata while running"
        );

        block_on(backend.stop(&handle.id)).expect("plan-only stop should succeed");

        let manifest_after_stop =
            fs::read_to_string(&manifest_path).expect("manifest should be readable after stop");
        assert!(
            manifest_after_stop.contains("\"buildah_container\": null"),
            "stop should clear buildah container metadata after cleanup"
        );

        let log = fs::read_to_string(log_path).expect("fake buildah log should be readable");
        let lines: Vec<_> = log.lines().collect();
        assert_eq!(
            lines,
            vec![
                format!("from --name {}-image postgres:16", handle.id.as_str()),
                format!("mount {}-image", handle.id.as_str()),
                format!("inspect --type container {}-image", handle.id.as_str()),
                format!("umount {}-image", handle.id.as_str()),
                format!("rm {}-image", handle.id.as_str()),
            ]
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

    fn sample_spec() -> SandboxSpec {
        SandboxSpec::new(
            TenantId::new("tenant").expect("tenant id should be valid"),
            "postgres-primary",
            SandboxBackendKind::Krun,
            SandboxFilesystemSpec::new(Path::new("/srv/rootfs")),
            SandboxProcessSpec::new(["/usr/bin/postgres", "-D", "/var/lib/postgresql/data"])
                .with_env(["PATH=/usr/bin", "PGDATA=/var/lib/postgresql/data"]),
        )
        .with_port_bindings([
            SandboxPortBinding::tcp("postgres", 15432, 5432),
            SandboxPortBinding::tcp("health", 18080, 8080),
        ])
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

    fn write_fake_buildah_script(temp_dir: &TempDir) -> (PathBuf, PathBuf) {
        let script_path = temp_dir.path().join("fake-buildah");
        let log_path = temp_dir.path().join("buildah.log");
        let script = format!(
            r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "{log_path}"
cmd="${{1:-}}"
if [ -z "$cmd" ]; then
  echo "missing buildah subcommand" >&2
  exit 1
fi
shift

if [ "$cmd" = "unshare" ]; then
  if [ "${{1:-}}" != "--" ]; then
    echo "expected -- after buildah unshare" >&2
    exit 1
  fi
  shift
  wrapped_program="${{1:-}}"
  if [ -z "$wrapped_program" ]; then
    echo "missing wrapped program for buildah unshare" >&2
    exit 1
  fi
  shift
  cmd="${{1:-}}"
  if [ -z "$cmd" ]; then
    printf 'missing subcommand for wrapped program %s\n' "$wrapped_program" >&2
    exit 1
  fi
  shift
fi

case "$cmd" in
  from|bud|umount|rm)
    exit 0
    ;;
  mount)
    printf '%s\n' "/tmp/fake-rootfs"
    exit 0
    ;;
  inspect)
    cat <<'JSON'
{inspect_json}
JSON
    exit 0
    ;;
  *)
    printf 'unexpected command: %s\n' "$cmd" >&2
    exit 1
    ;;
esac
"#,
            log_path = log_path.display(),
            inspect_json = sample_inspect_json()
        );

        fs::write(&script_path, script).expect("fake buildah script should be written");
        let mut permissions = fs::metadata(&script_path)
            .expect("fake buildah script metadata should exist")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions)
            .expect("fake buildah script should be executable");

        (script_path, log_path)
    }

    fn sample_inspect_json() -> String {
        json!([
            {
                "OCIv1": {
                    "Config": {
                        "Entrypoint": ["/usr/local/bin/docker-entrypoint.sh"],
                        "Cmd": ["postgres"],
                        "Env": [
                            "PATH=/usr/local/bin:/usr/bin",
                            "POSTGRES_DB=postgres"
                        ],
                        "WorkingDir": "/var/lib/postgresql",
                        "User": "999:999",
                        "ExposedPorts": {
                            "5432/tcp": {}
                        },
                        "Volumes": {
                            "/var/lib/postgresql/data": {}
                        },
                        "StopSignal": "SIGINT",
                        "Labels": {
                            "com.example.role": "primary"
                        }
                    }
                },
                "Docker": {
                    "Config": {
                        "Healthcheck": {
                            "Test": ["CMD-SHELL", "pg_isready -U postgres"],
                            "Interval": 10000000000_u64,
                            "Timeout": 5000000000_u64,
                            "StartPeriod": 30000000000_u64,
                            "Retries": 3
                        }
                    }
                }
            }
        ])
        .to_string()
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
