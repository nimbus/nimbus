use std::process::Stdio;

use nimbus::Error;
use semver::Version;

use crate::cli_ux;

use super::api;
use super::client::MachineApiClient;
use super::command::{
    MachineCommand, MachineCpCommand, MachineGuestConfigCommand, MachineGuestConfigSubcommand,
    MachineInfoCommand, MachineInitCommand, MachineInspectCommand, MachineListCommand,
    MachineOsApplyCommand, MachineOsCommand, MachineOsRollbackCommand, MachineOsSubcommand,
    MachineOsUpgradeCommand, MachineRmCommand, MachineSetCommand, MachineSshCommand,
    MachineStartCommand, MachineStatusCommand, MachineStopCommand, MachineSubcommand,
};
use super::files::{
    load_initialized_machine, load_machine_config_if_exists, load_machine_state_if_exists,
    remove_dir_if_empty, remove_dir_if_exists, remove_machine_runtime_artifacts,
    with_default_machine_lock, with_machine_lock, write_json_file,
};
use super::local_server::try_run_lifecycle_command_via_live_server;
use super::manager::{
    build_scp_command, build_ssh_command, refresh_machine_state, release_machine_ssh_port,
    start_machine, stop_machine,
};
use super::record::{
    MachineConfigRecord, MachineGuestProvisioning, MachineImageSource, MachineLifecycle,
    MachinePaths, MachineRootLayout, MachineStateRecord, resolve_runtime_root,
};
use super::render::{
    MachineCommandResult, MachineOsCommandResult, build_machine_info_view,
    build_machine_list_entries, render_machine_action_view, render_machine_info_view,
    render_machine_inspect_view, render_machine_list_view, render_machine_os_apply_view,
    render_machine_os_upgrade_view, render_machine_status_view,
};
use super::{
    DEFAULT_MACHINE_NAME, DEFAULT_NIMBUS_MACHINE_IMAGE_REPOSITORY,
    default_machine_image_for_provider, default_machine_volumes, describe_machine_image_source,
    invalidate_materialized_machine_os, machine_image_reference_repository,
    machine_image_reference_version_label, uses_nimbus_bootc_machine_image_source,
    uses_podman_machine_image_source,
};
#[cfg(unix)]
use nimbus_machine::api::{
    MachineApiBootcRollbackRequest, MachineApiBootcStatusResponse, MachineApiBootcSwitchRequest,
};

mod os;
mod transfer;

#[cfg(test)]
pub(in crate::machine) use os::plan_machine_os_upgrade;
use os::run_machine_os;
pub(in crate::machine) use os::{MachineOsApplyOutcome, MachineOsUpgradePlan};
#[cfg(test)]
pub(in crate::machine) use transfer::{
    MachineCpEndpoint, parse_machine_cp_endpoint, resolve_machine_cp_transfer,
    resolve_machine_ssh_target,
};
use transfer::{resolve_machine_cp_target_name, resolve_machine_ssh_target_name};
#[cfg(not(test))]
use transfer::{resolve_machine_cp_transfer, resolve_machine_ssh_target};

pub(crate) async fn run_machine_command(command: MachineCommand) -> Result<(), Error> {
    let roots = resolve_roots_for_command(&command)?;
    run_machine_command_with_layout(command, &roots).await
}

