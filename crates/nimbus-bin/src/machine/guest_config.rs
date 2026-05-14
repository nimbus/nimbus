use std::fs;
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use nimbus::Error;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use super::DEFAULT_BOOTC_MACHINE_SSH_USER;
use super::command::MachineGuestConfigApplyCommand;
#[cfg(any(unix, test))]
use super::files::write_json_file;
#[cfg(any(unix, test))]
use super::manager::mount_tag;
#[cfg(any(unix, test))]
use super::{MachineConfigRecord, MachinePaths, MachineVolume};

#[cfg(any(unix, test))]
pub(super) const GUEST_MACHINE_CONFIG_MOUNT_TAG: &str = "nimbus-machine-config";
const CURRENT_GUEST_MACHINE_CONFIG_VERSION: u32 = 1;
const GUEST_MACHINE_CONFIG_EVIDENCE_PATH: &str =
    "/var/lib/nimbus/control/machine-config-applied.json";
const GUEST_NIMBUS_CONTROL_DIR: &str = "/var/lib/nimbus/control";
const GUEST_NIMBUS_DATA_DIR: &str = "/var/lib/nimbus/data";
const GUEST_NIMBUS_RUN_DIR: &str = "/run/nimbus";
#[cfg(any(unix, test))]
const VIRTIOFS_SELINUX_CONTEXT: &str = "system_u:object_r:nfs_t:s0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MachineConfigBundle {
    version: u32,
    machine_id: String,
    hostname: String,
    ssh_user: String,
    api_socket: String,
    ready_signal: ReadySignalConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ReadySignalConfig {
    kind: String,
    port: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct VolumeConfigBundle {
    version: u32,
    volumes: Vec<VolumeConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct VolumeConfig {
    source: PathBuf,
    target: PathBuf,
    tag: String,
    readonly: bool,
    selinux_context: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct GuestConfigApplyEvidence {
    version: u32,
    machine_id: String,
    ssh_user: String,
    api_socket: String,
    config_dir: PathBuf,
    config_sha256: String,
    authorized_keys_sha256: String,
    volume_count: usize,
    ready_signal: ReadySignalConfig,
}

struct GuestConfigApplyOptions {
    root: PathBuf,
    run_systemctl: bool,
    send_ready_signal: bool,
}

impl GuestConfigApplyOptions {
    fn production() -> Self {
        Self {
            root: PathBuf::from("/"),
            run_systemctl: true,
            send_ready_signal: true,
        }
    }

    #[cfg(test)]
    fn test(root: PathBuf) -> Self {
        Self {
            root,
            run_systemctl: false,
            send_ready_signal: false,
        }
    }
}

#[cfg(any(unix, test))]
pub(super) fn render_machine_config_bundle(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    ready_vsock_port: u32,
) -> Result<PathBuf, Error> {
    let identity_path = config.guest.ssh_identity_path.as_ref().ok_or_else(|| {
        Error::InvalidInput(format!(
            "machine '{}' uses bootc-native provisioning and requires an SSH identity before rendering its machine-config bundle",
            config.name
        ))
    })?;
    let authorized_keys = read_authorized_key(identity_path, &config.name)?;

    remove_dir_if_exists(&paths.guest_config_bundle_dir)?;
    fs::create_dir_all(&paths.guest_config_bundle_dir).map_err(|error| {
        Error::Internal(format!(
            "failed to create machine-config bundle directory {}: {error}",
            paths.guest_config_bundle_dir.display()
        ))
    })?;
    fs::create_dir_all(paths.guest_config_bundle_dir.join("trust")).map_err(|error| {
        Error::Internal(format!(
            "failed to create machine-config trust directory under {}: {error}",
            paths.guest_config_bundle_dir.display()
        ))
    })?;
    fs::create_dir_all(paths.guest_config_bundle_dir.join("registry-auth")).map_err(|error| {
        Error::Internal(format!(
            "failed to create machine-config registry-auth directory under {}: {error}",
            paths.guest_config_bundle_dir.display()
        ))
    })?;

    let bundle = MachineConfigBundle {
        version: CURRENT_GUEST_MACHINE_CONFIG_VERSION,
        machine_id: config.name.clone(),
        hostname: format!("nimbus-{}", config.name),
        ssh_user: config.guest.ssh_user.clone(),
        api_socket: "/run/nimbus/nimbus.sock".to_owned(),
        ready_signal: ReadySignalConfig {
            kind: "vsock".to_owned(),
            port: ready_vsock_port,
        },
    };
    write_json_file(&paths.guest_config_bundle_dir.join("machine.json"), &bundle)?;
    write_text_file(
        &paths.guest_config_bundle_dir.join("authorized_keys"),
        &(authorized_keys + "\n"),
        0o644,
    )?;
    let volumes = VolumeConfigBundle {
        version: CURRENT_GUEST_MACHINE_CONFIG_VERSION,
        volumes: config.volumes.iter().map(volume_config).collect(),
    };
    write_json_file(
        &paths.guest_config_bundle_dir.join("volumes.json"),
        &volumes,
    )?;

    Ok(paths.guest_config_bundle_dir.clone())
}

pub(super) fn apply_machine_guest_config(
    command: MachineGuestConfigApplyCommand,
) -> Result<(), Error> {
    apply_machine_guest_config_with_options(
        &command.config_dir,
        &GuestConfigApplyOptions::production(),
    )
}

fn apply_machine_guest_config_with_options(
    config_dir: &Path,
    options: &GuestConfigApplyOptions,
) -> Result<(), Error> {
    let bundle = read_machine_config(config_dir)?;
    validate_machine_config(&bundle, config_dir)?;
    let authorized_keys = read_authorized_keys(config_dir)?;
    let volume_bundle = read_volume_config(config_dir)?;
    let config_sha256 = file_sha256(&config_dir.join("machine.json"))?;
    let authorized_keys_sha256 = file_sha256(&config_dir.join("authorized_keys"))?;

    ensure_base_directories(&options.root)?;
    write_guest_text(
        &options.root,
        Path::new("/etc/hostname"),
        &bundle.hostname,
        0o644,
    )?;
    apply_authorized_keys(&options.root, &bundle.ssh_user, &authorized_keys)?;
    apply_volume_units(&options.root, &volume_bundle.volumes, options.run_systemctl)?;

    let evidence = GuestConfigApplyEvidence {
        version: CURRENT_GUEST_MACHINE_CONFIG_VERSION,
        machine_id: bundle.machine_id.clone(),
        ssh_user: bundle.ssh_user.clone(),
        api_socket: bundle.api_socket.clone(),
        config_dir: config_dir.to_path_buf(),
        config_sha256,
        authorized_keys_sha256,
        volume_count: volume_bundle.volumes.len(),
        ready_signal: bundle.ready_signal.clone(),
    };
    write_guest_json(
        &options.root,
        Path::new(GUEST_MACHINE_CONFIG_EVIDENCE_PATH),
        &evidence,
        0o644,
    )?;

    if options.send_ready_signal {
        send_ready_signal(&bundle.ready_signal)?;
    }
    Ok(())
}

#[cfg(any(unix, test))]
fn read_authorized_key(identity_path: &Path, machine_name: &str) -> Result<String, Error> {
    let public_key_path = PathBuf::from(format!("{}.pub", identity_path.display()));
    let raw = fs::read_to_string(&public_key_path).map_err(|error| {
        Error::InvalidInput(format!(
            "machine '{}' SSH public key does not exist at {}: {error}",
            machine_name,
            public_key_path.display()
        ))
    })?;
    let key = raw.trim();
    if key.is_empty() {
        return Err(Error::InvalidInput(format!(
            "machine '{}' SSH public key at {} is empty",
            machine_name,
            public_key_path.display()
        )));
    }
    Ok(key.to_owned())
}

#[cfg(any(unix, test))]
fn volume_config(volume: &MachineVolume) -> VolumeConfig {
    VolumeConfig {
        source: volume.source.clone(),
        target: volume.target.clone(),
        tag: mount_tag(&volume.target),
        readonly: false,
        selinux_context: VIRTIOFS_SELINUX_CONTEXT.to_owned(),
    }
}

fn read_machine_config(config_dir: &Path) -> Result<MachineConfigBundle, Error> {
    read_json_config(&config_dir.join("machine.json"), "machine config")
}

fn read_volume_config(config_dir: &Path) -> Result<VolumeConfigBundle, Error> {
    let path = config_dir.join("volumes.json");
    if !path.exists() {
        return Ok(VolumeConfigBundle {
            version: CURRENT_GUEST_MACHINE_CONFIG_VERSION,
            volumes: Vec::new(),
        });
    }
    let bundle: VolumeConfigBundle = read_json_config(&path, "volume config")?;
    if bundle.version != CURRENT_GUEST_MACHINE_CONFIG_VERSION {
        return Err(Error::InvalidInput(format!(
            "machine volume config at {} uses unsupported version {}; this guest supports version {}",
            path.display(),
            bundle.version,
            CURRENT_GUEST_MACHINE_CONFIG_VERSION
        )));
    }
    for volume in &bundle.volumes {
        if !volume.source.is_absolute() || !volume.target.is_absolute() {
            return Err(Error::InvalidInput(format!(
                "machine volume config at {} contains non-absolute source or target",
                path.display()
            )));
        }
        if volume.tag.trim().is_empty() {
            return Err(Error::InvalidInput(format!(
                "machine volume config at {} contains an empty virtiofs tag",
                path.display()
            )));
        }
    }
    Ok(bundle)
}

fn read_json_config<T>(path: &Path, label: &str) -> Result<T, Error>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = fs::read(path).map_err(|error| {
        Error::InvalidInput(format!(
            "{label} file is missing or unreadable at {}: {error}",
            path.display()
        ))
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        Error::InvalidInput(format!(
            "{label} file at {} is invalid JSON: {error}",
            path.display()
        ))
    })
}

fn validate_machine_config(bundle: &MachineConfigBundle, config_dir: &Path) -> Result<(), Error> {
    if bundle.version != CURRENT_GUEST_MACHINE_CONFIG_VERSION {
        return Err(Error::InvalidInput(format!(
            "machine config at {} uses unsupported version {}; this guest supports version {}",
            config_dir.join("machine.json").display(),
            bundle.version,
            CURRENT_GUEST_MACHINE_CONFIG_VERSION
        )));
    }
    if bundle.machine_id.trim().is_empty() {
        return Err(Error::InvalidInput(
            "machine config machine_id cannot be empty".to_owned(),
        ));
    }
    if bundle.hostname.trim().is_empty() {
        return Err(Error::InvalidInput(
            "machine config hostname cannot be empty".to_owned(),
        ));
    }
    if bundle.ssh_user.trim().is_empty() {
        return Err(Error::InvalidInput(
            "machine config ssh_user cannot be empty".to_owned(),
        ));
    }
    if bundle.api_socket.trim().is_empty() || !Path::new(&bundle.api_socket).is_absolute() {
        return Err(Error::InvalidInput(
            "machine config api_socket must be an absolute path".to_owned(),
        ));
    }
    if bundle.ready_signal.kind != "vsock" {
        return Err(Error::InvalidInput(format!(
            "machine config ready_signal kind '{}' is unsupported; expected 'vsock'",
            bundle.ready_signal.kind
        )));
    }
    if bundle.ready_signal.port == 0 {
        return Err(Error::InvalidInput(
            "machine config ready_signal port must be non-zero".to_owned(),
        ));
    }
    Ok(())
}

fn read_authorized_keys(config_dir: &Path) -> Result<String, Error> {
    let path = config_dir.join("authorized_keys");
    let raw = fs::read_to_string(&path).map_err(|error| {
        Error::InvalidInput(format!(
            "machine authorized_keys file is missing or unreadable at {}: {error}",
            path.display()
        ))
    })?;
    let keys = raw.trim();
    if keys.is_empty() {
        return Err(Error::InvalidInput(format!(
            "machine authorized_keys file at {} is empty",
            path.display()
        )));
    }
    Ok(keys.to_owned() + "\n")
}

fn ensure_base_directories(root: &Path) -> Result<(), Error> {
    for path in [
        GUEST_NIMBUS_CONTROL_DIR,
        GUEST_NIMBUS_DATA_DIR,
        GUEST_NIMBUS_RUN_DIR,
    ] {
        create_guest_dir(root, Path::new(path), 0o755)?;
    }
    Ok(())
}

fn apply_authorized_keys(root: &Path, ssh_user: &str, authorized_keys: &str) -> Result<(), Error> {
    let home =
        home_dir_for_user(root, ssh_user).unwrap_or_else(|| default_home_for_user(root, ssh_user));
    install_authorized_keys(root, ssh_user, &home, authorized_keys)?;
    let root_home = root.join("root");
    install_authorized_keys(root, "root", &root_home, authorized_keys)?;
    Ok(())
}

fn home_dir_for_user(root: &Path, user: &str) -> Option<PathBuf> {
    let passwd = fs::read_to_string(root_join(root, Path::new("/etc/passwd"))).ok()?;
    passwd.lines().find_map(|line| {
        let mut fields = line.split(':');
        let name = fields.next()?;
        if name != user {
            return None;
        }
        let home = fields.nth(4)?;
        Some(root_join(root, Path::new(home)))
    })
}

fn default_home_for_user(root: &Path, user: &str) -> PathBuf {
    if user == DEFAULT_BOOTC_MACHINE_SSH_USER {
        root_join(root, Path::new("/var/lib/nimbus"))
    } else {
        root_join(root, Path::new(&format!("/home/{user}")))
    }
}

fn install_authorized_keys(
    root: &Path,
    user: &str,
    home: &Path,
    authorized_keys: &str,
) -> Result<(), Error> {
    let ssh_dir = home.join(".ssh");
    fs::create_dir_all(&ssh_dir).map_err(|error| {
        Error::Internal(format!(
            "failed to create SSH directory {}: {error}",
            ssh_dir.display()
        ))
    })?;
    set_permissions(&ssh_dir, 0o700)?;
    let authorized_keys_path = ssh_dir.join("authorized_keys");
    write_text_file(&authorized_keys_path, authorized_keys, 0o600)?;
    if root == Path::new("/") {
        run_command(
            "chown",
            &[&format!("{user}:{user}"), &ssh_dir.display().to_string()],
        )?;
        run_command(
            "chown",
            &[
                &format!("{user}:{user}"),
                &authorized_keys_path.display().to_string(),
            ],
        )?;
    }
    Ok(())
}

fn apply_volume_units(
    root: &Path,
    volumes: &[VolumeConfig],
    run_systemctl: bool,
) -> Result<(), Error> {
    if volumes.is_empty() {
        return Ok(());
    }

    with_mutable_root(root, || {
        for volume in volumes {
            create_volume_mountpoint(root, &volume.target, 0o755)?;
            let unit_name = mount_unit_name(&volume.target);
            let unit_path = root_join(
                root,
                Path::new("/etc/systemd/system").join(&unit_name).as_path(),
            );
            write_text_file(&unit_path, &render_volume_mount_unit(volume), 0o644)?;
            ensure_wants_symlink(root, "multi-user.target.wants", &unit_name)?;
        }
        Ok(())
    })?;

    if run_systemctl {
        run_command("systemctl", &["daemon-reload"])?;
        let mut args = vec!["enable", "--now"];
        let units = volumes
            .iter()
            .map(|volume| mount_unit_name(&volume.target))
            .collect::<Vec<_>>();
        args.extend(units.iter().map(String::as_str));
        run_command("systemctl", &args)?;
    }
    Ok(())
}

fn render_volume_mount_unit(volume: &VolumeConfig) -> String {
    let mut options = format!("context=\"{}\"", volume.selinux_context);
    if volume.readonly {
        options.push_str(",ro");
    }
    format!(
        "[Unit]\nDescription=Nimbus host volume {target}\n\n[Mount]\nWhat={tag}\nWhere={target}\nType=virtiofs\nOptions={options}\n\n[Install]\nWantedBy=multi-user.target\n",
        tag = volume.tag,
        target = volume.target.display(),
    )
}

fn ensure_wants_symlink(root: &Path, wants_dir: &str, unit_name: &str) -> Result<(), Error> {
    let wants_path = root_join(
        root,
        Path::new("/etc/systemd/system")
            .join(wants_dir)
            .join(unit_name)
            .as_path(),
    );
    let parent = wants_path.parent().ok_or_else(|| {
        Error::Internal(format!(
            "failed to resolve parent directory for {}",
            wants_path.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        Error::Internal(format!(
            "failed to create systemd wants directory {}: {error}",
            parent.display()
        ))
    })?;
    match fs::remove_file(&wants_path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(Error::Internal(format!(
                "failed to replace systemd wants link {}: {error}",
                wants_path.display()
            )));
        }
    }
    #[cfg(unix)]
    symlink(format!("/etc/systemd/system/{unit_name}"), &wants_path).map_err(|error| {
        Error::Internal(format!(
            "failed to create systemd wants link {}: {error}",
            wants_path.display()
        ))
    })?;
    Ok(())
}

fn send_ready_signal(signal: &ReadySignalConfig) -> Result<(), Error> {
    let mut child = Command::new("socat")
        .arg("-")
        .arg(format!("VSOCK-CONNECT:2:{}", signal.port))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| Error::Internal(format!("failed to start socat ready signal: {error}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(b"Ready").map_err(|error| {
            Error::Internal(format!("failed to write machine ready signal: {error}"))
        })?;
    }
    let output = child.wait_with_output().map_err(|error| {
        Error::Internal(format!("failed waiting for socat ready signal: {error}"))
    })?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(Error::Internal(format!(
        "socat ready signal exited unsuccessfully with status {}{}",
        output.status,
        if stderr.is_empty() {
            String::new()
        } else {
            format!(": {stderr}")
        }
    )))
}

fn write_guest_json<T: Serialize>(
    root: &Path,
    guest_path: &Path,
    value: &T,
    mode: u32,
) -> Result<(), Error> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| {
        Error::Internal(format!(
            "failed to serialize {}: {error}",
            guest_path.display()
        ))
    })?;
    write_guest_bytes(root, guest_path, &bytes, mode)
}

