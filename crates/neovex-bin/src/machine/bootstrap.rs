use std::fs;
use std::path::{Path, PathBuf};

use neovex::Error;
use serde_json::{Value, json};

use super::manager::mount_tag;
use super::{MachineConfigRecord, MachinePaths, MachineVolume, write_json_file};

const IGNITION_VERSION: &str = "3.2.0";
const CORE_USER: &str = "core";
const GUEST_NEOVEX_DATA_DIR: &str = "/var/lib/neovex";
const GUEST_NEOVEX_CONTROL_DIR: &str = "/var/lib/neovex/control";
const GUEST_NEOVEX_DB_DIR: &str = "/var/lib/neovex/data";
// On FCOS, /usr/local is a writable symlink to /var/usrlocal and carries an
// executable label, unlike /var/lib where systemd will not exec guest-managed binaries.
pub(super) const GUEST_NEOVEX_BIN: &str = "/usr/local/bin/neovex";
pub(super) const GUEST_NEOVEX_SOCKET: &str = "/run/neovex/neovex.sock";
const VIRTIOFS_SELINUX_CONTEXT: &str = "system_u:object_r:nfs_t:s0";
const READY_SERVICE_TEMPLATE: &str = include_str!("assets/ready.service.tmpl");
const NEOVEX_SERVICE_TEMPLATE: &str = include_str!("assets/neovex.service.tmpl");
const NEOVEX_SOCKET_TEMPLATE: &str = include_str!("assets/neovex.socket.tmpl");
const VIRTIOFS_ROOT_OFF_TEMPLATE: &str = include_str!("assets/virtiofs-root-off.service");
const VIRTIOFS_ROOT_ON_TEMPLATE: &str = include_str!("assets/virtiofs-root-on.service");
const VIRTIOFS_MOUNT_TEMPLATE: &str = include_str!("assets/virtiofs-mount.service.tmpl");

pub(super) fn resolve_ignition_file(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    ready_vsock_port: u32,
) -> Result<PathBuf, Error> {
    match &config.guest.ignition_file_path {
        Some(path) => {
            if path.is_file() {
                Ok(path.clone())
            } else {
                Err(Error::InvalidInput(format!(
                    "machine '{}' ignition file does not exist at {}",
                    config.name,
                    path.display()
                )))
            }
        }
        None => render_generated_ignition(paths, config, ready_vsock_port),
    }
}

fn render_generated_ignition(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    ready_vsock_port: u32,
) -> Result<PathBuf, Error> {
    let ignition = generated_ignition_value(config, ready_vsock_port)?;
    write_json_file(&paths.generated_ignition_path, &ignition)?;
    Ok(paths.generated_ignition_path.clone())
}

fn generated_ignition_value(
    config: &MachineConfigRecord,
    ready_vsock_port: u32,
) -> Result<Value, Error> {
    let authorized_keys = resolve_authorized_keys(config)?;
    let mut units = Vec::new();
    if !config.volumes.is_empty() {
        units.push(systemd_unit(
            "immutable-root-off.service",
            true,
            immutable_root_off_unit(),
        ));
        units.extend(config.volumes.iter().enumerate().map(|(index, volume)| {
            systemd_unit(
                &mount_unit_name(&volume.target),
                true,
                virtiofs_mount_unit(index, volume),
            )
        }));
        units.push(systemd_unit(
            "immutable-root-on.service",
            true,
            immutable_root_on_unit(),
        ));
    }
    units.push(systemd_unit(
        "ready.service",
        true,
        ready_signal_unit(ready_vsock_port),
    ));
    units.push(systemd_unit("neovex.socket", true, neovex_socket_unit()));
    units.push(systemd_unit("neovex.service", false, neovex_service_unit()));

    let mut root = json!({
        "ignition": { "version": IGNITION_VERSION },
        "storage": {
            "directories": [
                directory_entry(GUEST_NEOVEX_DATA_DIR, 0o755),
                directory_entry(GUEST_NEOVEX_CONTROL_DIR, 0o755),
                directory_entry(GUEST_NEOVEX_DB_DIR, 0o755),
            ]
        },
        "systemd": { "units": units },
    });
    let users = passwd_users(config, &authorized_keys);
    if !users.is_empty() {
        root["passwd"] = json!({ "users": users });
    }
    Ok(root)
}

fn resolve_authorized_keys(config: &MachineConfigRecord) -> Result<Vec<String>, Error> {
    let Some(identity_path) = config.guest.ssh_identity_path.as_ref() else {
        return Ok(Vec::new());
    };

    let public_key_path = public_key_path(identity_path);
    let raw = fs::read_to_string(&public_key_path).map_err(|error| {
        Error::InvalidInput(format!(
            "machine '{}' SSH public key does not exist at {}: {error}",
            config.name,
            public_key_path.display()
        ))
    })?;
    let key = raw.trim();
    if key.is_empty() {
        return Err(Error::InvalidInput(format!(
            "machine '{}' SSH public key at {} is empty",
            config.name,
            public_key_path.display()
        )));
    }
    Ok(vec![key.to_owned()])
}

fn public_key_path(identity_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.pub", identity_path.display()))
}

fn passwd_users(config: &MachineConfigRecord, authorized_keys: &[String]) -> Vec<Value> {
    if authorized_keys.is_empty() {
        return Vec::new();
    }

    let mut users = vec![json!({
        "name": config.guest.ssh_user,
        "sshAuthorizedKeys": authorized_keys,
    })];
    if config.guest.ssh_user != "root" {
        users.push(json!({
            "name": "root",
            "sshAuthorizedKeys": authorized_keys,
        }));
    }
    if config.guest.ssh_user != CORE_USER {
        users.push(json!({
            "name": CORE_USER,
            "shouldExist": false,
        }));
    }
    users
}

