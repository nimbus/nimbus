#![cfg(target_os = "linux")]
#![allow(clippy::field_reassign_with_default)]

use std::env;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;

use nimbus_core::TenantId;
use nimbus_sandbox::backends::krun::{
    KrunLaunchMode, KrunSandboxBackend, KrunSandboxBackendConfig,
};
use nimbus_sandbox::{
    PublishedEndpointProtocol, SandboxBackend, SandboxBackendKind, SandboxFilesystemSpec,
    SandboxImageLaunchSpec, SandboxImageProcessOverrides, SandboxPortBinding, SandboxProcessSpec,
    SandboxResourceLimits, SandboxRestartPolicy, SandboxSpec, SandboxStatus,
};

#[path = "krun_linux_smoke/cleanup.rs"]
mod cleanup;
#[path = "krun_linux_smoke/inspect.rs"]
mod inspect;
#[path = "krun_linux_smoke/launch.rs"]
mod launch;
#[path = "krun_linux_smoke/published_endpoints.rs"]
mod published_endpoints;
#[path = "krun_linux_smoke/restart.rs"]
mod restart;
#[path = "krun_linux_smoke/support.rs"]
mod support;
