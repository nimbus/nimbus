#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

#[cfg(test)]
use nimbus::Error;

#[cfg(unix)]
mod api;
#[cfg(not(unix))]
#[path = "stub/api.rs"]
mod api;
#[cfg(unix)]
mod backend;
#[cfg(not(unix))]
#[path = "stub/backend.rs"]
mod backend;
#[cfg(unix)]
mod bootstrap;
#[cfg(not(unix))]
#[path = "stub/bootstrap.rs"]
mod bootstrap;
#[cfg(unix)]
mod client;
#[cfg(not(unix))]
#[path = "stub/client.rs"]
mod client;
mod command;
mod files;
mod guest_config;
mod handlers;
mod local_server;
#[cfg(unix)]
mod manager;
#[cfg(not(unix))]
#[path = "stub/manager.rs"]
mod manager;
mod protocol;
mod record;
mod render;
mod server_control;

#[cfg(test)]
pub(crate) use self::api::{
    MachineApiListenMode, MachineApiState, bind_direct_listener, default_guest_helper_binary_dirs,
    serve_machine_api,
};
pub(crate) use self::backend::ForwardedMachineApiSandboxBackend;
pub(crate) use self::client::MachineApiClient;
pub(crate) use self::command::MachineCommand;
pub(crate) use self::handlers::{
    ensure_default_machine_api_client_started, require_default_machine_api_client,
    run_machine_command,
};
pub(crate) use self::protocol::MachineApiServiceSandboxDetails;
pub(crate) use self::server_control::host_machine_lifecycle_manager;

use self::command::MachineApiCommand;
use self::files::write_json_file;
#[cfg(any(unix, test))]
use self::record::MachineBootstrapMode;
#[cfg(any(unix, test))]
use self::record::MachineGuestProvisioning;
#[cfg(test)]
use self::record::MachineImageFormat;
use self::record::{
    MachineConfigRecord, MachineImageSource, MachineLifecycle, MachineManagerState, MachinePaths,
    MachineProvider, MachineRootLayout, MachineStateRecord, MachineVolume,
};

#[cfg(test)]
#[allow(unused_imports)]
use self::command::*;
#[cfg(test)]
#[allow(unused_imports)]
use self::files::*;
#[cfg(test)]
#[allow(unused_imports)]
use self::handlers::*;
#[cfg(test)]
#[allow(unused_imports)]
use self::manager::*;
#[cfg(test)]
use self::record::{MachineGuestConfig, MachineResources};
#[cfg(test)]
#[allow(unused_imports)]
use self::render::*;

const DEFAULT_MACHINE_NAME: &str = "default";
const DEFAULT_NIMBUS_MACHINE_IMAGE_REPOSITORY: &str = "ghcr.io/nimbus/machine-os";
const DEFAULT_NIMBUS_MACHINE_IMAGE_TAG: &str = "v0.1.30";
const DEFAULT_NIMBUS_MACHINE_IMAGE_DIGEST: &str =
    "sha256:f56553e212d2e077d8bedc1db902283f6e12315a621d6046b03d1cb43a0eb08d";
const DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY: &str = "quay.io/podman/machine-os";

fn current_machine_release_tag() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}

fn default_machine_image() -> String {
    default_machine_image_for_provider(MachineProvider::Krunkit)
}

fn default_machine_image_for_provider(provider: MachineProvider) -> String {
    match provider {
        MachineProvider::Krunkit if cfg!(target_os = "macos") => format!(
            "docker://{DEFAULT_NIMBUS_MACHINE_IMAGE_REPOSITORY}:{}@{DEFAULT_NIMBUS_MACHINE_IMAGE_DIGEST}",
            DEFAULT_NIMBUS_MACHINE_IMAGE_TAG
        ),
        MachineProvider::Krunkit | MachineProvider::Wsl2 => format!(
            "docker://{DEFAULT_NIMBUS_MACHINE_IMAGE_REPOSITORY}:{}",
            current_machine_release_tag()
        ),
    }
}

