use std::process::Stdio;

use nimbus::Error;
use semver::Version;

use crate::cli_ux;

use super::api;
use super::client::MachineApiClient;
use super::command::{
    MachineCommand, MachineCpCommand, MachineInfoCommand, MachineInitCommand,
    MachineInspectCommand, MachineListCommand, MachineOsApplyCommand, MachineOsCommand,
    MachineOsSubcommand, MachineOsUpgradeCommand, MachineRmCommand, MachineSetCommand,
    MachineSshCommand, MachineStartCommand, MachineStatusCommand, MachineStopCommand,
    MachineSubcommand,
};
use super::files::{
    load_initialized_machine, load_machine_config_if_exists, load_machine_state_if_exists,
    remove_dir_if_empty, remove_dir_if_exists, remove_machine_runtime_artifacts,
    with_default_machine_lock, with_machine_lock, write_json_file,
};
use super::manager::{
    build_scp_command, build_ssh_command, refresh_machine_state, release_machine_ssh_port,
    start_machine, stop_machine,
};
use super::record::{
    MachineConfigRecord, MachineImageSource, MachineLifecycle, MachinePaths, MachineProvider,
    MachineRootLayout, MachineStateRecord, resolve_runtime_root,
};
use super::render::{
    MachineCommandResult, MachineOsCommandResult, build_machine_info_view,
    build_machine_list_entries, render_machine_action_view, render_machine_info_view,
    render_machine_inspect_view, render_machine_list_view, render_machine_os_apply_view,
    render_machine_os_upgrade_view, render_machine_status_view,
};
use super::{
    DEFAULT_MACHINE_NAME, DEFAULT_NIMBUS_MACHINE_IMAGE_REPOSITORY,
    DEFAULT_PODMAN_MACHINE_IMAGE_DIGEST, DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY,
    default_machine_image_for_provider, default_machine_volumes, describe_machine_image_source,
    invalidate_materialized_machine_os, machine_image_reference_repository,
    machine_image_reference_version_label,
};

pub(crate) async fn run_machine_command(command: MachineCommand) -> Result<(), Error> {
    let roots = resolve_roots_for_command(&command)?;
    run_machine_command_with_layout(command, &roots).await
}

pub(super) fn resolve_roots_for_command(
    command: &MachineCommand,
) -> Result<MachineRootLayout, Error> {
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
        MachineSubcommand::Api(api) => api::run_machine_api_command(api, roots).await,
    }
}

fn machine_record_exists(roots: &MachineRootLayout, machine_name: &str) -> bool {
    roots.paths(machine_name).config_path.exists()
}

