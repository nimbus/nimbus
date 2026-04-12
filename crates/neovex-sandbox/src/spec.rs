use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxSpec {
    pub tenant_id: TenantId,
    pub name: String,
    pub backend: SandboxBackendKind,
    pub filesystem: SandboxFilesystemSpec,
    pub process: SandboxProcessSpec,
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
            port_bindings: Vec::new(),
        }
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
