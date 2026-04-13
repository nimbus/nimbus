use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Result, SandboxError};
use crate::spec::{SandboxPortBinding, SandboxProcessSpec, SandboxSpec};

const DEFAULT_PATH_ENV: &str = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct KrunBundleLayout {
    pub bundle_dir: PathBuf,
    pub config_path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct KrunBundleOptions {
    pub process_user: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProcessUser {
    uid: u32,
    gid: u32,
}

impl ProcessUser {
    const ROOT: Self = Self { uid: 0, gid: 0 };
}

impl KrunBundleLayout {
    pub(crate) fn new(bundle_dir: impl Into<PathBuf>) -> Self {
        let bundle_dir = bundle_dir.into();
        Self {
            config_path: bundle_dir.join("config.json"),
            bundle_dir,
        }
    }
}

pub(crate) fn write_bundle_config(
    layout: &KrunBundleLayout,
    hostname: &str,
    spec: &SandboxSpec,
    options: &KrunBundleOptions,
) -> Result<()> {
    std::fs::create_dir_all(&layout.bundle_dir).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to create krun bundle directory {}: {error}",
            layout.bundle_dir.display()
        ),
    })?;

    let config = build_bundle_config(hostname, spec, options)?;
    let rendered =
        serde_json::to_vec_pretty(&config).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to serialize krun bundle config: {error}"),
        })?;

    std::fs::write(&layout.config_path, rendered).map_err(|error| {
        SandboxError::OperationFailed {
            message: format!(
                "failed to write krun bundle config {}: {error}",
                layout.config_path.display()
            ),
        }
    })?;

    Ok(())
}

pub(crate) fn build_bundle_config(
    hostname: &str,
    spec: &SandboxSpec,
    options: &KrunBundleOptions,
) -> Result<Value> {
    if spec.process.args.is_empty() {
        return Err(SandboxError::InvalidSpec {
            message: "sandbox process args cannot be empty".to_owned(),
        });
    }

    if spec.process.terminal {
        return Err(SandboxError::InvalidSpec {
            message: "krun service-mode sandboxes require process.terminal = false".to_owned(),
        });
    }

    validate_port_bindings(&spec.port_bindings)?;
    let process_user =
        resolve_process_user(&spec.filesystem.rootfs, options.process_user.as_deref())?;

    let mut annotations = serde_json::Map::new();
    annotations.insert(
        "run.oci.handler".to_owned(),
        Value::String("krun".to_owned()),
    );
    if !spec.port_bindings.is_empty() {
        annotations.insert(
            "krun.port_map".to_owned(),
            Value::String(format_port_map(&spec.port_bindings)),
        );
    }

    Ok(json!({
        "ociVersion": "1.0.2",
        "process": {
            "terminal": false,
            "user": {
                "uid": process_user.uid,
                "gid": process_user.gid,
            },
            "args": spec.process.args,
            "env": process_env(&spec.process),
            "cwd": process_cwd(&spec.process),
        },
        "root": {
            "path": spec.filesystem.rootfs.to_string_lossy(),
            "readonly": spec.filesystem.readonly,
        },
        "hostname": hostname,
        "mounts": default_linux_mounts(),
        "annotations": annotations,
        "linux": {
            "namespaces": [
                { "type": "mount" },
                { "type": "uts" },
                { "type": "ipc" },
                { "type": "pid" },
            ],
        },
    }))
}

fn resolve_process_user(rootfs: &Path, configured_user: Option<&str>) -> Result<ProcessUser> {
    let Some(configured_user) = configured_user
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(ProcessUser::ROOT);
    };

    let (user_part, group_part) = split_user_spec(configured_user)?;
    let resolved_user = resolve_user_part(rootfs, configured_user, user_part)?;
    let gid = match group_part {
        Some(group_part) => resolve_group_part(rootfs, configured_user, group_part)?,
        None => default_group_for_user(rootfs, &resolved_user)?,
    };

    Ok(ProcessUser {
        uid: resolved_user.uid(),
        gid,
    })
}

