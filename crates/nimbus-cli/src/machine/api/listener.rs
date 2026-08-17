use super::*;

pub(super) fn resolve_machine_api_listener(
    command: &MachineApiCommand,
) -> Result<(tokio::net::UnixListener, MachineApiListenMode), Error> {
    bind_direct_listener(&command.socket_path)
        .map(|listener| (listener, MachineApiListenMode::DirectSocket))
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
    if let Err(error) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
        drop(listener);
        let cleanup = fs::remove_file(path)
            .err()
            .map(|cleanup_error| format!("; stale-socket cleanup also failed: {cleanup_error}"))
            .unwrap_or_default();
        return Err(Error::Internal(format!(
            "failed to restrict machine API socket {} to its owner: {error}{cleanup}",
            path.display()
        )));
    }
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