fn write_guest_text(
    root: &Path,
    guest_path: &Path,
    contents: &str,
    mode: u32,
) -> Result<(), Error> {
    write_guest_bytes(root, guest_path, contents.as_bytes(), mode)
}

fn write_guest_bytes(
    root: &Path,
    guest_path: &Path,
    contents: &[u8],
    mode: u32,
) -> Result<(), Error> {
    let path = root_join(root, guest_path);
    write_bytes_file(&path, contents, mode)
}

fn write_text_file(path: &Path, contents: &str, mode: u32) -> Result<(), Error> {
    write_bytes_file(path, contents.as_bytes(), mode)
}

fn write_bytes_file(path: &Path, contents: &[u8], mode: u32) -> Result<(), Error> {
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
    let mut temp = NamedTempFile::new_in(parent).map_err(|error| {
        Error::Internal(format!(
            "failed to create temporary file for {}: {error}",
            path.display()
        ))
    })?;
    temp.write_all(contents).map_err(|error| {
        Error::Internal(format!(
            "failed to write temporary file for {}: {error}",
            path.display()
        ))
    })?;
    temp.flush().map_err(|error| {
        Error::Internal(format!(
            "failed to flush temporary file for {}: {error}",
            path.display()
        ))
    })?;
    set_permissions(temp.path(), mode)?;
    temp.persist(path).map_err(|error| {
        Error::Internal(format!(
            "failed to persist {}: {}",
            path.display(),
            error.error
        ))
    })?;
    Ok(())
}

