use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use nimbus_core::TenantId;

use crate::backend::SandboxBackendKind;
use crate::egress::SandboxEgressPolicy;
use crate::endpoint::PublishedEndpointProtocol;
use crate::error::{Result, SandboxError};

const DEFAULT_SANDBOX_PATH: &str =
    "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
pub const DEFAULT_MAX_MOUNTS_PER_SANDBOX: usize = 32;
const BYTES_PER_MIB: u64 = 1024 * 1024;
const BYTES_PER_GIB: u64 = 1024 * BYTES_PER_MIB;
pub const DEFAULT_MAX_ACTIVE_SANDBOXES_PER_TENANT: usize = 64;
pub const DEFAULT_MAX_SANDBOX_VCPUS_PER_TENANT: u64 = 128;
pub const DEFAULT_MAX_SANDBOX_MEMORY_BYTES_PER_TENANT: u64 = 256 * BYTES_PER_GIB;
pub const DEFAULT_MAX_SANDBOX_DISK_BYTES_PER_TENANT: u64 = 2 * 1024 * BYTES_PER_GIB;
pub const DEFAULT_MAX_SANDBOX_LOG_BYTES_PER_TENANT: u64 = 64 * BYTES_PER_GIB;
pub const DEFAULT_ACCOUNTED_SANDBOX_VCPUS: u64 = 1;
pub const DEFAULT_ACCOUNTED_SANDBOX_MEMORY_BYTES: u64 = 512 * BYTES_PER_MIB;
pub const DEFAULT_ACCOUNTED_SANDBOX_DISK_BYTES: u64 = 10 * BYTES_PER_GIB;
pub const DEFAULT_ACCOUNTED_SANDBOX_LOG_BYTES: u64 = 64 * BYTES_PER_MIB;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxRootfsSpec {
    pub rootfs: PathBuf,
    pub readonly: bool,
}

impl SandboxRootfsSpec {
    pub fn new(rootfs: impl Into<PathBuf>) -> Self {
        Self {
            rootfs: rootfs.into(),
            readonly: false,
        }
    }

    pub fn read_only(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
        self
    }

