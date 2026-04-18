use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::State as AxumState;
use axum::routing::get;
use flate2::read::GzDecoder;
use fs2::FileExt;
use libc::{SIGKILL, SIGTERM, kill};
use neovex::Error;
use oci_client::Reference;
use oci_client::client::{Client as OciClient, ClientConfig as OciClientConfig, ClientProtocol};
use oci_client::manifest::{
    IMAGE_MANIFEST_LIST_MEDIA_TYPE, IMAGE_MANIFEST_MEDIA_TYPE, OCI_IMAGE_INDEX_MEDIA_TYPE,
    OCI_IMAGE_MEDIA_TYPE, OciDescriptor,
};
use oci_client::secrets::RegistryAuth;
use reqwest::blocking::Client as BlockingClient;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use signal_hook_registry::{SigId, register as register_signal, unregister as unregister_signal};
use tempfile::{NamedTempFile, tempdir_in};
use tokio::io::AsyncWriteExt;

use super::bootstrap::{GUEST_NEOVEX_BIN, GUEST_NEOVEX_SOCKET, resolve_ignition_file};
use super::client::MachineApiClient;
use super::{
    MachineBootstrapMode, MachineConfigRecord, MachineImageFormat, MachineImageSource,
    MachineLifecycle, MachineManagerState, MachinePaths, MachineRootLayout, MachineStateRecord,
    MachineVolume, describe_machine_image_source, desired_machine_image_source,
    invalidate_materialized_machine_os, uses_host_managed_machine_image_contract, write_json_file,
};

const DEFAULT_KRUNKIT_BINARY: &str = "krunkit";
const DEFAULT_GVPROXY_BINARY: &str = "gvproxy";
const DEFAULT_MACHINE_MAC_ADDRESS: &str = "5a:94:ef:e4:0c:ee";
const READY_VSOCK_PORT: u32 = 1025;
const READY_WAIT_TIMEOUT_ENV: &str = "NEOVEX_MACHINE_READY_TIMEOUT_SECS";
const DEFAULT_READY_WAIT_TIMEOUT: Duration = Duration::from_secs(90);
const SSH_READY_WAIT_TIMEOUT_ENV: &str = "NEOVEX_MACHINE_SSH_READY_TIMEOUT_SECS";
const DEFAULT_SSH_READY_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const MACHINE_API_READY_WAIT_TIMEOUT_ENV: &str = "NEOVEX_MACHINE_API_READY_TIMEOUT_SECS";
const DEFAULT_MACHINE_API_READY_TIMEOUT: Duration = Duration::from_secs(30);
const STOP_WAIT_TIMEOUT_ENV: &str = "NEOVEX_MACHINE_STOP_TIMEOUT_SECS";
const DEFAULT_STOP_WAIT_TIMEOUT: Duration = Duration::from_secs(90);
const GVPROXY_SOCKET_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const HARD_STOP_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(200);
const MACHINE_PORT_MIN: u16 = 10000;
const MACHINE_PORT_MAX: u16 = 65535;
const KRUNKIT_ENV: &str = "NEOVEX_MACHINE_KRUNKIT";
const GVPROXY_ENV: &str = "NEOVEX_MACHINE_GVPROXY";
const HELPER_BINARY_DIR_ENV: &str = "NEOVEX_MACHINE_HELPER_BINARY_DIR";
const HTTP_IMAGE_TIMEOUT: Duration = Duration::from_secs(300);
const GUEST_NEOVEX_BINARY_OVERRIDE_ENV: &str = "NEOVEX_MACHINE_GUEST_BINARY";
const GUEST_NEOVEX_RELEASE_BASE_URL_ENV: &str = "NEOVEX_MACHINE_GUEST_RELEASE_BASE_URL";
const DEFAULT_GUEST_NEOVEX_RELEASE_BASE_URL: &str =
    "https://github.com/agentstation/neovex/releases/download";
const DEFAULT_GUEST_NEOVEX_BINARY_ARCHIVE_NAME_ARM64: &str = "neovex_linux_arm64.tar.gz";
const DEFAULT_GUEST_NEOVEX_BINARY_ARCHIVE_NAME_X86_64: &str = "neovex_linux_x86_64.tar.gz";
const DEFAULT_GUEST_NEOVEX_TARGET_TRIPLE_ARM64: &str = "aarch64-unknown-linux-gnu";
const DEFAULT_GUEST_NEOVEX_TARGET_TRIPLE_X86_64: &str = "x86_64-unknown-linux-gnu";
const LOCAL_GUEST_BINARY_HELP_TEXT: &str =
    "run `make build-neovex-machine-guest-binary` or set `NEOVEX_MACHINE_GUEST_BINARY`";
const OCI_MACHINE_OS: &str = "linux";
const OCI_ANNOTATION_TITLE: &str = "org.opencontainers.image.title";
const OCI_ANNOTATION_SOURCE: &str = "org.opencontainers.image.source";
const OCI_ANNOTATION_MACHINE_ATTESTATION_REPOSITORY: &str =
    "io.neovex.machine.attestation.repository";
const OCI_ANNOTATION_MACHINE_NEOVEX_VERSION: &str = "io.neovex.machine.neovex.version";
pub(super) const MACHINE_API_FORWARD_TRANSPORT: &str = "gvproxy-ssh-forwarded-unix-socket";
pub(super) const MACHINE_API_FORWARD_USER: &str = "root";
const PODMAN_DARWIN_HELPER_DIRECTORIES: &[&str] = &[
    "/usr/local/opt/podman/libexec/podman",
    "/opt/homebrew/opt/podman/libexec/podman",
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/opt/homebrew/libexec/podman",
    "/usr/local/libexec/podman",
    "/usr/local/lib/podman",
    "/usr/libexec/podman",
    "/usr/lib/podman",
];

#[derive(Debug, Deserialize)]
struct RegistryImageIndex {
    manifests: Vec<RegistryManifestDescriptor>,
}

#[derive(Debug, Clone, Deserialize)]
struct RegistryManifestDescriptor {
    digest: String,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
    platform: Option<RegistryPlatform>,
}

#[derive(Debug, Clone, Deserialize)]
struct RegistryPlatform {
    architecture: String,
    os: String,
}

