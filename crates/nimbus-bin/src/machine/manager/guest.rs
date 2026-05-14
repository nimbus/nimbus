use std::env;
use std::fs;
use std::io::{self, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use flate2::read::GzDecoder;
use nimbus::Error;
use reqwest::blocking::Client as BlockingClient;
use tempfile::NamedTempFile;

use crate::cli_ux;

use super::super::bootstrap::{GUEST_NIMBUS_BIN, GUEST_NIMBUS_SOCKET};
use super::super::{
    MachineBootstrapMode, MachineConfigRecord, MachinePaths, MachineStateRecord,
    machine_bootstrap_mode,
};
use super::image::{compute_sha256, file_size, run_blocking_in_thread};
use super::readiness::{
    required_child, resolve_machine_api_ready_wait_timeout, wait_for_machine_api_ready,
};
use super::ssh::{run_guest_ssh_shell_capture, stream_guest_file_over_ssh};
use super::{
    DEFAULT_GUEST_NIMBUS_BINARY_ARCHIVE_NAME_ARM64,
    DEFAULT_GUEST_NIMBUS_BINARY_ARCHIVE_NAME_X86_64, DEFAULT_GUEST_NIMBUS_RELEASE_BASE_URL,
    DesiredGuestNimbusBinaryStatus, GUEST_NIMBUS_BINARY_OVERRIDE_ENV,
    GUEST_NIMBUS_RELEASE_BASE_URL_ENV, HTTP_IMAGE_TIMEOUT, LOCAL_GUEST_BINARY_HELP_TEXT,
    ObservedGuestNimbusBinaryStatus, StartupSignalMonitor,
};

pub(super) fn ensure_machine_bootstrap_identity(
    paths: &MachinePaths,
    config: &mut MachineConfigRecord,
) -> Result<(), Error> {
    let requires_identity =
        requires_host_guest_nimbus_sync(config) || requires_bootc_machine_config(config);
    if !requires_identity || config.guest.ssh_identity_path.is_some() {
        return Ok(());
    }

    super::emit_machine_progress("Generating machine SSH identity");
    let identity_path = paths.data_dir.join("machine");
    ensure_machine_ssh_keypair(&identity_path)?;
    config.guest.ssh_identity_path = Some(identity_path);
    super::write_json_file(&paths.config_path, config)?;
    Ok(())
}

fn ensure_machine_ssh_keypair(identity_path: &Path) -> Result<(), Error> {
    let public_key_path = PathBuf::from(format!("{}.pub", identity_path.display()));
    if identity_path.is_file() {
        if public_key_path.is_file() {
            return Ok(());
        }
        return Err(Error::InvalidInput(format!(
            "machine SSH identity exists at {}, but the public key is missing at {}",
            identity_path.display(),
            public_key_path.display()
        )));
    }

    let parent = identity_path.parent().ok_or_else(|| {
        Error::Internal(format!(
            "machine SSH identity path {} has no parent directory",
            identity_path.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        Error::Internal(format!(
            "failed to create machine SSH identity directory {}: {error}",
            parent.display()
        ))
    })?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|error| {
        Error::Internal(format!(
            "failed to set machine SSH identity directory permissions on {}: {error}",
            parent.display()
        ))
    })?;

    let output = Command::new("ssh-keygen")
        .arg("-N")
        .arg("")
        .arg("-t")
        .arg("ed25519")
        .arg("-f")
        .arg(identity_path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| {
            Error::Internal(format!(
                "failed to start ssh-keygen for machine identity {}: {error}",
                identity_path.display()
            ))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let detail = if stderr.is_empty() {
            format!("exit status {}", output.status)
        } else {
            stderr
        };
        return Err(Error::Internal(format!(
            "failed to generate machine SSH identity at {}: {}",
            identity_path.display(),
            detail
        )));
    }

    if !public_key_path.is_file() {
        return Err(Error::Internal(format!(
            "ssh-keygen completed, but the machine public key is missing at {}",
            public_key_path.display()
        )));
    }

    Ok(())
}

pub(super) fn converge_machine_image_contract(
    paths: &MachinePaths,
    config: &mut MachineConfigRecord,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    let desired_image_source = super::super::desired_machine_image_source(config);
    if config.guest.image_source != desired_image_source {
        config.guest.image_source = desired_image_source;
        super::write_json_file(&paths.config_path, config)?;
    }

    let desired_image = super::super::describe_machine_image_source(&config.guest.image_source);
    let Some(rebuild_reason) = machine_image_rebuild_reason(paths, state, &desired_image) else {
        return Ok(());
    };

    super::super::invalidate_materialized_machine_os(paths)?;
    *state = MachineStateRecord::rebuilt(rebuild_reason);
    super::write_json_file(&paths.state_path, state)?;
    Ok(())
}

pub(super) fn machine_image_rebuild_reason(
    paths: &MachinePaths,
    state: &MachineStateRecord,
    desired_image: &str,
) -> Option<String> {
    let current_boot_artifacts_exist =
        paths.materialized_image_path.is_file() || paths.efi_variable_store_path.exists();
    match state
        .runtime
        .as_ref()
        .map(|runtime| runtime.machine_image_source.as_str())
        .filter(|recorded| !recorded.is_empty())
    {
        Some(recorded) if recorded != desired_image => Some(format!(
            "machine base image changed from '{}' to '{}'; boot artifacts were reset and will be recreated on the next start",
            recorded, desired_image
        )),
        Some(_) => None,
        None if current_boot_artifacts_exist => Some(format!(
            "machine boot artifacts existed without a recorded base-image identity for '{}'; boot artifacts were reset and will be recreated on the next start",
            desired_image
        )),
        None => None,
    }
}

pub(super) fn validate_machine_bootstrap_contract(
    config: &MachineConfigRecord,
) -> Result<(), Error> {
    let requires_host_sync = requires_host_guest_nimbus_sync(config);
    let requires_bootc_config = requires_bootc_machine_config(config);
    if !requires_host_sync && !requires_bootc_config {
        return Ok(());
    }

    if requires_bootc_config && config.guest.ignition_file_path.is_some() {
        return Err(Error::InvalidInput(format!(
            "machine '{}' uses bootc-native machine-config provisioning and cannot also use an Ignition file",
            config.name
        )));
    }

    let identity_path = config.guest.ssh_identity_path.as_ref().ok_or_else(|| {
        let contract = if requires_bootc_config {
            "bootc-native machine-config provisioning"
        } else {
            "the host-managed macOS machine-image contract"
        };
        Error::InvalidInput(format!(
            "machine '{}' uses {contract} and requires `--identity <path>` or a generated machine identity so nimbus can validate the forwarded machine API",
            config.name
        ))
    })?;
    if !identity_path.is_file() {
        return Err(Error::InvalidInput(format!(
            "machine '{}' SSH identity does not exist at {}",
            config.name,
            identity_path.display()
        )));
    }

    Ok(())
}

pub(super) fn requires_host_guest_nimbus_sync(config: &MachineConfigRecord) -> bool {
    config.provider == super::super::MachineProvider::Krunkit
        && super::super::uses_host_managed_machine_image_contract(config)
}

pub(super) fn requires_bootc_machine_config(config: &MachineConfigRecord) -> bool {
    config.provider == super::super::MachineProvider::Krunkit
        && machine_bootstrap_mode(config) == MachineBootstrapMode::BootcMachineConfig
}

pub(super) fn ensure_guest_machine_api_ready(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    ssh_port: u16,
    krunkit_child: &mut Option<Child>,
    gvproxy_child: &mut Option<Child>,
    startup_signals: &StartupSignalMonitor,
) -> Result<(), Error> {
    if config.provider != super::super::MachineProvider::Krunkit
        || config.guest.ssh_identity_path.is_none()
    {
        return Ok(());
    }

    if requires_host_guest_nimbus_sync(config) {
        sync_guest_nimbus_binary(paths, config, ssh_port)?;
    }

    super::emit_machine_progress("Waiting for forwarded machine API");
    wait_for_machine_api_ready(
        paths,
        resolve_machine_api_ready_wait_timeout(),
        required_child(krunkit_child, "krunkit")?,
        required_child(gvproxy_child, "gvproxy")?,
        startup_signals,
    )
}

fn sync_guest_nimbus_binary(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    ssh_port: u16,
) -> Result<(), Error> {
    super::emit_machine_progress("Ensuring guest nimbus binary");
    let guest_binary = resolve_guest_nimbus_binary(paths)?;
    let desired_hash = compute_sha256(&guest_binary)?;
    let current_hash = read_guest_nimbus_hash(config, ssh_port)?;
    if current_hash.as_deref() != Some(desired_hash.as_str()) {
        super::emit_machine_progress("Updating guest nimbus binary inside the machine");
        stream_guest_file_over_ssh(
            config,
            ssh_port,
            &guest_binary,
            &format!(
                "set -eu; install_dir=\"{}\"; tmp_name=\".nimbus.$$.tmp\"; sudo mkdir -p \"$install_dir\"; tmp_path=\"$install_dir/$tmp_name\"; cat | sudo tee \"$tmp_path\" >/dev/null; sudo chmod 0755 \"$tmp_path\"; if command -v restorecon >/dev/null 2>&1; then sudo restorecon \"$tmp_path\"; fi; sudo mv \"$tmp_path\" \"{}\"; if command -v restorecon >/dev/null 2>&1; then sudo restorecon \"{}\"; fi",
                Path::new(GUEST_NIMBUS_BIN)
                    .parent()
                    .expect("guest nimbus binary path should have a parent")
                    .display(),
                GUEST_NIMBUS_BIN,
                GUEST_NIMBUS_BIN
            ),
        )?;
    }

    run_guest_ssh_shell_capture(config, ssh_port, &ensure_guest_nimbus_socket_shell_script())?;
    Ok(())
}

pub(super) fn ensure_guest_nimbus_socket_shell_script() -> String {
    format!(
        "set -eu; sudo systemctl daemon-reload; sudo systemctl stop nimbus.service nimbus.socket >/dev/null 2>&1 || true; sudo systemctl reset-failed nimbus.service nimbus.socket >/dev/null 2>&1 || true; sudo rm -f \"{socket}\"; sudo systemctl enable nimbus.socket >/dev/null 2>&1 || true; sudo systemctl start nimbus.socket; sudo systemctl is-active nimbus.socket >/dev/null; printf '%s' ok",
        socket = GUEST_NIMBUS_SOCKET
    )
}

fn read_guest_nimbus_hash(
    config: &MachineConfigRecord,
    ssh_port: u16,
) -> Result<Option<String>, Error> {
    let output = run_guest_ssh_shell_capture(
        config,
        ssh_port,
        &format!(
            "if [ -x \"{path}\" ]; then set -- $(sha256sum \"{path}\"); printf '%s' \"$1\"; fi",
            path = GUEST_NIMBUS_BIN
        ),
    )?;
    let hash = output.trim();
    if hash.is_empty() {
        Ok(None)
    } else {
        Ok(Some(hash.to_owned()))
    }
}

fn read_guest_nimbus_version(
    config: &MachineConfigRecord,
    ssh_port: u16,
) -> Result<Option<String>, Error> {
    let output = run_guest_ssh_shell_capture(
        config,
        ssh_port,
        &format!(
            "if [ -x \"{path}\" ]; then \"{path}\" --version | head -n1; fi",
            path = GUEST_NIMBUS_BIN
        ),
    )?;
    let version = output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned);
    Ok(version)
}

pub(super) fn inspect_desired_guest_nimbus_binary(
    paths: &MachinePaths,
) -> DesiredGuestNimbusBinaryStatus {
    if let Some(path) = env::var_os(GUEST_NIMBUS_BINARY_OVERRIDE_ENV).map(PathBuf::from) {
        let desired_exists = path.is_file();
        let desired_hash = desired_exists
            .then(|| compute_sha256(&path))
            .transpose()
            .ok()
            .flatten();
        let error = (!desired_exists).then(|| {
            format!(
                "guest nimbus binary override {} from ${GUEST_NIMBUS_BINARY_OVERRIDE_ENV} does not exist",
                path.display()
            )
        });
        return DesiredGuestNimbusBinaryStatus {
            install_path: PathBuf::from(GUEST_NIMBUS_BIN),
            source: super::GuestNimbusBinarySourceKind::ExplicitOverride,
            source_detail: format!("${GUEST_NIMBUS_BINARY_OVERRIDE_ENV}={}", path.display()),
            desired_path: path,
            desired_exists,
            desired_version: None,
            desired_hash,
            release_archive_path: None,
            release_archive_exists: None,
            release_url: None,
            error,
        };
    }

    let release_tag = super::super::current_machine_release_tag();
    match guest_nimbus_archive_name() {
        Ok(archive_name) => {
            let desired_path = paths.guest_binary_cache_dir.join(format!(
                "{}-{}-nimbus",
                release_tag,
                archive_name.trim_end_matches(".tar.gz")
            ));
            let desired_exists = desired_path.is_file();
            let desired_hash = desired_exists
                .then(|| compute_sha256(&desired_path))
                .transpose()
                .ok()
                .flatten();
            let release_archive_path = paths
                .guest_binary_cache_dir
                .join(format!("{release_tag}-{archive_name}"));
            DesiredGuestNimbusBinaryStatus {
                install_path: PathBuf::from(GUEST_NIMBUS_BIN),
                source: super::GuestNimbusBinarySourceKind::ReleaseAsset,
                source_detail: format!("GitHub release asset {}", release_tag),
                desired_path,
                desired_exists,
                desired_version: Some(release_tag.clone()),
                desired_hash,
                release_archive_exists: Some(release_archive_path.is_file()),
                release_url: Some(guest_nimbus_release_url(&release_tag, archive_name)),
                release_archive_path: Some(release_archive_path),
                error: None,
            }
        }
        Err(error) => DesiredGuestNimbusBinaryStatus {
            install_path: PathBuf::from(GUEST_NIMBUS_BIN),
            source: super::GuestNimbusBinarySourceKind::ReleaseAsset,
            source_detail: format!("GitHub release asset {}", release_tag),
            desired_path: paths
                .guest_binary_cache_dir
                .join("unsupported-host-arch-nimbus"),
            desired_exists: false,
            desired_version: Some(release_tag),
            desired_hash: None,
            release_archive_path: None,
            release_archive_exists: None,
            release_url: None,
            error: Some(error.to_string()),
        },
    }
}

pub(super) fn inspect_observed_guest_nimbus_binary(
    config: &MachineConfigRecord,
    state: &MachineStateRecord,
) -> Result<ObservedGuestNimbusBinaryStatus, Error> {
    let runtime = state.runtime.as_ref().ok_or_else(|| {
        Error::InvalidInput(format!(
            "machine '{}' has no recorded runtime; start it first",
            config.name
        ))
    })?;
    Ok(ObservedGuestNimbusBinaryStatus {
        version: read_guest_nimbus_version(config, runtime.ssh_port)?,
        hash: read_guest_nimbus_hash(config, runtime.ssh_port)?,
    })
}

pub(super) fn resolve_guest_nimbus_binary(paths: &MachinePaths) -> Result<PathBuf, Error> {
    if let Some(path) = env::var_os(GUEST_NIMBUS_BINARY_OVERRIDE_ENV).map(PathBuf::from) {
        if !path.is_file() {
            return Err(Error::InvalidInput(format!(
                "guest nimbus binary override {} from ${GUEST_NIMBUS_BINARY_OVERRIDE_ENV} does not exist",
                path.display()
            )));
        }
        return Ok(path);
    }

    let cache_dir = paths.guest_binary_cache_dir.clone();
    fs::create_dir_all(&cache_dir).map_err(|error| {
        Error::Internal(format!(
            "failed to create guest nimbus cache directory {}: {error}",
            cache_dir.display()
        ))
    })?;

    let release_tag = super::super::current_machine_release_tag();
    let archive_name = guest_nimbus_archive_name()?;
    let binary_path = cache_dir.join(format!(
        "{}-{}-nimbus",
        release_tag,
        archive_name.trim_end_matches(".tar.gz")
    ));
    if binary_path.is_file() {
        return Ok(binary_path);
    }

    let archive_path = cache_dir.join(format!("{release_tag}-{archive_name}"));
    if !archive_path.is_file() {
        let download_url = guest_nimbus_release_url(&release_tag, archive_name);
        download_guest_nimbus_archive(
            &archive_path,
            &download_url,
            &format!("Downloading guest nimbus {release_tag}"),
        )?;
    }
    extract_guest_nimbus_archive(
        &archive_path,
        &binary_path,
        &format!("Extracting guest nimbus {release_tag}"),
    )?;
    Ok(binary_path)
}

pub(super) fn guest_nimbus_archive_name() -> Result<&'static str, Error> {
    match env::consts::ARCH {
        "aarch64" | "arm64" => Ok(DEFAULT_GUEST_NIMBUS_BINARY_ARCHIVE_NAME_ARM64),
        "x86_64" => Ok(DEFAULT_GUEST_NIMBUS_BINARY_ARCHIVE_NAME_X86_64),
        arch => Err(Error::InvalidInput(format!(
            "unsupported macOS machine host architecture '{arch}' for guest nimbus binary sync"
        ))),
    }
}

fn guest_nimbus_release_url(release_tag: &str, archive_name: &str) -> String {
    let base = env::var(GUEST_NIMBUS_RELEASE_BASE_URL_ENV)
        .unwrap_or_else(|_| DEFAULT_GUEST_NIMBUS_RELEASE_BASE_URL.to_owned());
    format!("{}/{}", base.trim_end_matches('/'), release_tag).to_owned() + "/" + archive_name
}

fn download_guest_nimbus_archive(
    destination: &Path,
    url: &str,
    progress_message: &str,
) -> Result<(), Error> {
    let destination = destination.to_path_buf();
    let url = url.to_owned();
    let progress_message = progress_message.to_owned();
    run_blocking_in_thread("guest nimbus archive download", move || {
        let parent = destination.parent().ok_or_else(|| {
            Error::Internal(format!(
                "failed to resolve parent directory for guest nimbus archive {}",
                destination.display()
            ))
        })?;
        let mut temp = NamedTempFile::new_in(parent).map_err(|error| {
            Error::Internal(format!(
                "failed to create temporary guest nimbus archive under {}: {error}",
                parent.display()
            ))
        })?;
        let client = BlockingClient::builder()
            .timeout(HTTP_IMAGE_TIMEOUT)
            .build()
            .map_err(|error| Error::Internal(format!("failed to build HTTP client: {error}")))?;
        let response = client
            .get(&url)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|error| {
                Error::InvalidInput(format!(
                    "failed to download guest nimbus archive from {url}: {error}. To continue without the release asset, {LOCAL_GUEST_BINARY_HELP_TEXT} to a local Linux guest binary."
                ))
            })?;
        let mut progress = cli_ux::ByteProgress::new(progress_message, response.content_length())
            .map_err(|error| {
            Error::Internal(format!("failed to initialize progress output: {error}"))
        })?;
        let mut reader = progress.wrap_read(response);
        io::copy(&mut reader, &mut temp).map_err(|error| {
            Error::Internal(format!(
                "failed to write guest nimbus archive from {url} into {}: {error}",
                destination.display()
            ))
        })?;
        progress.finish();
        temp.flush().map_err(|error| {
            Error::Internal(format!(
                "failed to flush guest nimbus archive from {url}: {error}"
            ))
        })?;
        temp.persist(&destination).map_err(|error| {
            Error::Internal(format!(
                "failed to persist guest nimbus archive {}: {}",
                destination.display(),
                error.error
            ))
        })?;
        Ok(())
    })
}

