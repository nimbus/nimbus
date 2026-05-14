use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

use crate::cli_ux;

use super::record::MachineVolume;
use super::{
    DEFAULT_MACHINE_CPUS, DEFAULT_MACHINE_DISK_GIB, DEFAULT_MACHINE_MEMORY_MIB,
    DEFAULT_MACHINE_NAME, default_machine_image, parse_machine_volume,
};

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_GROUP_HELP_TEMPLATE,
    after_help = cli_ux::MACHINE_HELP_EXAMPLES,
    subcommand_help_heading = "Available Commands"
)]
pub(crate) struct MachineCommand {
    #[command(subcommand)]
    pub(super) command: MachineSubcommand,
}

#[derive(Debug, Subcommand)]
pub(super) enum MachineSubcommand {
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
    /// Display machine host info.
    Info(MachineInfoCommand),
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
    /// Internal guest machine configuration commands.
    #[command(hide = true)]
    GuestConfig(MachineGuestConfigCommand),
    /// Internal guest machine API daemon for macOS machine support.
    #[command(hide = true)]
    Api(MachineApiCommand),
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_GROUP_HELP_TEMPLATE,
    after_help = cli_ux::MACHINE_OS_HELP_EXAMPLES,
    subcommand_help_heading = "Available Commands"
)]
pub(super) struct MachineOsCommand {
    #[command(subcommand)]
    pub(super) command: MachineOsSubcommand,
}

#[derive(Debug, Subcommand)]
pub(super) enum MachineOsSubcommand {
    /// Use a specific machine OS image on the next boot.
    Apply(MachineOsApplyCommand),
    /// Switch to the supported machine OS image for this nimbus release.
    Upgrade(MachineOsUpgradeCommand),
    /// Queue the previous bootc deployment for the next boot.
    Rollback(MachineOsRollbackCommand),
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::MACHINE_OS_APPLY_HELP_EXAMPLES
)]
pub(super) struct MachineOsApplyCommand {
    /// OCI image reference or digest to use on the next boot.
    pub(super) image: String,

    /// Restart the machine immediately if it is running.
    #[arg(long)]
    pub(super) restart: bool,
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::MACHINE_OS_UPGRADE_HELP_EXAMPLES
)]
pub(super) struct MachineOsUpgradeCommand {
    /// Check whether an upgrade is available.
    #[arg(long)]
    pub(super) dry_run: bool,

    /// Restart the machine immediately if an upgrade is applied.
    #[arg(long)]
    pub(super) restart: bool,
}

#[derive(Debug, Args)]
#[command(help_template = cli_ux::COMMAND_HELP_TEMPLATE)]
pub(super) struct MachineOsRollbackCommand {
    /// Restart the machine immediately after queuing rollback.
    #[arg(long)]
    pub(super) restart: bool,
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::MACHINE_INIT_HELP_EXAMPLES
)]
pub(super) struct MachineInitCommand {
    /// Number of CPUs.
    #[arg(short = 'c', long, value_name = "COUNT", default_value_t = DEFAULT_MACHINE_CPUS)]
    pub(super) cpus: u8,

