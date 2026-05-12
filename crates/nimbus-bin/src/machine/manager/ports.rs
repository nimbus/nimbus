use std::fs;
use std::io;
use std::net::TcpListener;

use fs2::FileExt;
use nimbus::Error;
use serde::{Deserialize, Serialize};

use super::{MACHINE_PORT_MAX, MACHINE_PORT_MIN};
use crate::machine::MachineRootLayout;

#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct MachinePortAllocationState {
    #[serde(default)]
    pub(super) machine_ports: std::collections::BTreeMap<String, u16>,
}

pub(super) struct MachinePortAllocationLock {
    _file: fs::File,
}

pub(super) fn allocate_machine_ssh_port(
    roots: &MachineRootLayout,
    machine_name: &str,
    state: &super::MachineStateRecord,
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

pub(super) fn managed_machine_port_range_contains(port: u16) -> bool {
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

pub(super) fn with_port_allocation_lock<T>(
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

pub(super) fn load_machine_port_allocation_state(
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

pub(super) fn write_machine_port_allocation_state(
    roots: &MachineRootLayout,
    allocation_state: &MachinePortAllocationState,
) -> Result<(), Error> {
    super::write_json_file(&roots.port_allocation_state_path(), allocation_state)
}
