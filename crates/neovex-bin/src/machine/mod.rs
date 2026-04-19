use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use clap::{Args, Subcommand, ValueEnum};
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
    GuestNeovexBinarySourceKind, MACHINE_API_FORWARD_TRANSPORT, MACHINE_API_FORWARD_USER,
    MachineRuntimeState, build_scp_command, build_ssh_command, inspect_desired_guest_neovex_binary,
    inspect_observed_guest_neovex_binary, refresh_machine_state, release_machine_ssh_port,
    start_machine, stop_machine,
};
use self::protocol::MachineApiCapabilityResponse;
pub(crate) use self::protocol::MachineApiServiceSandboxDetails;

const DEFAULT_MACHINE_NAME: &str = "default";
const DEFAULT_NEOVEX_MACHINE_IMAGE_REPOSITORY: &str = "ghcr.io/agentstation/neovex-machine-os";
const DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY: &str = "quay.io/podman/machine-os";
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
const DEFAULT_MACHINE_SSH_USER: &str = "core";
const DEFAULT_MACHINE_RUNTIME_ROOT: &str = "/tmp/neovex";
const MACHINE_RUNTIME_ROOT_ENV: &str = "NEOVEX_MACHINE_RUNTIME_ROOT";
const DEFAULT_MACHINE_CPUS: u8 = 2;
const DEFAULT_MACHINE_MEMORY_MIB: u32 = 2048;
const DEFAULT_MACHINE_DISK_GIB: u32 = 20;
const CURRENT_MACHINE_CONFIG_VERSION: u32 = 2;
const CURRENT_MACHINE_STATE_VERSION: u32 = 1;

#[derive(Debug, Args)]
pub(crate) struct MachineCommand {
    #[command(subcommand)]
    command: MachineSubcommand,
}

#[derive(Debug, Subcommand)]
enum MachineSubcommand {
    /// Initialize a new machine.
    Init(MachineInitCommand),
    /// Start a machine, creating it if needed.
    Start(MachineStartCommand),
    /// Stop a running machine.
    Stop(MachineStopCommand),
    /// Display machine status.
    Status(MachineStatusCommand),
    /// List initialized machines.
    #[command(visible_alias = "ls")]
    List(MachineListCommand),
    /// Inspect a machine record.
    Inspect(MachineInspectCommand),
    /// Update a stopped machine.
    Set(MachineSetCommand),
    /// Securely copy files between the host and a machine.
    Cp(MachineCpCommand),
    /// Log in to a machine using SSH.
    Ssh(MachineSshCommand),
    /// Remove an existing machine.
    Rm(MachineRmCommand),
    /// Manage machine OS images.
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
    /// Use a specific machine OS image on the next boot.
    Apply(MachineOsApplyCommand),
    /// Switch to the supported machine OS image for this neovex release.
    Upgrade(MachineOsUpgradeCommand),
}

#[derive(Debug, Args)]
struct MachineOsApplyCommand {
    /// OCI image reference or digest to use on the next boot.
    image: String,

    /// Restart the machine immediately if it is running.
    #[arg(long)]
    restart: bool,
}

#[derive(Debug, Args)]
struct MachineOsUpgradeCommand {
    /// Check whether an upgrade is available.
    #[arg(long)]
    dry_run: bool,

    /// Restart the machine immediately if an upgrade is applied.
    #[arg(long)]
    restart: bool,
}

#[derive(Debug, Args)]
struct MachineInitCommand {
    /// Number of CPUs.
    #[arg(short = 'c', long, value_name = "COUNT", default_value_t = DEFAULT_MACHINE_CPUS)]
    cpus: u8,

    /// Memory in MiB.
    #[arg(
        short = 'm',
        long = "memory",
        value_name = "MIB",
        default_value_t = DEFAULT_MACHINE_MEMORY_MIB
    )]
    memory_mib: u32,

    /// Disk size in GiB.
    #[arg(
        short = 'd',
        long = "disk-size",
        value_name = "GIB",
        default_value_t = DEFAULT_MACHINE_DISK_GIB
    )]
    disk_gib: u32,

    /// Machine OS image.
    #[arg(long, value_name = "SOURCE", default_value_t = default_machine_image())]
    image: String,

    /// Path to SSH identity for guest access.
    #[arg(long = "identity", value_name = "PATH")]
    ssh_identity: Option<PathBuf>,

    /// Path to Ignition config file.
    #[arg(long = "ignition-path", value_name = "PATH")]
    ignition_file: Option<PathBuf>,

    /// Path to EFI variable store.
    #[arg(long = "firmware", value_name = "PATH")]
    efi_store: Option<PathBuf>,

    /// Host:guest volume mount.
    #[arg(
        short = 'v',
        long = "volume",
        value_name = "HOST:GUEST",
        value_parser = parse_machine_volume
    )]
    volumes: Vec<MachineVolume>,

    /// Start the machine after initializing it.
    #[arg(long)]
    now: bool,

    /// Machine name.
    #[arg(value_name = "NAME")]
    name: Option<String>,
}

#[derive(Debug, Args, Clone, Default)]
struct MachineStartCommand {
    /// Number of CPUs to use if start creates the machine.
    #[arg(short = 'c', long, value_name = "COUNT")]
    cpus: Option<u8>,

    /// Memory in MiB to use if start creates the machine.
    #[arg(short = 'm', long = "memory", value_name = "MIB")]
    memory_mib: Option<u32>,

    /// Disk size in GiB to use if start creates the machine.
    #[arg(short = 'd', long = "disk-size", value_name = "GIB")]
    disk_gib: Option<u32>,

    /// Machine OS image to use if start creates the machine.
    #[arg(long, value_name = "SOURCE")]
    image: Option<String>,

    /// Path to SSH identity for guest access if start creates the machine.
    #[arg(long = "identity", value_name = "PATH")]
    ssh_identity: Option<PathBuf>,

    /// Path to Ignition config file if start creates the machine.
    #[arg(long = "ignition-path", value_name = "PATH")]
    ignition_file: Option<PathBuf>,

    /// Path to EFI variable store if start creates the machine.
    #[arg(long = "firmware", value_name = "PATH")]
    efi_store: Option<PathBuf>,

    /// Host:guest volume mount if start creates the machine.
    #[arg(
        short = 'v',
        long = "volume",
        value_name = "HOST:GUEST",
        value_parser = parse_machine_volume
    )]
    volumes: Vec<MachineVolume>,

    /// Machine name.
    #[arg(value_name = "NAME")]
    name: Option<String>,
}

impl MachineStartCommand {
    fn name(&self) -> &str {
        self.name.as_deref().unwrap_or(DEFAULT_MACHINE_NAME)
    }

    fn has_create_overrides(&self) -> bool {
        self.cpus.is_some()
            || self.memory_mib.is_some()
            || self.disk_gib.is_some()
            || self.image.is_some()
            || self.ssh_identity.is_some()
            || self.ignition_file.is_some()
            || self.efi_store.is_some()
            || !self.volumes.is_empty()
    }

    fn into_init_command(self) -> MachineInitCommand {
        MachineInitCommand {
            cpus: self.cpus.unwrap_or(DEFAULT_MACHINE_CPUS),
            memory_mib: self.memory_mib.unwrap_or(DEFAULT_MACHINE_MEMORY_MIB),
            disk_gib: self.disk_gib.unwrap_or(DEFAULT_MACHINE_DISK_GIB),
            image: self.image.unwrap_or_else(default_machine_image),
            ssh_identity: self.ssh_identity,
            ignition_file: self.ignition_file,
            efi_store: self.efi_store,
            volumes: self.volumes,
            now: false,
            name: self.name,
        }
    }
}

impl MachineInitCommand {
    fn name(&self) -> &str {
        self.name.as_deref().unwrap_or(DEFAULT_MACHINE_NAME)
    }
}

#[derive(Debug, Args, Default)]
struct MachineStopCommand {
    /// Machine name.
    #[arg(value_name = "NAME")]
    name: Option<String>,
}

impl MachineStopCommand {
    fn name(&self) -> &str {
        self.name.as_deref().unwrap_or(DEFAULT_MACHINE_NAME)
    }
}

#[derive(Debug, Args, Default)]
struct MachineStatusCommand {
    /// Output format.
    #[arg(long, value_enum, default_value_t = MachineStatusOutputFormat::Table)]
    format: MachineStatusOutputFormat,

    /// Print the machine name only.
    #[arg(short = 'q', long)]
    quiet: bool,

    /// Machine name.
    #[arg(value_name = "NAME")]
    name: Option<String>,
}

impl MachineStatusCommand {
    fn name(&self) -> &str {
        self.name.as_deref().unwrap_or(DEFAULT_MACHINE_NAME)
    }
}

#[derive(Debug, Args, Default)]
struct MachineListCommand {
    /// Output format.
    #[arg(long, value_enum, default_value_t = MachineListOutputFormat::Table)]
    format: MachineListOutputFormat,

    /// Print machine names only.
    #[arg(short = 'q', long)]
    quiet: bool,
}

#[derive(Debug, Args, Default)]
struct MachineInspectCommand {
    /// Output format.
    #[arg(long, value_enum, default_value_t = MachineInspectOutputFormat::Json)]
    format: MachineInspectOutputFormat,

    /// Machine name.
    #[arg(value_name = "NAME")]
    name: Option<String>,
}

