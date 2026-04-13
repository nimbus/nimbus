use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use neovex_core::TenantId;

use crate::backend::SandboxBackendKind;
use crate::endpoint::PublishedEndpointProtocol;

const DEFAULT_SANDBOX_PATH: &str =
    "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

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
        self.cwd == PathBuf::from("/")
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
    pub cpu_count: Option<u8>,
    pub memory_limit_bytes: Option<u64>,
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

    pub fn is_unspecified(&self) -> bool {
        self.cpu_count.is_none() && self.memory_limit_bytes.is_none()
    }
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

    use super::{SandboxLifecycleSpec, SandboxRestartPolicy};

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