    pub fn is_unspecified(&self) -> bool {
        self.rootfs.as_os_str().is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SandboxRootSpec {
    Rootfs(SandboxRootfsSpec),
    OciImage(SandboxOciImageSpec),
}

impl SandboxRootSpec {
    pub fn rootfs(rootfs: impl Into<PathBuf>) -> Self {
        Self::Rootfs(SandboxRootfsSpec::new(rootfs))
    }

    pub fn oci_image(source: SandboxOciImageSource) -> Self {
        Self::OciImage(SandboxOciImageSpec::new(source))
    }

    pub fn oci_image_reference(reference: impl Into<String>) -> Self {
        Self::oci_image(SandboxOciImageSource::Reference(
            SandboxOciImageReferenceSpec::new(reference),
        ))
    }

    pub fn oci_image_build(
        image_name: impl Into<String>,
        dockerfile_path: impl Into<PathBuf>,
        context_path: impl Into<PathBuf>,
    ) -> Self {
        Self::oci_image(SandboxOciImageSource::Build(SandboxOciBuildSpec::new(
            image_name,
            dockerfile_path,
            context_path,
        )))
    }

    pub fn rootfs_spec(&self) -> Option<&SandboxRootfsSpec> {
        match self {
            Self::Rootfs(rootfs) => Some(rootfs),
            Self::OciImage(_) => None,
        }
    }

    pub fn is_unspecified_rootfs(&self) -> bool {
        self.rootfs_spec()
            .is_some_and(SandboxRootfsSpec::is_unspecified)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxOciImageSpec {
    pub source: SandboxOciImageSource,
}

impl SandboxOciImageSpec {
    pub fn new(source: SandboxOciImageSource) -> Self {
        Self { source }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SandboxOciImageSource {
    Reference(SandboxOciImageReferenceSpec),
    Build(SandboxOciBuildSpec),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxOciImageReferenceSpec {
    pub reference: String,
}

impl SandboxOciImageReferenceSpec {
    pub fn new(reference: impl Into<String>) -> Self {
        Self {
            reference: reference.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxOciBuildSpec {
    pub image_name: String,
    pub dockerfile_path: PathBuf,
    pub context_path: PathBuf,
}

impl SandboxOciBuildSpec {
    pub fn new(
        image_name: impl Into<String>,
        dockerfile_path: impl Into<PathBuf>,
        context_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            image_name: image_name.into(),
            dockerfile_path: dockerfile_path.into(),
            context_path: context_path.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SandboxOwnerSpec {
    Service { name: String },
    Standalone { display_name: Option<String> },
}

impl SandboxOwnerSpec {
    pub fn service(name: impl Into<String>) -> Self {
        Self::Service { name: name.into() }
    }

    pub fn standalone() -> Self {
        Self::Standalone { display_name: None }
    }

    pub fn standalone_named(display_name: impl Into<String>) -> Self {
        Self::Standalone {
            display_name: Some(display_name.into()),
        }
    }

    pub fn service_name(&self) -> Option<&str> {
        match self {
            Self::Service { name } => Some(name.as_str()),
            Self::Standalone { .. } => None,
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Self::Service { name } => name.as_str(),
            Self::Standalone {
                display_name: Some(display_name),
            } => display_name.as_str(),
            Self::Standalone { display_name: None } => "sandbox",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxProcessSpec {
    pub args: Vec<String>,
    #[serde(default)]
    pub entrypoint: Option<Vec<String>>,
    #[serde(default)]
    pub command: Option<Vec<String>>,
    pub env: Vec<String>,
    pub cwd: PathBuf,
    pub user: Option<String>,
    pub terminal: bool,
}

impl SandboxProcessSpec {
    pub fn new(args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            args: args.into_iter().map(Into::into).collect(),
            entrypoint: None,
            command: None,
            env: vec![DEFAULT_SANDBOX_PATH.to_owned()],
            cwd: PathBuf::from("/"),
            user: None,
            terminal: false,
        }
    }

    pub fn with_entrypoint(
        mut self,
        entrypoint: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.entrypoint = Some(entrypoint.into_iter().map(Into::into).collect());
        self
    }

    pub fn with_command(mut self, command: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.command = Some(command.into_iter().map(Into::into).collect());
        self
    }

    pub fn with_env(mut self, env: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.env = env.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = cwd.into();
        self
    }

    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    pub fn with_terminal(mut self, terminal: bool) -> Self {
        self.terminal = terminal;
        self
    }

    pub fn uses_default_env(&self) -> bool {
        self.env == [DEFAULT_SANDBOX_PATH.to_owned()]
    }

    pub fn uses_default_cwd(&self) -> bool {
        self.cwd == Path::new("/")
    }
}

pub(crate) fn resolve_process_without_image_defaults(
    process: &SandboxProcessSpec,
) -> Result<SandboxProcessSpec> {
    let mut resolved = process.clone();
    if resolved.args.is_empty() {
        let mut args = Vec::new();
        if let Some(entrypoint) = process.entrypoint.as_ref() {
            args.extend(entrypoint.iter().cloned());
        }
        if let Some(command) = process.command.as_ref() {
            args.extend(command.iter().cloned());
        }
        resolved.args = args;
    }

    resolved.entrypoint = None;
    resolved.command = None;

    if resolved.args.is_empty() {
        return Err(SandboxError::InvalidSpec {
            message:
                "rootfs-backed sandboxes must set process args or entrypoint/command to launch"
                    .to_owned(),
        });
    }

    Ok(resolved)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxPortBinding {
    pub name: String,
    pub protocol: PublishedEndpointProtocol,
    pub host_address: IpAddr,
    pub host_port: u16,
    pub guest_port: u16,
}

impl SandboxPortBinding {
    pub fn new(
        name: impl Into<String>,
        protocol: PublishedEndpointProtocol,
        host_port: u16,
        guest_port: u16,
    ) -> Self {
        Self {
            name: name.into(),
            protocol,
            host_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            host_port,
            guest_port,
        }
    }

    pub fn tcp(name: impl Into<String>, host_port: u16, guest_port: u16) -> Self {
        Self::new(name, PublishedEndpointProtocol::Tcp, host_port, guest_port)
    }

    pub fn with_host_address(mut self, host_address: IpAddr) -> Self {
        self.host_address = host_address;
        self
    }

    pub fn host_socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.host_address, self.host_port)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxResourceLimits {
    #[serde(default)]
    pub cpu_count: Option<u8>,
    #[serde(default)]
    pub memory_limit_bytes: Option<u64>,
    #[serde(default)]
    pub disk_limit_bytes: Option<u64>,
    #[serde(default)]
    pub log_limit_bytes: Option<u64>,
}

impl SandboxResourceLimits {
    pub fn with_cpu_count(mut self, cpu_count: u8) -> Self {
        self.cpu_count = Some(cpu_count);
        self
    }

    pub fn with_memory_limit_bytes(mut self, memory_limit_bytes: u64) -> Self {
        self.memory_limit_bytes = Some(memory_limit_bytes);
        self
    }

    pub fn with_disk_limit_bytes(mut self, disk_limit_bytes: u64) -> Self {
        self.disk_limit_bytes = Some(disk_limit_bytes);
        self
    }

    pub fn with_log_limit_bytes(mut self, log_limit_bytes: u64) -> Self {
        self.log_limit_bytes = Some(log_limit_bytes);
        self
    }

    pub fn is_unspecified(&self) -> bool {
        self.cpu_count.is_none()
            && self.memory_limit_bytes.is_none()
            && self.disk_limit_bytes.is_none()
            && self.log_limit_bytes.is_none()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxResourceCharge {
    pub active_sandboxes: usize,
    pub vcpus: u64,
    pub memory_bytes: u64,
    pub disk_bytes: u64,
    pub log_bytes: u64,
}

impl SandboxResourceCharge {
    pub fn plus(self, other: Self) -> Self {
        Self {
            active_sandboxes: self.active_sandboxes.saturating_add(other.active_sandboxes),
            vcpus: self.vcpus.saturating_add(other.vcpus),
            memory_bytes: self.memory_bytes.saturating_add(other.memory_bytes),
            disk_bytes: self.disk_bytes.saturating_add(other.disk_bytes),
            log_bytes: self.log_bytes.saturating_add(other.log_bytes),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxResourceQuotaPolicy {
    pub max_active_sandboxes_per_tenant: Option<usize>,
    pub max_vcpus_per_tenant: Option<u64>,
    pub max_memory_bytes_per_tenant: Option<u64>,
    pub max_disk_bytes_per_tenant: Option<u64>,
    pub max_log_bytes_per_tenant: Option<u64>,
    pub default_vcpus_per_sandbox: u64,
    pub default_memory_bytes_per_sandbox: u64,
    pub default_disk_bytes_per_sandbox: u64,
    pub default_log_bytes_per_sandbox: u64,
}

impl SandboxResourceQuotaPolicy {
    pub fn unlimited() -> Self {
        Self {
            max_active_sandboxes_per_tenant: None,
            max_vcpus_per_tenant: None,
            max_memory_bytes_per_tenant: None,
            max_disk_bytes_per_tenant: None,
            max_log_bytes_per_tenant: None,
            ..Self::default()
        }
    }

    pub fn with_max_active_sandboxes_per_tenant(mut self, limit: Option<usize>) -> Self {
        self.max_active_sandboxes_per_tenant = limit;
        self
    }

    pub fn with_max_vcpus_per_tenant(mut self, limit: Option<u64>) -> Self {
        self.max_vcpus_per_tenant = limit;
        self
    }

    pub fn with_max_memory_bytes_per_tenant(mut self, limit: Option<u64>) -> Self {
        self.max_memory_bytes_per_tenant = limit;
        self
    }

    pub fn with_max_disk_bytes_per_tenant(mut self, limit: Option<u64>) -> Self {
        self.max_disk_bytes_per_tenant = limit;
        self
    }

    pub fn with_max_log_bytes_per_tenant(mut self, limit: Option<u64>) -> Self {
        self.max_log_bytes_per_tenant = limit;
        self
    }

    pub fn charge_for(&self, resources: &SandboxResourceLimits) -> SandboxResourceCharge {
        SandboxResourceCharge {
            active_sandboxes: 1,
            vcpus: resources
                .cpu_count
                .map(u64::from)
                .unwrap_or(self.default_vcpus_per_sandbox),
            memory_bytes: resources
                .memory_limit_bytes
                .unwrap_or(self.default_memory_bytes_per_sandbox),
            disk_bytes: resources
                .disk_limit_bytes
                .unwrap_or(self.default_disk_bytes_per_sandbox),
            log_bytes: resources
                .log_limit_bytes
                .unwrap_or(self.default_log_bytes_per_sandbox),
        }
    }
}

impl Default for SandboxResourceQuotaPolicy {
    fn default() -> Self {
        Self {
            max_active_sandboxes_per_tenant: Some(DEFAULT_MAX_ACTIVE_SANDBOXES_PER_TENANT),
            max_vcpus_per_tenant: Some(DEFAULT_MAX_SANDBOX_VCPUS_PER_TENANT),
            max_memory_bytes_per_tenant: Some(DEFAULT_MAX_SANDBOX_MEMORY_BYTES_PER_TENANT),
            max_disk_bytes_per_tenant: Some(DEFAULT_MAX_SANDBOX_DISK_BYTES_PER_TENANT),
            max_log_bytes_per_tenant: Some(DEFAULT_MAX_SANDBOX_LOG_BYTES_PER_TENANT),
            default_vcpus_per_sandbox: DEFAULT_ACCOUNTED_SANDBOX_VCPUS,
            default_memory_bytes_per_sandbox: DEFAULT_ACCOUNTED_SANDBOX_MEMORY_BYTES,
            default_disk_bytes_per_sandbox: DEFAULT_ACCOUNTED_SANDBOX_DISK_BYTES,
            default_log_bytes_per_sandbox: DEFAULT_ACCOUNTED_SANDBOX_LOG_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SandboxMountSource {
    TenantVolume { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxMountSpec {
    pub source: SandboxMountSource,
    pub destination: PathBuf,
    #[serde(default)]
    pub read_only: bool,
}

impl SandboxMountSpec {
    pub fn tenant_volume(name: impl Into<String>, destination: impl Into<PathBuf>) -> Self {
        Self {
            source: SandboxMountSource::TenantVolume { name: name.into() },
            destination: destination.into(),
            read_only: false,
        }
    }

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub fn tenant_volume_name(&self) -> Option<&str> {
        match &self.source {
            SandboxMountSource::TenantVolume { name } => Some(name.as_str()),
        }
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        match &self.source {
            SandboxMountSource::TenantVolume { name } => validate_tenant_volume_name(name)?,
        }
        validate_mount_destination(&self.destination)
    }
}

pub fn validate_tenant_volume_name(name: &str) -> std::result::Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("tenant volume names cannot be empty".to_owned());
    }
    if trimmed != name {
        return Err(format!(
            "tenant volume name {name:?} must not contain surrounding whitespace"
        ));
    }
    if trimmed == "." || trimmed == ".." {
        return Err(format!("tenant volume name {name:?} is reserved"));
    }
    if trimmed.len() > 128 {
        return Err(format!(
            "tenant volume name {name:?} exceeds the 128 byte limit"
        ));
    }
    if !trimmed
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
    {
        return Err(format!(
            "tenant volume name {name:?} must contain only ASCII letters, digits, '.', '_' or '-'"
        ));
    }
    Ok(())
}

pub fn validate_sandbox_mounts(mounts: &[SandboxMountSpec]) -> std::result::Result<(), String> {
    if mounts.len() > DEFAULT_MAX_MOUNTS_PER_SANDBOX {
        return Err(format!(
            "sandbox mount quota exceeded: {} mounts requested, limit {}",
            mounts.len(),
            DEFAULT_MAX_MOUNTS_PER_SANDBOX
        ));
    }

    let mut destinations = BTreeSet::new();
    for mount in mounts {
        mount.validate()?;
        let destination = mount.destination.to_string_lossy().into_owned();
        if !destinations.insert(destination.clone()) {
            return Err(format!(
                "duplicate sandbox mount destination: {destination}"
            ));
        }
    }
    Ok(())
}

fn validate_mount_destination(destination: &Path) -> std::result::Result<(), String> {
    if !destination.is_absolute() {
        return Err(format!(
            "sandbox mount destination {} must be an absolute guest path",
            destination.display()
        ));
    }
    if destination == Path::new("/") {
        return Err("sandbox mount destination must not be the guest root".to_owned());
    }

    for component in destination.components() {
        if matches!(component, Component::ParentDir | Component::CurDir) {
            return Err(format!(
                "sandbox mount destination {} must not contain '.' or '..'",
                destination.display()
            ));
        }
    }

    for reserved in ["/proc", "/sys", "/dev", "/.nimbus"] {
        let reserved_path = Path::new(reserved);
        if destination == reserved_path || destination.starts_with(reserved_path) {
            return Err(format!(
                "sandbox mount destination {} overlaps reserved guest path {reserved}",
                destination.display()
            ));
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxRestartPolicy {
    #[default]
    Never,
    OnFailure {
        max_restarts: u32,
    },
    Always {
        max_restarts: u32,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxLifecycleSpec {
    pub restart_policy: SandboxRestartPolicy,
    #[serde(default, with = "duration_millis_option")]
    pub stop_timeout: Option<Duration>,
}

impl SandboxLifecycleSpec {
    pub fn with_restart_policy(mut self, restart_policy: SandboxRestartPolicy) -> Self {
        self.restart_policy = restart_policy;
        self
    }

    pub fn with_stop_timeout(mut self, stop_timeout: Duration) -> Self {
        self.stop_timeout = Some(stop_timeout);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxSpec {
    pub tenant_id: TenantId,
    pub owner: SandboxOwnerSpec,
    pub backend: SandboxBackendKind,
    pub root: SandboxRootSpec,
    pub process: SandboxProcessSpec,
    pub resources: SandboxResourceLimits,
    #[serde(default)]
    pub lifecycle: SandboxLifecycleSpec,
    pub port_bindings: Vec<SandboxPortBinding>,
    #[serde(default)]
    pub mounts: Vec<SandboxMountSpec>,
    #[serde(default)]
    pub egress: SandboxEgressPolicy,
}

impl SandboxSpec {
    pub fn new(
        tenant_id: TenantId,
        owner: SandboxOwnerSpec,
        backend: SandboxBackendKind,
        root: SandboxRootSpec,
        process: SandboxProcessSpec,
    ) -> Self {
        Self {
            tenant_id,
            owner,
            backend,
            root,
            process,
            resources: SandboxResourceLimits::default(),
            lifecycle: SandboxLifecycleSpec::default(),
            port_bindings: Vec::new(),
            mounts: Vec::new(),
            egress: SandboxEgressPolicy::default(),
        }
    }

    pub fn service_name(&self) -> Option<&str> {
        self.owner.service_name()
    }

    pub fn display_name(&self) -> &str {
        self.owner.display_name()
    }

    pub fn rootfs(&self) -> Option<&SandboxRootfsSpec> {
        self.root.rootfs_spec()
    }

    pub fn with_resource_limits(mut self, resources: SandboxResourceLimits) -> Self {
        self.resources = resources;
        self
    }

    pub fn with_lifecycle(mut self, lifecycle: SandboxLifecycleSpec) -> Self {
        self.lifecycle = lifecycle;
        self
    }

    pub fn with_restart_policy(mut self, restart_policy: SandboxRestartPolicy) -> Self {
        self.lifecycle.restart_policy = restart_policy;
        self
    }

    pub fn with_stop_timeout(mut self, stop_timeout: Duration) -> Self {
        self.lifecycle.stop_timeout = Some(stop_timeout);
        self
    }

    pub fn with_cpu_count(mut self, cpu_count: u8) -> Self {
        self.resources.cpu_count = Some(cpu_count);
        self
    }

    pub fn with_memory_limit_bytes(mut self, memory_limit_bytes: u64) -> Self {
        self.resources.memory_limit_bytes = Some(memory_limit_bytes);
        self
    }

    pub fn with_disk_limit_bytes(mut self, disk_limit_bytes: u64) -> Self {
        self.resources.disk_limit_bytes = Some(disk_limit_bytes);
        self
    }

    pub fn with_log_limit_bytes(mut self, log_limit_bytes: u64) -> Self {
        self.resources.log_limit_bytes = Some(log_limit_bytes);
        self
    }

    pub fn with_port_binding(mut self, port_binding: SandboxPortBinding) -> Self {
        self.port_bindings.push(port_binding);
        self
    }

    pub fn with_port_bindings(
        mut self,
        port_bindings: impl IntoIterator<Item = SandboxPortBinding>,
    ) -> Self {
        self.port_bindings.extend(port_bindings);
        self
    }

    pub fn with_mount(mut self, mount: SandboxMountSpec) -> Self {
        self.mounts.push(mount);
        self
    }

    pub fn with_mounts(mut self, mounts: impl IntoIterator<Item = SandboxMountSpec>) -> Self {
        self.mounts.extend(mounts);
        self
    }

    pub fn with_egress_policy(mut self, egress: SandboxEgressPolicy) -> Self {
        self.egress = egress;
        self
    }
}

mod duration_millis_option {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer, ser::Error as _};

    pub(super) fn serialize<S>(value: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(duration) => {
                let millis = u64::try_from(duration.as_millis())
                    .map_err(|_| S::Error::custom("duration overflowed u64 milliseconds"))?;
                serializer.serialize_some(&millis)
            }
            None => serializer.serialize_none(),
        }
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Option::<u64>::deserialize(deserializer)?.map(Duration::from_millis))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::{
        DEFAULT_MAX_MOUNTS_PER_SANDBOX, SandboxLifecycleSpec, SandboxMountSpec, SandboxProcessSpec,
        SandboxRestartPolicy, resolve_process_without_image_defaults, validate_sandbox_mounts,
    };

    #[test]
    fn sandbox_mount_validation_rejects_duplicate_destinations() {
        let mounts = vec![
            SandboxMountSpec::tenant_volume("cache-a", "/cache"),
            SandboxMountSpec::tenant_volume("cache-b", "/cache"),
        ];

        let error =
            validate_sandbox_mounts(&mounts).expect_err("duplicate destination should fail");
        assert!(
            error.contains("duplicate sandbox mount destination"),
            "expected duplicate destination error, got: {error}"
        );
    }

    #[test]
    fn sandbox_mount_validation_enforces_per_sandbox_quota() {
        let mounts = (0..=DEFAULT_MAX_MOUNTS_PER_SANDBOX)
            .map(|index| {
                SandboxMountSpec::tenant_volume(format!("cache-{index}"), format!("/cache/{index}"))
            })
            .collect::<Vec<_>>();

        let error = validate_sandbox_mounts(&mounts).expect_err("mount quota should fail");
        assert!(
            error.contains("sandbox mount quota exceeded"),
            "expected mount quota error, got: {error}"
        );
    }

    #[test]
    fn rootfs_process_resolution_uses_entrypoint_and_command_when_args_are_empty() {
        let process = SandboxProcessSpec::new(Vec::<String>::new())
            .with_entrypoint(["/bin/sh", "-lc"])
            .with_command(["exec app"]);

        let resolved = resolve_process_without_image_defaults(&process)
            .expect("entrypoint and command should resolve rootfs process args");

        assert_eq!(resolved.args, vec!["/bin/sh", "-lc", "exec app"]);
        assert_eq!(resolved.entrypoint, None);
        assert_eq!(resolved.command, None);
    }

    #[test]
    fn rootfs_process_resolution_prefers_explicit_args() {
        let process = SandboxProcessSpec::new(["/usr/bin/app"])
            .with_entrypoint(["/ignored"])
            .with_command(["ignored"]);

        let resolved = resolve_process_without_image_defaults(&process)
            .expect("explicit args should be valid for rootfs process launch");

        assert_eq!(resolved.args, vec!["/usr/bin/app"]);
        assert_eq!(resolved.entrypoint, None);
        assert_eq!(resolved.command, None);
    }

    #[test]
    fn rootfs_process_resolution_rejects_empty_runtime_command() {
        let process = SandboxProcessSpec::new(Vec::<String>::new());

        let error = resolve_process_without_image_defaults(&process)
            .expect_err("rootfs process launch requires a runtime command");

        assert!(
            error
                .to_string()
                .contains("rootfs-backed sandboxes must set process args"),
            "{error}"
        );
    }

    #[test]
    fn sandbox_lifecycle_spec_serializes_stop_timeout_as_millis() {
        let lifecycle = SandboxLifecycleSpec::default()
            .with_restart_policy(SandboxRestartPolicy::OnFailure { max_restarts: 3 })
            .with_stop_timeout(Duration::from_millis(30_500));

        let value = serde_json::to_value(&lifecycle).expect("lifecycle should serialize");
        assert_eq!(
            value,
            json!({
                "restart_policy": {
                    "on_failure": {
                        "max_restarts": 3
                    }
                },
                "stop_timeout": 30_500
            })
        );

        let roundtrip: SandboxLifecycleSpec =
            serde_json::from_value(value).expect("lifecycle should deserialize");
        assert_eq!(roundtrip, lifecycle);
    }
}
