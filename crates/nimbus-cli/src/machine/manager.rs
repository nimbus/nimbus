use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use nimbus::Error;
use nimbus_network::LocalPortLeaseAuthority;
use serde::Serialize;
use sha2::{Digest, Sha256};
use signal_hook_registry::{SigId, register as register_signal, unregister as unregister_signal};

use crate::cli_ux;

mod guest;
#[cfg(test)]
mod helper_env_guard;
mod helper_paths;
mod image;
mod launch;
mod ports;
mod process_identity;
mod readiness;
mod ssh;
mod stop;
mod vmm;

#[cfg(test)]
pub(crate) use self::helper_env_guard::MachineHelperEnvGuard;
use self::launch::MachineLaunchPlan;
use self::readiness::{
    bind_ready_listener, conduct_readiness_check, post_start_networking, pre_start_networking,
    secure_machine_runtime_root, start_bootstrap_server, start_vm, wait_for_machine_ready,
};
use self::stop::{cleanup_runtime_artifacts, handle_start_machine_error, remove_file_if_exists};

pub(super) use super::record::{MachineHelperBinaryPaths, MachineRuntimeState};
use super::{
    MachineConfigRecord, MachineLifecycle, MachineManagerState, MachinePaths, MachineStateRecord,
    write_json_file,
};

const DEFAULT_KRUNKIT_BINARY: &str = "krunkit";
const DEFAULT_VFKIT_BINARY: &str = "vfkit";
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
const VFKIT_ENV: &str = "NIMBUS_MACHINE_VFKIT";
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
// The known (Homebrew/Podman) fallback tier of the macOS helper-binary search
// for `krunkit` and `gvproxy`. This is *not* the whole search order:
// `resolve_helper_binary` consults the per-helper env override and the bundled
// `libexec` copies first, and only falls through to these directories when
// neither resolves.
//
// Within this tier the Homebrew prefix `bin` directories rank first: that is
// where the Nimbus cask's *declared* `krunkit` dependency (and its own
// `gvproxy` dependency) land. Preferring them keeps the managed, version-pinned
// helpers authoritative so an incidental `podman` install can never silently
// shadow the dependency the cask actually declares. The Podman `libexec`
// directories remain below them for hosts that only ship Podman's bundled
// helpers.
const PODMAN_DARWIN_HELPER_DIRECTORIES: &[&str] = &[
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/usr/local/opt/podman/libexec/podman",
    "/opt/homebrew/opt/podman/libexec/podman",
    "/opt/homebrew/libexec/podman",
    "/usr/local/libexec/podman",
    "/usr/local/lib/podman",
    "/usr/libexec/podman",
    "/usr/lib/podman",
];

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
            // SAFETY: `signal-hook-registry` requires the callback to be
            // signal-safe. This handler only performs an atomic store, and the
            // returned registration id is retained so Drop can unregister it
            // when startup monitoring ends.
            let registration = unsafe {
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
    network: &super::network_composition::HostMachineNetworkAuthority,
    paths: &MachinePaths,
    config: &mut MachineConfigRecord,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    start_machine_with_lifecycle(&network.lifecycle_handle()?, paths, config, state)
}

pub(super) fn next_machine_forwarder_authority(
    config: &MachineConfigRecord,
    state: &MachineStateRecord,
) -> Result<nimbus_machine::MachineForwarderAuthority, Error> {
    self::launch::next_machine_forwarder_authority(config, state)
}

/// Start only if the caller's already-prepared forwarder authority is still
/// the exact next authority for this locked config and state.
///
/// The first check precedes opening the lifecycle publication store. The
/// lifecycle implementation checks again before image/bootstrap preparation
/// and verifies the built launch plan before any provider process starts.
pub(super) fn start_machine_with_expected_forwarder_authority(
    network: &super::network_composition::HostMachineNetworkAuthority,
    paths: &MachinePaths,
    config: &mut MachineConfigRecord,
    state: &mut MachineStateRecord,
    expected: &nimbus_machine::MachineForwarderAuthority,
) -> Result<(), Error> {
    authenticate_expected_forwarder_authority(config, state, expected)?;
    start_machine_with_lifecycle_and_expected(
        &network.lifecycle_handle()?,
        paths,
        config,
        state,
        Some(expected),
    )
}

pub(super) fn start_machine_with_lifecycle(
    network: &super::network_composition::MachineNetworkLifecycleHandle,
    paths: &MachinePaths,
    config: &mut MachineConfigRecord,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    start_machine_with_lifecycle_and_expected(network, paths, config, state, None)
}