impl MachineInspectCommand {
    fn name(&self) -> &str {
        self.name.as_deref().unwrap_or(DEFAULT_MACHINE_NAME)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
enum MachineStatusOutputFormat {
    Json,
    Yaml,
    #[default]
    Table,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
enum MachineListOutputFormat {
    Json,
    #[default]
    Table,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
enum MachineInspectOutputFormat {
    #[default]
    Json,
    Yaml,
}

#[derive(Debug, Args, Default)]
struct MachineSetCommand {
    /// Number of CPUs.
    #[arg(short = 'c', long, value_name = "COUNT")]
    cpus: Option<u8>,

    /// Memory in MiB.
    #[arg(short = 'm', long = "memory", value_name = "MIB")]
    memory_mib: Option<u32>,

    /// Disk size in GiB.
    #[arg(short = 'd', long = "disk-size", value_name = "GIB")]
    disk_gib: Option<u32>,

    /// Machine name.
    #[arg(value_name = "NAME")]
    name: Option<String>,
}

impl MachineSetCommand {
    fn name(&self) -> &str {
        self.name.as_deref().unwrap_or(DEFAULT_MACHINE_NAME)
    }

    fn has_changes(&self) -> bool {
        self.cpus.is_some() || self.memory_mib.is_some() || self.disk_gib.is_some()
    }
}

#[derive(Debug, Args)]
struct MachineCpCommand {
    /// Suppress copy status output.
    #[arg(short = 'q', long)]
    quiet: bool,

    /// Source path.
    #[arg(value_name = "SRC_PATH")]
    src_path: String,

    /// Destination path.
    #[arg(value_name = "DEST_PATH")]
    dest_path: String,
}

#[derive(Debug, Args)]
struct MachineSshCommand {
    /// Optional command to execute in the guest once SSH wiring is available.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

#[derive(Debug, Args, Default)]
struct MachineRmCommand {
    /// Machine name.
    #[arg(value_name = "NAME")]
    name: Option<String>,
}

impl MachineRmCommand {
    fn name(&self) -> &str {
        self.name.as_deref().unwrap_or(DEFAULT_MACHINE_NAME)
    }
}

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
        let (paths, _, state) = load_initialized_machine(&roots, DEFAULT_MACHINE_NAME)?;
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
        let (paths, mut config, mut state) =
            load_initialized_machine(&roots, DEFAULT_MACHINE_NAME)?;
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
            let machine_name = init.name().to_owned();
            with_machine_lock(roots, &machine_name, || run_machine_init(init, roots))
        }
        MachineSubcommand::Start(start) => {
            let machine_name = start.name().to_owned();
            with_machine_lock(roots, &machine_name, || run_machine_start(start, roots))
        }
        MachineSubcommand::Stop(stop) => {
            let machine_name = stop.name().to_owned();
            with_machine_lock(roots, &machine_name, || run_machine_stop(stop, roots))
        }
        MachineSubcommand::Status(status) => {
            let machine_name = status.name().to_owned();
            with_machine_lock(roots, &machine_name, || run_machine_status(status, roots))
        }
        MachineSubcommand::List(list) => run_machine_list(list, roots),
        MachineSubcommand::Inspect(inspect) => {
            let machine_name = inspect.name().to_owned();
            with_machine_lock(roots, &machine_name, || run_machine_inspect(inspect, roots))
        }
        MachineSubcommand::Set(set) => {
            let machine_name = set.name().to_owned();
            with_machine_lock(roots, &machine_name, || run_machine_set(set, roots))
        }
        MachineSubcommand::Cp(copy) => {
            let machine_name = resolve_machine_cp_target_name(&copy)?;
            with_machine_lock(roots, &machine_name, || run_machine_cp(copy, roots))
        }
        MachineSubcommand::Ssh(ssh) => {
            let machine_name = resolve_machine_ssh_target_name(&ssh, roots)?.to_owned();
            with_machine_lock(roots, &machine_name, || run_machine_ssh(ssh, roots))
        }
        MachineSubcommand::Rm(remove) => {
            let machine_name = remove.name().to_owned();
            with_machine_lock(roots, &machine_name, || run_machine_rm(remove, roots))
        }
        MachineSubcommand::Os(os) => with_default_machine_lock(roots, || run_machine_os(os, roots)),
        MachineSubcommand::Api(api) => api::run_machine_api_command(api, roots).await,
    }
}

fn machine_record_exists(roots: &MachineRootLayout, machine_name: &str) -> bool {
    roots.paths(machine_name).config_path.exists()
}

fn resolve_machine_ssh_target(
    command: &MachineSshCommand,
    roots: &MachineRootLayout,
) -> Result<(String, Vec<String>), Error> {
    let Some(first_arg) = command.args.first() else {
        return Ok((DEFAULT_MACHINE_NAME.to_owned(), Vec::new()));
    };

    if machine_record_exists(roots, first_arg) {
        return Ok((first_arg.clone(), command.args[1..].to_vec()));
    }

    Ok((DEFAULT_MACHINE_NAME.to_owned(), command.args.clone()))
}

fn resolve_machine_ssh_target_name<'a>(
    command: &'a MachineSshCommand,
    roots: &'a MachineRootLayout,
) -> Result<&'a str, Error> {
    if let Some(first_arg) = command.args.first()
        && machine_record_exists(roots, first_arg)
    {
        return Ok(first_arg.as_str());
    }

    Ok(DEFAULT_MACHINE_NAME)
}

fn resolve_machine_cp_target_name(command: &MachineCpCommand) -> Result<String, Error> {
    Ok(resolve_machine_cp_transfer(&command.src_path, &command.dest_path)?.machine_name)
}

fn run_machine_init(command: MachineInitCommand, roots: &MachineRootLayout) -> Result<(), Error> {
    let now = command.now;
    let (paths, mut config, mut state) = initialize_machine_record(command, roots)?;

    let result = if now {
        paths.ensure_runtime_directories()?;
        start_machine(&paths, &mut config, &mut state)?;
        MachineCommandResult::InitializedAndStarted
    } else {
        MachineCommandResult::Initialized
    };

    print!(
        "{}",
        render_machine_view(result, &paths, Some(&config), Some(&state))?
    );
    Ok(())
}

fn initialize_machine_record(
    command: MachineInitCommand,
    roots: &MachineRootLayout,
) -> Result<(MachinePaths, MachineConfigRecord, MachineStateRecord), Error> {
    let machine_name = command.name().to_owned();
    let paths = roots.paths(&machine_name);
    if paths.config_path.exists() {
        return Err(Error::AlreadyExists(format!(
            "machine '{}' is already initialized at {}",
            machine_name,
            paths.config_path.display()
        )));
    }

    paths.ensure_directories()?;
    let MachineInitCommand {
        cpus,
        memory_mib,
        disk_gib,
        image,
        ssh_identity,
        ignition_file,
        efi_store,
        volumes,
        now: _,
        name: _,
    } = command;
    let config = MachineConfigRecord {
        version: CURRENT_MACHINE_CONFIG_VERSION,
        name: machine_name,
        provider: MachineProvider::Krunkit,
        guest: MachineGuestConfig {
            image_source: MachineImageSource::parse(&image)?,
            ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
            ssh_identity_path: ssh_identity,
            ignition_file_path: ignition_file,
            efi_variable_store_path: efi_store,
        },
        resources: MachineResources {
            cpus,
            memory_mib,
            disk_gib,
        },
        volumes: if volumes.is_empty() {
            default_machine_volumes()
        } else {
            volumes
        },
        roots: roots.clone(),
    };
    let state = MachineStateRecord::initialized();
    write_json_file(&paths.config_path, &config)?;
    write_json_file(&paths.state_path, &state)?;
    Ok((paths, config, state))
}

fn run_machine_start(command: MachineStartCommand, roots: &MachineRootLayout) -> Result<(), Error> {
    let machine_name = command.name().to_owned();
    let paths = roots.paths(&machine_name);
    let (paths, mut config, mut state, created) = if paths.config_path.exists() {
        if command.has_create_overrides() {
            return Err(Error::AlreadyExists(format!(
                "machine '{}' is already initialized at {}; `neovex machine start` only uses init flags when creating a new machine",
                machine_name,
                paths.config_path.display()
            )));
        }
        let (paths, config, state) = load_initialized_machine(roots, &machine_name)?;
        (paths, config, state, false)
    } else {
        let (paths, config, state) = initialize_machine_record(command.into_init_command(), roots)?;
        (paths, config, state, true)
    };
    paths.ensure_runtime_directories()?;
    start_machine(&paths, &mut config, &mut state)?;
    let result = if created {
        MachineCommandResult::InitializedAndStarted
    } else {
        MachineCommandResult::Started
    };
    print!(
        "{}",
        render_machine_view(result, &paths, Some(&config), Some(&state))?
    );
    Ok(())
}

fn run_machine_stop(command: MachineStopCommand, roots: &MachineRootLayout) -> Result<(), Error> {
    let machine_name = command.name().to_owned();
    let (paths, config, mut state) = load_initialized_machine(roots, &machine_name)?;
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
    command: MachineStatusCommand,
    roots: &MachineRootLayout,
) -> Result<(), Error> {
    let paths = roots.paths(command.name());
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
        render_machine_status_view(
            result,
            &paths,
            config.as_ref(),
            state.as_ref(),
            command.format,
            command.quiet
        )?
    );
    Ok(())
}

fn run_machine_list(command: MachineListCommand, roots: &MachineRootLayout) -> Result<(), Error> {
    let machines = build_machine_list_entries(roots)?;
    print!(
        "{}",
        render_machine_list_view(&machines, command.format, command.quiet)?
    );
    Ok(())
}

fn run_machine_inspect(
    command: MachineInspectCommand,
    roots: &MachineRootLayout,
) -> Result<(), Error> {
    let machine_name = command.name().to_owned();
    let (_paths, config, state) = load_initialized_machine(roots, &machine_name)?;
    print!(
        "{}",
        render_machine_inspect_view(&config, &state, command.format)?
    );
    Ok(())
}

fn run_machine_cp(command: MachineCpCommand, roots: &MachineRootLayout) -> Result<(), Error> {
    let transfer = resolve_machine_cp_transfer(&command.src_path, &command.dest_path)?;
    let (_paths, config, state) = load_initialized_machine(roots, &transfer.machine_name)?;

    let mut scp = build_scp_command(
        &config,
        &state,
        transfer.guest_is_src,
        &transfer.machine_path,
        &transfer.host_path,
    )?;
    if !command.quiet {
        scp.stdout(Stdio::inherit());
    }
    scp.stderr(Stdio::inherit());

    let status = scp
        .status()
        .map_err(|error| Error::Internal(format!("failed to start scp: {error}")))?;
    if !status.success() {
        return Err(Error::Internal(format!(
            "scp exited unsuccessfully with status {status}"
        )));
    }

    if !command.quiet {
        println!("Copy successful");
    }
    Ok(())
}

fn run_machine_ssh(command: MachineSshCommand, roots: &MachineRootLayout) -> Result<(), Error> {
    let (machine_name, ssh_args) = resolve_machine_ssh_target(&command, roots)?;
    let (paths, config, mut state) = load_initialized_machine(roots, &machine_name)?;
    refresh_machine_state(&paths, &mut state)?;
    write_json_file(&paths.state_path, &state)?;

    let mut ssh = build_ssh_command(&config, &state)?;
    ssh.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    ssh.args(ssh_args);

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

fn run_machine_set(command: MachineSetCommand, roots: &MachineRootLayout) -> Result<(), Error> {
    if !command.has_changes() {
        return Err(Error::InvalidInput(
            "machine set requires at least one of `--cpus`, `--memory`, or `--disk-size`"
                .to_owned(),
        ));
    }

    let machine_name = command.name().to_owned();
    let (paths, mut config, state) = load_initialized_machine(roots, &machine_name)?;
    if state.lifecycle != MachineLifecycle::Stopped {
        return Err(Error::Conflict(format!(
            "machine '{}' is {} and must be stopped before applying `neovex machine set`",
            machine_name,
            state.lifecycle.as_str()
        )));
    }

    if let Some(cpus) = command.cpus {
        config.resources.cpus = cpus;
    }
    if let Some(memory_mib) = command.memory_mib {
        config.resources.memory_mib = memory_mib;
    }
    if let Some(disk_gib) = command.disk_gib {
        config.resources.disk_gib = disk_gib;
    }
    write_json_file(&paths.config_path, &config)?;

    print!(
        "{}",
        render_machine_view(
            MachineCommandResult::Updated,
            &paths,
            Some(&config),
            Some(&state)
        )?
    );
    Ok(())
}

fn run_machine_rm(command: MachineRmCommand, roots: &MachineRootLayout) -> Result<(), Error> {
    let machine_name = command.name().to_owned();
    let paths = roots.paths(&machine_name);
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
            machine_name,
            state.lifecycle.as_str()
        )));
    }

    release_machine_ssh_port(roots, &machine_name)?;
    remove_dir_if_exists(&paths.config_dir)?;
    remove_dir_if_exists(&paths.state_dir)?;
    remove_dir_if_exists(&paths.data_dir)?;
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
    let (paths, mut config, mut state) = load_initialized_machine(roots, DEFAULT_MACHINE_NAME)?;
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
    let (paths, mut config, mut state) = load_initialized_machine(roots, DEFAULT_MACHINE_NAME)?;
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
            additional_supported_repositories: &[],
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
    let view = build_machine_status_view(result, paths, config, state);
    serde_yaml::to_string(&view)
        .map_err(|error| Error::Internal(format!("failed to serialize machine status: {error}")))
}

