//! Shared machine record and provider contracts.
//!
//! This crate owns the render-independent machine model used by the CLI today
//! and by the server control plane as machine lifecycle endpoints move out of
//! `nimbus-bin`.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use nimbus_core::Error;
use serde::{Deserialize, Serialize};

pub const DEFAULT_MACHINE_RUNTIME_ROOT: &str = "/tmp/nimbus";
pub const MACHINE_RUNTIME_ROOT_ENV: &str = "NIMBUS_MACHINE_RUNTIME_ROOT";
// The machine config schema (`config.json`) is at its first version. Like the
// state schema it starts at 1 pre-launch -- there is no shipped older version to
// account for, so the dev-era 1 -> 2 -> 3 history collapses to a single
// canonical v1. Unlike the state schema, the config loader is *strict*: a
// version mismatch is a hard error, never a silent rebuild. config.json is the
// operator's declared configuration (provider, resources, image source,
// volumes) -- durable user data -- so rebuilding it from defaults would
// silently invent intent that exists nowhere else. To keep that strictness
// non-destructive, the loader first copies the rejected file aside to a
// `config.json.v{N}.bak` sibling and then directs the operator to recreate the
// machine, so their declared settings survive for reference instead of being
// destroyed by the recovery. That asymmetry with `CURRENT_MACHINE_STATE_VERSION`
// (rebuildable runtime data) is deliberate; do not make config self-heal. The
// first post-launch schema change bumps to 2.
pub const CURRENT_MACHINE_CONFIG_VERSION: u32 = 1;
// The machine state schema (`status.json`) is at its first version. The
// `krunkit` -> `vmm` runtime-helper rename -- one provider-neutral VMM slot per
// machine, with the matching `*-krunkit.{pid,sock}` -> `*-vmm.*` runtime-file
// scheme -- was made directly as a pre-launch breaking change, not a migration.
// A `status.json` written before the rename simply lacks the now-required
// `runtime.helper_binaries.vmm` field; the loader rebuilds that unparseable
// record into a clean Stopped/Stale state (see
// `files::load_machine_state_if_exists`) rather than stranding the machine, so
// the rename needs no version bump. State is rebuildable runtime data, not
// durable user data. The version gate (probe + newer/older rebuild arms) stays
// so the first post-launch schema change can bump to 2 and route pre-existing
// files through the rebuild arm with an explicit "schema evolved" reason;
// pre-launch there is no shipped older version to account for, so the schema
// starts at 1.
pub const CURRENT_MACHINE_STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineRootLayout {
    pub config_root: PathBuf,
    pub state_root: PathBuf,
    pub data_root: PathBuf,
    pub cache_root: PathBuf,
    pub runtime_root: PathBuf,
}

impl MachineRootLayout {
    pub fn resolve() -> Result<Self, Error> {
        Self::resolve_with_env(|name| env::var_os(name))
    }

    fn resolve_with_env(mut lookup: impl FnMut(&str) -> Option<OsString>) -> Result<Self, Error> {
        Ok(Self {
            config_root: resolve_config_root_with_env(&mut lookup)?,
            state_root: resolve_state_root_with_env(&mut lookup)?,
            data_root: resolve_data_root_with_env(&mut lookup)?,
            cache_root: resolve_cache_root_with_env(&mut lookup)?,
            runtime_root: resolve_runtime_root_with_env(&mut lookup),
        })
    }

    pub fn guest_api_default(runtime_root: PathBuf) -> Self {
        Self {
            config_root: PathBuf::from("/var/lib/nimbus/machine/config"),
            state_root: PathBuf::from("/var/lib/nimbus/machine/state"),
            data_root: PathBuf::from("/var/lib/nimbus/machine/data"),
            cache_root: PathBuf::from("/var/lib/nimbus/machine/cache"),
            runtime_root,
        }
    }

    pub fn new(
        config_root: PathBuf,
        state_root: PathBuf,
        data_root: PathBuf,
        cache_root: PathBuf,
        runtime_root: PathBuf,
    ) -> Self {
        Self {
            config_root,
            state_root,
            data_root,
            cache_root,
            runtime_root,
        }
    }

