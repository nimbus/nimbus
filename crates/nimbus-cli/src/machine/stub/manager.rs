use std::env;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use nimbus::Error;
use serde::Serialize;
#[cfg(test)]
use sha2::{Digest, Sha256};

// `MachineHelperBinaryPaths` is consumed by the unix manager
// submodules; on the non-unix stub it is only available via this
// re-export for parity with `manager.rs` and stays unused.
#[allow(unused_imports)]
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
    config: &MachineConfigRecord,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    let error = config.provider.unavailable_error();
    state.lifecycle = MachineLifecycle::Failed;
    state.manager = MachineManagerState::Failed;
    state.last_error = Some(error.to_string());
    write_json_file(&paths.state_path, state)?;
    Err(error)
}

pub(super) fn stop_machine(
    _paths: &MachinePaths,
    config: &MachineConfigRecord,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    if matches!(
        state.lifecycle,
        MachineLifecycle::Stopped | MachineLifecycle::Uninitialized
    ) {
        return Ok(());
    }

    Err(config.provider.unavailable_error())
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
    state: &MachineStateRecord,
) -> Result<(), Error> {
    if state.runtime.is_some() {
        return Err(Error::conflict(
            "the non-unix machine stub has retained runtime authority and cannot attest SSH port release"
                .to_owned(),
        ));
    }
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

#[cfg(test)]
mod tests {
    use nimbus_network::ListenerId;
    use tempfile::TempDir;

    use super::*;
    use crate::machine::{
        CURRENT_MACHINE_CONFIG_VERSION, MachineGuestConfig, MachineGuestProvisioning,
        MachineHelperBinaryPaths, MachineImageSource, MachineProvider, MachineResources,
        MachineRuntimeState, MachineVolume,
    };

    fn fixture(
        temp_dir: &TempDir,
        provider: MachineProvider,
    ) -> (MachinePaths, MachineConfigRecord) {
        let root = temp_dir.path();
        let roots = MachineRootLayout::new(
            root.join("config"),
            root.join("state"),
            root.join("data"),
            root.join("cache"),
            root.join("runtime"),
        );
        let paths = roots.paths("stub-contract");
        let config = MachineConfigRecord {
            version: CURRENT_MACHINE_CONFIG_VERSION,
            name: "stub-contract".to_owned(),
            provider,
            guest: MachineGuestConfig {
                image_source: MachineImageSource::LocalDisk {
                    path: root.join("machine.raw"),
                },
                provisioning: MachineGuestProvisioning::Ignition,
                ssh_user: "core".to_owned(),
                ssh_identity_path: None,
                ignition_file_path: None,
                efi_variable_store_path: None,
            },
            resources: MachineResources {
                cpus: 2,
                memory_mib: 2_048,
                disk_gib: 20,
            },
            volumes: Vec::<MachineVolume>::new(),
            roots,
        };
        (paths, config)
    }

    fn retained_runtime() -> MachineRuntimeState {
        MachineRuntimeState {
            helper_binaries: MachineHelperBinaryPaths {
                vmm: PathBuf::from("/unavailable/vmm"),
                gvproxy: PathBuf::from("/unavailable/gvproxy"),
            },
            image_path: PathBuf::from("/unavailable/machine.raw"),
            efi_variable_store_path: PathBuf::from("/unavailable/efi"),
            machine_image_source: "unavailable".to_owned(),
            ssh_listener_id: ListenerId::for_workload_listener(
                "nimbus-cli-non-unix-stub-test",
                "retained-runtime",
            ),
            ssh_port: 22_222,
            rest_uri: "unavailable://machine".to_owned(),
            ready_vsock_port: 1_025,
        }
    }

    #[test]
    fn non_unix_unavailable_start_uses_the_named_provider_error() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let (paths, config) = fixture(&temp_dir, MachineProvider::Wsl2);
        let mut state = MachineStateRecord::initialized();

        let error = start_machine(&paths, &config, &mut state)
            .expect_err("the unavailable WSL2 stub must fail closed");

        assert!(
            error.to_string().contains("WSL2"),
            "the unavailable provider must be named: {error}"
        );
        assert_eq!(state.lifecycle, MachineLifecycle::Failed);
    }

    #[test]
    fn non_unix_unavailable_stop_never_transitions_to_stopped() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let (paths, config) = fixture(&temp_dir, MachineProvider::Wsl2);
        let mut state = MachineStateRecord::initialized();
        state.lifecycle = MachineLifecycle::Running;
        state.manager = MachineManagerState::Ready;
        state.runtime = Some(retained_runtime());
        state.last_error = Some("retained provider evidence".to_owned());
        let before = state.clone();

        let error = stop_machine(&paths, &config, &mut state)
            .expect_err("an unavailable stub cannot attest a provider stop");

        assert!(
            error.to_string().contains("WSL2"),
            "the unavailable provider must be named: {error}"
        );
        assert_eq!(
            state, before,
            "a failed unavailable-provider stop must preserve retained evidence"
        );
        assert!(
            !paths.state_path.exists(),
            "the rejected stop must not persist false terminal state"
        );
    }

    #[test]
    fn non_unix_release_rejects_retained_runtime_authority() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let (_paths, config) = fixture(&temp_dir, MachineProvider::Wsl2);
        let mut state = MachineStateRecord::initialized();
        state.runtime = Some(retained_runtime());

        let error = release_machine_ssh_port(&config.roots, &state)
            .expect_err("retained runtime evidence cannot be reported released by a no-op stub");

        assert!(
            error.to_string().contains("retained"),
            "release refusal should name the retained authority: {error}"
        );
    }
}
