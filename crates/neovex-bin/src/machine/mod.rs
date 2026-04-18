use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use clap::{Args, Subcommand};
use fs2::FileExt;
use neovex::Error;
use semver::Version;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

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
#[cfg(unix)]
mod manager;
#[cfg(not(unix))]
#[path = "stub/manager.rs"]
mod manager;
mod protocol;

#[cfg(test)]
pub(crate) use self::api::{
    MachineApiListenMode, MachineApiState, bind_direct_listener, default_guest_helper_binary_dirs,
    serve_machine_api,
};
pub(crate) use self::backend::ForwardedMachineApiSandboxBackend;
pub(crate) use self::client::MachineApiClient;
use self::manager::{
    MACHINE_API_FORWARD_TRANSPORT, MACHINE_API_FORWARD_USER, MachineRuntimeState,
    build_ssh_command, refresh_machine_state, release_machine_ssh_port, start_machine,
    stop_machine,
};
use self::protocol::MachineApiCapabilityResponse;
pub(crate) use self::protocol::MachineApiServiceSandboxDetails;

const DEFAULT_MACHINE_NAME: &str = "default";
const DEFAULT_NEOVEX_MACHINE_IMAGE_REPOSITORY: &str = "ghcr.io/agentstation/neovex-machine-os";
const DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY: &str = "quay.io/podman/machine-os";
const LEGACY_PODMAN_MACHINE_IMAGE_TAG: &str = "6.0";
const DEFAULT_PODMAN_MACHINE_IMAGE_DIGEST: &str =
    "sha256:02ce56eb3a353f3d909eeb6742db7052e13fcad01937ef9536d41178c4865000";

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

fn legacy_podman_machine_image_for_provider(provider: MachineProvider) -> Option<String> {
    match provider {
        MachineProvider::Krunkit if cfg!(target_os = "macos") => Some(format!(
            "docker://{DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY}:{LEGACY_PODMAN_MACHINE_IMAGE_TAG}"
        )),
        MachineProvider::Krunkit | MachineProvider::Wsl2 => None,
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
            if matches!(
                machine_image_reference_repository(reference).as_str(),
                DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY | DEFAULT_NEOVEX_MACHINE_IMAGE_REPOSITORY
            )
    )
}

fn desired_machine_image_source(config: &MachineConfigRecord) -> MachineImageSource {
    let follows_release_default = matches!(
        &config.guest.image_source,
        MachineImageSource::OciReference { reference }
            if reference == &default_machine_image_for_provider(config.provider)
                || legacy_podman_machine_image_for_provider(config.provider)
                    .as_ref()
                    .is_some_and(|legacy| legacy == reference)
                || machine_image_reference_repository(reference)
                    == DEFAULT_NEOVEX_MACHINE_IMAGE_REPOSITORY
    );
    if follows_release_default {
        MachineImageSource::OciReference {
            reference: default_machine_image_for_provider(config.provider),
        }
    } else {
        config.guest.image_source.clone()
    }
}
const DEFAULT_MACHINE_SSH_USER: &str = "core";
const DEFAULT_MACHINE_RUNTIME_ROOT: &str = "/tmp/neovex";
const MACHINE_RUNTIME_ROOT_ENV: &str = "NEOVEX_MACHINE_RUNTIME_ROOT";
const DEFAULT_MACHINE_CPUS: u8 = 2;
const DEFAULT_MACHINE_MEMORY_MIB: u32 = 2048;
const DEFAULT_MACHINE_DISK_GIB: u32 = 20;
const CURRENT_MACHINE_CONFIG_VERSION: u32 = 1;
const CURRENT_MACHINE_STATE_VERSION: u32 = 1;

#[derive(Debug, Args)]
pub(crate) struct MachineCommand {
    #[command(subcommand)]
    command: MachineSubcommand,
}

#[derive(Debug, Subcommand)]
enum MachineSubcommand {
    /// Initialize the local machine config and state roots.
    Init(MachineInitCommand),
    /// Validate persisted machine state and prepare runtime roots for startup.
    Start(MachineStartCommand),
    /// Validate persisted machine state before a future graceful stop.
    Stop(MachineStopCommand),
    /// Show the current machine config, state, and derived paths.
    Status(MachineStatusCommand),
    /// Show the future guest SSH target once host orchestration is available.
    Ssh(MachineSshCommand),
    /// Remove the local machine config, state, and runtime roots.
    Rm(MachineRmCommand),
    /// Manage the pinned machine OS image contract for this host.
    Os(MachineOsCommand),
    /// Internal guest machine API daemon for macOS machine support.
    #[command(hide = true)]
    Api(MachineApiCommand),
}

#[derive(Debug, Args)]
struct MachineOsCommand {
    #[command(subcommand)]
    command: MachineOsSubcommand,
}

#[derive(Debug, Subcommand)]
enum MachineOsSubcommand {
    /// Apply a specific immutable OCI image reference or digest as the next machine OS.
    Apply(MachineOsApplyCommand),
    /// Upgrade the machine OS to the supported image that matches this neovex host version.
    Upgrade(MachineOsUpgradeCommand),
}

#[derive(Debug, Args)]
struct MachineOsApplyCommand {
    /// OCI image reference or digest to apply on the next machine boot.
    image: String,

    /// Stop and restart the machine immediately if it is running.
    #[arg(long)]
    restart: bool,
}

#[derive(Debug, Args)]
struct MachineOsUpgradeCommand {
    /// Only report whether an upgrade is available in the current supported release stream.
    #[arg(long)]
    dry_run: bool,

    /// Stop and restart the machine immediately if an upgrade is applied while it is running.
    #[arg(long)]
    restart: bool,
}

#[derive(Debug, Args)]
struct MachineInitCommand {
    /// Guest vCPU count to record in the machine config.
    #[arg(long, default_value_t = DEFAULT_MACHINE_CPUS)]
    cpus: u8,

    /// Guest memory size in MiB to record in the machine config.
    #[arg(long, default_value_t = DEFAULT_MACHINE_MEMORY_MIB)]
    memory_mib: u32,

    /// Guest disk size in GiB to record in the machine config.
    #[arg(long, default_value_t = DEFAULT_MACHINE_DISK_GIB)]
    disk_gib: u32,

    /// Guest image source. Accepts a published OCI reference, an absolute local
    /// raw-disk path, or an http(s) URL override for diagnostics.
    #[arg(long, default_value_t = default_machine_image())]
    image: String,

    /// Optional SSH identity path used for direct guest debugging on bootable
    /// local disk images.
    #[arg(long)]
    ssh_identity: Option<PathBuf>,

    /// Optional first-boot Ignition file to serve over the guest bootstrap
    /// vsock channel.
    #[arg(long)]
    ignition_file: Option<PathBuf>,

    /// Optional EFI variable-store path for booting an existing disk with its
    /// known-good firmware state.
    #[arg(long)]
    efi_store: Option<PathBuf>,

    /// Host:guest volume mapping to record for future virtiofs setup.
    #[arg(long = "volume", value_parser = parse_machine_volume)]
    volumes: Vec<MachineVolume>,
}

#[derive(Debug, Args)]
struct MachineStartCommand {}

#[derive(Debug, Args)]
struct MachineStopCommand {}

#[derive(Debug, Args)]
struct MachineStatusCommand {}

#[derive(Debug, Args)]
struct MachineSshCommand {
    /// Optional command to execute in the guest once SSH wiring is available.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

#[derive(Debug, Args)]
struct MachineRmCommand {}

#[derive(Debug, Args)]
struct MachineApiCommand {
    /// Direct unix socket path to bind for the guest machine API.
    #[arg(long, conflicts_with = "socket_activation")]
    socket_path: Option<PathBuf>,

    /// Inherit the listening unix socket from systemd socket activation.
    #[arg(long, conflicts_with = "socket_path")]
    socket_activation: bool,