fn machine_image_reference_repository(reference: &str) -> String {
    let stripped = reference.trim_start_matches("docker://");
    let without_digest = stripped.split('@').next().unwrap_or(stripped);
    let last_component = without_digest.rsplit('/').next().unwrap_or(without_digest);
    if last_component.contains(':') {
        without_digest
            .rsplit_once(':')
            .map(|(repository, _)| repository)
            .unwrap_or(without_digest)
            .to_owned()
    } else {
        without_digest.to_owned()
    }
}

fn machine_image_reference_version_label(reference: &str) -> String {
    let stripped = reference.trim_start_matches("docker://");
    if let Some((_, digest)) = stripped.rsplit_once('@') {
        return digest.to_owned();
    }
    let last_component = stripped.rsplit('/').next().unwrap_or(stripped);
    if let Some((_, tag)) = last_component.rsplit_once(':') {
        return tag.to_owned();
    }
    stripped.to_owned()
}

fn machine_image_source_repository(source: &MachineImageSource) -> Option<String> {
    match source {
        MachineImageSource::OciReference { reference } => {
            Some(machine_image_reference_repository(reference))
        }
        MachineImageSource::HttpUrl { .. } | MachineImageSource::LocalDisk { .. } => None,
    }
}

fn uses_nimbus_bootc_machine_image_source(source: &MachineImageSource) -> bool {
    machine_image_source_repository(source).as_deref()
        == Some(DEFAULT_NIMBUS_MACHINE_IMAGE_REPOSITORY)
}

fn uses_podman_machine_image_source(source: &MachineImageSource) -> bool {
    machine_image_source_repository(source).as_deref()
        == Some(DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY)
}

fn uses_host_managed_machine_image_contract(config: &MachineConfigRecord) -> bool {
    if !(cfg!(target_os = "macos") && config.provider == MachineProvider::Krunkit) {
        return false;
    }

    uses_podman_machine_image_source(&config.guest.image_source)
}

fn desired_machine_image_source(config: &MachineConfigRecord) -> MachineImageSource {
    config.guest.image_source.clone()
}

fn describe_machine_image_source(source: &MachineImageSource) -> String {
    match source {
        MachineImageSource::OciReference { reference } => reference.clone(),
        MachineImageSource::HttpUrl { url } => url.clone(),
        MachineImageSource::LocalDisk { path } => path.display().to_string(),
    }
}

const DEFAULT_MACHINE_SSH_USER: &str = "core";
const DEFAULT_BOOTC_MACHINE_SSH_USER: &str = "nimbus";
const DEFAULT_MACHINE_CPUS: u8 = 2;
const DEFAULT_MACHINE_MEMORY_MIB: u32 = 2048;
const DEFAULT_MACHINE_DISK_GIB: u32 = 20;
const CURRENT_MACHINE_CONFIG_VERSION: u32 = nimbus_machine::CURRENT_MACHINE_CONFIG_VERSION;
const CURRENT_MACHINE_STATE_VERSION: u32 = nimbus_machine::CURRENT_MACHINE_STATE_VERSION;

fn default_machine_volumes() -> Vec<MachineVolume> {
    if cfg!(target_os = "macos") {
        vec![
            MachineVolume {
                source: PathBuf::from("/Users"),
                target: PathBuf::from("/Users"),
            },
            MachineVolume {
                source: PathBuf::from("/private"),
                target: PathBuf::from("/private"),
            },
            MachineVolume {
                source: PathBuf::from("/var/folders"),
                target: PathBuf::from("/var/folders"),
            },
        ]
    } else {
        Vec::new()
    }
}

#[cfg(any(unix, test))]
fn machine_bootstrap_mode(config: &MachineConfigRecord) -> MachineBootstrapMode {
    match config.guest.provisioning {
        MachineGuestProvisioning::BootcMachineConfig => MachineBootstrapMode::BootcMachineConfig,
        MachineGuestProvisioning::Ignition => config.provider.bootstrap_mode(),
    }
}

fn parse_machine_volume(value: &str) -> Result<MachineVolume, String> {
    MachineVolume::parse(value)
}

fn invalidate_materialized_machine_os(paths: &MachinePaths) -> Result<(), nimbus::Error> {
    files::remove_file_if_exists(&paths.materialized_image_path)?;
    files::remove_file_if_exists(&paths.efi_variable_store_path)
}

#[cfg(test)]
mod tests;
