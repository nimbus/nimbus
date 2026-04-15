use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use clap::{Args, Subcommand};
use neovex::Error;
use serde::{Deserialize, Serialize};

mod api;
mod backend;
mod bootstrap;
mod client;
mod manager;
mod protocol;

#[cfg(test)]
pub(crate) use self::api::{
    MachineApiListenMode, MachineApiState, bind_direct_listener, serve_machine_api,
};
pub(crate) use self::backend::ForwardedMachineApiSandboxBackend;
pub(crate) use self::client::MachineApiClient;
use self::manager::{
    MACHINE_API_FORWARD_TRANSPORT, MACHINE_API_FORWARD_USER, MachineRuntimeState,
    build_ssh_command, refresh_machine_state, start_machine, stop_machine,
};
use self::protocol::MachineApiCapabilityResponse;

const DEFAULT_MACHINE_NAME: &str = "default";
/// The default machine image reference, derived from the crate version at build time.
/// neovex v0.0.1 → `docker://ghcr.io/agentstation/neovex-machine-os:v0.0.1`
fn default_machine_image() -> String {
    format!(
        "docker://ghcr.io/agentstation/neovex-machine-os:v{}",
        env!("CARGO_PKG_VERSION")
    )
}
const DEFAULT_MACHINE_SSH_USER: &str = "core";
const DEFAULT_MACHINE_RUNTIME_ROOT: &str = "/tmp/neovex";
const MACHINE_RUNTIME_ROOT_ENV: &str = "NEOVEX_MACHINE_RUNTIME_ROOT";
const DEFAULT_MACHINE_CPUS: u8 = 2;
const DEFAULT_MACHINE_MEMORY_MIB: u32 = 2048;
const DEFAULT_MACHINE_DISK_GIB: u32 = 20;

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
    /// Internal guest machine API daemon for macOS machine support.
    #[command(hide = true)]
    Api(MachineApiCommand),
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
    let roots = MachineRootLayout::resolve()?;
    run_machine_command_with_layout(command, &roots).await
}

pub(crate) fn require_default_machine_api_client() -> Result<MachineApiClient, Error> {
    let roots = MachineRootLayout::resolve()?;
    let (paths, _, state) = load_initialized_machine(&roots)?;
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

async fn run_machine_command_with_layout(
    command: MachineCommand,
    roots: &MachineRootLayout,
) -> Result<(), Error> {
    match command.command {
        MachineSubcommand::Init(init) => run_machine_init(init, roots),
        MachineSubcommand::Start(start) => run_machine_start(start, roots),
        MachineSubcommand::Stop(stop) => run_machine_stop(stop, roots),
        MachineSubcommand::Status(status) => run_machine_status(status, roots),
        MachineSubcommand::Ssh(ssh) => run_machine_ssh(ssh, roots),
        MachineSubcommand::Rm(remove) => run_machine_rm(remove, roots),
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
    let (paths, config, mut state) = load_initialized_machine(roots)?;
    paths.ensure_runtime_directories()?;
    start_machine(&paths, &config, &mut state)?;
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
    stop_machine(&paths, &mut state)?;
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
    let config = read_json_file_if_exists::<MachineConfigRecord>(&paths.config_path)?;
    let mut state = read_json_file_if_exists::<MachineStateRecord>(&paths.state_path)?;
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
    let config = read_json_file_if_exists::<MachineConfigRecord>(&paths.config_path)?;
    let state = read_json_file_if_exists::<MachineStateRecord>(&paths.state_path)?;

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

fn load_initialized_machine(
    roots: &MachineRootLayout,
) -> Result<(MachinePaths, MachineConfigRecord, MachineStateRecord), Error> {
    let paths = roots.paths(DEFAULT_MACHINE_NAME);
    let config =
        read_json_file_if_exists::<MachineConfigRecord>(&paths.config_path)?.ok_or_else(|| {
            Error::InvalidInput(format!(
                "machine '{}' is not initialized; run `neovex machine init` first",
                DEFAULT_MACHINE_NAME
            ))
        })?;
    let mut state = read_json_file_if_exists::<MachineStateRecord>(&paths.state_path)?
        .unwrap_or_else(MachineStateRecord::initialized);
    refresh_machine_state(&paths, &mut state)?;
    write_json_file(&paths.state_path, &state)?;
    Ok((paths, config, state))
}

fn render_machine_view(
    result: MachineCommandResult,
    paths: &MachinePaths,
    config: Option<&MachineConfigRecord>,
    state: Option<&MachineStateRecord>,
) -> Result<String, Error> {
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
        machine_api: machine_api_status_view(paths, config),
        last_error: state.and_then(|state| state.last_error.clone()),
    };
    serde_yaml::to_string(&view)
        .map_err(|error| Error::Internal(format!("failed to serialize machine status: {error}")))
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
        vec![MachineVolume {
            source: PathBuf::from("/Users"),
            target: PathBuf::from("/Users"),
        }]
    } else {
        Vec::new()
    }
}

fn parse_machine_volume(value: &str) -> Result<MachineVolume, String> {
    MachineVolume::parse(value)
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            Error::Internal(format!(
                "failed to create parent directory {}: {error}",
                parent.display()
            ))
        })?;
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| {
        Error::Internal(format!("failed to serialize {}: {error}", path.display()))
    })?;
    fs::write(path, bytes)
        .map_err(|error| Error::Internal(format!("failed to write {}: {error}", path.display())))
}

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

    #[cfg(test)]
    fn new(config_root: PathBuf, state_root: PathBuf, runtime_root: PathBuf) -> Self {
        Self {
            config_root,
            state_root,
            runtime_root,
        }
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
    lifecycle: MachineLifecycle,
    manager: MachineManagerState,
    runtime: Option<MachineRuntimeState>,
    last_error: Option<String>,
}

