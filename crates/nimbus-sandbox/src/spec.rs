use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use nimbus_core::TenantId;

use crate::backend::SandboxBackendKind;
use crate::endpoint::PublishedEndpointProtocol;

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
pub struct SandboxFilesystemSpec {
    pub rootfs: PathBuf,
    pub readonly: bool,
}

impl SandboxFilesystemSpec {
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxImageProcessOverrides {
    pub entrypoint: Option<Vec<String>>,
    pub cmd: Option<Vec<String>>,
    #[serde(default)]
    pub env: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub user: Option<String>,
    #[serde(default)]
    pub terminal: bool,
}

impl SandboxImageProcessOverrides {
    pub fn with_entrypoint(
        mut self,
        entrypoint: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.entrypoint = Some(entrypoint.into_iter().map(Into::into).collect());
        self
    }

    pub fn with_cmd(mut self, cmd: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.cmd = Some(cmd.into_iter().map(Into::into).collect());
        self
    }

    pub fn with_env(mut self, env: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.env = env.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxProcessSpec {
    pub args: Vec<String>,
    pub env: Vec<String>,
    pub cwd: PathBuf,
    pub terminal: bool,
}

impl SandboxProcessSpec {
    pub fn new(args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            args: args.into_iter().map(Into::into).collect(),
            env: vec![DEFAULT_SANDBOX_PATH.to_owned()],
            cwd: PathBuf::from("/"),
            terminal: false,
        }
    }

    pub fn with_env(mut self, env: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.env = env.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = cwd.into();
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxImageLaunchSpec {
    pub spec: SandboxSpec,
    pub image_reference: String,
    #[serde(default)]
    pub process_overrides: SandboxImageProcessOverrides,
}

impl SandboxImageLaunchSpec {
    pub fn new(spec: SandboxSpec, image_reference: impl Into<String>) -> Self {
        Self {
            spec,
            image_reference: image_reference.into(),
            process_overrides: SandboxImageProcessOverrides::default(),
        }
    }

    pub fn with_process_overrides(
        mut self,
        process_overrides: SandboxImageProcessOverrides,
    ) -> Self {
        self.process_overrides = process_overrides;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxBuildLaunchSpec {
    pub spec: SandboxSpec,
    pub image_name: String,
    pub dockerfile_path: PathBuf,
    pub context_path: PathBuf,
    #[serde(default)]
    pub process_overrides: SandboxImageProcessOverrides,
}

impl SandboxBuildLaunchSpec {
    pub fn new(
        spec: SandboxSpec,
        image_name: impl Into<String>,
        dockerfile_path: impl Into<PathBuf>,
        context_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            spec,
            image_name: image_name.into(),
            dockerfile_path: dockerfile_path.into(),
            context_path: context_path.into(),
            process_overrides: SandboxImageProcessOverrides::default(),
        }
    }

    pub fn with_process_overrides(
        mut self,
        process_overrides: SandboxImageProcessOverrides,
    ) -> Self {
        self.process_overrides = process_overrides;
        self
    }
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
    pub name: String,
    pub backend: SandboxBackendKind,
    pub filesystem: SandboxFilesystemSpec,
    pub process: SandboxProcessSpec,
    pub resources: SandboxResourceLimits,
    #[serde(default)]
    pub lifecycle: SandboxLifecycleSpec,
    pub port_bindings: Vec<SandboxPortBinding>,
    #[serde(default)]
    pub mounts: Vec<SandboxMountSpec>,
}

impl SandboxSpec {
    pub fn new(
        tenant_id: TenantId,
        name: impl Into<String>,
        backend: SandboxBackendKind,
        filesystem: SandboxFilesystemSpec,
        process: SandboxProcessSpec,
    ) -> Self {
        Self {
            tenant_id,
            name: name.into(),
            backend,
            filesystem,
            process,
            resources: SandboxResourceLimits::default(),
            lifecycle: SandboxLifecycleSpec::default(),
            port_bindings: Vec::new(),
            mounts: Vec::new(),
        }
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
        DEFAULT_MAX_MOUNTS_PER_SANDBOX, SandboxLifecycleSpec, SandboxMountSpec,
        SandboxRestartPolicy, validate_sandbox_mounts,
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
