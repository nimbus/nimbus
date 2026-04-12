use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use super::bundle::{KrunBundleLayout, write_bundle_config};
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
        match self.config.launch_mode {
            KrunLaunchMode::PlanOnly => {
                let mut manifest = launch_plan.manifest.clone();
                manifest.last_exit_code = None;
                manifest.shutdown_requested = false;
                self.write_manifest(&manifest)?;
                Ok(manifest.handle)
            }
            KrunLaunchMode::Execute => self.execute_start(&launch_plan),
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
                self.write_manifest(&manifest)
            }
            KrunLaunchMode::Execute => self.execute_stop(&mut manifest),
        }
    }

    pub(crate) fn plan_start(&self, spec: &SandboxSpec) -> Result<KrunLaunchPlan> {
        if spec.backend != SandboxBackendKind::Krun {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "krun backend cannot lower sandbox spec for backend {:?}",
                    spec.backend
                ),
            });
        }

        let sandbox_id = SandboxId::new(format!(
            "{}-{}",
            slugify(&spec.name),
            Ulid::new().to_string().to_ascii_lowercase()
        ));
        let bundle_layout =
            KrunBundleLayout::new(self.config.bundle_root.join(sandbox_id.as_str()));
        write_bundle_config(&bundle_layout, &hostname_for(spec), spec)?;

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
        );

        let handle = SandboxHandle::new(
            sandbox_id.clone(),
            spec.name.clone(),
            SandboxBackendKind::Krun,
            SandboxStatus::Starting,
            published_endpoints(spec),
        );
        let manifest = KrunSandboxManifest {
            handle,
            spec: spec.clone(),
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
        signal_process("TERM", pid)?;
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
    use std::fs;
    use std::path::Path;

    use futures::executor::block_on;
    use tempfile::TempDir;

    use neovex_core::TenantId;

    use super::{KrunSandboxBackend, KrunSandboxBackendConfig, slugify};
    use crate::backend::{SandboxBackend, SandboxBackendKind};
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
}