    /// Optional override for the persisted control-plane directory root.
    #[arg(long)]
    control_data_dir: Option<PathBuf>,
}

pub(crate) async fn run_machine_command(command: MachineCommand) -> Result<(), Error> {
    let roots = resolve_roots_for_command(&command)?;
    run_machine_command_with_layout(command, &roots).await
}

fn resolve_roots_for_command(command: &MachineCommand) -> Result<MachineRootLayout, Error> {
    match &command.command {
        MachineSubcommand::Api(_) => MachineRootLayout::resolve()
            .or_else(|_| Ok(MachineRootLayout::guest_api_default(resolve_runtime_root()))),
        _ => MachineRootLayout::resolve(),
    }
}

pub(crate) fn require_default_machine_api_client() -> Result<MachineApiClient, Error> {
    let roots = MachineRootLayout::resolve()?;
    let (paths, state) = with_default_machine_lock(&roots, || {
        let (paths, _, state) = load_initialized_machine(&roots)?;
        Ok((paths, state))
    })?;
    if !matches!(state.lifecycle, MachineLifecycle::Running) {
        return Err(Error::InvalidInput(format!(
            "machine '{}' is {} and its guest machine API is not available; run `neovex machine start` first",
            DEFAULT_MACHINE_NAME,
            state.lifecycle.as_str()
        )));
    }
    if !paths.api_socket_path.exists() {
        return Err(Error::InvalidInput(format!(
            "machine '{}' is running but guest machine API socket {} is missing; run `neovex machine status` or restart the machine",
            DEFAULT_MACHINE_NAME,
            paths.api_socket_path.display()
        )));
    }

    let client = MachineApiClient::new(paths.api_socket_path.clone());
    client.health().map_err(|error| {
        Error::InvalidInput(format!(
            "machine '{}' guest machine API is not reachable at {}: {error}",
            DEFAULT_MACHINE_NAME,
            paths.api_socket_path.display()
        ))
    })?;
    Ok(client)
}

pub(crate) fn ensure_default_machine_api_client_started() -> Result<MachineApiClient, Error> {
    let roots = MachineRootLayout::resolve()?;
    let paths = with_default_machine_lock(&roots, || {
        let (paths, mut config, mut state) = load_initialized_machine(&roots)?;
        if !matches!(state.lifecycle, MachineLifecycle::Running) {
            paths.ensure_runtime_directories()?;
            start_machine(&paths, &mut config, &mut state)?;
        }
        Ok(paths)
    })?;

    if !paths.api_socket_path.exists() {
        return Err(Error::InvalidInput(format!(
            "machine '{}' started but guest machine API socket {} is missing; run `neovex machine status` or restart the machine",
            DEFAULT_MACHINE_NAME,
            paths.api_socket_path.display()
        )));
    }

    let client = MachineApiClient::new(paths.api_socket_path.clone());
    client.health().map_err(|error| {
        Error::InvalidInput(format!(
            "machine '{}' guest machine API is not reachable at {} after startup: {error}",
            DEFAULT_MACHINE_NAME,
            paths.api_socket_path.display()
        ))
    })?;
    Ok(client)
}

async fn run_machine_command_with_layout(
    command: MachineCommand,
    roots: &MachineRootLayout,
) -> Result<(), Error> {
    match command.command {
        MachineSubcommand::Init(init) => {
            with_default_machine_lock(roots, || run_machine_init(init, roots))
        }
        MachineSubcommand::Start(start) => {
            with_default_machine_lock(roots, || run_machine_start(start, roots))
        }
        MachineSubcommand::Stop(stop) => {
            with_default_machine_lock(roots, || run_machine_stop(stop, roots))
        }
        MachineSubcommand::Status(status) => {
            with_default_machine_lock(roots, || run_machine_status(status, roots))
        }
        MachineSubcommand::Ssh(ssh) => {
            with_default_machine_lock(roots, || run_machine_ssh(ssh, roots))
        }
        MachineSubcommand::Rm(remove) => {
            with_default_machine_lock(roots, || run_machine_rm(remove, roots))
        }
        MachineSubcommand::Os(os) => with_default_machine_lock(roots, || run_machine_os(os, roots)),
        MachineSubcommand::Api(api) => api::run_machine_api_command(api, roots).await,
    }
}

fn run_machine_init(command: MachineInitCommand, roots: &MachineRootLayout) -> Result<(), Error> {
    let paths = roots.paths(DEFAULT_MACHINE_NAME);
    if paths.config_path.exists() {
        return Err(Error::AlreadyExists(format!(
            "machine '{}' is already initialized at {}",
            DEFAULT_MACHINE_NAME,
            paths.config_path.display()
        )));
    }

    paths.ensure_directories()?;
    let config = MachineConfigRecord {
        version: CURRENT_MACHINE_CONFIG_VERSION,
        name: DEFAULT_MACHINE_NAME.to_owned(),
        provider: MachineProvider::Krunkit,
        guest: MachineGuestConfig {
            image_source: MachineImageSource::parse(&command.image)?,
            ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
            ssh_identity_path: command.ssh_identity,
            ignition_file_path: command.ignition_file,
            efi_variable_store_path: command.efi_store,
        },
        resources: MachineResources {
            cpus: command.cpus,
            memory_mib: command.memory_mib,
            disk_gib: command.disk_gib,
        },
        volumes: if command.volumes.is_empty() {
            default_machine_volumes()
        } else {
            command.volumes
        },
        roots: roots.clone(),
    };
    let state = MachineStateRecord::initialized();
    write_json_file(&paths.config_path, &config)?;
    write_json_file(&paths.state_path, &state)?;

    print!(
        "{}",
        render_machine_view(
            MachineCommandResult::Initialized,
            &paths,
            Some(&config),
            Some(&state)
        )?
    );
    Ok(())
}

fn run_machine_start(
    _command: MachineStartCommand,
    roots: &MachineRootLayout,
) -> Result<(), Error> {
    let (paths, mut config, mut state) = load_initialized_machine(roots)?;
    paths.ensure_runtime_directories()?;
    start_machine(&paths, &mut config, &mut state)?;
    print!(
        "{}",
        render_machine_view(
            MachineCommandResult::Started,
            &paths,
            Some(&config),
            Some(&state)
        )?
    );
    Ok(())
}

fn run_machine_stop(_command: MachineStopCommand, roots: &MachineRootLayout) -> Result<(), Error> {
    let (paths, config, mut state) = load_initialized_machine(roots)?;
    stop_machine(&paths, &config, &mut state)?;
    print!(
        "{}",
        render_machine_view(
            MachineCommandResult::Stopped,
            &paths,
            Some(&config),
            Some(&state)
        )?
    );
    Ok(())
}

fn run_machine_status(
    _command: MachineStatusCommand,
    roots: &MachineRootLayout,
) -> Result<(), Error> {
    let paths = roots.paths(DEFAULT_MACHINE_NAME);
    let config = load_machine_config_if_exists(&paths.config_path)?;
    let mut state = load_machine_state_if_exists(&paths.state_path)?;
    if let Some(state) = state.as_mut() {
        refresh_machine_state(&paths, state)?;
    }
    let result = if config.is_some() {
        MachineCommandResult::Status
    } else {
        MachineCommandResult::Uninitialized
    };
    print!(
        "{}",
        render_machine_view(result, &paths, config.as_ref(), state.as_ref())?
    );
    Ok(())
}

fn run_machine_ssh(command: MachineSshCommand, roots: &MachineRootLayout) -> Result<(), Error> {
    let (paths, config, mut state) = load_initialized_machine(roots)?;
    refresh_machine_state(&paths, &mut state)?;
    write_json_file(&paths.state_path, &state)?;

    let mut ssh = build_ssh_command(&config, &state)?;
    ssh.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    ssh.args(command.args);

    let status = ssh
        .status()
        .map_err(|error| Error::Internal(format!("failed to start ssh: {error}")))?;
    if status.success() {
        return Ok(());
    }
    Err(Error::Internal(format!(
        "ssh exited unsuccessfully with status {status}"
    )))
}

fn run_machine_rm(_command: MachineRmCommand, roots: &MachineRootLayout) -> Result<(), Error> {
    let paths = roots.paths(DEFAULT_MACHINE_NAME);
    let config = load_machine_config_if_exists(&paths.config_path)?;
    let state = load_machine_state_if_exists(&paths.state_path)?;

    if let Some(state) = state.as_ref()
        && matches!(
            state.lifecycle,
            MachineLifecycle::Starting | MachineLifecycle::Running
        )
    {
        return Err(Error::Conflict(format!(
            "machine '{}' is {} and cannot be removed safely",
            DEFAULT_MACHINE_NAME,
            state.lifecycle.as_str()
        )));
    }

    release_machine_ssh_port(roots, DEFAULT_MACHINE_NAME)?;
    remove_dir_if_exists(&paths.config_dir)?;
    remove_dir_if_exists(&paths.state_dir)?;
    remove_machine_runtime_artifacts(&paths)?;
    remove_dir_if_empty(&paths.runtime_dir)?;

    print!(
        "{}",
        render_machine_view(
            MachineCommandResult::Removed,
            &paths,
            config.as_ref(),
            state.as_ref()
        )?
    );
    Ok(())
}

fn run_machine_os(command: MachineOsCommand, roots: &MachineRootLayout) -> Result<(), Error> {
    match command.command {
        MachineOsSubcommand::Apply(apply) => run_machine_os_apply(apply, roots),
        MachineOsSubcommand::Upgrade(upgrade) => run_machine_os_upgrade(upgrade, roots),
    }
}

fn run_machine_os_apply(
    command: MachineOsApplyCommand,
    roots: &MachineRootLayout,
) -> Result<(), Error> {
    let (paths, mut config, mut state) = load_initialized_machine(roots)?;
    let target_source = parse_machine_os_apply_source(&command.image)?;
    let outcome = apply_machine_os_change(
        &paths,
        &mut config,
        &mut state,
        target_source,
        command.restart,
    )?;

    let result = if outcome.changed {
        MachineOsCommandResult::Applied
    } else {
        MachineOsCommandResult::AlreadyCurrent
    };
    print!(
        "{}",
        render_machine_os_apply_view(result, &paths, &outcome, command.restart)?
    );
    Ok(())
}

fn run_machine_os_upgrade(
    command: MachineOsUpgradeCommand,
    roots: &MachineRootLayout,
) -> Result<(), Error> {
    let (paths, mut config, mut state) = load_initialized_machine(roots)?;
    let plan = plan_machine_os_upgrade(&config)?;
    if command.dry_run || !plan.update_available {
        let result = if plan.update_available {
            MachineOsCommandResult::UpgradeCheck
        } else {
            MachineOsCommandResult::AlreadyCurrent
        };
        print!(
            "{}",
            render_machine_os_upgrade_view(result, &paths, &plan, command.dry_run, false, false)?
        );
        return Ok(());
    }

    let outcome = apply_machine_os_change(
        &paths,
        &mut config,
        &mut state,
        MachineImageSource::OciReference {
            reference: plan.target_image.clone(),
        },
        command.restart,
    )?;
    print!(
        "{}",
        render_machine_os_upgrade_view(
            MachineOsCommandResult::Upgraded,
            &paths,
            &plan,
            false,
            command.restart,
            outcome.restarted,
        )?
    );
    Ok(())
}

fn load_initialized_machine(
    roots: &MachineRootLayout,
) -> Result<(MachinePaths, MachineConfigRecord, MachineStateRecord), Error> {
    let paths = roots.paths(DEFAULT_MACHINE_NAME);
    let config = load_machine_config_if_exists(&paths.config_path)?.ok_or_else(|| {
        Error::InvalidInput(format!(
            "machine '{}' is not initialized; run `neovex machine init` first",
            DEFAULT_MACHINE_NAME
        ))
    })?;
    let mut state = load_machine_state_if_exists(&paths.state_path)?
        .unwrap_or_else(MachineStateRecord::initialized);
    refresh_machine_state(&paths, &mut state)?;
    write_json_file(&paths.state_path, &state)?;
    Ok((paths, config, state))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MachineOsApplyOutcome {
    previous_image: String,
    current_image: String,
    changed: bool,
    restarted: bool,
    lifecycle: MachineLifecycle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MachineOsUpgradePlan {
    current_image: String,
    current_version: String,
    target_image: String,
    target_version: String,
    update_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MachineOsUpgradeStream {
    repository: &'static str,
    additional_supported_repositories: &'static [&'static str],
    target_image: String,
    target_version: String,
    follows_host_release: bool,
}

fn apply_machine_os_change(
    paths: &MachinePaths,
    config: &mut MachineConfigRecord,
    state: &mut MachineStateRecord,
    target_source: MachineImageSource,
    restart: bool,
) -> Result<MachineOsApplyOutcome, Error> {
    let previous_image = describe_machine_image_source(&config.guest.image_source);
    let current_image = describe_machine_image_source(&target_source);
    if config.guest.image_source == target_source {
        return Ok(MachineOsApplyOutcome {
            previous_image,
            current_image,
            changed: false,
            restarted: false,
            lifecycle: state.lifecycle,
        });
    }

    if matches!(state.lifecycle, MachineLifecycle::Starting) {
        return Err(Error::Conflict(format!(
            "machine '{}' is starting; wait for startup to finish before applying a machine OS change",
            DEFAULT_MACHINE_NAME
        )));
    }
    let was_running = matches!(state.lifecycle, MachineLifecycle::Running);
    if was_running && !restart {
        return Err(Error::Conflict(format!(
            "machine '{}' is running; rerun with `--restart` to apply a machine OS change safely",
            DEFAULT_MACHINE_NAME
        )));
    }
    if was_running {
        stop_machine(paths, config, state)?;
    }

    config.guest.image_source = target_source;
    invalidate_materialized_machine_os(paths)?;
    *state = MachineStateRecord::initialized();
    write_json_file(&paths.config_path, config)?;
    write_json_file(&paths.state_path, state)?;

    let restarted = if restart {
        start_machine(paths, config, state)?;
        true
    } else {
        false
    };

    Ok(MachineOsApplyOutcome {
        previous_image,
        current_image,
        changed: true,
        restarted,
        lifecycle: state.lifecycle,
    })
}

fn plan_machine_os_upgrade(config: &MachineConfigRecord) -> Result<MachineOsUpgradePlan, Error> {
    let reference = current_machine_oci_reference(config)?;
    let stream = default_machine_os_upgrade_stream(config);
    let repository = machine_image_reference_repository(reference.as_str());
    let repository_supported = repository == stream.repository
        || stream
            .additional_supported_repositories
            .contains(&repository.as_str());
    if !repository_supported {
        return Err(Error::InvalidInput(format!(
            "machine os upgrade only supports the default release stream '{}'; current image source is '{}'. Use `neovex machine os apply <oci-ref-or-digest>` for explicit rollouts instead.",
            stream.repository, reference
        )));
    }
    if cfg!(target_os = "macos") && config.provider == MachineProvider::Krunkit {
        let current_version = machine_image_reference_version_label(&reference);
        let update_available = reference != stream.target_image;
        return Ok(MachineOsUpgradePlan {
            current_image: reference.clone(),
            current_version: current_version.clone(),
            target_image: stream.target_image,
            target_version: stream.target_version.clone(),
            update_available,
        });
    }

    let (_, current_tag) = split_tagged_machine_image_reference(reference.as_str())?;
    let current_version = parse_machine_release_version(&current_tag)?;
    let target_version = parse_machine_release_version(&stream.target_version)?;
    if stream.follows_host_release && current_version > target_version {
        return Err(Error::Conflict(format!(
            "configured machine image version {} is newer than the supported machine stream version {}. Install a matching neovex build or use `neovex machine os apply <oci-ref-or-digest>` explicitly.",
            current_tag, stream.target_version
        )));
    }

    Ok(MachineOsUpgradePlan {
        current_image: reference,
        current_version: current_tag.clone(),
        target_image: stream.target_image,
        target_version: stream.target_version.clone(),
        update_available: current_tag != stream.target_version,
    })
}

fn default_machine_os_upgrade_stream(config: &MachineConfigRecord) -> MachineOsUpgradeStream {
    match config.provider {
        MachineProvider::Krunkit if cfg!(target_os = "macos") => MachineOsUpgradeStream {
            repository: DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY,
            additional_supported_repositories: &[DEFAULT_NEOVEX_MACHINE_IMAGE_REPOSITORY],
            target_image: default_machine_image_for_provider(config.provider),
            target_version: DEFAULT_PODMAN_MACHINE_IMAGE_DIGEST.to_owned(),
            follows_host_release: false,
        },
        MachineProvider::Krunkit | MachineProvider::Wsl2 => MachineOsUpgradeStream {
            repository: DEFAULT_NEOVEX_MACHINE_IMAGE_REPOSITORY,
            additional_supported_repositories: &[],
            target_image: default_machine_image_for_provider(config.provider),
            target_version: current_machine_release_tag(),
            follows_host_release: true,
        },
    }
}

fn parse_machine_os_apply_source(value: &str) -> Result<MachineImageSource, Error> {
    match MachineImageSource::parse(value)? {
        source @ MachineImageSource::OciReference { .. } => Ok(source),
        MachineImageSource::HttpUrl { .. } => Err(Error::InvalidInput(
            "machine os apply requires an OCI image reference or digest; HTTP URLs are only supported for diagnostic machine init overrides".to_owned(),
        )),
        MachineImageSource::LocalDisk { .. } => Err(Error::InvalidInput(
            "machine os apply requires an OCI image reference or digest; local raw disks are only supported for diagnostic machine init overrides".to_owned(),
        )),
    }
}

fn current_machine_oci_reference(config: &MachineConfigRecord) -> Result<String, Error> {
    match &config.guest.image_source {
        MachineImageSource::OciReference { reference } => Ok(reference.clone()),
        MachineImageSource::HttpUrl { url } => Err(Error::InvalidInput(format!(
            "machine os upgrade only supports OCI image sources, but this machine uses HTTP override '{}'. Use `neovex machine os apply <oci-ref-or-digest>` to return to a supported release stream.",
            url
        ))),
        MachineImageSource::LocalDisk { path } => Err(Error::InvalidInput(format!(
            "machine os upgrade only supports OCI image sources, but this machine uses local disk '{}'. Use `neovex machine os apply <oci-ref-or-digest>` to return to a supported release stream.",
            path.display()
        ))),
    }
}

fn split_tagged_machine_image_reference(reference: &str) -> Result<(String, String), Error> {
    let stripped = reference.trim_start_matches("docker://");
    if stripped.contains('@') {
        return Err(Error::InvalidInput(format!(
            "machine os upgrade requires a tagged OCI reference in the supported release stream, but '{}' is digest-pinned. Use `neovex machine os apply <oci-ref-or-digest>` for explicit pinned rollouts.",
            reference
        )));
    }
    let Some(last_component) = stripped.rsplit('/').next() else {
        return Err(Error::InvalidInput(format!(
            "machine image reference '{}' is not a valid tagged OCI reference",
            reference
        )));
    };
    let Some((_, tag)) = last_component.rsplit_once(':') else {
        return Err(Error::InvalidInput(format!(
            "machine image reference '{}' is missing a release tag. Use `neovex machine os apply <oci-ref-or-digest>` for explicit pinned rollouts.",
            reference
        )));
    };
    let repository = stripped
        .rsplit_once(':')
        .map(|(repository, _)| repository)
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "machine image reference '{}' is not a valid tagged OCI reference",
                reference
            ))
        })?;
    Ok((repository.to_owned(), tag.to_owned()))
}