#[derive(Debug, Deserialize)]
struct RegistryImageManifest {
    layers: Vec<RegistryLayerDescriptor>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RegistryLayerDescriptor {
    digest: String,
    size: i64,
    #[serde(rename = "mediaType")]
    media_type: String,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct MachineArtifactMetadata {
    attestation_repository: Option<String>,
    source_repository_url: Option<String>,
    neovex_version: Option<String>,
}

#[derive(Debug, Clone)]
struct SelectedMachineArtifact {
    child_reference: Reference,
    layer: RegistryLayerDescriptor,
    metadata: MachineArtifactMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MachineRuntimeState {
    pub(super) helper_binaries: MachineHelperBinaryPaths,
    pub(super) image_path: PathBuf,
    pub(super) efi_variable_store_path: PathBuf,
    #[serde(default)]
    pub(super) machine_image_source: String,
    pub(super) ssh_port: u16,
    pub(super) rest_uri: String,
    pub(super) ready_vsock_port: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MachineHelperBinaryPaths {
    pub(super) krunkit: PathBuf,
    pub(super) gvproxy: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MachineLaunchPlan {
    runtime: MachineRuntimeState,
    gvproxy_command: MachineCommandLine,
    krunkit_command: MachineCommandLine,
    ignition_file_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MachineCommandLine {
    program: PathBuf,
    args: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct MachinePortAllocationState {
    #[serde(default)]
    machine_ports: BTreeMap<String, u16>,
}

struct MachinePortAllocationLock {
    _file: fs::File,
}

struct StartupSignalMonitor {
    interrupted: Arc<AtomicBool>,
    registrations: Vec<SigId>,
}

impl StartupSignalMonitor {
    fn install() -> Result<Self, Error> {
        let interrupted = Arc::new(AtomicBool::new(false));
        let mut registrations = Vec::new();
        for signal in [libc::SIGINT, libc::SIGTERM] {
            let interrupted = Arc::clone(&interrupted);
            let registration = unsafe {
                // The callback performs only an atomic store, which is
                // signal-safe and lets the synchronous startup loops observe
                // interruption without doing non-signal-safe work in the
                // handler itself.
                register_signal(signal, move || {
                    interrupted.store(true, Ordering::SeqCst);
                })
            }
            .map_err(|error| {
                Error::Internal(format!(
                    "failed to register startup signal handler for signal {signal}: {error}"
                ))
            })?;
            registrations.push(registration);
        }
        Ok(Self {
            interrupted,
            registrations,
        })
    }

    fn check(&self) -> Result<(), Error> {
        if self.interrupted.load(Ordering::SeqCst) {
            return Err(Error::Cancelled);
        }
        Ok(())
    }

    #[cfg(test)]
    fn inactive_for_test() -> Self {
        Self {
            interrupted: Arc::new(AtomicBool::new(false)),
            registrations: Vec::new(),
        }
    }

    #[cfg(test)]
    fn interrupted_for_test() -> Self {
        Self {
            interrupted: Arc::new(AtomicBool::new(true)),
            registrations: Vec::new(),
        }
    }
}

impl Drop for StartupSignalMonitor {
    fn drop(&mut self) {
        for registration in self.registrations.drain(..) {
            let _ = unregister_signal(registration);
        }
    }
}

impl MachineCommandLine {
    fn spawn(&self) -> Result<Child, Error> {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        unsafe {
            // Machine helpers should survive the launching CLI process exiting.
            // Put them in their own session so host validation and normal shell
            // use do not depend on the parent process group remaining alive.
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command.spawn().map_err(|error| {
            Error::Internal(format!(
                "failed to start {}: {error}",
                self.program.display()
            ))
        })
    }
}

impl MachineHelperBinaryPaths {
    pub(super) fn resolve() -> Result<Self, Error> {
        let bundled_gvproxy = bundled_helper_candidates(DEFAULT_GVPROXY_BINARY);
        let known_krunkit = known_helper_candidates(DEFAULT_KRUNKIT_BINARY);
        let known_gvproxy = known_helper_candidates(DEFAULT_GVPROXY_BINARY);
        Ok(Self {
            krunkit: resolve_helper_binary(
                KRUNKIT_ENV,
                DEFAULT_KRUNKIT_BINARY,
                &[],
                &known_krunkit,
            )?,
            gvproxy: resolve_helper_binary(
                GVPROXY_ENV,
                DEFAULT_GVPROXY_BINARY,
                &bundled_gvproxy,
                &known_gvproxy,
            )?,
        })
    }
}

impl MachineLaunchPlan {
    pub(super) fn build(
        paths: &MachinePaths,
        config: &MachineConfigRecord,
        state: &MachineStateRecord,
    ) -> Result<Self, Error> {
        let helper_binaries = MachineHelperBinaryPaths::resolve()?;
        let image_path =
            resolve_bootable_image_path(paths, &config.guest.image_source, config.provider)?;
        let ignition_file_path = match config.provider.bootstrap_mode() {
            MachineBootstrapMode::Ignition => {
                Some(resolve_ignition_file(paths, config, READY_VSOCK_PORT)?)
            }
            MachineBootstrapMode::ShellScript => None,
        };
        let ssh_port = allocate_machine_ssh_port(&config.roots, &config.name, state)?;
        let rest_uri = format!("unix://{}", paths.krunkit_endpoint_path.display());
        let runtime = MachineRuntimeState {
            helper_binaries: helper_binaries.clone(),
            image_path: image_path.clone(),
            efi_variable_store_path: config
                .guest
                .efi_variable_store_path
                .clone()
                .unwrap_or_else(|| paths.efi_variable_store_path.clone()),
            machine_image_source: describe_machine_image_source(&config.guest.image_source),
            ssh_port,
            rest_uri: rest_uri.clone(),
            ready_vsock_port: READY_VSOCK_PORT,
        };

        let gvproxy_command = MachineCommandLine {
            program: helper_binaries.gvproxy.clone(),
            args: build_gvproxy_args(paths, config, ssh_port),
        };

        let mut krunkit_args = vec![
            "--cpus".to_owned(),
            config.resources.cpus.to_string(),
            "--memory".to_owned(),
            config.resources.memory_mib.to_string(),
            "--bootloader".to_owned(),
            format!(
                "efi,variable-store={},create",
                runtime.efi_variable_store_path.display()
            ),
            "--restful-uri".to_owned(),
            rest_uri,
            "--pidfile".to_owned(),
            paths.krunkit_pid_path.display().to_string(),
            "--log-file".to_owned(),
            paths.krunkit_log_path.display().to_string(),
            "--device".to_owned(),
            format!("virtio-blk,path={},format=raw", image_path.display()),
            "--device".to_owned(),
            format!(
                "virtio-net,type=unixgram,path={},mac={},offloading=on,vfkitMagic=on",
                paths.gvproxy_socket_path.display(),
                DEFAULT_MACHINE_MAC_ADDRESS
            ),
            "--device".to_owned(),
            format!(
                "virtio-serial,logFilePath={}",
                paths.machine_log_path.display()
            ),
        ];
        if config.provider.bootstrap_mode() == MachineBootstrapMode::Ignition {
            krunkit_args.extend([
                "--device".to_owned(),
                build_virtio_vsock_listen_arg(READY_VSOCK_PORT, &paths.ready_socket_path),
                "--device".to_owned(),
                build_virtio_vsock_listen_arg(1024, &paths.ignition_socket_path),
            ]);
        }
        krunkit_args.extend(
            config
                .volumes
                .iter()
                .flat_map(build_virtiofs_args)
                .collect::<Vec<_>>(),
        );

        let krunkit_command = MachineCommandLine {
            program: helper_binaries.krunkit.clone(),
            args: krunkit_args,
        };

        Ok(Self {
            runtime,
            gvproxy_command,
            krunkit_command,
            ignition_file_path,
        })
    }

    pub(super) fn runtime(&self) -> &MachineRuntimeState {
        &self.runtime
    }
}

fn build_gvproxy_args(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    ssh_port: u16,
) -> Vec<String> {
    let mut args = vec![
        "-listen-vfkit".to_owned(),
        format!("unixgram://{}", paths.gvproxy_socket_path.display()),
        "-pid-file".to_owned(),
        paths.gvproxy_pid_path.display().to_string(),
        "-log-file".to_owned(),
        paths.gvproxy_log_path.display().to_string(),
        "-ssh-port".to_owned(),
        ssh_port.to_string(),
    ];

    if let Some(identity_path) = config.guest.ssh_identity_path.as_ref() {
        // Match Podman's machine-plumbing shape: gvproxy owns the host-local
        // forwarded socket and reaches the guest system socket over SSH.
        // The guest machine API lives at /run/neovex/neovex.sock, so we
        // forward as root rather than the interactive SSH user.
        args.extend([
            "-forward-sock".to_owned(),
            paths.api_socket_path.display().to_string(),
            "-forward-dest".to_owned(),
            GUEST_NEOVEX_SOCKET.to_owned(),
            "-forward-user".to_owned(),
            MACHINE_API_FORWARD_USER.to_owned(),
            "-forward-identity".to_owned(),
            identity_path.display().to_string(),
        ]);
    }

    args
}

fn build_virtio_vsock_listen_arg(port: u32, socket_path: &Path) -> String {
    // Match Podman's vfkit/libkrun contract: the host owns these Unix sockets
    // and krunkit must connect the guest-side vsock device to that listener.
    format!(
        "virtio-vsock,port={port},socketURL={},listen",
        socket_path.display()
    )
}

pub(super) fn start_machine(
    paths: &MachinePaths,
    config: &mut MachineConfigRecord,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    ensure_machine_can_start(paths, config, state)?;
    converge_machine_image_contract(paths, config, state)?;
    validate_machine_bootstrap_contract(config)?;

    cleanup_runtime_artifacts(paths)?;
    let launch_plan = MachineLaunchPlan::build(paths, config, state)?;
    let startup_signals = StartupSignalMonitor::install()?;

    state.lifecycle = MachineLifecycle::Starting;
    state.manager = MachineManagerState::Launching;
    state.runtime = Some(launch_plan.runtime().clone());
    state.last_error = None;
    write_json_file(&paths.state_path, state)?;

    let ready_listener = bind_ready_listener(&paths.ready_socket_path)?;
    let _ignition_server = start_bootstrap_server(paths, config, &launch_plan)?;

    let mut gvproxy_child = None;
    if let Err(error) = pre_start_networking(
        paths,
        config,
        &launch_plan,
        &mut gvproxy_child,
        &startup_signals,
    ) {
        return handle_start_machine_error(
            paths,
            config,
            state,
            error,
            None,
            gvproxy_child.as_mut(),
        );
    }

    let mut krunkit_child = None;
    if let Err(error) = start_vm(config, &launch_plan, &mut krunkit_child) {
        return handle_start_machine_error(
            paths,
            config,
            state,
            error,
            krunkit_child.as_mut(),
            gvproxy_child.as_mut(),
        );
    }
    if let Err(error) = wait_for_machine_ready(
        config,
        &ready_listener,
        &mut krunkit_child,
        &mut gvproxy_child,
        &startup_signals,
    ) {
        return handle_start_machine_error(
            paths,
            config,
            state,
            error,
            krunkit_child.as_mut(),
            gvproxy_child.as_mut(),
        );
    }
    if let Err(error) = post_start_networking(paths, config, &mut gvproxy_child, &startup_signals) {
        return handle_start_machine_error(
            paths,
            config,
            state,
            error,
            krunkit_child.as_mut(),
            gvproxy_child.as_mut(),
        );
    }
    if let Err(error) = conduct_readiness_check(
        config,
        launch_plan.runtime().ssh_port,
        &mut krunkit_child,
        &mut gvproxy_child,
        &startup_signals,
    ) {
        return handle_start_machine_error(
            paths,
            config,
            state,
            error,
            krunkit_child.as_mut(),
            gvproxy_child.as_mut(),
        );
    }
    if let Err(error) = ensure_guest_machine_api_ready(
        paths,
        config,
        launch_plan.runtime().ssh_port,
        &mut krunkit_child,
        &mut gvproxy_child,
        &startup_signals,
    ) {
        return handle_start_machine_error(
            paths,
            config,
            state,
            error,
            krunkit_child.as_mut(),
            gvproxy_child.as_mut(),
        );
    }

    state.lifecycle = MachineLifecycle::Running;
    state.manager = MachineManagerState::Ready;
    state.last_error = None;
    write_json_file(&paths.state_path, state)?;
    Ok(())
}

fn converge_machine_image_contract(
    paths: &MachinePaths,
    config: &mut MachineConfigRecord,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    let desired_image_source = desired_machine_image_source(config);
    if config.guest.image_source != desired_image_source {
        config.guest.image_source = desired_image_source;
        write_json_file(&paths.config_path, config)?;
    }

    let desired_image = describe_machine_image_source(&config.guest.image_source);
    let Some(rebuild_reason) = machine_image_rebuild_reason(paths, state, &desired_image) else {
        return Ok(());
    };

    invalidate_materialized_machine_os(paths)?;
    *state = MachineStateRecord::rebuilt(rebuild_reason);
    write_json_file(&paths.state_path, state)?;
    Ok(())
}

fn machine_image_rebuild_reason(
    paths: &MachinePaths,
    state: &MachineStateRecord,
    desired_image: &str,
) -> Option<String> {
    match state
        .runtime
        .as_ref()
        .map(|runtime| runtime.machine_image_source.as_str())
        .filter(|recorded| !recorded.is_empty())
    {
        Some(recorded) if recorded != desired_image => Some(format!(
            "machine base image changed from '{}' to '{}'; boot artifacts were reset and will be recreated on the next start",
            recorded, desired_image
        )),
        Some(_) => None,
        None if paths.materialized_image_path.is_file()
            || paths.efi_variable_store_path.exists() =>
        {
            Some(format!(
                "machine boot artifacts existed without a recorded base-image identity for '{}'; boot artifacts were reset and will be recreated on the next start",
                desired_image
            ))
        }
        None => None,
    }
}

fn ensure_machine_can_start(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    state: &MachineStateRecord,
) -> Result<(), Error> {
    if matches!(
        state.lifecycle,
        MachineLifecycle::Starting | MachineLifecycle::Running
    ) {
        let exclusivity_note = if config.provider.requires_exclusive_active() {
            " and this provider requires one active machine at a time"
        } else {
            ""
        };
        return Err(Error::Conflict(format!(
            "machine '{}' is already {}{}",
            paths.name,
            state.lifecycle.as_str(),
            exclusivity_note
        )));
    }
    Ok(())
}

fn validate_machine_bootstrap_contract(config: &MachineConfigRecord) -> Result<(), Error> {
    if !requires_host_guest_neovex_sync(config) {
        return Ok(());
    }

    let identity_path = config.guest.ssh_identity_path.as_ref().ok_or_else(|| {
        Error::InvalidInput(format!(
            "machine '{}' uses the host-managed macOS machine-image contract and requires `--ssh-identity <path>` so neovex can stage the guest binary and validate the forwarded machine API",
            config.name
        ))
    })?;
    if !identity_path.is_file() {
        return Err(Error::InvalidInput(format!(
            "machine '{}' SSH identity does not exist at {}",
            config.name,
            identity_path.display()
        )));
    }

    Ok(())
}

fn requires_host_guest_neovex_sync(config: &MachineConfigRecord) -> bool {
    config.provider == super::MachineProvider::Krunkit
        && uses_host_managed_machine_image_contract(config)
}

fn ensure_guest_machine_api_ready(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    ssh_port: u16,
    krunkit_child: &mut Option<Child>,
    gvproxy_child: &mut Option<Child>,
    startup_signals: &StartupSignalMonitor,
) -> Result<(), Error> {
    if config.provider != super::MachineProvider::Krunkit
        || config.guest.ssh_identity_path.is_none()
    {
        return Ok(());
    }

    if requires_host_guest_neovex_sync(config) {
        sync_guest_neovex_binary(paths, config, ssh_port)?;
    }

    wait_for_machine_api_ready(
        paths,
        resolve_machine_api_ready_wait_timeout(),
        required_child(krunkit_child, "krunkit")?,
        required_child(gvproxy_child, "gvproxy")?,
        startup_signals,
    )
}

fn sync_guest_neovex_binary(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    ssh_port: u16,
) -> Result<(), Error> {
    let guest_binary = resolve_guest_neovex_binary(paths)?;
    let desired_hash = compute_sha256(&guest_binary)?;
    let current_hash = read_guest_neovex_hash(config, ssh_port)?;
    if current_hash.as_deref() != Some(desired_hash.as_str()) {
        stream_guest_file_over_ssh(
            config,
            ssh_port,
            &guest_binary,
            &format!(
                "set -eu; install_dir=\"{}\"; tmp_name=\".neovex.$$.tmp\"; sudo mkdir -p \"$install_dir\"; tmp_path=\"$install_dir/$tmp_name\"; cat | sudo tee \"$tmp_path\" >/dev/null; sudo chmod 0755 \"$tmp_path\"; if command -v restorecon >/dev/null 2>&1; then sudo restorecon \"$tmp_path\"; fi; sudo mv \"$tmp_path\" \"{}\"; if command -v restorecon >/dev/null 2>&1; then sudo restorecon \"{}\"; fi",
                Path::new(GUEST_NEOVEX_BIN)
                    .parent()
                    .expect("guest neovex binary path should have a parent")
                    .display(),
                GUEST_NEOVEX_BIN,
                GUEST_NEOVEX_BIN
            ),
        )?;
    }

    run_guest_ssh_shell_capture(config, ssh_port, &ensure_guest_neovex_socket_shell_script())?;
    Ok(())
}

fn ensure_guest_neovex_socket_shell_script() -> String {
    format!(
        "set -eu; sudo systemctl daemon-reload; sudo systemctl stop neovex.service neovex.socket >/dev/null 2>&1 || true; sudo systemctl reset-failed neovex.service neovex.socket >/dev/null 2>&1 || true; sudo rm -f \"{socket}\"; sudo systemctl enable neovex.socket >/dev/null 2>&1 || true; sudo systemctl start neovex.socket; sudo systemctl is-active neovex.socket >/dev/null; printf '%s' ok",
        socket = GUEST_NEOVEX_SOCKET
    )
}

fn read_guest_neovex_hash(
    config: &MachineConfigRecord,
    ssh_port: u16,
) -> Result<Option<String>, Error> {
    let output = run_guest_ssh_shell_capture(
        config,
        ssh_port,
        &format!(
            "if [ -x \"{path}\" ]; then set -- $(sha256sum \"{path}\"); printf '%s' \"$1\"; fi",
            path = GUEST_NEOVEX_BIN
        ),
    )?;
    let hash = output.trim();
    if hash.is_empty() {
        Ok(None)
    } else {
        Ok(Some(hash.to_owned()))
    }
}

fn resolve_guest_neovex_binary(paths: &MachinePaths) -> Result<PathBuf, Error> {
    if let Some(path) = env::var_os(GUEST_NEOVEX_BINARY_OVERRIDE_ENV).map(PathBuf::from) {
        if !path.is_file() {
            return Err(Error::InvalidInput(format!(
                "guest neovex binary override {} from ${GUEST_NEOVEX_BINARY_OVERRIDE_ENV} does not exist",
                path.display()
            )));
        }
        return Ok(path);
    }
    if let Some(path) = resolve_local_guest_neovex_binary()? {
        return Ok(path);
    }

    let cache_dir = paths.state_dir.join("guest-neovex");
    fs::create_dir_all(&cache_dir).map_err(|error| {
        Error::Internal(format!(
            "failed to create guest neovex cache directory {}: {error}",
            cache_dir.display()
        ))
    })?;

    let release_tag = super::current_machine_release_tag();
    let archive_name = guest_neovex_archive_name()?;
    let binary_path = cache_dir.join(format!(
        "{}-{}-neovex",
        release_tag,
        archive_name.trim_end_matches(".tar.gz")
    ));
    if binary_path.is_file() {
        return Ok(binary_path);
    }

    let archive_path = cache_dir.join(format!("{release_tag}-{archive_name}"));
    if !archive_path.is_file() {
        let download_url = guest_neovex_release_url(&release_tag, archive_name);
        download_guest_neovex_archive(&archive_path, &download_url)?;
    }
    extract_guest_neovex_archive(&archive_path, &binary_path)?;
    Ok(binary_path)
}

fn guest_neovex_archive_name() -> Result<&'static str, Error> {
    match env::consts::ARCH {
        "aarch64" | "arm64" => Ok(DEFAULT_GUEST_NEOVEX_BINARY_ARCHIVE_NAME_ARM64),
        "x86_64" => Ok(DEFAULT_GUEST_NEOVEX_BINARY_ARCHIVE_NAME_X86_64),
        arch => Err(Error::InvalidInput(format!(
            "unsupported macOS machine host architecture '{arch}' for guest neovex binary sync"
        ))),
    }
}

fn resolve_local_guest_neovex_binary() -> Result<Option<PathBuf>, Error> {
    let Some(workspace_root) = compiled_workspace_root() else {
        return Ok(None);
    };
    let target_triple = guest_neovex_target_triple()?;
    Ok(find_local_guest_neovex_binary_under(
        workspace_root,
        target_triple,
    ))
}

fn compiled_workspace_root() -> Option<&'static Path> {
    workspace_root_from_manifest_dir(Path::new(env!("CARGO_MANIFEST_DIR")))
}

fn workspace_root_from_manifest_dir(manifest_dir: &Path) -> Option<&Path> {
    manifest_dir.parent()?.parent()
}

fn guest_neovex_target_triple() -> Result<&'static str, Error> {
    match env::consts::ARCH {
        "aarch64" | "arm64" => Ok(DEFAULT_GUEST_NEOVEX_TARGET_TRIPLE_ARM64),
        "x86_64" => Ok(DEFAULT_GUEST_NEOVEX_TARGET_TRIPLE_X86_64),
        arch => Err(Error::InvalidInput(format!(
            "unsupported macOS machine host architecture '{arch}' for guest neovex binary sync"
        ))),
    }
}

fn find_local_guest_neovex_binary_under(
    workspace_root: &Path,
    target_triple: &str,
) -> Option<PathBuf> {
    ["release", "debug"]
        .into_iter()
        .map(|profile| {
            workspace_root
                .join("target")
                .join(target_triple)
                .join(profile)
                .join("neovex")
        })
        .find(|candidate| candidate.is_file())
}

fn guest_neovex_release_url(release_tag: &str, archive_name: &str) -> String {
    let base = env::var(GUEST_NEOVEX_RELEASE_BASE_URL_ENV)
        .unwrap_or_else(|_| DEFAULT_GUEST_NEOVEX_RELEASE_BASE_URL.to_owned());
    format!("{}/{}", base.trim_end_matches('/'), release_tag).to_owned() + "/" + archive_name
}

fn download_guest_neovex_archive(destination: &Path, url: &str) -> Result<(), Error> {
    let destination = destination.to_path_buf();
    let url = url.to_owned();
    run_blocking_in_thread("guest neovex archive download", move || {
        let parent = destination.parent().ok_or_else(|| {
            Error::Internal(format!(
                "failed to resolve parent directory for guest neovex archive {}",
                destination.display()
            ))
        })?;
        let mut temp = NamedTempFile::new_in(parent).map_err(|error| {
            Error::Internal(format!(
                "failed to create temporary guest neovex archive under {}: {error}",
                parent.display()
            ))
        })?;
        let client = BlockingClient::builder()
            .timeout(HTTP_IMAGE_TIMEOUT)
            .build()
            .map_err(|error| Error::Internal(format!("failed to build HTTP client: {error}")))?;
        let mut response = client
            .get(&url)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|error| {
                Error::InvalidInput(format!(
                    "failed to download guest neovex archive from {url}: {error}. To continue without the release asset, {LOCAL_GUEST_BINARY_HELP_TEXT} to a local Linux guest binary."
                ))
            })?;
        io::copy(&mut response, &mut temp).map_err(|error| {
            Error::Internal(format!(
                "failed to write guest neovex archive from {url} into {}: {error}",
                destination.display()
            ))
        })?;
        temp.flush().map_err(|error| {
            Error::Internal(format!(
                "failed to flush guest neovex archive from {url}: {error}"
            ))
        })?;
        temp.persist(&destination).map_err(|error| {
            Error::Internal(format!(
                "failed to persist guest neovex archive {}: {}",
                destination.display(),
                error.error
            ))
        })?;
        Ok(())
    })
}

fn extract_guest_neovex_archive(archive_path: &Path, output_path: &Path) -> Result<(), Error> {
    let parent = output_path.parent().ok_or_else(|| {
        Error::Internal(format!(
            "failed to resolve parent directory for guest neovex binary {}",
            output_path.display()
        ))
    })?;
    let extract_dir = tempdir_in(parent).map_err(|error| {
        Error::Internal(format!(
            "failed to create temporary extraction directory under {}: {error}",
            parent.display()
        ))
    })?;
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(archive_path)
        .arg("-C")
        .arg(extract_dir.path())
        .arg("neovex")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| {
            Error::Internal(format!(
                "failed to start tar while extracting guest neovex archive {}: {error}",
                archive_path.display()
            ))
        })?;
    if !status.success() {
        return Err(Error::Internal(format!(
            "tar failed while extracting guest neovex archive {} with status {status}",
            archive_path.display()
        )));
    }

    let extracted_binary = extract_dir.path().join("neovex");
    if !extracted_binary.is_file() {
        return Err(Error::Internal(format!(
            "guest neovex archive {} did not contain a top-level 'neovex' binary",
            archive_path.display()
        )));
    }

    let temp_output = NamedTempFile::new_in(parent).map_err(|error| {
        Error::Internal(format!(
            "failed to create temporary guest neovex binary under {}: {error}",
            parent.display()
        ))
    })?;
    fs::copy(&extracted_binary, temp_output.path()).map_err(|error| {
        Error::Internal(format!(
            "failed to stage extracted guest neovex binary {}: {error}",
            extracted_binary.display()
        ))
    })?;
    fs::set_permissions(temp_output.path(), fs::Permissions::from_mode(0o755)).map_err(
        |error| {
            Error::Internal(format!(
                "failed to mark extracted guest neovex binary {} executable: {error}",
                temp_output.path().display()
            ))
        },
    )?;
    temp_output.persist(output_path).map_err(|error| {
        Error::Internal(format!(
            "failed to persist guest neovex binary {}: {}",
            output_path.display(),
            error.error
        ))
    })?;
    Ok(())
}

fn wait_for_machine_api_ready(
    paths: &MachinePaths,
    timeout: Duration,
    krunkit_child: &mut Child,
    gvproxy_child: &mut Child,
    startup_signals: &StartupSignalMonitor,
) -> Result<(), Error> {
    let deadline = Instant::now() + timeout;
    let client = MachineApiClient::new(paths.api_socket_path.clone());
    loop {
        startup_signals.check()?;
        if let Some(status) = krunkit_child.try_wait().map_err(|error| {
            Error::Internal(format!("failed to poll krunkit process state: {error}"))
        })? {
            return Err(Error::Internal(format!(
                "krunkit exited before machine API readiness with status {status}"
            )));
        }
        if let Some(status) = gvproxy_child.try_wait().map_err(|error| {
            Error::Internal(format!("failed to poll gvproxy process state: {error}"))
        })? {
            return Err(Error::Internal(format!(
                "gvproxy exited before machine API readiness with status {status}"
            )));
        }

        let current_probe_error = if paths.api_socket_path.exists() {
            match client.health() {
                Ok(_) => match client.capabilities() {
                    Ok(_) => return Ok(()),
                    Err(error) => error.to_string(),
                },
                Err(error) => error.to_string(),
            }
        } else {
            format!(
                "forwarded machine API socket {} is not present yet",
                paths.api_socket_path.display()
            )
        };

        if Instant::now() >= deadline {
            return Err(Error::Internal(format!(
                "guest machine API readiness did not arrive within {} seconds{}",
                timeout.as_secs(),
                if current_probe_error.is_empty() {
                    String::new()
                } else {
                    format!(": {current_probe_error}")
                }
            )));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn start_bootstrap_server(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    launch_plan: &MachineLaunchPlan,
) -> Result<Option<thread::JoinHandle<()>>, Error> {
    if config.provider.bootstrap_mode() != MachineBootstrapMode::Ignition {
        return Ok(None);
    }
    match launch_plan.ignition_file_path.as_ref() {
        Some(path) => serve_ignition_file(&paths.ignition_socket_path, path).map(Some),
        None => Ok(None),
    }
}

fn pre_start_networking(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    launch_plan: &MachineLaunchPlan,
    gvproxy_child: &mut Option<Child>,
    startup_signals: &StartupSignalMonitor,
) -> Result<(), Error> {
    if config.provider.uses_provider_networking() {
        return Ok(());
    }

    let mut child = launch_plan.gvproxy_command.spawn()?;
    wait_for_path(
        &paths.gvproxy_socket_path,
        GVPROXY_SOCKET_WAIT_TIMEOUT,
        &mut child,
        startup_signals,
    )?;
    *gvproxy_child = Some(child);
    Ok(())
}

fn start_vm(
    config: &MachineConfigRecord,
    launch_plan: &MachineLaunchPlan,
    krunkit_child: &mut Option<Child>,
) -> Result<(), Error> {
    match config.provider {
        super::MachineProvider::Krunkit => {
            *krunkit_child = Some(launch_plan.krunkit_command.spawn()?);
            Ok(())
        }
        super::MachineProvider::Wsl2 => Err(Error::InvalidInput(
            "the WSL2 machine provider is not available on this host yet".to_owned(),
        )),
    }
}

fn wait_for_machine_ready(
    config: &MachineConfigRecord,
    ready_listener: &UnixListener,
    krunkit_child: &mut Option<Child>,
    gvproxy_child: &mut Option<Child>,
    startup_signals: &StartupSignalMonitor,
) -> Result<(), Error> {
    match config.provider.bootstrap_mode() {
        MachineBootstrapMode::Ignition => wait_for_ready(
            ready_listener,
            resolve_ready_wait_timeout(),
            required_child(krunkit_child, "krunkit")?,
            required_child(gvproxy_child, "gvproxy")?,
            startup_signals,
        ),
        MachineBootstrapMode::ShellScript => Ok(()),
    }
}

fn post_start_networking(
    _paths: &MachinePaths,
    config: &MachineConfigRecord,
    _gvproxy_child: &mut Option<Child>,
    _startup_signals: &StartupSignalMonitor,
) -> Result<(), Error> {
    if config.provider.uses_provider_networking() {
        // Future providers such as WSL own their own host networking startup
        // and will wire their post-start verification here.
        return Ok(());
    }

    // The current krunkit path launches gvproxy before VM boot, so there is no
    // additional post-start networking step beyond readiness checks.
    Ok(())
}

fn conduct_readiness_check(
    config: &MachineConfigRecord,
    ssh_port: u16,
    krunkit_child: &mut Option<Child>,
    gvproxy_child: &mut Option<Child>,
    startup_signals: &StartupSignalMonitor,
) -> Result<(), Error> {
    match config.provider {
        super::MachineProvider::Krunkit => wait_for_ssh_ready(
            config,
            ssh_port,
            resolve_ssh_ready_wait_timeout(),
            required_child(krunkit_child, "krunkit")?,
            required_child(gvproxy_child, "gvproxy")?,
            startup_signals,
        ),
        super::MachineProvider::Wsl2 => Err(Error::InvalidInput(
            "the WSL2 machine provider is not available on this host yet".to_owned(),
        )),
    }
}

fn required_child<'a>(child: &'a mut Option<Child>, label: &str) -> Result<&'a mut Child, Error> {
    child.as_mut().ok_or_else(|| {
        Error::Internal(format!(
            "machine startup phase expected a running {label} helper, but none was recorded"
        ))
    })
}

pub(super) fn stop_machine(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    if matches!(
        state.lifecycle,
        MachineLifecycle::Stopped | MachineLifecycle::Uninitialized
    ) {
        return Ok(());
    }

    let mut stop_errors = Vec::new();
    if let Err(error) = stop_provider_machine(paths, config, resolve_stop_wait_timeout()) {
        stop_errors.push(error.to_string());
    }

    if let Some(pid) = read_pid(&paths.krunkit_pid_path)?
        && pid_is_alive(pid)
    {
        stop_errors.push(format!(
            "provider stop completed but krunkit is still alive at pid {pid}"
        ));
    }
    if !config.provider.uses_provider_networking()
        && let Some(pid) = read_pid(&paths.gvproxy_pid_path)?
        && let Err(error) = stop_pid(pid, HARD_STOP_WAIT_TIMEOUT)
    {
        stop_errors.push(error.to_string());
    }

    cleanup_runtime_artifacts(paths)?;
    state.lifecycle = MachineLifecycle::Stopped;
    state.manager = if state.runtime.is_some() {
        MachineManagerState::HelpersResolved
    } else {
        MachineManagerState::Unconfigured
    };
    state.last_error = if stop_errors.is_empty() {
        None
    } else {
        Some(stop_errors.join("; "))
    };
    write_json_file(&paths.state_path, state)?;
    Ok(())
}

fn stop_provider_machine(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    timeout: Duration,
) -> Result<(), Error> {
    match config.provider {
        super::MachineProvider::Krunkit => stop_krunkit_machine(paths, timeout),
        super::MachineProvider::Wsl2 => Err(Error::InvalidInput(
            "the WSL2 machine provider is not available on this host yet".to_owned(),
        )),
    }
}

fn stop_krunkit_machine(paths: &MachinePaths, timeout: Duration) -> Result<(), Error> {
    let Some(pid) = read_pid(&paths.krunkit_pid_path)? else {
        return Ok(());
    };
    if !pid_is_alive(pid) {
        return Ok(());
    }

    if let Err(error) = request_krunkit_state_change(&paths.krunkit_endpoint_path, "Stop") {
        force_stop_pid(pid, HARD_STOP_WAIT_TIMEOUT).map_err(|kill_error| {
            Error::Internal(format!(
                "{error}; failed to recover by force-stopping krunkit pid {pid}: {kill_error}"
            ))
        })?;
        return Ok(());
    }
    if wait_for_pid_exit(pid, timeout)? {
        return Ok(());
    }

    if let Err(error) = request_krunkit_state_change(&paths.krunkit_endpoint_path, "HardStop") {
        force_stop_pid(pid, HARD_STOP_WAIT_TIMEOUT).map_err(|kill_error| {
            Error::Internal(format!(
                "{error}; failed to recover by force-stopping krunkit pid {pid}: {kill_error}"
            ))
        })?;
        return Ok(());
    }
    if wait_for_pid_exit(pid, HARD_STOP_WAIT_TIMEOUT)? {
        return Ok(());
    }

    force_stop_pid(pid, HARD_STOP_WAIT_TIMEOUT)
}

pub(super) fn release_machine_ssh_port(
    roots: &MachineRootLayout,
    machine_name: &str,
) -> Result<(), Error> {
    with_port_allocation_lock(roots, || {
        let mut allocation_state = load_machine_port_allocation_state(roots)?;
        if allocation_state
            .machine_ports
            .remove(machine_name)
            .is_some()
        {
            write_machine_port_allocation_state(roots, &allocation_state)?;
        }
        Ok(())
    })
}

pub(super) fn refresh_machine_state(
    paths: &MachinePaths,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    if !matches!(
        state.lifecycle,
        MachineLifecycle::Starting | MachineLifecycle::Running
    ) {
        return Ok(());
    }

    let krunkit_alive = read_pid(&paths.krunkit_pid_path)?
        .map(pid_is_alive)
        .unwrap_or(false);
    let gvproxy_alive = read_pid(&paths.gvproxy_pid_path)?
        .map(pid_is_alive)
        .unwrap_or(false);

    if krunkit_alive && gvproxy_alive {
        if state.lifecycle == MachineLifecycle::Starting && paths.ready_socket_path.exists() {
            state.manager = MachineManagerState::Launching;
        }
        return Ok(());
    }

    state.lifecycle = MachineLifecycle::Failed;
    state.manager = MachineManagerState::Stale;
    state.last_error = Some(format!(
        "machine runtime is stale: krunkit_alive={krunkit_alive} gvproxy_alive={gvproxy_alive}"
    ));
    write_json_file(&paths.state_path, state)
}

fn handle_start_machine_error(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    state: &mut MachineStateRecord,
    error: Error,
    mut krunkit_child: Option<&mut Child>,
    mut gvproxy_child: Option<&mut Child>,
) -> Result<(), Error> {
    if let Some(child) = krunkit_child.as_mut() {
        let _ = cleanup_process(child);
    }
    if let Some(child) = gvproxy_child.as_mut() {
        let _ = cleanup_process(child);
    }

    if matches!(error, Error::Cancelled) {
        return finalize_interrupted_start(paths, state);
    }

    let error = annotate_machine_start_error(paths, config, error);
    state.lifecycle = MachineLifecycle::Failed;
    state.manager = MachineManagerState::Failed;
    state.last_error = Some(error.to_string());
    write_json_file(&paths.state_path, state)?;
    Err(error)
}

fn annotate_machine_start_error(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    error: Error,
) -> Error {
    let Some(hint) = detect_guest_bootstrap_hint(paths, config, &error) else {
        return error;
    };

    match error {
        Error::Internal(message) => Error::Internal(format!("{message}; {hint}")),
        other => other,
    }
}

fn detect_guest_bootstrap_hint(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    error: &Error,
) -> Option<&'static str> {
    if config.provider != super::MachineProvider::Krunkit
        || config.provider.bootstrap_mode() != MachineBootstrapMode::Ignition
    {
        return None;
    }

    let error_text = error.to_string();
    let startup_gate_failed = error_text.contains("gvproxy exited before machine readiness")
        || error_text.contains("gvproxy exited before SSH readiness")
        || error_text.contains("machine ready signal did not arrive")
        || error_text.contains("guest SSH readiness did not arrive");
    if !startup_gate_failed {
        return None;
    }

    let console_log = fs::read_to_string(&paths.machine_log_path).ok()?;
    if !console_log.to_ascii_lowercase().contains("login:") {
        return None;
    }

    Some(
        "guest reached a console login prompt without consuming the first-boot ignition payload. On macOS, Neovex guest images must stay Podman-aligned (Fedora CoreOS/libkrun ignition path); generic fedora-bootc raw images are not a supported substitute",
    )
}

fn finalize_interrupted_start(
    paths: &MachinePaths,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    cleanup_runtime_artifacts(paths)?;
    state.lifecycle = MachineLifecycle::Stopped;
    state.manager = if state.runtime.is_some() {
        MachineManagerState::HelpersResolved
    } else {
        MachineManagerState::Unconfigured
    };
    state.last_error = None;
    write_json_file(&paths.state_path, state)?;
    Err(Error::Cancelled)
}

pub(super) fn build_ssh_command(
    config: &MachineConfigRecord,
    state: &MachineStateRecord,
) -> Result<Command, Error> {
    if state.lifecycle != MachineLifecycle::Running {
        return Err(Error::Conflict(format!(
            "machine '{}' is {} and cannot accept SSH",
            config.name,
            state.lifecycle.as_str()
        )));
    }

    let runtime = state.runtime.as_ref().ok_or_else(|| {
        Error::InvalidInput(format!(
            "machine '{}' has no recorded runtime; start it first",
            config.name
        ))
    })?;
    let identity_path = config.guest.ssh_identity_path.as_ref().ok_or_else(|| {
        Error::InvalidInput(format!(
            "machine '{}' has no SSH identity configured; re-run `neovex machine init --ssh-identity <path>` or wait for MAC4 guest bootstrap",
            config.name
        ))
    })?;
    if !identity_path.is_file() {
        return Err(Error::InvalidInput(format!(
            "machine '{}' SSH identity does not exist at {}",
            config.name,
            identity_path.display()
        )));
    }

    let mut command = Command::new("ssh");
    append_localhost_ssh_options(
        &mut command,
        identity_path,
        runtime.ssh_port,
        &config.guest.ssh_user,
    );
    Ok(command)
}

fn append_localhost_ssh_options(
    command: &mut Command,
    identity_path: &Path,
    ssh_port: u16,
    ssh_user: &str,
) {
    command
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("IdentitiesOnly=yes")
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("UserKnownHostsFile=/dev/null")
        .arg("-o")
        .arg("CheckHostIP=no")
        .arg("-o")
        .arg("LogLevel=ERROR")
        .arg("-o")
        .arg("SetEnv=LC_ALL=")
        .arg("-i")
        .arg(identity_path)
        .arg("-p")
        .arg(ssh_port.to_string())
        .arg(format!("{ssh_user}@127.0.0.1"));
}

fn build_localhost_ssh_command(
    config: &MachineConfigRecord,
    ssh_port: u16,
) -> Result<Command, Error> {
    let identity_path = config.guest.ssh_identity_path.as_ref().ok_or_else(|| {
        Error::InvalidInput(format!(
            "machine '{}' has no SSH identity configured",
            config.name
        ))
    })?;
    if !identity_path.is_file() {
        return Err(Error::InvalidInput(format!(
            "machine '{}' SSH identity does not exist at {}",
            config.name,
            identity_path.display()
        )));
    }

    let mut command = Command::new("ssh");
    append_localhost_ssh_options(
        &mut command,
        identity_path,
        ssh_port,
        &config.guest.ssh_user,
    );
    Ok(command)
}

fn run_guest_ssh_shell_capture(
    config: &MachineConfigRecord,
    ssh_port: u16,
    remote_shell_script: &str,
) -> Result<String, Error> {
    let output = build_localhost_ssh_command(config, ssh_port)?
        .arg(remote_shell_command(remote_shell_script))
        .stdin(Stdio::null())
        .output()
        .map_err(|error| {
            Error::Internal(format!(
                "failed to run guest SSH command on localhost:{ssh_port}: {error}"
            ))
        })?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }

    Err(Error::Internal(format!(
        "guest SSH command failed on localhost:{ssh_port} with status {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

fn stream_guest_file_over_ssh(
    config: &MachineConfigRecord,
    ssh_port: u16,
    source_path: &Path,
    remote_shell_script: &str,
) -> Result<(), Error> {
    let input = fs::File::open(source_path).map_err(|error| {
        Error::Internal(format!(
            "failed to open guest neovex binary {}: {error}",
            source_path.display()
        ))
    })?;
    let status = build_localhost_ssh_command(config, ssh_port)?
        .arg(remote_shell_command(remote_shell_script))
        .stdin(Stdio::from(input))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| {
            Error::Internal(format!(
                "failed to stream guest neovex binary over SSH to localhost:{ssh_port}: {error}"
            ))
        })?;
    if status.success() {
        return Ok(());
    }

    Err(Error::Internal(format!(
        "guest neovex binary sync failed on localhost:{ssh_port} with status {status}"
    )))
}

fn remote_shell_command(script: &str) -> String {
    format!("sh -lc {}", shell_single_quote(script))
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn bind_ready_listener(path: &Path) -> Result<UnixListener, Error> {
    remove_file_if_exists(path)?;
    let listener = UnixListener::bind(path).map_err(|error| {
        Error::Internal(format!(
            "failed to bind machine ready socket {}: {error}",
            path.display()
        ))
    })?;
    listener.set_nonblocking(true).map_err(|error| {
        Error::Internal(format!(
            "failed to configure machine ready socket {}: {error}",
            path.display()
        ))
    })?;
    Ok(listener)
}

fn serve_ignition_file(
    socket_path: &Path,
    ignition_path: &Path,
) -> Result<thread::JoinHandle<()>, Error> {
    remove_file_if_exists(socket_path)?;
    let bytes = Arc::new(fs::read(ignition_path).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to read ignition file {}: {error}",
            ignition_path.display()
        ))
    })?);
    let listener = UnixListener::bind(socket_path).map_err(|error| {
        Error::Internal(format!(
            "failed to bind ignition socket {}: {error}",
            socket_path.display()
        ))
    })?;
    listener.set_nonblocking(true).map_err(|error| {
        Error::Internal(format!(
            "failed to configure ignition socket {} as non-blocking: {error}",
            socket_path.display()
        ))
    })?;
    let router = Router::new()
        .route("/", get(machine_ignition_payload))
        .with_state(bytes);
    Ok(thread::spawn(move || {
        // The machine start path is synchronous, so the ignition helper needs
        // its own Tokio runtime to serve Podman-style HTTP over the Unix socket.
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        runtime.block_on(async move {
            let Ok(listener) = tokio::net::UnixListener::from_std(listener) else {
                return;
            };
            let _ = axum::serve(listener, router).await;
        });
    }))
}

async fn machine_ignition_payload(AxumState(bytes): AxumState<Arc<Vec<u8>>>) -> Vec<u8> {
    bytes.as_ref().clone()
}

fn wait_for_ready(
    listener: &UnixListener,
    timeout: Duration,
    krunkit_child: &mut Child,
    gvproxy_child: &mut Child,
    startup_signals: &StartupSignalMonitor,
) -> Result<(), Error> {
    let deadline = Instant::now() + timeout;
    loop {
        startup_signals.check()?;
        if let Some(status) = krunkit_child.try_wait().map_err(|error| {
            Error::Internal(format!("failed to poll krunkit process state: {error}"))
        })? {
            return Err(Error::Internal(format!(
                "krunkit exited before machine readiness with status {status}"
            )));
        }
        if let Some(status) = gvproxy_child.try_wait().map_err(|error| {
            Error::Internal(format!("failed to poll gvproxy process state: {error}"))
        })? {
            return Err(Error::Internal(format!(
                "gvproxy exited before machine readiness with status {status}"
            )));
        }

        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut buffer = [0u8; 32];
                let _ = stream.read(&mut buffer);
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => {
                return Err(Error::Internal(format!(
                    "failed while waiting for machine ready signal: {error}"
                )));
            }
        }

        if Instant::now() >= deadline {
            return Err(Error::Internal(format!(
                "machine ready signal did not arrive within {} seconds",
                timeout.as_secs()
            )));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_ssh_ready(
    config: &MachineConfigRecord,
    ssh_port: u16,
    timeout: Duration,
    krunkit_child: &mut Child,
    gvproxy_child: &mut Child,
    startup_signals: &StartupSignalMonitor,
) -> Result<(), Error> {
    // Mirror Podman's macOS machine layering: the ready signal alone is not
    // enough to prove host reachability, so only declare the machine started
    // once localhost SSH is actually up too.
    let deadline = Instant::now() + timeout;
    let mut last_probe_error: Option<String>;
    loop {
        startup_signals.check()?;
        if let Some(status) = krunkit_child.try_wait().map_err(|error| {
            Error::Internal(format!("failed to poll krunkit process state: {error}"))
        })? {
            return Err(Error::Internal(format!(
                "krunkit exited before SSH readiness with status {status}"
            )));
        }
        if let Some(status) = gvproxy_child.try_wait().map_err(|error| {
            Error::Internal(format!("failed to poll gvproxy process state: {error}"))
        })? {
            return Err(Error::Internal(format!(
                "gvproxy exited before SSH readiness with status {status}"
            )));
        }

        if ssh_port_is_listening(ssh_port) {
            if config.guest.ssh_identity_path.is_none() {
                return Ok(());
            }
            match run_silent_ssh_probe(config, ssh_port) {
                Ok(()) => return Ok(()),
                Err(error) => last_probe_error = Some(error.to_string()),
            }
        } else {
            last_probe_error = Some(format!(
                "guest SSH port {ssh_port} is not listening on localhost yet"
            ));
        }

        if Instant::now() >= deadline {
            return Err(Error::Internal(format!(
                "guest SSH readiness did not arrive within {} seconds{}",
                timeout.as_secs(),
                last_probe_error
                    .as_deref()
                    .map(|error| format!(": {error}"))
                    .unwrap_or_default()
            )));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn ssh_port_is_listening(ssh_port: u16) -> bool {
    TcpStream::connect_timeout(
        &format!("127.0.0.1:{ssh_port}")
            .parse()
            .expect("ssh localhost socket address should parse"),
        Duration::from_millis(100),
    )
    .map(|stream| {
        let _ = stream.shutdown(std::net::Shutdown::Both);
    })
    .is_ok()
}

fn run_silent_ssh_probe(config: &MachineConfigRecord, ssh_port: u16) -> Result<(), Error> {
    let identity_path = config.guest.ssh_identity_path.as_ref().ok_or_else(|| {
        Error::InvalidInput(format!(
            "machine '{}' has no SSH identity configured",
            config.name
        ))
    })?;
    let mut command = Command::new("ssh");
    append_localhost_ssh_options(
        &mut command,
        identity_path,
        ssh_port,
        &config.guest.ssh_user,
    );
    let status = command
        .arg("true")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| {
            Error::Internal(format!(
                "failed to run guest SSH readiness probe on localhost:{ssh_port}: {error}"
            ))
        })?;
    if status.success() {
        return Ok(());
    }
    Err(Error::Internal(format!(
        "guest SSH readiness probe failed on localhost:{ssh_port} with status {status}"
    )))
}

fn wait_for_path(
    path: &Path,
    timeout: Duration,
    child: &mut Child,
    startup_signals: &StartupSignalMonitor,
) -> Result<(), Error> {
    let deadline = Instant::now() + timeout;
    loop {
        startup_signals.check()?;
        if path.exists() {
            return Ok(());
        }
        if let Some(status) = child.try_wait().map_err(|error| {
            Error::Internal(format!(
                "failed to poll process while waiting for {}: {error}",
                path.display()
            ))
        })? {
            return Err(Error::Internal(format!(
                "process exited before {} appeared with status {status}",
                path.display()
            )));
        }
        if Instant::now() >= deadline {
            return Err(Error::Internal(format!(
                "timed out waiting for {}",
                path.display()
            )));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn request_krunkit_state_change(endpoint_path: &Path, state: &str) -> Result<(), Error> {
    if !endpoint_path.exists() {
        return Ok(());
    }

    let body = format!("{{\"state\":\"{state}\"}}");
    let mut stream = UnixStream::connect(endpoint_path).map_err(|error| {
        Error::Internal(format!(
            "failed to connect to krunkit control socket {}: {error}",
            endpoint_path.display()
        ))
    })?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| {
            Error::Internal(format!(
                "failed to configure krunkit control socket timeout {}: {error}",
                endpoint_path.display()
            ))
        })?;
    stream
        .write_all(
            format!(
                "POST /vm/state HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            )
            .as_bytes(),
        )
        .map_err(|error| {
            Error::Internal(format!(
                "failed to send krunkit state-change request {}: {error}",
                endpoint_path.display()
            ))
        })?;
    let mut response = String::new();
    stream.read_to_string(&mut response).map_err(|error| {
        Error::Internal(format!(
            "failed to read krunkit state-change response {}: {error}",
            endpoint_path.display()
        ))
    })?;
    if response.contains("200 OK") {
        return Ok(());
    }
    Err(Error::Internal(format!(
        "krunkit {state} request did not return 200 OK: {}",
        response.lines().next().unwrap_or("<empty-response>")
    )))
}

fn wait_for_pid_exit(pid: i32, timeout: Duration) -> Result<bool, Error> {
    if !pid_is_alive(pid) {
        return Ok(true);
    }
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !pid_is_alive(pid) {
            return Ok(true);
        }
        thread::sleep(POLL_INTERVAL);
    }
    Ok(!pid_is_alive(pid))
}

fn force_stop_pid(pid: i32, timeout: Duration) -> Result<(), Error> {
    if !pid_is_alive(pid) {
        return Ok(());
    }
    send_signal(pid, SIGKILL)?;
    if wait_for_pid_exit(pid, timeout)? {
        return Ok(());
    }
    Err(Error::Internal(format!(
        "process {pid} did not stop after provider hard-stop and SIGKILL"
    )))
}

fn stop_pid(pid: i32, timeout: Duration) -> Result<(), Error> {
    if !pid_is_alive(pid) {
        return Ok(());
    }
    send_signal(pid, SIGTERM)?;
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !pid_is_alive(pid) {
            return Ok(());
        }
        thread::sleep(POLL_INTERVAL);
    }

    send_signal(pid, SIGKILL)?;
    let kill_deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < kill_deadline {
        if !pid_is_alive(pid) {
            return Ok(());
        }
        thread::sleep(POLL_INTERVAL);
    }

    Err(Error::Internal(format!(
        "process {pid} did not stop after SIGTERM and SIGKILL"
    )))
}

fn cleanup_process(child: &mut Child) -> Result<(), Error> {
    match child.try_wait() {
        Ok(Some(_)) => Ok(()),
        Ok(None) => {
            child.kill().map_err(|error| {
                Error::Internal(format!(
                    "failed to terminate child process {}: {error}",
                    child.id()
                ))
            })?;
            child.wait().map(|_| ()).map_err(|error| {
                Error::Internal(format!(
                    "failed to reap child process {}: {error}",
                    child.id()
                ))
            })
        }
        Err(error) => Err(Error::Internal(format!(
            "failed to poll child process {}: {error}",
            child.id()
        ))),
    }
}

fn cleanup_runtime_artifacts(paths: &MachinePaths) -> Result<(), Error> {
    for path in [
        &paths.ready_socket_path,
        &paths.ignition_socket_path,
        &paths.api_socket_path,
        &paths.gvproxy_socket_path,
        &paths.krunkit_endpoint_path,
        &paths.gvproxy_pid_path,
        &paths.krunkit_pid_path,
    ] {
        remove_file_if_exists(path)?;
    }
    for path in [
        &paths.machine_log_path,
        &paths.krunkit_log_path,
        &paths.gvproxy_log_path,
    ] {
        truncate_file(path)?;
    }
    Ok(())
}

fn truncate_file(path: &Path) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            Error::Internal(format!("failed to create {}: {error}", parent.display()))
        })?;
    }
    fs::write(path, [])
        .map_err(|error| Error::Internal(format!("failed to truncate {}: {error}", path.display())))
}

fn remove_file_if_exists(path: &Path) -> Result<(), Error> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Internal(format!(
            "failed to remove {}: {error}",
            path.display()
        ))),
    }
}