fn render_machine_status_view(
    result: MachineCommandResult,
    paths: &MachinePaths,
    config: Option<&MachineConfigRecord>,
    state: Option<&MachineStateRecord>,
    format: MachineStatusOutputFormat,
    quiet: bool,
) -> Result<String, Error> {
    let view = build_machine_status_view(result, paths, config, state);
    if quiet {
        return Ok(format!("{}\n", view.name));
    }
    match format {
        MachineStatusOutputFormat::Json => serde_json::to_string_pretty(&view).map_err(|error| {
            Error::Internal(format!("failed to serialize machine status: {error}"))
        }),
        MachineStatusOutputFormat::Yaml => serde_yaml::to_string(&view).map_err(|error| {
            Error::Internal(format!("failed to serialize machine status: {error}"))
        }),
        MachineStatusOutputFormat::Table => Ok(render_machine_status_table(&view)),
    }
}

fn build_machine_status_view(
    result: MachineCommandResult,
    paths: &MachinePaths,
    config: Option<&MachineConfigRecord>,
    state: Option<&MachineStateRecord>,
) -> MachineStatusView {
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
    MachineStatusView {
        result,
        initialized: config.is_some(),
        name: paths.name.clone(),
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
        guest_binary_contract: guest_binary_status_view(paths, config, state),
        last_error: state.and_then(|state| state.last_error.clone()),
    }
}

fn render_machine_status_table(view: &MachineStatusView) -> String {
    let provider = view
        .provider
        .map(|provider| provider.as_str().to_owned())
        .unwrap_or_else(|| "-".to_owned());
    let (cpus, memory_mib, disk_gib) = view
        .resources
        .map(|resources| {
            (
                resources.cpus.to_string(),
                resources.memory_mib.to_string(),
                resources.disk_gib.to_string(),
            )
        })
        .unwrap_or_else(|| ("-".to_owned(), "-".to_owned(), "-".to_owned()));
    let api = if view.machine_api.reachable {
        "reachable"
    } else {
        "unreachable"
    };

    format!(
        "{:<18} {:<14} {:<17} {:<10} {:>4} {:>12} {:>10} {:<11}\n{:<18} {:<14} {:<17} {:<10} {:>4} {:>12} {:>10} {:<11}\n",
        "NAME",
        "LIFECYCLE",
        "MANAGER",
        "PROVIDER",
        "CPUS",
        "MEMORY(MiB)",
        "DISK(GiB)",
        "API",
        view.name,
        view.lifecycle.as_str(),
        view.manager.as_str(),
        provider,
        cpus,
        memory_mib,
        disk_gib,
        api,
    )
}