fn systemd_unit(name: &str, enabled: bool, contents: String) -> Value {
    json!({
        "name": name,
        "enabled": enabled,
        "contents": contents,
    })
}

fn directory_entry(path: &str, mode: u32) -> Value {
    json!({
        "path": path,
        "mode": mode,
        "user": { "name": "root" },
        "group": { "name": "root" },
    })
}

fn ready_signal_unit(ready_vsock_port: u32) -> String {
    READY_SERVICE_TEMPLATE.replace("{ready_vsock_port}", &ready_vsock_port.to_string())
}

fn neovex_service_unit() -> String {
    NEOVEX_SERVICE_TEMPLATE
        .replace("{guest_neovex_data_dir}", GUEST_NEOVEX_DATA_DIR)
        .replace("{guest_neovex_bin}", GUEST_NEOVEX_BIN)
        .replace("{guest_neovex_control_dir}", GUEST_NEOVEX_CONTROL_DIR)
}

fn neovex_socket_unit() -> String {
    NEOVEX_SOCKET_TEMPLATE.replace("{guest_neovex_socket}", GUEST_NEOVEX_SOCKET)
}

fn immutable_root_off_unit() -> String {
    VIRTIOFS_ROOT_OFF_TEMPLATE.to_owned()
}

fn immutable_root_on_unit() -> String {
    VIRTIOFS_ROOT_ON_TEMPLATE.to_owned()
}

fn virtiofs_mount_unit(index: usize, volume: &MachineVolume) -> String {
    VIRTIOFS_MOUNT_TEMPLATE
        .replace("{index}", &index.to_string())
        .replace("{target}", &volume.target.display().to_string())
        .replace("{tag}", &mount_tag(&volume.target))
        .replace("{virtiofs_selinux_context}", VIRTIOFS_SELINUX_CONTEXT)
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

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::machine::{
        CURRENT_MACHINE_CONFIG_VERSION, MachineGuestConfig, MachineImageSource, MachineProvider,
        MachineResources, MachineRootLayout,
    };

    fn sample_config(temp_dir: &TempDir) -> MachineConfigRecord {
        MachineConfigRecord {
            version: CURRENT_MACHINE_CONFIG_VERSION,
            name: "default".to_owned(),
            provider: MachineProvider::Krunkit,
            guest: MachineGuestConfig {
                image_source: MachineImageSource::LocalDisk {
                    path: temp_dir.path().join("disk.raw"),
                },
                ssh_user: "core".to_owned(),
                ssh_identity_path: None,
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
    fn generated_ignition_includes_ready_neovex_and_mount_units() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let config = sample_config(&temp_dir);

        let ignition = generated_ignition_value(&config, 1025).expect("ignition should render");
        let units = ignition["systemd"]["units"]
            .as_array()
            .expect("systemd units should render");
        let names = units
            .iter()
            .filter_map(|unit| unit["name"].as_str())
            .collect::<Vec<_>>();

        assert!(names.contains(&"ready.service"));
        assert!(names.contains(&"neovex.socket"));
        assert!(names.contains(&"neovex.service"));
        assert!(names.contains(&"immutable-root-off.service"));
        assert!(names.contains(&"immutable-root-on.service"));
        assert!(names.contains(&"Users.mount"));
        assert!(units.iter().any(|unit| {
            unit["contents"]
                .as_str()
                .is_some_and(|contents| contents.contains("VSOCK-CONNECT:2:1025"))
        }));
        assert!(units.iter().any(|unit| {
            unit["contents"]
                .as_str()
                .is_some_and(|contents| contents.contains("[Mount]"))
        }));
        assert!(units.iter().any(|unit| {
            unit["contents"]
                .as_str()
                .is_some_and(|contents| contents.contains("machine api --socket-activation"))
        }));
        assert!(
            !ignition["storage"]["directories"]
                .as_array()
                .expect("storage directories should render")
                .iter()
                .any(|directory| directory["path"] == "/var/lib/neovex/bin")
        );
    }

    #[test]
    fn generated_ignition_reads_ssh_public_key_when_identity_is_present() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let mut config = sample_config(&temp_dir);
        let identity_path = temp_dir.path().join("machine");
        fs::write(&identity_path, "private").expect("private key should write");
        let public_key_path = PathBuf::from(format!("{}.pub", identity_path.display()));
        fs::write(&public_key_path, "ssh-ed25519 AAAA neovex@test\n")
            .expect("public key should write");
        config.guest.ssh_identity_path = Some(identity_path);

        let ignition = generated_ignition_value(&config, 1025).expect("ignition should render");
        let users = ignition["passwd"]["users"]
            .as_array()
            .expect("passwd users should render");

        assert!(users.iter().any(|user| {
            user["name"] == "core"
                && user["sshAuthorizedKeys"].as_array().is_some_and(|keys| {
                    keys.iter().any(|key| key == "ssh-ed25519 AAAA neovex@test")
                })
        }));
        assert!(users.iter().any(|user| user["name"] == "root"));
    }

    #[test]
    fn resolve_ignition_file_writes_generated_ignition_when_no_override_is_present() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let config = sample_config(&temp_dir);
        let paths = config.roots.paths("default");
        fs::create_dir_all(&paths.config_dir).expect("config dir should exist");

        let ignition_path =
            resolve_ignition_file(&paths, &config, 1025).expect("ignition path should resolve");

        assert_eq!(ignition_path, paths.generated_ignition_path);
        assert!(ignition_path.is_file());
    }
}
