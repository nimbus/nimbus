use std::fs::{self, OpenOptions};
use std::io::{self, Read as _};
use std::os::unix::fs::{
    FileTypeExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
};
use std::os::unix::net::UnixListener;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::State as AxumState;
use axum::routing::get;
use nimbus::Error;
use nimbus_network::NetworkManagementMode;

use super::super::client::MachineApiClient;
use super::super::{
    MachineBootstrapMode, MachineConfigRecord, MachinePaths, machine_bootstrap_mode,
};
use super::launch::MachineLaunchPlan;
use super::ssh::run_silent_ssh_probe;
use super::{
    GVPROXY_SOCKET_WAIT_TIMEOUT, MACHINE_API_FORWARD_USER, POLL_INTERVAL, StartupSignalMonitor,
};

const MACHINE_RUNTIME_DIRECTORY_MODE: u32 = 0o700;
const MACHINE_FORWARDER_SOCKET_MODE: u32 = 0o600;

pub(super) fn secure_machine_runtime_root(paths: &MachinePaths) -> Result<(), Error> {
    secure_machine_runtime_root_for_owner(&paths.runtime_dir, unsafe { libc::geteuid() })
}

pub(super) fn secure_machine_runtime_root_for_owner(
    path: &Path,
    expected_uid: u32,
) -> Result<(), Error> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| {
            Error::PreconditionFailed(format!(
                "machine runtime root {} must be an existing non-symlink directory: {error}",
                path.display()
            ))
        })?;
    let metadata = directory.metadata().map_err(|error| {
        Error::Internal(format!(
            "failed to inspect machine runtime root {}: {error}",
            path.display()
        ))
    })?;
    if !metadata.is_dir() || metadata.uid() != expected_uid {
        return Err(Error::PreconditionFailed(format!(
            "machine runtime root {} must be owned by effective uid {expected_uid}",
            path.display()
        )));
    }
    directory
        .set_permissions(fs::Permissions::from_mode(MACHINE_RUNTIME_DIRECTORY_MODE))
        .map_err(|error| {
            Error::Internal(format!(
                "failed to restrict machine runtime root {} to its owner: {error}",
                path.display()
            ))
        })
}

fn secure_machine_forwarder_services_socket(path: &Path) -> Result<(), Error> {
    secure_machine_forwarder_services_socket_for_owner(path, unsafe { libc::geteuid() })
}

pub(super) fn secure_machine_forwarder_services_socket_for_owner(
    path: &Path,
    expected_uid: u32,
) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        Error::Internal(format!(
            "failed to inspect machine forwarder services socket {}: {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_socket() || metadata.uid() != expected_uid {
        return Err(Error::PreconditionFailed(format!(
            "machine forwarder services endpoint {} must be an owner-controlled Unix socket",
            path.display()
        )));
    }
    fs::set_permissions(
        path,
        fs::Permissions::from_mode(MACHINE_FORWARDER_SOCKET_MODE),
    )
    .map_err(|error| {
        Error::Internal(format!(
            "failed to restrict machine forwarder services socket {} to its owner: {error}",
            path.display()
        ))
    })?;
    let secured = fs::symlink_metadata(path).map_err(|error| {
        Error::Internal(format!(
            "failed to re-inspect machine forwarder services socket {}: {error}",
            path.display()
        ))
    })?;
    if !secured.file_type().is_socket()
        || secured.uid() != expected_uid
        || secured.mode() & 0o777 != MACHINE_FORWARDER_SOCKET_MODE
    {
        return Err(Error::PreconditionFailed(format!(
            "machine forwarder services endpoint {} changed while it was secured",
            path.display()
        )));
    }
    Ok(())
}