fn parse_machine_release_version(tag: &str) -> Result<Version, Error> {
    let normalized = tag.strip_prefix('v').unwrap_or(tag);
    let normalized = match normalized.matches('.').count() {
        0 => format!("{normalized}.0.0"),
        1 => format!("{normalized}.0"),
        _ => normalized.to_owned(),
    };
    Version::parse(&normalized).map_err(|error| {
        Error::InvalidInput(format!(
            "machine image tag '{}' is not a supported semantic version tag: {error}",
            tag
        ))
    })
}

fn describe_machine_image_source(source: &MachineImageSource) -> String {
    match source {
        MachineImageSource::OciReference { reference } => reference.clone(),
        MachineImageSource::HttpUrl { url } => url.clone(),
        MachineImageSource::LocalDisk { path } => path.display().to_string(),
    }
}

fn invalidate_materialized_machine_os(paths: &MachinePaths) -> Result<(), Error> {
    remove_file_if_exists(&paths.materialized_image_path)?;
    remove_file_if_exists(&paths.efi_variable_store_path)
}

fn render_machine_view(
    result: MachineCommandResult,
    paths: &MachinePaths,
    config: Option<&MachineConfigRecord>,
    state: Option<&MachineStateRecord>,
) -> Result<String, Error> {
    let machine_image_contract = config.map(|config| {
        let desired_source = desired_machine_image_source(config);
        let configured_image = describe_machine_image_source(&config.guest.image_source);
        let desired_image = describe_machine_image_source(&desired_source);
        let recorded_image = state
            .and_then(|state| state.runtime.as_ref())
            .map(|runtime| runtime.machine_image_source.clone())
            .filter(|recorded| !recorded.is_empty());
        let recorded_matches_desired = recorded_image.as_deref() == Some(desired_image.as_str());
        let materialized_image_exists = paths.materialized_image_path.is_file();
        let efi_store_exists = paths.efi_variable_store_path.exists();
        let rebuild_reason = if recorded_image.is_none()
            && (materialized_image_exists || efi_store_exists)
        {
            Some(
                "boot artifacts exist, but no recorded base-image identity is available".to_owned(),
            )
        } else if !recorded_matches_desired {
            recorded_image.as_ref().map(|recorded| {
                format!(
                    "recorded base image '{}' differs from desired '{}'",
                    recorded, desired_image
                )
            })
        } else {
            None
        };
        MachineImageContractStatusView {
            host_managed: uses_host_managed_machine_image_contract(config),
            configured_image,
            desired_image,
            recorded_image,
            recorded_matches_desired,
            materialized_image_path: paths.materialized_image_path.clone(),
            materialized_image_exists,
            efi_store_path: paths.efi_variable_store_path.clone(),
            efi_store_exists,
            rebuild_required: rebuild_reason.is_some(),
            rebuild_reason,
        }
    });
    let view = MachineStatusView {
        result,
        initialized: config.is_some(),
        name: DEFAULT_MACHINE_NAME.to_owned(),
        lifecycle: state
            .map(|state| state.lifecycle)
            .unwrap_or(MachineLifecycle::Uninitialized),
        manager: state
            .map(|state| state.manager)
            .unwrap_or(MachineManagerState::Unconfigured),
        provider: config.map(|config| config.provider),
        guest: config.map(|config| config.guest.clone()),
        resources: config.map(|config| config.resources),
        volumes: config
            .map(|config| config.volumes.clone())
            .unwrap_or_default(),
        roots: roots_view(paths),
        paths: paths.clone(),
        runtime: state.and_then(|state| state.runtime.clone()),
        machine_image_contract,
        machine_api: machine_api_status_view(paths, config),
        last_error: state.and_then(|state| state.last_error.clone()),
    };
    serde_yaml::to_string(&view)
        .map_err(|error| Error::Internal(format!("failed to serialize machine status: {error}")))
}

fn render_machine_os_apply_view(
    result: MachineOsCommandResult,
    paths: &MachinePaths,
    outcome: &MachineOsApplyOutcome,
    restart_requested: bool,
) -> Result<String, Error> {
    let view = MachineOsApplyStatusView {
        result,
        name: paths.name.clone(),
        previous_image: outcome.previous_image.clone(),
        current_image: outcome.current_image.clone(),
        image_changed: outcome.changed,
        restart_requested,
        restarted: outcome.restarted,
        lifecycle: outcome.lifecycle,
    };
    serde_yaml::to_string(&view).map_err(|error| {
        Error::Internal(format!(
            "failed to serialize machine os apply status: {error}"
        ))
    })
}

fn render_machine_os_upgrade_view(
    result: MachineOsCommandResult,
    paths: &MachinePaths,
    plan: &MachineOsUpgradePlan,
    dry_run: bool,
    restart_requested: bool,
    restarted: bool,
) -> Result<String, Error> {
    let view = MachineOsUpgradeStatusView {
        result,
        name: paths.name.clone(),
        current_image: plan.current_image.clone(),
        current_version: plan.current_version.clone(),
        target_image: plan.target_image.clone(),
        target_version: plan.target_version.clone(),
        update_available: plan.update_available,
        dry_run,
        restart_requested,
        restarted,
    };
    serde_yaml::to_string(&view).map_err(|error| {
        Error::Internal(format!(
            "failed to serialize machine os upgrade status: {error}"
        ))
    })
}

