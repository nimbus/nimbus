use std::fs;
use std::io::{self, Write};
use std::path::Path;

use fs2::FileExt;
use neovex::Error;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use super::DEFAULT_MACHINE_NAME;
use super::manager::refresh_machine_state;
use super::record::{MachineConfigRecord, MachinePaths, MachineRootLayout, MachineStateRecord};

pub(super) fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<(), Error> {
    let parent = path.parent().ok_or_else(|| {
        Error::Internal(format!(
            "failed to resolve parent directory for {}",
            path.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        Error::Internal(format!(
            "failed to create parent directory {}: {error}",
            parent.display()
        ))
    })?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| {
        Error::Internal(format!("failed to serialize {}: {error}", path.display()))
    })?;
    let mut temp_file = NamedTempFile::new_in(parent).map_err(|error| {
        Error::Internal(format!(
            "failed to create temporary file for {}: {error}",
            path.display()
        ))
    })?;
    temp_file.write_all(&bytes).map_err(|error| {
        Error::Internal(format!(
            "failed to write temporary file for {}: {error}",
            path.display()
        ))
    })?;
    temp_file.flush().map_err(|error| {
        Error::Internal(format!(
            "failed to flush temporary file for {}: {error}",
            path.display()
        ))
    })?;
    temp_file.as_file().sync_all().map_err(|error| {
        Error::Internal(format!(
            "failed to sync temporary file for {}: {error}",
            path.display()
        ))
    })?;
    temp_file.into_temp_path().persist(path).map_err(|error| {
        Error::Internal(format!(
            "failed to atomically replace {}: {}",
            path.display(),
            error.error
        ))
    })
}

#[cfg(test)]
pub(super) fn read_json_file_if_exists<T>(path: &Path) -> Result<Option<T>, Error>
where
    T: for<'de> Deserialize<'de>,
{
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|error| {
            Error::Internal(format!("failed to parse {}: {error}", path.display()))
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::Internal(format!(
            "failed to read {}: {error}",
            path.display()
        ))),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
struct MachineRecordVersionProbe {
    #[serde(default)]
    version: u32,
}

fn read_file_if_exists(path: &Path) -> Result<Option<Vec<u8>>, Error> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::Internal(format!(
            "failed to read {}: {error}",
            path.display()
        ))),
    }
}

fn probe_machine_record_version(
    path: &Path,
    bytes: &[u8],
    record_kind: &str,
) -> Result<u32, Error> {
    serde_json::from_slice::<MachineRecordVersionProbe>(bytes)
        .map(|probe| probe.version)
        .map_err(|error| {
            Error::InvalidInput(format!(
                "{record_kind} at {} is unreadable and cannot determine its schema version: {error}",
                path.display()
            ))
        })
}

fn parse_machine_record<T>(path: &Path, bytes: &[u8], record_kind: &str) -> Result<T, Error>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_slice(bytes).map_err(|error| {
        Error::InvalidInput(format!(
            "{record_kind} at {} is unreadable: {error}",
            path.display()
        ))
    })
}

pub(super) fn load_machine_config_if_exists(
    path: &Path,
) -> Result<Option<MachineConfigRecord>, Error> {
    let Some(bytes) = read_file_if_exists(path)? else {
        return Ok(None);
    };

    let version = probe_machine_record_version(path, &bytes, "machine config")?;
    match version {
        super::CURRENT_MACHINE_CONFIG_VERSION => {
            parse_machine_record::<MachineConfigRecord>(path, &bytes, "machine config").map(Some)
        }
        newer if newer > super::CURRENT_MACHINE_CONFIG_VERSION => {
            Err(Error::InvalidInput(format!(
                "machine config at {} uses newer schema version {}; this neovex build supports version {}. Upgrade neovex or recreate the machine.",
                path.display(),
                newer,
                super::CURRENT_MACHINE_CONFIG_VERSION
            )))
        }
        older => Err(Error::InvalidInput(format!(
            "machine config at {} uses unsupported schema version {}; this neovex build supports version {}. Recreate the machine with `neovex machine rm` then `neovex machine init`.",
            path.display(),
            older,
            super::CURRENT_MACHINE_CONFIG_VERSION
        ))),
    }
}

fn rebuild_machine_state(
    path: &Path,
    reason: impl Into<String>,
) -> Result<MachineStateRecord, Error> {
    let state = MachineStateRecord::rebuilt(reason);
    write_json_file(path, &state)?;
    Ok(state)
}