    /// Memory in MiB.
    #[arg(
        short = 'm',
        long = "memory",
        value_name = "MIB",
        default_value_t = DEFAULT_MACHINE_MEMORY_MIB
    )]
    pub(super) memory_mib: u32,

    /// Disk size in GiB.
    #[arg(
        short = 'd',
        long = "disk-size",
        value_name = "GIB",
        default_value_t = DEFAULT_MACHINE_DISK_GIB
    )]
    pub(super) disk_gib: u32,

    /// Machine OS image.
    #[arg(long, value_name = "SOURCE", default_value_t = default_machine_image())]
    pub(super) image: String,

    /// Path to SSH identity for guest access.
    #[arg(long = "identity", value_name = "PATH")]
    pub(super) ssh_identity: Option<PathBuf>,

    /// Legacy Ignition config file for explicit non-bootc image overrides.
    #[arg(long = "ignition-path", value_name = "PATH")]
    pub(super) ignition_file: Option<PathBuf>,

    /// Use bootc-native machine-config provisioning instead of Ignition.
    #[arg(long, hide = true, conflicts_with = "ignition_file")]
    pub(super) bootc_native: bool,

    /// Path to EFI variable store.
    #[arg(long = "firmware", value_name = "PATH")]
    pub(super) efi_store: Option<PathBuf>,

    /// Host:guest volume mount.
    #[arg(
        short = 'v',
        long = "volume",
        value_name = "HOST:GUEST",
        value_parser = parse_machine_volume
    )]
    pub(super) volumes: Vec<MachineVolume>,

    /// Start the machine after initializing it.
    #[arg(long)]
    pub(super) now: bool,

    /// Machine name.
    #[arg(value_name = "NAME")]
    pub(super) name: Option<String>,
}

impl MachineInitCommand {
    pub(super) fn name(&self) -> &str {
        self.name.as_deref().unwrap_or(DEFAULT_MACHINE_NAME)
    }
}

#[derive(Debug, Args, Clone, Default)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::MACHINE_START_HELP_EXAMPLES
)]
pub(super) struct MachineStartCommand {
    /// Number of CPUs to use if start creates the machine.
    #[arg(short = 'c', long, value_name = "COUNT")]
    pub(super) cpus: Option<u8>,

    /// Memory in MiB to use if start creates the machine.
    #[arg(short = 'm', long = "memory", value_name = "MIB")]
    pub(super) memory_mib: Option<u32>,

    /// Disk size in GiB to use if start creates the machine.
    #[arg(short = 'd', long = "disk-size", value_name = "GIB")]
    pub(super) disk_gib: Option<u32>,

    /// Machine OS image to use if start creates the machine.
    #[arg(long, value_name = "SOURCE")]
    pub(super) image: Option<String>,

    /// Path to SSH identity for guest access if start creates the machine.
    #[arg(long = "identity", value_name = "PATH")]
    pub(super) ssh_identity: Option<PathBuf>,

    /// Legacy Ignition config file if start creates a machine from an explicit non-bootc image.
    #[arg(long = "ignition-path", value_name = "PATH")]
    pub(super) ignition_file: Option<PathBuf>,

    /// Use bootc-native machine-config provisioning if start creates the machine.
    #[arg(long, hide = true, conflicts_with = "ignition_file")]
    pub(super) bootc_native: bool,

    /// Path to EFI variable store if start creates the machine.
    #[arg(long = "firmware", value_name = "PATH")]
    pub(super) efi_store: Option<PathBuf>,

    /// Host:guest volume mount if start creates the machine.
    #[arg(
        short = 'v',
        long = "volume",
        value_name = "HOST:GUEST",
        value_parser = parse_machine_volume
    )]
    pub(super) volumes: Vec<MachineVolume>,

    /// Suppress machine starting status output.
    #[arg(short = 'q', long)]
    pub(super) quiet: bool,

    /// Suppress informational tips.
    #[arg(long)]
    pub(super) no_info: bool,

    /// Machine name.
    #[arg(value_name = "NAME")]
    pub(super) name: Option<String>,
}

impl MachineStartCommand {
    pub(super) fn name(&self) -> &str {
        self.name.as_deref().unwrap_or(DEFAULT_MACHINE_NAME)
    }

    pub(super) fn has_create_overrides(&self) -> bool {
        self.cpus.is_some()
            || self.memory_mib.is_some()
            || self.disk_gib.is_some()
            || self.image.is_some()
            || self.ssh_identity.is_some()
            || self.ignition_file.is_some()
            || self.bootc_native
            || self.efi_store.is_some()
            || !self.volumes.is_empty()
    }

