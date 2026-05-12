use std::env;
use std::fs;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

use nimbus::Error;
use serde::{Deserialize, Serialize};

use super::manager::MachineRuntimeState;
use super::{
    CURRENT_MACHINE_STATE_VERSION, DEFAULT_MACHINE_RUNTIME_ROOT, MACHINE_RUNTIME_ROOT_ENV,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MachineRootLayout {
    pub(super) config_root: PathBuf,
    pub(super) state_root: PathBuf,
    pub(super) data_root: PathBuf,
    pub(super) cache_root: PathBuf,
    pub(super) runtime_root: PathBuf,
}

impl MachineRootLayout {
    pub(super) fn resolve() -> Result<Self, Error> {
        Ok(Self {
            config_root: resolve_config_root()?,
            state_root: resolve_state_root()?,
            data_root: resolve_data_root()?,
            cache_root: resolve_cache_root()?,
            runtime_root: resolve_runtime_root(),
        })
    }

    pub(super) fn guest_api_default(runtime_root: PathBuf) -> Self {
        Self {
            config_root: PathBuf::from("/var/lib/nimbus/machine/config"),
            state_root: PathBuf::from("/var/lib/nimbus/machine/state"),
            data_root: PathBuf::from("/var/lib/nimbus/machine/data"),
            cache_root: PathBuf::from("/var/lib/nimbus/machine/cache"),
            runtime_root,
        }
    }

    #[cfg(test)]
    pub(super) fn new(config_root: PathBuf, state_root: PathBuf, runtime_root: PathBuf) -> Self {
        let shared_parent = config_root
            .parent()
            .map(Path::to_path_buf)
            .and_then(|config_parent| {
                (state_root.parent() == Some(config_parent.as_path())
                    && runtime_root.parent() == Some(config_parent.as_path()))
                .then_some(config_parent)
            });
        Self {
            config_root,
            state_root,
            data_root: shared_parent
                .as_ref()
                .map(|parent| parent.join("data"))
                .unwrap_or_else(|| PathBuf::from("/tmp/nimbus-test-data")),
            cache_root: shared_parent
                .as_ref()
                .map(|parent| parent.join("cache"))
                .unwrap_or_else(|| PathBuf::from("/tmp/nimbus-test-cache")),
            runtime_root,
        }
    }

    pub(super) fn lock_path(&self, name: &str) -> PathBuf {
        self.state_root.join(format!("{name}.lock"))
    }

    #[cfg(any(unix, test))]
    pub(super) fn port_allocation_state_path(&self) -> PathBuf {
        self.state_root.join("port-alloc.dat")
    }

    #[cfg(any(unix, test))]
    pub(super) fn port_allocation_lock_path(&self) -> PathBuf {
        self.state_root.join("port-alloc.lck")
    }

    pub(super) fn paths(&self, name: &str) -> MachinePaths {
        let config_dir = self.config_root.join(name);
        let state_dir = self.state_root.join(name);
        let data_dir = self.data_root.join(name);
        let runtime_dir = self.runtime_root.clone();
        MachinePaths {
            name: name.to_owned(),
            config_dir: config_dir.clone(),
            state_dir: state_dir.clone(),
            data_dir: data_dir.clone(),
            runtime_dir: runtime_dir.clone(),
            config_path: config_dir.join("config.json"),
            generated_ignition_path: config_dir.join("generated.ign"),
            state_path: state_dir.join("status.json"),
            image_cache_dir: self.cache_root.join("images"),
            guest_binary_cache_dir: self.cache_root.join("guest-nimbus"),
            materialized_image_path: data_dir.join("images").join(format!("{name}.raw")),
            api_socket_path: runtime_dir.join(format!("{name}-api.sock")),
            ready_socket_path: runtime_dir.join(format!("{name}.sock")),
            ignition_socket_path: runtime_dir.join(format!("{name}-ignition.sock")),
            gvproxy_socket_path: runtime_dir.join(format!("{name}-gvproxy.sock")),
            krunkit_endpoint_path: runtime_dir.join(format!("{name}-krunkit.sock")),
            efi_variable_store_path: data_dir.join("efi-variable-store"),
            gvproxy_pid_path: runtime_dir.join(format!("{name}-gvproxy.pid")),
            krunkit_pid_path: runtime_dir.join(format!("{name}-krunkit.pid")),
            machine_log_path: runtime_dir.join(format!("{name}.log")),
            gvproxy_log_path: runtime_dir.join(format!("{name}-gvproxy.log")),
            krunkit_log_path: runtime_dir.join(format!("{name}-krunkit.log")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MachineConfigRecord {
    pub(super) version: u32,
    pub(super) name: String,
    pub(super) provider: MachineProvider,
    pub(super) guest: MachineGuestConfig,
    pub(super) resources: MachineResources,
    pub(super) volumes: Vec<MachineVolume>,
    pub(super) roots: MachineRootLayout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MachineGuestConfig {
    pub(super) image_source: MachineImageSource,
    pub(super) ssh_user: String,
    pub(super) ssh_identity_path: Option<PathBuf>,
    pub(super) ignition_file_path: Option<PathBuf>,
    pub(super) efi_variable_store_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(super) enum MachineImageSource {
    OciReference { reference: String },
    HttpUrl { url: String },
    LocalDisk { path: PathBuf },
}

impl MachineImageSource {
    pub(super) fn parse(value: &str) -> Result<Self, Error> {
        let value = value.trim();
        if value.is_empty() {
            return Err(Error::InvalidInput(
                "machine image source cannot be empty".to_owned(),
            ));
        }

        if value.starts_with("http://") || value.starts_with("https://") {
            return Ok(Self::HttpUrl {
                url: value.to_owned(),
            });
        }

        if value.starts_with("docker://") {
            return Ok(Self::OciReference {
                reference: value.to_owned(),
            });
        }

        let path = PathBuf::from(value);
        if path.is_absolute() {
            return Ok(Self::LocalDisk { path });
        }

        Ok(Self::OciReference {
            reference: format!("docker://{value}"),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MachineResources {
    pub(super) cpus: u8,
    pub(super) memory_mib: u32,
    pub(super) disk_gib: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MachineVolume {
    pub(super) source: PathBuf,
    pub(super) target: PathBuf,
}

impl MachineVolume {
    pub(super) fn parse(value: &str) -> Result<Self, String> {
        let (source, target) = value.split_once(':').ok_or_else(|| {
            format!("invalid machine volume '{value}'; expected <source>:<target>")
        })?;
        if source.is_empty() || target.is_empty() {
            return Err(format!(
                "invalid machine volume '{value}'; expected non-empty <source>:<target>"
            ));
        }
        let source = PathBuf::from(source);
        let target = PathBuf::from(target);
        if !source.is_absolute() {
            return Err(format!(
                "invalid machine volume '{value}'; source path must be absolute"
            ));
        }
        if !target.is_absolute() {
            return Err(format!(
                "invalid machine volume '{value}'; target path must be absolute"
            ));
        }
        Ok(Self { source, target })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MachineStateRecord {
    pub(super) version: u32,
    pub(super) lifecycle: MachineLifecycle,
    pub(super) manager: MachineManagerState,
    pub(super) runtime: Option<MachineRuntimeState>,
    pub(super) last_error: Option<String>,
}

impl MachineStateRecord {
    pub(super) fn initialized() -> Self {
        Self {
            version: CURRENT_MACHINE_STATE_VERSION,
            lifecycle: MachineLifecycle::Stopped,
            manager: MachineManagerState::Unconfigured,
            runtime: None,
            last_error: None,
        }
    }

    pub(super) fn rebuilt(reason: impl Into<String>) -> Self {
        Self {
            version: CURRENT_MACHINE_STATE_VERSION,
            lifecycle: MachineLifecycle::Stopped,
            manager: MachineManagerState::Stale,
            runtime: None,
            last_error: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum MachineProvider {
    Krunkit,
    Wsl2,
}

#[cfg(any(unix, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MachineImageFormat {
    Raw,
    Tar,
}

#[cfg(any(unix, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MachineBootstrapMode {
    Ignition,
    ShellScript,
}

#[cfg(any(unix, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MachineProviderCapabilities {
    pub(super) uses_provider_networking: bool,
    pub(super) requires_exclusive_active: bool,
    pub(super) image_format: MachineImageFormat,
    pub(super) bootstrap_mode: MachineBootstrapMode,
    pub(super) oci_artifact_disk_type: &'static str,
}

#[cfg(any(unix, test))]
const KRUNKIT_PROVIDER_CAPABILITIES: MachineProviderCapabilities = MachineProviderCapabilities {
    uses_provider_networking: false,
    requires_exclusive_active: true,
    image_format: MachineImageFormat::Raw,
    bootstrap_mode: MachineBootstrapMode::Ignition,
    oci_artifact_disk_type: "applehv",
};

#[cfg(any(unix, test))]
const WSL2_PROVIDER_CAPABILITIES: MachineProviderCapabilities = MachineProviderCapabilities {
    uses_provider_networking: true,
    requires_exclusive_active: false,
    image_format: MachineImageFormat::Tar,
    bootstrap_mode: MachineBootstrapMode::ShellScript,
    oci_artifact_disk_type: "wsl",
};

impl MachineProvider {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Krunkit => "krunkit",
            Self::Wsl2 => "wsl2",
        }
    }

    #[cfg(any(unix, test))]
    pub(super) fn capabilities(self) -> MachineProviderCapabilities {
        match self {
            Self::Krunkit => KRUNKIT_PROVIDER_CAPABILITIES,
            Self::Wsl2 => WSL2_PROVIDER_CAPABILITIES,
        }
    }

    #[cfg(any(unix, test))]
    pub(super) fn uses_provider_networking(self) -> bool {
        self.capabilities().uses_provider_networking
    }

    #[cfg(any(unix, test))]
    pub(super) fn requires_exclusive_active(self) -> bool {
        self.capabilities().requires_exclusive_active
    }

    #[cfg(any(unix, test))]
    pub(super) fn image_format(self) -> MachineImageFormat {
        self.capabilities().image_format
    }

    #[cfg(any(unix, test))]
    pub(super) fn bootstrap_mode(self) -> MachineBootstrapMode {
        self.capabilities().bootstrap_mode
    }

    #[cfg(any(unix, test))]
    pub(super) fn oci_artifact_disk_type(self) -> &'static str {
        self.capabilities().oci_artifact_disk_type
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum MachineLifecycle {
    Uninitialized,
    Stopped,
    Starting,
    Running,
    Failed,
}

impl MachineLifecycle {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Uninitialized => "uninitialized",
            Self::Stopped => "stopped",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum MachineManagerState {
    Unconfigured,
    HelpersResolved,
    Launching,
    Ready,
    Failed,
    Stale,
}

impl MachineManagerState {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Unconfigured => "unconfigured",
            Self::HelpersResolved => "helpers-resolved",
            Self::Launching => "launching",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Stale => "stale",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct MachinePaths {
    pub(super) name: String,
    pub(super) config_dir: PathBuf,
    pub(super) state_dir: PathBuf,
    pub(super) data_dir: PathBuf,
    pub(super) runtime_dir: PathBuf,
    pub(super) config_path: PathBuf,
    pub(super) generated_ignition_path: PathBuf,
    pub(super) state_path: PathBuf,
    pub(super) image_cache_dir: PathBuf,
    pub(super) guest_binary_cache_dir: PathBuf,
    pub(super) materialized_image_path: PathBuf,
    pub(super) api_socket_path: PathBuf,
    pub(super) ready_socket_path: PathBuf,
    pub(super) ignition_socket_path: PathBuf,
    pub(super) gvproxy_socket_path: PathBuf,
    pub(super) krunkit_endpoint_path: PathBuf,
    pub(super) efi_variable_store_path: PathBuf,
    pub(super) gvproxy_pid_path: PathBuf,
    pub(super) krunkit_pid_path: PathBuf,
    pub(super) machine_log_path: PathBuf,
    pub(super) gvproxy_log_path: PathBuf,
    pub(super) krunkit_log_path: PathBuf,
}

impl MachinePaths {
    pub(super) fn ensure_directories(&self) -> Result<(), Error> {
        fs::create_dir_all(&self.config_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine config directory {}: {error}",
                self.config_dir.display()
            ))
        })?;
        fs::create_dir_all(&self.state_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine state directory {}: {error}",
                self.state_dir.display()
            ))
        })?;
        fs::create_dir_all(&self.data_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine data directory {}: {error}",
                self.data_dir.display()
            ))
        })?;
        fs::create_dir_all(&self.image_cache_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine image cache directory {}: {error}",
                self.image_cache_dir.display()
            ))
        })?;
        fs::create_dir_all(&self.guest_binary_cache_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create guest binary cache directory {}: {error}",
                self.guest_binary_cache_dir.display()
            ))
        })?;
        let materialized_parent = self.materialized_image_path.parent().ok_or_else(|| {
            Error::Internal(format!(
                "failed to resolve parent directory for machine image {}",
                self.materialized_image_path.display()
            ))
        })?;
        fs::create_dir_all(materialized_parent).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine image data directory {}: {error}",
                materialized_parent.display()
            ))
        })?;
        self.ensure_runtime_directories()
    }

    pub(super) fn ensure_runtime_directories(&self) -> Result<(), Error> {
        fs::create_dir_all(&self.runtime_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine runtime directory {}: {error}",
                self.runtime_dir.display()
            ))
        })
    }

    pub(super) fn krunkit_gvproxy_socket_path(&self) -> PathBuf {
        PathBuf::from(format!("{}-krun.sock", self.gvproxy_socket_path.display()))
    }
}

