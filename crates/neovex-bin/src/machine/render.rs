use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

use neovex::Error;
use serde::Serialize;

use crate::cli_ux::{self, TableColumn};

use super::client::MachineApiClient;
use super::command::{
    MachineInfoOutputFormat, MachineInspectOutputFormat, MachineListCommand,
    MachineListOutputFormat, MachineStatusOutputFormat,
};
use super::files::{
    load_machine_config_if_exists, load_machine_state_if_exists, with_default_machine_lock,
    with_machine_lock, write_json_file,
};
use super::manager::{
    GuestNeovexBinarySourceKind, MACHINE_API_FORWARD_TRANSPORT, MACHINE_API_FORWARD_USER,
    MachineRuntimeState, inspect_desired_guest_neovex_binary, inspect_observed_guest_neovex_binary,
    refresh_machine_state,
};
use super::protocol::MachineApiCapabilityResponse;
use super::record::{
    MachineConfigRecord, MachineGuestConfig, MachineLifecycle, MachineManagerState, MachinePaths,
    MachineProvider, MachineResources, MachineRootLayout, MachineStateRecord, MachineVolume,
};
use super::{
    DEFAULT_MACHINE_NAME, describe_machine_image_source, desired_machine_image_source,
    uses_host_managed_machine_image_contract,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum MachineCommandResult {
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
pub(super) enum MachineOsCommandResult {
    Applied,
    UpgradeCheck,
    Upgraded,
    AlreadyCurrent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct MachineRootsView {
    pub(super) config_root: PathBuf,
    pub(super) state_root: PathBuf,
    pub(super) data_root: PathBuf,
    pub(super) cache_root: PathBuf,
    pub(super) runtime_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct MachineInfoView {
    pub(super) version: String,
    pub(super) host: MachineHostInfoView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct MachineHostInfoView {
    pub(super) arch: String,
    pub(super) os: String,
    pub(super) current_release: String,
    pub(super) default_machine_name: String,
    pub(super) machine_count: usize,
    pub(super) running_machine_count: usize,
    pub(super) image_cache_dir: PathBuf,
    pub(super) guest_binary_cache_dir: PathBuf,
    pub(super) roots: MachineRootsView,
    pub(super) default_machine: MachineInfoDefaultMachineView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct MachineInfoDefaultMachineView {
    pub(super) initialized: bool,
    pub(super) lifecycle: MachineLifecycle,
    pub(super) manager: MachineManagerState,
    pub(super) provider: Option<MachineProvider>,
    pub(super) api_reachable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct MachineStatusView {
    pub(super) result: MachineCommandResult,
    pub(super) initialized: bool,
    pub(super) name: String,
    pub(super) lifecycle: MachineLifecycle,
    pub(super) manager: MachineManagerState,
    pub(super) provider: Option<MachineProvider>,
    pub(super) guest: Option<MachineGuestConfig>,
    pub(super) resources: Option<MachineResources>,
    pub(super) volumes: Vec<MachineVolume>,
    pub(super) roots: MachineRootsView,
    pub(super) paths: MachinePaths,
    pub(super) runtime: Option<MachineRuntimeState>,
    pub(super) machine_image_contract: Option<MachineImageContractStatusView>,
    pub(super) machine_api: MachineApiStatusView,
    pub(super) guest_binary_contract: Option<MachineGuestBinaryStatusView>,
    pub(super) last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct MachineListEntryView {
    pub(super) name: String,
    #[serde(rename = "default")]
    pub(super) is_default: bool,
    pub(super) lifecycle: MachineLifecycle,
    pub(super) provider: MachineProvider,
    pub(super) cpus: u8,
    pub(super) memory_mib: u32,
    pub(super) disk_gib: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct MachineInspectView {
    pub(super) config: MachineConfigRecord,
    pub(super) state: MachineStateRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct MachineImageContractStatusView {
    pub(super) host_managed: bool,
    pub(super) configured_image: String,
    pub(super) desired_image: String,
    pub(super) recorded_image: Option<String>,
    pub(super) recorded_matches_desired: bool,
    pub(super) materialized_image_path: PathBuf,
    pub(super) materialized_image_exists: bool,
    pub(super) efi_store_path: PathBuf,
    pub(super) efi_store_exists: bool,
    pub(super) rebuild_required: bool,
    pub(super) rebuild_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct MachineApiStatusView {
    pub(super) socket_path: PathBuf,
    pub(super) guest_socket_path: Option<PathBuf>,
    pub(super) transport: Option<String>,
    pub(super) forward_user: Option<String>,
    pub(super) identity_path: Option<PathBuf>,
    pub(super) exists: bool,
    pub(super) reachable: bool,
    pub(super) role: Option<String>,
    pub(super) protocol_version: Option<String>,
    pub(super) listen_mode: Option<String>,
    pub(super) capabilities: Option<MachineApiCapabilityResponse>,
    pub(super) error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct MachineGuestBinaryStatusView {
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
    pub(super) observed_version: Option<String>,
    pub(super) observed_hash: Option<String>,
    pub(super) observed_matches_desired: Option<bool>,
    pub(super) desired_error: Option<String>,
    pub(super) observed_error: Option<String>,
}

pub(super) fn render_machine_action_view(
    result: MachineCommandResult,
    paths: &MachinePaths,
) -> Result<String, Error> {
    let summary = match result {
        MachineCommandResult::Initialized => {
            format!("Machine \"{}\" initialized successfully", paths.name)
        }
        MachineCommandResult::InitializedAndStarted => {
            format!(
                "Machine \"{}\" initialized and started successfully",
                paths.name
            )
        }
        MachineCommandResult::Started => {
            format!("Machine \"{}\" started successfully", paths.name)
        }
        MachineCommandResult::Updated => {
            format!("Machine \"{}\" updated successfully", paths.name)
        }
        MachineCommandResult::Stopped => {
            format!("Machine \"{}\" stopped successfully", paths.name)
        }
        MachineCommandResult::Removed => {
            format!("Machine \"{}\" removed successfully", paths.name)
        }
        MachineCommandResult::Status | MachineCommandResult::Uninitialized => {
            return Err(Error::Internal(format!(
                "machine action renderer cannot summarize {:?}",
                result
            )));
        }
    };
    Ok(cli_ux::format_action_summary(&summary))
}

pub(super) fn render_machine_status_view(
    result: MachineCommandResult,
    paths: &MachinePaths,
    config: Option<&MachineConfigRecord>,
    state: Option<&MachineStateRecord>,
    format: MachineStatusOutputFormat,
    no_heading: bool,
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
        MachineStatusOutputFormat::Table => Ok(render_machine_status_table(&view, no_heading)),
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

fn render_machine_status_table(view: &MachineStatusView, no_heading: bool) -> String {
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

    let columns = [
        TableColumn::left("NAME", 18),
        TableColumn::left("LIFECYCLE", 14),
        TableColumn::left("MANAGER", 17),
        TableColumn::left("PROVIDER", 10),
        TableColumn::right("CPUS", 4),
        TableColumn::right("MEMORY(MiB)", 12),
        TableColumn::right("DISK(GiB)", 10),
        TableColumn::left("API", 11),
    ];
    let rows = vec![vec![
        view.name.clone(),
        view.lifecycle.as_str().to_owned(),
        view.manager.as_str().to_owned(),
        provider,
        cpus,
        memory_mib,
        disk_gib,
        api.to_owned(),
    ]];
    cli_ux::render_table_with_options(
        &columns,
        &rows,
        cli_ux::TableRenderOptions {
            omit_header: no_heading,
        },
    )
}

pub(super) fn build_machine_list_entries(
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
                is_default: machine_name == DEFAULT_MACHINE_NAME,
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
    entries.sort_by(|left, right| {
        machine_list_sort_rank(left)
            .cmp(&machine_list_sort_rank(right))
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(entries)
}

fn machine_list_sort_rank(machine: &MachineListEntryView) -> u8 {
    match machine.lifecycle {
        MachineLifecycle::Starting | MachineLifecycle::Running => 0,
        _ if machine.is_default => 1,
        MachineLifecycle::Failed => 2,
        MachineLifecycle::Stopped => 3,
        MachineLifecycle::Uninitialized => 4,
    }
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

pub(super) fn render_machine_list_view(
    machines: &[MachineListEntryView],
    command: &MachineListCommand,
) -> Result<String, Error> {
    if command.quiet && command.format.is_none() {
        return Ok(render_machine_list_quiet(machines));
    }

    match command.format() {
        MachineListOutputFormat::Json => serde_json::to_string_pretty(machines)
            .map_err(|error| Error::Internal(format!("failed to serialize machine list: {error}"))),
        MachineListOutputFormat::Table => {
            Ok(render_machine_list_table(machines, command.no_heading))
        }
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

fn render_machine_list_name(machine: &MachineListEntryView) -> String {
    if machine.is_default {
        format!("{}*", machine.name)
    } else {
        machine.name.clone()
    }
}

fn render_machine_list_table(machines: &[MachineListEntryView], no_heading: bool) -> String {
    let columns = [
        TableColumn::left("NAME", 18),
        TableColumn::left("LIFECYCLE", 14),
        TableColumn::left("PROVIDER", 10),
        TableColumn::right("CPUS", 4),
        TableColumn::right("MEMORY(MiB)", 12),
        TableColumn::right("DISK(GiB)", 10),
    ];
    let rows = machines
        .iter()
        .map(|machine| {
            vec![
                render_machine_list_name(machine),
                machine.lifecycle.as_str().to_owned(),
                machine.provider.as_str().to_owned(),
                machine.cpus.to_string(),
                machine.memory_mib.to_string(),
                machine.disk_gib.to_string(),
            ]
        })
        .collect::<Vec<_>>();
    cli_ux::render_table_with_options(
        &columns,
        &rows,
        cli_ux::TableRenderOptions {
            omit_header: no_heading,
        },
    )
}

pub(super) fn build_machine_info_view(roots: &MachineRootLayout) -> Result<MachineInfoView, Error> {
    let machines = build_machine_list_entries(roots)?;
    let default_paths = roots.paths(DEFAULT_MACHINE_NAME);
    let (
        default_initialized,
        default_lifecycle,
        default_manager,
        default_provider,
        default_api_reachable,
    ) = with_default_machine_lock(roots, || {
        let default_config = load_machine_config_if_exists(&default_paths.config_path)?;
        Ok(if let Some(config) = default_config.as_ref() {
            let mut state = load_machine_state_if_exists(&default_paths.state_path)?
                .unwrap_or_else(MachineStateRecord::initialized);
            refresh_machine_state(&default_paths, &mut state)?;
            write_json_file(&default_paths.state_path, &state)?;
            (
                true,
                state.lifecycle,
                state.manager,
                Some(config.provider),
                machine_api_status_view(&default_paths, Some(config)).reachable,
            )
        } else {
            (
                false,
                MachineLifecycle::Uninitialized,
                MachineManagerState::Unconfigured,
                None,
                false,
            )
        })
    })?;

    Ok(MachineInfoView {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        host: MachineHostInfoView {
            arch: env::consts::ARCH.to_owned(),
            os: env::consts::OS.to_owned(),
            current_release: super::current_machine_release_tag(),
            default_machine_name: DEFAULT_MACHINE_NAME.to_owned(),
            machine_count: machines.len(),
            running_machine_count: machines
                .iter()
                .filter(|machine| machine.lifecycle == MachineLifecycle::Running)
                .count(),
            image_cache_dir: default_paths.image_cache_dir.clone(),
            guest_binary_cache_dir: default_paths.guest_binary_cache_dir.clone(),
            roots: roots_view(&default_paths),
            default_machine: MachineInfoDefaultMachineView {
                initialized: default_initialized,
                lifecycle: default_lifecycle,
                manager: default_manager,
                provider: default_provider,
                api_reachable: default_api_reachable,
            },
        },
    })
}

pub(super) fn render_machine_info_view(
    view: &MachineInfoView,
    format: MachineInfoOutputFormat,
) -> Result<String, Error> {
    match format {
        MachineInfoOutputFormat::Json => serde_json::to_string_pretty(view)
            .map_err(|error| Error::Internal(format!("failed to serialize machine info: {error}"))),
        MachineInfoOutputFormat::Yaml => serde_yaml::to_string(view)
            .map_err(|error| Error::Internal(format!("failed to serialize machine info: {error}"))),
    }
}

pub(super) fn render_machine_inspect_view(
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

pub(super) fn render_machine_os_apply_view(
    result: MachineOsCommandResult,
    paths: &MachinePaths,
    outcome: &super::handlers::MachineOsApplyOutcome,
    restart_requested: bool,
) -> Result<String, Error> {
    let summary = match result {
        MachineOsCommandResult::Applied if outcome.restarted => format!(
            "Machine \"{}\" machine OS applied and restarted successfully",
            paths.name
        ),
        MachineOsCommandResult::Applied => {
            format!("Machine \"{}\" machine OS applied successfully", paths.name)
        }
        MachineOsCommandResult::AlreadyCurrent => format!(
            "Machine \"{}\" already uses the requested machine OS image",
            paths.name
        ),
        MachineOsCommandResult::UpgradeCheck | MachineOsCommandResult::Upgraded => {
            return Err(Error::Internal(format!(
                "machine os apply renderer cannot summarize {:?}",
                result
            )));
        }
    };
    let mut detail_lines = vec![format!("Image: {}", outcome.current_image)];
    if matches!(result, MachineOsCommandResult::Applied)
        && outcome.previous_image != outcome.current_image
    {
        detail_lines.push(format!("Previous image: {}", outcome.previous_image));
    }
    if matches!(result, MachineOsCommandResult::Applied) && !outcome.restarted && !restart_requested
    {
        detail_lines.push(cli_ux::format_hint(&format!(
            "run `{}` to boot the updated image",
            super::handlers::machine_command_with_optional_name("start", &paths.name)
        )));
    }
    Ok(cli_ux::format_action_block(&summary, &detail_lines))
}

pub(super) fn render_machine_os_upgrade_view(
    result: MachineOsCommandResult,
    paths: &MachinePaths,
    plan: &super::handlers::MachineOsUpgradePlan,
    dry_run: bool,
    restart_requested: bool,
    restarted: bool,
) -> Result<String, Error> {
    let summary = match result {
        MachineOsCommandResult::UpgradeCheck => {
            format!("Machine \"{}\" machine OS update available", paths.name)
        }
        MachineOsCommandResult::AlreadyCurrent => format!(
            "Machine \"{}\" already uses the supported machine OS image",
            paths.name
        ),
        MachineOsCommandResult::Upgraded if restarted => format!(
            "Machine \"{}\" machine OS upgraded and restarted successfully",
            paths.name
        ),
        MachineOsCommandResult::Upgraded => {
            format!(
                "Machine \"{}\" machine OS upgraded successfully",
                paths.name
            )
        }
        MachineOsCommandResult::Applied => {
            return Err(Error::Internal(format!(
                "machine os upgrade renderer cannot summarize {:?}",
                result
            )));
        }
    };
    let mut detail_lines = match result {
        MachineOsCommandResult::UpgradeCheck => vec![
            format!("Current image: {}", plan.current_image),
            format!("Target image: {}", plan.target_image),
        ],
        MachineOsCommandResult::AlreadyCurrent => vec![format!("Image: {}", plan.current_image)],
        MachineOsCommandResult::Upgraded => vec![format!("Image: {}", plan.target_image)],
        MachineOsCommandResult::Applied => Vec::new(),
    };
    if matches!(result, MachineOsCommandResult::UpgradeCheck) && dry_run {
        detail_lines.push(cli_ux::format_hint(
            "run `neovex machine os upgrade` to apply the supported image",
        ));
    }
    if matches!(result, MachineOsCommandResult::Upgraded) && !restarted && !restart_requested {
        detail_lines.push(cli_ux::format_hint(&format!(
            "run `{}` to boot the updated image",
            super::handlers::machine_command_with_optional_name("start", &paths.name)
        )));
    }
    Ok(cli_ux::format_action_block(&summary, &detail_lines))
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

pub(super) fn machine_api_status_view(
    paths: &MachinePaths,
    config: Option<&MachineConfigRecord>,
) -> MachineApiStatusView {
    let socket_path = paths.api_socket_path.clone();
    let exists = socket_path.exists();
    let guest_socket_path = config
        .and_then(|config| config.guest.ssh_identity_path.as_ref())
        .map(|_| PathBuf::from(super::bootstrap::GUEST_NEOVEX_SOCKET));
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