pub(super) fn load_machine_state_if_exists(
    path: &Path,
) -> Result<Option<MachineStateRecord>, Error> {
    let Some(bytes) = read_file_if_exists(path)? else {
        return Ok(None);
    };

    let version = match probe_machine_record_version(path, &bytes, "machine state") {
        Ok(version) => version,
        Err(error) => return rebuild_machine_state(path, error.to_string()).map(Some),
    };

    match version {
        super::CURRENT_MACHINE_STATE_VERSION => {
            match parse_machine_record::<MachineStateRecord>(path, &bytes, "machine state") {
                Ok(state) => Ok(Some(state)),
                Err(error) => rebuild_machine_state(path, error.to_string()).map(Some),
            }
        }
        newer if newer > super::CURRENT_MACHINE_STATE_VERSION => rebuild_machine_state(
            path,
            format!(
                "machine state at {} used newer schema version {}; rebuilt with version {}",
                path.display(),
                newer,
                super::CURRENT_MACHINE_STATE_VERSION
            ),
        )
        .map(Some),
        older => rebuild_machine_state(
            path,
            format!(
                "machine state at {} used unsupported schema version {}; rebuilt with version {}",
                path.display(),
                older,
                super::CURRENT_MACHINE_STATE_VERSION
            ),
        )
        .map(Some),
    }
}

pub(super) fn load_initialized_machine(
    roots: &MachineRootLayout,
    machine_name: &str,
) -> Result<(MachinePaths, MachineConfigRecord, MachineStateRecord), Error> {
    let paths = roots.paths(machine_name);
    let config = load_machine_config_if_exists(&paths.config_path)?.ok_or_else(|| {
        Error::InvalidInput(format!(
            "machine '{}' is not initialized; run `neovex machine start` to create it with defaults or `neovex machine init` to configure it first",
            machine_name
        ))
    })?;
    let mut state = load_machine_state_if_exists(&paths.state_path)?
        .unwrap_or_else(MachineStateRecord::initialized);
    refresh_machine_state(&paths, &mut state)?;
    write_json_file(&paths.state_path, &state)?;
    Ok((paths, config, state))
}

pub(super) fn remove_dir_if_exists(path: &Path) -> Result<(), Error> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Internal(format!(
            "failed to remove {}: {error}",
            path.display()
        ))),
    }
}

struct MachineRecordLock {
    _file: fs::File,
}

pub(super) fn with_machine_lock<T>(
    roots: &MachineRootLayout,
    machine_name: &str,
    operation: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    let _lock = lock_machine_records(roots, machine_name)?;
    operation()
}

pub(super) fn with_default_machine_lock<T>(
    roots: &MachineRootLayout,
    operation: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    with_machine_lock(roots, DEFAULT_MACHINE_NAME, operation)
}

fn lock_machine_records(
    roots: &MachineRootLayout,
    machine_name: &str,
) -> Result<MachineRecordLock, Error> {
    let lock_path = roots.lock_path(machine_name);
    let parent = lock_path.parent().ok_or_else(|| {
        Error::Internal(format!(
            "failed to resolve parent directory for machine lock {}",
            lock_path.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        Error::Internal(format!(
            "failed to create machine lock directory {}: {error}",
            parent.display()
        ))
    })?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| {
            Error::Internal(format!(
                "failed to open machine lock {}: {error}",
                lock_path.display()
            ))
        })?;
    file.lock_exclusive().map_err(|error| {
        Error::Internal(format!(
            "failed to acquire machine lock {}: {error}",
            lock_path.display()
        ))
    })?;
    Ok(MachineRecordLock { _file: file })
}

pub(super) fn remove_dir_if_empty(path: &Path) -> Result<(), Error> {
    match fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(Error::Internal(format!(
            "failed to remove {}: {error}",
            path.display()
        ))),
    }
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

pub(super) fn remove_machine_runtime_artifacts(paths: &MachinePaths) -> Result<(), Error> {
    for path in [
        &paths.api_socket_path,
        &paths.ready_socket_path,
        &paths.ignition_socket_path,
        &paths.gvproxy_socket_path,
        &paths.krunkit_gvproxy_socket_path(),
        &paths.krunkit_endpoint_path,
        &paths.gvproxy_pid_path,
        &paths.krunkit_pid_path,
        &paths.machine_log_path,
        &paths.gvproxy_log_path,
        &paths.krunkit_log_path,
    ] {
        remove_file_if_exists(path)?;
    }
    Ok(())
}