fn read_pid(path: &Path) -> Result<Option<i32>, Error> {
    match fs::read_to_string(path) {
        Ok(value) => value.trim().parse::<i32>().map(Some).map_err(|error| {
            Error::Internal(format!(
                "failed to parse pid file {}: {error}",
                path.display()
            ))
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::Internal(format!(
            "failed to read pid file {}: {error}",
            path.display()
        ))),
    }
}

fn send_signal(pid: i32, signal: i32) -> Result<(), Error> {
    let rc = unsafe { kill(pid, signal) };
    if rc == 0 || !pid_is_alive(pid) {
        return Ok(());
    }
    Err(Error::Internal(format!(
        "failed to send signal {signal} to process {pid}: {}",
        io::Error::last_os_error()
    )))
}

fn pid_is_alive(pid: i32) -> bool {
    let rc = unsafe { kill(pid, 0) };
    if rc == 0 {
        return true;
    }
    io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn resolve_helper_binary(
    env_name: &str,
    command_name: &str,
    preferred_candidates: &[PathBuf],
    fallbacks: &[PathBuf],
) -> Result<PathBuf, Error> {
    if let Some(path) = std::env::var_os(env_name) {
        return resolve_existing_file(PathBuf::from(path), env_name);
    }
    if let Some(path) = helper_binary_dir_candidate(command_name) {
        return Ok(path);
    }
    for candidate in preferred_candidates {
        if candidate.is_file() {
            return Ok(candidate.clone());
        }
    }
    for fallback in fallbacks {
        if fallback.is_file() {
            return Ok(fallback.clone());
        }
    }
    Err(Error::InvalidInput(format!(
        "required helper '{command_name}' was not found; set {env_name}, set {HELPER_BINARY_DIR_ENV}, or install it in a supported packaged or Homebrew helper directory"
    )))
}

fn helper_binary_dir_candidate(command_name: &str) -> Option<PathBuf> {
    let helper_dir = std::env::var_os(HELPER_BINARY_DIR_ENV)?;
    let candidate = PathBuf::from(helper_dir).join(command_name);
    candidate.is_file().then_some(candidate)
}

fn known_helper_candidates(helper_name: &str) -> Vec<PathBuf> {
    PODMAN_DARWIN_HELPER_DIRECTORIES
        .iter()
        .map(|directory| PathBuf::from(directory).join(helper_name))
        .collect()
}

fn bundled_helper_candidates(helper_name: &str) -> Vec<PathBuf> {
    let Ok(current_exe) = std::env::current_exe() else {
        return Vec::new();
    };

    let mut candidates = bundled_helper_candidates_for_executable(&current_exe, helper_name);
    if let Ok(canonical_exe) = current_exe.canonicalize() {
        for candidate in bundled_helper_candidates_for_executable(&canonical_exe, helper_name) {
            push_unique_path(&mut candidates, candidate);
        }
    }
    candidates
}

fn bundled_helper_candidates_for_executable(
    executable_path: &Path,
    helper_name: &str,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let Some(executable_dir) = executable_path.parent() else {
        return candidates;
    };

    push_unique_path(
        &mut candidates,
        executable_dir.join("libexec").join(helper_name),
    );
    if executable_dir.file_name().and_then(|value| value.to_str()) == Some("bin")
        && let Some(prefix_dir) = executable_dir.parent()
    {
        push_unique_path(
            &mut candidates,
            prefix_dir.join("libexec").join(helper_name),
        );
    }
    candidates
}

fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.contains(&candidate) {
        paths.push(candidate);
    }
}

fn resolve_existing_file(path: PathBuf, env_name: &str) -> Result<PathBuf, Error> {
    if path.is_file() {
        return Ok(path);
    }
    Err(Error::InvalidInput(format!(
        "{env_name} points to {}, but that file does not exist",
        path.display()
    )))
}

#[cfg(test)]
fn helper_env_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

#[cfg(test)]
pub(crate) struct MachineHelperEnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous_krunkit: Option<std::ffi::OsString>,
    previous_gvproxy: Option<std::ffi::OsString>,
    previous_helper_dir: Option<std::ffi::OsString>,
    previous_path: Option<std::ffi::OsString>,
}

