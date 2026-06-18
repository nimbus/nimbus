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
use nimbus_sandbox::backends::krun::{KrunSandboxBackend, KrunSandboxBackendConfig, KrunStartMode};
use nimbus_sandbox::{
    PublishedEndpointProtocol, SandboxBackend, SandboxBackendKind, SandboxOwnerSpec,
    SandboxPortBinding, SandboxProcessSpec, SandboxResourceLimits, SandboxRestartPolicy,
    SandboxRootSpec, SandboxSpec, SandboxStatus,
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
