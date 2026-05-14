use std::fs;
use std::io::{self, Read as _};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::Child;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::State as AxumState;
use axum::routing::get;
use nimbus::Error;

use super::super::client::MachineApiClient;
use super::super::{
    MachineBootstrapMode, MachineConfigRecord, MachinePaths, MachineProvider,
    machine_bootstrap_mode,
};
use super::launch::MachineLaunchPlan;
use super::ssh::run_silent_ssh_probe;
use super::{GVPROXY_SOCKET_WAIT_TIMEOUT, POLL_INTERVAL, StartupSignalMonitor};

pub(super) fn wait_for_machine_api_ready(
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

pub(super) fn start_vm(
    config: &MachineConfigRecord,
    launch_plan: &MachineLaunchPlan,
    krunkit_child: &mut Option<Child>,
) -> Result<(), Error> {
    match config.provider {
        MachineProvider::Krunkit => {
            *krunkit_child = Some(launch_plan.krunkit_command.spawn()?);
            Ok(())
        }
        MachineProvider::Wsl2 => Err(Error::InvalidInput(
            "the WSL2 machine provider is not available on this host yet".to_owned(),
        )),
    }
}

pub(super) fn wait_for_machine_ready(
    config: &MachineConfigRecord,
    ready_listener: &UnixListener,
    krunkit_child: &mut Option<Child>,
    gvproxy_child: &mut Option<Child>,
    startup_signals: &StartupSignalMonitor,
) -> Result<(), Error> {
    match machine_bootstrap_mode(config) {
        MachineBootstrapMode::Ignition | MachineBootstrapMode::BootcMachineConfig => {
            wait_for_ready(
                ready_listener,
                resolve_ready_wait_timeout(),
                required_child(krunkit_child, "krunkit")?,
                required_child(gvproxy_child, "gvproxy")?,
                startup_signals,
            )
        }
        MachineBootstrapMode::ShellScript => Ok(()),
    }
}

pub(super) fn post_start_networking(
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

pub(super) fn conduct_readiness_check(
    config: &MachineConfigRecord,
    ssh_port: u16,
    krunkit_child: &mut Option<Child>,
    gvproxy_child: &mut Option<Child>,
    startup_signals: &StartupSignalMonitor,
) -> Result<(), Error> {
    match config.provider {
        MachineProvider::Krunkit => wait_for_ssh_ready(
            config,
            ssh_port,
            resolve_ssh_ready_wait_timeout(),
            required_child(krunkit_child, "krunkit")?,
            required_child(gvproxy_child, "gvproxy")?,
            startup_signals,
        ),
        MachineProvider::Wsl2 => Err(Error::InvalidInput(
            "the WSL2 machine provider is not available on this host yet".to_owned(),
        )),
    }
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

pub(super) fn wait_for_ssh_ready(
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