fn extract_guest_nimbus_archive(
    archive_path: &Path,
    output_path: &Path,
    progress_message: &str,
) -> Result<(), Error> {
    let parent = output_path.parent().ok_or_else(|| {
        Error::Internal(format!(
            "failed to resolve parent directory for guest nimbus binary {}",
            output_path.display()
        ))
    })?;
    let temp_output = NamedTempFile::new_in(parent).map_err(|error| {
        Error::Internal(format!(
            "failed to create temporary guest nimbus binary under {}: {error}",
            parent.display()
        ))
    })?;
    let archive_path = archive_path.to_path_buf();
    let temp_output_path = temp_output.path().to_path_buf();
    let progress_message = progress_message.to_owned();
    run_blocking_in_thread("guest nimbus archive extraction", move || {
        let input = fs::File::open(&archive_path).map_err(|error| {
            Error::Internal(format!(
                "failed to open guest nimbus archive {}: {error}",
                archive_path.display()
            ))
        })?;
        let mut progress = cli_ux::ByteProgress::new(
            progress_message,
            Some(file_size(&archive_path).map_err(|error| {
                Error::Internal(format!(
                    "failed to determine guest nimbus archive size {}: {error}",
                    archive_path.display()
                ))
            })?),
        )
        .map_err(|error| {
            Error::Internal(format!("failed to initialize progress output: {error}"))
        })?;
        let reader = progress.wrap_read(BufReader::new(input));
        let decoder = GzDecoder::new(reader);
        let mut archive = tar::Archive::new(decoder);

        let mut entry_found = false;
        for entry in archive.entries().map_err(|error| {
            Error::Internal(format!(
                "failed to read guest nimbus archive {}: {error}",
                archive_path.display()
            ))
        })? {
            let mut entry = entry.map_err(|error| {
                Error::Internal(format!(
                    "failed to read an entry from guest nimbus archive {}: {error}",
                    archive_path.display()
                ))
            })?;
            let entry_path = entry.path().map_err(|error| {
                Error::Internal(format!(
                    "failed to resolve an entry path from guest nimbus archive {}: {error}",
                    archive_path.display()
                ))
            })?;
            if entry_path.as_ref() != Path::new("nimbus") {
                continue;
            }

            let mut output = fs::File::create(&temp_output_path).map_err(|error| {
                Error::Internal(format!(
                    "failed to stage extracted guest nimbus binary {}: {error}",
                    temp_output_path.display()
                ))
            })?;
            io::copy(&mut entry, &mut output).map_err(|error| {
                Error::Internal(format!(
                    "failed to extract guest nimbus binary from {}: {error}",
                    archive_path.display()
                ))
            })?;
            output.flush().map_err(|error| {
                Error::Internal(format!(
                    "failed to flush staged guest nimbus binary {}: {error}",
                    temp_output_path.display()
                ))
            })?;
            entry_found = true;
            break;
        }

        progress.finish();

        if !entry_found {
            return Err(Error::Internal(format!(
                "guest nimbus archive {} did not contain a top-level 'nimbus' binary",
                archive_path.display()
            )));
        }

        Ok(())
    })?;
    fs::set_permissions(temp_output.path(), fs::Permissions::from_mode(0o755)).map_err(
        |error| {
            Error::Internal(format!(
                "failed to mark extracted guest nimbus binary {} executable: {error}",
                temp_output.path().display()
            ))
        },
    )?;
    temp_output.persist(output_path).map_err(|error| {
        Error::Internal(format!(
            "failed to persist guest nimbus binary {}: {}",
            output_path.display(),
            error.error
        ))
    })?;
    Ok(())
}