fn build_machine_list_entries(
    roots: &MachineRootLayout,
) -> Result<Vec<MachineListEntryView>, Error> {
    let mut entries = Vec::new();
    for machine_name in initialized_machine_names(roots)? {
        let entry = with_machine_lock(roots, &machine_name, || {
            let paths = roots.paths(&machine_name);
            let Some(config) = load_machine_config_if_exists(&paths.config_path)? else {
                return Ok(None);
            };
            let mut state = load_machine_state_if_exists(&paths.state_path)?
                .unwrap_or_else(MachineStateRecord::initialized);
            refresh_machine_state(&paths, &mut state)?;
            write_json_file(&paths.state_path, &state)?;
            Ok(Some(MachineListEntryView {
                name: machine_name.clone(),
                lifecycle: state.lifecycle,
                provider: config.provider,
                cpus: config.resources.cpus,
                memory_mib: config.resources.memory_mib,
                disk_gib: config.resources.disk_gib,
            }))
        })?;
        if let Some(entry) = entry {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn initialized_machine_names(roots: &MachineRootLayout) -> Result<Vec<String>, Error> {
    let mut names = Vec::new();
    let entries = match fs::read_dir(&roots.config_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(names),
        Err(error) => {
            return Err(Error::Internal(format!(
                "failed to read machine config root {}: {error}",
                roots.config_root.display()
            )));
        }
    };

    for entry in entries {
        let entry = entry.map_err(|error| {
            Error::Internal(format!(
                "failed to read machine config root entry under {}: {error}",
                roots.config_root.display()
            ))
        })?;
        let file_type = entry.file_type().map_err(|error| {
            Error::Internal(format!(
                "failed to inspect machine config root entry {}: {error}",
                entry.path().display()
            ))
        })?;
        if !file_type.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().into_owned();
        if roots.paths(&name).config_path.is_file() {
            names.push(name);
        }
    }

    names.sort();
    Ok(names)
}

fn render_machine_list_view(
    machines: &[MachineListEntryView],
    format: MachineListOutputFormat,
    quiet: bool,
) -> Result<String, Error> {
    if quiet {
        return Ok(render_machine_list_quiet(machines));
    }

    match format {
        MachineListOutputFormat::Json => serde_json::to_string_pretty(machines)
            .map_err(|error| Error::Internal(format!("failed to serialize machine list: {error}"))),
        MachineListOutputFormat::Table => Ok(render_machine_list_table(machines)),
    }
}

fn render_machine_list_quiet(machines: &[MachineListEntryView]) -> String {
    if machines.is_empty() {
        return String::new();
    }

    let mut rendered = machines
        .iter()
        .map(|machine| machine.name.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    rendered.push('\n');
    rendered
}

fn render_machine_list_table(machines: &[MachineListEntryView]) -> String {
    let mut rendered =
        String::from("NAME               LIFECYCLE      PROVIDER   CPUS  MEMORY(MiB)  DISK(GiB)\n");
    for machine in machines {
        rendered.push_str(&format!(
            "{:<18} {:<14} {:<10} {:>4} {:>12} {:>10}\n",
            machine.name,
            machine.lifecycle.as_str(),
            machine.provider.as_str(),
            machine.cpus,
            machine.memory_mib,
            machine.disk_gib,
        ));
    }
    rendered
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MachineCpEndpoint {
    Host(String),
    Machine { name: String, path: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MachineCpTransfer {
    machine_name: String,
    machine_path: String,
    host_path: String,
    guest_is_src: bool,
}

fn resolve_machine_cp_transfer(
    src_path: &str,
    dest_path: &str,
) -> Result<MachineCpTransfer, Error> {
    let src = parse_machine_cp_endpoint(src_path)?;
    let dest = parse_machine_cp_endpoint(dest_path)?;

    match (src, dest) {
        (MachineCpEndpoint::Machine { name, path }, MachineCpEndpoint::Host(host_path)) => {
            Ok(MachineCpTransfer {
                machine_name: name,
                machine_path: path,
                host_path,
                guest_is_src: true,
            })
        }
        (MachineCpEndpoint::Host(host_path), MachineCpEndpoint::Machine { name, path }) => {
            Ok(MachineCpTransfer {
                machine_name: name,
                machine_path: path,
                host_path,
                guest_is_src: false,
            })
        }
        (MachineCpEndpoint::Machine { .. }, MachineCpEndpoint::Machine { .. }) => Err(
            Error::InvalidInput("copying between two machines is unsupported".to_owned()),
        ),
        (MachineCpEndpoint::Host(_), MachineCpEndpoint::Host(_)) => Err(Error::InvalidInput(
            "a machine name must prefix either the source path or destination path".to_owned(),
        )),
    }
}

fn parse_machine_cp_endpoint(value: &str) -> Result<MachineCpEndpoint, Error> {
    if looks_like_windows_host_path(value) {
        return Ok(MachineCpEndpoint::Host(value.to_owned()));
    }

    let Some((name, path)) = value.split_once(':') else {
        return Ok(MachineCpEndpoint::Host(value.to_owned()));
    };
    if name.is_empty() {
        return Ok(MachineCpEndpoint::Host(value.to_owned()));
    }
    if path.is_empty() {
        return Err(Error::InvalidInput(format!(
            "machine copy path '{}' is invalid; expected <machine>:<path>",
            value
        )));
    }

    Ok(MachineCpEndpoint::Machine {
        name: name.to_owned(),
        path: path.to_owned(),
    })
}

fn looks_like_windows_host_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}

fn render_machine_inspect_view(
    config: &MachineConfigRecord,
    state: &MachineStateRecord,
    format: MachineInspectOutputFormat,
) -> Result<String, Error> {
    let view = MachineInspectView {
        config: config.clone(),
        state: state.clone(),
    };
    match format {
        MachineInspectOutputFormat::Json => serde_json::to_string_pretty(&view).map_err(|error| {
            Error::Internal(format!(
                "failed to serialize machine inspect output: {error}"
            ))
        }),
        MachineInspectOutputFormat::Yaml => serde_yaml::to_string(&view).map_err(|error| {
            Error::Internal(format!(
                "failed to serialize machine inspect output: {error}"
            ))
        }),
    }
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

fn guest_binary_status_view(
    paths: &MachinePaths,
    config: Option<&MachineConfigRecord>,
    state: Option<&MachineStateRecord>,
) -> Option<MachineGuestBinaryStatusView> {
    let config = config?;
    if config.provider != MachineProvider::Krunkit
        || !uses_host_managed_machine_image_contract(config)
    {
        return None;
    }

    let desired = inspect_desired_guest_neovex_binary(paths);
    let mut observed_version = None;
    let mut observed_hash = None;
    let mut observed_error = None;

    if let Some(state) = state.filter(|state| state.lifecycle == MachineLifecycle::Running) {
        match inspect_observed_guest_neovex_binary(config, state) {
            Ok(observed) => {
                observed_version = observed.version;
                observed_hash = observed.hash;
            }
            Err(error) => observed_error = Some(error.to_string()),
        }
    }

    let observed_matches_desired = observed_hash
        .as_deref()
        .zip(desired.desired_hash.as_deref())
        .map(|(observed, desired)| observed == desired);

    Some(MachineGuestBinaryStatusView {
        install_path: desired.install_path,
        source: desired.source,
        source_detail: desired.source_detail,
        desired_path: desired.desired_path,
        desired_exists: desired.desired_exists,
        desired_version: desired.desired_version,
        desired_hash: desired.desired_hash,
        release_archive_path: desired.release_archive_path,
        release_archive_exists: desired.release_archive_exists,
        release_url: desired.release_url,
        observed_version,
        observed_hash,
        observed_matches_desired,
        desired_error: desired.error,
        observed_error,
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
        data_root: paths
            .data_dir
            .parent()
            .expect("machine data dir should have a root")
            .to_path_buf(),
        cache_root: paths
            .image_cache_dir
            .parent()
            .expect("machine image cache dir should have a root")
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
            parse_machine_record::<MachineConfigRecord>(path, &bytes, "machine config").map(Some)
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

fn with_machine_lock<T>(
    roots: &MachineRootLayout,
    machine_name: &str,
    operation: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    let _lock = lock_machine_records(roots, machine_name)?;
    operation()
}

fn with_default_machine_lock<T>(
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

fn resolve_data_root() -> Result<PathBuf, Error> {
    if let Some(path) = env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(path).join("neovex").join("machine"));
    }
    Ok(resolve_home_dir()?
        .join(".local")
        .join("share")
        .join("neovex")
        .join("machine"))
}

fn resolve_cache_root() -> Result<PathBuf, Error> {
    if let Some(path) = env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(path).join("neovex").join("machine"));
    }
    Ok(resolve_home_dir()?
        .join(".cache")
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
    data_root: PathBuf,
    cache_root: PathBuf,
    runtime_root: PathBuf,
}

impl MachineRootLayout {
    fn resolve() -> Result<Self, Error> {
        Ok(Self {
            config_root: resolve_config_root()?,
            state_root: resolve_state_root()?,
            data_root: resolve_data_root()?,
            cache_root: resolve_cache_root()?,
            runtime_root: resolve_runtime_root(),
        })
    }

    fn guest_api_default(runtime_root: PathBuf) -> Self {
        Self {
            config_root: PathBuf::from("/var/lib/neovex/machine/config"),
            state_root: PathBuf::from("/var/lib/neovex/machine/state"),
            data_root: PathBuf::from("/var/lib/neovex/machine/data"),
            cache_root: PathBuf::from("/var/lib/neovex/machine/cache"),
            runtime_root,
        }
    }

    #[cfg(test)]
    fn new(config_root: PathBuf, state_root: PathBuf, runtime_root: PathBuf) -> Self {
        let shared_parent = config_root
            .parent()
            .map(Path::to_path_buf)
            .and_then(|config_parent| {
                (state_root.parent() == Some(config_parent.as_path())
                    && runtime_root.parent() == Some(config_parent.as_path()))
                .then_some(config_parent)
            });
        Self {
            config_root,
            state_root,
            data_root: shared_parent
                .as_ref()
                .map(|parent| parent.join("data"))
                .unwrap_or_else(|| PathBuf::from("/tmp/neovex-test-data")),
            cache_root: shared_parent
                .as_ref()
                .map(|parent| parent.join("cache"))
                .unwrap_or_else(|| PathBuf::from("/tmp/neovex-test-cache")),
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
        let data_dir = self.data_root.join(name);
        let runtime_dir = self.runtime_root.clone();
        MachinePaths {
            name: name.to_owned(),
            config_dir: config_dir.clone(),
            state_dir: state_dir.clone(),
            data_dir: data_dir.clone(),
            runtime_dir: runtime_dir.clone(),
            config_path: config_dir.join("config.json"),
            generated_ignition_path: config_dir.join("generated.ign"),
            state_path: state_dir.join("status.json"),
            image_cache_dir: self.cache_root.join("images"),
            guest_binary_cache_dir: self.cache_root.join("guest-neovex"),
            materialized_image_path: data_dir.join("images").join(format!("{name}.raw")),
            api_socket_path: runtime_dir.join(format!("{name}-api.sock")),
            ready_socket_path: runtime_dir.join(format!("{name}.sock")),
            ignition_socket_path: runtime_dir.join(format!("{name}-ignition.sock")),
            gvproxy_socket_path: runtime_dir.join(format!("{name}-gvproxy.sock")),
            krunkit_endpoint_path: runtime_dir.join(format!("{name}-krunkit.sock")),
            efi_variable_store_path: data_dir.join("efi-variable-store"),
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
    fn as_str(self) -> &'static str {
        match self {
            Self::Krunkit => "krunkit",
            Self::Wsl2 => "wsl2",
        }
    }

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

impl MachineManagerState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Unconfigured => "unconfigured",
            Self::HelpersResolved => "helpers-resolved",
            Self::Launching => "launching",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Stale => "stale",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum MachineCommandResult {
    Initialized,
    InitializedAndStarted,
    Started,
    Status,
    Updated,
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
    data_root: PathBuf,
    cache_root: PathBuf,
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
    guest_binary_contract: Option<MachineGuestBinaryStatusView>,
    last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct MachineListEntryView {
    name: String,
    lifecycle: MachineLifecycle,
    provider: MachineProvider,
    cpus: u8,
    memory_mib: u32,
    disk_gib: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct MachineInspectView {
    config: MachineConfigRecord,
    state: MachineStateRecord,
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
struct MachineGuestBinaryStatusView {
    install_path: PathBuf,
    source: GuestNeovexBinarySourceKind,
    source_detail: String,
    desired_path: PathBuf,
    desired_exists: bool,
    desired_version: Option<String>,
    desired_hash: Option<String>,
    release_archive_path: Option<PathBuf>,
    release_archive_exists: Option<bool>,
    release_url: Option<String>,
    observed_version: Option<String>,
    observed_hash: Option<String>,
    observed_matches_desired: Option<bool>,
    desired_error: Option<String>,
    observed_error: Option<String>,
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
    data_dir: PathBuf,
    runtime_dir: PathBuf,
    config_path: PathBuf,
    generated_ignition_path: PathBuf,
    state_path: PathBuf,
    image_cache_dir: PathBuf,
    guest_binary_cache_dir: PathBuf,
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
        fs::create_dir_all(&self.data_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine data directory {}: {error}",
                self.data_dir.display()
            ))
        })?;
        fs::create_dir_all(&self.image_cache_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine image cache directory {}: {error}",
                self.image_cache_dir.display()
            ))
        })?;
        fs::create_dir_all(&self.guest_binary_cache_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create guest binary cache directory {}: {error}",
                self.guest_binary_cache_dir.display()
            ))
        })?;
        let materialized_parent = self.materialized_image_path.parent().ok_or_else(|| {
            Error::Internal(format!(
                "failed to resolve parent directory for machine image {}",
                self.materialized_image_path.display()
            ))
        })?;
        fs::create_dir_all(materialized_parent).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine image data directory {}: {error}",
                materialized_parent.display()
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

    fn krunkit_gvproxy_socket_path(&self) -> PathBuf {
        PathBuf::from(format!("{}-krun.sock", self.gvproxy_socket_path.display()))
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener as StdUnixListener;
    use std::sync::{Mutex, OnceLock};

    use super::*;
    use crate::machine::manager::MachineHelperEnvGuard;
    use clap::{Parser, error::ErrorKind};
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

    fn machine_guest_binary_override_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn lock_machine_guest_binary_override_env() -> std::sync::MutexGuard<'static, ()> {
        machine_guest_binary_override_env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    struct GuestBinaryOverrideEnvGuard {
        previous: Option<std::ffi::OsString>,
    }

    impl GuestBinaryOverrideEnvGuard {
        fn clear() -> Self {
            let previous = std::env::var_os("NEOVEX_MACHINE_GUEST_BINARY");
            unsafe { std::env::remove_var("NEOVEX_MACHINE_GUEST_BINARY") };
            Self { previous }
        }

        fn set(path: &Path) -> Self {
            let previous = std::env::var_os("NEOVEX_MACHINE_GUEST_BINARY");
            unsafe { std::env::set_var("NEOVEX_MACHINE_GUEST_BINARY", path) };
            Self { previous }
        }
    }

    impl Drop for GuestBinaryOverrideEnvGuard {
        fn drop(&mut self) {
            match self.previous.as_ref() {
                Some(value) => unsafe { std::env::set_var("NEOVEX_MACHINE_GUEST_BINARY", value) },
                None => unsafe { std::env::remove_var("NEOVEX_MACHINE_GUEST_BINARY") },
            }
        }
    }

    fn supported_stream_current_image_for_upgrade_test() -> String {
        if cfg!(target_os = "macos") {
            "docker://quay.io/podman/machine-os@sha256:abc123".to_owned()
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
            "--memory",
            "4096",
            "--disk-size",
            "40",
            "--image",
            "docker://ghcr.io/agentstation/neovex-machine-os:test",
            "--identity",
            "/tmp/neovex-test-ed25519",
            "--ignition-path",
            "/tmp/neovex-test.ign",
            "--firmware",
            "/tmp/neovex-test.efi",
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
                assert_eq!(
                    init.ssh_identity,
                    Some(PathBuf::from("/tmp/neovex-test-ed25519"))
                );
                assert_eq!(
                    init.ignition_file,
                    Some(PathBuf::from("/tmp/neovex-test.ign"))
                );
                assert_eq!(init.efi_store, Some(PathBuf::from("/tmp/neovex-test.efi")));
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
    fn machine_init_accepts_short_flag_aliases() {
        let cli = RootCli::parse_from([
            "neovex",
            "machine",
            "init",
            "-c",
            "4",
            "-m",
            "4096",
            "-d",
            "40",
            "-v",
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
    fn machine_init_rejects_legacy_flag_names() {
        for legacy_flag in [
            "--ssh-identity",
            "--ignition-file",
            "--efi-store",
            "--memory-mib",
            "--disk-gib",
        ] {
            let error =
                RootCli::try_parse_from(["neovex", "machine", "init", legacy_flag, "value"])
                    .expect_err("legacy flag should be rejected");
            assert_eq!(error.kind(), ErrorKind::UnknownArgument);
            let rendered = error.to_string();
            assert!(rendered.contains(legacy_flag));
            assert!(rendered.contains("unexpected argument"));
        }
    }

    #[test]
    fn machine_init_parses_now_flag() {
        let cli = RootCli::parse_from(["neovex", "machine", "init", "--now"]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine init should parse");
        };

        match machine.command {
            MachineSubcommand::Init(init) => assert!(init.now),
            _ => panic!("expected init subcommand"),
        }
    }

    #[test]
    fn machine_start_parses_create_if_missing_overrides() {
        let cli = RootCli::parse_from([
            "neovex",
            "machine",
            "start",
            "-c",
            "4",
            "--memory",
            "4096",
            "--disk-size",
            "40",
            "--image",
            "docker://ghcr.io/agentstation/neovex-machine-os:test",
            "--identity",
            "/tmp/neovex-test-ed25519",
            "--ignition-path",
            "/tmp/neovex-test.ign",
            "--firmware",
            "/tmp/neovex-test.efi",
            "-v",
            "/Users:/Users",
        ]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine start should parse");
        };

        match machine.command {
            MachineSubcommand::Start(start) => {
                assert_eq!(start.cpus, Some(4));
                assert_eq!(start.memory_mib, Some(4096));
                assert_eq!(start.disk_gib, Some(40));
                assert_eq!(
                    start.image,
                    Some("docker://ghcr.io/agentstation/neovex-machine-os:test".to_owned())
                );
                assert_eq!(
                    start.ssh_identity,
                    Some(PathBuf::from("/tmp/neovex-test-ed25519"))
                );
                assert_eq!(
                    start.ignition_file,
                    Some(PathBuf::from("/tmp/neovex-test.ign"))
                );
                assert_eq!(start.efi_store, Some(PathBuf::from("/tmp/neovex-test.efi")));
                assert_eq!(
                    start.volumes,
                    vec![MachineVolume {
                        source: PathBuf::from("/Users"),
                        target: PathBuf::from("/Users"),
                    }]
                );
            }
            _ => panic!("expected start subcommand"),
        }
    }

    #[test]
    fn machine_lifecycle_subcommands_accept_optional_name_positionals() {
        let cli = RootCli::parse_from(["neovex", "machine", "init", "team-a"]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine init should parse");
        };
        match machine.command {
            MachineSubcommand::Init(init) => assert_eq!(init.name.as_deref(), Some("team-a")),
            _ => panic!("expected init subcommand"),
        }

        let cli = RootCli::parse_from(["neovex", "machine", "start", "team-a"]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine start should parse");
        };
        match machine.command {
            MachineSubcommand::Start(start) => assert_eq!(start.name.as_deref(), Some("team-a")),
            _ => panic!("expected start subcommand"),
        }

        let cli = RootCli::parse_from(["neovex", "machine", "stop", "team-a"]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine stop should parse");
        };
        match machine.command {
            MachineSubcommand::Stop(stop) => assert_eq!(stop.name.as_deref(), Some("team-a")),
            _ => panic!("expected stop subcommand"),
        }

        let cli = RootCli::parse_from(["neovex", "machine", "status", "team-a"]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine status should parse");
        };
        match machine.command {
            MachineSubcommand::Status(status) => {
                assert_eq!(status.name.as_deref(), Some("team-a"))
            }
            _ => panic!("expected status subcommand"),
        }

        let cli = RootCli::parse_from(["neovex", "machine", "inspect", "team-a"]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine inspect should parse");
        };
        match machine.command {
            MachineSubcommand::Inspect(inspect) => {
                assert_eq!(inspect.name.as_deref(), Some("team-a"))
            }
            _ => panic!("expected inspect subcommand"),
        }

        let cli = RootCli::parse_from(["neovex", "machine", "set", "--cpus", "4", "team-a"]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine set should parse");
        };
        match machine.command {
            MachineSubcommand::Set(set) => {
                assert_eq!(set.cpus, Some(4));
                assert_eq!(set.name.as_deref(), Some("team-a"));
            }
            _ => panic!("expected set subcommand"),
        }

        let cli = RootCli::parse_from(["neovex", "machine", "rm", "team-a"]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine rm should parse");
        };
        match machine.command {
            MachineSubcommand::Rm(remove) => assert_eq!(remove.name.as_deref(), Some("team-a")),
            _ => panic!("expected rm subcommand"),
        }
    }

    #[test]
    fn machine_status_defaults_to_table_output_format() {
        let cli = RootCli::parse_from(["neovex", "machine", "status"]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine status should parse");
        };

        match machine.command {
            MachineSubcommand::Status(status) => {
                assert!(!status.quiet);
                assert_eq!(status.format, MachineStatusOutputFormat::Table);
                assert_eq!(status.name.as_deref(), None);
            }
            _ => panic!("expected status subcommand"),
        }
    }

    #[test]
    fn machine_status_accepts_json_and_yaml_output_formats() {
        for (format_value, expected) in [
            ("json", MachineStatusOutputFormat::Json),
            ("yaml", MachineStatusOutputFormat::Yaml),
            ("table", MachineStatusOutputFormat::Table),
        ] {
            let cli =
                RootCli::parse_from(["neovex", "machine", "status", "--format", format_value]);
            let Some(RootCommand::Machine(machine)) = cli.command else {
                panic!("machine status should parse");
            };

            match machine.command {
                MachineSubcommand::Status(status) => assert_eq!(status.format, expected),
                _ => panic!("expected status subcommand"),
            }
        }
    }

    #[test]
    fn machine_status_accepts_quiet_mode() {
        let cli = RootCli::parse_from(["neovex", "machine", "status", "--quiet", "team-a"]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine status should parse");
        };

        match machine.command {
            MachineSubcommand::Status(status) => {
                assert!(status.quiet);
                assert_eq!(status.name.as_deref(), Some("team-a"));
            }
            _ => panic!("expected status subcommand"),
        }
    }

    #[test]
    fn parses_machine_lifecycle_subcommands() {
        for command in ["start", "stop", "status", "list", "inspect", "rm"] {
            let cli = RootCli::parse_from(["neovex", "machine", command]);
            let Some(RootCommand::Machine(_)) = cli.command else {
                panic!("machine {command} should parse");
            };
        }
    }

    #[test]
    fn machine_help_uses_user_facing_descriptions() {
        let error = RootCli::try_parse_from(["neovex", "machine", "--help"])
            .expect_err("help should short-circuit");
        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        let rendered = error.to_string();
        assert!(rendered.contains("Initialize a new machine"));
        assert!(rendered.contains("Start a machine, creating it if needed"));
        assert!(rendered.contains("Stop a running machine"));
        assert!(rendered.contains("Display machine status"));
        assert!(rendered.contains("List initialized machines"));
        assert!(rendered.contains("Inspect a machine record"));
        assert!(rendered.contains("Update a stopped machine"));
        assert!(rendered.contains("Securely copy files between the host and a machine"));
        assert!(rendered.contains("Log in to a machine using SSH"));
        assert!(rendered.contains("Remove an existing machine"));
        assert!(rendered.contains("Manage machine OS images"));
        assert!(!rendered.contains("Validate persisted machine state"));
        assert!(!rendered.contains("runtime roots"));
    }

    #[test]
    fn machine_os_help_uses_user_facing_descriptions() {
        let error = RootCli::try_parse_from(["neovex", "machine", "os", "--help"])
            .expect_err("help should short-circuit");
        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        let rendered = error.to_string();
        assert!(rendered.contains("Use a specific machine OS image on the next boot"));
        assert!(
            rendered.contains("Switch to the supported machine OS image for this neovex release")
        );
        assert!(!rendered.contains("supported image that matches this neovex host version"));
    }

    #[test]
    fn machine_init_help_uses_user_facing_flag_descriptions() {
        let error = RootCli::try_parse_from(["neovex", "machine", "init", "--help"])
            .expect_err("help should short-circuit");
        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        let rendered = error.to_string();
        assert!(rendered.contains("--now"));
        assert!(rendered.contains("--cpus"));
        assert!(rendered.contains("-c"));
        assert!(rendered.contains("--memory"));
        assert!(rendered.contains("-m"));
        assert!(rendered.contains("--disk-size"));
        assert!(rendered.contains("-d"));
        assert!(rendered.contains("--identity"));
        assert!(rendered.contains("--ignition-path"));
        assert!(rendered.contains("--firmware"));
        assert!(rendered.contains("--volume"));
        assert!(rendered.contains("-v"));
        assert!(rendered.contains("Number of CPUs"));
        assert!(rendered.contains("Memory in MiB"));
        assert!(rendered.contains("Disk size in GiB"));
        assert!(rendered.contains("Machine OS image"));
        assert!(rendered.contains("Path to SSH identity for guest access"));
        assert!(rendered.contains("Path to Ignition config file"));
        assert!(rendered.contains("Path to EFI variable store"));
        assert!(rendered.contains("Host:guest volume mount"));
        assert!(!rendered.contains("to record in the machine config"));
        assert!(!rendered.contains("future virtiofs setup"));
        assert!(!rendered.contains("bootstrap vsock channel"));
        assert!(!rendered.contains("--ssh-identity"));
        assert!(!rendered.contains("--ignition-file"));
        assert!(!rendered.contains("--efi-store"));
        assert!(!rendered.contains("--memory-mib"));
        assert!(!rendered.contains("--disk-gib"));
    }

    #[test]
    fn machine_start_help_describes_create_if_missing_overrides() {
        let error = RootCli::try_parse_from(["neovex", "machine", "start", "--help"])
            .expect_err("help should short-circuit");
        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        let rendered = error.to_string();
        assert!(rendered.contains("Start a machine, creating it if needed"));
        assert!(rendered.contains("Number of CPUs to use if start creates the machine"));
        assert!(rendered.contains("Machine OS image to use if start creates the machine"));
        assert!(
            rendered.contains("Path to SSH identity for guest access if start creates the machine")
        );
        assert!(rendered.contains("--memory"));
        assert!(rendered.contains("--disk-size"));
        assert!(rendered.contains("--identity"));
        assert!(rendered.contains("--ignition-path"));
        assert!(rendered.contains("--firmware"));
        assert!(rendered.contains("--volume"));
    }

    #[test]
    fn machine_status_help_describes_output_formats() {
        let error = RootCli::try_parse_from(["neovex", "machine", "status", "--help"])
            .expect_err("help should short-circuit");
        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        let rendered = error.to_string();
        assert!(rendered.contains("--format"));
        assert!(rendered.contains("--quiet"));
        assert!(rendered.contains("-q"));
        assert!(rendered.contains("json"));
        assert!(rendered.contains("yaml"));
        assert!(rendered.contains("table"));
        assert!(rendered.contains("[default: table]"));
    }

    #[test]
    fn machine_list_parses_alias_formats_and_quiet_mode() {
        let cli = RootCli::parse_from(["neovex", "machine", "list"]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine list should parse");
        };
        match machine.command {
            MachineSubcommand::List(list) => {
                assert_eq!(list.format, MachineListOutputFormat::Table);
                assert!(!list.quiet);
            }
            _ => panic!("expected list subcommand"),
        }

        let cli = RootCli::parse_from(["neovex", "machine", "ls", "--format", "json", "--quiet"]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine ls should parse");
        };
        match machine.command {
            MachineSubcommand::List(list) => {
                assert_eq!(list.format, MachineListOutputFormat::Json);
                assert!(list.quiet);
            }
            _ => panic!("expected list subcommand"),
        }
    }

    #[test]
    fn machine_list_help_describes_formats_and_quiet_mode() {
        let error = RootCli::try_parse_from(["neovex", "machine", "list", "--help"])
            .expect_err("help should short-circuit");
        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        let rendered = error.to_string();
        assert!(rendered.contains("List initialized machines"));
        assert!(rendered.contains("--format"));
        assert!(rendered.contains("json"));
        assert!(rendered.contains("table"));
        assert!(rendered.contains("--quiet"));
        assert!(rendered.contains("-q"));
    }

    #[test]
    fn machine_inspect_defaults_to_json_and_accepts_yaml() {
        let cli = RootCli::parse_from(["neovex", "machine", "inspect"]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine inspect should parse");
        };
        match machine.command {
            MachineSubcommand::Inspect(inspect) => {
                assert_eq!(inspect.format, MachineInspectOutputFormat::Json);
                assert_eq!(inspect.name.as_deref(), None);
            }
            _ => panic!("expected inspect subcommand"),
        }

        let cli =
            RootCli::parse_from(["neovex", "machine", "inspect", "--format", "yaml", "team-a"]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine inspect with yaml should parse");
        };
        match machine.command {
            MachineSubcommand::Inspect(inspect) => {
                assert_eq!(inspect.format, MachineInspectOutputFormat::Yaml);
                assert_eq!(inspect.name.as_deref(), Some("team-a"));
            }
            _ => panic!("expected inspect subcommand"),
        }
    }

    #[test]
    fn machine_inspect_help_describes_output_formats() {
        let error = RootCli::try_parse_from(["neovex", "machine", "inspect", "--help"])
            .expect_err("help should short-circuit");
        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        let rendered = error.to_string();
        assert!(rendered.contains("Inspect a machine record"));
        assert!(rendered.contains("--format"));
        assert!(rendered.contains("json"));
        assert!(rendered.contains("yaml"));
        assert!(rendered.contains("[default: json]"));
    }

    #[test]
    fn machine_cp_parses_paths_and_quiet_mode() {
        let cli = RootCli::parse_from([
            "neovex",
            "machine",
            "cp",
            "--quiet",
            "./local.txt",
            "default:/tmp/remote.txt",
        ]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine cp should parse");
        };
        match machine.command {
            MachineSubcommand::Cp(copy) => {
                assert!(copy.quiet);
                assert_eq!(copy.src_path, "./local.txt");
                assert_eq!(copy.dest_path, "default:/tmp/remote.txt");
            }
            _ => panic!("expected cp subcommand"),
        }
    }

    #[test]
    fn machine_cp_help_describes_machine_prefixed_paths() {
        let error = RootCli::try_parse_from(["neovex", "machine", "cp", "--help"])
            .expect_err("help should short-circuit");
        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        let rendered = error.to_string();
        assert!(rendered.contains("Securely copy files between the host and a machine"));
        assert!(rendered.contains("SRC_PATH"));
        assert!(rendered.contains("DEST_PATH"));
        assert!(rendered.contains("--quiet"));
        assert!(rendered.contains("-q"));
    }

    #[test]
    fn machine_set_help_describes_resource_flags() {
        let error = RootCli::try_parse_from(["neovex", "machine", "set", "--help"])
            .expect_err("help should short-circuit");
        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        let rendered = error.to_string();
        assert!(rendered.contains("Update a stopped machine"));
        assert!(rendered.contains("--cpus"));
        assert!(rendered.contains("--memory"));
        assert!(rendered.contains("--disk-size"));
        assert!(rendered.contains("Number of CPUs"));
        assert!(rendered.contains("Memory in MiB"));
        assert!(rendered.contains("Disk size in GiB"));
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
    fn machine_ssh_prefers_existing_machine_name_before_guest_command() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths("team-a");
        paths.ensure_directories().expect("paths should exist");
        write_json_file(
            &paths.config_path,
            &MachineConfigRecord {
                version: CURRENT_MACHINE_CONFIG_VERSION,
                name: "team-a".to_owned(),
                provider: MachineProvider::Krunkit,
                guest: MachineGuestConfig {
                    image_source: MachineImageSource::parse(&default_machine_image())
                        .expect("default image should parse"),
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
                roots: layout.clone(),
            },
        )
        .expect("config should write");

        let ssh = MachineSshCommand {
            args: vec!["team-a".to_owned(), "uname".to_owned(), "-a".to_owned()],
        };

        let (machine_name, args) =
            resolve_machine_ssh_target(&ssh, &layout).expect("ssh target should resolve");

        assert_eq!(machine_name, "team-a");
        assert_eq!(args, vec!["uname", "-a"]);
    }

    #[test]
    fn machine_ssh_treats_unknown_first_arg_as_guest_command() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let ssh = MachineSshCommand {
            args: vec!["uname".to_owned(), "-a".to_owned()],
        };

        let (machine_name, args) =
            resolve_machine_ssh_target(&ssh, &layout).expect("ssh target should resolve");

        assert_eq!(machine_name, DEFAULT_MACHINE_NAME);
        assert_eq!(args, vec!["uname", "-a"]);
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
        assert_eq!(
            roots.data_root,
            PathBuf::from("/var/lib/neovex/machine/data")
        );
        assert_eq!(
            roots.cache_root,
            PathBuf::from("/var/lib/neovex/machine/cache")
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
            PathBuf::from("/tmp/data/default/images/default.raw")
        );
        assert_eq!(paths.image_cache_dir, PathBuf::from("/tmp/cache/images"));
        assert_eq!(
            paths.guest_binary_cache_dir,
            PathBuf::from("/tmp/cache/guest-neovex")
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
                    now: false,
                    name: None,
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
        assert_eq!(config.roots.data_root, temp_dir.path().join("data"));
        assert_eq!(config.roots.cache_root, temp_dir.path().join("cache"));
        assert_eq!(state.version, CURRENT_MACHINE_STATE_VERSION);
        assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
        assert!(paths.data_dir.exists());
        assert!(paths.image_cache_dir.exists());
        assert!(paths.guest_binary_cache_dir.exists());
        assert!(paths.runtime_dir.exists());
    }

    #[test]
    fn machine_init_writes_named_machine_records() {
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
                    now: false,
                    name: Some("team-a".to_owned()),
                }),
            },
            &layout,
        )
        .expect("named machine init should succeed");

        let named_paths = layout.paths("team-a");
        let default_paths = layout.paths(DEFAULT_MACHINE_NAME);
        let config = read_json_file_if_exists::<MachineConfigRecord>(&named_paths.config_path)
            .expect("named config should read")
            .expect("named config should exist");

        assert_eq!(config.name, "team-a");
        assert!(named_paths.config_path.is_file());
        assert!(named_paths.state_path.is_file());
        assert!(!default_paths.config_path.exists());
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
                    now: false,
                    name: None,
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
                command: MachineSubcommand::Rm(MachineRmCommand { name: None }),
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
    fn machine_remove_only_deletes_requested_machine() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );

        for machine_name in [DEFAULT_MACHINE_NAME, "team-a"] {
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
                        now: false,
                        name: Some(machine_name.to_owned()),
                    }),
                },
                &layout,
            )
            .expect("machine init should succeed");
        }

        run_machine_command_for_test(
            MachineCommand {
                command: MachineSubcommand::Rm(MachineRmCommand {
                    name: Some("team-a".to_owned()),
                }),
            },
            &layout,
        )
        .expect("named machine rm should succeed");

        assert!(layout.paths(DEFAULT_MACHINE_NAME).config_path.exists());
        assert!(layout.paths(DEFAULT_MACHINE_NAME).state_path.exists());
        assert!(!layout.paths("team-a").config_path.exists());
        assert!(!layout.paths("team-a").state_path.exists());
    }

    #[test]
    fn machine_set_updates_stopped_machine_config() {
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
                    now: false,
                    name: Some("team-a".to_owned()),
                }),
            },
            &layout,
        )
        .expect("machine init should succeed");

        run_machine_command_for_test(
            MachineCommand {
                command: MachineSubcommand::Set(MachineSetCommand {
                    cpus: Some(4),
                    memory_mib: Some(4096),
                    disk_gib: Some(40),
                    name: Some("team-a".to_owned()),
                }),
            },
            &layout,
        )
        .expect("machine set should succeed");

        let config =
            read_json_file_if_exists::<MachineConfigRecord>(&layout.paths("team-a").config_path)
                .expect("config should read")
                .expect("config should exist");
        assert_eq!(config.resources.cpus, 4);
        assert_eq!(config.resources.memory_mib, 4096);
        assert_eq!(config.resources.disk_gib, 40);
    }

    #[test]
    fn machine_set_requires_at_least_one_resource_flag() {
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
                    now: false,
                    name: None,
                }),
            },
            &layout,
        )
        .expect("machine init should succeed");

        let error = run_machine_command_for_test(
            MachineCommand {
                command: MachineSubcommand::Set(MachineSetCommand::default()),
            },
            &layout,
        )
        .expect_err("machine set without flags should fail");

        assert!(
            error
                .to_string()
                .contains("requires at least one of `--cpus`, `--memory`, or `--disk-size`")
        );
    }

    #[test]
    fn machine_set_rejects_running_machine() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths("team-a");
        paths
            .ensure_directories()
            .expect("machine directories should exist");
        write_json_file(
            &paths.config_path,
            &MachineConfigRecord {
                version: CURRENT_MACHINE_CONFIG_VERSION,
                name: "team-a".to_owned(),
                provider: MachineProvider::Krunkit,
                guest: MachineGuestConfig {
                    image_source: MachineImageSource::parse(&default_machine_image())
                        .expect("default image should parse"),
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
                roots: layout.clone(),
            },
        )
        .expect("config should write");
        write_json_file(
            &paths.state_path,
            &MachineStateRecord {
                version: CURRENT_MACHINE_STATE_VERSION,
                lifecycle: MachineLifecycle::Running,
                manager: MachineManagerState::Ready,
                runtime: None,
                last_error: None,
            },
        )
        .expect("state should write");

        let error = run_machine_command_for_test(
            MachineCommand {
                command: MachineSubcommand::Set(MachineSetCommand {
                    cpus: Some(4),
                    memory_mib: None,
                    disk_gib: None,
                    name: Some("team-a".to_owned()),
                }),
            },
            &layout,
        )
        .expect_err("machine set should reject running machine");

        let rendered = error.to_string();
        assert!(rendered.contains("machine 'team-a'"));
        assert!(rendered.contains("must be stopped"));
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
                    now: false,
                    name: None,
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
                "sha256:abc123"
            } else {
                "v0.1.0"
            }
        );
        assert_eq!(plan.target_image, default_machine_image());
        assert_eq!(plan.target_version, expected_upgrade_target_version());
        assert!(plan.update_available);
    }

    #[test]
    fn host_managed_macos_stream_uses_podman_repository_contract() {
        let config = MachineConfigRecord {
            version: CURRENT_MACHINE_CONFIG_VERSION,
            name: DEFAULT_MACHINE_NAME.to_owned(),
            provider: MachineProvider::Krunkit,
            guest: MachineGuestConfig {
                image_source: MachineImageSource::OciReference {
                    reference: default_machine_image(),
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

        assert_eq!(desired, config.guest.image_source);
        assert_eq!(
            uses_host_managed_machine_image_contract(&config),
            cfg!(target_os = "macos")
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
    fn load_machine_config_rejects_older_schema_versions_with_clear_error() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths(DEFAULT_MACHINE_NAME);
        fs::create_dir_all(&paths.config_dir).expect("config dir should exist");
        let older_config = serde_json::json!({
            "version": CURRENT_MACHINE_CONFIG_VERSION - 1,
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
                "data_root": layout.data_root,
                "cache_root": layout.cache_root,
                "runtime_root": layout.runtime_root,
            },
        });
        fs::write(
            &paths.config_path,
            serde_json::to_vec_pretty(&older_config).expect("older config should serialize"),
        )
        .expect("older config should write");

        let error = load_machine_config_if_exists(&paths.config_path)
            .expect_err("older config version should fail");

        assert!(
            error
                .to_string()
                .contains("uses unsupported schema version")
        );
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
                "data_root": layout.data_root,
                "cache_root": layout.cache_root,
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
    fn load_machine_state_rebuilds_older_schema_versions_with_explicit_error() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths(DEFAULT_MACHINE_NAME);
        fs::create_dir_all(&paths.state_dir).expect("state dir should exist");
        let older_state = serde_json::json!({
            "version": CURRENT_MACHINE_STATE_VERSION - 1,
            "lifecycle": "running",
            "manager": "ready",
            "runtime": null,
            "last_error": null,
        });
        fs::write(
            &paths.state_path,
            serde_json::to_vec_pretty(&older_state).expect("older state should serialize"),
        )
        .expect("older state should write");

        let state = load_machine_state_if_exists(&paths.state_path)
            .expect("state load should succeed by rebuilding")
            .expect("rebuilt state should exist");
        let rewritten = read_json_file_if_exists::<MachineStateRecord>(&paths.state_path)
            .expect("rewritten state should read")
            .expect("rewritten state should exist");

        assert_eq!(state.version, CURRENT_MACHINE_STATE_VERSION);
        assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
        assert_eq!(state.manager, MachineManagerState::Stale);
        assert!(
            state
                .last_error
                .as_deref()
                .is_some_and(|message| message.contains("unsupported schema version"))
        );
        assert_eq!(rewritten.version, CURRENT_MACHINE_STATE_VERSION);
        assert_eq!(rewritten, state);
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
    fn machine_status_table_output_is_default_human_summary() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths("team-a");
        let config = MachineConfigRecord {
            version: CURRENT_MACHINE_CONFIG_VERSION,
            name: "team-a".to_owned(),
            provider: MachineProvider::Krunkit,
            guest: MachineGuestConfig {
                image_source: MachineImageSource::parse(&default_machine_image())
                    .expect("default image should parse"),
                ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
                ssh_identity_path: None,
                ignition_file_path: None,
                efi_variable_store_path: None,
            },
            resources: MachineResources {
                cpus: 4,
                memory_mib: 4096,
                disk_gib: 40,
            },
            volumes: Vec::new(),
            roots: layout,
        };
        let state = MachineStateRecord {
            version: CURRENT_MACHINE_STATE_VERSION,
            lifecycle: MachineLifecycle::Running,
            manager: MachineManagerState::Ready,
            runtime: None,
            last_error: None,
        };

        let rendered = render_machine_status_view(
            MachineCommandResult::Status,
            &paths,
            Some(&config),
            Some(&state),
            MachineStatusOutputFormat::Table,
            false,
        )
        .expect("table output should render");

        assert!(rendered.contains("NAME"));
        assert!(rendered.contains("LIFECYCLE"));
        assert!(rendered.contains("MEMORY(MiB)"));
        assert!(rendered.contains("team-a"));
        assert!(rendered.contains("running"));
        assert!(rendered.contains("krunkit"));
        assert!(rendered.contains("4096"));
        assert!(rendered.contains("reachable") || rendered.contains("unreachable"));
        assert!(!rendered.contains("guest:"));
    }

    #[test]
    fn machine_status_json_output_serializes_full_status_view() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths("team-a");
        let config = MachineConfigRecord {
            version: CURRENT_MACHINE_CONFIG_VERSION,
            name: "team-a".to_owned(),
            provider: MachineProvider::Krunkit,
            guest: MachineGuestConfig {
                image_source: MachineImageSource::parse(&default_machine_image())
                    .expect("default image should parse"),
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
            roots: layout,
        };

        let rendered = render_machine_status_view(
            MachineCommandResult::Status,
            &paths,
            Some(&config),
            Some(&MachineStateRecord::initialized()),
            MachineStatusOutputFormat::Json,
            false,
        )
        .expect("json output should render");
        let json: serde_json::Value =
            serde_json::from_str(&rendered).expect("status JSON should parse");

        assert_eq!(json["name"], "team-a");
        assert_eq!(json["result"], "status");
        assert_eq!(json["provider"], "krunkit");
        assert_eq!(json["resources"]["cpus"], DEFAULT_MACHINE_CPUS);
        assert_eq!(json["initialized"], true);
    }

    #[test]
    fn machine_status_yaml_output_serializes_full_status_view() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths("team-a");

        let rendered = render_machine_status_view(
            MachineCommandResult::Uninitialized,
            &paths,
            None,
            None,
            MachineStatusOutputFormat::Yaml,
            false,
        )
        .expect("yaml output should render");

        assert!(rendered.contains("result: uninitialized"));
        assert!(rendered.contains("name: team-a"));
        assert!(rendered.contains("initialized: false"));
    }

    #[test]
    fn machine_list_table_output_is_human_summary() {
        let machines = vec![
            MachineListEntryView {
                name: "default".to_owned(),
                lifecycle: MachineLifecycle::Stopped,
                provider: MachineProvider::Krunkit,
                cpus: 2,
                memory_mib: 2048,
                disk_gib: 20,
            },
            MachineListEntryView {
                name: "team-a".to_owned(),
                lifecycle: MachineLifecycle::Running,
                provider: MachineProvider::Krunkit,
                cpus: 4,
                memory_mib: 4096,
                disk_gib: 40,
            },
        ];

        let rendered = render_machine_list_view(&machines, MachineListOutputFormat::Table, false)
            .expect("table output should render");

        assert!(rendered.contains("NAME"));
        assert!(rendered.contains("LIFECYCLE"));
        assert!(rendered.contains("PROVIDER"));
        assert!(rendered.contains("MEMORY(MiB)"));
        assert!(rendered.contains("default"));
        assert!(rendered.contains("team-a"));
        assert!(rendered.contains("running"));
        assert!(!rendered.contains("\"name\""));
    }

    #[test]
    fn machine_list_json_output_serializes_machine_summaries() {
        let machines = vec![MachineListEntryView {
            name: "team-a".to_owned(),
            lifecycle: MachineLifecycle::Running,
            provider: MachineProvider::Krunkit,
            cpus: 4,
            memory_mib: 4096,
            disk_gib: 40,
        }];

        let rendered = render_machine_list_view(&machines, MachineListOutputFormat::Json, false)
            .expect("json output should render");
        let json: serde_json::Value =
            serde_json::from_str(&rendered).expect("machine list JSON should parse");

        assert_eq!(json[0]["name"], "team-a");
        assert_eq!(json[0]["lifecycle"], "running");
        assert_eq!(json[0]["provider"], "krunkit");
        assert_eq!(json[0]["cpus"], 4);
    }

    #[test]
    fn machine_list_quiet_output_prints_names_only() {
        let machines = vec![
            MachineListEntryView {
                name: "default".to_owned(),
                lifecycle: MachineLifecycle::Stopped,
                provider: MachineProvider::Krunkit,
                cpus: 2,
                memory_mib: 2048,
                disk_gib: 20,
            },
            MachineListEntryView {
                name: "team-a".to_owned(),
                lifecycle: MachineLifecycle::Running,
                provider: MachineProvider::Krunkit,
                cpus: 4,
                memory_mib: 4096,
                disk_gib: 40,
            },
        ];

        let rendered = render_machine_list_view(&machines, MachineListOutputFormat::Table, true)
            .expect("quiet output should render");

        assert_eq!(rendered, "default\nteam-a\n");
    }

    #[test]
    fn machine_list_scans_initialized_machine_records_in_sorted_order() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );

        for (name, cpus, memory_mib, disk_gib) in [
            ("team-b", 6, 6144, 60),
            ("default", 2, 2048, 20),
            ("team-a", 4, 4096, 40),
        ] {
            let paths = layout.paths(name);
            paths.ensure_directories().expect("paths should exist");
            write_json_file(
                &paths.config_path,
                &MachineConfigRecord {
                    version: CURRENT_MACHINE_CONFIG_VERSION,
                    name: name.to_owned(),
                    provider: MachineProvider::Krunkit,
                    guest: MachineGuestConfig {
                        image_source: MachineImageSource::parse(&default_machine_image())
                            .expect("default image should parse"),
                        ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
                        ssh_identity_path: None,
                        ignition_file_path: None,
                        efi_variable_store_path: None,
                    },
                    resources: MachineResources {
                        cpus,
                        memory_mib,
                        disk_gib,
                    },
                    volumes: Vec::new(),
                    roots: layout.clone(),
                },
            )
            .expect("config should write");
            write_json_file(
                &paths.state_path,
                &MachineStateRecord {
                    version: CURRENT_MACHINE_STATE_VERSION,
                    lifecycle: MachineLifecycle::Stopped,
                    manager: MachineManagerState::Unconfigured,
                    runtime: None,
                    last_error: None,
                },
            )
            .expect("state should write");
        }

        let machines = build_machine_list_entries(&layout).expect("machine list should build");

        assert_eq!(
            machines
                .iter()
                .map(|machine| machine.name.as_str())
                .collect::<Vec<_>>(),
            vec!["default", "team-a", "team-b"]
        );
        assert_eq!(machines[0].lifecycle, MachineLifecycle::Stopped);
        assert_eq!(machines[1].cpus, 4);
        assert_eq!(machines[2].disk_gib, 60);
    }

    #[test]
    fn machine_cp_transfer_resolves_host_to_guest() {
        let transfer = resolve_machine_cp_transfer("./local.txt", "team-a:/tmp/remote.txt")
            .expect("host to guest transfer should parse");

        assert_eq!(transfer.machine_name, "team-a");
        assert_eq!(transfer.machine_path, "/tmp/remote.txt");
        assert_eq!(transfer.host_path, "./local.txt");
        assert!(!transfer.guest_is_src);
    }

    #[test]
    fn machine_cp_transfer_resolves_guest_to_host() {
        let transfer = resolve_machine_cp_transfer("team-a:/tmp/remote.txt", "./local.txt")
            .expect("guest to host transfer should parse");

        assert_eq!(transfer.machine_name, "team-a");
        assert_eq!(transfer.machine_path, "/tmp/remote.txt");
        assert_eq!(transfer.host_path, "./local.txt");
        assert!(transfer.guest_is_src);
    }

    #[test]
    fn machine_cp_transfer_rejects_invalid_endpoint_combinations() {
        let error = resolve_machine_cp_transfer("./left", "./right")
            .expect_err("host to host transfer should fail");
        assert!(
            error
                .to_string()
                .contains("a machine name must prefix either the source path or destination path")
        );

        let error = resolve_machine_cp_transfer("one:/tmp/a", "two:/tmp/b")
            .expect_err("machine to machine transfer should fail");
        assert!(
            error
                .to_string()
                .contains("copying between two machines is unsupported")
        );
    }

    #[test]
    fn machine_cp_treats_windows_drive_paths_as_host_paths() {
        assert_eq!(
            parse_machine_cp_endpoint(r"C:\temp\artifact.txt")
                .expect("windows path should parse as host"),
            MachineCpEndpoint::Host(r"C:\temp\artifact.txt".to_owned())
        );
    }

    #[test]
    fn machine_status_quiet_output_prints_name_only() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths("team-a");

        let rendered = render_machine_status_view(
            MachineCommandResult::Uninitialized,
            &paths,
            None,
            None,
            MachineStatusOutputFormat::Json,
            true,
        )
        .expect("quiet output should render");

        assert_eq!(rendered, "team-a\n");
    }

    #[test]
    fn machine_inspect_json_output_serializes_full_config_and_state() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let config = MachineConfigRecord {
            version: CURRENT_MACHINE_CONFIG_VERSION,
            name: "team-a".to_owned(),
            provider: MachineProvider::Krunkit,
            guest: MachineGuestConfig {
                image_source: MachineImageSource::parse(&default_machine_image())
                    .expect("default image should parse"),
                ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
                ssh_identity_path: Some(PathBuf::from("/tmp/team-a")),
                ignition_file_path: None,
                efi_variable_store_path: None,
            },
            resources: MachineResources {
                cpus: 4,
                memory_mib: 4096,
                disk_gib: 40,
            },
            volumes: vec![MachineVolume {
                source: PathBuf::from("/Users"),
                target: PathBuf::from("/Users"),
            }],
            roots: layout,
        };
        let state = MachineStateRecord {
            version: CURRENT_MACHINE_STATE_VERSION,
            lifecycle: MachineLifecycle::Stopped,
            manager: MachineManagerState::Unconfigured,
            runtime: None,
            last_error: Some("none".to_owned()),
        };

        let rendered =
            render_machine_inspect_view(&config, &state, MachineInspectOutputFormat::Json)
                .expect("inspect json should render");
        let json: serde_json::Value =
            serde_json::from_str(&rendered).expect("inspect JSON should parse");

        assert_eq!(json["config"]["name"], "team-a");
        assert_eq!(json["config"]["provider"], "krunkit");
        assert_eq!(json["config"]["resources"]["cpus"], 4);
        assert_eq!(json["state"]["lifecycle"], "stopped");
        assert_eq!(json["state"]["manager"], "unconfigured");
    }

    #[test]
    fn machine_inspect_yaml_output_serializes_full_config_and_state() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let config = MachineConfigRecord {
            version: CURRENT_MACHINE_CONFIG_VERSION,
            name: "default".to_owned(),
            provider: MachineProvider::Krunkit,
            guest: MachineGuestConfig {
                image_source: MachineImageSource::parse(&default_machine_image())
                    .expect("default image should parse"),
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
            roots: layout,
        };

        let rendered = render_machine_inspect_view(
            &config,
            &MachineStateRecord::initialized(),
            MachineInspectOutputFormat::Yaml,
        )
        .expect("inspect yaml should render");

        assert!(rendered.contains("config:"));
        assert!(rendered.contains("state:"));
        assert!(rendered.contains("name: default"));
        assert!(rendered.contains("provider: krunkit"));
        assert!(rendered.contains("lifecycle: stopped"));
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
    fn machine_status_renders_release_asset_guest_binary_contract() {
        let _env_lock = lock_machine_guest_binary_override_env();
        let _env_guard = GuestBinaryOverrideEnvGuard::clear();

        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths(DEFAULT_MACHINE_NAME);
        paths
            .ensure_directories()
            .expect("machine directories should exist");
        let config = MachineConfigRecord {
            version: CURRENT_MACHINE_CONFIG_VERSION,
            name: DEFAULT_MACHINE_NAME.to_owned(),
            provider: MachineProvider::Krunkit,
            guest: MachineGuestConfig {
                image_source: MachineImageSource::parse(&default_machine_image())
                    .expect("default image should parse"),
                ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
                ssh_identity_path: Some(temp_dir.path().join("neovex-test-ed25519")),
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
        let desired = inspect_desired_guest_neovex_binary(&paths);
        fs::write(&desired.desired_path, b"release guest binary")
            .expect("guest binary should write");

        let rendered = render_machine_view(
            MachineCommandResult::Status,
            &paths,
            Some(&config),
            Some(&MachineStateRecord::initialized()),
        )
        .expect("machine view should render");
        let desired = inspect_desired_guest_neovex_binary(&paths);

        if !cfg!(target_os = "macos") {
            assert!(rendered.contains("guest_binary_contract: null"));
            return;
        }

        assert!(rendered.contains("guest_binary_contract:"));
        assert!(rendered.contains("source: release-asset"));
        assert!(rendered.contains(&format!(
            "source_detail: GitHub release asset {}",
            current_machine_release_tag()
        )));
        assert!(rendered.contains(&format!(
            "desired_version: {}",
            current_machine_release_tag()
        )));
        assert!(rendered.contains(&format!("desired_path: {}", desired.desired_path.display())));
        assert!(rendered.contains("desired_exists: true"));
        assert!(rendered.contains(&format!(
            "desired_hash: {}",
            desired
                .desired_hash
                .as_deref()
                .expect("desired hash should exist for cached release asset")
        )));
    }

    #[test]
    fn machine_status_renders_explicit_override_guest_binary_contract() {
        let _env_lock = lock_machine_guest_binary_override_env();

        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths(DEFAULT_MACHINE_NAME);
        paths
            .ensure_directories()
            .expect("machine directories should exist");
        let override_binary = temp_dir.path().join("override-neovex");
        fs::write(&override_binary, b"override guest binary")
            .expect("override binary should write");
        let _env_guard = GuestBinaryOverrideEnvGuard::set(&override_binary);

        let config = MachineConfigRecord {
            version: CURRENT_MACHINE_CONFIG_VERSION,
            name: DEFAULT_MACHINE_NAME.to_owned(),
            provider: MachineProvider::Krunkit,
            guest: MachineGuestConfig {
                image_source: MachineImageSource::parse(&default_machine_image())
                    .expect("default image should parse"),
                ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
                ssh_identity_path: Some(temp_dir.path().join("neovex-test-ed25519")),
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

        let rendered = render_machine_view(
            MachineCommandResult::Status,
            &paths,
            Some(&config),
            Some(&MachineStateRecord::initialized()),
        )
        .expect("machine view should render");
        let desired = inspect_desired_guest_neovex_binary(&paths);

        if !cfg!(target_os = "macos") {
            assert!(rendered.contains("guest_binary_contract: null"));
            return;
        }

        assert!(rendered.contains("guest_binary_contract:"));
        assert!(rendered.contains("source: explicit-override"));
        assert!(rendered.contains(&format!(
            "source_detail: $NEOVEX_MACHINE_GUEST_BINARY={}",
            override_binary.display()
        )));
        assert!(rendered.contains(&format!("desired_path: {}", override_binary.display())));
        assert!(rendered.contains("desired_exists: true"));
        assert!(rendered.contains(&format!(
            "desired_hash: {}",
            desired
                .desired_hash
                .as_deref()
                .expect("desired hash should exist for explicit override")
        )));
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
                    now: false,
                    name: None,
                }),
            },
            &layout,
        )
        .expect("machine init should succeed");
        let paths = layout.paths(DEFAULT_MACHINE_NAME);
        fs::create_dir_all(&paths.runtime_dir).expect("runtime dir should exist");
        fs::write(paths.krunkit_gvproxy_socket_path(), [])
            .expect("derived krunkit socket should write");
        run_machine_command_for_test(
            MachineCommand {
                command: MachineSubcommand::Rm(MachineRmCommand { name: None }),
            },
            &layout,
        )
        .expect("machine rm should succeed");

        assert!(!paths.config_dir.exists());
        assert!(!paths.state_dir.exists());
        assert!(!paths.data_dir.exists());
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
                    now: false,
                    name: None,
                }),
            },
            &layout,
        )
        .expect("machine init should succeed");

        let error = run_machine_command_for_test(
            MachineCommand {
                command: MachineSubcommand::Start(MachineStartCommand::default()),
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

    #[test]
    fn machine_start_auto_initializes_before_start_failure() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );

        let error = run_machine_command_for_test(
            MachineCommand {
                command: MachineSubcommand::Start(MachineStartCommand {
                    cpus: Some(4),
                    memory_mib: Some(4096),
                    disk_gib: Some(40),
                    image: Some("docker://127.0.0.1:1/example/neovex-machine-os:test".to_owned()),
                    ssh_identity: None,
                    ignition_file: None,
                    efi_store: None,
                    volumes: vec![MachineVolume {
                        source: PathBuf::from("/Users"),
                        target: PathBuf::from("/Users"),
                    }],
                    name: None,
                }),
            },
            &layout,
        )
        .expect_err("machine start should surface OCI pull failure after auto-init");

        assert!(
            error
                .to_string()
                .contains("failed to resolve machine guest OCI reference")
        );

        let paths = layout.paths(DEFAULT_MACHINE_NAME);
        let config = read_json_file_if_exists::<MachineConfigRecord>(&paths.config_path)
            .expect("config should read")
            .expect("config should exist after auto-init");
        assert_eq!(config.resources.cpus, 4);
        assert_eq!(config.resources.memory_mib, 4096);
        assert_eq!(config.resources.disk_gib, 40);
        assert_eq!(
            config.guest.image_source,
            MachineImageSource::OciReference {
                reference: "docker://127.0.0.1:1/example/neovex-machine-os:test".to_owned(),
            }
        );
        assert_eq!(
            config.volumes,
            vec![MachineVolume {
                source: PathBuf::from("/Users"),
                target: PathBuf::from("/Users"),
            }]
        );
    }

    #[test]
    fn machine_start_auto_initializes_named_machine_before_start_failure() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );

        let error = run_machine_command_for_test(
            MachineCommand {
                command: MachineSubcommand::Start(MachineStartCommand {
                    image: Some("docker://127.0.0.1:1/example/neovex-machine-os:test".to_owned()),
                    name: Some("team-a".to_owned()),
                    ..MachineStartCommand::default()
                }),
            },
            &layout,
        )
        .expect_err("machine start should surface OCI pull failure after named auto-init");

        assert!(
            error
                .to_string()
                .contains("failed to resolve machine guest OCI reference")
        );

        assert!(layout.paths("team-a").config_path.is_file());
        assert!(layout.paths("team-a").state_path.is_file());
        assert!(!layout.paths(DEFAULT_MACHINE_NAME).config_path.exists());
    }

    #[test]
    fn machine_init_now_attempts_start_after_initialization() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );

        let error = run_machine_command_for_test(
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
                    now: true,
                    name: None,
                }),
            },
            &layout,
        )
        .expect_err("machine init --now should attempt start");

        assert!(
            error
                .to_string()
                .contains("failed to resolve machine guest OCI reference")
        );

        let paths = layout.paths(DEFAULT_MACHINE_NAME);
        assert!(paths.config_path.is_file());
        assert!(paths.state_path.is_file());
    }

    #[test]
    fn machine_start_rejects_create_if_missing_overrides_when_machine_exists() {
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
                    now: false,
                    name: None,
                }),
            },
            &layout,
        )
        .expect("machine init should succeed");

        let error = run_machine_command_for_test(
            MachineCommand {
                command: MachineSubcommand::Start(MachineStartCommand {
                    memory_mib: Some(4096),
                    ..MachineStartCommand::default()
                }),
            },
            &layout,
        )
        .expect_err("machine start should reject create-only overrides on existing machines");

        assert!(
            error
                .to_string()
                .contains("only uses init flags when creating a new machine"),
            "unexpected error: {error}"
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
