use std::env;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use neovex::Error;
use serde::{Deserialize, Serialize};

use super::{
    MachineConfigRecord, MachineLifecycle, MachineManagerState, MachinePaths, MachineRootLayout,
    MachineStateRecord, write_json_file,
};

pub(super) const MACHINE_API_FORWARD_TRANSPORT: &str = "gvproxy-ssh-forwarded-unix-socket";
pub(super) const MACHINE_API_FORWARD_USER: &str = "root";

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
pub(super) enum GuestNeovexBinarySourceKind {
    ReleaseAsset,
    ExplicitOverride,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct DesiredGuestNeovexBinaryStatus {
    pub(super) install_path: PathBuf,
    pub(super) source: GuestNeovexBinarySourceKind,
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
pub(super) struct ObservedGuestNeovexBinaryStatus {
    pub(super) version: Option<String>,
    pub(super) hash: Option<String>,
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
    _config: &MachineConfigRecord,
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

pub(super) fn release_machine_ssh_port(
    _roots: &MachineRootLayout,
    _machine_name: &str,
) -> Result<(), Error> {
    Ok(())
}

pub(super) fn inspect_desired_guest_neovex_binary(
    paths: &MachinePaths,
) -> DesiredGuestNeovexBinaryStatus {
    if let Some(path) = env::var_os("NEOVEX_MACHINE_GUEST_BINARY").map(PathBuf::from) {
        let desired_exists = path.is_file();
        return DesiredGuestNeovexBinaryStatus {
            install_path: PathBuf::from("/usr/local/bin/neovex"),
            source: GuestNeovexBinarySourceKind::ExplicitOverride,
            source_detail: format!("$NEOVEX_MACHINE_GUEST_BINARY={}", path.display()),
            desired_path: path,
            desired_exists,
            desired_version: None,
            desired_hash: None,
            release_archive_path: None,
            release_archive_exists: None,
            release_url: None,
            error: Some(unsupported_machine_host_error().to_string()),
        };
    }

    DesiredGuestNeovexBinaryStatus {
        install_path: PathBuf::from("/usr/local/bin/neovex"),
        source: GuestNeovexBinarySourceKind::ReleaseAsset,
        source_detail: "GitHub release asset lookup is unavailable on non-unix host stubs"
            .to_owned(),
        desired_path: paths.guest_binary_cache_dir.join("unsupported-host-neovex"),
        desired_exists: false,
        desired_version: None,
        desired_hash: None,
        release_archive_path: None,
        release_archive_exists: None,
        release_url: None,
        error: Some(unsupported_machine_host_error().to_string()),
    }
}

pub(super) fn inspect_observed_guest_neovex_binary(
    _config: &MachineConfigRecord,
    _state: &MachineStateRecord,
) -> Result<ObservedGuestNeovexBinaryStatus, Error> {
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