fn machine_api_status_view(
    paths: &MachinePaths,
    config: Option<&MachineConfigRecord>,
) -> MachineApiStatusView {
    let socket_path = paths.api_socket_path.clone();
    let exists = socket_path.exists();
    let guest_socket_path = config
        .and_then(|config| config.guest.ssh_identity_path.as_ref())
        .map(|_| PathBuf::from(bootstrap::GUEST_NEOVEX_SOCKET));
    let transport = guest_socket_path
        .as_ref()
        .map(|_| MACHINE_API_FORWARD_TRANSPORT.to_owned());
    let forward_user = guest_socket_path
        .as_ref()
        .map(|_| MACHINE_API_FORWARD_USER.to_owned());
    let identity_path = config.and_then(|config| config.guest.ssh_identity_path.clone());
    if !exists {
        return MachineApiStatusView {
            socket_path,
            guest_socket_path,
            transport,
            forward_user,
            identity_path,
            exists,
            reachable: false,
            role: None,
            protocol_version: None,
            listen_mode: None,
            capabilities: None,
            error: None,
        };
    }

    let client = MachineApiClient::new(socket_path.clone());
    match client.health() {
        Ok(health) => match client.capabilities() {
            Ok(capabilities) => MachineApiStatusView {
                socket_path,
                guest_socket_path,
                transport,
                forward_user,
                identity_path,
                exists,
                reachable: true,
                role: Some(health.role),
                protocol_version: Some(health.protocol_version),
                listen_mode: Some(health.listen_mode),
                capabilities: Some(capabilities),
                error: None,
            },
            Err(error) => MachineApiStatusView {
                socket_path,
                guest_socket_path,
                transport,
                forward_user,
                identity_path,
                exists,
                reachable: true,
                role: Some(health.role),
                protocol_version: Some(health.protocol_version),
                listen_mode: Some(health.listen_mode),
                capabilities: None,
                error: Some(format!(
                    "machine API health succeeded, but capability discovery failed: {error}"
                )),
            },
        },
        Err(error) => MachineApiStatusView {
            socket_path,
            guest_socket_path,
            transport,
            forward_user,
            identity_path,
            exists,
            reachable: false,
            role: None,
            protocol_version: None,
            listen_mode: None,
            capabilities: None,
            error: Some(error.to_string()),
        },
    }
}