#[cfg(test)]
impl MachineHelperEnvGuard {
    pub(crate) fn install_stub_binaries(dir: &Path) -> Self {
        let krunkit_path = dir.join("krunkit");
        let gvproxy_path = dir.join("gvproxy");
        write_helper_stub(&krunkit_path, "krunkit");
        write_helper_stub(&gvproxy_path, "gvproxy");
        Self::set_paths(&krunkit_path, &gvproxy_path)
    }

    pub(crate) fn set_paths(krunkit_path: &Path, gvproxy_path: &Path) -> Self {
        let lock = helper_env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous_krunkit = std::env::var_os(KRUNKIT_ENV);
        let previous_gvproxy = std::env::var_os(GVPROXY_ENV);
        let previous_helper_dir = std::env::var_os(HELPER_BINARY_DIR_ENV);
        let previous_path = std::env::var_os("PATH");
        unsafe {
            std::env::set_var(KRUNKIT_ENV, krunkit_path);
            std::env::set_var(GVPROXY_ENV, gvproxy_path);
        }
        Self {
            _lock: lock,
            previous_krunkit,
            previous_gvproxy,
            previous_helper_dir,
            previous_path,
        }
    }

    pub(crate) fn with_helper_binary_dir(dir: &Path) -> Self {
        let lock = helper_env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous_krunkit = std::env::var_os(KRUNKIT_ENV);
        let previous_gvproxy = std::env::var_os(GVPROXY_ENV);
        let previous_helper_dir = std::env::var_os(HELPER_BINARY_DIR_ENV);
        let previous_path = std::env::var_os("PATH");
        unsafe {
            std::env::remove_var(KRUNKIT_ENV);
            std::env::remove_var(GVPROXY_ENV);
            std::env::set_var(HELPER_BINARY_DIR_ENV, dir);
        }
        Self {
            _lock: lock,
            previous_krunkit,
            previous_gvproxy,
            previous_helper_dir,
            previous_path,
        }
    }

    pub(crate) fn with_path_only(dir: &Path) -> Self {
        let lock = helper_env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous_krunkit = std::env::var_os(KRUNKIT_ENV);
        let previous_gvproxy = std::env::var_os(GVPROXY_ENV);
        let previous_helper_dir = std::env::var_os(HELPER_BINARY_DIR_ENV);
        let previous_path = std::env::var_os("PATH");
        unsafe {
            std::env::remove_var(KRUNKIT_ENV);
            std::env::remove_var(GVPROXY_ENV);
            std::env::remove_var(HELPER_BINARY_DIR_ENV);
            std::env::set_var("PATH", dir);
        }
        Self {
            _lock: lock,
            previous_krunkit,
            previous_gvproxy,
            previous_helper_dir,
            previous_path,
        }
    }
}

#[cfg(test)]
impl Drop for MachineHelperEnvGuard {
    fn drop(&mut self) {
        match &self.previous_krunkit {
            Some(path) => unsafe { std::env::set_var(KRUNKIT_ENV, path) },
            None => unsafe { std::env::remove_var(KRUNKIT_ENV) },
        }
        match &self.previous_gvproxy {
            Some(path) => unsafe { std::env::set_var(GVPROXY_ENV, path) },
            None => unsafe { std::env::remove_var(GVPROXY_ENV) },
        }
        match &self.previous_helper_dir {
            Some(path) => unsafe { std::env::set_var(HELPER_BINARY_DIR_ENV, path) },
            None => unsafe { std::env::remove_var(HELPER_BINARY_DIR_ENV) },
        }
        match &self.previous_path {
            Some(path) => unsafe { std::env::set_var("PATH", path) },
            None => unsafe { std::env::remove_var("PATH") },
        }
    }
}

#[cfg(test)]
fn write_helper_stub(path: &Path, _helper_name: &str) {
    crate::test_support::write_executable_stub(path, "#!/bin/sh\n");
}

fn resolve_bootable_image_path(
    paths: &MachinePaths,
    image_source: &MachineImageSource,
    provider: super::MachineProvider,
) -> Result<PathBuf, Error> {
    let image_format = provider.image_format();
    ensure_image_materialization_supported(image_format)?;
    match image_source {
        MachineImageSource::LocalDisk { path } => {
            if !path.is_file() {
                return Err(Error::InvalidInput(format!(
                    "machine guest image {} does not exist",
                    path.display()
                )));
            }
            Ok(path.clone())
        }
        MachineImageSource::OciReference { reference } => {
            if paths.materialized_image_path.is_file() {
                return Ok(paths.materialized_image_path.clone());
            }
            materialize_oci_image(paths, reference, provider)
        }
        MachineImageSource::HttpUrl { url } => {
            if paths.materialized_image_path.is_file() {
                return Ok(paths.materialized_image_path.clone());
            }
            materialize_http_image(paths, url)
        }
    }
}

fn ensure_image_materialization_supported(image_format: MachineImageFormat) -> Result<(), Error> {
    match image_format {
        MachineImageFormat::Raw => Ok(()),
        MachineImageFormat::Tar => Err(Error::InvalidInput(
            "the current machine manager can only materialize raw-disk guest images".to_owned(),
        )),
    }
}

fn materialize_http_image(paths: &MachinePaths, url: &str) -> Result<PathBuf, Error> {
    fs::create_dir_all(&paths.image_cache_dir).map_err(|error| {
        Error::Internal(format!(
            "failed to create machine image cache directory {}: {error}",
            paths.image_cache_dir.display()
        ))
    })?;

    let image_cache_dir = paths.image_cache_dir.clone();
    let url = url.to_owned();
    let download_url = url.clone();
    let download = run_blocking_in_thread("machine HTTP image download", move || {
        let download = NamedTempFile::new_in(&image_cache_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to allocate temporary download file under {}: {error}",
                image_cache_dir.display()
            ))
        })?;
        let client = BlockingClient::builder()
            .timeout(HTTP_IMAGE_TIMEOUT)
            .build()
            .map_err(|error| Error::Internal(format!("failed to build HTTP client: {error}")))?;
        let mut response = client
            .get(&download_url)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|error| {
                Error::InvalidInput(format!(
                    "failed to download machine guest image from {download_url}: {error}"
                ))
            })?;

        let mut writer = download.reopen().map_err(|error| {
            Error::Internal(format!(
                "failed to reopen temporary download file under {}: {error}",
                image_cache_dir.display()
            ))
        })?;
        io::copy(&mut response, &mut writer).map_err(|error| {
            Error::Internal(format!(
                "failed to write downloaded machine image from {download_url} into {}: {error}",
                image_cache_dir.display()
            ))
        })?;
        writer.flush().map_err(|error| {
            Error::Internal(format!(
                "failed to flush downloaded machine image for {download_url}: {error}"
            ))
        })?;
        drop(writer);
        Ok(download)
    })?;

    let temp_output = NamedTempFile::new_in(&paths.image_cache_dir).map_err(|error| {
        Error::Internal(format!(
            "failed to allocate temporary materialization file under {}: {error}",
            paths.image_cache_dir.display()
        ))
    })?;

    if url.ends_with(".gz") {
        let input = download.reopen().map_err(|error| {
            Error::Internal(format!(
                "failed to reopen temporary download file for gzip decode: {error}"
            ))
        })?;
        let mut decoder = GzDecoder::new(BufReader::new(input));
        let mut output = temp_output.reopen().map_err(|error| {
            Error::Internal(format!(
                "failed to reopen temporary materialization file for gzip decode: {error}"
            ))
        })?;
        io::copy(&mut decoder, &mut output).map_err(|error| {
            Error::Internal(format!(
                "failed to decompress gzip machine image from {url}: {error}"
            ))
        })?;
        output.flush().map_err(|error| {
            Error::Internal(format!(
                "failed to flush decompressed machine image for {url}: {error}"
            ))
        })?;
    } else {
        fs::copy(download.path(), temp_output.path()).map_err(|error| {
            Error::Internal(format!(
                "failed to stage downloaded machine image from {url}: {error}"
            ))
        })?;
    }

    temp_output
        .persist(&paths.materialized_image_path)
        .map_err(|error| {
            Error::Internal(format!(
                "failed to persist machine image from {url} into {}: {}",
                paths.materialized_image_path.display(),
                error.error
            ))
        })?;

    Ok(paths.materialized_image_path.clone())
}

fn materialize_oci_image(
    paths: &MachinePaths,
    reference: &str,
    provider: super::MachineProvider,
) -> Result<PathBuf, Error> {
    fs::create_dir_all(&paths.image_cache_dir).map_err(|error| {
        Error::Internal(format!(
            "failed to create machine image cache directory {}: {error}",
            paths.image_cache_dir.display()
        ))
    })?;

    let cache_dir = paths.image_cache_dir.clone();
    let reference = reference.to_owned();
    let source_label = format!("published OCI artifact '{reference}'");
    let reference_for_pull = reference.clone();
    let cached_blob_path = run_async_in_thread(move || async move {
        pull_oci_artifact_to_cache(cache_dir, reference_for_pull, provider).await
    })?;

    materialize_cached_disk(
        &cached_blob_path,
        &paths.materialized_image_path,
        &source_label,
    )?;
    Ok(paths.materialized_image_path.clone())
}

