use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Result, SandboxError};
use crate::spec::{SandboxPortBinding, SandboxProcessSpec, SandboxResourceLimits, SandboxSpec};
use nimbus_egress::{
    EGRESS_ENFORCEMENT_ENV, EGRESS_PROXY_URL_ENV, EGRESS_RESERVED_ENV_KEYS, EgressEnforcementPlan,
    EgressReloadPolicy,
};

const DEFAULT_PATH_ENV: &str = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
const DEFAULT_CPU_PERIOD: u64 = 100_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ContainerBundleLayout {
    pub bundle_dir: PathBuf,
    pub config_path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ContainerBundleOptions {
    pub additional_mounts: Vec<ContainerBundleMount>,
    pub egress_proxy_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContainerBundleMount {
    pub destination: String,
    pub source: PathBuf,
    pub options: Vec<String>,
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
    process_user: Option<&str>,
    network_namespace_path: Option<&Path>,
    options: &ContainerBundleOptions,
) -> Result<()> {
    std::fs::create_dir_all(&layout.bundle_dir).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to create container bundle directory {}: {error}",
            layout.bundle_dir.display()
        ),
    })?;

    let config = build_bundle_config(
        hostname,
        spec,
        process_user,
        network_namespace_path,
        options,
    )?;
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
    process_user: Option<&str>,
    network_namespace_path: Option<&Path>,
    options: &ContainerBundleOptions,
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
    let compiled_egress = spec
        .egress
        .compile()
        .map_err(|message| SandboxError::InvalidSpec { message })?;
    let egress_enforcement =
        EgressEnforcementPlan::supervisor_proxy(&compiled_egress, EgressReloadPolicy::LiveReload);
    let process_user = parse_process_user(process_user)?;
    let process_env = process_env(
        spec,
        &egress_enforcement,
        options.egress_proxy_url.as_deref(),
    )?;

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

    let mut mounts = default_linux_mounts();
    mounts.extend(options.additional_mounts.iter().map(bundle_mount_json));

    let rootfs = spec.rootfs().ok_or_else(|| SandboxError::InvalidSpec {
        message: format!(
            "container sandbox {} must be resolved to a rootfs before writing an OCI bundle",
            spec.display_name()
        ),
    })?;

    Ok(json!({
        "ociVersion": "1.0.2",
        "process": {
            "terminal": false,
            "user": {
                "uid": process_user.uid,
                "gid": process_user.gid,
            },
            "args": spec.process.args,
            "env": process_env,
            "cwd": process_cwd(&spec.process),
        },
        "root": {
            "path": rootfs.rootfs.to_string_lossy(),
            "readonly": rootfs.readonly,
        },
        "hostname": hostname,
        "mounts": mounts,
        "linux": Value::Object(linux),
    }))
}

fn bundle_mount_json(mount: &ContainerBundleMount) -> Value {
    json!({
        "destination": mount.destination,
        "type": "bind",
        "source": mount.source,
        "options": mount.options,
    })
}

fn parse_process_user(process_user: Option<&str>) -> Result<ProcessUser> {
    let Some(process_user) = process_user.map(str::trim).filter(|user| !user.is_empty()) else {
        return Ok(ProcessUser::ROOT);
    };

    let (uid, gid) = match process_user.split_once(':') {
        Some((uid, gid)) => (
            parse_user_component("uid", uid, process_user)?,
            parse_user_component("gid", gid, process_user)?,
        ),
        None => (parse_user_component("uid", process_user, process_user)?, 0),
    };

    Ok(ProcessUser { uid, gid })
}

