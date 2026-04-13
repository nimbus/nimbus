use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Result, SandboxError};
use crate::spec::{SandboxPortBinding, SandboxProcessSpec, SandboxResourceLimits, SandboxSpec};

const DEFAULT_PATH_ENV: &str = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
const MIN_MEMORY_LIMIT_BYTES: u64 = 1024 * 1024;

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
    validate_resource_limits(&spec.resources)?;

    // krun VMMs always run as root because the crun process needs /dev/kvm access.
    // The image USER is resolved separately and stored in the sandbox manifest for
    // guest-side application via the guest init process.  Unlike regular containers
    // where the host manages user namespace mapping, krun guests have their own
    // kernel and handle user switching internally.
    let _configured_user = options.process_user.as_deref();
    let process_user = ProcessUser::ROOT;

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

    let mut linux = serde_json::Map::new();
    linux.insert(
        "namespaces".to_owned(),
        json!([
            { "type": "mount" },
            { "type": "uts" },
            { "type": "ipc" },
            { "type": "pid" },
        ]),
    );
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
        "annotations": annotations,
        "linux": Value::Object(linux),
    }))
}

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

fn validate_resource_limits(resources: &SandboxResourceLimits) -> Result<()> {
    if matches!(resources.cpu_count, Some(0)) {
        return Err(SandboxError::InvalidSpec {
            message: "krun sandbox cpu_count must be greater than zero".to_owned(),
        });
    }

    if matches!(resources.memory_limit_bytes, Some(0)) {
        return Err(SandboxError::InvalidSpec {
            message: "krun sandbox memory_limit_bytes must be greater than zero".to_owned(),
        });
    }

    if let Some(memory_limit_bytes) = resources.memory_limit_bytes {
        if memory_limit_bytes < MIN_MEMORY_LIMIT_BYTES {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "krun sandbox memory_limit_bytes must be at least {MIN_MEMORY_LIMIT_BYTES} bytes"
                ),
            });
        }
    }

    if resources.cpu_count.is_some() && resources.memory_limit_bytes.is_none() {
        return Err(SandboxError::InvalidSpec {
            message: "krun sandbox cpu_count requires memory_limit_bytes so crun can materialize /.krun_vm.json".to_owned(),
        });
    }

    Ok(())
}

fn build_linux_resources(resources: &SandboxResourceLimits) -> Option<Value> {
    let memory_limit_bytes = resources.memory_limit_bytes?;
    Some(json!({
        "memory": {
            "limit": memory_limit_bytes,
        },
    }))
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
    use crate::spec::{
        SandboxFilesystemSpec, SandboxPortBinding, SandboxProcessSpec, SandboxResourceLimits,
        SandboxSpec,
    };

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
    fn bundle_config_always_uses_root_user_for_krun_vmm() {
        // krun VMMs always run as root because the crun process needs /dev/kvm.
        // Image USER is stored in the manifest for guest-side application.
        let spec = sample_spec();
        let config = build_bundle_config(
            "neovex-db",
            &spec,
            &KrunBundleOptions {
                process_user: Some("1001:1002".to_owned()),
            },
        )
        .expect("bundle config should build even with non-root configured user");

        assert_eq!(
            config["process"]["user"]["uid"], 0,
            "krun bundle must use root uid for VMM access to /dev/kvm"
        );
        assert_eq!(
            config["process"]["user"]["gid"], 0,
            "krun bundle must use root gid for VMM access to /dev/kvm"
        );
    }

    #[test]
    fn bundle_config_uses_root_even_when_named_user_configured() {
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
        .expect("bundle config should build with named user configured");

        assert_eq!(config["process"]["user"]["uid"], 0);
        assert_eq!(config["process"]["user"]["gid"], 0);
    }

    #[test]
    fn bundle_config_uses_root_when_no_passwd_available() {
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
        .expect("bundle config should build with numeric user without /etc/passwd");

        assert_eq!(config["process"]["user"]["uid"], 0);
        assert_eq!(config["process"]["user"]["gid"], 0);
    }

    #[test]
    fn bundle_config_sets_linux_memory_limit_from_generic_resources() {
        let spec = sample_spec().with_resource_limits(
            SandboxResourceLimits::default().with_memory_limit_bytes(256 * 1024 * 1024),
        );

        let config = build_bundle_config("neovex-db", &spec, &KrunBundleOptions::default())
            .expect("bundle config should build with memory limits");

        assert_eq!(
            config["linux"]["resources"]["memory"]["limit"],
            256 * 1024 * 1024
        );
    }

    #[test]
    fn bundle_config_rejects_cpu_count_without_memory_limit() {
        let spec =
            sample_spec().with_resource_limits(SandboxResourceLimits::default().with_cpu_count(2));

        let error = build_bundle_config("neovex-db", &spec, &KrunBundleOptions::default())
            .expect_err("krun cpu count without memory should be rejected");

        assert!(
            error
                .to_string()
                .contains("cpu_count requires memory_limit_bytes"),
            "expected actionable validation error, got: {error}"
        );
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
