use std::env;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use nimbus::Error;
use serde::Serialize;
#[cfg(test)]
use sha2::{Digest, Sha256};

pub(super) use super::record::{MachineHelperBinaryPaths, MachineRuntimeState};
use super::{
    MachineConfigRecord, MachineLifecycle, MachineManagerState, MachinePaths, MachineRootLayout,
    MachineStateRecord, write_json_file,
};

pub(super) const MACHINE_API_FORWARD_TRANSPORT: &str = "gvproxy-ssh-forwarded-unix-socket";
pub(super) const MACHINE_API_FORWARD_USER: &str = "root";

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

pub(super) fn build_scp_command(
    _config: &MachineConfigRecord,
    _state: &MachineStateRecord,
    _guest_is_src: bool,
    _guest_path: &str,
    _host_path: &str,
) -> Result<Command, Error> {
    Err(unsupported_machine_host_error())
}

#[cfg(test)]
pub(super) fn mount_tag(target: &Path) -> String {
    let digest = Sha256::digest(target.as_os_str().as_encoded_bytes());
    format!("{digest:x}")[..36].to_owned()
}

pub(super) fn release_machine_ssh_port(
    _roots: &MachineRootLayout,
    _machine_name: &str,
) -> Result<(), Error> {
    Ok(())
}

pub(super) fn inspect_desired_guest_nimbus_binary(
    paths: &MachinePaths,
) -> DesiredGuestNimbusBinaryStatus {
    if let Some(path) = env::var_os("NIMBUS_MACHINE_GUEST_BINARY").map(PathBuf::from) {
        let desired_exists = path.is_file();
        return DesiredGuestNimbusBinaryStatus {
            install_path: PathBuf::from("/usr/local/bin/nimbus"),
            source: GuestNimbusBinarySourceKind::ExplicitOverride,
            source_detail: format!("$NIMBUS_MACHINE_GUEST_BINARY={}", path.display()),
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

    DesiredGuestNimbusBinaryStatus {
        install_path: PathBuf::from("/usr/local/bin/nimbus"),
        source: GuestNimbusBinarySourceKind::ReleaseAsset,
        source_detail: "GitHub release asset lookup is unavailable on non-unix host stubs"
            .to_owned(),
        desired_path: paths.guest_binary_cache_dir.join("unsupported-host-nimbus"),
        desired_exists: false,
        desired_version: None,
        desired_hash: None,
        release_archive_path: None,
        release_archive_exists: None,
        release_url: None,
        error: Some(unsupported_machine_host_error().to_string()),
    }
}

pub(super) fn inspect_observed_guest_nimbus_binary(
    _config: &MachineConfigRecord,
    _state: &MachineStateRecord,
) -> Result<ObservedGuestNimbusBinaryStatus, Error> {
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
        "nimbus machine support currently requires a unix host; Windows builds keep the CLI surface but cannot start or forward a machine"
            .to_owned(),
    )
}
