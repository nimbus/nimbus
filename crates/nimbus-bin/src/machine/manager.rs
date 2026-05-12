use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use nimbus::Error;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use signal_hook_registry::{SigId, register as register_signal, unregister as unregister_signal};

use crate::cli_ux;

mod guest;
mod helpers;
mod image;
mod launch;
mod ports;
mod readiness;
mod ssh;
mod stop;

#[cfg(test)]
pub(crate) use self::helpers::MachineHelperEnvGuard;
use self::launch::MachineLaunchPlan;
use self::readiness::{
    bind_ready_listener, conduct_readiness_check, post_start_networking, pre_start_networking,
    start_bootstrap_server, start_vm, wait_for_machine_ready,
};
use self::stop::{cleanup_runtime_artifacts, handle_start_machine_error, remove_file_if_exists};

use super::{
    MachineConfigRecord, MachineLifecycle, MachineManagerState, MachinePaths, MachineRootLayout,
    MachineStateRecord, write_json_file,
};

const DEFAULT_KRUNKIT_BINARY: &str = "krunkit";
const DEFAULT_GVPROXY_BINARY: &str = "gvproxy";
const DEFAULT_MACHINE_MAC_ADDRESS: &str = "5a:94:ef:e4:0c:ee";
const READY_VSOCK_PORT: u32 = 1025;
const READY_WAIT_TIMEOUT_ENV: &str = "NIMBUS_MACHINE_READY_TIMEOUT_SECS";
const DEFAULT_READY_WAIT_TIMEOUT: Duration = Duration::from_secs(90);
const SSH_READY_WAIT_TIMEOUT_ENV: &str = "NIMBUS_MACHINE_SSH_READY_TIMEOUT_SECS";
const DEFAULT_SSH_READY_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const MACHINE_API_READY_WAIT_TIMEOUT_ENV: &str = "NIMBUS_MACHINE_API_READY_TIMEOUT_SECS";
const DEFAULT_MACHINE_API_READY_TIMEOUT: Duration = Duration::from_secs(30);
const STOP_WAIT_TIMEOUT_ENV: &str = "NIMBUS_MACHINE_STOP_TIMEOUT_SECS";
const DEFAULT_STOP_WAIT_TIMEOUT: Duration = Duration::from_secs(90);
const GVPROXY_SOCKET_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const HARD_STOP_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(200);
const MACHINE_PORT_MIN: u16 = 10000;
const MACHINE_PORT_MAX: u16 = 65535;
const KRUNKIT_ENV: &str = "NIMBUS_MACHINE_KRUNKIT";
const GVPROXY_ENV: &str = "NIMBUS_MACHINE_GVPROXY";
const HELPER_BINARY_DIR_ENV: &str = "NIMBUS_MACHINE_HELPER_BINARY_DIR";
const HTTP_IMAGE_TIMEOUT: Duration = Duration::from_secs(300);
const GUEST_NIMBUS_BINARY_OVERRIDE_ENV: &str = "NIMBUS_MACHINE_GUEST_BINARY";
const GUEST_NIMBUS_RELEASE_BASE_URL_ENV: &str = "NIMBUS_MACHINE_GUEST_RELEASE_BASE_URL";
const DEFAULT_GUEST_NIMBUS_RELEASE_BASE_URL: &str =
    "https://github.com/nimbus/nimbus/releases/download";
const DEFAULT_GUEST_NIMBUS_BINARY_ARCHIVE_NAME_ARM64: &str = "nimbus_linux_arm64.tar.gz";
const DEFAULT_GUEST_NIMBUS_BINARY_ARCHIVE_NAME_X86_64: &str = "nimbus_linux_x86_64.tar.gz";
const LOCAL_GUEST_BINARY_HELP_TEXT: &str =
    "set `NIMBUS_MACHINE_GUEST_BINARY` to an explicit local Linux guest binary override";
const OCI_MACHINE_OS: &str = "linux";
const OCI_ANNOTATION_TITLE: &str = "org.opencontainers.image.title";
const OCI_ANNOTATION_SOURCE: &str = "org.opencontainers.image.source";
const OCI_ANNOTATION_MACHINE_ATTESTATION_REPOSITORY: &str =
    "io.nimbus.machine.attestation.repository";