fn split_user_spec(configured_user: &str) -> Result<(&str, Option<&str>)> {
    let mut parts = configured_user.split(':');
    let user_part = parts.next().unwrap_or_default().trim();
    let group_part = parts.next().map(str::trim);
    if parts.next().is_some() {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "image user {configured_user:?} must be USER or USER:GROUP, not multiple ':' segments"
            ),
        });
    }
    if user_part.is_empty() {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "image user {configured_user:?} must include a non-empty user segment"
            ),
        });
    }
    if matches!(group_part, Some("")) {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "image user {configured_user:?} must include a non-empty group segment"
            ),
        });
    }
    Ok((user_part, group_part))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PasswdEntry {
    name: String,
    uid: u32,
    gid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GroupEntry {
    name: String,
    gid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResolvedUser {
    Numeric(u32),
    Passwd(PasswdEntry),
}

impl ResolvedUser {
    fn uid(&self) -> u32 {
        match self {
            Self::Numeric(uid) => *uid,
            Self::Passwd(entry) => entry.uid,
        }
    }
}

fn resolve_user_part(
    rootfs: &Path,
    configured_user: &str,
    user_part: &str,
) -> Result<ResolvedUser> {
    if let Ok(uid) = user_part.parse::<u32>() {
        return Ok(ResolvedUser::Numeric(uid));
    }

    let entry = read_passwd_entries(rootfs)?
        .into_iter()
        .find(|entry| entry.name == user_part)
        .ok_or_else(|| SandboxError::InvalidSpec {
            message: format!(
                "image user {configured_user:?} references user {user_part:?} that was not found in {}/etc/passwd",
                rootfs.display()
            ),
        })?;
    Ok(ResolvedUser::Passwd(entry))
}

fn resolve_group_part(rootfs: &Path, configured_user: &str, group_part: &str) -> Result<u32> {
    if let Ok(gid) = group_part.parse::<u32>() {
        return Ok(gid);
    }

    read_group_entries(rootfs)?
        .into_iter()
        .find(|entry| entry.name == group_part)
        .map(|entry| entry.gid)
        .ok_or_else(|| SandboxError::InvalidSpec {
            message: format!(
                "image user {configured_user:?} references group {group_part:?} that was not found in {}/etc/group",
                rootfs.display()
            ),
        })
}

fn default_group_for_user(rootfs: &Path, user: &ResolvedUser) -> Result<u32> {
    match user {
        ResolvedUser::Passwd(entry) => Ok(entry.gid),
        ResolvedUser::Numeric(uid) => Ok(read_passwd_entries_if_present(rootfs)?
            .and_then(|entries| entries.into_iter().find(|entry| entry.uid == *uid))
            .map(|entry| entry.gid)
            .unwrap_or(0)),
    }
}

fn read_passwd_entries(rootfs: &Path) -> Result<Vec<PasswdEntry>> {
    let passwd_path = rootfs.join("etc/passwd");
    let contents =
        std::fs::read_to_string(&passwd_path).map_err(|error| SandboxError::InvalidSpec {
            message: format!(
                "failed to read image passwd database {}: {error}",
                passwd_path.display()
            ),
        })?;

    contents
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(parse_passwd_entry)
        .collect()
}

fn read_passwd_entries_if_present(rootfs: &Path) -> Result<Option<Vec<PasswdEntry>>> {
    let passwd_path = rootfs.join("etc/passwd");
    match std::fs::read_to_string(&passwd_path) {
        Ok(contents) => contents
            .lines()
            .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
            .map(parse_passwd_entry)
            .collect::<Result<Vec<_>>>()
            .map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(SandboxError::InvalidSpec {
            message: format!(
                "failed to read image passwd database {}: {error}",
                passwd_path.display()
            ),
        }),
    }
}

fn read_group_entries(rootfs: &Path) -> Result<Vec<GroupEntry>> {
    let group_path = rootfs.join("etc/group");
    let contents =
        std::fs::read_to_string(&group_path).map_err(|error| SandboxError::InvalidSpec {
            message: format!(
                "failed to read image group database {}: {error}",
                group_path.display()
            ),
        })?;

    contents
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(parse_group_entry)
        .collect()
}

fn parse_passwd_entry(line: &str) -> Result<PasswdEntry> {
    let fields: Vec<_> = line.split(':').collect();
    if fields.len() < 4 {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "invalid passwd entry {line:?}: expected at least 4 colon-delimited fields"
            ),
        });
    }

    Ok(PasswdEntry {
        name: fields[0].to_owned(),
        uid: fields[2]
            .parse::<u32>()
            .map_err(|error| SandboxError::InvalidSpec {
                message: format!("invalid passwd uid in entry {line:?}: {error}"),
            })?,
        gid: fields[3]
            .parse::<u32>()
            .map_err(|error| SandboxError::InvalidSpec {
                message: format!("invalid passwd gid in entry {line:?}: {error}"),
            })?,
    })
}