async fn pull_oci_artifact_to_cache(
    image_cache_dir: PathBuf,
    reference: String,
    provider: super::MachineProvider,
) -> Result<PathBuf, Error> {
    let stripped_reference = strip_docker_reference_prefix(&reference);
    let registry_reference = Reference::try_from(stripped_reference.as_str()).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to parse machine guest OCI reference '{reference}': {error}"
        ))
    })?;
    let client = build_oci_client(&stripped_reference)?;
    let auth = RegistryAuth::Anonymous;
    let accepted_media_types = vec![
        OCI_IMAGE_INDEX_MEDIA_TYPE,
        IMAGE_MANIFEST_LIST_MEDIA_TYPE,
        OCI_IMAGE_MEDIA_TYPE,
        IMAGE_MANIFEST_MEDIA_TYPE,
    ];
    let (top_manifest_bytes, _) = client
        .pull_manifest_raw(&registry_reference, &auth, &accepted_media_types)
        .await
        .map_err(|error| {
            Error::InvalidInput(format!(
                "failed to resolve machine guest OCI reference '{reference}': {error}"
            ))
        })?;

    let selected_artifact =
        select_oci_artifact_layer(&reference, &top_manifest_bytes, &client, &auth, provider)
            .await?;
    let cache_path = image_cache_dir.join(cached_oci_blob_file_name(&selected_artifact.layer));
    if cache_path.is_file() {
        return Ok(cache_path);
    }

    let download_path = image_cache_dir.join(format!(
        "{}.download",
        digest_hex(&selected_artifact.layer.digest)?
    ));
    if download_path.exists() {
        fs::remove_file(&download_path).map_err(|error| {
            Error::Internal(format!(
                "failed to remove stale machine image download {}: {error}",
                download_path.display()
            ))
        })?;
    }

    let mut output = tokio::fs::File::create(&download_path)
        .await
        .map_err(|error| {
            Error::Internal(format!(
                "failed to create temporary machine image download {}: {error}",
                download_path.display()
            ))
        })?;
    let layer = to_oci_descriptor(&selected_artifact.layer);
    client
        .pull_blob(&selected_artifact.child_reference, &layer, &mut output)
        .await
        .map_err(|error| {
            Error::InvalidInput(format!(
                "failed to download machine guest OCI artifact '{}': {error}",
                reference
            ))
        })?;
    output.flush().await.map_err(|error| {
        Error::Internal(format!(
            "failed to flush downloaded machine guest OCI artifact '{}': {error}",
            reference
        ))
    })?;
    output.shutdown().await.map_err(|error| {
        Error::Internal(format!(
            "failed to close downloaded machine guest OCI artifact '{}': {error}",
            reference
        ))
    })?;
    drop(output);

    verify_downloaded_oci_blob(&download_path, &selected_artifact.layer)?;
    log_machine_artifact_metadata(&reference, &selected_artifact.metadata);
    check_build_attestation(
        &reference,
        &selected_artifact.layer.digest,
        selected_artifact.metadata.attestation_repository.as_deref(),
    );
    fs::rename(&download_path, &cache_path).map_err(|error| {
        Error::Internal(format!(
            "failed to persist machine guest OCI artifact cache {}: {error}",
            cache_path.display()
        ))
    })?;

    Ok(cache_path)
}

async fn select_oci_artifact_layer(
    reference: &str,
    top_manifest_bytes: &[u8],
    client: &OciClient,
    auth: &RegistryAuth,
    provider: super::MachineProvider,
) -> Result<SelectedMachineArtifact, Error> {
    if let Ok(index) = serde_json::from_slice::<RegistryImageIndex>(top_manifest_bytes) {
        let manifest_descriptor =
            select_oci_manifest_descriptor(reference, &index.manifests, provider)?.clone();
        let child_reference = build_digest_reference(reference, &manifest_descriptor.digest)?;
        let (child_manifest_bytes, _) = client
            .pull_manifest_raw(
                &child_reference,
                auth,
                &[OCI_IMAGE_MEDIA_TYPE, IMAGE_MANIFEST_MEDIA_TYPE],
            )
            .await
            .map_err(|error| {
                Error::InvalidInput(format!(
                    "failed to pull machine guest OCI child manifest '{}': {error}",
                    manifest_descriptor.digest
                ))
            })?;
        let child_manifest = serde_json::from_slice::<RegistryImageManifest>(&child_manifest_bytes)
            .map_err(|error| {
                Error::Internal(format!(
                    "failed to parse machine guest OCI child manifest '{}': {error}",
                    manifest_descriptor.digest
                ))
            })?;
        let layer = select_machine_layer(reference, &child_manifest.layers)?;
        return Ok(SelectedMachineArtifact {
            child_reference,
            layer: layer.clone(),
            metadata: machine_artifact_metadata_from_annotations(
                Some(&manifest_descriptor.annotations),
                Some(&child_manifest.annotations),
            ),
        });
    }

    let image_manifest = serde_json::from_slice::<RegistryImageManifest>(top_manifest_bytes)
        .map_err(|error| {
            Error::Internal(format!(
                "failed to parse machine guest OCI manifest '{}': {error}",
                reference
            ))
        })?;
    let layer = select_machine_layer(reference, &image_manifest.layers)?;
    let registry_reference = Reference::try_from(strip_docker_reference_prefix(reference).as_str())
        .map_err(|error| {
            Error::InvalidInput(format!(
                "failed to parse machine guest OCI reference '{reference}': {error}"
            ))
        })?;
    Ok(SelectedMachineArtifact {
        child_reference: registry_reference,
        layer: layer.clone(),
        metadata: machine_artifact_metadata_from_annotations(
            Some(&image_manifest.annotations),
            None,
        ),
    })
}

fn build_oci_client(reference: &str) -> Result<OciClient, Error> {
    let mut config = OciClientConfig::default();
    if is_loopback_registry(reference) {
        config.protocol = ClientProtocol::Http;
    }
    OciClient::try_from(config).map_err(|error| {
        Error::Internal(format!(
            "failed to initialize OCI client for machine image '{reference}': {error}"
        ))
    })
}

fn is_loopback_registry(reference: &str) -> bool {
    let stripped_reference = strip_docker_reference_prefix(reference);
    let host = stripped_reference.split('/').next().unwrap_or_default();
    host.starts_with("localhost") || host.starts_with("127.0.0.1") || host.starts_with("[::1]")
}

fn strip_docker_reference_prefix(reference: &str) -> String {
    reference
        .strip_prefix("docker://")
        .unwrap_or(reference)
        .to_owned()
}

fn select_oci_manifest_descriptor<'a>(
    reference: &str,
    manifests: &'a [RegistryManifestDescriptor],
    provider: super::MachineProvider,
) -> Result<&'a RegistryManifestDescriptor, Error> {
    let disk_type = provider.oci_artifact_disk_type();
    manifests
        .iter()
        .find(|descriptor| {
            let Some(platform) = descriptor.platform.as_ref() else {
                return false;
            };
            platform.os == OCI_MACHINE_OS
                && current_machine_oci_architectures()
                    .iter()
                    .any(|arch| platform.architecture == *arch)
                && descriptor
                    .annotations
                    .get("disktype")
                    .map(|value| value == disk_type)
                    .unwrap_or(false)
        })
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "machine guest OCI reference '{}' does not contain a linux/{:?} '{}' disk artifact",
                reference,
                current_machine_oci_architectures(),
                disk_type
            ))
        })
}

fn select_machine_layer<'a>(
    reference: &str,
    layers: &'a [RegistryLayerDescriptor],
) -> Result<&'a RegistryLayerDescriptor, Error> {
    match layers {
        [layer] => Ok(layer),
        [] => Err(Error::InvalidInput(format!(
            "machine guest OCI reference '{}' has no disk layers",
            reference
        ))),
        _ => Err(Error::InvalidInput(format!(
            "machine guest OCI reference '{}' has {} disk layers; expected exactly 1",
            reference,
            layers.len()
        ))),
    }
}

fn current_machine_oci_architectures() -> &'static [&'static str] {
    #[cfg(target_arch = "aarch64")]
    {
        &["aarch64", "arm64"]
    }
    #[cfg(target_arch = "x86_64")]
    {
        &["x86_64", "amd64"]
    }
}

fn build_digest_reference(reference: &str, digest: &str) -> Result<Reference, Error> {
    let reference = strip_docker_reference_prefix(reference);
    let repository = reference
        .split_once('@')
        .map(|(value, _)| value.to_owned())
        .unwrap_or_else(|| {
            let last_slash = reference.rfind('/');
            let last_colon = reference.rfind(':');
            match (last_slash, last_colon) {
                (_, None) => reference.clone(),
                (Some(slash), Some(colon)) if colon > slash => reference[..colon].to_owned(),
                (None, Some(colon)) if !reference[..colon].contains('/') => reference.clone(),
                _ => reference.clone(),
            }
        });
    Reference::try_from(format!("{repository}@{digest}")).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to build machine guest OCI digest reference '{repository}@{digest}': {error}"
        ))
    })
}

fn cached_oci_blob_file_name(layer: &RegistryLayerDescriptor) -> String {
    let digest = digest_hex(&layer.digest).unwrap_or_else(|_| "machine-image".to_owned());
    let suffix = layer
        .annotations
        .get(OCI_ANNOTATION_TITLE)
        .and_then(|title| oci_artifact_suffix(title))
        .unwrap_or(".blob");
    format!("{digest}{suffix}")
}

fn oci_artifact_suffix(title: &str) -> Option<&str> {
    [
        ".raw.zst",
        ".raw.gz",
        ".raw",
        ".qcow2.xz",
        ".qcow2.gz",
        ".qcow2",
    ]
    .into_iter()
    .find(|suffix| title.ends_with(suffix))
}

fn verify_downloaded_oci_blob(path: &Path, layer: &RegistryLayerDescriptor) -> Result<(), Error> {
    let metadata = fs::metadata(path).map_err(|error| {
        Error::Internal(format!(
            "failed to stat downloaded machine guest OCI artifact {}: {error}",
            path.display()
        ))
    })?;
    if metadata.len() != layer.size as u64 {
        return Err(Error::InvalidInput(format!(
            "downloaded machine guest OCI artifact {} has size {}, expected {}",
            path.display(),
            metadata.len(),
            layer.size
        )));
    }
    let digest = compute_sha256(path)?;
    let expected = digest_hex(&layer.digest)?;
    if digest != expected {
        return Err(Error::InvalidInput(format!(
            "downloaded machine guest OCI artifact {} has sha256 {}, expected {}",
            path.display(),
            digest,
            expected
        )));
    }
    Ok(())
}

fn machine_artifact_metadata_from_annotations(
    primary: Option<&BTreeMap<String, String>>,
    fallback: Option<&BTreeMap<String, String>>,
) -> MachineArtifactMetadata {
    MachineArtifactMetadata {
        attestation_repository: annotation_value(
            primary,
            fallback,
            OCI_ANNOTATION_MACHINE_ATTESTATION_REPOSITORY,
        ),
        source_repository_url: annotation_value(primary, fallback, OCI_ANNOTATION_SOURCE),
        neovex_version: annotation_value(primary, fallback, OCI_ANNOTATION_MACHINE_NEOVEX_VERSION),
    }
}

fn annotation_value(
    primary: Option<&BTreeMap<String, String>>,
    fallback: Option<&BTreeMap<String, String>>,
    key: &str,
) -> Option<String> {
    primary
        .and_then(|annotations| annotations.get(key))
        .or_else(|| fallback.and_then(|annotations| annotations.get(key)))
        .filter(|value| !value.is_empty())
        .cloned()
}

fn log_machine_artifact_metadata(reference: &str, metadata: &MachineArtifactMetadata) {
    if let Some(neovex_version) = metadata.neovex_version.as_deref() {
        eprintln!("info: machine image '{reference}' embeds neovex {neovex_version}");
    }
    if let Some(source_repository_url) = metadata.source_repository_url.as_deref() {
        eprintln!("info: machine image '{reference}' source={source_repository_url}");
    }
}

/// The neovex source repo, used as a fallback for legacy machine images that
/// were published before OCI metadata recorded the attestation repository.
const NEOVEX_SOURCE_REPO: &str = "agentstation/neovex";

/// Query the GitHub Attestations API for a signed build provenance attestation
/// matching the downloaded artifact digest. Prefer the explicit attestation
/// repository published in the OCI metadata; fall back to the historical
/// dual-repo lookup only for older machine images. Advisory only — logs the
/// result but does not block the download.
fn check_build_attestation(
    reference: &str,
    subject_digest: &str,
    explicit_repository: Option<&str>,
) {
    let stripped = strip_docker_reference_prefix(reference);
    let Some(image_repo) = extract_ghcr_repo_path(&stripped) else {
        return;
    };

    let subject_digest = subject_digest.to_owned();
    let explicit_repository = explicit_repository.map(ToOwned::to_owned);
    let _ = run_blocking_in_thread("machine build attestation lookup", move || {
        let repos_to_check = attestation_repositories_for_reference(
            &image_repo,
            explicit_repository
                .as_deref()
                .filter(|repo| !repo.is_empty()),
        );

        let client = match BlockingClient::builder()
            .timeout(Duration::from_secs(10))
            .build()
        {
            Ok(client) => client,
            Err(error) => {
                eprintln!("warning: attestation lookup failed: {error}");
                return Ok(());
            }
        };

        for repo in &repos_to_check {
            match query_attestations(&client, repo, &subject_digest) {
                Ok(count) if count > 0 => {
                    eprintln!(
                        "verified: {count} build attestation(s) found for {subject_digest} in {repo}"
                    );
                    return Ok(());
                }
                Ok(_) => {}
                Err(msg) => {
                    eprintln!("warning: attestation lookup for {repo}: {msg}");
                }
            }
        }

        eprintln!("warning: no build attestations found for {subject_digest}");
        Ok(())
    });
}

fn run_blocking_in_thread<F, T>(label: &'static str, work: F) -> Result<T, Error>
where
    F: FnOnce() -> Result<T, Error> + Send + 'static,
    T: Send + 'static,
{
    thread::spawn(work)
        .join()
        .map_err(|_| Error::Internal(format!("{label} worker panicked")))?
}

/// Query the GitHub Attestations API for a specific repo and digest.
/// Returns the number of attestations found, or an error message.
fn query_attestations(
    client: &BlockingClient,
    repo: &str,
    subject_digest: &str,
) -> Result<usize, String> {
    let url = format!("https://api.github.com/repos/{repo}/attestations/{subject_digest}");

    let response = client
        .get(&url)
        .header("Accept", "application/json")
        .header("User-Agent", "neovex-machine-manager")
        .send()
        .map_err(|e| format!("{e}"))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let body: serde_json::Value = response.json().map_err(|e| format!("{e}"))?;

    Ok(body
        .get("attestations")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0))
}

fn attestation_repositories_for_reference(
    image_repo: &str,
    explicit_repository: Option<&str>,
) -> Vec<String> {
    if let Some(explicit_repository) = explicit_repository {
        return vec![explicit_repository.to_owned()];
    }

    if image_repo == NEOVEX_SOURCE_REPO {
        vec![image_repo.to_owned()]
    } else {
        vec![image_repo.to_owned(), NEOVEX_SOURCE_REPO.to_owned()]
    }
}

/// Extract the GitHub repository path (owner/repo) from a ghcr.io image
/// reference. Returns None for non-GHCR references.
fn extract_ghcr_repo_path(reference: &str) -> Option<String> {
    let without_host = reference.strip_prefix("ghcr.io/")?;
    let without_tag = without_host
        .split_once('@')
        .map(|(r, _)| r)
        .unwrap_or(without_host);
    let without_tag = without_tag
        .split_once(':')
        .map(|(r, _)| r)
        .unwrap_or(without_tag);
    let parts: Vec<&str> = without_tag.splitn(3, '/').collect();
    if parts.len() >= 2 {
        Some(format!("{}/{}", parts[0], parts[1]))
    } else {
        None
    }
}

fn compute_sha256(path: &Path) -> Result<String, Error> {
    let mut reader = BufReader::new(fs::File::open(path).map_err(|error| {
        Error::Internal(format!(
            "failed to open {} for sha256 verification: {error}",
            path.display()
        ))
    })?);
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer).map_err(|error| {
            Error::Internal(format!(
                "failed to read {} for sha256 verification: {error}",
                path.display()
            ))
        })?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn digest_hex(digest: &str) -> Result<String, Error> {
    let (algorithm, hex) = digest.split_once(':').ok_or_else(|| {
        Error::InvalidInput(format!(
            "invalid OCI digest '{}': missing algorithm prefix",
            digest
        ))
    })?;
    if algorithm != "sha256" {
        return Err(Error::InvalidInput(format!(
            "unsupported OCI digest algorithm '{}'; expected sha256",
            algorithm
        )));
    }
    Ok(hex.to_owned())
}

fn materialize_cached_disk(
    source_path: &Path,
    output_path: &Path,
    source_label: &str,
) -> Result<(), Error> {
    let temp_output = NamedTempFile::new_in(output_path.parent().ok_or_else(|| {
        Error::Internal(format!("{} has no parent directory", output_path.display()))
    })?)
    .map_err(|error| {
        Error::Internal(format!(
            "failed to allocate temporary materialization file for {}: {error}",
            source_label
        ))
    })?;

    let compression = detect_disk_compression(source_path)?;
    match compression {
        DiskCompression::None => {
            fs::copy(source_path, temp_output.path()).map_err(|error| {
                Error::Internal(format!(
                    "failed to stage {} into {}: {error}",
                    source_label,
                    temp_output.path().display()
                ))
            })?;
        }
        DiskCompression::Gzip => {
            let input = fs::File::open(source_path).map_err(|error| {
                Error::Internal(format!(
                    "failed to open {} for gzip decode: {error}",
                    source_path.display()
                ))
            })?;
            let mut decoder = GzDecoder::new(BufReader::new(input));
            let mut output = temp_output.reopen().map_err(|error| {
                Error::Internal(format!(
                    "failed to reopen {} for gzip decode: {error}",
                    temp_output.path().display()
                ))
            })?;
            io::copy(&mut decoder, &mut output).map_err(|error| {
                Error::Internal(format!(
                    "failed to decompress gzip {}: {error}",
                    source_label
                ))
            })?;
            output.flush().map_err(|error| {
                Error::Internal(format!(
                    "failed to flush decompressed {}: {error}",
                    source_label
                ))
            })?;
        }
        DiskCompression::Zstd => {
            let input = fs::File::open(source_path).map_err(|error| {
                Error::Internal(format!(
                    "failed to open {} for zstd decode: {error}",
                    source_path.display()
                ))
            })?;
            let mut output = temp_output.reopen().map_err(|error| {
                Error::Internal(format!(
                    "failed to reopen {} for zstd decode: {error}",
                    temp_output.path().display()
                ))
            })?;
            zstd::stream::copy_decode(BufReader::new(input), &mut output).map_err(|error| {
                Error::Internal(format!(
                    "failed to decompress zstd {}: {error}",
                    source_label
                ))
            })?;
            output.flush().map_err(|error| {
                Error::Internal(format!(
                    "failed to flush decompressed {}: {error}",
                    source_label
                ))
            })?;
        }
    }

    temp_output.persist(output_path).map_err(|error| {
        Error::Internal(format!(
            "failed to persist materialized machine image {}: {}",
            output_path.display(),
            error.error
        ))
    })?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiskCompression {
    None,
    Gzip,
    Zstd,
}

fn detect_disk_compression(path: &Path) -> Result<DiskCompression, Error> {
    let mut file = fs::File::open(path).map_err(|error| {
        Error::Internal(format!(
            "failed to open machine image {} for compression detection: {error}",
            path.display()
        ))
    })?;
    let mut header = [0_u8; 4];
    let read = file.read(&mut header).map_err(|error| {
        Error::Internal(format!(
            "failed to read machine image {} for compression detection: {error}",
            path.display()
        ))
    })?;
    if read >= 2 && header[..2] == [0x1f, 0x8b] {
        return Ok(DiskCompression::Gzip);
    }
    if read >= 4 && header == [0x28, 0xb5, 0x2f, 0xfd] {
        return Ok(DiskCompression::Zstd);
    }
    Ok(DiskCompression::None)
}

fn to_oci_descriptor(layer: &RegistryLayerDescriptor) -> OciDescriptor {
    OciDescriptor {
        digest: layer.digest.clone(),
        media_type: layer.media_type.clone(),
        size: layer.size,
        annotations: if layer.annotations.is_empty() {
            None
        } else {
            Some(layer.annotations.clone())
        },
        ..Default::default()
    }
}

fn run_async_in_thread<F, Fut, T>(build: F) -> Result<T, Error>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, Error>> + Send + 'static,
    T: Send + 'static,
{
    thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| {
                Error::Internal(format!("failed to build machine async runtime: {error}"))
            })?
            .block_on(build())
    })
    .join()
    .map_err(|_| Error::Internal("machine async worker panicked".to_owned()))?
}

