use std::fs;
use std::io::{self, Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Child;
use std::thread;
use std::time::{Duration, Instant};

use libc::{SIGKILL, SIGTERM, kill};
use nimbus::Error;

use super::super::{
    MachineBootstrapMode, MachineConfigRecord, MachineLifecycle, MachinePaths, MachineProvider,
    MachineStateRecord, machine_bootstrap_mode,
};
use super::{HARD_STOP_WAIT_TIMEOUT, MachineManagerState, POLL_INTERVAL};

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
    super::write_json_file(&paths.state_path, state)?;
    Ok(())
}

fn stop_provider_machine(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    timeout: Duration,
) -> Result<(), Error> {
    match config.provider {
        MachineProvider::Krunkit => stop_krunkit_machine(paths, timeout),
        MachineProvider::Wsl2 => Err(Error::InvalidInput(
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
    super::write_json_file(&paths.state_path, state)
}

pub(super) fn handle_start_machine_error(
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
    super::write_json_file(&paths.state_path, state)?;
    Err(error)
}

pub(super) fn annotate_machine_start_error(
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
    if config.provider != MachineProvider::Krunkit
        || machine_bootstrap_mode(config) != MachineBootstrapMode::Ignition
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
        "guest reached a console login prompt without consuming the legacy Ignition payload. This hint applies only to explicit legacy image overrides; the default Nimbus bootc machine OS uses the machine-config channel instead of Ignition",
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
    super::write_json_file(&paths.state_path, state)?;
    Err(Error::Cancelled)
}

pub(super) fn request_krunkit_state_change(endpoint_path: &Path, state: &str) -> Result<(), Error> {
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

pub(super) fn wait_for_pid_exit(pid: i32, timeout: Duration) -> Result<bool, Error> {
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

pub(super) fn force_stop_pid(pid: i32, timeout: Duration) -> Result<(), Error> {
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

pub(super) fn cleanup_process(child: &mut Child) -> Result<(), Error> {
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

pub(super) fn cleanup_runtime_artifacts(paths: &MachinePaths) -> Result<(), Error> {
    for path in [
        &paths.ready_socket_path,
        &paths.ignition_socket_path,
        &paths.api_socket_path,
        &paths.gvproxy_socket_path,
        &paths.krunkit_gvproxy_socket_path(),
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

pub(super) fn remove_file_if_exists(path: &Path) -> Result<(), Error> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Internal(format!(
            "failed to remove {}: {error}",
            path.display()
        ))),
    }
}

pub(super) fn read_pid_if_alive(path: &Path) -> Result<Option<i32>, Error> {
    Ok(read_pid(path)?.filter(|pid| pid_is_alive(*pid)))
}

pub(super) fn read_pid(path: &Path) -> Result<Option<i32>, Error> {
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

pub(super) fn send_signal(pid: i32, signal: i32) -> Result<(), Error> {
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

fn resolve_stop_wait_timeout() -> Duration {
    let seconds = env_parse_u64(super::STOP_WAIT_TIMEOUT_ENV)
        .unwrap_or(super::DEFAULT_STOP_WAIT_TIMEOUT.as_secs());
    Duration::from_secs(seconds.max(1))
}

fn env_parse_u64(name: &str) -> Option<u64> {
    std::env::var(name).ok()?.trim().parse().ok()
}