    pub(super) fn output_mode(&self) -> cli_ux::OutputMode {
        cli_ux::OutputMode {
            suppress_phase: self.quiet,
            suppress_info: self.quiet || self.no_info,
            suppress_progress: self.quiet,
        }
    }

    pub(super) fn into_init_command(self) -> MachineInitCommand {
        MachineInitCommand {
            cpus: self.cpus.unwrap_or(DEFAULT_MACHINE_CPUS),
            memory_mib: self.memory_mib.unwrap_or(DEFAULT_MACHINE_MEMORY_MIB),
            disk_gib: self.disk_gib.unwrap_or(DEFAULT_MACHINE_DISK_GIB),
            image: self.image.unwrap_or_else(default_machine_image),
            ssh_identity: self.ssh_identity,
            ignition_file: self.ignition_file,
            bootc_native: self.bootc_native,
            efi_store: self.efi_store,
            volumes: self.volumes,
            now: false,
            name: self.name,
        }
    }
}

#[derive(Debug, Args, Default)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::MACHINE_STOP_HELP_EXAMPLES
)]
pub(super) struct MachineStopCommand {
    /// Machine name.
    #[arg(value_name = "NAME")]
    pub(super) name: Option<String>,
}

impl MachineStopCommand {
    pub(super) fn name(&self) -> &str {
        self.name.as_deref().unwrap_or(DEFAULT_MACHINE_NAME)
    }
}

#[derive(Debug, Args, Default)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::MACHINE_STATUS_HELP_EXAMPLES
)]
pub(super) struct MachineStatusCommand {
    /// Output format.
    #[arg(short = 'f', long, value_enum, default_value_t = MachineStatusOutputFormat::Table)]
    pub(super) format: MachineStatusOutputFormat,

    /// Print the machine name only.
    #[arg(short = 'q', long)]
    pub(super) quiet: bool,

    /// Omit table headings from table output.
    #[arg(short = 'n', long = "noheading")]
    pub(super) no_heading: bool,

    /// Machine name.
    #[arg(value_name = "NAME")]
    pub(super) name: Option<String>,
}

impl MachineStatusCommand {
    pub(super) fn name(&self) -> &str {
        self.name.as_deref().unwrap_or(DEFAULT_MACHINE_NAME)
    }
}

#[derive(Debug, Args, Default)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::MACHINE_LIST_HELP_EXAMPLES
)]
pub(super) struct MachineListCommand {
    /// Output format. Defaults to table.
    #[arg(short = 'f', long, value_enum)]
    pub(super) format: Option<MachineListOutputFormat>,

    /// Print machine names only.
    #[arg(short = 'q', long)]
    pub(super) quiet: bool,

    /// Omit table headings from table output.
    #[arg(short = 'n', long = "noheading")]
    pub(super) no_heading: bool,
}

impl MachineListCommand {
    pub(super) fn format(&self) -> MachineListOutputFormat {
        self.format.unwrap_or_default()
    }
}

#[derive(Debug, Args, Default)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::MACHINE_INFO_HELP_EXAMPLES
)]
pub(super) struct MachineInfoCommand {
    /// Output format.
    #[arg(short = 'f', long, value_enum, default_value_t = MachineInfoOutputFormat::Yaml)]
    pub(super) format: MachineInfoOutputFormat,
}

#[derive(Debug, Args, Default)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::MACHINE_INSPECT_HELP_EXAMPLES
)]
pub(super) struct MachineInspectCommand {
    /// Output format.
    #[arg(short = 'f', long, value_enum, default_value_t = MachineInspectOutputFormat::Json)]
    pub(super) format: MachineInspectOutputFormat,

    /// Machine name.
    #[arg(value_name = "NAME")]
    pub(super) name: Option<String>,
}