fn allocate_machine_ssh_port(
    roots: &MachineRootLayout,
    machine_name: &str,
    state: &MachineStateRecord,
) -> Result<u16, Error> {
    with_port_allocation_lock(roots, || {
        let mut allocation_state = load_machine_port_allocation_state(roots)?;
        let preferred_port = state
            .runtime
            .as_ref()
            .map(|runtime| runtime.ssh_port)
            .or_else(|| allocation_state.machine_ports.get(machine_name).copied());

        if let Some(port) = preferred_port
            && machine_port_is_assignable(&allocation_state, machine_name, port)
        {
            allocation_state
                .machine_ports
                .insert(machine_name.to_owned(), port);
            write_machine_port_allocation_state(roots, &allocation_state)?;
            return Ok(port);
        }

        allocation_state.machine_ports.remove(machine_name);
        let port = next_available_machine_port(&allocation_state).ok_or_else(|| {
            Error::Internal(format!(
                "failed to allocate managed SSH port in range {MACHINE_PORT_MIN}-{MACHINE_PORT_MAX}"
            ))
        })?;
        allocation_state
            .machine_ports
            .insert(machine_name.to_owned(), port);
        write_machine_port_allocation_state(roots, &allocation_state)?;
        Ok(port)
    })
}

fn machine_port_is_assignable(
    allocation_state: &MachinePortAllocationState,
    machine_name: &str,
    port: u16,
) -> bool {
    if !managed_machine_port_range_contains(port) {
        return false;
    }
    if allocation_state
        .machine_ports
        .iter()
        .any(|(owner, owner_port)| owner != machine_name && *owner_port == port)
    {
        return false;
    }
    machine_port_is_available(port)
}

fn managed_machine_port_range_contains(port: u16) -> bool {
    (MACHINE_PORT_MIN..=MACHINE_PORT_MAX).contains(&port)
}

fn machine_port_is_available(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port))
        .map(|listener| {
            drop(listener);
        })
        .is_ok()
}

fn next_available_machine_port(allocation_state: &MachinePortAllocationState) -> Option<u16> {
    (MACHINE_PORT_MIN..=MACHINE_PORT_MAX).find(|port| {
        !allocation_state
            .machine_ports
            .values()
            .any(|reserved| reserved == port)
            && machine_port_is_available(*port)
    })
}

fn with_port_allocation_lock<T>(
    roots: &MachineRootLayout,
    operation: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    let _lock = lock_machine_port_allocation(roots)?;
    operation()
}

fn lock_machine_port_allocation(
    roots: &MachineRootLayout,
) -> Result<MachinePortAllocationLock, Error> {
    fs::create_dir_all(&roots.state_root).map_err(|error| {
        Error::Internal(format!(
            "failed to create machine state root {} for SSH port allocation: {error}",
            roots.state_root.display()
        ))
    })?;
    let lock_path = roots.port_allocation_lock_path();
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| {
            Error::Internal(format!(
                "failed to open machine SSH port allocation lock {}: {error}",
                lock_path.display()
            ))
        })?;
    file.lock_exclusive().map_err(|error| {
        Error::Internal(format!(
            "failed to acquire machine SSH port allocation lock {}: {error}",
            lock_path.display()
        ))
    })?;
    Ok(MachinePortAllocationLock { _file: file })
}

fn load_machine_port_allocation_state(
    roots: &MachineRootLayout,
) -> Result<MachinePortAllocationState, Error> {
    let path = roots.port_allocation_state_path();
    match fs::read(&path) {
        Ok(bytes) => {
            serde_json::from_slice::<MachinePortAllocationState>(&bytes).map_err(|error| {
                Error::Internal(format!(
                    "failed to parse machine SSH port allocation state {}: {error}",
                    path.display()
                ))
            })
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(MachinePortAllocationState::default())
        }
        Err(error) => Err(Error::Internal(format!(
            "failed to read machine SSH port allocation state {}: {error}",
            path.display()
        ))),
    }
}

fn write_machine_port_allocation_state(
    roots: &MachineRootLayout,
    allocation_state: &MachinePortAllocationState,
) -> Result<(), Error> {
    write_json_file(&roots.port_allocation_state_path(), allocation_state)
}

fn build_virtiofs_args(volume: &MachineVolume) -> Vec<String> {
    vec![
        "--device".to_owned(),
        format!(
            "virtio-fs,sharedDir={},mountTag={}",
            volume.source.display(),
            mount_tag(&volume.target)
        ),
    ]
}

fn resolve_ready_wait_timeout() -> Duration {
    let seconds =
        env_parse_u64(READY_WAIT_TIMEOUT_ENV).unwrap_or(DEFAULT_READY_WAIT_TIMEOUT.as_secs());
    Duration::from_secs(seconds.max(1))
}

fn resolve_ssh_ready_wait_timeout() -> Duration {
    let seconds = env_parse_u64(SSH_READY_WAIT_TIMEOUT_ENV)
        .unwrap_or(DEFAULT_SSH_READY_WAIT_TIMEOUT.as_secs());
    Duration::from_secs(seconds.max(1))
}

fn resolve_machine_api_ready_wait_timeout() -> Duration {
    let seconds = env_parse_u64(MACHINE_API_READY_WAIT_TIMEOUT_ENV)
        .unwrap_or(DEFAULT_MACHINE_API_READY_TIMEOUT.as_secs());
    Duration::from_secs(seconds.max(1))
}

fn resolve_stop_wait_timeout() -> Duration {
    let seconds =
        env_parse_u64(STOP_WAIT_TIMEOUT_ENV).unwrap_or(DEFAULT_STOP_WAIT_TIMEOUT.as_secs());
    Duration::from_secs(seconds.max(1))
}

fn env_parse_u64(name: &str) -> Option<u64> {
    std::env::var(name).ok()?.trim().parse().ok()
}