fn parse_group_entry(line: &str) -> Result<GroupEntry> {
    let fields: Vec<_> = line.split(':').collect();
    if fields.len() < 3 {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "invalid group entry {line:?}: expected at least 3 colon-delimited fields"
            ),
        });
    }

    Ok(GroupEntry {
        name: fields[0].to_owned(),
        gid: fields[2]
            .parse::<u32>()
            .map_err(|error| SandboxError::InvalidSpec {
                message: format!("invalid group gid in entry {line:?}: {error}"),
            })?,
    })
}

/// Standard OCI Linux mounts that `crun spec` would generate.  crun requires
/// at least a `mounts` block to be present in config.json.
fn default_linux_mounts() -> Value {
    json!([
        {
            "destination": "/proc",
            "type": "proc",
            "source": "proc"
        },
        {
            "destination": "/dev",
            "type": "tmpfs",
            "source": "tmpfs",
            "options": ["nosuid", "strictatime", "mode=755", "size=65536k"]
        },
        {
            "destination": "/dev/pts",
            "type": "devpts",
            "source": "devpts",
            "options": ["nosuid", "noexec", "newinstance", "ptmxmode=0666", "mode=0620"]
        },
        {
            "destination": "/dev/shm",
            "type": "tmpfs",
            "source": "shm",
            "options": ["nosuid", "noexec", "nodev", "mode=1777", "size=65536k"]
        },
        {
            "destination": "/dev/mqueue",
            "type": "mqueue",
            "source": "mqueue",
            "options": ["nosuid", "noexec", "nodev"]
        },
        {
            "destination": "/sys",
            "type": "sysfs",
            "source": "sysfs",
            "options": ["nosuid", "noexec", "nodev", "ro"]
        },
        {
            "destination": "/sys/fs/cgroup",
            "type": "cgroup",
            "source": "cgroup",
            "options": ["nosuid", "noexec", "nodev", "relatime", "ro"]
        }
    ])
}

fn process_cwd(process: &SandboxProcessSpec) -> String {
    let cwd = process.cwd.to_string_lossy();
    if cwd.is_empty() {
        "/".to_owned()
    } else {
        cwd.into_owned()
    }
}

fn process_env(process: &SandboxProcessSpec) -> Vec<String> {
    if process.env.is_empty() {
        return vec![DEFAULT_PATH_ENV.to_owned()];
    }

    process.env.clone()
}

fn validate_port_bindings(port_bindings: &[SandboxPortBinding]) -> Result<()> {
    let mut names = BTreeSet::new();
    let mut host_ports = BTreeSet::new();

    for port_binding in port_bindings {
        if port_binding.name.trim().is_empty() {
            return Err(SandboxError::InvalidSpec {
                message: "sandbox port binding names cannot be empty".to_owned(),
            });
        }
        if !names.insert(port_binding.name.clone()) {
            return Err(SandboxError::InvalidSpec {
                message: format!("duplicate sandbox port binding name: {}", port_binding.name),
            });
        }
        if !host_ports.insert((port_binding.host_address, port_binding.host_port)) {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "duplicate sandbox host port binding: {}:{}",
                    port_binding.host_address, port_binding.host_port
                ),
            });
        }
    }

    Ok(())
}