pub(super) fn resolve_roots_for_command(
    command: &MachineCommand,
) -> Result<MachineRootLayout, Error> {
    match &command.command {
        MachineSubcommand::Api(_) | MachineSubcommand::GuestConfig(_) => {
            MachineRootLayout::resolve()
                .or_else(|_| Ok(MachineRootLayout::guest_api_default(resolve_runtime_root())))
        }
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
            "machine '{}' is {} and its guest machine API is not available; run `nimbus machine start` first",
            DEFAULT_MACHINE_NAME,
            state.lifecycle.as_str()
        )));
    }
    if !paths.api_socket_path.exists() {
        return Err(Error::InvalidInput(format!(
            "machine '{}' is running but guest machine API socket {} is missing; run `nimbus machine status` or restart the machine",
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
            "machine '{}' started but guest machine API socket {} is missing; run `nimbus machine status` or restart the machine",
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

pub(super) async fn run_machine_command_with_layout(
    command: MachineCommand,
    roots: &MachineRootLayout,
) -> Result<(), Error> {
    if try_run_lifecycle_command_via_live_server(&command.command, roots).await? {
        return Ok(());
    }

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
        MachineSubcommand::Info(info) => run_machine_info(info, roots),
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
        MachineSubcommand::GuestConfig(guest_config) => run_machine_guest_config(guest_config),
        MachineSubcommand::Api(api) => api::run_machine_api_command(api, roots).await,
    }
}

fn run_machine_init(command: MachineInitCommand, roots: &MachineRootLayout) -> Result<(), Error> {
    let now = command.now;
    let (paths, mut config, mut state) = create_machine_with_layout_locked(command, roots)?;

    let result = if now {
        paths.ensure_runtime_directories()?;
        start_machine(&paths, &mut config, &mut state)?;
        MachineCommandResult::InitializedAndStarted
    } else {
        MachineCommandResult::Initialized
    };

    emit_machine_stdout(&render_machine_action_view(result, &paths)?)?;
    Ok(())
}

pub(super) fn create_machine_with_layout(
    command: MachineInitCommand,
    roots: &MachineRootLayout,
) -> Result<(MachineConfigRecord, MachineStateRecord), Error> {
    let machine_name = command.name().to_owned();
    with_machine_lock(roots, &machine_name, || {
        let (_paths, config, state) = create_machine_with_layout_locked(command, roots)?;
        Ok((config, state))
    })
}

fn create_machine_with_layout_locked(
    command: MachineInitCommand,
    roots: &MachineRootLayout,
) -> Result<(MachinePaths, MachineConfigRecord, MachineStateRecord), Error> {
    initialize_machine_record(command, roots)
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
        bootc_native,
        efi_store,
        volumes,
        now: _,
        name: _,
    } = command;
    let image_source = MachineImageSource::parse(&image)?;
    let bootc_native = bootc_native || uses_nimbus_bootc_machine_image_source(&image_source);
    if bootc_native && ignition_file.is_some() {
        return Err(Error::InvalidInput(
            "bootc-native machine images use Nimbus machine-config provisioning and cannot also use an Ignition file; use a Podman machine-os image override for the legacy Ignition contract".to_owned(),
        ));
    }
    let provisioning = if bootc_native {
        super::record::MachineGuestProvisioning::BootcMachineConfig
    } else {
        super::record::MachineGuestProvisioning::Ignition
    };
    let config = MachineConfigRecord {
        version: super::CURRENT_MACHINE_CONFIG_VERSION,
        name: machine_name,
        // No CLI/config provider flag is wired yet, so selection comes from the
        // environment (`NIMBUS_MACHINE_PROVIDER`) or the static krunkit default.
        provider: super::resolve_machine_provider(None)?,
        guest: super::record::MachineGuestConfig {
            image_source,
            provisioning,
            ssh_user: if bootc_native {
                super::DEFAULT_BOOTC_MACHINE_SSH_USER.to_owned()
            } else {
                super::DEFAULT_MACHINE_SSH_USER.to_owned()
            },
            ssh_identity_path: ssh_identity,
            ignition_file_path: ignition_file,
            efi_variable_store_path: efi_store,
        },
        resources: super::record::MachineResources {
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
    let output_mode = command.output_mode();
    let (paths, _config, _state, created) =
        start_machine_with_layout_and_command_locked(command, roots, Some(output_mode))?;
    let result = if created {
        MachineCommandResult::InitializedAndStarted
    } else {
        MachineCommandResult::Started
    };
    emit_machine_stdout(&render_machine_action_view(result, &paths)?)?;
    Ok(())
}

pub(crate) fn start_machine_with_layout(
    machine_name: &str,
    roots: &MachineRootLayout,
) -> Result<(MachineConfigRecord, MachineStateRecord), Error> {
    let command = MachineStartCommand {
        name: Some(machine_name.to_owned()),
        quiet: true,
        no_info: true,
        ..MachineStartCommand::default()
    };
    let output_mode = command.output_mode();
    let (_paths, config, state, _created) =
        start_machine_with_layout_and_command(command, roots, Some(output_mode))?;
    Ok((config, state))
}

fn start_machine_with_layout_and_command(
    command: MachineStartCommand,
    roots: &MachineRootLayout,
    output_mode: Option<cli_ux::OutputMode>,
) -> Result<(MachinePaths, MachineConfigRecord, MachineStateRecord, bool), Error> {
    let machine_name = command.name().to_owned();
    with_machine_lock(roots, &machine_name, || {
        start_machine_with_layout_and_command_locked(command, roots, output_mode)
    })
}

fn start_machine_with_layout_and_command_locked(
    command: MachineStartCommand,
    roots: &MachineRootLayout,
    output_mode: Option<cli_ux::OutputMode>,
) -> Result<(MachinePaths, MachineConfigRecord, MachineStateRecord, bool), Error> {
    let machine_name = command.name().to_owned();
    let paths = roots.paths(&machine_name);
    let (paths, mut config, mut state, created) = if paths.config_path.exists() {
        if command.has_create_overrides() {
            return Err(Error::AlreadyExists(format!(
                "machine '{}' is already initialized at {}.\n{}",
                machine_name,
                paths.config_path.display(),
                cli_ux::format_hint(
                    "use `nimbus machine set` to change CPU, memory, or disk for an existing machine, or `nimbus machine os apply <oci-ref-or-digest>` to change its base image"
                )
            )));
        }
        let (paths, config, state) = load_initialized_machine(roots, &machine_name)?;
        (paths, config, state, false)
    } else {
        let (paths, config, state) = initialize_machine_record(command.into_init_command(), roots)?;
        (paths, config, state, true)
    };
    paths.ensure_runtime_directories()?;
    let _output_mode_guard = output_mode.map(cli_ux::push_output_mode);
    start_machine(&paths, &mut config, &mut state)?;
    Ok((paths, config, state, created))
}

fn run_machine_stop(command: MachineStopCommand, roots: &MachineRootLayout) -> Result<(), Error> {
    let machine_name = command.name().to_owned();
    let (paths, _config, _state) = stop_machine_with_layout_locked(&machine_name, roots)?;
    emit_machine_stdout(&render_machine_action_view(
        MachineCommandResult::Stopped,
        &paths,
    )?)?;
    Ok(())
}

pub(crate) fn stop_machine_with_layout(
    machine_name: &str,
    roots: &MachineRootLayout,
) -> Result<(MachinePaths, MachineConfigRecord, MachineStateRecord), Error> {
    with_machine_lock(roots, machine_name, || {
        stop_machine_with_layout_locked(machine_name, roots)
    })
}

fn stop_machine_with_layout_locked(
    machine_name: &str,
    roots: &MachineRootLayout,
) -> Result<(MachinePaths, MachineConfigRecord, MachineStateRecord), Error> {
    let (paths, config, mut state) = load_initialized_machine(roots, machine_name)?;
    stop_machine(&paths, &config, &mut state)?;
    Ok((paths, config, state))
}

pub(crate) fn restart_machine_with_layout(
    machine_name: &str,
    roots: &MachineRootLayout,
) -> Result<(MachineConfigRecord, MachineStateRecord), Error> {
    with_machine_lock(roots, machine_name, || {
        let (paths, mut config, mut state) = stop_machine_with_layout_locked(machine_name, roots)?;
        paths.ensure_runtime_directories()?;
        let _output_mode_guard = cli_ux::push_output_mode(cli_ux::OutputMode {
            suppress_phase: true,
            suppress_info: true,
            suppress_progress: true,
        });
        start_machine(&paths, &mut config, &mut state)?;
        Ok((config, state))
    })
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
    emit_machine_stdout(&render_machine_status_view(
        result,
        &paths,
        config.as_ref(),
        state.as_ref(),
        command.format,
        command.no_heading,
        command.quiet,
    )?)?;
    Ok(())
}

fn run_machine_list(command: MachineListCommand, roots: &MachineRootLayout) -> Result<(), Error> {
    let machines = build_machine_list_entries(roots)?;
    emit_machine_stdout(&render_machine_list_view(&machines, &command)?)?;
    Ok(())
}

fn run_machine_info(command: MachineInfoCommand, roots: &MachineRootLayout) -> Result<(), Error> {
    let view = build_machine_info_view(roots)?;
    emit_machine_stdout(&render_machine_info_view(&view, command.format)?)?;
    Ok(())
}

fn run_machine_inspect(
    command: MachineInspectCommand,
    roots: &MachineRootLayout,
) -> Result<(), Error> {
    let machine_name = command.name().to_owned();
    let (_paths, config, state) = load_initialized_machine(roots, &machine_name)?;
    emit_machine_stdout(&render_machine_inspect_view(
        &config,
        &state,
        command.format,
    )?)?;
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
        cli_ux::write_stdout_line("Copy successful")
            .map_err(|error| Error::Internal(format!("failed to write copy summary: {error}")))?;
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
    let (paths, _config, _state) = update_machine_with_layout_locked(command, roots)?;
    emit_machine_stdout(&render_machine_action_view(
        MachineCommandResult::Updated,
        &paths,
    )?)?;
    Ok(())
}

pub(super) fn update_machine_with_layout(
    command: MachineSetCommand,
    roots: &MachineRootLayout,
) -> Result<(MachineConfigRecord, MachineStateRecord), Error> {
    let machine_name = command.name().to_owned();
    with_machine_lock(roots, &machine_name, || {
        let (_paths, config, state) = update_machine_with_layout_locked(command, roots)?;
        Ok((config, state))
    })
}

fn update_machine_with_layout_locked(
    command: MachineSetCommand,
    roots: &MachineRootLayout,
) -> Result<(MachinePaths, MachineConfigRecord, MachineStateRecord), Error> {
    if !command.has_changes() {
        return Err(Error::InvalidInput(
            "machine set requires at least one of `--cpus`, `--memory`, or `--disk-size`"
                .to_owned(),
        ));
    }

    let machine_name = command.name().to_owned();
    let (paths, mut config, state) = load_initialized_machine(roots, &machine_name)?;
    if state.lifecycle != MachineLifecycle::Stopped {
        return Err(Error::conflict(format!(
            "machine '{}' is {} and must be stopped before applying `nimbus machine set`.\n{}",
            machine_name,
            state.lifecycle.as_str(),
            cli_ux::format_hint(&format!(
                "run `{}` and retry once the machine is stopped",
                machine_command_with_optional_name("stop", &machine_name)
            ))
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
    Ok((paths, config, state))
}

fn run_machine_rm(command: MachineRmCommand, roots: &MachineRootLayout) -> Result<(), Error> {
    let machine_name = command.name().to_owned();
    let (paths, _config, _state) = delete_machine_with_layout_locked(&machine_name, roots)?;
    emit_machine_stdout(&render_machine_action_view(
        MachineCommandResult::Removed,
        &paths,
    )?)?;
    Ok(())
}

pub(super) fn delete_machine_with_layout(
    machine_name: &str,
    roots: &MachineRootLayout,
) -> Result<(MachineConfigRecord, MachineStateRecord), Error> {
    with_machine_lock(roots, machine_name, || {
        let (_paths, config, state) = delete_machine_with_layout_locked(machine_name, roots)?;
        Ok((config, state))
    })
}

fn delete_machine_with_layout_locked(
    machine_name: &str,
    roots: &MachineRootLayout,
) -> Result<(MachinePaths, MachineConfigRecord, MachineStateRecord), Error> {
    let (paths, config, state) = load_initialized_machine(roots, machine_name)?;

    if matches!(
        state.lifecycle,
        MachineLifecycle::Starting | MachineLifecycle::Running
    ) {
        return Err(Error::conflict(format!(
            "machine '{}' is {} and cannot be removed safely.\n{}",
            machine_name,
            state.lifecycle.as_str(),
            cli_ux::format_hint(&format!(
                "run `{}` first, then remove the machine once it is stopped",
                machine_command_with_optional_name("stop", machine_name)
            ))
        )));
    }

    release_machine_ssh_port(roots, &state)?;
    remove_dir_if_exists(&paths.config_dir)?;
    remove_dir_if_exists(&paths.state_dir)?;
    remove_dir_if_exists(&paths.data_dir)?;
    remove_machine_runtime_artifacts(&paths)?;
    remove_dir_if_empty(&paths.runtime_dir)?;
    Ok((paths, config, state))
}

fn run_machine_guest_config(command: MachineGuestConfigCommand) -> Result<(), Error> {
    match command.command {
        MachineGuestConfigSubcommand::Apply(apply) => {
            super::guest_config::apply_machine_guest_config(apply)
        }
    }
}

pub(super) fn emit_machine_stdout(rendered: &str) -> Result<(), Error> {
    cli_ux::write_stdout(rendered)
        .map_err(|error| Error::Internal(format!("failed to write machine output: {error}")))
}

pub(super) fn machine_command_with_optional_name(subcommand: &str, machine_name: &str) -> String {
    if machine_name == DEFAULT_MACHINE_NAME {
        format!("nimbus machine {subcommand}")
    } else {
        format!("nimbus machine {subcommand} {machine_name}")
    }
}