    pub fn from_sibling_roots(
        config_root: PathBuf,
        state_root: PathBuf,
        runtime_root: PathBuf,
    ) -> Result<Self, Error> {
        let shared_parent = config_root
            .parent()
            .map(Path::to_path_buf)
            .and_then(|config_parent| {
                (state_root.parent() == Some(config_parent.as_path())
                    && runtime_root.parent() == Some(config_parent.as_path()))
                .then_some(config_parent)
            })
            .ok_or_else(|| {
                Error::InvalidInput(
                    "machine config, state, and runtime roots must share a parent when deriving data/cache roots"
                        .to_owned(),
                )
            })?;
        Ok(Self::new(
            config_root,
            state_root,
            shared_parent.join("data"),
            shared_parent.join("cache"),
            runtime_root,
        ))
    }

    #[doc(hidden)]
    pub fn test_sibling_roots(
        config_root: PathBuf,
        state_root: PathBuf,
        runtime_root: PathBuf,
    ) -> Self {
        Self::from_sibling_roots(config_root, state_root, runtime_root)
            .expect("machine test roots must share a parent")
    }

    pub fn lock_path(&self, name: &str) -> PathBuf {
        self.state_root.join(format!("{name}.lock"))
    }

    pub fn port_allocation_state_path(&self) -> PathBuf {
        self.state_root.join("port-alloc.dat")
    }

    pub fn port_allocation_lock_path(&self) -> PathBuf {
        self.state_root.join("port-alloc.lck")
    }