fn start_machine_with_lifecycle_and_expected(
    network: &super::network_composition::MachineNetworkLifecycleHandle,
    paths: &MachinePaths,
    config: &mut MachineConfigRecord,
    state: &mut MachineStateRecord,
    expected: Option<&nimbus_machine::MachineForwarderAuthority>,
) -> Result<(), Error> {
    if let Some(expected) = expected {
        authenticate_expected_forwarder_authority(config, state, expected)?;
    }
    emit_machine_progress(format!("Starting machine \"{}\"", config.name));
    ensure_machine_can_start(paths, config, state)?;
    super::publication_authority::ensure_no_fenced_machine_publications(
        &network.machine_publications(),
        config.network_authority.provider_instance(),
    )?;
    converge_machine_image_contract(paths, config, state)?;
    ensure_machine_bootstrap_identity(paths, config)?;
    validate_machine_bootstrap_contract(config)?;
    secure_machine_runtime_root(paths)?;

    let startup_signals = StartupSignalMonitor::install()?;
    cleanup_runtime_artifacts(paths)?;
    let port_authority = network.port_leases();
    let launch_plan = MachineLaunchPlan::build(&port_authority, paths, config, state)?;
    if let Some(expected) = expected
        && launch_plan.runtime().forwarder_authority != *expected
    {
        return Err(with_pre_provider_lease_cleanup(
            &launch_plan,
            Error::conflict(format!(
                "machine '{}' launch authority changed after exact preparation",
                config.name
            )),
        ));
    }

    state.lifecycle = MachineLifecycle::Starting;
    state.manager = MachineManagerState::Launching;
    state.runtime = Some(launch_plan.runtime().clone());
    state.last_error = None;
    if let Err(error) = write_json_file(&paths.state_path, state) {
        return Err(with_pre_provider_lease_cleanup(&launch_plan, error));
    }

    let ready_listener = match bind_ready_listener(&paths.ready_socket_path) {
        Ok(listener) => listener,
        Err(error) => {
            return handle_start_machine_error(
                paths,
                config,
                state,
                with_pre_provider_lease_cleanup(&launch_plan, error),
                None,
                None,
                None,
            );
        }
    };
    let _ignition_server = match start_bootstrap_server(paths, config, &launch_plan) {
        Ok(server) => server,
        Err(error) => {
            return handle_start_machine_error(
                paths,
                config,
                state,
                with_pre_provider_lease_cleanup(&launch_plan, error),
                None,
                None,
                None,
            );
        }
    };

    let mut gvproxy_child = None;
    let mut api_forward_child = None;
    emit_machine_progress("Starting machine networking");
    if let Err(error) =
        pre_start_networking(paths, &launch_plan, &mut gvproxy_child, &startup_signals)
    {
        let error = if gvproxy_child.is_none() {
            with_pre_provider_lease_cleanup(&launch_plan, error)
        } else {
            error
        };
        return handle_start_machine_error(
            paths,
            config,
            state,
            error,
            None,
            gvproxy_child.as_mut(),
            api_forward_child.as_mut(),
        );
    }

    let mut vmm_child = None;
    emit_machine_progress("Booting virtual machine");
    if let Err(error) = start_vm(&launch_plan, &mut vmm_child) {
        return handle_start_machine_error(
            paths,
            config,
            state,
            error,
            vmm_child.as_mut(),
            gvproxy_child.as_mut(),
            api_forward_child.as_mut(),
        );
    }
    emit_machine_progress("Waiting for guest boot");
    if let Err(error) = wait_for_machine_ready(
        config,
        &ready_listener,
        &mut vmm_child,
        &mut gvproxy_child,
        &startup_signals,
    ) {
        return handle_start_machine_error(
            paths,
            config,
            state,
            error,
            vmm_child.as_mut(),
            gvproxy_child.as_mut(),
            api_forward_child.as_mut(),
        );
    }
    emit_machine_progress("Waiting for guest SSH");
    if let Err(error) = conduct_readiness_check(
        config,
        launch_plan.runtime().ssh_port,
        &mut vmm_child,
        &mut gvproxy_child,
        &startup_signals,
    ) {
        return handle_start_machine_error(
            paths,
            config,
            state,
            error,
            vmm_child.as_mut(),
            gvproxy_child.as_mut(),
            api_forward_child.as_mut(),
        );
    }
    if let Err(error) = launch_plan.ssh_port_lease().activate_exact_loopback() {
        return handle_start_machine_error(
            paths,
            config,
            state,
            error,
            vmm_child.as_mut(),
            gvproxy_child.as_mut(),
            api_forward_child.as_mut(),
        );
    }
    if let Err(error) = post_start_networking(
        paths,
        config,
        launch_plan.runtime().ssh_port,
        &mut api_forward_child,
        &startup_signals,
    ) {
        return handle_start_machine_error(
            paths,
            config,
            state,
            error,
            vmm_child.as_mut(),
            gvproxy_child.as_mut(),
            api_forward_child.as_mut(),
        );
    }
    if let Err(error) = self::guest::ensure_guest_machine_api_ready(
        paths,
        config,
        &launch_plan.runtime().forwarder_authority,
        launch_plan.runtime().ssh_port,
        self::guest::GuestMachineApiProcesses {
            vmm: &mut vmm_child,
            gvproxy: &mut gvproxy_child,
            api_forward: &mut api_forward_child,
        },
        &startup_signals,
    ) {
        return handle_start_machine_error(
            paths,
            config,
            state,
            error,
            vmm_child.as_mut(),
            gvproxy_child.as_mut(),
            api_forward_child.as_mut(),
        );
    }

    state.lifecycle = MachineLifecycle::Running;
    state.manager = MachineManagerState::Ready;
    state.last_error = None;
    write_json_file(&paths.state_path, state)?;
    Ok(())
}

