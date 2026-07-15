use super::*;

use crate::machine::record::MachineProvider;

pub(in crate::machine) fn run_machine_os(
    command: MachineOsCommand,
    roots: &MachineRootLayout,
) -> Result<(), Error> {
    match command.command {
        MachineOsSubcommand::Apply(apply) => run_machine_os_apply(apply, roots),
        MachineOsSubcommand::Upgrade(upgrade) => run_machine_os_upgrade(upgrade, roots),
        MachineOsSubcommand::Rollback(rollback) => run_machine_os_rollback(rollback, roots),
    }
}

fn run_machine_os_apply(
    command: MachineOsApplyCommand,
    roots: &MachineRootLayout,
) -> Result<(), Error> {
    let (paths, mut config, mut state) = load_initialized_machine(roots, DEFAULT_MACHINE_NAME)?;
    let target_source = parse_machine_os_apply_source(&command.image)?;
    if uses_bootc_native_os_lifecycle(&config) {
        let outcome = apply_bootc_machine_os_change(
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
        return Ok(());
    }
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
    if uses_bootc_native_os_lifecycle(&config) {
        return run_bootc_machine_os_upgrade(command, &paths, &mut config, &mut state);
    }
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

#[cfg(unix)]
fn run_machine_os_rollback(
    command: MachineOsRollbackCommand,
    roots: &MachineRootLayout,
) -> Result<(), Error> {
    let (paths, mut config, mut state) = load_initialized_machine(roots, DEFAULT_MACHINE_NAME)?;
    if !uses_bootc_native_os_lifecycle(&config) {
        return Err(Error::InvalidInput(
            "machine os rollback is only supported for bootc-native machines".to_owned(),
        ));
    }
    let client = require_running_bootc_machine_api_client(&paths, &state)?;
    let before = client.bootc_status()?;
    let rollback_image = before
        .rollback_image
        .clone()
        .ok_or_else(|| Error::conflict("bootc status does not report a rollback deployment"))?;
    let operation = client.bootc_rollback(MachineApiBootcRollbackRequest {})?;
    if command.restart {
        restart_bootc_machine(&paths, &mut config, &mut state)?;
    }
    let summary = if command.restart {
        format!(
            "Machine \"{}\" machine OS rollback queued to {} and restarted successfully\n",
            paths.name, rollback_image
        )
    } else {
        format!(
            "Machine \"{}\" machine OS rollback queued to {}\n{}",
            paths.name,
            rollback_image,
            cli_ux::format_hint("restart the machine to boot the rollback deployment")
        )
    };
    emit_machine_stdout(&summary)?;
    drop(operation);
    Ok(())
}

#[cfg(not(unix))]
fn run_machine_os_rollback(
    _command: MachineOsRollbackCommand,
    _roots: &MachineRootLayout,
) -> Result<(), Error> {
    Err(unsupported_bootc_machine_os_error())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::machine) struct MachineOsApplyOutcome {
    pub(in crate::machine) previous_image: String,
    pub(in crate::machine) current_image: String,
    pub(in crate::machine) changed: bool,
    pub(in crate::machine) restarted: bool,
    pub(in crate::machine) lifecycle: MachineLifecycle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::machine) struct MachineOsUpgradePlan {
    pub(in crate::machine) current_image: String,
    pub(in crate::machine) current_version: String,
    pub(in crate::machine) target_image: String,
    pub(in crate::machine) target_version: String,
    pub(in crate::machine) update_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MachineOsUpgradeStream {
    repository: &'static str,
    additional_supported_repositories: &'static [&'static str],
    target_image: String,
    target_version: String,
    follows_host_release: bool,
}

fn uses_bootc_native_os_lifecycle(config: &MachineConfigRecord) -> bool {
    matches!(
        config.guest.provisioning,
        MachineGuestProvisioning::BootcMachineConfig
    )
}

#[cfg(unix)]
fn apply_bootc_machine_os_change(
    paths: &MachinePaths,
    config: &mut MachineConfigRecord,
    state: &mut MachineStateRecord,
    target_source: MachineImageSource,
    restart: bool,
) -> Result<MachineOsApplyOutcome, Error> {
    let target_reference = bootc_target_reference_from_source(&target_source)?;
    let client = require_running_bootc_machine_api_client(paths, state)?;
    let before = client.bootc_status()?;
    let previous_image = describe_bootc_status_image(&before);
    if bootc_status_matches_target(&before, &target_reference) {
        return Ok(MachineOsApplyOutcome {
            previous_image,
            current_image: target_reference,
            changed: false,
            restarted: false,
            lifecycle: state.lifecycle,
        });
    }

    let (transport, image) = bootc_switch_target(&target_reference);
    let _operation = client.bootc_switch(MachineApiBootcSwitchRequest {
        image,
        transport: Some(transport),
    })?;

    let restarted = if restart {
        restart_bootc_machine(paths, config, state)?;
        true
    } else {
        false
    };

    Ok(MachineOsApplyOutcome {
        previous_image,
        current_image: target_reference,
        changed: true,
        restarted,
        lifecycle: state.lifecycle,
    })
}

#[cfg(not(unix))]
fn apply_bootc_machine_os_change(
    _paths: &MachinePaths,
    _config: &mut MachineConfigRecord,
    _state: &mut MachineStateRecord,
    _target_source: MachineImageSource,
    _restart: bool,
) -> Result<MachineOsApplyOutcome, Error> {
    Err(unsupported_bootc_machine_os_error())
}

#[cfg(unix)]
fn run_bootc_machine_os_upgrade(
    command: MachineOsUpgradeCommand,
    paths: &MachinePaths,
    config: &mut MachineConfigRecord,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    let client = require_running_bootc_machine_api_client(paths, state)?;
    let before = client.bootc_status()?;
    let stream = default_bootc_machine_os_upgrade_stream();
    let current_image = describe_bootc_status_image(&before);
    let current_version = before
        .booted_digest
        .clone()
        .unwrap_or_else(|| machine_image_reference_version_label(&current_image));
    let update_available = !bootc_status_matches_target(&before, &stream.target_image);
    let plan = MachineOsUpgradePlan {
        current_image,
        current_version,
        target_image: stream.target_image.clone(),
        target_version: stream.target_version.clone(),
        update_available,
    };

    if command.dry_run || !plan.update_available {
        let result = if plan.update_available {
            MachineOsCommandResult::UpgradeCheck
        } else {
            MachineOsCommandResult::AlreadyCurrent
        };
        emit_machine_stdout(&render_machine_os_upgrade_view(
            result,
            paths,
            &plan,
            command.dry_run,
            false,
            false,
        )?)?;
        return Ok(());
    }

    let (transport, image) = bootc_switch_target(&stream.target_image);
    let _operation = client.bootc_switch(MachineApiBootcSwitchRequest {
        image,
        transport: Some(transport),
    })?;
    let restarted = if command.restart {
        restart_bootc_machine(paths, config, state)?;
        true
    } else {
        false
    };
    emit_machine_stdout(&render_machine_os_upgrade_view(
        MachineOsCommandResult::Upgraded,
        paths,
        &plan,
        false,
        command.restart,
        restarted,
    )?)?;
    Ok(())
}

#[cfg(not(unix))]
fn run_bootc_machine_os_upgrade(
    _command: MachineOsUpgradeCommand,
    _paths: &MachinePaths,
    _config: &mut MachineConfigRecord,
    _state: &mut MachineStateRecord,
) -> Result<(), Error> {
    Err(unsupported_bootc_machine_os_error())
}

#[cfg(unix)]
fn default_bootc_machine_os_upgrade_stream() -> MachineOsUpgradeStream {
    MachineOsUpgradeStream {
        repository: DEFAULT_NIMBUS_MACHINE_IMAGE_REPOSITORY,
        additional_supported_repositories: &[],
        target_image: default_machine_image_for_provider(MachineProvider::Krunkit),
        target_version: machine_image_reference_version_label(&default_machine_image_for_provider(
            MachineProvider::Krunkit,
        )),
        follows_host_release: false,
    }
}

#[cfg(unix)]
fn require_running_bootc_machine_api_client(
    paths: &MachinePaths,
    state: &MachineStateRecord,
) -> Result<MachineApiClient, Error> {
    if !matches!(state.lifecycle, MachineLifecycle::Running) {
        return Err(Error::InvalidInput(format!(
            "machine '{}' is {} and bootc-native machine OS changes require the guest machine API; run `nimbus machine start` first",
            paths.name,
            state.lifecycle.as_str()
        )));
    }
    let client = MachineApiClient::new(paths.api_socket_path.clone());
    client.health().map_err(|error| {
        Error::InvalidInput(format!(
            "machine '{}' guest machine API is not reachable at {}: {error}",
            paths.name,
            paths.api_socket_path.display()
        ))
    })?;
    Ok(client)
}

#[cfg(unix)]
fn restart_bootc_machine(
    paths: &MachinePaths,
    config: &mut MachineConfigRecord,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    stop_machine(paths, config, state)?;
    start_machine(paths, config, state)
}

#[cfg(unix)]
fn bootc_target_reference_from_source(source: &MachineImageSource) -> Result<String, Error> {
    match source {
        MachineImageSource::OciReference { reference } => Ok(reference.clone()),
        MachineImageSource::HttpUrl { url, .. } => Err(Error::InvalidInput(format!(
            "bootc-native machine os apply requires an OCI image reference, not HTTP URL '{}'",
            url
        ))),
        MachineImageSource::LocalDisk { path } => Err(Error::InvalidInput(format!(
            "bootc-native machine os apply requires an OCI image reference, not local disk '{}'",
            path.display()
        ))),
    }
}

#[cfg(unix)]
fn bootc_switch_target(reference: &str) -> (String, String) {
    let stripped = reference.trim_start_matches("docker://");
    ("registry".to_owned(), stripped.to_owned())
}

#[cfg(unix)]
fn bootc_status_matches_target(status: &MachineApiBootcStatusResponse, target: &str) -> bool {
    let normalized_target = target.trim_start_matches("docker://");
    if let Some((_, digest)) = normalized_target.rsplit_once('@') {
        return status.booted_digest.as_deref() == Some(digest)
            || status.staged_digest.as_deref() == Some(digest);
    }
    status.booted_image.as_deref() == Some(normalized_target)
        || status.staged_image.as_deref() == Some(normalized_target)
}

#[cfg(unix)]
fn describe_bootc_status_image(status: &MachineApiBootcStatusResponse) -> String {
    match (&status.booted_image, &status.booted_digest) {
        (Some(image), Some(digest)) => format!("docker://{image}@{digest}"),
        (Some(image), None) => format!("docker://{image}"),
        (None, Some(digest)) => digest.clone(),
        (None, None) => "unknown bootc image".to_owned(),
    }
}

#[cfg(not(unix))]
fn unsupported_bootc_machine_os_error() -> Error {
    Error::InvalidInput(
        "bootc-native machine OS lifecycle requires a unix host with the guest machine API"
            .to_owned(),
    )
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
        return Err(Error::conflict(format!(
            "machine '{}' is starting; wait for startup to finish before applying a machine OS change.\n{}",
            DEFAULT_MACHINE_NAME,
            cli_ux::format_hint("rerun the command after the current start completes")
        )));
    }
    let was_running = matches!(state.lifecycle, MachineLifecycle::Running);
    if was_running && !restart {
        return Err(Error::conflict(format!(
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

    let target_uses_bootc_native = uses_nimbus_bootc_machine_image_source(&target_source);
    let target_uses_host_managed = uses_podman_machine_image_source(&target_source);
    config.guest.image_source = target_source;
    if target_uses_bootc_native {
        config.guest.provisioning = MachineGuestProvisioning::BootcMachineConfig;
        config.guest.ssh_user = super::super::DEFAULT_BOOTC_MACHINE_SSH_USER.to_owned();
        config.guest.ignition_file_path = None;
    } else if target_uses_host_managed {
        config.guest.provisioning = MachineGuestProvisioning::Ignition;
        config.guest.ssh_user = super::super::DEFAULT_MACHINE_SSH_USER.to_owned();
    }
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

pub(in crate::machine) fn plan_machine_os_upgrade(
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
    if cfg!(target_os = "macos") && config.provider.uses_managed_applehv_guest() {
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
        return Err(Error::conflict(format!(
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
        provider if provider.uses_managed_applehv_guest() && cfg!(target_os = "macos") => {
            MachineOsUpgradeStream {
                repository: DEFAULT_NIMBUS_MACHINE_IMAGE_REPOSITORY,
                additional_supported_repositories: &[],
                target_image: default_machine_image_for_provider(config.provider),
                target_version: machine_image_reference_version_label(
                    &default_machine_image_for_provider(config.provider),
                ),
                follows_host_release: false,
            }
        }
        MachineProvider::Krunkit | MachineProvider::Vfkit | MachineProvider::Wsl2 => {
            MachineOsUpgradeStream {
                repository: DEFAULT_NIMBUS_MACHINE_IMAGE_REPOSITORY,
                additional_supported_repositories: &[],
                target_image: default_machine_image_for_provider(config.provider),
                target_version: super::super::current_machine_release_tag(),
                follows_host_release: true,
            }
        }
    }
}

pub(in crate::machine) fn parse_machine_os_apply_source(
    value: &str,
) -> Result<MachineImageSource, Error> {
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

pub(in crate::machine) fn current_machine_oci_reference(
    config: &MachineConfigRecord,
) -> Result<String, Error> {
    match &config.guest.image_source {
        MachineImageSource::OciReference { reference } => Ok(reference.clone()),
        MachineImageSource::HttpUrl { url, .. } => Err(Error::InvalidInput(format!(
            "machine os upgrade only supports OCI image sources, but this machine uses HTTP override '{}'. Use `nimbus machine os apply <oci-ref-or-digest>` to return to a supported release stream.",
            url
        ))),
        MachineImageSource::LocalDisk { path } => Err(Error::InvalidInput(format!(
            "machine os upgrade only supports OCI image sources, but this machine uses local disk '{}'. Use `nimbus machine os apply <oci-ref-or-digest>` to return to a supported release stream.",
            path.display()
        ))),
    }
}

pub(in crate::machine) fn split_tagged_machine_image_reference(
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

pub(in crate::machine) fn parse_machine_release_version(tag: &str) -> Result<Version, Error> {
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