impl MachineInspectCommand {
    pub(super) fn name(&self) -> &str {
        self.name.as_deref().unwrap_or(DEFAULT_MACHINE_NAME)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub(super) enum MachineStatusOutputFormat {
    Json,
    Yaml,
    #[default]
    Table,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub(super) enum MachineListOutputFormat {
    Json,
    #[default]
    Table,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub(super) enum MachineInfoOutputFormat {
    Json,
    #[default]
    Yaml,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub(super) enum MachineInspectOutputFormat {
    #[default]
    Json,
    Yaml,
}

#[derive(Debug, Args, Default)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::MACHINE_SET_HELP_EXAMPLES
)]
pub(super) struct MachineSetCommand {
    /// Number of CPUs.
    #[arg(short = 'c', long, value_name = "COUNT")]
    pub(super) cpus: Option<u8>,

    /// Memory in MiB.
    #[arg(short = 'm', long = "memory", value_name = "MIB")]
    pub(super) memory_mib: Option<u32>,

    /// Disk size in GiB.
    #[arg(short = 'd', long = "disk-size", value_name = "GIB")]
    pub(super) disk_gib: Option<u32>,

    /// Machine name.
    #[arg(value_name = "NAME")]
    pub(super) name: Option<String>,
}

impl MachineSetCommand {
    pub(super) fn name(&self) -> &str {
        self.name.as_deref().unwrap_or(DEFAULT_MACHINE_NAME)
    }

    pub(super) fn has_changes(&self) -> bool {
        self.cpus.is_some() || self.memory_mib.is_some() || self.disk_gib.is_some()
    }
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::MACHINE_CP_HELP_EXAMPLES
)]
pub(super) struct MachineCpCommand {
    /// Suppress copy status output.
    #[arg(short = 'q', long)]
    pub(super) quiet: bool,

    /// Source path.
    #[arg(value_name = "SRC_PATH")]
    pub(super) src_path: String,

    /// Destination path.
    #[arg(value_name = "DEST_PATH")]
    pub(super) dest_path: String,
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::MACHINE_SSH_HELP_EXAMPLES
)]
pub(super) struct MachineSshCommand {
    /// Optional command to execute in the guest once SSH wiring is available.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(super) args: Vec<String>,
}

#[derive(Debug, Args, Default)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::MACHINE_RM_HELP_EXAMPLES
)]
pub(super) struct MachineRmCommand {
    /// Machine name.
    #[arg(value_name = "NAME")]
    pub(super) name: Option<String>,
}

impl MachineRmCommand {
    pub(super) fn name(&self) -> &str {
        self.name.as_deref().unwrap_or(DEFAULT_MACHINE_NAME)
    }
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_GROUP_HELP_TEMPLATE,
    subcommand_help_heading = "Available Commands"
)]
pub(super) struct MachineGuestConfigCommand {
    #[command(subcommand)]
    pub(super) command: MachineGuestConfigSubcommand,
}

#[derive(Debug, Subcommand)]
pub(super) enum MachineGuestConfigSubcommand {
    /// Apply the host-provided bootc-native machine config bundle.
    Apply(MachineGuestConfigApplyCommand),
}

#[derive(Debug, Args)]
#[command(help_template = cli_ux::COMMAND_HELP_TEMPLATE)]
pub(super) struct MachineGuestConfigApplyCommand {
    /// Directory containing machine.json, authorized_keys, and volumes.json.
    #[arg(
        long,
        value_name = "PATH",
        default_value = "/run/nimbus-machine-config"
    )]
    pub(super) config_dir: PathBuf,
}

#[derive(Debug, Args)]
pub(super) struct MachineApiCommand {
    /// Direct unix socket path to bind for the guest machine API.
    #[arg(long, conflicts_with = "socket_activation")]
    pub(super) socket_path: Option<PathBuf>,

    /// Inherit the listening unix socket from systemd socket activation.
    #[arg(long, conflicts_with = "socket_path")]
    pub(super) socket_activation: bool,

    /// Optional override for the persisted control-plane directory root.
    #[arg(long)]
    pub(super) control_data_dir: Option<PathBuf>,
}
