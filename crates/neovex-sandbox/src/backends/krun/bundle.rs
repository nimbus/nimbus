use std::collections::BTreeSet;
use std::path::PathBuf;

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
) -> Result<()> {
    std::fs::create_dir_all(&layout.bundle_dir).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to create krun bundle directory {}: {error}",
            layout.bundle_dir.display()
        ),
    })?;

    let config = build_bundle_config(hostname, spec)?;
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

pub(crate) fn build_bundle_config(hostname: &str, spec: &SandboxSpec) -> Result<Value> {
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
                "uid": 0,
                "gid": 0,
            },
            "args": spec.process.args,
            "env": process_env(&spec.process),
            "cwd": spec.process.cwd.to_string_lossy(),
        },
        "root": {
            "path": spec.filesystem.rootfs.to_string_lossy(),
            "readonly": spec.filesystem.readonly,
        },
        "hostname": hostname,
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

    use super::{KrunBundleLayout, build_bundle_config, write_bundle_config};
    use crate::backend::SandboxBackendKind;
    use crate::endpoint::PublishedEndpointProtocol;
    use crate::spec::{SandboxFilesystemSpec, SandboxPortBinding, SandboxProcessSpec, SandboxSpec};

    #[test]
    fn bundle_config_sets_krun_handler_and_port_map() {
        let spec = sample_spec();
        let config = build_bundle_config("neovex-db", &spec).expect("bundle config should build");

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
        let config = build_bundle_config("neovex-db", &spec).expect("bundle config should build");
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

        write_bundle_config(&layout, "neovex-db", &spec).expect("bundle should be written");

        let rendered = fs::read_to_string(&layout.config_path).expect("config should be readable");
        assert!(
            rendered.contains("\"krun.port_map\": \"15432:5432,18080:8080\""),
            "rendered config should include the expected krun port map annotation"
        );
    }

    fn sample_spec() -> SandboxSpec {
        SandboxSpec::new(
            TenantId::new("tenant").expect("tenant id should be valid"),
            "db",
            SandboxBackendKind::Krun,
            SandboxFilesystemSpec::new(Path::new("/srv/rootfs")),
            SandboxProcessSpec::new(["/usr/bin/postgres", "-D", "/var/lib/postgresql/data"])
                .with_env(["PATH=/usr/bin", "PGDATA=/var/lib/postgresql/data"]),
        )
        .with_port_bindings([
            SandboxPortBinding::new("postgres", PublishedEndpointProtocol::Tcp, 15432, 5432),
            SandboxPortBinding::new("health", PublishedEndpointProtocol::Http, 18080, 8080),
        ])
    }
}