pub(crate) fn format_port_map(port_bindings: &[SandboxPortBinding]) -> String {
    port_bindings
        .iter()
        .map(|binding| format!("{}:{}", binding.host_port, binding.guest_port))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use neovex_core::TenantId;

    use super::{KrunBundleLayout, KrunBundleOptions, build_bundle_config, write_bundle_config};
    use crate::backend::SandboxBackendKind;
    use crate::endpoint::PublishedEndpointProtocol;
    use crate::spec::{SandboxFilesystemSpec, SandboxPortBinding, SandboxProcessSpec, SandboxSpec};

    #[test]
    fn bundle_config_sets_krun_handler_and_port_map() {
        let spec = sample_spec();
        let config = build_bundle_config("neovex-db", &spec, &KrunBundleOptions::default())
            .expect("bundle config should build");

        assert_eq!(config["annotations"]["run.oci.handler"], "krun");
        assert_eq!(
            config["annotations"]["krun.port_map"],
            "15432:5432,18080:8080"
        );
        assert_eq!(config["process"]["terminal"], false);
    }

    #[test]
    fn bundle_config_omits_network_namespace() {
        let spec = sample_spec();
        let config = build_bundle_config("neovex-db", &spec, &KrunBundleOptions::default())
            .expect("bundle config should build");
        let namespaces = config["linux"]["namespaces"]
            .as_array()
            .expect("linux.namespaces should be an array");

        assert!(
            namespaces
                .iter()
                .all(|namespace| namespace["type"] != "network"),
            "krun bundles must omit the network namespace so TSI ports bind on the host"
        );
    }

    #[test]
    fn write_bundle_config_materializes_config_json() {
        let temp_dir = TempDir::new().expect("temporary directory should be created");
        let layout = KrunBundleLayout::new(temp_dir.path().join("bundle"));
        let spec = sample_spec();

        write_bundle_config(&layout, "neovex-db", &spec, &KrunBundleOptions::default())
            .expect("bundle should be written");

        let rendered = fs::read_to_string(&layout.config_path).expect("config should be readable");
        assert!(
            rendered.contains("\"krun.port_map\": \"15432:5432,18080:8080\""),
            "rendered config should include the expected krun port map annotation"
        );
    }

    #[test]
    fn bundle_config_uses_numeric_image_user_from_bundle_options() {
        let spec = sample_spec();
        let config = build_bundle_config(
            "neovex-db",
            &spec,
            &KrunBundleOptions {
                process_user: Some("1001:1002".to_owned()),
            },
        )
        .expect("bundle config should lower a numeric image user");

        assert_eq!(config["process"]["user"]["uid"], 1001);
        assert_eq!(config["process"]["user"]["gid"], 1002);
    }

    #[test]
    fn bundle_config_resolves_named_image_user_from_rootfs_passwd() {
        let temp_dir = TempDir::new().expect("temporary directory should be created");
        let rootfs = temp_dir.path().join("rootfs");
        fs::create_dir_all(rootfs.join("etc")).expect("rootfs etc directory should exist");
        fs::write(
            rootfs.join("etc/passwd"),
            "postgres:x:26:27:Postgres:/var/lib/postgresql:/bin/sh\n",
        )
        .expect("passwd file should be written");

        let spec = sample_spec_with_rootfs(&rootfs);
        let config = build_bundle_config(
            "neovex-db",
            &spec,
            &KrunBundleOptions {
                process_user: Some("postgres".to_owned()),
            },
        )
        .expect("bundle config should resolve a named image user from /etc/passwd");

        assert_eq!(config["process"]["user"]["uid"], 26);
        assert_eq!(config["process"]["user"]["gid"], 27);
    }

    #[test]
    fn bundle_config_rejects_unknown_named_image_user() {
        let temp_dir = TempDir::new().expect("temporary directory should be created");
        let rootfs = temp_dir.path().join("rootfs");
        fs::create_dir_all(rootfs.join("etc")).expect("rootfs etc directory should exist");
        fs::write(rootfs.join("etc/passwd"), "root:x:0:0:root:/root:/bin/sh\n")
            .expect("passwd file should be written");

        let spec = sample_spec_with_rootfs(&rootfs);
        let error = build_bundle_config(
            "neovex-db",
            &spec,
            &KrunBundleOptions {
                process_user: Some("postgres".to_owned()),
            },
        )
        .expect_err("unknown named image users should be rejected");

        assert!(
            error.to_string().contains("was not found"),
            "expected a missing-user error, got: {error}"
        );
    }

    #[test]
    fn bundle_config_defaults_numeric_image_user_group_to_root_when_passwd_is_absent() {
        let temp_dir = TempDir::new().expect("temporary directory should be created");
        let rootfs = temp_dir.path().join("rootfs");
        fs::create_dir_all(rootfs.join("etc")).expect("rootfs etc directory should exist");

        let spec = sample_spec_with_rootfs(&rootfs);
        let config = build_bundle_config(
            "neovex-db",
            &spec,
            &KrunBundleOptions {
                process_user: Some("1234".to_owned()),
            },
        )
        .expect("numeric image users should not require /etc/passwd");

        assert_eq!(config["process"]["user"]["uid"], 1234);
        assert_eq!(config["process"]["user"]["gid"], 0);
    }

    fn sample_spec() -> SandboxSpec {
        sample_spec_with_rootfs(Path::new("/srv/rootfs"))
    }

    fn sample_spec_with_rootfs(rootfs: &Path) -> SandboxSpec {
        SandboxSpec::new(
            TenantId::new("tenant").expect("tenant id should be valid"),
            "db",
            SandboxBackendKind::Krun,
            SandboxFilesystemSpec::new(rootfs),
            SandboxProcessSpec::new(["/usr/bin/postgres", "-D", "/var/lib/postgresql/data"])
                .with_env(["PATH=/usr/bin", "PGDATA=/var/lib/postgresql/data"]),
        )
        .with_port_bindings([
            SandboxPortBinding::new("postgres", PublishedEndpointProtocol::Tcp, 15432, 5432),
            SandboxPortBinding::new("health", PublishedEndpointProtocol::Http, 18080, 8080),
        ])
    }
}
