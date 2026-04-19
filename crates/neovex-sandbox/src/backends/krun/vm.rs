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

mod launch;
mod lifecycle;
mod readiness;

#[cfg(test)]
use self::launch::{desired_krun_vm_config, krun_vm_config_path, parse_guest_user, slugify};
#[cfg(test)]
use self::lifecycle::{
    configured_stop_signal, configured_stop_timeout, restart_backoff_delay,
    restart_policy_allows_restart,
};
#[cfg(test)]
use self::readiness::{
    probe_target_ready, readiness_probe_target, running_status, visible_published_endpoints,
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

    pub fn start_from_image(&self, launch: SandboxImageLaunchSpec) -> SandboxFuture<SandboxHandle> {
        let backend = self.clone();
        Box::pin(async move { backend.start_from_image_sync(launch) })
    }

    pub fn start_from_build(&self, launch: SandboxBuildLaunchSpec) -> SandboxFuture<SandboxHandle> {
        let backend = self.clone();
        Box::pin(async move { backend.start_from_build_sync(launch) })
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

#[cfg(test)]
mod tests;
