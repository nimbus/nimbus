use super::*;

const DEFAULT_SYSTEMD_SOCKET_FD: i32 = 3;

pub(super) fn resolve_machine_api_listener(
    command: &MachineApiCommand,
) -> Result<(tokio::net::UnixListener, MachineApiListenMode), Error> {
    if command.socket_activation {
        return inherited_systemd_listener()
            .map(|listener| (listener, MachineApiListenMode::SystemdSocketActivation));
    }

    let socket_path = command.socket_path.as_ref().ok_or_else(|| {
        Error::InvalidInput(
            "machine api requires either --socket-path <path> or --socket-activation".to_owned(),
        )
    })?;
    bind_direct_listener(socket_path).map(|listener| (listener, MachineApiListenMode::DirectSocket))
}

pub(crate) fn bind_direct_listener(path: &Path) -> Result<tokio::net::UnixListener, Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine API socket directory {}: {error}",
                parent.display()
            ))
        })?;
    }
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(Error::Internal(format!(
                "failed to clear stale machine API socket {}: {error}",
                path.display()
            )));
        }
    }

    let listener = StdUnixListener::bind(path).map_err(|error| {
        Error::Internal(format!(
            "failed to bind machine API socket {}: {error}",
            path.display()
        ))
    })?;
    listener.set_nonblocking(true).map_err(|error| {
        Error::Internal(format!(
            "failed to configure machine API socket {}: {error}",
            path.display()
        ))
    })?;
    tokio::net::UnixListener::from_std(listener).map_err(|error| {
        Error::Internal(format!(
            "failed to convert machine API socket {} to tokio listener: {error}",
            path.display()
        ))
    })
}

pub(super) fn inherited_systemd_listener() -> Result<tokio::net::UnixListener, Error> {
    let current_pid = std::process::id();
    let listen_pid = std::env::var("LISTEN_PID")
        .map_err(|_| {
            Error::InvalidInput(
                "machine API socket activation requires LISTEN_PID from systemd".to_owned(),
            )
        })?
        .parse::<u32>()
        .map_err(|error| {
            Error::InvalidInput(format!(
                "machine API socket activation could not parse LISTEN_PID: {error}"
            ))
        })?;
    let listen_fds = std::env::var("LISTEN_FDS")
        .map_err(|_| {
            Error::InvalidInput(
                "machine API socket activation requires LISTEN_FDS from systemd".to_owned(),
            )
        })?
        .parse::<u32>()
        .map_err(|error| {
            Error::InvalidInput(format!(
                "machine API socket activation could not parse LISTEN_FDS: {error}"
            ))
        })?;

    if listen_pid != current_pid {
        return Err(Error::InvalidInput(format!(
            "machine API socket activation expected LISTEN_PID={} but found {}",
            current_pid, listen_pid
        )));
    }
    if listen_fds != 1 {
        return Err(Error::InvalidInput(format!(
            "machine API socket activation supports exactly one inherited socket, found {}",
            listen_fds
        )));
    }

    remove_env_var("LISTEN_PID");
    remove_env_var("LISTEN_FDS");
    tokio_listener_from_inherited_fd(DEFAULT_SYSTEMD_SOCKET_FD)
}

pub(super) fn tokio_listener_from_inherited_fd(fd: i32) -> Result<tokio::net::UnixListener, Error> {
    let listener = unsafe { StdUnixListener::from_raw_fd(fd) };
    listener.set_nonblocking(true).map_err(|error| {
        Error::Internal(format!(
            "failed to configure inherited machine API socket fd {}: {error}",
            fd
        ))
    })?;
    tokio::net::UnixListener::from_std(listener).map_err(|error| {
        Error::Internal(format!(
            "failed to convert inherited machine API socket fd {} to tokio listener: {error}",
            fd
        ))
    })
}

#[cfg(test)]
pub(super) fn set_env_var(key: &str, value: &str) {
    // SAFETY: the machine API test lane mutates process-local LISTEN_* values
    // in a serialized scope and restores them before returning.
    unsafe { std::env::set_var(key, value) }
}

pub(super) fn remove_env_var(key: &str) {
    // SAFETY: the machine API activation path clears only the inherited
    // LISTEN_* variables for the current process after validating them.
    unsafe { std::env::remove_var(key) }
}