fn create_guest_dir(root: &Path, guest_path: &Path, mode: u32) -> Result<(), Error> {
    let path = root_join(root, guest_path);
    fs::create_dir_all(&path).map_err(|error| {
        Error::Internal(format!(
            "failed to create guest directory {}: {error}",
            guest_path.display()
        ))
    })?;
    set_permissions(&path, mode)
}

fn create_volume_mountpoint(root: &Path, guest_path: &Path, mode: u32) -> Result<(), Error> {
    let path = root_join(root, guest_path);
    fs::create_dir_all(&path).map_err(|error| {
        Error::Internal(format!(
            "failed to create guest volume mountpoint {}: {error}",
            guest_path.display()
        ))
    })?;
    match set_permissions_io(&path, mode) {
        Ok(()) => Ok(()),
        Err(error) if is_mountpoint_permission_error(&error) => {
            eprintln!(
                "warning: leaving permissions unchanged for host volume mountpoint {}: {error}",
                guest_path.display()
            );
            Ok(())
        }
        Err(error) => Err(permission_error(&path, mode, error)),
    }
}

fn set_permissions(path: &Path, mode: u32) -> Result<(), Error> {
    set_permissions_io(path, mode).map_err(|error| permission_error(path, mode, error))
}

fn permission_error(path: &Path, mode: u32, error: io::Error) -> Error {
    Error::Internal(format!(
        "failed to set permissions {:o} on {}: {error}",
        mode,
        path.display()
    ))
}