pub(super) fn resolve_machine_ssh_target(
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

    emit_machine_stdout(&render_machine_action_view(result, &paths)?)?;
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
        version: super::CURRENT_MACHINE_CONFIG_VERSION,
        name: machine_name,
        provider: MachineProvider::Krunkit,
        guest: super::record::MachineGuestConfig {
            image_source: MachineImageSource::parse(&image)?,
            ssh_user: super::DEFAULT_MACHINE_SSH_USER.to_owned(),
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
    let _output_mode_guard = cli_ux::push_output_mode(output_mode);
    start_machine(&paths, &mut config, &mut state)?;
    let result = if created {
        MachineCommandResult::InitializedAndStarted
    } else {
        MachineCommandResult::Started
    };
    emit_machine_stdout(&render_machine_action_view(result, &paths)?)?;
    Ok(())
}

fn run_machine_stop(command: MachineStopCommand, roots: &MachineRootLayout) -> Result<(), Error> {
    let machine_name = command.name().to_owned();
    let (paths, config, mut state) = load_initialized_machine(roots, &machine_name)?;
    stop_machine(&paths, &config, &mut state)?;
    emit_machine_stdout(&render_machine_action_view(
        MachineCommandResult::Stopped,
        &paths,
    )?)?;
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

    emit_machine_stdout(&render_machine_action_view(
        MachineCommandResult::Updated,
        &paths,
    )?)?;
    Ok(())
}

fn run_machine_rm(command: MachineRmCommand, roots: &MachineRootLayout) -> Result<(), Error> {
    let machine_name = command.name().to_owned();
    let paths = roots.paths(&machine_name);
    let state = load_machine_state_if_exists(&paths.state_path)?;

    if let Some(state) = state.as_ref()
        && matches!(
            state.lifecycle,
            MachineLifecycle::Starting | MachineLifecycle::Running
        )
    {
        return Err(Error::Conflict(format!(
            "machine '{}' is {} and cannot be removed safely.\n{}",
            machine_name,
            state.lifecycle.as_str(),
            cli_ux::format_hint(&format!(
                "run `{}` first, then remove the machine once it is stopped",
                machine_command_with_optional_name("stop", &machine_name)
            ))
        )));
    }

    release_machine_ssh_port(roots, &machine_name)?;
    remove_dir_if_exists(&paths.config_dir)?;
    remove_dir_if_exists(&paths.state_dir)?;
    remove_dir_if_exists(&paths.data_dir)?;
    remove_machine_runtime_artifacts(&paths)?;
    remove_dir_if_empty(&paths.runtime_dir)?;

    emit_machine_stdout(&render_machine_action_view(
        MachineCommandResult::Removed,
        &paths,
    )?)?;
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
    emit_machine_stdout(&render_machine_os_apply_view(
        result,
        &paths,
        &outcome,
        command.restart,
    )?)?;
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
        emit_machine_stdout(&render_machine_os_upgrade_view(
            result,
            &paths,
            &plan,
            command.dry_run,
            false,
            false,
        )?)?;
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
    emit_machine_stdout(&render_machine_os_upgrade_view(
        MachineOsCommandResult::Upgraded,
        &paths,
        &plan,
        false,
        command.restart,
        outcome.restarted,
    )?)?;
    Ok(())
}

fn emit_machine_stdout(rendered: &str) -> Result<(), Error> {
    cli_ux::write_stdout(rendered)
        .map_err(|error| Error::Internal(format!("failed to write machine output: {error}")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MachineOsApplyOutcome {
    pub(super) previous_image: String,
    pub(super) current_image: String,
    pub(super) changed: bool,
    pub(super) restarted: bool,
    pub(super) lifecycle: MachineLifecycle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MachineOsUpgradePlan {
    pub(super) current_image: String,
    pub(super) current_version: String,
    pub(super) target_image: String,
    pub(super) target_version: String,
    pub(super) update_available: bool,
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
            "machine '{}' is starting; wait for startup to finish before applying a machine OS change.\n{}",
            DEFAULT_MACHINE_NAME,
            cli_ux::format_hint("rerun the command after the current start completes")
        )));
    }
    let was_running = matches!(state.lifecycle, MachineLifecycle::Running);
    if was_running && !restart {
        return Err(Error::Conflict(format!(
            "machine '{}' is running; rerun with `--restart` to apply the machine OS change immediately, or stop it first.\n{}",
            DEFAULT_MACHINE_NAME,
            cli_ux::format_hint(&format!(
                "run `{}` to stop the machine before retrying without `--restart`",
                machine_command_with_optional_name("stop", DEFAULT_MACHINE_NAME)
            ))
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

pub(super) fn plan_machine_os_upgrade(
    config: &MachineConfigRecord,
) -> Result<MachineOsUpgradePlan, Error> {
    let reference = current_machine_oci_reference(config)?;
    let stream = default_machine_os_upgrade_stream(config);
    let repository = machine_image_reference_repository(reference.as_str());
    let repository_supported = repository == stream.repository
        || stream
            .additional_supported_repositories
            .contains(&repository.as_str());
    if !repository_supported {
        return Err(Error::InvalidInput(format!(
            "machine os upgrade only supports the default release stream '{}'; current image source is '{}'. Use `nimbus machine os apply <oci-ref-or-digest>` for explicit rollouts instead.",
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
            "configured machine image version {} is newer than the supported machine stream version {}. Install a matching nimbus build or use `nimbus machine os apply <oci-ref-or-digest>` explicitly.",
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
            repository: DEFAULT_NIMBUS_MACHINE_IMAGE_REPOSITORY,
            additional_supported_repositories: &[],
            target_image: default_machine_image_for_provider(config.provider),
            target_version: super::current_machine_release_tag(),
            follows_host_release: true,
        },
    }
}

pub(super) fn parse_machine_os_apply_source(value: &str) -> Result<MachineImageSource, Error> {
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

pub(super) fn machine_command_with_optional_name(subcommand: &str, machine_name: &str) -> String {
    if machine_name == DEFAULT_MACHINE_NAME {
        format!("nimbus machine {subcommand}")
    } else {
        format!("nimbus machine {subcommand} {machine_name}")
    }
}

pub(super) fn current_machine_oci_reference(config: &MachineConfigRecord) -> Result<String, Error> {
    match &config.guest.image_source {
        MachineImageSource::OciReference { reference } => Ok(reference.clone()),
        MachineImageSource::HttpUrl { url } => Err(Error::InvalidInput(format!(
            "machine os upgrade only supports OCI image sources, but this machine uses HTTP override '{}'. Use `nimbus machine os apply <oci-ref-or-digest>` to return to a supported release stream.",
            url
        ))),
        MachineImageSource::LocalDisk { path } => Err(Error::InvalidInput(format!(
            "machine os upgrade only supports OCI image sources, but this machine uses local disk '{}'. Use `nimbus machine os apply <oci-ref-or-digest>` to return to a supported release stream.",
            path.display()
        ))),
    }
}

pub(super) fn split_tagged_machine_image_reference(
    reference: &str,
) -> Result<(String, String), Error> {
    let stripped = reference.trim_start_matches("docker://");
    if stripped.contains('@') {
        return Err(Error::InvalidInput(format!(
            "machine os upgrade requires a tagged OCI reference in the supported release stream, but '{}' is digest-pinned. Use `nimbus machine os apply <oci-ref-or-digest>` for explicit pinned rollouts.",
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
            "machine image reference '{}' is missing a release tag. Use `nimbus machine os apply <oci-ref-or-digest>` for explicit pinned rollouts.",
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

pub(super) fn parse_machine_release_version(tag: &str) -> Result<Version, Error> {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MachineCpEndpoint {
    Host(String),
    Machine { name: String, path: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MachineCpTransfer {
    pub(super) machine_name: String,
    pub(super) machine_path: String,
    pub(super) host_path: String,
    pub(super) guest_is_src: bool,
}

pub(super) fn resolve_machine_cp_transfer(
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

pub(super) fn parse_machine_cp_endpoint(value: &str) -> Result<MachineCpEndpoint, Error> {
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
