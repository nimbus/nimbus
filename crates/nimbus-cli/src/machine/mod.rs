#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

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
mod record;
mod render;
mod server_control;

#[cfg(test)]
pub(crate) use self::api::{
    MachineApiListenMode, MachineApiState, bind_direct_listener, default_guest_helper_binary_dirs,
    machine_api_node_workload_facade_from_sandbox_backend, serve_machine_api,
};
pub(crate) use self::backend::ForwardedMachineApiSandboxBackend;
pub(crate) use self::client::MachineApiClient;
pub(crate) use self::command::MachineCommand;
pub(crate) use self::handlers::{
    ensure_default_machine_api_client_started, require_default_machine_api_client,
    run_machine_command,
};
pub(crate) use self::server_control::host_machine_lifecycle_manager;
pub(crate) use nimbus_machine::api::MachineApiServiceSandboxDetails;

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
const MACHINE_PROVIDER_ENV: &str = "NIMBUS_MACHINE_PROVIDER";
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
        // Both macOS micro-VM monitors boot the pinned, digest-addressed
        // `applehv` machine-os image.
        provider if provider.uses_managed_applehv_guest() && cfg!(target_os = "macos") => format!(
            "docker://{DEFAULT_NIMBUS_MACHINE_IMAGE_REPOSITORY}:{}@{DEFAULT_NIMBUS_MACHINE_IMAGE_DIGEST}",
            DEFAULT_NIMBUS_MACHINE_IMAGE_TAG
        ),
        MachineProvider::Krunkit | MachineProvider::Vfkit | MachineProvider::Wsl2 => format!(
            "docker://{DEFAULT_NIMBUS_MACHINE_IMAGE_REPOSITORY}:{}",
            current_machine_release_tag()
        ),
    }
}

/// Resolve the machine VMM provider. Precedence: an explicit selection (CLI flag
/// or persisted config) wins; otherwise the `NIMBUS_MACHINE_PROVIDER` environment
/// variable; otherwise the static default, krunkit. There is no auto-detection or
/// capability sniffing — vfkit and any future backend are strictly opt-in.
fn resolve_machine_provider(explicit: Option<MachineProvider>) -> Result<MachineProvider, Error> {
    resolve_machine_provider_from(
        explicit,
        std::env::var(MACHINE_PROVIDER_ENV).ok().as_deref(),
    )
}

/// Pure provider-precedence resolution, separated from environment access so it
/// is testable without mutating process state.
fn resolve_machine_provider_from(
    explicit: Option<MachineProvider>,
    env_value: Option<&str>,
) -> Result<MachineProvider, Error> {
    if let Some(provider) = explicit {
        return Ok(provider);
    }
    if let Some(raw) = env_value {
        let token = raw.trim();
        if !token.is_empty() {
            return MachineProvider::from_token(token).ok_or_else(|| {
                Error::InvalidInput(format!(
                    "{MACHINE_PROVIDER_ENV} is set to '{raw}', which is not a known machine provider; expected one of: krunkit, vfkit, wsl2"
                ))
            });
        }
    }
    Ok(MachineProvider::Krunkit)
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
    if !(cfg!(target_os = "macos") && config.provider.uses_managed_applehv_guest()) {
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
        MachineImageSource::HttpUrl { url, sha256 } => format!("{url}#sha256={sha256}"),
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

/// macOS default host->guest volume mounts, expressed in the same
/// `<source>:<target>` grammar that every user-supplied `--volume` passes
/// through. Keeping the defaults as grammar strings (rather than pre-built
/// `MachineVolume` literals) forces them through [`MachineVolume::parse`] in
/// [`default_machine_volumes`], so a default can never silently drift away from
/// the validated grammar.
const DEFAULT_MACOS_MACHINE_VOLUME_SPECS: &[&str] = &[
    "/Users:/Users",
    "/private:/private",
    "/var/folders:/var/folders",
];

fn default_machine_volumes() -> Vec<MachineVolume> {
    if cfg!(target_os = "macos") {
        DEFAULT_MACOS_MACHINE_VOLUME_SPECS
            .iter()
            .map(|spec| {
                MachineVolume::parse(spec).unwrap_or_else(|error| {
                    panic!("default machine volume spec {spec:?} must parse-validate: {error}")
                })
            })
            .collect()
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
    MachineVolume::parse(value).map_err(|error| error.to_string())
}

fn invalidate_materialized_machine_os(paths: &MachinePaths) -> Result<(), nimbus::Error> {
    files::remove_file_if_exists(&paths.materialized_image_path)?;
    files::remove_file_if_exists(&paths.efi_variable_store_path)
}

#[cfg(test)]
mod tests;