fn is_mountpoint_permission_error(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::PermissionDenied || error.raw_os_error() == Some(1)
}

fn set_permissions_io(path: &Path, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

fn with_mutable_root(
    root: &Path,
    operation: impl FnOnce() -> Result<(), Error>,
) -> Result<(), Error> {
    if root == Path::new("/") {
        let _ = Command::new("chattr").arg("-i").arg("/").status();
        let result = operation();
        let _ = Command::new("chattr").arg("+i").arg("/").status();
        return result;
    }
    operation()
}

fn run_command(program: &str, args: &[&str]) -> Result<(), Error> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| Error::Internal(format!("failed to start {program}: {error}")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(Error::Internal(format!(
        "{program} exited unsuccessfully with status {}{}",
        output.status,
        if stderr.is_empty() {
            String::new()
        } else {
            format!(": {stderr}")
        }
    )))
}

fn root_join(root: &Path, guest_path: &Path) -> PathBuf {
    let relative = guest_path.strip_prefix("/").unwrap_or(guest_path);
    root.join(relative)
}

fn mount_unit_name(target: &Path) -> String {
    if target == Path::new("/") {
        return "-.mount".to_owned();
    }

    let escaped = target
        .components()
        .filter_map(|component| match component {
            std::path::Component::RootDir => None,
            _ => Some(
                component
                    .as_os_str()
                    .to_string_lossy()
                    .bytes()
                    .flat_map(|byte| match byte {
                        b'-' => br"\x2d".to_vec(),
                        b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z' | b'_' | b'.' => vec![byte],
                        _ => format!(r"\x{byte:02x}").into_bytes(),
                    })
                    .map(char::from)
                    .collect::<String>(),
            ),
        })
        .collect::<Vec<_>>()
        .join("-");

    format!("{escaped}.mount")
}

fn file_sha256(path: &Path) -> Result<String, Error> {
    let bytes = fs::read(path).map_err(|error| {
        Error::Internal(format!(
            "failed to read {} for SHA-256: {error}",
            path.display()
        ))
    })?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

#[cfg(any(unix, test))]
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

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::machine::{
        CURRENT_MACHINE_CONFIG_VERSION, MachineGuestConfig, MachineGuestProvisioning,
        MachineImageSource, MachineProvider, MachineResources, MachineRootLayout,
    };

    fn sample_config(temp_dir: &TempDir) -> MachineConfigRecord {
        let identity_path = temp_dir.path().join("machine");
        fs::write(&identity_path, "private").expect("private key should write");
        fs::write(
            PathBuf::from(format!("{}.pub", identity_path.display())),
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestKey jack@example\n",
        )
        .expect("public key should write");

        MachineConfigRecord {
            version: CURRENT_MACHINE_CONFIG_VERSION,
            name: "default".to_owned(),
            provider: MachineProvider::Krunkit,
            guest: MachineGuestConfig {
                image_source: MachineImageSource::LocalDisk {
                    path: temp_dir.path().join("disk.raw"),
                },
                provisioning: MachineGuestProvisioning::BootcMachineConfig,
                ssh_user: DEFAULT_BOOTC_MACHINE_SSH_USER.to_owned(),
                ssh_identity_path: Some(identity_path),
                ignition_file_path: None,
                efi_variable_store_path: None,
            },
            resources: MachineResources {
                cpus: 2,
                memory_mib: 2048,
                disk_gib: 20,
            },
            volumes: vec![MachineVolume {
                source: PathBuf::from("/Users"),
                target: PathBuf::from("/Users"),
            }],
            roots: MachineRootLayout::new(
                temp_dir.path().join("config"),
                temp_dir.path().join("state"),
                temp_dir.path().join("runtime"),
            ),
        }
    }

    #[test]
    fn render_machine_config_bundle_writes_versioned_contract() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let config = sample_config(&temp_dir);
        let paths = config.roots.paths("default");

        render_machine_config_bundle(&paths, &config, 1025).expect("bundle should render");

        let machine: serde_json::Value = serde_json::from_slice(
            &fs::read(paths.guest_config_bundle_dir.join("machine.json"))
                .expect("machine config should read"),
        )
        .expect("machine config should parse");
        assert_eq!(machine["version"], CURRENT_GUEST_MACHINE_CONFIG_VERSION);
        assert_eq!(machine["machine_id"], "default");
        assert_eq!(machine["ssh_user"], DEFAULT_BOOTC_MACHINE_SSH_USER);
        assert_eq!(machine["ready_signal"]["kind"], "vsock");
        assert_eq!(machine["ready_signal"]["port"], 1025);
        assert_eq!(
            fs::read_to_string(paths.guest_config_bundle_dir.join("authorized_keys"))
                .expect("authorized_keys should read")
                .trim(),
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestKey jack@example"
        );
        let volumes: serde_json::Value = serde_json::from_slice(
            &fs::read(paths.guest_config_bundle_dir.join("volumes.json"))
                .expect("volume config should read"),
        )
        .expect("volume config should parse");
        assert_eq!(volumes["volumes"][0]["target"], "/Users");
        assert_eq!(volumes["volumes"][0]["tag"], mount_tag(Path::new("/Users")));
    }

    #[test]
    fn guest_apply_rejects_missing_machine_json() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let error = apply_machine_guest_config_with_options(
            temp_dir.path(),
            &GuestConfigApplyOptions::test(temp_dir.path().join("root")),
        )
        .expect_err("missing machine config should fail");
        assert!(error.to_string().contains("machine.json"));
    }

    #[test]
    fn guest_apply_rejects_unsupported_version() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        write_json_file(
            &temp_dir.path().join("machine.json"),
            &json!({
                "version": 99,
                "machine_id": "default",
                "hostname": "nimbus-default",
                "ssh_user": "nimbus",
                "api_socket": "/run/nimbus/nimbus.sock",
                "ready_signal": {"kind": "vsock", "port": 1025}
            }),
        )
        .expect("machine config should write");
        fs::write(
            temp_dir.path().join("authorized_keys"),
            "ssh-ed25519 AAAA test\n",
        )
        .expect("authorized_keys should write");

        let error = apply_machine_guest_config_with_options(
            temp_dir.path(),
            &GuestConfigApplyOptions::test(temp_dir.path().join("root")),
        )
        .expect_err("unsupported version should fail");
        assert!(error.to_string().contains("unsupported version 99"));
    }

    #[test]
    fn guest_apply_installs_keys_volume_units_and_evidence() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let config = sample_config(&temp_dir);
        let paths = config.roots.paths("default");
        let config_dir =
            render_machine_config_bundle(&paths, &config, 1025).expect("bundle should render");
        let root = temp_dir.path().join("guest-root");
        fs::create_dir_all(root.join("etc")).expect("etc should exist");
        fs::write(
            root.join("etc/passwd"),
            "root:x:0:0:root:/root:/bin/bash\nnimbus:x:976:976:Nimbus:/var/lib/nimbus:/bin/bash\n",
        )
        .expect("passwd should write");

        apply_machine_guest_config_with_options(
            &config_dir,
            &GuestConfigApplyOptions::test(root.clone()),
        )
        .expect("guest config should apply");

        assert_eq!(
            fs::read_to_string(root.join("etc/hostname")).expect("hostname should read"),
            "nimbus-default"
        );
        assert!(
            fs::read_to_string(root.join("var/lib/nimbus/.ssh/authorized_keys"))
                .expect("nimbus authorized_keys should read")
                .contains("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestKey")
        );
        assert!(
            fs::read_to_string(root.join("etc/systemd/system/Users.mount"))
                .expect("mount unit should read")
                .contains("Type=virtiofs")
        );
        let evidence: serde_json::Value = serde_json::from_slice(
            &fs::read(root.join("var/lib/nimbus/control/machine-config-applied.json"))
                .expect("evidence should read"),
        )
        .expect("evidence should parse");
        assert_eq!(evidence["machine_id"], "default");
        assert_eq!(evidence["volume_count"], 1);
    }

    #[test]
    fn volume_mountpoint_permission_denied_is_nonfatal() {
        let denied = io::Error::new(io::ErrorKind::PermissionDenied, "operation not permitted");
        assert!(is_mountpoint_permission_error(&denied));

        let eperm = io::Error::from_raw_os_error(1);
        assert!(is_mountpoint_permission_error(&eperm));

        let other = io::Error::new(io::ErrorKind::InvalidInput, "bad mode");
        assert!(!is_mountpoint_permission_error(&other));
    }
}