fn resolve_config_root() -> Result<PathBuf, Error> {
    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(path).join("nimbus").join("machine"));
    }
    Ok(resolve_home_dir()?
        .join(".config")
        .join("nimbus")
        .join("machine"))
}

fn resolve_state_root() -> Result<PathBuf, Error> {
    if let Some(path) = env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(path).join("nimbus").join("machine"));
    }
    Ok(resolve_home_dir()?
        .join(".local")
        .join("state")
        .join("nimbus")
        .join("machine"))
}

fn resolve_data_root() -> Result<PathBuf, Error> {
    if let Some(path) = env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(path).join("nimbus").join("machine"));
    }
    Ok(resolve_home_dir()?
        .join(".local")
        .join("share")
        .join("nimbus")
        .join("machine"))
}

fn resolve_cache_root() -> Result<PathBuf, Error> {
    if let Some(path) = env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(path).join("nimbus").join("machine"));
    }
    Ok(resolve_home_dir()?
        .join(".cache")
        .join("nimbus")
        .join("machine"))
}

fn resolve_home_dir() -> Result<PathBuf, Error> {
    env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
        Error::InvalidInput("HOME is not set; cannot resolve machine directories".to_owned())
    })
}

pub(super) fn resolve_runtime_root() -> PathBuf {
    env::var_os(MACHINE_RUNTIME_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_MACHINE_RUNTIME_ROOT))
}