pub(super) fn mount_tag(target: &Path) -> String {
    let digest = Sha256::digest(target.as_os_str().as_encoded_bytes());
    format!("{digest:x}")[..36].to_owned()
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::thread;

    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tempfile::TempDir;

    use super::*;
    use crate::machine::{
        CURRENT_MACHINE_CONFIG_VERSION, MachineBootstrapMode, MachineGuestConfig,
        MachineImageFormat, MachineImageSource, MachineProvider, MachineResources,
        MachineRootLayout, machine_image_reference_repository,
    };

    fn sample_config(image: &Path) -> MachineConfigRecord {
        let base_root = image
            .parent()
            .expect("test image path should have a parent directory");
        MachineConfigRecord {
            version: CURRENT_MACHINE_CONFIG_VERSION,
            name: "default".to_owned(),
            provider: MachineProvider::Krunkit,
            guest: MachineGuestConfig {
                image_source: MachineImageSource::LocalDisk {
                    path: image.to_path_buf(),
                },
                ssh_user: "core".to_owned(),
                ssh_identity_path: None,
                ignition_file_path: None,
                efi_variable_store_path: None,
            },
            resources: MachineResources {
                cpus: 2,
                memory_mib: 2048,
                disk_gib: 20,
            },
            volumes: vec![MachineVolume {
                source: PathBuf::from("/Users"),
                target: PathBuf::from("/Users"),
            }],
            roots: MachineRootLayout::new(
                base_root.join("config-root"),
                base_root.join("state-root"),
                base_root.join("runtime-root"),
            ),
        }
    }

    fn machine_lifecycle_test_lock() -> &'static std::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }

    #[test]
    fn krunkit_provider_capabilities_match_podman_aligned_contract() {
        assert!(!MachineProvider::Krunkit.uses_provider_networking());
        assert!(MachineProvider::Krunkit.requires_exclusive_active());
        assert_eq!(
            MachineProvider::Krunkit.image_format(),
            MachineImageFormat::Raw
        );
        assert_eq!(
            MachineProvider::Krunkit.bootstrap_mode(),
            MachineBootstrapMode::Ignition
        );
        assert_eq!(MachineProvider::Krunkit.oci_artifact_disk_type(), "applehv");
        assert!(MachineProvider::Wsl2.uses_provider_networking());
        assert!(!MachineProvider::Wsl2.requires_exclusive_active());
        assert_eq!(
            MachineProvider::Wsl2.image_format(),
            MachineImageFormat::Tar
        );
        assert_eq!(
            MachineProvider::Wsl2.bootstrap_mode(),
            MachineBootstrapMode::ShellScript
        );
        assert_eq!(MachineProvider::Wsl2.oci_artifact_disk_type(), "wsl");
    }

    #[test]
    fn machine_image_reference_repository_strips_tag_and_digest() {
        assert_eq!(
            machine_image_reference_repository("docker://quay.io/podman/machine-os:6.0"),
            "quay.io/podman/machine-os"
        );
        assert_eq!(
            machine_image_reference_repository("docker://quay.io/podman/machine-os@sha256:abc123"),
            "quay.io/podman/machine-os"
        );
    }

    #[test]
    fn podman_machine_os_requires_host_guest_neovex_sync() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let image_path = temp_dir.path().join("disk.raw");
        fs::write(&image_path, []).expect("image should write");
        let mut config = sample_config(&image_path);
        config.guest.image_source = MachineImageSource::OciReference {
            reference: "docker://quay.io/podman/machine-os:6.0".to_owned(),
        };

        assert_eq!(
            requires_host_guest_neovex_sync(&config),
            cfg!(target_os = "macos")
        );
    }

    #[test]
    fn podman_machine_os_bootstrap_contract_requires_ssh_identity() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let image_path = temp_dir.path().join("disk.raw");
        fs::write(&image_path, []).expect("image should write");
        let mut config = sample_config(&image_path);
        config.guest.image_source = MachineImageSource::OciReference {
            reference: "docker://quay.io/podman/machine-os:6.0".to_owned(),
        };

        if cfg!(target_os = "macos") {
            let error = validate_machine_bootstrap_contract(&config)
                .expect_err("podman machine-os should require ssh identity");
            assert!(error.to_string().contains("--ssh-identity"));
        } else {
            validate_machine_bootstrap_contract(&config)
                .expect("non-macOS hosts should not require macOS SSH bootstrapping");
        }
    }

    #[test]
    fn local_guest_binary_lookup_prefers_release_over_debug() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let target_dir = temp_dir
            .path()
            .join("target")
            .join(DEFAULT_GUEST_NEOVEX_TARGET_TRIPLE_ARM64);
        let release_binary = target_dir.join("release").join("neovex");
        let debug_binary = target_dir.join("debug").join("neovex");
        fs::create_dir_all(release_binary.parent().expect("release dir should exist"))
            .expect("release dir should create");
        fs::create_dir_all(debug_binary.parent().expect("debug dir should exist"))
            .expect("debug dir should create");
        fs::write(&release_binary, b"release").expect("release binary should write");
        fs::write(&debug_binary, b"debug").expect("debug binary should write");

        assert_eq!(
            find_local_guest_neovex_binary_under(
                temp_dir.path(),
                DEFAULT_GUEST_NEOVEX_TARGET_TRIPLE_ARM64,
            ),
            Some(release_binary)
        );
    }

    #[test]
    fn local_guest_binary_lookup_falls_back_to_debug() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let debug_binary = temp_dir
            .path()
            .join("target")
            .join(DEFAULT_GUEST_NEOVEX_TARGET_TRIPLE_ARM64)
            .join("debug")
            .join("neovex");
        fs::create_dir_all(debug_binary.parent().expect("debug dir should exist"))
            .expect("debug dir should create");
        fs::write(&debug_binary, b"debug").expect("debug binary should write");

        assert_eq!(
            find_local_guest_neovex_binary_under(
                temp_dir.path(),
                DEFAULT_GUEST_NEOVEX_TARGET_TRIPLE_ARM64,
            ),
            Some(debug_binary)
        );
    }

    #[test]
    fn workspace_root_from_manifest_dir_climbs_out_of_crate_dir() {
        let manifest_dir = Path::new("/tmp/neovex/crates/neovex-bin");

        assert_eq!(
            workspace_root_from_manifest_dir(manifest_dir),
            Some(Path::new("/tmp/neovex"))
        );
    }

    #[test]
    fn converge_machine_image_contract_updates_legacy_stream_and_rebuilds_boot_artifacts() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let image_path = temp_dir.path().join("disk.raw");
        fs::write(&image_path, []).expect("image should write");
        let mut config = sample_config(&image_path);
        config.guest.image_source = MachineImageSource::OciReference {
            reference: "docker://ghcr.io/agentstation/neovex-machine-os:v0.1.0".to_owned(),
        };
        let paths = config.roots.paths("default");
        paths
            .ensure_directories()
            .expect("machine directories should exist");
        fs::write(&paths.materialized_image_path, b"old-image").expect("image should write");
        fs::write(&paths.efi_variable_store_path, b"old-efi").expect("efi store should write");

        let mut state = MachineStateRecord::initialized();
        state.runtime = Some(MachineRuntimeState {
            helper_binaries: MachineHelperBinaryPaths {
                krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
                gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
            },
            image_path: paths.materialized_image_path.clone(),
            efi_variable_store_path: paths.efi_variable_store_path.clone(),
            machine_image_source: "docker://ghcr.io/agentstation/neovex-machine-os:v0.1.0"
                .to_owned(),
            ssh_port: 20022,
            rest_uri: format!("unix://{}", paths.krunkit_endpoint_path.display()),
            ready_vsock_port: READY_VSOCK_PORT,
        });

        converge_machine_image_contract(&paths, &mut config, &mut state)
            .expect("contract convergence should succeed");

        assert_eq!(
            config.guest.image_source,
            MachineImageSource::OciReference {
                reference: super::super::default_machine_image_for_provider(
                    MachineProvider::Krunkit,
                ),
            }
        );
        assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
        assert_eq!(state.manager, MachineManagerState::Stale);
        assert!(
            state
                .last_error
                .as_deref()
                .unwrap_or_default()
                .contains("boot artifacts were reset")
        );
        assert!(!paths.materialized_image_path.exists());
        assert!(!paths.efi_variable_store_path.exists());
    }

    #[test]
    fn machine_image_rebuild_reason_requires_rebuild_when_boot_artifacts_lack_identity() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let image_path = temp_dir.path().join("disk.raw");
        fs::write(&image_path, []).expect("image should write");
        let config = sample_config(&image_path);
        let paths = config.roots.paths("default");
        paths
            .ensure_directories()
            .expect("machine directories should exist");
        fs::write(&paths.materialized_image_path, b"old-image").expect("image should write");

        let reason = machine_image_rebuild_reason(
            &paths,
            &MachineStateRecord::initialized(),
            "docker://quay.io/podman/machine-os@sha256:test",
        )
        .expect("boot artifacts without recorded identity should rebuild");

        assert!(reason.contains("without a recorded base-image identity"));
    }

    #[test]
    fn launch_plan_requires_bootable_local_disk_image() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
        let image_path = temp_dir.path().join("disk.raw");
        fs::write(&image_path, []).expect("image should write");
        let config = sample_config(&image_path);
        let paths = config.roots.paths("default");
        let state = MachineStateRecord::initialized();
        let plan =
            MachineLaunchPlan::build(&paths, &config, &state).expect("launch plan should build");

        assert!(
            plan.krunkit_command
                .args
                .iter()
                .any(|arg| arg.contains("virtio-blk,path="))
        );
        assert!(
            plan.krunkit_command
                .args
                .iter()
                .any(|arg| arg.contains("virtio-net,type=unixgram"))
        );
        assert!(plan.krunkit_command.args.iter().any(|arg| {
            arg == &format!(
                "virtio-vsock,port=1025,socketURL={},listen",
                paths.ready_socket_path.display()
            )
        }));
        assert!(plan.krunkit_command.args.iter().any(|arg| {
            arg == &format!(
                "virtio-vsock,port=1024,socketURL={},listen",
                paths.ignition_socket_path.display()
            )
        }));
        assert!(
            !plan
                .gvproxy_command
                .args
                .iter()
                .any(|arg| arg == "-forward-sock")
        );
        assert_eq!(
            plan.ignition_file_path,
            Some(paths.generated_ignition_path.clone())
        );
    }

    #[test]
    fn launch_plan_adds_gvproxy_machine_api_forwarding_when_ssh_identity_exists() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
        let image_path = temp_dir.path().join("disk.raw");
        let ssh_identity_path = temp_dir.path().join("machine-key");
        let ssh_public_key_path = temp_dir.path().join("machine-key.pub");
        fs::write(&image_path, []).expect("image should write");
        fs::write(&ssh_identity_path, "fake key").expect("identity should write");
        fs::write(
            &ssh_public_key_path,
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestKey jack@example",
        )
        .expect("public key should write");

        let mut config = sample_config(&image_path);
        config.guest.ssh_identity_path = Some(ssh_identity_path.clone());

        let paths = config.roots.paths("default");
        let state = MachineStateRecord::initialized();
        let plan =
            MachineLaunchPlan::build(&paths, &config, &state).expect("launch plan should build");

        assert!(plan.gvproxy_command.args.windows(2).any(|pair| {
            pair[0] == "-forward-sock" && pair[1] == paths.api_socket_path.display().to_string()
        }));
        assert!(
            plan.gvproxy_command
                .args
                .windows(2)
                .any(|pair| { pair[0] == "-forward-dest" && pair[1] == GUEST_NEOVEX_SOCKET })
        );
        assert!(
            plan.gvproxy_command
                .args
                .windows(2)
                .any(|pair| { pair[0] == "-forward-user" && pair[1] == MACHINE_API_FORWARD_USER })
        );
        assert!(plan.gvproxy_command.args.windows(2).any(|pair| {
            pair[0] == "-forward-identity" && pair[1] == ssh_identity_path.display().to_string()
        }));
    }

    #[test]
    fn build_virtio_vsock_listen_arg_matches_podman_listen_mode() {
        let socket_path = Path::new("/tmp/neovex-test.sock");

        assert_eq!(
            build_virtio_vsock_listen_arg(1024, socket_path),
            "virtio-vsock,port=1024,socketURL=/tmp/neovex-test.sock,listen"
        );
    }

    #[test]
    fn remote_shell_command_single_quotes_guest_scripts_for_ssh() {
        let script = "if [ -x '/usr/local/bin/neovex' ]; then printf '%s' ok; fi";

        assert_eq!(
            remote_shell_command(script),
            "sh -lc 'if [ -x '\"'\"'/usr/local/bin/neovex'\"'\"' ]; then printf '\"'\"'%s'\"'\"' ok; fi'"
        );
    }

    #[test]
    fn ensure_guest_neovex_socket_shell_repairs_first_boot_failures() {
        let script = ensure_guest_neovex_socket_shell_script();

        assert!(script.contains("systemctl daemon-reload"), "{script}");
        assert!(
            script.contains("systemctl stop neovex.service neovex.socket"),
            "{script}"
        );
        assert!(
            script.contains("systemctl reset-failed neovex.service neovex.socket"),
            "{script}"
        );
        assert!(script.contains("systemctl start neovex.socket"), "{script}");
        assert!(script.contains(GUEST_NEOVEX_SOCKET), "{script}");
    }

    #[test]
    fn annotate_machine_start_error_hints_when_guest_reaches_login_prompt() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let image_path = temp_dir.path().join("disk.raw");
        fs::write(&image_path, []).expect("image should write");
        let config = sample_config(&image_path);
        let paths = config.roots.paths("default");
        paths
            .ensure_directories()
            .expect("machine directories should exist");
        fs::write(&paths.machine_log_path, "Fedora Linux 42\nfedora login:\n")
            .expect("machine log should write");

        let error = annotate_machine_start_error(
            &paths,
            &config,
            Error::Internal(
                "gvproxy exited before machine readiness with status exit status: 0".to_owned(),
            ),
        );

        let message = error.to_string();
        assert!(message.contains("gvproxy exited before machine readiness"));
        assert!(message.contains("guest reached a console login prompt"));
        assert!(message.contains("generic fedora-bootc raw images"));
    }

    #[test]
    fn annotate_machine_start_error_leaves_unrelated_failures_unchanged() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let image_path = temp_dir.path().join("disk.raw");
        fs::write(&image_path, []).expect("image should write");
        let config = sample_config(&image_path);
        let paths = config.roots.paths("default");
        paths
            .ensure_directories()
            .expect("machine directories should exist");
        fs::write(&paths.machine_log_path, "Fedora Linux 42\nfedora login:\n")
            .expect("machine log should write");

        let error = annotate_machine_start_error(
            &paths,
            &config,
            Error::Internal("failed to resolve machine guest OCI reference".to_owned()),
        );

        assert_eq!(
            error.to_string(),
            "internal error: failed to resolve machine guest OCI reference"
        );
    }

    #[test]
    fn registry_image_reference_materializes_raw_disk_from_oci_registry() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths("default");
        let raw_payload = b"raw-disk-oci-bytes".to_vec();
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&raw_payload)
            .expect("gzip payload should write");
        let gzip_payload = encoder.finish().expect("gzip payload should finish");
        let reference = serve_fake_oci_registry(gzip_payload);

        let materialized = resolve_bootable_image_path(
            &paths,
            &MachineImageSource::OciReference { reference },
            MachineProvider::Krunkit,
        )
        .expect("registry image should materialize");

        assert_eq!(materialized, paths.materialized_image_path);
        assert_eq!(
            fs::read(&paths.materialized_image_path).expect("materialized image should read"),
            raw_payload
        );
    }

    #[test]
    fn registry_image_reference_reuses_materialized_disk_when_present() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths("default");
        fs::create_dir_all(&paths.image_cache_dir).expect("image cache dir should exist");
        fs::write(&paths.materialized_image_path, []).expect("materialized image should write");

        let config = MachineConfigRecord {
            version: CURRENT_MACHINE_CONFIG_VERSION,
            name: "default".to_owned(),
            provider: MachineProvider::Krunkit,
            guest: MachineGuestConfig {
                image_source: MachineImageSource::OciReference {
                    reference: format!(
                        "docker://ghcr.io/agentstation/neovex-machine-os:v{}",
                        env!("CARGO_PKG_VERSION")
                    ),
                },
                ssh_user: "core".to_owned(),
                ssh_identity_path: None,
                ignition_file_path: None,
                efi_variable_store_path: None,
            },
            resources: MachineResources {
                cpus: 2,
                memory_mib: 2048,
                disk_gib: 20,
            },
            volumes: Vec::new(),
            roots: layout.clone(),
        };

        let plan = MachineLaunchPlan::build(&paths, &config, &MachineStateRecord::initialized())
            .expect("materialized disk should satisfy launch plan");

        assert_eq!(plan.runtime.image_path, paths.materialized_image_path);
        assert!(
            plan.krunkit_command
                .args
                .iter()
                .any(|arg| arg.contains(&format!(
                    "virtio-blk,path={}",
                    paths.materialized_image_path.display()
                )))
        );
    }

    #[test]
    fn http_image_source_materializes_raw_disk_into_reserved_path() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths("default");
        let payload = b"raw-disk-bytes".to_vec();
        let url = serve_single_http_response(payload.clone(), None);

        let materialized = resolve_bootable_image_path(
            &paths,
            &MachineImageSource::HttpUrl { url: url.clone() },
            MachineProvider::Krunkit,
        )
        .expect("http source should materialize");

        assert_eq!(materialized, paths.materialized_image_path);
        assert_eq!(
            fs::read(&paths.materialized_image_path).expect("materialized image should read"),
            payload
        );
    }

    #[test]
    fn cached_zstd_machine_image_materializes_into_reserved_path() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let source_path = temp_dir.path().join("disk.raw.zst");
        let output_path = temp_dir.path().join("disk.raw");
        let payload = b"raw-disk-zstd-bytes".to_vec();
        let compressed = zstd::stream::encode_all(std::io::Cursor::new(&payload), 1)
            .expect("zstd payload should encode");
        fs::write(&source_path, compressed).expect("compressed source should write");

        materialize_cached_disk(&source_path, &output_path, "test zstd image")
            .expect("zstd image should materialize");

        assert_eq!(
            fs::read(&output_path).expect("materialized image should read"),
            payload
        );
    }

    #[test]
    fn http_gzip_image_source_materializes_decompressed_disk_into_reserved_path() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths("default");
        let payload = b"raw-disk-gzip-bytes".to_vec();
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&payload)
            .expect("gzip payload should write");
        let gzip_payload = encoder.finish().expect("gzip payload should finish");
        let url = serve_single_http_response(gzip_payload, Some("/disk.raw.gz"));

        let materialized = resolve_bootable_image_path(
            &paths,
            &MachineImageSource::HttpUrl { url: url.clone() },
            MachineProvider::Krunkit,
        )
        .expect("gzip http source should materialize");

        assert_eq!(materialized, paths.materialized_image_path);
        assert_eq!(
            fs::read(&paths.materialized_image_path).expect("materialized image should read"),
            payload
        );
    }

    #[test]
    fn helper_resolution_honors_environment_overrides() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let krunkit_path = temp_dir.path().join("krunkit");
        let gvproxy_path = temp_dir.path().join("gvproxy");
        let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
        let resolved =
            MachineHelperBinaryPaths::resolve().expect("helper binaries should resolve via env");

        assert_eq!(resolved.krunkit, krunkit_path);
        assert_eq!(resolved.gvproxy, gvproxy_path);
    }

    #[test]
    fn bundled_helper_candidates_cover_root_and_bin_layouts() {
        let root_layout = bundled_helper_candidates_for_executable(
            Path::new("/opt/homebrew/Caskroom/neovex/0.1.10/neovex"),
            "gvproxy",
        );
        assert_eq!(
            root_layout,
            vec![PathBuf::from(
                "/opt/homebrew/Caskroom/neovex/0.1.10/libexec/gvproxy"
            )]
        );

        let bin_layout = bundled_helper_candidates_for_executable(
            Path::new("/opt/homebrew/bin/neovex"),
            "gvproxy",
        );
        assert_eq!(
            bin_layout,
            vec![
                PathBuf::from("/opt/homebrew/bin/libexec/gvproxy"),
                PathBuf::from("/opt/homebrew/libexec/gvproxy"),
            ]
        );
    }

    #[test]
    fn helper_resolution_prefers_packaged_candidates_before_fallbacks() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let packaged_dir = temp_dir.path().join("libexec");
        let fallback_dir = temp_dir.path().join("fallback");
        fs::create_dir_all(&packaged_dir).expect("packaged helper dir should exist");
        fs::create_dir_all(&fallback_dir).expect("fallback helper dir should exist");
        let packaged_gvproxy = packaged_dir.join("gvproxy");
        let fallback_gvproxy = fallback_dir.join("gvproxy");
        write_helper_stub(&packaged_gvproxy, "gvproxy");
        write_helper_stub(&fallback_gvproxy, "gvproxy");

        let resolved = resolve_helper_binary(
            "NEOVEX_TEST_GVPROXY",
            "gvproxy-does-not-exist",
            std::slice::from_ref(&packaged_gvproxy),
            &[fallback_gvproxy],
        )
        .expect("packaged helper should resolve");

        assert_eq!(resolved, packaged_gvproxy);
    }

    #[test]
    fn helper_resolution_honors_helper_binary_directory_override() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let helper_dir = temp_dir.path().join("helpers");
        fs::create_dir_all(&helper_dir).expect("helper dir should exist");
        let helper_gvproxy = helper_dir.join("gvproxy");
        write_helper_stub(&helper_gvproxy, "gvproxy");
        let _guard = MachineHelperEnvGuard::with_helper_binary_dir(&helper_dir);

        let resolved = resolve_helper_binary("NEOVEX_TEST_GVPROXY", "gvproxy", &[], &[])
            .expect("helper dir override should resolve");

        assert_eq!(resolved, helper_gvproxy);
    }

    #[test]
    fn known_helper_candidates_mirror_podman_darwin_defaults() {
        assert_eq!(
            known_helper_candidates("gvproxy"),
            vec![
                PathBuf::from("/usr/local/opt/podman/libexec/podman/gvproxy"),
                PathBuf::from("/opt/homebrew/opt/podman/libexec/podman/gvproxy"),
                PathBuf::from("/opt/homebrew/bin/gvproxy"),
                PathBuf::from("/usr/local/bin/gvproxy"),
                PathBuf::from("/opt/homebrew/libexec/podman/gvproxy"),
                PathBuf::from("/usr/local/libexec/podman/gvproxy"),
                PathBuf::from("/usr/local/lib/podman/gvproxy"),
                PathBuf::from("/usr/libexec/podman/gvproxy"),
                PathBuf::from("/usr/lib/podman/gvproxy"),
            ]
        );
    }

    #[test]
    fn helper_resolution_does_not_fall_back_to_path() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let helper_dir = temp_dir.path().join("path-only");
        fs::create_dir_all(&helper_dir).expect("path-only helper dir should exist");
        let helper_gvproxy = helper_dir.join("gvproxy");
        write_helper_stub(&helper_gvproxy, "gvproxy");
        let _guard = MachineHelperEnvGuard::with_path_only(&helper_dir);

        let error = resolve_helper_binary("NEOVEX_TEST_GVPROXY", "gvproxy", &[], &[])
            .expect_err("PATH-only helpers should be ignored");

        assert!(
            error
                .to_string()
                .contains("supported packaged or Homebrew helper directory"),
            "{error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn machine_command_spawn_detaches_helpers_into_new_session() {
        let command = MachineCommandLine {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_owned(), "sleep 30".to_owned()],
        };
        let mut child = command.spawn().expect("helper process should spawn");
        let child_pid = child.id() as i32;
        let parent_sid = unsafe { libc::getsid(0) };
        let child_sid = unsafe { libc::getsid(child_pid) };

        assert!(parent_sid > 0, "parent sid should resolve");
        assert_eq!(child_sid, child_pid, "child should lead its own session");
        assert_ne!(
            child_sid, parent_sid,
            "child session should differ from parent"
        );

        cleanup_process(&mut child).expect("child should clean up");
    }

    #[test]
    fn ssh_port_is_listening_detects_local_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let port = listener
            .local_addr()
            .expect("listener address should resolve")
            .port();

        assert!(ssh_port_is_listening(port));
    }

    #[test]
    fn wait_for_ssh_ready_accepts_listening_port_without_identity_probe() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let port = listener
            .local_addr()
            .expect("listener address should resolve")
            .port();
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let image_path = temp_dir.path().join("disk.raw");
        fs::write(&image_path, []).expect("image should write");
        let config = sample_config(&image_path);
        let mut gvproxy_child = MachineCommandLine {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_owned(), "sleep 30".to_owned()],
        }
        .spawn()
        .expect("gvproxy probe child should spawn");
        let mut krunkit_child = MachineCommandLine {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_owned(), "sleep 30".to_owned()],
        }
        .spawn()
        .expect("krunkit probe child should spawn");

        let result = wait_for_ssh_ready(
            &config,
            port,
            Duration::from_secs(1),
            &mut krunkit_child,
            &mut gvproxy_child,
            &StartupSignalMonitor::inactive_for_test(),
        );

        cleanup_process(&mut krunkit_child).expect("krunkit probe child should clean up");
        cleanup_process(&mut gvproxy_child).expect("gvproxy probe child should clean up");
        drop(listener);

        assert!(result.is_ok(), "listener-backed SSH readiness should pass");
    }

    #[test]
    fn wait_for_path_returns_cancelled_when_startup_signal_is_set() {
        let _guard = machine_lifecycle_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let path = temp_dir.path().join("gvproxy.sock");
        let mut child = MachineCommandLine {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_owned(), "sleep 30".to_owned()],
        }
        .spawn()
        .expect("probe child should spawn");

        let result = wait_for_path(
            &path,
            Duration::from_secs(1),
            &mut child,
            &StartupSignalMonitor::interrupted_for_test(),
        );

        cleanup_process(&mut child).expect("probe child should clean up");

        assert!(matches!(result, Err(Error::Cancelled)));
    }

    #[test]
    fn interrupted_start_transitions_to_stopped_and_cleans_runtime_artifacts() {
        let _guard = machine_lifecycle_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let image_path = temp_dir.path().join("disk.raw");
        fs::write(&image_path, []).expect("image should write");
        let config = sample_config(&image_path);
        let paths = config.roots.paths("default");
        paths
            .ensure_directories()
            .expect("machine directories should exist");

        for path in [
            &paths.ready_socket_path,
            &paths.ignition_socket_path,
            &paths.api_socket_path,
            &paths.gvproxy_socket_path,
            &paths.krunkit_endpoint_path,
            &paths.gvproxy_pid_path,
            &paths.krunkit_pid_path,
        ] {
            fs::write(path, b"artifact").expect("runtime artifact should write");
        }
        for path in [
            &paths.machine_log_path,
            &paths.krunkit_log_path,
            &paths.gvproxy_log_path,
        ] {
            fs::write(path, b"non-empty").expect("log artifact should write");
        }

        let mut krunkit_child = MachineCommandLine {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_owned(), "sleep 30".to_owned()],
        }
        .spawn()
        .expect("krunkit child should spawn");
        let mut gvproxy_child = MachineCommandLine {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_owned(), "sleep 30".to_owned()],
        }
        .spawn()
        .expect("gvproxy child should spawn");

        let mut state = MachineStateRecord::initialized();
        state.lifecycle = MachineLifecycle::Starting;
        state.manager = MachineManagerState::Launching;
        state.runtime = Some(MachineRuntimeState {
            helper_binaries: MachineHelperBinaryPaths {
                krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
                gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
            },
            image_path,
            efi_variable_store_path: paths.efi_variable_store_path.clone(),
            machine_image_source: describe_machine_image_source(&config.guest.image_source),
            ssh_port: 20022,
            rest_uri: format!("unix://{}", paths.krunkit_endpoint_path.display()),
            ready_vsock_port: READY_VSOCK_PORT,
        });

        let result = handle_start_machine_error(
            &paths,
            &config,
            &mut state,
            Error::Cancelled,
            Some(&mut krunkit_child),
            Some(&mut gvproxy_child),
        );

        assert!(matches!(result, Err(Error::Cancelled)));
        assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
        assert_eq!(state.manager, MachineManagerState::HelpersResolved);
        assert_eq!(state.last_error, None);
        assert!(
            krunkit_child
                .try_wait()
                .expect("krunkit child status should resolve")
                .is_some(),
            "krunkit child should be reaped on interrupted startup"
        );
        assert!(
            gvproxy_child
                .try_wait()
                .expect("gvproxy child status should resolve")
                .is_some(),
            "gvproxy child should be reaped on interrupted startup"
        );
        for path in [
            &paths.ready_socket_path,
            &paths.ignition_socket_path,
            &paths.api_socket_path,
            &paths.gvproxy_socket_path,
            &paths.krunkit_endpoint_path,
            &paths.gvproxy_pid_path,
            &paths.krunkit_pid_path,
        ] {
            assert!(
                !path.exists(),
                "runtime artifact {} should be removed",
                path.display()
            );
        }
        for path in [
            &paths.machine_log_path,
            &paths.krunkit_log_path,
            &paths.gvproxy_log_path,
        ] {
            assert_eq!(
                fs::read(path).expect("log artifact should remain readable"),
                Vec::<u8>::new(),
                "log artifact {} should be truncated",
                path.display()
            );
        }
    }

    #[test]
    fn stop_machine_uses_graceful_krunkit_stop_before_cleaning_up_helpers() {
        let _guard = machine_lifecycle_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let image_path = temp_dir.path().join("disk.raw");
        fs::write(&image_path, []).expect("image should write");
        let config = sample_config(&image_path);
        let paths = config.roots.paths("default");
        paths
            .ensure_directories()
            .expect("machine directories should exist");

        let (krunkit_pid, krunkit_reaper) = spawn_reaped_process("exec sleep 30");
        let (gvproxy_pid, gvproxy_reaper) = spawn_reaped_process("exec sleep 30");
        fs::write(&paths.krunkit_pid_path, krunkit_pid.to_string())
            .expect("krunkit pid should write");
        fs::write(&paths.gvproxy_pid_path, gvproxy_pid.to_string())
            .expect("gvproxy pid should write");

        let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let requests_for_server = std::sync::Arc::clone(&requests);
        let endpoint_path = paths.krunkit_endpoint_path.clone();
        let request_path = endpoint_path.clone();
        let server = thread::spawn(move || {
            let listener =
                UnixListener::bind(&endpoint_path).expect("endpoint listener should bind");
            let (mut stream, _) = listener.accept().expect("endpoint should accept request");
            let mut buffer = [0_u8; 1024];
            let read = stream.read(&mut buffer).expect("request should read");
            let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
            let state = if request.contains("\"HardStop\"") {
                "HardStop"
            } else {
                "Stop"
            };
            requests_for_server
                .lock()
                .expect("request log should lock")
                .push(state.to_owned());
            let _ = send_signal(krunkit_pid, SIGKILL);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .expect("response should write");
            stream.flush().expect("response should flush");
        });

        let deadline = Instant::now() + Duration::from_secs(1);
        while !request_path.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(request_path.exists(), "endpoint should appear before stop");

        let mut state = MachineStateRecord::initialized();
        state.lifecycle = MachineLifecycle::Running;
        state.manager = MachineManagerState::Ready;
        state.runtime = Some(MachineRuntimeState {
            helper_binaries: MachineHelperBinaryPaths {
                krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
                gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
            },
            image_path,
            efi_variable_store_path: paths.efi_variable_store_path.clone(),
            machine_image_source: describe_machine_image_source(&config.guest.image_source),
            ssh_port: 20022,
            rest_uri: format!("unix://{}", paths.krunkit_endpoint_path.display()),
            ready_vsock_port: READY_VSOCK_PORT,
        });

        stop_machine(&paths, &config, &mut state).expect("machine stop should succeed");
        server.join().expect("endpoint server should finish");

        assert_eq!(
            requests.lock().expect("request log should lock").clone(),
            vec!["Stop".to_owned()]
        );
        assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
        assert_eq!(state.manager, MachineManagerState::HelpersResolved);
        assert_eq!(state.last_error, None);
        assert!(
            wait_for_pid_exit(krunkit_pid, Duration::from_secs(2))
                .expect("krunkit pid should become not alive"),
            "krunkit process should exit during graceful provider stop"
        );
        assert!(
            wait_for_pid_exit(gvproxy_pid, Duration::from_secs(2))
                .expect("gvproxy pid should become not alive"),
            "gvproxy process should be stopped during cleanup"
        );
        krunkit_reaper
            .join()
            .expect("krunkit reaper should observe process exit");
        gvproxy_reaper
            .join()
            .expect("gvproxy reaper should observe process exit");
    }

    #[test]
    fn request_krunkit_state_change_sends_hard_stop_payload() {
        let _guard = machine_lifecycle_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let endpoint_path = temp_dir.path().join("krunkit.sock");
        let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let requests_for_server = std::sync::Arc::clone(&requests);
        let request_path = endpoint_path.clone();
        let server = thread::spawn(move || {
            let listener =
                UnixListener::bind(&endpoint_path).expect("endpoint listener should bind");
            let (mut stream, _) = listener.accept().expect("endpoint should accept request");
            let mut buffer = [0_u8; 1024];
            let read = stream.read(&mut buffer).expect("request should read");
            let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
            let state = if request.contains("\"HardStop\"") {
                "HardStop"
            } else {
                "Stop"
            };
            requests_for_server
                .lock()
                .expect("request log should lock")
                .push(state.to_owned());
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .expect("response should write");
            stream.flush().expect("response should flush");
        });

        let deadline = Instant::now() + Duration::from_secs(1);
        while !request_path.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            request_path.exists(),
            "endpoint should appear before request"
        );

        request_krunkit_state_change(&request_path, "HardStop")
            .expect("hard-stop request should succeed");
        server.join().expect("endpoint server should finish");

        assert_eq!(
            requests.lock().expect("request log should lock").clone(),
            vec!["HardStop".to_owned()]
        );
    }

    #[test]
    fn wait_for_pid_exit_reports_timeout_while_process_is_still_running() {
        let _guard = machine_lifecycle_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (pid, reaper) = spawn_reaped_process("exec sleep 30");

        assert!(
            !wait_for_pid_exit(pid, Duration::from_millis(50))
                .expect("wait should report timeout for a running process")
        );

        force_stop_pid(pid, Duration::from_secs(2)).expect("force stop should succeed");
        reaper
            .join()
            .expect("process reaper should observe process exit");
    }

    #[test]
    fn launch_plan_reuses_recorded_managed_ssh_port_when_available() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
        let image_path = temp_dir.path().join("disk.raw");
        fs::write(&image_path, []).expect("image should write");
        let config = sample_config(&image_path);
        let paths = config.roots.paths("default");
        let mut state = MachineStateRecord::initialized();
        state.runtime = Some(MachineRuntimeState {
            helper_binaries: MachineHelperBinaryPaths {
                krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
                gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
            },
            image_path: image_path.clone(),
            efi_variable_store_path: paths.efi_variable_store_path.clone(),
            machine_image_source: describe_machine_image_source(&config.guest.image_source),
            ssh_port: 20022,
            rest_uri: format!("unix://{}", paths.krunkit_endpoint_path.display()),
            ready_vsock_port: READY_VSOCK_PORT,
        });

        let plan =
            MachineLaunchPlan::build(&paths, &config, &state).expect("launch plan should build");
        let allocation_state = load_machine_port_allocation_state(&config.roots)
            .expect("port allocation state should load");

        assert_eq!(plan.runtime.ssh_port, 20022);
        assert_eq!(allocation_state.machine_ports.get("default"), Some(&20022));
    }

    #[test]
    fn launch_plan_reassigns_recorded_ssh_port_when_it_is_busy() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
        let image_path = temp_dir.path().join("disk.raw");
        fs::write(&image_path, []).expect("image should write");
        let config = sample_config(&image_path);
        let paths = config.roots.paths("default");
        let listener = TcpListener::bind("127.0.0.1:20023")
            .or_else(|_| TcpListener::bind("127.0.0.1:0"))
            .expect("listener should bind");
        let busy_port = listener
            .local_addr()
            .expect("listener address should resolve")
            .port();
        let mut state = MachineStateRecord::initialized();
        state.runtime = Some(MachineRuntimeState {
            helper_binaries: MachineHelperBinaryPaths {
                krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
                gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
            },
            image_path: image_path.clone(),
            efi_variable_store_path: paths.efi_variable_store_path.clone(),
            machine_image_source: describe_machine_image_source(&config.guest.image_source),
            ssh_port: busy_port,
            rest_uri: format!("unix://{}", paths.krunkit_endpoint_path.display()),
            ready_vsock_port: READY_VSOCK_PORT,
        });

        let plan =
            MachineLaunchPlan::build(&paths, &config, &state).expect("launch plan should build");
        let allocation_state = load_machine_port_allocation_state(&config.roots)
            .expect("port allocation state should load");

        assert_ne!(plan.runtime.ssh_port, busy_port);
        assert!(managed_machine_port_range_contains(plan.runtime.ssh_port));
        assert_eq!(
            allocation_state.machine_ports.get("default"),
            Some(&plan.runtime.ssh_port)
        );
    }

    #[test]
    fn release_machine_ssh_port_removes_reserved_port() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let roots = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        with_port_allocation_lock(&roots, || {
            let mut state = load_machine_port_allocation_state(&roots)?;
            state.machine_ports.insert("default".to_owned(), 20024);
            write_machine_port_allocation_state(&roots, &state)
        })
        .expect("reserved machine port should write");

        release_machine_ssh_port(&roots, "default").expect("port release should succeed");

        let allocation_state =
            load_machine_port_allocation_state(&roots).expect("allocation state should load");
        assert!(allocation_state.machine_ports.is_empty());
    }

    #[test]
    fn refresh_machine_state_marks_missing_pids_as_stale() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths("default");
        paths
            .ensure_runtime_directories()
            .expect("runtime directories should exist");

        let mut state = MachineStateRecord::initialized();
        state.lifecycle = MachineLifecycle::Running;
        state.manager = MachineManagerState::Ready;
        state.runtime = Some(MachineRuntimeState {
            helper_binaries: MachineHelperBinaryPaths {
                krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
                gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
            },
            image_path: PathBuf::from("/tmp/disk.raw"),
            efi_variable_store_path: paths.efi_variable_store_path.clone(),
            machine_image_source: "docker://quay.io/podman/machine-os@sha256:test".to_owned(),
            ssh_port: 2222,
            rest_uri: format!("unix://{}", paths.krunkit_endpoint_path.display()),
            ready_vsock_port: READY_VSOCK_PORT,
        });

        refresh_machine_state(&paths, &mut state).expect("refresh should succeed");

        assert_eq!(state.lifecycle, MachineLifecycle::Failed);
        assert_eq!(state.manager, MachineManagerState::Stale);
        assert!(
            state
                .last_error
                .expect("stale error should be present")
                .contains("krunkit_alive=false")
        );
    }

    #[test]
    fn ssh_command_requires_running_machine_and_identity() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let image_path = temp_dir.path().join("disk.raw");
        fs::write(&image_path, []).expect("image should write");
        let mut config = sample_config(&image_path);
        config.guest.ssh_identity_path = None;

        let mut state = MachineStateRecord::initialized();
        state.lifecycle = MachineLifecycle::Running;
        state.runtime = Some(MachineRuntimeState {
            helper_binaries: MachineHelperBinaryPaths {
                krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
                gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
            },
            image_path,
            efi_variable_store_path: PathBuf::from("/tmp/efi"),
            machine_image_source: describe_machine_image_source(&config.guest.image_source),
            ssh_port: 2222,
            rest_uri: "unix:///tmp/krunkit.sock".to_owned(),
            ready_vsock_port: READY_VSOCK_PORT,
        });

        let error = build_ssh_command(&config, &state).expect_err("missing identity should fail");
        assert!(error.to_string().contains("no SSH identity configured"));
    }

    #[test]
    fn ssh_command_applies_localhost_machine_safety_options() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let image_path = temp_dir.path().join("disk.raw");
        let identity_path = temp_dir.path().join("machine");
        fs::write(&image_path, []).expect("image should write");
        fs::write(&identity_path, "fake-private-key").expect("identity should write");

        let mut config = sample_config(&image_path);
        config.guest.ssh_identity_path = Some(identity_path.clone());

        let mut state = MachineStateRecord::initialized();
        state.lifecycle = MachineLifecycle::Running;
        state.runtime = Some(MachineRuntimeState {
            helper_binaries: MachineHelperBinaryPaths {
                krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
                gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
            },
            image_path,
            efi_variable_store_path: PathBuf::from("/tmp/efi"),
            machine_image_source: describe_machine_image_source(&config.guest.image_source),
            ssh_port: 2222,
            rest_uri: "unix:///tmp/krunkit.sock".to_owned(),
            ready_vsock_port: READY_VSOCK_PORT,
        });

        let command = build_ssh_command(&config, &state).expect("ssh command should build");
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(
            args.windows(2)
                .any(|window| window == ["-o", "BatchMode=yes"])
        );
        assert!(
            args.windows(2)
                .any(|window| window == ["-o", "StrictHostKeyChecking=no"])
        );
        assert!(
            args.windows(2)
                .any(|window| window == ["-o", "UserKnownHostsFile=/dev/null"])
        );
        assert!(
            args.windows(2)
                .any(|window| window == ["-i", identity_path.to_string_lossy().as_ref()])
        );
        assert!(args.windows(2).any(|window| window == ["-p", "2222"]));
        assert_eq!(args.last().map(String::as_str), Some("core@127.0.0.1"));
    }

    fn serve_single_http_response(body: Vec<u8>, path: Option<&str>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let address = listener
            .local_addr()
            .expect("listener address should resolve");
        let request_path = path.unwrap_or("/disk.raw").to_owned();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("server should accept one request");
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("response header should write");
            stream.write_all(&body).expect("response body should write");
        });
        format!("http://{}:{}{}", address.ip(), address.port(), request_path)
    }

    fn spawn_reaped_process(command: &str) -> (i32, thread::JoinHandle<()>) {
        let mut child = MachineCommandLine {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_owned(), command.to_owned()],
        }
        .spawn()
        .expect("managed process should spawn");
        let pid = child.id() as i32;
        let reaper = thread::spawn(move || {
            let _ = child.wait();
        });
        (pid, reaper)
    }

    fn serve_fake_oci_registry(layer_body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let address = listener
            .local_addr()
            .expect("listener address should resolve");
        let repository = "example/neovex-machine-os";
        let tag = "test";
        let layer_digest = format!("sha256:{:x}", Sha256::digest(&layer_body));
        let current_arch = current_machine_oci_architectures()[0];
        let child_manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": OCI_IMAGE_MEDIA_TYPE,
            "config": {
                "mediaType": "application/vnd.oci.empty.v1+json",
                "size": 2,
                "digest": "sha256:44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
            },
            "layers": [{
                "mediaType": "application/vnd.neovex.machine.disk.layer.v1.tar+gzip",
                "size": layer_body.len(),
                "digest": layer_digest,
                "annotations": {
                    "org.opencontainers.image.title": "neovex-machine-os.raw.gz"
                }
            }]
        });
        let child_manifest_bytes =
            serde_json::to_vec(&child_manifest).expect("child manifest should serialize");
        let child_manifest_digest = format!("sha256:{:x}", Sha256::digest(&child_manifest_bytes));
        let ignored_manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": OCI_IMAGE_MEDIA_TYPE,
            "config": {
                "mediaType": "application/vnd.oci.empty.v1+json",
                "size": 2,
                "digest": "sha256:44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
            },
            "layers": [{
                "mediaType": "application/vnd.neovex.machine.disk.layer.v1.tar+gzip",
                "size": layer_body.len(),
                "digest": layer_digest,
                "annotations": {
                    "org.opencontainers.image.title": "ignored.raw.gz"
                }
            }]
        });
        let ignored_manifest_bytes =
            serde_json::to_vec(&ignored_manifest).expect("ignored manifest should serialize");
        let ignored_manifest_digest =
            format!("sha256:{:x}", Sha256::digest(&ignored_manifest_bytes));
        let index_manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": OCI_IMAGE_INDEX_MEDIA_TYPE,
            "manifests": [
                {
                    "mediaType": OCI_IMAGE_MEDIA_TYPE,
                    "size": ignored_manifest_bytes.len(),
                    "digest": ignored_manifest_digest,
                    "platform": {
                        "architecture": current_arch,
                        "os": OCI_MACHINE_OS
                    },
                    "annotations": {
                        "disktype": "raw"
                    }
                },
                {
                    "mediaType": OCI_IMAGE_MEDIA_TYPE,
                    "size": child_manifest_bytes.len(),
                    "digest": child_manifest_digest,
                    "platform": {
                        "architecture": current_arch,
                        "os": OCI_MACHINE_OS
                    },
                    "annotations": {
                        "disktype": MachineProvider::Krunkit.oci_artifact_disk_type(),
                        "org.opencontainers.image.source": "https://github.com/agentstation/neovex-machine-os",
                        "io.neovex.machine.attestation.repository": "agentstation/neovex-machine-os",
                        "io.neovex.machine.neovex.version": "v1.2.3"
                    }
                }
            ]
        });
        let index_manifest_bytes =
            serde_json::to_vec(&index_manifest).expect("index manifest should serialize");

        thread::spawn(move || {
            for _ in 0..8 {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let mut buffer = [0_u8; 4096];
                let read = stream.read(&mut buffer).expect("request should read");
                let request = String::from_utf8_lossy(&buffer[..read]);
                let mut parts = request
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .split_whitespace();
                let method = parts.next().unwrap_or("GET");
                let path = parts.next().unwrap_or("/");
                let (status, content_type, body) = match path {
                    "/v2/" | "/v2" => (200, "text/plain", Vec::new()),
                    _ if path == format!("/v2/{repository}/manifests/{tag}") => (
                        200,
                        OCI_IMAGE_INDEX_MEDIA_TYPE,
                        index_manifest_bytes.clone(),
                    ),
                    _ if path
                        == format!("/v2/{repository}/manifests/{ignored_manifest_digest}") =>
                    {
                        (200, OCI_IMAGE_MEDIA_TYPE, ignored_manifest_bytes.clone())
                    }
                    _ if path == format!("/v2/{repository}/manifests/{child_manifest_digest}") => {
                        (200, OCI_IMAGE_MEDIA_TYPE, child_manifest_bytes.clone())
                    }
                    _ if path == format!("/v2/{repository}/blobs/{layer_digest}") => {
                        (200, "application/octet-stream", layer_body.clone())
                    }
                    _ => (404, "text/plain", b"not found".to_vec()),
                };
                let status_line = if status == 200 {
                    "HTTP/1.1 200 OK"
                } else {
                    "HTTP/1.1 404 Not Found"
                };
                let response = format!(
                    "{status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("response header should write");
                if method != "HEAD" {
                    stream.write_all(&body).expect("response body should write");
                }
            }
        });

        format!("docker://127.0.0.1:{}/{repository}:{tag}", address.port())
    }

    #[test]
    fn attestation_repository_prefers_explicit_metadata() {
        assert_eq!(
            attestation_repositories_for_reference(
                "agentstation/neovex-machine-os",
                Some("agentstation/neovex")
            ),
            vec!["agentstation/neovex".to_owned()]
        );
    }

    #[test]
    fn attestation_repository_falls_back_to_known_repo_order() {
        assert_eq!(
            attestation_repositories_for_reference("agentstation/neovex-machine-os", None),
            vec![
                "agentstation/neovex-machine-os".to_owned(),
                "agentstation/neovex".to_owned()
            ]
        );
    }

    #[test]
    fn machine_artifact_metadata_uses_primary_then_fallback_annotations() {
        let mut primary = BTreeMap::new();
        primary.insert(
            OCI_ANNOTATION_MACHINE_ATTESTATION_REPOSITORY.to_owned(),
            "agentstation/neovex".to_owned(),
        );
        let mut fallback = BTreeMap::new();
        fallback.insert(
            OCI_ANNOTATION_SOURCE.to_owned(),
            "https://github.com/agentstation/neovex-machine-os".to_owned(),
        );
        fallback.insert(
            OCI_ANNOTATION_MACHINE_NEOVEX_VERSION.to_owned(),
            "v1.2.3".to_owned(),
        );

        let metadata = machine_artifact_metadata_from_annotations(Some(&primary), Some(&fallback));

        assert_eq!(
            metadata.attestation_repository.as_deref(),
            Some("agentstation/neovex")
        );
        assert_eq!(
            metadata.source_repository_url.as_deref(),
            Some("https://github.com/agentstation/neovex-machine-os")
        );
        assert_eq!(metadata.neovex_version.as_deref(), Some("v1.2.3"));
    }
}