impl MachineStateRecord {
    fn initialized() -> Self {
        Self {
            lifecycle: MachineLifecycle::Stopped,
            manager: MachineManagerState::Unconfigured,
            runtime: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum MachineProvider {
    Krunkit,
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
    machine_api: MachineApiStatusView,
    last_error: Option<String>,
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

    #[test]
    fn parses_machine_init_defaults_to_stable_release_channel() {
        let cli = RootCli::parse_from(["neovex", "machine", "init"]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine init should parse");
        };

        match machine.command {
            MachineSubcommand::Init(init) => {
                assert_eq!(init.image, default_machine_image());
                assert_eq!(
                    init.image,
                    "docker://ghcr.io/agentstation/neovex-machine-os:stable"
                );
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
        assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
        assert!(paths.runtime_dir.exists());
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
                "protocol_version": "v1alpha1",
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
                "protocol_version": "v1alpha1",
                "service_execution_ready": false,
                "service_execution_mode": "standard_containers",
                "supported_service_backends": ["container"],
                "supported_operations": ["healthz", "capabilities"],
                "required_binaries": [
                    {
                        "name": "buildah",
                        "present": true,
                        "resolved_path": "/usr/bin/buildah"
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
        assert_eq!(api.protocol_version.as_deref(), Some("v1alpha1"));
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
        let krunkit_stub = temp_dir.path().join("krunkit");
        let gvproxy_stub = temp_dir.path().join("gvproxy");
        std::fs::write(&krunkit_stub, "#!/bin/sh\n").expect("krunkit stub should write");
        std::fs::write(&gvproxy_stub, "#!/bin/sh\n").expect("gvproxy stub should write");
        unsafe {
            std::env::set_var("NEOVEX_MACHINE_KRUNKIT", &krunkit_stub);
            std::env::set_var("NEOVEX_MACHINE_GVPROXY", &gvproxy_stub);
        }
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
                    image: "docker://127.0.0.1:9/example/neovex-machine-os:test".to_owned(),
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

        unsafe {
            std::env::remove_var("NEOVEX_MACHINE_KRUNKIT");
            std::env::remove_var("NEOVEX_MACHINE_GVPROXY");
        }

        assert!(
            error
                .to_string()
                .contains("failed to resolve machine guest OCI reference")
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