fn parse_user_component(kind: &str, value: &str, process_user: &str) -> Result<u32> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|_| SandboxError::InvalidSpec {
            message: format!(
                "container process user must resolve to numeric uid[:gid], got {process_user:?} with invalid {kind} component {value:?}"
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
    if resources.disk_limit_bytes.is_some() {
        return Err(SandboxError::InvalidSpec {
            message: "container sandbox disk_limit_bytes is not enforceable: the writable surface is a host bind-mount and OCI linux.resources has no total-disk-capacity control".to_owned(),
        });
    }
    if matches!(resources.log_limit_bytes, Some(0)) {
        return Err(SandboxError::InvalidSpec {
            message: "container sandbox log_limit_bytes must be greater than zero".to_owned(),
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

fn process_env(
    spec: &SandboxSpec,
    egress_enforcement: &EgressEnforcementPlan,
    egress_proxy_url: Option<&str>,
) -> Result<Vec<String>> {
    let mut env = if spec.process.env.is_empty() {
        vec![DEFAULT_PATH_ENV.to_owned()]
    } else {
        spec.process.env.clone()
    };
    env.retain(|entry| env_key(entry).is_none_or(|key| !EGRESS_RESERVED_ENV_KEYS.contains(&key)));
    let rendered = serde_json::to_string(egress_enforcement).map_err(|error| {
        SandboxError::OperationFailed {
            message: format!("failed to serialize sandbox egress enforcement plan: {error}"),
        }
    })?;
    env.push(format!("{EGRESS_ENFORCEMENT_ENV}={rendered}"));
    if let Some(egress_proxy_url) = egress_proxy_url {
        env.extend(egress_proxy_env_entries(egress_proxy_url));
    }
    Ok(env)
}

fn egress_proxy_env_entries(egress_proxy_url: &str) -> Vec<String> {
    [
        (EGRESS_PROXY_URL_ENV, egress_proxy_url),
        ("HTTP_PROXY", egress_proxy_url),
        ("http_proxy", egress_proxy_url),
        ("HTTPS_PROXY", egress_proxy_url),
        ("https_proxy", egress_proxy_url),
        ("ALL_PROXY", egress_proxy_url),
        ("all_proxy", egress_proxy_url),
        ("NO_PROXY", ""),
        ("no_proxy", ""),
    ]
    .into_iter()
    .map(|(key, value)| format!("{key}={value}"))
    .collect()
}

fn env_key(entry: &str) -> Option<&str> {
    let (key, _) = entry.split_once('=')?;
    (!key.is_empty()).then_some(key)
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

    use nimbus_core::TenantId;

    use super::build_bundle_config;
    use crate::backend::SandboxBackendKind;
    use crate::spec::{
        SandboxOwnerSpec, SandboxPortBinding, SandboxProcessSpec, SandboxResourceLimits,
        SandboxRootSpec, SandboxRootfsSpec, SandboxSpec,
    };
    use nimbus_egress::{
        EGRESS_ENFORCEMENT_ENV, EGRESS_ENFORCEMENT_SCHEMA_VERSION, EGRESS_LEGACY_POLICY_ENV,
        EGRESS_PROXY_URL_ENV, EgressEnforcementMode, EgressEnforcementPlan, EgressPolicy,
        EgressProtocol, EgressReloadPolicy, EgressRule,
    };

    fn egress_enforcement_from_config(config: &serde_json::Value) -> EgressEnforcementPlan {
        let env = config["process"]["env"]
            .as_array()
            .expect("env should be an array");
        let enforcement_prefix = format!("{EGRESS_ENFORCEMENT_ENV}=");
        let enforcement_entries = env
            .iter()
            .filter_map(serde_json::Value::as_str)
            .filter_map(|entry| entry.strip_prefix(&enforcement_prefix))
            .collect::<Vec<_>>();
        assert_eq!(
            enforcement_entries.len(),
            1,
            "bundle generation should emit exactly one egress enforcement env value"
        );
        serde_json::from_str(enforcement_entries[0])
            .expect("egress enforcement env should contain JSON")
    }

    fn sample_spec() -> SandboxSpec {
        SandboxSpec::new(
            TenantId::new("svc-demo").expect("tenant should parse"),
            SandboxOwnerSpec::service("db"),
            SandboxBackendKind::Container,
            SandboxRootSpec::Rootfs(SandboxRootfsSpec::new(PathBuf::from("/tmp/rootfs"))),
            SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
        )
    }

    fn env_from_config(config: &serde_json::Value) -> Vec<&str> {
        config["process"]["env"]
            .as_array()
            .expect("env should be an array")
            .iter()
            .map(|value| value.as_str().expect("env entries should be strings"))
            .collect()
    }

    #[test]
    fn bundle_config_uses_numeric_image_user_when_present() {
        let config = build_bundle_config(
            "db",
            &sample_spec(),
            Some("33:33"),
            None,
            &crate::backends::container::bundle::ContainerBundleOptions::default(),
        )
        .expect("bundle should render");

        assert_eq!(config["process"]["user"]["uid"], 33);
        assert_eq!(config["process"]["user"]["gid"], 33);
    }

    #[test]
    fn bundle_config_includes_explicit_network_namespace_and_remapped_ports() {
        let spec = sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18080, 8080));
        let netns_path = PathBuf::from("/run/nimbus/netns/db-01");

        let config = build_bundle_config(
            "db",
            &spec,
            None,
            Some(netns_path.as_path()),
            &crate::backends::container::bundle::ContainerBundleOptions::default(),
        )
        .expect("bundle should render");

        let namespaces = config["linux"]["namespaces"]
            .as_array()
            .expect("linux.namespaces should be present");
        assert!(namespaces.iter().any(|namespace| {
            namespace["type"] == "network" && namespace["path"] == "/run/nimbus/netns/db-01"
        }));
        assert_eq!(config["process"]["user"]["uid"], 0);
    }

    #[test]
    fn bundle_config_materializes_sandbox_egress_enforcement_contract_env() {
        let mut spec = sample_spec().with_egress_policy(EgressPolicy::new([EgressRule::new(
            "stripe",
            EgressProtocol::Https,
            "api.stripe.com",
            443,
        )
        .with_methods(["POST"])
        .with_path_prefixes(["/v1/"])]));
        spec.process.env = vec![
            "PATH=/usr/bin".to_owned(),
            format!("{EGRESS_ENFORCEMENT_ENV}={{\"schema_version\":0}}"),
            format!("{EGRESS_LEGACY_POLICY_ENV}={{\"allow\":[]}}"),
        ];

        let config = build_bundle_config(
            "db",
            &spec,
            None,
            None,
            &crate::backends::container::bundle::ContainerBundleOptions::default(),
        )
        .expect("bundle should render");

        let env = config["process"]["env"]
            .as_array()
            .expect("env should be an array");
        let enforcement_prefix = format!("{EGRESS_ENFORCEMENT_ENV}=");
        let legacy_policy_prefix = format!("{EGRESS_LEGACY_POLICY_ENV}=");
        let enforcement_entries = env
            .iter()
            .filter_map(serde_json::Value::as_str)
            .filter_map(|entry| entry.strip_prefix(&enforcement_prefix))
            .collect::<Vec<_>>();
        assert_eq!(
            enforcement_entries.len(),
            1,
            "bundle generation should replace spoofed egress enforcement env values"
        );
        assert!(
            env.iter()
                .filter_map(serde_json::Value::as_str)
                .all(|entry| !entry.starts_with(&legacy_policy_prefix)),
            "bundle generation should remove spoofed legacy egress policy env values"
        );
        let enforcement: EgressEnforcementPlan = serde_json::from_str(enforcement_entries[0])
            .expect("egress enforcement env should contain JSON");
        assert_eq!(
            enforcement.schema_version,
            EGRESS_ENFORCEMENT_SCHEMA_VERSION
        );
        assert_eq!(enforcement.mode, EgressEnforcementMode::SupervisorProxy);
        assert_eq!(enforcement.reload_policy, EgressReloadPolicy::LiveReload);
        assert_eq!(enforcement.policy().rules()[0].host, "api.stripe.com");
        enforcement
            .validate()
            .expect("materialized egress enforcement contract should validate");
    }

    #[test]
    fn bundle_config_scrubs_spoofed_proxy_env_and_injects_backend_proxy_url() {
        let mut spec = sample_spec();
        spec.process.env = vec![
            "PATH=/usr/bin".to_owned(),
            "HTTP_PROXY=http://attacker.invalid:1".to_owned(),
            "http_proxy=http://attacker.invalid:2".to_owned(),
            "HTTPS_PROXY=http://attacker.invalid:3".to_owned(),
            "NO_PROXY=metadata.google.internal,169.254.169.254".to_owned(),
            format!("{EGRESS_PROXY_URL_ENV}=http://attacker.invalid:4"),
        ];

        let config = build_bundle_config(
            "db",
            &spec,
            None,
            None,
            &crate::backends::container::bundle::ContainerBundleOptions {
                egress_proxy_url: Some("http://10.89.0.1:15000".to_owned()),
                ..Default::default()
            },
        )
        .expect("bundle should render");
        let env = env_from_config(&config);

        assert!(env.contains(&"PATH=/usr/bin"));
        for expected in [
            format!("{EGRESS_PROXY_URL_ENV}=http://10.89.0.1:15000"),
            "HTTP_PROXY=http://10.89.0.1:15000".to_owned(),
            "http_proxy=http://10.89.0.1:15000".to_owned(),
            "HTTPS_PROXY=http://10.89.0.1:15000".to_owned(),
            "https_proxy=http://10.89.0.1:15000".to_owned(),
            "ALL_PROXY=http://10.89.0.1:15000".to_owned(),
            "all_proxy=http://10.89.0.1:15000".to_owned(),
            "NO_PROXY=".to_owned(),
            "no_proxy=".to_owned(),
        ] {
            assert!(
                env.contains(&expected.as_str()),
                "expected proxy env {expected:?} in {env:?}"
            );
        }
        assert!(
            env.iter().all(|entry| !entry.contains("attacker.invalid")),
            "operator-provided proxy env must be scrubbed: {env:?}"
        );
    }

    #[test]
    fn bundle_config_materializes_default_deny_supervisor_proxy_egress_contract_env() {
        let config = build_bundle_config(
            "db",
            &sample_spec(),
            None,
            None,
            &crate::backends::container::bundle::ContainerBundleOptions::default(),
        )
        .expect("bundle should render");

        let enforcement = egress_enforcement_from_config(&config);

        assert_eq!(
            enforcement.schema_version,
            EGRESS_ENFORCEMENT_SCHEMA_VERSION
        );
        assert_eq!(enforcement.mode, EgressEnforcementMode::SupervisorProxy);
        assert_eq!(enforcement.reload_policy, EgressReloadPolicy::LiveReload);
        assert!(
            enforcement.policy().is_deny_all(),
            "default sandbox egress should remain deny-all"
        );
        enforcement
            .validate()
            .expect("default supervisor egress contract should validate");
    }

    #[test]
    fn bundle_config_rejects_invalid_sandbox_egress_policy() {
        let spec = sample_spec().with_egress_policy(EgressPolicy::new([EgressRule::new(
            "wildcard",
            EgressProtocol::Https,
            "*",
            443,
        )]));

        let error = build_bundle_config(
            "db",
            &spec,
            None,
            None,
            &crate::backends::container::bundle::ContainerBundleOptions::default(),
        )
        .expect_err("invalid sandbox egress policy should fail bundle generation");

        assert!(
            error.to_string().contains("wildcards"),
            "bundle generation should expose the invalid egress policy error: {error}"
        );
    }

    #[test]
    fn bundle_config_rejects_unenforceable_disk_limit() {
        let spec = sample_spec()
            .with_resource_limits(SandboxResourceLimits::default().with_disk_limit_bytes(1024));

        let error = build_bundle_config(
            "db",
            &spec,
            None,
            None,
            &crate::backends::container::bundle::ContainerBundleOptions::default(),
        )
        .expect_err("an unenforceable disk_limit_bytes must fail closed");

        assert!(
            error
                .to_string()
                .contains("disk_limit_bytes is not enforceable"),
            "expected actionable disk-limit validation error, got: {error}"
        );

        // An enforceable limit on the same surface (memory) must still build a bundle:
        // the guard is specific to the disk knob Nimbus cannot honor, not a blanket
        // rejection of resource limits.
        let with_memory = sample_spec().with_resource_limits(
            SandboxResourceLimits::default().with_memory_limit_bytes(256 * 1024 * 1024),
        );
        build_bundle_config(
            "db",
            &with_memory,
            None,
            None,
            &crate::backends::container::bundle::ContainerBundleOptions::default(),
        )
        .expect("a spec without disk_limit_bytes should still render a bundle");
    }
}