fn authenticate_expected_forwarder_authority(
    config: &MachineConfigRecord,
    state: &MachineStateRecord,
    expected: &nimbus_machine::MachineForwarderAuthority,
) -> Result<(), Error> {
    let current = self::launch::next_machine_forwarder_authority(config, state)?;
    current
        .authenticate(expected)
        .map_err(|error| Error::PreconditionFailed(error.to_string()))
}

fn with_pre_provider_lease_cleanup(launch_plan: &MachineLaunchPlan, primary: Error) -> Error {
    match launch_plan.ssh_port_lease().abandon_before_provider_start() {
        Ok(()) => primary,
        Err(cleanup) => Error::Internal(format!(
            "{primary}; failed to settle the machine SSH lease before gvproxy started: {cleanup}"
        )),
    }
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
        return Err(Error::conflict(format!(
            "machine '{}' is already {}{}",
            paths.name,
            state.lifecycle.as_str(),
            exclusivity_note
        )));
    }
    ensure_no_external_machine_collision(paths)?;
    Ok(())
}

fn ensure_no_external_machine_collision(paths: &MachinePaths) -> Result<(), Error> {
    let vmm_owner = self::stop::read_pid_if_alive(&paths.vmm_pid_path)?;
    let gvproxy_owner = self::stop::read_pid_if_alive(&paths.gvproxy_pid_path)?;
    let api_forward_owner = self::stop::read_pid_if_alive(&paths.api_forward_pid_path)?;
    let owners: Vec<(&str, i32, &Path)> = vmm_owner
        .into_iter()
        .map(|pid| ("machine-vmm", pid, paths.vmm_pid_path.as_path()))
        .chain(
            gvproxy_owner
                .into_iter()
                .map(|pid| ("gvproxy", pid, paths.gvproxy_pid_path.as_path())),
        )
        .chain(api_forward_owner.into_iter().map(|pid| {
            (
                "machine-api-forward",
                pid,
                paths.api_forward_pid_path.as_path(),
            )
        }))
        .collect();
    if owners.is_empty() {
        return Ok(());
    }
    let summary = owners
        .iter()
        .map(|(label, pid, path)| format!("{label} pid {pid} at {}", path.display()))
        .collect::<Vec<_>>()
        .join(", ");
    Err(Error::conflict(format!(
        "machine '{}' cannot start: another process owns the runtime sockets ({summary}). \
         Stop the other machine, or set {} to a separate path before starting.",
        paths.name,
        nimbus_machine::MACHINE_RUNTIME_ROOT_ENV,
    )))
}

fn validate_machine_bootstrap_contract(config: &MachineConfigRecord) -> Result<(), Error> {
    self::guest::validate_machine_bootstrap_contract(config)
}

#[cfg(test)]
fn requires_host_guest_nimbus_sync(config: &MachineConfigRecord) -> bool {
    self::guest::requires_host_guest_nimbus_sync(config)
}

#[cfg(test)]
fn requires_bootc_machine_config(config: &MachineConfigRecord) -> bool {
    self::guest::requires_bootc_machine_config(config)
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
    network: &super::network_composition::HostMachineNetworkAuthority,
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    self::stop::stop_machine(&network.lifecycle_handle()?, paths, config, state)
}

pub(super) fn release_machine_ssh_port(
    port_authority: &LocalPortLeaseAuthority,
    state: &MachineStateRecord,
) -> Result<(), Error> {
    self::ports::release_machine_ssh_port(port_authority, state)
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