const OCI_ANNOTATION_MACHINE_NIMBUS_VERSION: &str = "io.nimbus.machine.nimbus.version";
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum GuestNimbusBinarySourceKind {
    ReleaseAsset,
    ExplicitOverride,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct DesiredGuestNimbusBinaryStatus {
    pub(super) install_path: PathBuf,
    pub(super) source: GuestNimbusBinarySourceKind,
    pub(super) source_detail: String,
    pub(super) desired_path: PathBuf,
    pub(super) desired_exists: bool,
    pub(super) desired_version: Option<String>,
    pub(super) desired_hash: Option<String>,
    pub(super) release_archive_path: Option<PathBuf>,
    pub(super) release_archive_exists: Option<bool>,
    pub(super) release_url: Option<String>,
    pub(super) error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct ObservedGuestNimbusBinaryStatus {
    pub(super) version: Option<String>,
    pub(super) hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MachineHelperBinaryPaths {
    pub(super) krunkit: PathBuf,
    pub(super) gvproxy: PathBuf,
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

fn emit_machine_progress(message: impl AsRef<str>) {
    let _ = cli_ux::emit_phase(message.as_ref());
}

fn emit_machine_info(message: impl AsRef<str>) {
    if cli_ux::info_output_enabled() {
        let _ = cli_ux::write_stderr_prefixed_line("info:", message.as_ref());
    }
}

fn emit_machine_warning(message: impl AsRef<str>) {
    let _ = cli_ux::write_stderr_prefixed_line("warning:", message.as_ref());
}

pub(super) fn start_machine(
    paths: &MachinePaths,
    config: &mut MachineConfigRecord,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    emit_machine_progress(format!("Starting machine \"{}\"", config.name));
    ensure_machine_can_start(paths, config, state)?;
    converge_machine_image_contract(paths, config, state)?;
    ensure_machine_bootstrap_identity(paths, config)?;
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
    emit_machine_progress("Starting machine networking");
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
    emit_machine_progress("Booting virtual machine");
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
    emit_machine_progress("Waiting for guest boot");
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
    emit_machine_progress("Waiting for guest SSH");
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

fn ensure_machine_bootstrap_identity(
    paths: &MachinePaths,
    config: &mut MachineConfigRecord,
) -> Result<(), Error> {
    self::guest::ensure_machine_bootstrap_identity(paths, config)
}

fn converge_machine_image_contract(
    paths: &MachinePaths,
    config: &mut MachineConfigRecord,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    self::guest::converge_machine_image_contract(paths, config, state)
}

#[cfg(test)]
fn machine_image_rebuild_reason(
    paths: &MachinePaths,
    state: &MachineStateRecord,
    desired_image: &str,
) -> Option<String> {
    self::guest::machine_image_rebuild_reason(paths, state, desired_image)
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
    self::guest::validate_machine_bootstrap_contract(config)
}

#[cfg(test)]
fn requires_host_guest_nimbus_sync(config: &MachineConfigRecord) -> bool {
    self::guest::requires_host_guest_nimbus_sync(config)
}

fn ensure_guest_machine_api_ready(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    ssh_port: u16,
    krunkit_child: &mut Option<Child>,
    gvproxy_child: &mut Option<Child>,
    startup_signals: &StartupSignalMonitor,
) -> Result<(), Error> {
    self::guest::ensure_guest_machine_api_ready(
        paths,
        config,
        ssh_port,
        krunkit_child,
        gvproxy_child,
        startup_signals,
    )
}

pub(super) fn inspect_desired_guest_nimbus_binary(
    paths: &MachinePaths,
) -> DesiredGuestNimbusBinaryStatus {
    self::guest::inspect_desired_guest_nimbus_binary(paths)
}

pub(super) fn inspect_observed_guest_nimbus_binary(
    config: &MachineConfigRecord,
    state: &MachineStateRecord,
) -> Result<ObservedGuestNimbusBinaryStatus, Error> {
    self::guest::inspect_observed_guest_nimbus_binary(config, state)
}

#[cfg(test)]
fn resolve_guest_nimbus_binary(paths: &MachinePaths) -> Result<PathBuf, Error> {
    self::guest::resolve_guest_nimbus_binary(paths)
}

pub(super) fn stop_machine(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    self::stop::stop_machine(paths, config, state)
}

pub(super) fn release_machine_ssh_port(
    roots: &MachineRootLayout,
    machine_name: &str,
) -> Result<(), Error> {
    self::ports::release_machine_ssh_port(roots, machine_name)
}

pub(super) fn refresh_machine_state(
    paths: &MachinePaths,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    self::stop::refresh_machine_state(paths, state)
}

pub(super) fn build_ssh_command(
    config: &MachineConfigRecord,
    state: &MachineStateRecord,
) -> Result<Command, Error> {
    self::ssh::build_ssh_command(config, state)
}

pub(super) fn build_scp_command(
    config: &MachineConfigRecord,
    state: &MachineStateRecord,
    guest_is_src: bool,
    guest_path: &str,
    host_path: &str,
) -> Result<Command, Error> {
    self::ssh::build_scp_command(config, state, guest_is_src, guest_path, host_path)
}

pub(super) fn mount_tag(target: &Path) -> String {
    let digest = Sha256::digest(target.as_os_str().as_encoded_bytes());
    format!("{digest:x}")[..36].to_owned()
}

#[cfg(test)]
mod tests;
