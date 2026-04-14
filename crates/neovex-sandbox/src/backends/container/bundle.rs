use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Result, SandboxError};
use crate::spec::{SandboxPortBinding, SandboxProcessSpec, SandboxResourceLimits, SandboxSpec};

const DEFAULT_PATH_ENV: &str = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
const DEFAULT_CPU_PERIOD: u64 = 100_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ContainerBundleLayout {
    pub bundle_dir: PathBuf,
    pub config_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProcessUser {
    uid: u32,
    gid: u32,
}

impl ProcessUser {
    const ROOT: Self = Self { uid: 0, gid: 0 };
}

impl ContainerBundleLayout {
    pub(crate) fn new(bundle_dir: impl Into<PathBuf>) -> Self {
        let bundle_dir = bundle_dir.into();
        Self {
            config_path: bundle_dir.join("config.json"),
            bundle_dir,
        }
    }
}

pub(crate) fn write_bundle_config(
    layout: &ContainerBundleLayout,
    hostname: &str,
    spec: &SandboxSpec,
    image_user: Option<&str>,
    network_namespace_path: Option<&Path>,
) -> Result<()> {
    std::fs::create_dir_all(&layout.bundle_dir).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to create container bundle directory {}: {error}",
            layout.bundle_dir.display()
        ),
    })?;

    let config = build_bundle_config(hostname, spec, image_user, network_namespace_path)?;
    let rendered =
        serde_json::to_vec_pretty(&config).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to serialize container bundle config: {error}"),
        })?;
    std::fs::write(&layout.config_path, rendered).map_err(|error| {
        SandboxError::OperationFailed {
            message: format!(
                "failed to write container bundle config {}: {error}",
                layout.config_path.display()
            ),
        }
    })?;
    Ok(())
}

pub(crate) fn build_bundle_config(
    hostname: &str,
    spec: &SandboxSpec,
    image_user: Option<&str>,
    network_namespace_path: Option<&Path>,
) -> Result<Value> {
    if spec.process.args.is_empty() {
        return Err(SandboxError::InvalidSpec {
            message: "sandbox process args cannot be empty".to_owned(),
        });
    }
    if spec.process.terminal {
        return Err(SandboxError::InvalidSpec {
            message: "container service-mode sandboxes require process.terminal = false".to_owned(),
        });
    }

    validate_port_bindings(&spec.port_bindings)?;
    validate_resource_limits(&spec.resources)?;
    let process_user = parse_process_user(image_user)?;

    let mut linux = serde_json::Map::new();
    let mut namespaces = vec![
        json!({ "type": "mount" }),
        json!({ "type": "uts" }),
        json!({ "type": "ipc" }),
        json!({ "type": "pid" }),
    ];
    if let Some(network_namespace_path) = network_namespace_path {
        namespaces.push(json!({
            "type": "network",
            "path": network_namespace_path,
        }));
    }
    linux.insert("namespaces".to_owned(), Value::Array(namespaces));
    if let Some(resources) = build_linux_resources(&spec.resources) {
        linux.insert("resources".to_owned(), resources);
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
        "linux": Value::Object(linux),
    }))
}

fn parse_process_user(image_user: Option<&str>) -> Result<ProcessUser> {
    let Some(image_user) = image_user.map(str::trim).filter(|user| !user.is_empty()) else {
        return Ok(ProcessUser::ROOT);
    };

    let (uid, gid) = match image_user.split_once(':') {
        Some((uid, gid)) => (
            parse_user_component("uid", uid, image_user)?,
            parse_user_component("gid", gid, image_user)?,
        ),
        None => (parse_user_component("uid", image_user, image_user)?, 0),
    };

    Ok(ProcessUser { uid, gid })
}

fn parse_user_component(kind: &str, value: &str, image_user: &str) -> Result<u32> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|_| SandboxError::InvalidSpec {
            message: format!(
                "container image user must resolve to numeric uid[:gid], got {image_user:?} with invalid {kind} component {value:?}"
            ),
        })
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

