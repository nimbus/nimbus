use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use clap::{Parser, Subcommand};
use nimbus::{
    SandboxBackendKind, SandboxBuildLaunchSpec, SandboxFilesystemSpec, SandboxId,
    SandboxImageLaunchSpec, SandboxProcessSpec, SandboxServiceLaunch, SandboxSpec, SandboxStatus,
};
use nimbus_sandbox::SandboxFuture;
use nimbus_sandbox::backends::container::{
    ContainerLaunchMode, ContainerSandboxBackend, ContainerSandboxBackendConfig,
};
use serde_json::json;
use tempfile::TempDir;

use crate::compose::execution::{
    load_host_backed_project_backend, load_host_backed_sandbox_service_manager_for_platform,
    should_auto_start_default_machine_for_host_loader,
};
use crate::compose::lifecycle::{start_service_launch, stop_service_target};
use crate::compose::logs::{read_log_chunk, resolve_service_ctr_log_path};
use crate::compose::process::{parse_process_rows, read_pid_file_if_exists};
use crate::machine::{
    MachineApiClient, MachineApiListenMode, MachineApiState, bind_direct_listener,
    default_guest_helper_binary_dirs, serve_machine_api,
};

mod forwarded_api;
mod lifecycle;
mod logs_process;
mod parse_help;
mod render_state;
mod support;

use self::support::*;
