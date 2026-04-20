#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

#[cfg(test)]
use neovex::Error;

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
mod handlers;
#[cfg(unix)]
mod manager;
#[cfg(not(unix))]
#[path = "stub/manager.rs"]
mod manager;
mod protocol;
mod record;
mod render;

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

use self::command::MachineApiCommand;
use self::files::write_json_file;
#[cfg(test)]
use self::record::MachineImageFormat;
use self::record::{
    MachineBootstrapMode, MachineConfigRecord, MachineImageSource, MachineLifecycle,
    MachineManagerState, MachinePaths, MachineProvider, MachineRootLayout, MachineStateRecord,
    MachineVolume,
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
const DEFAULT_NEOVEX_MACHINE_IMAGE_REPOSITORY: &str = "ghcr.io/agentstation/neovex-machine-os";
const DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY: &str = "quay.io/podman/machine-os";
const DEFAULT_PODMAN_MACHINE_IMAGE_DIGEST: &str =
    "sha256:972a9fb73e96c903320b3bef32cd212eb0c386f9b6a19737878a063d4703c6ff";

fn current_machine_release_tag() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}

fn default_machine_image() -> String {
    default_machine_image_for_provider(MachineProvider::Krunkit)
}

fn default_machine_image_for_provider(provider: MachineProvider) -> String {
    match provider {
        MachineProvider::Krunkit if cfg!(target_os = "macos") => format!(
            "docker://{DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY}@{DEFAULT_PODMAN_MACHINE_IMAGE_DIGEST}"
        ),
        MachineProvider::Krunkit | MachineProvider::Wsl2 => format!(
            "docker://{DEFAULT_NEOVEX_MACHINE_IMAGE_REPOSITORY}:{}",
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

fn uses_host_managed_machine_image_contract(config: &MachineConfigRecord) -> bool {
    if !(cfg!(target_os = "macos") && config.provider == MachineProvider::Krunkit) {
        return false;
    }

    matches!(
        &config.guest.image_source,
        MachineImageSource::OciReference { reference }
            if machine_image_reference_repository(reference) == DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY
    )
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
const DEFAULT_MACHINE_RUNTIME_ROOT: &str = "/tmp/neovex";
const MACHINE_RUNTIME_ROOT_ENV: &str = "NEOVEX_MACHINE_RUNTIME_ROOT";
const DEFAULT_MACHINE_CPUS: u8 = 2;
const DEFAULT_MACHINE_MEMORY_MIB: u32 = 2048;
const DEFAULT_MACHINE_DISK_GIB: u32 = 20;
const CURRENT_MACHINE_CONFIG_VERSION: u32 = 2;
const CURRENT_MACHINE_STATE_VERSION: u32 = 1;

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

fn parse_machine_volume(value: &str) -> Result<MachineVolume, String> {
    MachineVolume::parse(value)
}

fn invalidate_materialized_machine_os(paths: &MachinePaths) -> Result<(), neovex::Error> {
    files::remove_file_if_exists(&paths.materialized_image_path)?;
    files::remove_file_if_exists(&paths.efi_variable_store_path)
}

#[cfg(test)]
mod tests;