fn roots_view(paths: &MachinePaths) -> MachineRootsView {
    MachineRootsView {
        config_root: paths
            .config_dir
            .parent()
            .expect("machine config dir should have a root")
            .to_path_buf(),
        state_root: paths
            .state_dir
            .parent()
            .expect("machine state dir should have a root")
            .to_path_buf(),
        runtime_root: paths.runtime_dir.clone(),
    }
}

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

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<(), Error> {
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
fn read_json_file_if_exists<T>(path: &Path) -> Result<Option<T>, Error>
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

fn load_machine_config_if_exists(path: &Path) -> Result<Option<MachineConfigRecord>, Error> {
    let Some(bytes) = read_file_if_exists(path)? else {
        return Ok(None);
    };

    let version = probe_machine_record_version(path, &bytes, "machine config")?;
    match version {
        CURRENT_MACHINE_CONFIG_VERSION => {
            let config =
                parse_machine_record::<MachineConfigRecord>(path, &bytes, "machine config")?;
            Ok(Some(config))
        }
        0 => {
            let mut config =
                parse_machine_record::<MachineConfigRecord>(path, &bytes, "machine config")?;
            config.version = CURRENT_MACHINE_CONFIG_VERSION;
            write_json_file(path, &config)?;
            Ok(Some(config))
        }
        newer if newer > CURRENT_MACHINE_CONFIG_VERSION => Err(Error::InvalidInput(format!(
            "machine config at {} uses newer schema version {}; this neovex build supports version {}. Upgrade neovex or recreate the machine.",
            path.display(),
            newer,
            CURRENT_MACHINE_CONFIG_VERSION
        ))),
        older => Err(Error::InvalidInput(format!(
            "machine config at {} uses unsupported schema version {}; this neovex build supports version {}. Recreate the machine with `neovex machine rm` then `neovex machine init`.",
            path.display(),
            older,
            CURRENT_MACHINE_CONFIG_VERSION
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

fn load_machine_state_if_exists(path: &Path) -> Result<Option<MachineStateRecord>, Error> {
    let Some(bytes) = read_file_if_exists(path)? else {
        return Ok(None);
    };

    let version = match probe_machine_record_version(path, &bytes, "machine state") {
        Ok(version) => version,
        Err(error) => return rebuild_machine_state(path, error.to_string()).map(Some),
    };

    match version {
        CURRENT_MACHINE_STATE_VERSION => {
            match parse_machine_record::<MachineStateRecord>(path, &bytes, "machine state") {
                Ok(state) => Ok(Some(state)),
                Err(error) => rebuild_machine_state(path, error.to_string()).map(Some),
            }
        }
        0 => match parse_machine_record::<MachineStateRecord>(path, &bytes, "machine state") {
            Ok(mut state) => {
                state.version = CURRENT_MACHINE_STATE_VERSION;
                write_json_file(path, &state)?;
                Ok(Some(state))
            }
            Err(error) => rebuild_machine_state(path, error.to_string()).map(Some),
        },
        newer if newer > CURRENT_MACHINE_STATE_VERSION => rebuild_machine_state(
            path,
            format!(
                "machine state at {} used newer schema version {}; rebuilt with version {}",
                path.display(),
                newer,
                CURRENT_MACHINE_STATE_VERSION
            ),
        )
        .map(Some),
        older => rebuild_machine_state(
            path,
            format!(
                "machine state at {} used unsupported schema version {}; rebuilt with version {}",
                path.display(),
                older,
                CURRENT_MACHINE_STATE_VERSION
            ),
        )
        .map(Some),
    }
}

fn remove_dir_if_exists(path: &Path) -> Result<(), Error> {
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

fn with_default_machine_lock<T>(
    roots: &MachineRootLayout,
    operation: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    let _lock = lock_machine_records(roots, DEFAULT_MACHINE_NAME)?;
    operation()
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

fn remove_dir_if_empty(path: &Path) -> Result<(), Error> {
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

fn remove_file_if_exists(path: &Path) -> Result<(), Error> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Internal(format!(
            "failed to remove {}: {error}",
            path.display()
        ))),
    }
}

fn remove_machine_runtime_artifacts(paths: &MachinePaths) -> Result<(), Error> {
    for path in [
        &paths.api_socket_path,
        &paths.ready_socket_path,
        &paths.ignition_socket_path,
        &paths.gvproxy_socket_path,
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

fn resolve_config_root() -> Result<PathBuf, Error> {
    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(path).join("neovex").join("machine"));
    }
    Ok(resolve_home_dir()?
        .join(".config")
        .join("neovex")
        .join("machine"))
}

fn resolve_state_root() -> Result<PathBuf, Error> {
    if let Some(path) = env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(path).join("neovex").join("machine"));
    }
    Ok(resolve_home_dir()?
        .join(".local")
        .join("state")
        .join("neovex")
        .join("machine"))
}

fn resolve_home_dir() -> Result<PathBuf, Error> {
    env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
        Error::InvalidInput("HOME is not set; cannot resolve machine directories".to_owned())
    })
}

fn resolve_runtime_root() -> PathBuf {
    env::var_os(MACHINE_RUNTIME_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_MACHINE_RUNTIME_ROOT))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MachineRootLayout {
    config_root: PathBuf,
    state_root: PathBuf,
    runtime_root: PathBuf,
}

impl MachineRootLayout {
    fn resolve() -> Result<Self, Error> {
        Ok(Self {
            config_root: resolve_config_root()?,
            state_root: resolve_state_root()?,
            runtime_root: resolve_runtime_root(),
        })
    }

    fn guest_api_default(runtime_root: PathBuf) -> Self {
        Self {
            config_root: PathBuf::from("/var/lib/neovex/machine/config"),
            state_root: PathBuf::from("/var/lib/neovex/machine/state"),
            runtime_root,
        }
    }

    #[cfg(test)]
    fn new(config_root: PathBuf, state_root: PathBuf, runtime_root: PathBuf) -> Self {
        Self {
            config_root,
            state_root,
            runtime_root,
        }
    }

    fn lock_path(&self, name: &str) -> PathBuf {
        self.state_root.join(format!("{name}.lock"))
    }

    #[cfg(any(unix, test))]
    fn port_allocation_state_path(&self) -> PathBuf {
        self.state_root.join("port-alloc.dat")
    }

    #[cfg(any(unix, test))]
    fn port_allocation_lock_path(&self) -> PathBuf {
        self.state_root.join("port-alloc.lck")
    }

    fn paths(&self, name: &str) -> MachinePaths {
        let config_dir = self.config_root.join(name);
        let state_dir = self.state_root.join(name);
        let runtime_dir = self.runtime_root.clone();
        MachinePaths {
            name: name.to_owned(),
            config_dir: config_dir.clone(),
            state_dir: state_dir.clone(),
            runtime_dir: runtime_dir.clone(),
            config_path: config_dir.join("config.json"),
            generated_ignition_path: config_dir.join("generated.ign"),
            state_path: state_dir.join("status.json"),
            image_cache_dir: state_dir.join("images"),
            materialized_image_path: state_dir.join("images").join(format!("{name}.raw")),
            api_socket_path: runtime_dir.join(format!("{name}-api.sock")),
            ready_socket_path: runtime_dir.join(format!("{name}.sock")),
            ignition_socket_path: runtime_dir.join(format!("{name}-ignition.sock")),
            gvproxy_socket_path: runtime_dir.join(format!("{name}-gvproxy.sock")),
            krunkit_endpoint_path: runtime_dir.join(format!("{name}-krunkit.sock")),
            efi_variable_store_path: state_dir.join("efi-variable-store"),
            gvproxy_pid_path: runtime_dir.join(format!("{name}-gvproxy.pid")),
            krunkit_pid_path: runtime_dir.join(format!("{name}-krunkit.pid")),
            machine_log_path: runtime_dir.join(format!("{name}.log")),
            gvproxy_log_path: runtime_dir.join(format!("{name}-gvproxy.log")),
            krunkit_log_path: runtime_dir.join(format!("{name}-krunkit.log")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MachineConfigRecord {
    #[serde(default)]
    version: u32,
    name: String,
    provider: MachineProvider,
    guest: MachineGuestConfig,
    resources: MachineResources,
    volumes: Vec<MachineVolume>,
    roots: MachineRootLayout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MachineGuestConfig {
    image_source: MachineImageSource,
    ssh_user: String,
    ssh_identity_path: Option<PathBuf>,
    ignition_file_path: Option<PathBuf>,
    efi_variable_store_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum MachineImageSource {
    OciReference { reference: String },
    HttpUrl { url: String },
    LocalDisk { path: PathBuf },
}

impl MachineImageSource {
    fn parse(value: &str) -> Result<Self, Error> {
        let value = value.trim();
        if value.is_empty() {
            return Err(Error::InvalidInput(
                "machine image source cannot be empty".to_owned(),
            ));
        }

        if value.starts_with("http://") || value.starts_with("https://") {
            return Ok(Self::HttpUrl {
                url: value.to_owned(),
            });
        }

        if value.starts_with("docker://") {
            return Ok(Self::OciReference {
                reference: value.to_owned(),
            });
        }

        let path = PathBuf::from(value);
        if path.is_absolute() {
            return Ok(Self::LocalDisk { path });
        }

        Ok(Self::OciReference {
            reference: format!("docker://{value}"),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct MachineResources {
    cpus: u8,
    memory_mib: u32,
    disk_gib: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MachineVolume {
    source: PathBuf,
    target: PathBuf,
}

impl MachineVolume {
    fn parse(value: &str) -> Result<Self, String> {
        let (source, target) = value.split_once(':').ok_or_else(|| {
            format!("invalid machine volume '{value}'; expected <source>:<target>")
        })?;
        if source.is_empty() || target.is_empty() {
            return Err(format!(
                "invalid machine volume '{value}'; expected non-empty <source>:<target>"
            ));
        }
        let source = PathBuf::from(source);
        let target = PathBuf::from(target);
        if !source.is_absolute() {
            return Err(format!(
                "invalid machine volume '{value}'; source path must be absolute"
            ));
        }
        if !target.is_absolute() {
            return Err(format!(
                "invalid machine volume '{value}'; target path must be absolute"
            ));
        }
        Ok(Self { source, target })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MachineStateRecord {
    #[serde(default)]
    version: u32,
    lifecycle: MachineLifecycle,
    manager: MachineManagerState,
    runtime: Option<MachineRuntimeState>,
    last_error: Option<String>,
}

impl MachineStateRecord {
    fn initialized() -> Self {
        Self {
            version: CURRENT_MACHINE_STATE_VERSION,
            lifecycle: MachineLifecycle::Stopped,
            manager: MachineManagerState::Unconfigured,
            runtime: None,
            last_error: None,
        }
    }

    fn rebuilt(reason: impl Into<String>) -> Self {
        Self {
            version: CURRENT_MACHINE_STATE_VERSION,
            lifecycle: MachineLifecycle::Stopped,
            manager: MachineManagerState::Stale,
            runtime: None,
            last_error: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum MachineProvider {
    Krunkit,
    Wsl2,
}

#[cfg(any(unix, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MachineImageFormat {
    Raw,
    Tar,
}

#[cfg(any(unix, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MachineBootstrapMode {
    Ignition,
    ShellScript,
}

#[cfg(any(unix, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MachineProviderCapabilities {
    uses_provider_networking: bool,
    requires_exclusive_active: bool,
    image_format: MachineImageFormat,
    bootstrap_mode: MachineBootstrapMode,
    oci_artifact_disk_type: &'static str,
}

#[cfg(any(unix, test))]
const KRUNKIT_PROVIDER_CAPABILITIES: MachineProviderCapabilities = MachineProviderCapabilities {
    uses_provider_networking: false,
    requires_exclusive_active: true,
    image_format: MachineImageFormat::Raw,
    bootstrap_mode: MachineBootstrapMode::Ignition,
    oci_artifact_disk_type: "applehv",
};

#[cfg(any(unix, test))]
const WSL2_PROVIDER_CAPABILITIES: MachineProviderCapabilities = MachineProviderCapabilities {
    uses_provider_networking: true,
    requires_exclusive_active: false,
    image_format: MachineImageFormat::Tar,
    bootstrap_mode: MachineBootstrapMode::ShellScript,
    oci_artifact_disk_type: "wsl",
};

impl MachineProvider {
    #[cfg(any(unix, test))]
    fn capabilities(self) -> MachineProviderCapabilities {
        match self {
            Self::Krunkit => KRUNKIT_PROVIDER_CAPABILITIES,
            Self::Wsl2 => WSL2_PROVIDER_CAPABILITIES,
        }
    }

    #[cfg(any(unix, test))]
    fn uses_provider_networking(self) -> bool {
        self.capabilities().uses_provider_networking
    }

    #[cfg(any(unix, test))]
    fn requires_exclusive_active(self) -> bool {
        self.capabilities().requires_exclusive_active
    }

    #[cfg(any(unix, test))]
    fn image_format(self) -> MachineImageFormat {
        self.capabilities().image_format
    }

    #[cfg(any(unix, test))]
    fn bootstrap_mode(self) -> MachineBootstrapMode {
        self.capabilities().bootstrap_mode
    }

    #[cfg(any(unix, test))]
    fn oci_artifact_disk_type(self) -> &'static str {
        self.capabilities().oci_artifact_disk_type
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum MachineLifecycle {
    Uninitialized,
    Stopped,
    Starting,
    Running,
    Failed,
}

impl MachineLifecycle {
    fn as_str(self) -> &'static str {
        match self {
            Self::Uninitialized => "uninitialized",
            Self::Stopped => "stopped",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum MachineManagerState {
    Unconfigured,
    HelpersResolved,
    Launching,
    Ready,
    Failed,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum MachineCommandResult {
    Initialized,
    Started,
    Status,
    Stopped,
    Removed,
    Uninitialized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum MachineOsCommandResult {
    Applied,
    UpgradeCheck,
    Upgraded,
    AlreadyCurrent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct MachineRootsView {
    config_root: PathBuf,
    state_root: PathBuf,
    runtime_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct MachineStatusView {
    result: MachineCommandResult,
    initialized: bool,
    name: String,
    lifecycle: MachineLifecycle,
    manager: MachineManagerState,
    provider: Option<MachineProvider>,
    guest: Option<MachineGuestConfig>,
    resources: Option<MachineResources>,
    volumes: Vec<MachineVolume>,
    roots: MachineRootsView,
    paths: MachinePaths,
    runtime: Option<MachineRuntimeState>,
    machine_image_contract: Option<MachineImageContractStatusView>,
    machine_api: MachineApiStatusView,
    last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct MachineImageContractStatusView {
    host_managed: bool,
    configured_image: String,
    desired_image: String,
    recorded_image: Option<String>,
    recorded_matches_desired: bool,
    materialized_image_path: PathBuf,
    materialized_image_exists: bool,
    efi_store_path: PathBuf,
    efi_store_exists: bool,
    rebuild_required: bool,
    rebuild_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct MachineApiStatusView {
    socket_path: PathBuf,
    guest_socket_path: Option<PathBuf>,
    transport: Option<String>,
    forward_user: Option<String>,
    identity_path: Option<PathBuf>,
    exists: bool,
    reachable: bool,
    role: Option<String>,
    protocol_version: Option<String>,
    listen_mode: Option<String>,
    capabilities: Option<MachineApiCapabilityResponse>,
    error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct MachineOsApplyStatusView {
    result: MachineOsCommandResult,
    name: String,
    previous_image: String,
    current_image: String,
    image_changed: bool,
    restart_requested: bool,
    restarted: bool,
    lifecycle: MachineLifecycle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct MachineOsUpgradeStatusView {
    result: MachineOsCommandResult,
    name: String,
    current_image: String,
    current_version: String,
    target_image: String,
    target_version: String,
    update_available: bool,
    dry_run: bool,
    restart_requested: bool,
    restarted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct MachinePaths {
    name: String,
    config_dir: PathBuf,
    state_dir: PathBuf,
    runtime_dir: PathBuf,
    config_path: PathBuf,
    generated_ignition_path: PathBuf,
    state_path: PathBuf,
    image_cache_dir: PathBuf,
    materialized_image_path: PathBuf,
    api_socket_path: PathBuf,
    ready_socket_path: PathBuf,
    ignition_socket_path: PathBuf,
    gvproxy_socket_path: PathBuf,
    krunkit_endpoint_path: PathBuf,
    efi_variable_store_path: PathBuf,
    gvproxy_pid_path: PathBuf,
    krunkit_pid_path: PathBuf,
    machine_log_path: PathBuf,
    gvproxy_log_path: PathBuf,
    krunkit_log_path: PathBuf,
}

impl MachinePaths {
    fn ensure_directories(&self) -> Result<(), Error> {
        fs::create_dir_all(&self.config_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine config directory {}: {error}",
                self.config_dir.display()
            ))
        })?;
        fs::create_dir_all(&self.state_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine state directory {}: {error}",
                self.state_dir.display()
            ))
        })?;
        fs::create_dir_all(&self.image_cache_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine image cache directory {}: {error}",
                self.image_cache_dir.display()
            ))
        })?;
        self.ensure_runtime_directories()
    }

    fn ensure_runtime_directories(&self) -> Result<(), Error> {
        fs::create_dir_all(&self.runtime_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine runtime directory {}: {error}",
                self.runtime_dir.display()
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener as StdUnixListener;

    use super::*;
    use crate::machine::manager::MachineHelperEnvGuard;
    use clap::Parser;
    use tempfile::TempDir;

    #[derive(Debug, Parser)]
    struct RootCli {
        #[command(subcommand)]
        command: Option<RootCommand>,
    }

    #[derive(Debug, Subcommand)]
    enum RootCommand {
        Machine(MachineCommand),
    }

    fn expected_default_machine_image() -> String {
        if cfg!(target_os = "macos") {
            format!(
                "docker://{DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY}@{DEFAULT_PODMAN_MACHINE_IMAGE_DIGEST}"
            )
        } else {
            format!(
                "docker://{DEFAULT_NEOVEX_MACHINE_IMAGE_REPOSITORY}:{}",
                current_machine_release_tag()
            )
        }
    }

    fn supported_stream_current_image_for_upgrade_test() -> String {
        if cfg!(target_os = "macos") {
            "docker://quay.io/podman/machine-os:5.9".to_owned()
        } else {
            "docker://ghcr.io/agentstation/neovex-machine-os:v0.1.0".to_owned()
        }
    }

    fn supported_stream_digest_image_for_upgrade_test() -> String {
        if cfg!(target_os = "macos") {
            format!("docker://{DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY}@sha256:abc123")
        } else {
            "docker://ghcr.io/agentstation/neovex-machine-os@sha256:abc123".to_owned()
        }
    }

    fn expected_upgrade_target_version() -> String {
        if cfg!(target_os = "macos") {
            DEFAULT_PODMAN_MACHINE_IMAGE_DIGEST.to_owned()
        } else {
            current_machine_release_tag()
        }
    }

    #[test]
    fn parses_machine_init_defaults_to_version_pinned_release_image() {
        let cli = RootCli::parse_from(["neovex", "machine", "init"]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine init should parse");
        };

        match machine.command {
            MachineSubcommand::Init(init) => {
                assert_eq!(init.image, expected_default_machine_image());
                if cfg!(target_os = "macos") {
                    assert!(init.volumes.is_empty());
                    assert_eq!(
                        default_machine_volumes(),
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
                    );
                }
            }
            _ => panic!("expected init subcommand"),
        }
    }

    #[test]
    fn parses_machine_init_with_resource_overrides() {
        let cli = RootCli::parse_from([
            "neovex",
            "machine",
            "init",
            "--cpus",
            "4",
            "--memory-mib",
            "4096",
            "--disk-gib",
            "40",
            "--image",
            "docker://ghcr.io/agentstation/neovex-machine-os:test",
            "--volume",
            "/Users:/Users",
        ]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine subcommand should parse");
        };

        match machine.command {
            MachineSubcommand::Init(init) => {
                assert_eq!(init.cpus, 4);
                assert_eq!(init.memory_mib, 4096);
                assert_eq!(init.disk_gib, 40);
                assert_eq!(
                    init.image,
                    "docker://ghcr.io/agentstation/neovex-machine-os:test"
                );
                assert_eq!(init.ssh_identity, None);
                assert_eq!(init.ignition_file, None);
                assert_eq!(init.efi_store, None);
                assert_eq!(
                    init.volumes,
                    vec![MachineVolume {
                        source: PathBuf::from("/Users"),
                        target: PathBuf::from("/Users"),
                    }]
                );
            }
            _ => panic!("expected init subcommand"),
        }
    }

    #[test]
    fn parses_machine_lifecycle_subcommands() {
        for command in ["start", "stop", "status", "rm"] {
            let cli = RootCli::parse_from(["neovex", "machine", command]);
            let Some(RootCommand::Machine(_)) = cli.command else {
                panic!("machine {command} should parse");
            };
        }
    }

    #[test]
    fn parses_machine_os_subcommands() {
        let cli = RootCli::parse_from([
            "neovex",
            "machine",
            "os",
            "apply",
            "ghcr.io/agentstation/neovex-machine-os:v9.9.9",
            "--restart",
        ]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine os apply should parse");
        };
        match machine.command {
            MachineSubcommand::Os(os) => match os.command {
                MachineOsSubcommand::Apply(apply) => {
                    assert_eq!(apply.image, "ghcr.io/agentstation/neovex-machine-os:v9.9.9");
                    assert!(apply.restart);
                }
                _ => panic!("expected machine os apply subcommand"),
            },
            _ => panic!("expected machine os subcommand"),
        }

        let cli = RootCli::parse_from([
            "neovex",
            "machine",
            "os",
            "upgrade",
            "--dry-run",
            "--restart",
        ]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine os upgrade should parse");
        };
        match machine.command {
            MachineSubcommand::Os(os) => match os.command {
                MachineOsSubcommand::Upgrade(upgrade) => {
                    assert!(upgrade.dry_run);
                    assert!(upgrade.restart);
                }
                _ => panic!("expected machine os upgrade subcommand"),
            },
            _ => panic!("expected machine os subcommand"),
        }
    }

    #[test]
    fn parses_machine_ssh_with_guest_command() {
        let cli = RootCli::parse_from(["neovex", "machine", "ssh", "uname", "-a"]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine ssh should parse");
        };

        match machine.command {
            MachineSubcommand::Ssh(ssh) => {
                assert_eq!(ssh.args, vec!["uname", "-a"]);
            }
            _ => panic!("expected ssh subcommand"),
        }
    }

    #[test]
    fn parses_hidden_machine_api_subcommand() {
        let cli = RootCli::parse_from([
            "neovex",
            "machine",
            "api",
            "--socket-path",
            "/tmp/neovex.sock",
            "--control-data-dir",
            "/tmp/neovex-control",
        ]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine api should parse");
        };

        match machine.command {
            MachineSubcommand::Api(api) => {
                assert_eq!(api.socket_path, Some(PathBuf::from("/tmp/neovex.sock")));
                assert_eq!(
                    api.control_data_dir,
                    Some(PathBuf::from("/tmp/neovex-control"))
                );
                assert!(!api.socket_activation);
            }
            _ => panic!("expected api subcommand"),
        }
    }

    #[test]
    fn hidden_machine_api_subcommand_falls_back_without_home() {
        let original_home = std::env::var_os("HOME");
        // SAFETY: this test runs in the serialized machine lane and restores HOME before returning.
        unsafe { std::env::remove_var("HOME") };

        let roots = resolve_roots_for_command(&MachineCommand {
            command: MachineSubcommand::Api(MachineApiCommand {
                socket_path: Some(PathBuf::from("/tmp/neovex.sock")),
                socket_activation: false,
                control_data_dir: Some(PathBuf::from("/tmp/neovex-control")),
            }),
        })
        .expect("hidden machine api should fall back without HOME");

        if let Some(home) = original_home {
            // SAFETY: see comment above; restore process-local HOME for later tests.
            unsafe { std::env::set_var("HOME", home) };
        }

        assert_eq!(
            roots.config_root,
            PathBuf::from("/var/lib/neovex/machine/config")
        );
        assert_eq!(
            roots.state_root,
            PathBuf::from("/var/lib/neovex/machine/state")
        );
    }

    #[test]
    fn machine_paths_use_short_runtime_root_and_typed_socket_layout() {
        let layout = MachineRootLayout::new(
            PathBuf::from("/tmp/config-root"),
            PathBuf::from("/tmp/state-root"),
            PathBuf::from("/tmp/neovex"),
        );
        let paths = layout.paths("default");

        assert_eq!(paths.runtime_dir, PathBuf::from("/tmp/neovex"));
        assert_eq!(
            paths.materialized_image_path,
            PathBuf::from("/tmp/state-root/default/images/default.raw")
        );
        assert_eq!(
            paths.api_socket_path,
            PathBuf::from("/tmp/neovex/default-api.sock")
        );
        assert_eq!(
            paths.krunkit_log_path,
            PathBuf::from("/tmp/neovex/default-krunkit.log")
        );
        assert_eq!(
            layout.lock_path("default"),
            PathBuf::from("/tmp/state-root/default.lock")
        );
    }

    #[test]
    fn machine_volume_requires_absolute_host_and_guest_paths() {
        let error = MachineVolume::parse("Users:/Users").expect_err("relative source should fail");
        assert!(error.contains("source path must be absolute"));

        let error = MachineVolume::parse("/Users:Users").expect_err("relative target should fail");
        assert!(error.contains("target path must be absolute"));
    }

    #[test]
    fn machine_init_writes_config_and_status_files() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );

        run_machine_command_for_test(
            MachineCommand {
                command: MachineSubcommand::Init(MachineInitCommand {
                    cpus: DEFAULT_MACHINE_CPUS,
                    memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
                    disk_gib: DEFAULT_MACHINE_DISK_GIB,
                    image: default_machine_image().to_owned(),
                    ssh_identity: None,
                    ignition_file: None,
                    efi_store: None,
                    volumes: vec![MachineVolume {
                        source: PathBuf::from("/Users"),
                        target: PathBuf::from("/Users"),
                    }],
                }),
            },
            &layout,
        )
        .expect("machine init should succeed");

        let paths = layout.paths(DEFAULT_MACHINE_NAME);
        let config = read_json_file_if_exists::<MachineConfigRecord>(&paths.config_path)
            .expect("config should read")
            .expect("config should exist");
        let state = read_json_file_if_exists::<MachineStateRecord>(&paths.state_path)
            .expect("state should read")
            .expect("state should exist");

        assert_eq!(config.version, CURRENT_MACHINE_CONFIG_VERSION);
        assert_eq!(config.provider, MachineProvider::Krunkit);
        assert_eq!(config.resources.cpus, DEFAULT_MACHINE_CPUS);
        assert_eq!(
            config.guest.image_source,
            MachineImageSource::OciReference {
                reference: default_machine_image().to_owned(),
            }
        );
        assert_eq!(config.guest.ssh_user, DEFAULT_MACHINE_SSH_USER);
        assert_eq!(config.guest.ssh_identity_path, None);
        assert_eq!(config.guest.ignition_file_path, None);
        assert_eq!(config.guest.efi_variable_store_path, None);
        assert_eq!(state.version, CURRENT_MACHINE_STATE_VERSION);
        assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
        assert!(paths.runtime_dir.exists());
    }

    #[test]
    fn write_json_file_atomically_replaces_existing_state_record() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let path = temp_dir.path().join("status.json");
        let first = MachineStateRecord::initialized();
        let second = MachineStateRecord::rebuilt("rewritten for atomic replace test");

        write_json_file(&path, &first).expect("first state write should succeed");
        write_json_file(&path, &second).expect("second state write should succeed");

        let stored = read_json_file_if_exists::<MachineStateRecord>(&path)
            .expect("stored state should read")
            .expect("stored state should exist");

        assert_eq!(stored, second);
        assert_eq!(stored.version, CURRENT_MACHINE_STATE_VERSION);
        assert_eq!(stored.manager, MachineManagerState::Stale);
    }

    #[test]
    fn machine_remove_releases_reserved_machine_port() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths(DEFAULT_MACHINE_NAME);
        run_machine_command_for_test(
            MachineCommand {
                command: MachineSubcommand::Init(MachineInitCommand {
                    cpus: DEFAULT_MACHINE_CPUS,
                    memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
                    disk_gib: DEFAULT_MACHINE_DISK_GIB,
                    image: default_machine_image().to_owned(),
                    ssh_identity: None,
                    ignition_file: None,
                    efi_store: None,
                    volumes: Vec::new(),
                }),
            },
            &layout,
        )
        .expect("machine init should succeed");
        fs::write(
            layout.port_allocation_state_path(),
            serde_json::to_vec_pretty(&serde_json::json!({
                "machine_ports": {
                    DEFAULT_MACHINE_NAME: 20022
                }
            }))
            .expect("port allocation state should serialize"),
        )
        .expect("port allocation state should write");

        run_machine_command_for_test(
            MachineCommand {
                command: MachineSubcommand::Rm(MachineRmCommand {}),
            },
            &layout,
        )
        .expect("machine rm should succeed");

        let allocation_state = fs::read(layout.port_allocation_state_path())
            .expect("port allocation state should still read after release");
        let json: serde_json::Value =
            serde_json::from_slice(&allocation_state).expect("port allocation state should parse");
        assert_eq!(
            json["machine_ports"]
                .as_object()
                .expect("machine ports should be an object")
                .len(),
            0
        );
        assert!(!paths.config_dir.exists());
        assert!(!paths.state_dir.exists());
    }

    #[test]
    fn machine_os_apply_updates_config_and_invalidates_materialized_artifacts() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        run_machine_command_for_test(
            MachineCommand {
                command: MachineSubcommand::Init(MachineInitCommand {
                    cpus: DEFAULT_MACHINE_CPUS,
                    memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
                    disk_gib: DEFAULT_MACHINE_DISK_GIB,
                    image: "docker://ghcr.io/agentstation/neovex-machine-os:v0.1.0".to_owned(),
                    ssh_identity: None,
                    ignition_file: None,
                    efi_store: None,
                    volumes: Vec::new(),
                }),
            },
            &layout,
        )
        .expect("machine init should succeed");

        let paths = layout.paths(DEFAULT_MACHINE_NAME);
        fs::write(&paths.materialized_image_path, b"old-image").expect("image path should write");
        fs::write(&paths.efi_variable_store_path, b"old-efi").expect("efi store should write");

        run_machine_command_for_test(
            MachineCommand {
                command: MachineSubcommand::Os(MachineOsCommand {
                    command: MachineOsSubcommand::Apply(MachineOsApplyCommand {
                        image: format!(
                            "docker://{DEFAULT_NEOVEX_MACHINE_IMAGE_REPOSITORY}:{}",
                            current_machine_release_tag()
                        ),
                        restart: false,
                    }),
                }),
            },
            &layout,
        )
        .expect("machine os apply should succeed");

        let config = read_json_file_if_exists::<MachineConfigRecord>(&paths.config_path)
            .expect("config should read")
            .expect("config should exist");
        let state = read_json_file_if_exists::<MachineStateRecord>(&paths.state_path)
            .expect("state should read")
            .expect("state should exist");

        assert_eq!(
            config.guest.image_source,
            MachineImageSource::OciReference {
                reference: format!(
                    "docker://{DEFAULT_NEOVEX_MACHINE_IMAGE_REPOSITORY}:{}",
                    current_machine_release_tag()
                ),
            }
        );
        assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
        assert_eq!(state.manager, MachineManagerState::Unconfigured);
        assert!(!paths.materialized_image_path.exists());
        assert!(!paths.efi_variable_store_path.exists());
    }

    #[test]
    fn machine_os_upgrade_plan_uses_supported_stream_target() {
        let config = MachineConfigRecord {
            version: CURRENT_MACHINE_CONFIG_VERSION,
            name: DEFAULT_MACHINE_NAME.to_owned(),
            provider: MachineProvider::Krunkit,
            guest: MachineGuestConfig {
                image_source: MachineImageSource::OciReference {
                    reference: supported_stream_current_image_for_upgrade_test(),
                },
                ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
                ssh_identity_path: None,
                ignition_file_path: None,
                efi_variable_store_path: None,
            },
            resources: MachineResources {
                cpus: DEFAULT_MACHINE_CPUS,
                memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
                disk_gib: DEFAULT_MACHINE_DISK_GIB,
            },
            volumes: Vec::new(),
            roots: MachineRootLayout::new(
                PathBuf::from("/tmp/config"),
                PathBuf::from("/tmp/state"),
                PathBuf::from("/tmp/runtime"),
            ),
        };

        let plan = plan_machine_os_upgrade(&config).expect("upgrade plan should resolve");

        assert_eq!(
            plan.current_image,
            supported_stream_current_image_for_upgrade_test()
        );
        assert_eq!(
            plan.current_version,
            if cfg!(target_os = "macos") {
                "5.9"
            } else {
                "v0.1.0"
            }
        );
        assert_eq!(plan.target_image, default_machine_image());
        assert_eq!(plan.target_version, expected_upgrade_target_version());
        assert!(plan.update_available);
    }

    #[test]
    fn host_managed_macos_stream_converges_to_pinned_podman_digest() {
        let config = MachineConfigRecord {
            version: CURRENT_MACHINE_CONFIG_VERSION,
            name: DEFAULT_MACHINE_NAME.to_owned(),
            provider: MachineProvider::Krunkit,
            guest: MachineGuestConfig {
                image_source: MachineImageSource::OciReference {
                    reference: "docker://ghcr.io/agentstation/neovex-machine-os:v0.1.0".to_owned(),
                },
                ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
                ssh_identity_path: None,
                ignition_file_path: None,
                efi_variable_store_path: None,
            },
            resources: MachineResources {
                cpus: DEFAULT_MACHINE_CPUS,
                memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
                disk_gib: DEFAULT_MACHINE_DISK_GIB,
            },
            volumes: Vec::new(),
            roots: MachineRootLayout::new(
                PathBuf::from("/tmp/config"),
                PathBuf::from("/tmp/state"),
                PathBuf::from("/tmp/runtime"),
            ),
        };

        let desired = desired_machine_image_source(&config);

        assert_eq!(
            desired,
            MachineImageSource::OciReference {
                reference: default_machine_image(),
            }
        );
    }

    #[test]
    fn explicit_podman_override_does_not_get_rewritten_to_default_digest() {
        let config = MachineConfigRecord {
            version: CURRENT_MACHINE_CONFIG_VERSION,
            name: DEFAULT_MACHINE_NAME.to_owned(),
            provider: MachineProvider::Krunkit,
            guest: MachineGuestConfig {
                image_source: MachineImageSource::OciReference {
                    reference: "docker://quay.io/podman/machine-os@sha256:customoverride"
                        .to_owned(),
                },
                ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
                ssh_identity_path: None,
                ignition_file_path: None,
                efi_variable_store_path: None,
            },
            resources: MachineResources {
                cpus: DEFAULT_MACHINE_CPUS,
                memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
                disk_gib: DEFAULT_MACHINE_DISK_GIB,
            },
            volumes: Vec::new(),
            roots: MachineRootLayout::new(
                PathBuf::from("/tmp/config"),
                PathBuf::from("/tmp/state"),
                PathBuf::from("/tmp/runtime"),
            ),
        };

        assert_eq!(
            desired_machine_image_source(&config),
            config.guest.image_source
        );
    }

    #[test]
    fn machine_os_upgrade_handles_digest_pinned_supported_streams() {
        let config = MachineConfigRecord {
            version: CURRENT_MACHINE_CONFIG_VERSION,
            name: DEFAULT_MACHINE_NAME.to_owned(),
            provider: MachineProvider::Krunkit,
            guest: MachineGuestConfig {
                image_source: MachineImageSource::OciReference {
                    reference: supported_stream_digest_image_for_upgrade_test(),
                },
                ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
                ssh_identity_path: None,
                ignition_file_path: None,
                efi_variable_store_path: None,
            },
            resources: MachineResources {
                cpus: DEFAULT_MACHINE_CPUS,
                memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
                disk_gib: DEFAULT_MACHINE_DISK_GIB,
            },
            volumes: Vec::new(),
            roots: MachineRootLayout::new(
                PathBuf::from("/tmp/config"),
                PathBuf::from("/tmp/state"),
                PathBuf::from("/tmp/runtime"),
            ),
        };

        if cfg!(target_os = "macos") {
            let plan =
                plan_machine_os_upgrade(&config).expect("macOS digest streams should resolve");
            assert_eq!(
                plan.current_image,
                supported_stream_digest_image_for_upgrade_test()
            );
            assert_eq!(plan.current_version, "sha256:abc123");
            assert_eq!(plan.target_image, default_machine_image());
            assert_eq!(plan.target_version, expected_upgrade_target_version());
            assert!(plan.update_available);
        } else {
            let error =
                plan_machine_os_upgrade(&config).expect_err("linux digest streams should fail");
            assert!(error.to_string().contains("digest-pinned"));
        }
    }

    #[test]
    fn load_machine_config_upgrades_versionless_record_and_rewrites_it() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths(DEFAULT_MACHINE_NAME);
        fs::create_dir_all(&paths.config_dir).expect("config dir should exist");
        let legacy_config = serde_json::json!({
            "name": DEFAULT_MACHINE_NAME,
            "provider": "krunkit",
            "guest": {
                "image_source": {
                    "kind": "oci-reference",
                    "reference": default_machine_image(),
                },
                "ssh_user": DEFAULT_MACHINE_SSH_USER,
                "ssh_identity_path": null,
                "ignition_file_path": null,
                "efi_variable_store_path": null,
            },
            "resources": {
                "cpus": DEFAULT_MACHINE_CPUS,
                "memory_mib": DEFAULT_MACHINE_MEMORY_MIB,
                "disk_gib": DEFAULT_MACHINE_DISK_GIB,
            },
            "volumes": [],
            "roots": {
                "config_root": layout.config_root,
                "state_root": layout.state_root,
                "runtime_root": layout.runtime_root,
            },
        });
        fs::write(
            &paths.config_path,
            serde_json::to_vec_pretty(&legacy_config).expect("legacy config should serialize"),
        )
        .expect("legacy config should write");

        let config = load_machine_config_if_exists(&paths.config_path)
            .expect("config load should succeed")
            .expect("config should exist");
        let rewritten = read_json_file_if_exists::<MachineConfigRecord>(&paths.config_path)
            .expect("rewritten config should read")
            .expect("rewritten config should exist");

        assert_eq!(config.version, CURRENT_MACHINE_CONFIG_VERSION);
        assert_eq!(rewritten.version, CURRENT_MACHINE_CONFIG_VERSION);
        assert_eq!(config.guest.ssh_user, DEFAULT_MACHINE_SSH_USER);
        assert_eq!(rewritten.resources.memory_mib, DEFAULT_MACHINE_MEMORY_MIB);
    }

    #[test]
    fn load_machine_config_rejects_newer_schema_version_with_clear_error() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths(DEFAULT_MACHINE_NAME);
        fs::create_dir_all(&paths.config_dir).expect("config dir should exist");
        let newer_config = serde_json::json!({
            "version": CURRENT_MACHINE_CONFIG_VERSION + 1,
            "name": DEFAULT_MACHINE_NAME,
            "provider": "krunkit",
            "guest": {
                "image_source": {
                    "kind": "oci-reference",
                    "reference": default_machine_image(),
                },
                "ssh_user": DEFAULT_MACHINE_SSH_USER,
                "ssh_identity_path": null,
                "ignition_file_path": null,
                "efi_variable_store_path": null,
            },
            "resources": {
                "cpus": DEFAULT_MACHINE_CPUS,
                "memory_mib": DEFAULT_MACHINE_MEMORY_MIB,
                "disk_gib": DEFAULT_MACHINE_DISK_GIB,
            },
            "volumes": [],
            "roots": {
                "config_root": layout.config_root,
                "state_root": layout.state_root,
                "runtime_root": layout.runtime_root,
            },
        });
        fs::write(
            &paths.config_path,
            serde_json::to_vec_pretty(&newer_config).expect("newer config should serialize"),
        )
        .expect("newer config should write");

        let error = load_machine_config_if_exists(&paths.config_path)
            .expect_err("newer config version should fail");

        assert!(error.to_string().contains("uses newer schema version"));
        assert!(
            error
                .to_string()
                .contains(&(CURRENT_MACHINE_CONFIG_VERSION + 1).to_string())
        );
    }

    #[test]
    fn load_machine_state_upgrades_versionless_record_and_rewrites_it() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths(DEFAULT_MACHINE_NAME);
        fs::create_dir_all(&paths.state_dir).expect("state dir should exist");
        let legacy_state = serde_json::json!({
            "lifecycle": "running",
            "manager": "ready",
            "runtime": null,
            "last_error": null,
        });
        fs::write(
            &paths.state_path,
            serde_json::to_vec_pretty(&legacy_state).expect("legacy state should serialize"),
        )
        .expect("legacy state should write");

        let state = load_machine_state_if_exists(&paths.state_path)
            .expect("state load should succeed")
            .expect("state should exist");
        let rewritten = read_json_file_if_exists::<MachineStateRecord>(&paths.state_path)
            .expect("rewritten state should read")
            .expect("rewritten state should exist");

        assert_eq!(state.version, CURRENT_MACHINE_STATE_VERSION);
        assert_eq!(state.lifecycle, MachineLifecycle::Running);
        assert_eq!(rewritten.version, CURRENT_MACHINE_STATE_VERSION);
        assert_eq!(rewritten.manager, MachineManagerState::Ready);
    }

    #[test]
    fn load_machine_state_rebuilds_unreadable_record_with_explicit_error() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths(DEFAULT_MACHINE_NAME);
        fs::create_dir_all(&paths.state_dir).expect("state dir should exist");
        fs::write(&paths.state_path, b"{not-json").expect("corrupt state should write");

        let state = load_machine_state_if_exists(&paths.state_path)
            .expect("state load should succeed by rebuilding")
            .expect("rebuilt state should exist");
        let rewritten = read_json_file_if_exists::<MachineStateRecord>(&paths.state_path)
            .expect("rebuilt state should read")
            .expect("rebuilt state should exist");

        assert_eq!(state.version, CURRENT_MACHINE_STATE_VERSION);
        assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
        assert_eq!(state.manager, MachineManagerState::Stale);
        assert!(state.runtime.is_none());
        assert!(
            state
                .last_error
                .as_deref()
                .is_some_and(|message| message.contains("machine state"))
        );
        assert_eq!(rewritten, state);
    }

    #[test]
    fn machine_status_renders_uninitialized_view_when_machine_is_absent() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths(DEFAULT_MACHINE_NAME);

        let rendered = render_machine_view(MachineCommandResult::Uninitialized, &paths, None, None)
            .expect("uninitialized machine view should render");

        assert!(rendered.contains("result: uninitialized"));
        assert!(rendered.contains("initialized: false"));
        assert!(rendered.contains("lifecycle: uninitialized"));
        assert!(rendered.contains("reachable: false"));
    }

    #[test]
    fn machine_status_marks_missing_machine_api_socket_as_unreachable() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths(DEFAULT_MACHINE_NAME);

        let api = machine_api_status_view(&paths, None);

        assert_eq!(api.socket_path, paths.api_socket_path);
        assert_eq!(api.guest_socket_path, None);
        assert_eq!(api.transport, None);
        assert_eq!(api.forward_user, None);
        assert_eq!(api.identity_path, None);
        assert!(!api.exists);
        assert!(!api.reachable);
        assert!(api.capabilities.is_none());
        assert!(api.error.is_none());
    }

    #[test]
    fn machine_status_detects_reachable_machine_api_socket() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths(DEFAULT_MACHINE_NAME);

        std::fs::create_dir_all(
            paths
                .api_socket_path
                .parent()
                .expect("machine api socket should have a parent"),
        )
        .expect("socket parent should exist");
        let listener =
            StdUnixListener::bind(&paths.api_socket_path).expect("listener should bind cleanly");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("server should accept");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let body = serde_json::json!({
                "status": "ok",
                "role": "guest-machine-api",
                "protocol_version": "v1alpha2",
                "listen_mode": "direct-socket",
                "control_data_dir": temp_dir.path().join("control").display().to_string(),
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("server should write response");

            let (mut stream, _) = listener
                .accept()
                .expect("server should accept capabilities");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let body = serde_json::json!({
                "protocol_version": "v1alpha2",
                "service_execution_ready": false,
                "service_execution_mode": "standard_containers",
                "supported_service_backends": ["container"],
                "supported_operations": ["healthz", "capabilities"],
                "binary_statuses": [
                    {
                        "name": "buildah",
                        "present": true,
                        "resolved_path": "/usr/bin/buildah",
                        "required_for_operations": ["service-sandboxes.build-start"]
                    }
                ],
                "operation_statuses": [
                    {
                        "name": "service-sandboxes.build-start",
                        "available": false,
                        "blockers": ["guest machine API does not yet expose service lifecycle operations"]
                    }
                ],
                "service_execution_blockers": [
                    "guest machine API does not yet expose service lifecycle operations"
                ]
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("server should write capabilities response");
        });

        std::thread::sleep(std::time::Duration::from_millis(100));
        let api = machine_api_status_view(&paths, None);
        server
            .join()
            .expect("machine API server thread should join cleanly");

        assert_eq!(api.socket_path, paths.api_socket_path);
        assert_eq!(api.guest_socket_path, None);
        assert_eq!(api.transport, None);
        assert_eq!(api.forward_user, None);
        assert_eq!(api.identity_path, None);
        assert!(api.exists);
        assert!(api.reachable);
        assert_eq!(api.role.as_deref(), Some("guest-machine-api"));
        assert_eq!(api.protocol_version.as_deref(), Some("v1alpha2"));
        assert_eq!(api.listen_mode.as_deref(), Some("direct-socket"));
        assert_eq!(
            api.capabilities
                .as_ref()
                .map(|capabilities| capabilities.service_execution_mode),
            Some(protocol::MachineApiServiceExecutionMode::StandardContainers)
        );
        assert_eq!(
            api.capabilities
                .as_ref()
                .map(|capabilities| capabilities.supported_service_backends.clone()),
            Some(vec![neovex::SandboxBackendKind::Container])
        );
        assert!(api.error.is_none());
    }

    #[test]
    fn machine_status_reports_forwarding_contract_when_machine_identity_exists() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            PathBuf::from("/tmp/neovex"),
        );
        let paths = layout.paths(DEFAULT_MACHINE_NAME);
        let config = MachineConfigRecord {
            version: CURRENT_MACHINE_CONFIG_VERSION,
            name: DEFAULT_MACHINE_NAME.to_owned(),
            provider: MachineProvider::Krunkit,
            guest: MachineGuestConfig {
                image_source: MachineImageSource::parse(&default_machine_image())
                    .expect("default image should parse"),
                ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
                ssh_identity_path: Some(PathBuf::from("/tmp/neovex-test-ed25519")),
                ignition_file_path: None,
                efi_variable_store_path: None,
            },
            resources: MachineResources {
                cpus: DEFAULT_MACHINE_CPUS,
                memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
                disk_gib: DEFAULT_MACHINE_DISK_GIB,
            },
            volumes: Vec::new(),
            roots: layout,
        };

        let api = machine_api_status_view(&paths, Some(&config));

        assert_eq!(api.socket_path, paths.api_socket_path);
        assert_eq!(
            api.guest_socket_path,
            Some(PathBuf::from("/run/neovex/neovex.sock"))
        );
        assert_eq!(
            api.transport.as_deref(),
            Some("gvproxy-ssh-forwarded-unix-socket")
        );
        assert_eq!(api.forward_user.as_deref(), Some("root"));
        assert_eq!(
            api.identity_path,
            Some(PathBuf::from("/tmp/neovex-test-ed25519"))
        );
        assert!(!api.exists);
        assert!(!api.reachable);
        assert!(api.capabilities.is_none());
        assert!(api.error.is_none());
    }

    #[test]
    fn machine_remove_deletes_config_state_and_runtime_roots() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );

        run_machine_command_for_test(
            MachineCommand {
                command: MachineSubcommand::Init(MachineInitCommand {
                    cpus: DEFAULT_MACHINE_CPUS,
                    memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
                    disk_gib: DEFAULT_MACHINE_DISK_GIB,
                    image: default_machine_image().to_owned(),
                    ssh_identity: None,
                    ignition_file: None,
                    efi_store: None,
                    volumes: Vec::new(),
                }),
            },
            &layout,
        )
        .expect("machine init should succeed");
        run_machine_command_for_test(
            MachineCommand {
                command: MachineSubcommand::Rm(MachineRmCommand {}),
            },
            &layout,
        )
        .expect("machine rm should succeed");

        let paths = layout.paths(DEFAULT_MACHINE_NAME);
        assert!(!paths.config_dir.exists());
        assert!(!paths.state_dir.exists());
        assert!(!paths.runtime_dir.exists());
    }

    #[test]
    fn machine_start_reports_oci_materialization_failure_for_unreachable_registry_image() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );

        run_machine_command_for_test(
            MachineCommand {
                command: MachineSubcommand::Init(MachineInitCommand {
                    cpus: DEFAULT_MACHINE_CPUS,
                    memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
                    disk_gib: DEFAULT_MACHINE_DISK_GIB,
                    image: "docker://127.0.0.1:1/example/neovex-machine-os:test".to_owned(),
                    ssh_identity: None,
                    ignition_file: None,
                    efi_store: None,
                    volumes: Vec::new(),
                }),
            },
            &layout,
        )
        .expect("machine init should succeed");

        let error = run_machine_command_for_test(
            MachineCommand {
                command: MachineSubcommand::Start(MachineStartCommand {}),
            },
            &layout,
        )
        .expect_err("machine start should surface OCI pull failure");

        let error_message = error.to_string();
        assert!(
            error_message.contains("failed to resolve machine guest OCI reference"),
            "expected OCI resolution error, got: {error_message}"
        );
    }

    fn run_machine_command_for_test(
        command: MachineCommand,
        layout: &MachineRootLayout,
    ) -> Result<(), Error> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build")
            .block_on(run_machine_command_with_layout(command, layout))
    }

    #[test]
    fn machine_image_source_parse_supports_published_local_and_url_sources() {
        assert_eq!(
            MachineImageSource::parse("ghcr.io/agentstation/neovex-machine-os:test")
                .expect("bare registry ref should parse"),
            MachineImageSource::OciReference {
                reference: "docker://ghcr.io/agentstation/neovex-machine-os:test".to_owned(),
            }
        );
        assert_eq!(
            MachineImageSource::parse("https://example.com/neovex-machine.raw.zst")
                .expect("url should parse"),
            MachineImageSource::HttpUrl {
                url: "https://example.com/neovex-machine.raw.zst".to_owned(),
            }
        );
        assert_eq!(
            MachineImageSource::parse("/tmp/neovex-machine.raw").expect("path should parse"),
            MachineImageSource::LocalDisk {
                path: PathBuf::from("/tmp/neovex-machine.raw"),
            }
        );
    }
}