pub(super) fn wait_for_machine_api_ready(
    paths: &MachinePaths,
    timeout: Duration,
    vmm_child: &mut Child,
    gvproxy_child: &mut Child,
    api_forward_child: &mut Child,
    startup_signals: &StartupSignalMonitor,
) -> Result<(), Error> {
    let deadline = Instant::now() + timeout;
    let client = MachineApiClient::new(paths.api_socket_path.clone());
    loop {
        startup_signals.check()?;
        if let Some(status) = vmm_child.try_wait().map_err(|error| {
            Error::Internal(format!("failed to poll machine VMM process state: {error}"))
        })? {
            return Err(Error::Internal(format!(
                "the machine VMM exited before machine API readiness with status {status}"
            )));
        }
        if let Some(status) = gvproxy_child.try_wait().map_err(|error| {
            Error::Internal(format!("failed to poll gvproxy process state: {error}"))
        })? {
            return Err(Error::Internal(format!(
                "gvproxy exited before machine API readiness with status {status}"
            )));
        }
        if let Some(status) = api_forward_child.try_wait().map_err(|error| {
            Error::Internal(format!(
                "failed to poll machine API forward process state: {error}"
            ))
        })? {
            return Err(Error::Internal(format!(
                "machine API forward exited before machine API readiness with status {status}"
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

pub(super) fn resolve_machine_api_ready_wait_timeout() -> Duration {
    let seconds = env_parse_u64(super::MACHINE_API_READY_WAIT_TIMEOUT_ENV)
        .unwrap_or(super::DEFAULT_MACHINE_API_READY_TIMEOUT.as_secs());
    Duration::from_secs(seconds.max(1))
}

pub(super) fn start_bootstrap_server(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    launch_plan: &MachineLaunchPlan,
) -> Result<Option<thread::JoinHandle<()>>, Error> {
    if machine_bootstrap_mode(config) != MachineBootstrapMode::Ignition {
        return Ok(None);
    }
    match launch_plan.ignition_file_path.as_ref() {
        Some(path) => serve_ignition_file(&paths.ignition_socket_path, path).map(Some),
        None => Ok(None),
    }
}

pub(super) fn pre_start_networking(
    paths: &MachinePaths,
    launch_plan: &MachineLaunchPlan,
    gvproxy_child: &mut Option<Child>,
    startup_signals: &StartupSignalMonitor,
) -> Result<(), Error> {
    // Store the child in the caller's slot before awaiting readiness so a
    // `wait_for_path` failure still hands the spawned gvproxy back through the
    // start-error cleanup path to be reaped, rather than dropping it un-waited.
    let child = gvproxy_child.insert(launch_plan.gvproxy_command.spawn()?);
    let receipt = super::process_identity::GvproxyProcessReceipt::capture(
        child.id(),
        &launch_plan.runtime().forwarder_authority,
    )?;
    super::write_json_file(&paths.gvproxy_process_identity_path, &receipt)?;
    wait_for_path(
        &paths.gvproxy_socket_path,
        GVPROXY_SOCKET_WAIT_TIMEOUT,
        child,
        startup_signals,
    )?;
    wait_for_path(
        &paths.gvproxy_services_socket_path(),
        GVPROXY_SOCKET_WAIT_TIMEOUT,
        child,
        startup_signals,
    )?;
    secure_machine_forwarder_services_socket(&paths.gvproxy_services_socket_path())?;
    Ok(())
}

pub(super) fn start_vm(
    launch_plan: &MachineLaunchPlan,
    vmm_child: &mut Option<Child>,
) -> Result<(), Error> {
    // The provider was already gated to a supported VMM backend when the launch
    // plan was built, so booting is provider-agnostic: spawn the resolved VMM
    // command the backend assembled.
    *vmm_child = Some(launch_plan.vmm_command.spawn()?);
    Ok(())
}

pub(super) fn wait_for_machine_ready(
    config: &MachineConfigRecord,
    ready_listener: &UnixListener,
    vmm_child: &mut Option<Child>,
    gvproxy_child: &mut Option<Child>,
    startup_signals: &StartupSignalMonitor,
) -> Result<(), Error> {
    match machine_bootstrap_mode(config) {
        MachineBootstrapMode::Ignition | MachineBootstrapMode::BootcMachineConfig => {
            wait_for_ready(
                ready_listener,
                resolve_ready_wait_timeout(),
                required_child(vmm_child, "machine VMM")?,
                required_child(gvproxy_child, "gvproxy")?,
                startup_signals,
            )
        }
        MachineBootstrapMode::ShellScript => Ok(()),
    }
}

pub(super) fn post_start_networking(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    ssh_port: u16,
    api_forward_child: &mut Option<Child>,
    startup_signals: &StartupSignalMonitor,
) -> Result<(), Error> {
    match config.provider.network_management_mode() {
        NetworkManagementMode::NimbusHostManaged => {
            start_machine_api_forward(paths, config, ssh_port, api_forward_child, startup_signals)
        }
        NetworkManagementMode::ProviderManaged => Err(config.provider.unavailable_error()),
    }
}

pub(super) fn conduct_readiness_check(
    config: &MachineConfigRecord,
    ssh_port: u16,
    vmm_child: &mut Option<Child>,
    gvproxy_child: &mut Option<Child>,
    startup_signals: &StartupSignalMonitor,
) -> Result<(), Error> {
    // Every gvproxy-backed macOS VMM shares the SSH-on-localhost readiness gate;
    // the launch plan already rejected providers without a supported backend.
    wait_for_ssh_ready(
        config,
        ssh_port,
        resolve_ssh_ready_wait_timeout(),
        required_child(vmm_child, "machine VMM")?,
        required_child(gvproxy_child, "gvproxy")?,
        startup_signals,
    )
}

pub(super) fn bind_ready_listener(path: &Path) -> Result<UnixListener, Error> {
    super::remove_file_if_exists(path)?;
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
    super::remove_file_if_exists(socket_path)?;
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

pub(super) fn required_child<'a>(
    child: &'a mut Option<Child>,
    label: &str,
) -> Result<&'a mut Child, Error> {
    child.as_mut().ok_or_else(|| {
        Error::Internal(format!(
            "machine startup phase expected a running {label} helper, but none was recorded"
        ))
    })
}

fn wait_for_ready(
    listener: &UnixListener,
    timeout: Duration,
    vmm_child: &mut Child,
    gvproxy_child: &mut Child,
    startup_signals: &StartupSignalMonitor,
) -> Result<(), Error> {
    let deadline = Instant::now() + timeout;
    loop {
        startup_signals.check()?;
        if let Some(status) = vmm_child.try_wait().map_err(|error| {
            Error::Internal(format!("failed to poll machine VMM process state: {error}"))
        })? {
            return Err(Error::Internal(format!(
                "the machine VMM exited before machine readiness with status {status}"
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

pub(super) fn wait_for_ssh_ready(
    config: &MachineConfigRecord,
    ssh_port: u16,
    timeout: Duration,
    vmm_child: &mut Child,
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
        if let Some(status) = vmm_child.try_wait().map_err(|error| {
            Error::Internal(format!("failed to poll machine VMM process state: {error}"))
        })? {
            return Err(Error::Internal(format!(
                "the machine VMM exited before SSH readiness with status {status}"
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

pub(super) fn ssh_port_is_listening(ssh_port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
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

pub(super) fn build_machine_api_forward_command(
    paths: &MachinePaths,
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
    command
        .arg("-N")
        .arg("-L")
        .arg(format!(
            "{}:{}",
            paths.api_socket_path.display(),
            super::super::bootstrap::GUEST_NIMBUS_SOCKET
        ))
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ExitOnForwardFailure=yes")
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
        .arg("-o")
        .arg("StreamLocalBindUnlink=yes")
        .arg("-i")
        .arg(identity_path)
        .arg("-p")
        .arg(ssh_port.to_string())
        .arg(format!("{MACHINE_API_FORWARD_USER}@127.0.0.1"));
    Ok(command)
}

fn start_machine_api_forward(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    ssh_port: u16,
    api_forward_child: &mut Option<Child>,
    startup_signals: &StartupSignalMonitor,
) -> Result<(), Error> {
    super::remove_file_if_exists(&paths.api_socket_path)?;
    super::remove_file_if_exists(&paths.api_forward_pid_path)?;
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.api_forward_log_path)
        .map_err(|error| {
            Error::Internal(format!(
                "failed to open machine API forward log {}: {error}",
                paths.api_forward_log_path.display()
            ))
        })?;
    let stderr = log_file.try_clone().map_err(|error| {
        Error::Internal(format!(
            "failed to clone machine API forward log {}: {error}",
            paths.api_forward_log_path.display()
        ))
    })?;
    let mut command = build_machine_api_forward_command(paths, config, ssh_port)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(stderr));
    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command.spawn().map_err(|error| {
        Error::Internal(format!(
            "failed to start machine API SSH forward for '{}': {error}",
            config.name
        ))
    })?;
    fs::write(&paths.api_forward_pid_path, child.id().to_string()).map_err(|error| {
        Error::Internal(format!(
            "failed to write machine API forward pid {}: {error}",
            paths.api_forward_pid_path.display()
        ))
    })?;
    *api_forward_child = Some(child);
    wait_for_path(
        &paths.api_socket_path,
        GVPROXY_SOCKET_WAIT_TIMEOUT,
        required_child(api_forward_child, "machine API forward")?,
        startup_signals,
    )
}

fn resolve_ready_wait_timeout() -> Duration {
    let seconds = env_parse_u64(super::READY_WAIT_TIMEOUT_ENV)
        .unwrap_or(super::DEFAULT_READY_WAIT_TIMEOUT.as_secs());
    Duration::from_secs(seconds.max(1))
}

fn resolve_ssh_ready_wait_timeout() -> Duration {
    let seconds = env_parse_u64(super::SSH_READY_WAIT_TIMEOUT_ENV)
        .unwrap_or(super::DEFAULT_SSH_READY_WAIT_TIMEOUT.as_secs());
    Duration::from_secs(seconds.max(1))
}

fn env_parse_u64(name: &str) -> Option<u64> {
    std::env::var(name).ok()?.trim().parse().ok()
}

pub(super) fn wait_for_path(
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