    pub fn paths(&self, name: &str) -> MachinePaths {
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
            guest_config_bundle_dir: state_dir.join("machine-config"),
            image_cache_dir: self.cache_root.join("images"),
            guest_binary_cache_dir: self.cache_root.join("guest-nimbus"),
            materialized_image_path: data_dir.join("images").join(format!("{name}.raw")),
            api_socket_path: runtime_dir.join(format!("{name}-api.sock")),
            ready_socket_path: runtime_dir.join(format!("{name}.sock")),
            ignition_socket_path: runtime_dir.join(format!("{name}-ignition.sock")),
            gvproxy_socket_path: runtime_dir.join(format!("{name}-gvproxy.sock")),
            vmm_endpoint_path: runtime_dir.join(format!("{name}-vmm.sock")),
            efi_variable_store_path: data_dir.join("efi-variable-store"),
            api_forward_pid_path: runtime_dir.join(format!("{name}-api-forward.pid")),
            gvproxy_pid_path: runtime_dir.join(format!("{name}-gvproxy.pid")),
            vmm_pid_path: runtime_dir.join(format!("{name}-vmm.pid")),
            api_forward_log_path: runtime_dir.join(format!("{name}-api-forward.log")),
            machine_log_path: runtime_dir.join(format!("{name}.log")),
            gvproxy_log_path: runtime_dir.join(format!("{name}-gvproxy.log")),
            vmm_log_path: runtime_dir.join(format!("{name}-vmm.log")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineConfigRecord {
    pub version: u32,
    pub name: String,
    pub provider: MachineProvider,
    pub guest: MachineGuestConfig,
    pub resources: MachineResources,
    pub volumes: Vec<MachineVolume>,
    pub roots: MachineRootLayout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineGuestConfig {
    pub image_source: MachineImageSource,
    pub provisioning: MachineGuestProvisioning,
    pub ssh_user: String,
    pub ssh_identity_path: Option<PathBuf>,
    pub ignition_file_path: Option<PathBuf>,
    pub efi_variable_store_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MachineGuestProvisioning {
    Ignition,
    BootcMachineConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum MachineImageSource {
    OciReference { reference: String },
    HttpUrl { url: String, sha256: String },
    LocalDisk { path: PathBuf },
}

impl MachineImageSource {
    pub fn parse(value: &str) -> Result<Self, Error> {
        let value = value.trim();
        if value.is_empty() {
            return Err(Error::InvalidInput(
                "machine image source cannot be empty".to_owned(),
            ));
        }

        if value.starts_with("http://") || value.starts_with("https://") {
            return parse_http_image_source(value);
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

fn parse_http_image_source(value: &str) -> Result<MachineImageSource, Error> {
    let (url, fragment) = value.rsplit_once('#').ok_or_else(|| {
        Error::InvalidInput(
            "HTTP machine image sources must include an integrity suffix: #sha256=<64 hex>"
                .to_owned(),
        )
    })?;
    if url.is_empty() {
        return Err(Error::InvalidInput(
            "HTTP machine image source URL cannot be empty".to_owned(),
        ));
    }
    let digest = fragment.strip_prefix("sha256=").ok_or_else(|| {
        Error::InvalidInput(
            "HTTP machine image source integrity suffix must be #sha256=<64 hex>".to_owned(),
        )
    })?;
    let sha256 = normalize_sha256_hex(digest)?;
    Ok(MachineImageSource::HttpUrl {
        url: url.to_owned(),
        sha256,
    })
}

fn normalize_sha256_hex(value: &str) -> Result<String, Error> {
    if value.len() != 64 || !value.as_bytes().iter().all(u8::is_ascii_hexdigit) {
        return Err(Error::InvalidInput(format!(
            "HTTP machine image sha256 must be exactly 64 hex characters, got {value:?}"
        )));
    }
    Ok(value.to_ascii_lowercase())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineResources {
    pub cpus: u8,
    pub memory_mib: u32,
    pub disk_gib: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineVolume {
    pub source: PathBuf,
    pub target: PathBuf,
}

impl MachineVolume {
    pub fn parse(value: &str) -> Result<Self, Error> {
        let (source, target) = value.split_once(':').ok_or_else(|| {
            Error::InvalidInput(format!(
                "invalid machine volume '{value}'; expected <source>:<target>"
            ))
        })?;
        if source.is_empty() || target.is_empty() {
            return Err(Error::InvalidInput(format!(
                "invalid machine volume '{value}'; expected non-empty <source>:<target>"
            )));
        }
        let source = PathBuf::from(source);
        let target = PathBuf::from(target);
        if !source.is_absolute() {
            return Err(Error::InvalidInput(format!(
                "invalid machine volume '{value}'; source path must be absolute"
            )));
        }
        if !target.is_absolute() {
            return Err(Error::InvalidInput(format!(
                "invalid machine volume '{value}'; target path must be absolute"
            )));
        }
        Ok(Self { source, target })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineStateRecord {
    pub version: u32,
    pub lifecycle: MachineLifecycle,
    pub manager: MachineManagerState,
    pub runtime: Option<MachineRuntimeState>,
    pub last_error: Option<String>,
}

impl MachineStateRecord {
    pub fn initialized() -> Self {
        Self {
            version: CURRENT_MACHINE_STATE_VERSION,
            lifecycle: MachineLifecycle::Stopped,
            manager: MachineManagerState::Unconfigured,
            runtime: None,
            last_error: None,
        }
    }

    pub fn rebuilt(reason: impl Into<String>) -> Self {
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
pub enum MachineProvider {
    Krunkit,
    Vfkit,
    Wsl2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineImageFormat {
    Raw,
    Tar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineBootstrapMode {
    Ignition,
    BootcMachineConfig,
    ShellScript,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineProviderCapabilities {
    pub uses_provider_networking: bool,
    pub requires_exclusive_active: bool,
    pub image_format: MachineImageFormat,
    pub bootstrap_mode: MachineBootstrapMode,
    pub oci_artifact_disk_type: &'static str,
}

const KRUNKIT_PROVIDER_CAPABILITIES: MachineProviderCapabilities = MachineProviderCapabilities {
    uses_provider_networking: false,
    requires_exclusive_active: true,
    image_format: MachineImageFormat::Raw,
    bootstrap_mode: MachineBootstrapMode::Ignition,
    oci_artifact_disk_type: "applehv",
};

// vfkit is the second macOS VMM. Like krunkit it boots the Nimbus-managed
// `applehv` disk over EFI, bootstraps via an Ignition vsock, and relies on an
// external gvproxy userspace network stack — so its capabilities mirror
// krunkit's. The two differ only in the VMM binary and the on-VMM net-device
// syntax, both of which are owned by the per-provider `MachineVmmBackend`.
const VFKIT_PROVIDER_CAPABILITIES: MachineProviderCapabilities = MachineProviderCapabilities {
    uses_provider_networking: false,
    requires_exclusive_active: true,
    image_format: MachineImageFormat::Raw,
    bootstrap_mode: MachineBootstrapMode::Ignition,
    oci_artifact_disk_type: "applehv",
};

const WSL2_PROVIDER_CAPABILITIES: MachineProviderCapabilities = MachineProviderCapabilities {
    uses_provider_networking: true,
    requires_exclusive_active: false,
    image_format: MachineImageFormat::Tar,
    bootstrap_mode: MachineBootstrapMode::ShellScript,
    oci_artifact_disk_type: "wsl",
};

impl MachineProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Krunkit => "krunkit",
            Self::Vfkit => "vfkit",
            Self::Wsl2 => "wsl2",
        }
    }

    /// Parse a provider selection token (config field or `NIMBUS_MACHINE_PROVIDER`
    /// value). Matching is case-insensitive and ignores surrounding whitespace.
    /// Returns `None` for unknown tokens so callers can surface a clear error.
    pub fn from_token(token: &str) -> Option<Self> {
        match token.trim().to_ascii_lowercase().as_str() {
            "krunkit" => Some(Self::Krunkit),
            "vfkit" => Some(Self::Vfkit),
            "wsl2" => Some(Self::Wsl2),
            _ => None,
        }
    }

    /// Whether this provider runs the Nimbus-managed macOS `applehv` guest that
    /// needs host↔guest binary sync and a forwarded machine API over SSH. Both
    /// macOS microVM backends (krunkit and vfkit) qualify; WSL2 owns its own
    /// guest plumbing.
    pub fn uses_managed_applehv_guest(self) -> bool {
        matches!(self, Self::Krunkit | Self::Vfkit)
    }

    pub fn capabilities(self) -> MachineProviderCapabilities {
        match self {
            Self::Krunkit => KRUNKIT_PROVIDER_CAPABILITIES,
            Self::Vfkit => VFKIT_PROVIDER_CAPABILITIES,
            Self::Wsl2 => WSL2_PROVIDER_CAPABILITIES,
        }
    }

    pub fn uses_provider_networking(self) -> bool {
        self.capabilities().uses_provider_networking
    }

    pub fn requires_exclusive_active(self) -> bool {
        self.capabilities().requires_exclusive_active
    }

    pub fn image_format(self) -> MachineImageFormat {
        self.capabilities().image_format
    }

    pub fn bootstrap_mode(self) -> MachineBootstrapMode {
        self.capabilities().bootstrap_mode
    }

    pub fn oci_artifact_disk_type(self) -> &'static str {
        self.capabilities().oci_artifact_disk_type
    }

    /// The canonical "this provider has no backend on this host yet" error.
    ///
    /// Both the start path (`vmm_backend`) and the stop path
    /// (`stop_provider_machine`) reject not-yet-implemented providers, and both
    /// must reject with the *same* message so selection stays a deliberate,
    /// fail-closed opt-in rather than a silent no-op. Owning the text here keeps
    /// the two gates from drifting apart. The provider name is upper-cased so the
    /// message reads as a proper noun (e.g. `WSL2`).
    pub fn unavailable_error(self) -> Error {
        Error::InvalidInput(format!(
            "the {} machine provider is not available on this host yet",
            self.as_str().to_ascii_uppercase()
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MachineLifecycle {
    Uninitialized,
    Stopped,
    Starting,
    Running,
    Failed,
}

impl MachineLifecycle {
    pub fn as_str(self) -> &'static str {
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
pub enum MachineManagerState {
    Unconfigured,
    HelpersResolved,
    Launching,
    Ready,
    Failed,
    Stale,
}

impl MachineManagerState {
    pub fn as_str(self) -> &'static str {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineRuntimeState {
    pub helper_binaries: MachineHelperBinaryPaths,
    pub image_path: PathBuf,
    pub efi_variable_store_path: PathBuf,
    #[serde(default)]
    pub machine_image_source: String,
    pub ssh_port: u16,
    pub rest_uri: String,
    pub ready_vsock_port: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineHelperBinaryPaths {
    /// The resolved VMM binary for the machine's provider (krunkit or vfkit).
    /// One VMM runs per machine, so this is a single provider-neutral slot.
    pub vmm: PathBuf,
    pub gvproxy: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MachinePaths {
    pub name: String,
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub data_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub config_path: PathBuf,
    pub generated_ignition_path: PathBuf,
    pub state_path: PathBuf,
    pub guest_config_bundle_dir: PathBuf,
    pub image_cache_dir: PathBuf,
    pub guest_binary_cache_dir: PathBuf,
    pub materialized_image_path: PathBuf,
    pub api_socket_path: PathBuf,
    pub ready_socket_path: PathBuf,
    pub ignition_socket_path: PathBuf,
    pub gvproxy_socket_path: PathBuf,
    /// Restful control endpoint for the active VMM (krunkit/vfkit `--restful-uri`).
    /// One VMM runs per machine, so this is a single provider-neutral slot.
    pub vmm_endpoint_path: PathBuf,
    pub efi_variable_store_path: PathBuf,
    pub api_forward_pid_path: PathBuf,
    pub gvproxy_pid_path: PathBuf,
    /// Pidfile for the active VMM process (krunkit/vfkit `--pidfile`). The
    /// readiness/stop lifecycle reads this slot regardless of provider.
    pub vmm_pid_path: PathBuf,
    pub api_forward_log_path: PathBuf,
    pub machine_log_path: PathBuf,
    pub gvproxy_log_path: PathBuf,
    /// Diagnostic log for the active VMM. krunkit writes it via `--log-file`.
    /// vfkit has no such flag, so the spawn path instead captures vfkit's
    /// stdout+stderr into this same file, keeping failed-boot triage uniform
    /// across providers (the guest console log still lives in
    /// [`machine_log_path`](Self::machine_log_path) for both).
    pub vmm_log_path: PathBuf,
}

impl MachinePaths {
    pub fn ensure_directories(&self) -> Result<(), Error> {
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

    pub fn ensure_runtime_directories(&self) -> Result<(), Error> {
        fs::create_dir_all(&self.runtime_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine runtime directory {}: {error}",
                self.runtime_dir.display()
            ))
        })
    }

    pub fn krunkit_gvproxy_socket_path(&self) -> PathBuf {
        PathBuf::from(format!("{}-krun.sock", self.gvproxy_socket_path.display()))
    }
}

fn resolve_config_root_with_env(
    lookup: &mut impl FnMut(&str) -> Option<OsString>,
) -> Result<PathBuf, Error> {
    if let Some(path) = lookup("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(path).join("nimbus").join("machine"));
    }
    Ok(resolve_home_dir_with_env(lookup)?
        .join(".config")
        .join("nimbus")
        .join("machine"))
}

fn resolve_state_root_with_env(
    lookup: &mut impl FnMut(&str) -> Option<OsString>,
) -> Result<PathBuf, Error> {
    if let Some(path) = lookup("XDG_STATE_HOME") {
        return Ok(PathBuf::from(path).join("nimbus").join("machine"));
    }
    Ok(resolve_home_dir_with_env(lookup)?
        .join(".local")
        .join("state")
        .join("nimbus")
        .join("machine"))
}

fn resolve_data_root_with_env(
    lookup: &mut impl FnMut(&str) -> Option<OsString>,
) -> Result<PathBuf, Error> {
    if let Some(path) = lookup("XDG_DATA_HOME") {
        return Ok(PathBuf::from(path).join("nimbus").join("machine"));
    }
    Ok(resolve_home_dir_with_env(lookup)?
        .join(".local")
        .join("share")
        .join("nimbus")
        .join("machine"))
}

fn resolve_cache_root_with_env(
    lookup: &mut impl FnMut(&str) -> Option<OsString>,
) -> Result<PathBuf, Error> {
    if let Some(path) = lookup("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(path).join("nimbus").join("machine"));
    }
    Ok(resolve_home_dir_with_env(lookup)?
        .join(".cache")
        .join("nimbus")
        .join("machine"))
}

fn resolve_home_dir_with_env(
    lookup: &mut impl FnMut(&str) -> Option<OsString>,
) -> Result<PathBuf, Error> {
    if let Some(home) = lookup("HOME") {
        return Ok(PathBuf::from(home));
    }
    if cfg!(windows) {
        if let Some(profile) = lookup("USERPROFILE") {
            return Ok(PathBuf::from(profile));
        }
        if let (Some(drive), Some(path)) = (lookup("HOMEDRIVE"), lookup("HOMEPATH"))
            && !drive.is_empty()
            && !path.is_empty()
        {
            return Ok(PathBuf::from(drive).join(path));
        }
    }
    Err(Error::InvalidInput(
        "HOME is not set; cannot resolve machine directories".to_owned(),
    ))
}

pub fn resolve_runtime_root() -> PathBuf {
    resolve_runtime_root_with_env(&mut |name| env::var_os(name))
}

fn resolve_runtime_root_with_env(lookup: &mut impl FnMut(&str) -> Option<OsString>) -> PathBuf {
    lookup(MACHINE_RUNTIME_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_MACHINE_RUNTIME_ROOT))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn machine_image_source_parse_classifies_supported_sources() {
        let digest = "A".repeat(64);
        assert_eq!(
            MachineImageSource::parse(&format!("https://example.com/disk.raw#sha256={digest}"))
                .expect("http source should parse"),
            MachineImageSource::HttpUrl {
                url: "https://example.com/disk.raw".to_owned(),
                sha256: digest.to_ascii_lowercase(),
            }
        );
        assert_eq!(
            MachineImageSource::parse("docker://registry.example.com/nimbus/machine:latest")
                .expect("explicit docker source should parse"),
            MachineImageSource::OciReference {
                reference: "docker://registry.example.com/nimbus/machine:latest".to_owned(),
            }
        );
        let local_disk = std::env::temp_dir().join("nimbus-machine.raw");
        assert_eq!(
            MachineImageSource::parse(local_disk.to_str().expect("temp path should be utf-8"))
                .expect("absolute disk path should parse"),
            MachineImageSource::LocalDisk { path: local_disk }
        );
        assert_eq!(
            MachineImageSource::parse("registry.example.com/nimbus/machine:latest")
                .expect("implicit docker source should parse"),
            MachineImageSource::OciReference {
                reference: "docker://registry.example.com/nimbus/machine:latest".to_owned(),
            }
        );
    }

    #[test]
    fn machine_image_source_parse_rejects_empty_or_unverified_http_sources() {
        assert_invalid_image_source("", "cannot be empty");
        assert_invalid_image_source("https://example.com/disk.raw", "integrity suffix");
        assert_invalid_image_source(
            "https://example.com/disk.raw#md5=abc",
            "must be #sha256=<64 hex>",
        );
        assert_invalid_image_source("https://example.com/disk.raw#sha256=abc", "exactly 64 hex");
    }

    #[test]
    fn machine_volume_parse_accepts_absolute_source_and_target() {
        assert_eq!(
            MachineVolume::parse("/host/data:/guest/data").expect("volume should parse"),
            MachineVolume {
                source: PathBuf::from("/host/data"),
                target: PathBuf::from("/guest/data"),
            }
        );
    }

    #[test]
    fn machine_volume_parse_rejects_missing_or_relative_paths() {
        assert_volume_error("missing-separator", "expected <source>:<target>");
        assert_volume_error(":/guest", "expected non-empty");
        assert_volume_error("/host:", "expected non-empty");
        assert_volume_error("relative:/guest", "source path must be absolute");
        assert_volume_error("/host:relative", "target path must be absolute");
    }

    #[test]
    fn machine_root_layout_new_uses_explicit_roots() {
        let layout = MachineRootLayout::new(
            PathBuf::from("root/config"),
            PathBuf::from("root/state"),
            PathBuf::from("root/data"),
            PathBuf::from("root/cache"),
            PathBuf::from("root/runtime"),
        );

        assert_eq!(layout.data_root, PathBuf::from("root/data"));
        assert_eq!(layout.cache_root, PathBuf::from("root/cache"));
    }

    #[test]
    fn machine_root_layout_from_sibling_roots_derives_data_and_cache() {
        let layout = MachineRootLayout::from_sibling_roots(
            PathBuf::from("root/config"),
            PathBuf::from("root/state"),
            PathBuf::from("root/runtime"),
        )
        .expect("sibling roots should derive");

        assert_eq!(layout.data_root, PathBuf::from("root/data"));
        assert_eq!(layout.cache_root, PathBuf::from("root/cache"));
    }

    #[test]
    fn machine_root_layout_from_sibling_roots_rejects_unshared_roots() {
        let error = MachineRootLayout::from_sibling_roots(
            PathBuf::from("config-root/config"),
            PathBuf::from("state-root/state"),
            PathBuf::from("runtime-root/runtime"),
        )
        .expect_err("unshared roots should be rejected");

        assert!(
            error.to_string().contains("must share a parent"),
            "error should reject derived roots without falling back to /tmp: {error}"
        );
    }

    #[test]
    fn machine_root_layout_resolve_uses_injected_xdg_and_runtime_env() {
        let layout = MachineRootLayout::resolve_with_env(env_lookup(&[
            ("XDG_CONFIG_HOME", "/xdg/config"),
            ("XDG_STATE_HOME", "/xdg/state"),
            ("XDG_DATA_HOME", "/xdg/data"),
            ("XDG_CACHE_HOME", "/xdg/cache"),
            (MACHINE_RUNTIME_ROOT_ENV, "/run/nimbus-machine"),
        ]))
        .expect("xdg roots should resolve");

        assert_eq!(
            layout.config_root,
            PathBuf::from("/xdg/config/nimbus/machine")
        );
        assert_eq!(
            layout.state_root,
            PathBuf::from("/xdg/state/nimbus/machine")
        );
        assert_eq!(layout.data_root, PathBuf::from("/xdg/data/nimbus/machine"));
        assert_eq!(
            layout.cache_root,
            PathBuf::from("/xdg/cache/nimbus/machine")
        );
        assert_eq!(layout.runtime_root, PathBuf::from("/run/nimbus-machine"));
    }

    #[test]
    fn machine_root_layout_resolve_falls_back_to_home_and_default_runtime() {
        let layout = MachineRootLayout::resolve_with_env(env_lookup(&[("HOME", "/home/alice")]))
            .expect("home fallback should resolve");

        assert_eq!(
            layout.config_root,
            PathBuf::from("/home/alice/.config/nimbus/machine")
        );
        assert_eq!(
            layout.state_root,
            PathBuf::from("/home/alice/.local/state/nimbus/machine")
        );
        assert_eq!(
            layout.data_root,
            PathBuf::from("/home/alice/.local/share/nimbus/machine")
        );
        assert_eq!(
            layout.cache_root,
            PathBuf::from("/home/alice/.cache/nimbus/machine")
        );
        assert_eq!(
            layout.runtime_root,
            PathBuf::from(DEFAULT_MACHINE_RUNTIME_ROOT)
        );
    }

    #[test]
    fn machine_root_layout_resolve_errors_without_home() {
        let error = MachineRootLayout::resolve_with_env(env_lookup(&[]))
            .expect_err("missing home should fail");

        assert!(error.to_string().contains("HOME is not set"), "{error}");
    }

    #[cfg(windows)]
    #[test]
    fn machine_root_layout_resolve_uses_windows_profile_fallback() {
        let layout =
            MachineRootLayout::resolve_with_env(env_lookup(&[("USERPROFILE", "C:\\Users\\Alice")]))
                .expect("windows profile fallback should resolve");

        assert_eq!(
            layout.config_root,
            PathBuf::from("C:\\Users\\Alice").join(".config/nimbus/machine")
        );
    }

    #[test]
    fn machine_paths_ensure_directories_creates_config_state_data_cache_and_runtime_roots() {
        let root = unique_temp_dir("ensure-directories");
        let layout = MachineRootLayout::test_sibling_roots(
            root.join("config"),
            root.join("state"),
            root.join("runtime"),
        );
        let paths = layout.paths("default");

        paths
            .ensure_directories()
            .expect("machine directories should be created");

        assert!(paths.config_dir.is_dir());
        assert!(paths.state_dir.is_dir());
        assert!(paths.data_dir.is_dir());
        assert!(paths.image_cache_dir.is_dir());
        assert!(paths.guest_binary_cache_dir.is_dir());
        assert!(
            paths
                .materialized_image_path
                .parent()
                .expect("materialized image should have parent")
                .is_dir()
        );
        assert!(paths.runtime_dir.is_dir());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn machine_paths_ensure_directories_reports_create_failures_with_path_context() {
        let root = unique_temp_dir("ensure-directories-error");
        fs::create_dir_all(&root).expect("temp root should create");
        fs::write(root.join("config"), b"not a directory").expect("blocking file should write");
        let layout = MachineRootLayout::test_sibling_roots(
            root.join("config"),
            root.join("state"),
            root.join("runtime"),
        );
        let paths = layout.paths("default");

        let error = paths
            .ensure_directories()
            .expect_err("file in place of config directory should fail");

        let message = error.to_string();
        assert!(
            message.contains("failed to create machine config directory")
                && message.contains("default"),
            "{message}"
        );

        let _ = fs::remove_dir_all(root);
    }

    fn assert_invalid_image_source(value: &str, expected: &str) {
        let error = MachineImageSource::parse(value).expect_err("image source should be rejected");
        assert!(
            error.to_string().contains(expected),
            "expected {expected:?} in {error}"
        );
    }

    fn assert_volume_error(value: &str, expected: &str) {
        let error = MachineVolume::parse(value).expect_err("volume should be rejected");
        assert!(
            error.to_string().contains(expected),
            "expected {expected:?} in {error}"
        );
    }

    fn env_lookup(entries: &[(&str, &str)]) -> impl FnMut(&str) -> Option<OsString> {
        let values = entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), OsString::from(value)))
            .collect::<BTreeMap<_, _>>();
        move |name| values.get(name).cloned()
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "nimbus-machine-{}-{}-{label}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }
}
