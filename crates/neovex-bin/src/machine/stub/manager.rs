#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use neovex::Error;
use serde::{Deserialize, Serialize};

use super::{
    MachineConfigRecord, MachineLifecycle, MachineManagerState, MachinePaths, MachineStateRecord,
    write_json_file,
};

pub(super) const MACHINE_API_FORWARD_TRANSPORT: &str = "gvproxy-ssh-forwarded-unix-socket";
pub(super) const MACHINE_API_FORWARD_USER: &str = "root";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MachineRuntimeState {
    pub(super) helper_binaries: MachineHelperBinaryPaths,
    pub(super) image_path: PathBuf,
    pub(super) efi_variable_store_path: PathBuf,
    pub(super) ssh_port: u16,
    pub(super) rest_uri: String,
    pub(super) ready_vsock_port: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MachineHelperBinaryPaths {
    pub(super) krunkit: PathBuf,
    pub(super) gvproxy: PathBuf,
}

pub(super) fn start_machine(
    paths: &MachinePaths,
    _config: &MachineConfigRecord,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    let error = unsupported_machine_host_error();
    state.lifecycle = MachineLifecycle::Failed;
    state.manager = MachineManagerState::Failed;
    state.last_error = Some(error.to_string());
    write_json_file(&paths.state_path, state)?;
    Err(error)
}

pub(super) fn stop_machine(
    paths: &MachinePaths,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    if matches!(
        state.lifecycle,
        MachineLifecycle::Stopped | MachineLifecycle::Uninitialized
    ) {
        return Ok(());
    }

    state.lifecycle = MachineLifecycle::Stopped;
    state.manager = if state.runtime.is_some() {
        MachineManagerState::HelpersResolved
    } else {
        MachineManagerState::Unconfigured
    };
    state.last_error = None;
    write_json_file(&paths.state_path, state)
}

pub(super) fn refresh_machine_state(
    _paths: &MachinePaths,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    if matches!(
        state.lifecycle,
        MachineLifecycle::Starting | MachineLifecycle::Running
    ) {
        state.lifecycle = MachineLifecycle::Failed;
        state.manager = MachineManagerState::Failed;
        state.last_error = Some(unsupported_machine_host_error().to_string());
    }
    Ok(())
}

pub(super) fn build_ssh_command(
    _config: &MachineConfigRecord,
    _state: &MachineStateRecord,
) -> Result<Command, Error> {
    Err(unsupported_machine_host_error())
}

#[cfg(test)]
pub(crate) struct MachineHelperEnvGuard;

#[cfg(test)]
impl MachineHelperEnvGuard {
    pub(crate) fn install_stub_binaries(_dir: &Path) -> Self {
        Self
    }

    pub(crate) fn set_paths(_krunkit_path: &Path, _gvproxy_path: &Path) -> Self {
        Self
    }
}

fn unsupported_machine_host_error() -> Error {
    Error::InvalidInput(
        "neovex machine support currently requires a unix host; Windows builds keep the CLI surface but cannot start or forward a machine"
            .to_owned(),
    )
}