fn validate_resource_limits(resources: &SandboxResourceLimits) -> Result<()> {
    if matches!(resources.cpu_count, Some(0)) {
        return Err(SandboxError::InvalidSpec {
            message: "container sandbox cpu_count must be greater than zero".to_owned(),
        });
    }
    if matches!(resources.memory_limit_bytes, Some(0)) {
        return Err(SandboxError::InvalidSpec {
            message: "container sandbox memory_limit_bytes must be greater than zero".to_owned(),
        });
    }
    Ok(())
}

fn build_linux_resources(resources: &SandboxResourceLimits) -> Option<Value> {
    let mut map = serde_json::Map::new();

    if let Some(memory_limit_bytes) = resources.memory_limit_bytes {
        map.insert(
            "memory".to_owned(),
            json!({
                "limit": memory_limit_bytes,
            }),
        );
    }

    if let Some(cpu_count) = resources.cpu_count {
        map.insert(
            "cpu".to_owned(),
            json!({
                "quota": u64::from(cpu_count) * DEFAULT_CPU_PERIOD,
                "period": DEFAULT_CPU_PERIOD,
            }),
        );
    }

    (!map.is_empty()).then_some(Value::Object(map))
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
        vec![DEFAULT_PATH_ENV.to_owned()]
    } else {
        process.env.clone()
    }
}

fn default_linux_mounts() -> Vec<Value> {
    vec![
        json!({
            "destination": "/proc",
            "type": "proc",
            "source": "proc"
        }),
        json!({
            "destination": "/dev",
            "type": "tmpfs",
            "source": "tmpfs",
            "options": ["nosuid", "strictatime", "mode=755", "size=65536k"]
        }),
        json!({
            "destination": "/dev/pts",
            "type": "devpts",
            "source": "devpts",
            "options": ["nosuid", "noexec", "newinstance", "ptmxmode=0666", "mode=0620"]
        }),
        json!({
            "destination": "/dev/shm",
            "type": "tmpfs",
            "source": "shm",
            "options": ["nosuid", "noexec", "nodev", "mode=1777", "size=65536k"]
        }),
        json!({
            "destination": "/dev/mqueue",
            "type": "mqueue",
            "source": "mqueue",
            "options": ["nosuid", "noexec", "nodev"]
        }),
        json!({
            "destination": "/sys",
            "type": "sysfs",
            "source": "sysfs",
            "options": ["nosuid", "noexec", "nodev", "ro"]
        }),
        json!({
            "destination": "/sys/fs/cgroup",
            "type": "cgroup",
            "source": "cgroup",
            "options": ["nosuid", "noexec", "nodev", "relatime", "rw"]
        }),
    ]
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use neovex_core::TenantId;

    use super::build_bundle_config;
    use crate::backend::SandboxBackendKind;
    use crate::spec::{SandboxFilesystemSpec, SandboxPortBinding, SandboxProcessSpec, SandboxSpec};

    fn sample_spec() -> SandboxSpec {
        SandboxSpec::new(
            TenantId::new("svc-demo").expect("tenant should parse"),
            "db",
            SandboxBackendKind::Container,
            SandboxFilesystemSpec::new(PathBuf::from("/tmp/rootfs")),
            SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
        )
    }

    #[test]
    fn bundle_config_uses_numeric_image_user_when_present() {
        let config = build_bundle_config("db", &sample_spec(), Some("33:33"), None)
            .expect("bundle should render");

        assert_eq!(config["process"]["user"]["uid"], 33);
        assert_eq!(config["process"]["user"]["gid"], 33);
    }

    #[test]
    fn bundle_config_includes_explicit_network_namespace_and_remapped_ports() {
        let spec = sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18080, 8080));
        let netns_path = PathBuf::from("/run/neovex/netns/db-01");

        let config = build_bundle_config("db", &spec, None, Some(netns_path.as_path()))
            .expect("bundle should render");

        let namespaces = config["linux"]["namespaces"]
            .as_array()
            .expect("linux.namespaces should be present");
        assert!(namespaces.iter().any(|namespace| {
            namespace["type"] == "network" && namespace["path"] == "/run/neovex/netns/db-01"
        }));
        assert_eq!(config["process"]["user"]["uid"], 0);
    }
}
