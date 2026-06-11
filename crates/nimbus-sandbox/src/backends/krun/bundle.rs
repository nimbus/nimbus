use std::collections::BTreeSet;
use std::net::IpAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::egress::{
    SANDBOX_EGRESS_ENFORCEMENT_ENV, SANDBOX_EGRESS_RESERVED_ENV_KEYS, SandboxEgressEnforcementPlan,
    SandboxEgressLaunchEnforcement,
};
use crate::error::{Result, SandboxError};
use crate::spec::{SandboxPortBinding, SandboxProcessSpec, SandboxResourceLimits, SandboxSpec};

const DEFAULT_PATH_ENV: &str = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
const MIN_MEMORY_LIMIT_BYTES: u64 = 1024 * 1024;
const KRUN_REQUIRED_CAPABILITIES: &[&str] =
    &["CAP_NET_ADMIN", "CAP_NET_BIND_SERVICE", "CAP_SYS_ADMIN"];
const KRUN_SECCOMP_ALLOWLIST: &[&str] = &[
    "accept",
    "accept4",
    "access",
    "arch_prctl",
    "bind",
    "brk",
    "capget",
    "capset",
    "chdir",
    "clock_getres",
    "clock_gettime",
    "clock_nanosleep",
    "clone",
    "clone3",
    "close",
    "close_range",
    "connect",
    "copy_file_range",
    "dup",
    "dup2",
    "dup3",
    "epoll_create1",
    "epoll_ctl",
    "epoll_pwait",
    "epoll_pwait2",
    "epoll_wait",
    "eventfd2",
    "execve",
    "exit",
    "exit_group",
    "faccessat",
    "faccessat2",
    "fadvise64",
    "fallocate",
    "fcntl",
    "fdatasync",
    "fgetxattr",
    "flistxattr",
    "fstat",
    "fstatfs",
    "fsync",
    "ftruncate",
    "futex",
    "getcwd",
    "getdents64",
    "getegid",
    "geteuid",
    "getgid",
    "getpeername",
    "getpid",
    "getppid",
    "getrandom",
    "getrlimit",
    "getrusage",
    "getsockname",
    "getsockopt",
    "gettid",
    "gettimeofday",
    "getuid",
    "getxattr",
    "ioctl",
    "lgetxattr",
    "listxattr",
    "listen",
    "lseek",
    "madvise",
    "membarrier",
    "memfd_create",
    "mkdirat",
    "mmap",
    "mprotect",
    "mremap",
    "munmap",
    "nanosleep",
    "newfstatat",
    "open",
    "openat",
    "openat2",
    "pipe2",
    "poll",
    "ppoll",
    "prctl",
    "pread64",
    "preadv",
    "preadv2",
    "prlimit64",
    "pwrite64",
    "pwritev",
    "pwritev2",
    "read",
    "readv",
    "readlink",
    "readlinkat",
    "recvfrom",
    "recvmmsg",
    "recvmsg",
    "renameat",
    "renameat2",
    "restart_syscall",
    "rseq",
    "rt_sigaction",
    "rt_sigprocmask",
    "rt_sigreturn",
    "sched_getaffinity",
    "sched_setaffinity",
    "sched_yield",
    "sendmmsg",
    "sendmsg",
    "sendto",
    "set_robust_list",
    "set_tid_address",
    "setrlimit",
    "setsockopt",
    "shutdown",
    "sigaltstack",
    "socket",
    "socketpair",
    "stat",
    "statfs",
    "statx",
    "symlinkat",
    "tgkill",
    "timerfd_create",
    "timerfd_settime",
    "umask",
    "uname",
    "unlinkat",
    "wait4",
    "write",
    "writev",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct KrunBundleLayout {
    pub bundle_dir: PathBuf,
    pub config_path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct KrunBundleOptions {
    pub additional_mounts: Vec<KrunBundleMount>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KrunBundleMount {
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
    let egress_enforcement = SandboxEgressLaunchEnforcement::ProcessSupervisorProxy
        .materialize(&spec.egress)
        .map_err(|message| SandboxError::InvalidSpec { message })?;
    let process_env = process_env(spec, &egress_enforcement)?;

    // krun VMMs always run as root because the crun process needs /dev/kvm access.
    // Any image USER is applied later inside the guest after the VMM is already
    // running, so the OCI bundle must keep the host-side VMM process as root.
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
    linux.insert("seccomp".to_owned(), krun_seccomp_profile());

    let mut mounts = default_linux_mounts();
    mounts.extend(options.additional_mounts.iter().map(bundle_mount_json));

    let rootfs = spec.rootfs().ok_or_else(|| SandboxError::InvalidSpec {
        message: format!(
            "krun sandbox {} must be resolved to a rootfs before writing an OCI bundle",
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
            "noNewPrivileges": true,
            "capabilities": krun_process_capabilities(),
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
        "annotations": annotations,
        "linux": Value::Object(linux),
    }))
}

fn krun_process_capabilities() -> Value {
    json!({
        "bounding": KRUN_REQUIRED_CAPABILITIES,
        "effective": KRUN_REQUIRED_CAPABILITIES,
        "inheritable": [],
        "permitted": KRUN_REQUIRED_CAPABILITIES,
        "ambient": [],
    })
}

fn krun_seccomp_profile() -> Value {
    json!({
        "defaultAction": "SCMP_ACT_ERRNO",
        "defaultErrnoRet": 1,
        "architectures": [
            "SCMP_ARCH_X86_64",
            "SCMP_ARCH_X86",
            "SCMP_ARCH_X32",
            "SCMP_ARCH_AARCH64",
        ],
        "syscalls": [
            {
                "names": KRUN_SECCOMP_ALLOWLIST,
                "action": "SCMP_ACT_ALLOW",
            },
        ],
    })
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
            "options": ["nosuid", "noexec", "nodev", "relatime", "ro"]
        }),
    ]
}

fn bundle_mount_json(mount: &KrunBundleMount) -> Value {
    json!({
        "destination": mount.destination,
        "type": "bind",
        "source": mount.source,
        "options": mount.options,
    })
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
    egress_enforcement: &SandboxEgressEnforcementPlan,
) -> Result<Vec<String>> {
    let mut env = if spec.process.env.is_empty() {
        vec![DEFAULT_PATH_ENV.to_owned()]
    } else {
        spec.process.env.clone()
    };
    env.retain(|entry| {
        env_key(entry).is_none_or(|key| !SANDBOX_EGRESS_RESERVED_ENV_KEYS.contains(&key))
    });
    let rendered = serde_json::to_string(egress_enforcement).map_err(|error| {
        SandboxError::OperationFailed {
            message: format!("failed to serialize sandbox egress enforcement plan: {error}"),
        }
    })?;
    env.push(format!("{SANDBOX_EGRESS_ENFORCEMENT_ENV}={rendered}"));
    Ok(env)
}

fn env_key(entry: &str) -> Option<&str> {
    let (key, _) = entry.split_once('=')?;
    (!key.is_empty()).then_some(key)
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
    if matches!(resources.disk_limit_bytes, Some(0)) {
        return Err(SandboxError::InvalidSpec {
            message: "krun sandbox disk_limit_bytes must be greater than zero".to_owned(),
        });
    }
    if matches!(resources.log_limit_bytes, Some(0)) {
        return Err(SandboxError::InvalidSpec {
            message: "krun sandbox log_limit_bytes must be greater than zero".to_owned(),
        });
    }

    if let Some(memory_limit_bytes) = resources.memory_limit_bytes
        && memory_limit_bytes < MIN_MEMORY_LIMIT_BYTES
    {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "krun sandbox memory_limit_bytes must be at least {MIN_MEMORY_LIMIT_BYTES} bytes"
            ),
        });
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
        if port_binding.host_port == 0 {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "krun sandbox host_port must be greater than zero for binding {}",
                    port_binding.name
                ),
            });
        }
        if port_binding.guest_port == 0 {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "krun sandbox guest_port must be greater than zero for binding {}",
                    port_binding.name
                ),
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
        .map(|binding| {
            format!(
                "{}:{}:{}",
                format_port_map_host_address(binding.host_address),
                binding.host_port,
                binding.guest_port
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn format_port_map_host_address(host_address: IpAddr) -> String {
    match host_address {
        IpAddr::V4(address) => address.to_string(),
        IpAddr::V6(address) => format!("[{address}]"),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::path::Path;

    use serde_json::json;
    use tempfile::TempDir;

    use nimbus_core::TenantId;

    use super::{
        KrunBundleLayout, KrunBundleMount, KrunBundleOptions, build_bundle_config, format_port_map,
        write_bundle_config,
    };
    use crate::backend::SandboxBackendKind;
    use crate::egress::{
        SANDBOX_EGRESS_ENFORCEMENT_ENV, SANDBOX_EGRESS_ENFORCEMENT_SCHEMA_VERSION,
        SANDBOX_EGRESS_LEGACY_POLICY_ENV, SandboxEgressEnforcementMode,
        SandboxEgressEnforcementPlan, SandboxEgressPolicy, SandboxEgressReloadPolicy,
        SandboxEgressRule,
    };
    use crate::endpoint::PublishedEndpointProtocol;
    use crate::spec::{
        SandboxOwnerSpec, SandboxPortBinding, SandboxProcessSpec, SandboxResourceLimits,
        SandboxRootSpec, SandboxRootfsSpec, SandboxSpec,
    };

    fn egress_enforcement_from_config(config: &serde_json::Value) -> SandboxEgressEnforcementPlan {
        let env = config["process"]["env"]
            .as_array()
            .expect("env should be an array");
        let enforcement_prefix = format!("{SANDBOX_EGRESS_ENFORCEMENT_ENV}=");
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

    #[test]
    fn bundle_config_sets_krun_handler_and_port_map() {
        let spec = sample_spec();
        let config = build_bundle_config("nimbus-db", &spec, &KrunBundleOptions::default())
            .expect("bundle config should build");

        assert_eq!(config["annotations"]["run.oci.handler"], "krun");
        assert_eq!(
            config["annotations"]["krun.port_map"],
            "127.0.0.1:15432:5432,127.0.0.1:18080:8080"
        );
        assert_eq!(config["process"]["terminal"], false);
        assert_eq!(
            config["mounts"]
                .as_array()
                .expect("mounts should be present")
                .len(),
            7
        );
    }

    #[test]
    fn bundle_config_omits_network_namespace() {
        let spec = sample_spec();
        let config = build_bundle_config("nimbus-db", &spec, &KrunBundleOptions::default())
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

        write_bundle_config(&layout, "nimbus-db", &spec, &KrunBundleOptions::default())
            .expect("bundle should be written");

        let rendered = fs::read_to_string(&layout.config_path).expect("config should be readable");
        assert!(
            rendered.contains("\"krun.port_map\": \"127.0.0.1:15432:5432,127.0.0.1:18080:8080\""),
            "rendered config should include the expected krun port map annotation"
        );
    }

    #[test]
    fn format_port_map_carries_configured_host_addresses() {
        let port_bindings = [
            SandboxPortBinding::tcp("loopback", 18080, 8080)
                .with_host_address(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2))),
            SandboxPortBinding::tcp("external", 18443, 8443)
                .with_host_address(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))),
        ];

        assert_eq!(
            format_port_map(&port_bindings),
            "127.0.0.2:18080:8080,10.0.0.5:18443:8443"
        );
    }

    #[test]
    fn format_port_map_brackets_ipv6_host_addresses() {
        let port_bindings = [SandboxPortBinding::tcp("ipv6", 18080, 8080)
            .with_host_address(IpAddr::V6(Ipv6Addr::LOCALHOST))];

        assert_eq!(format_port_map(&port_bindings), "[::1]:18080:8080");
    }

    #[test]
    fn bundle_config_always_uses_root_user_for_krun_vmm() {
        // krun VMMs always run as root because the crun process needs /dev/kvm.
        // Image USER is stored in the manifest for guest-side application.
        let spec = sample_spec();
        let config = build_bundle_config("nimbus-db", &spec, &KrunBundleOptions::default())
            .expect("bundle config should build");

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
    fn bundle_config_sets_no_new_privileges() {
        let spec = sample_spec();
        let config = build_bundle_config("nimbus-db", &spec, &KrunBundleOptions::default())
            .expect("bundle config should build");

        assert_eq!(
            config["process"]["noNewPrivileges"], true,
            "krun VMM process should not be able to gain new privileges through exec"
        );
    }

    #[test]
    fn bundle_config_materializes_sandbox_egress_enforcement_contract_env() {
        let mut spec =
            sample_spec().with_egress_policy(SandboxEgressPolicy::new([SandboxEgressRule::new(
                "stripe",
                PublishedEndpointProtocol::Https,
                "api.stripe.com",
                443,
            )
            .with_methods(["POST"])
            .with_path_prefixes(["/v1/"])]));
        spec.process.env = vec![
            "PATH=/usr/bin".to_owned(),
            format!("{SANDBOX_EGRESS_ENFORCEMENT_ENV}={{\"schema_version\":0}}"),
            format!("{SANDBOX_EGRESS_LEGACY_POLICY_ENV}={{\"allow\":[]}}"),
        ];

        let config = build_bundle_config("nimbus-db", &spec, &KrunBundleOptions::default())
            .expect("bundle config should build");

        let env = config["process"]["env"]
            .as_array()
            .expect("env should be an array");
        let enforcement_prefix = format!("{SANDBOX_EGRESS_ENFORCEMENT_ENV}=");
        let legacy_policy_prefix = format!("{SANDBOX_EGRESS_LEGACY_POLICY_ENV}=");
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
        let enforcement: SandboxEgressEnforcementPlan =
            serde_json::from_str(enforcement_entries[0])
                .expect("egress enforcement env should contain JSON");
        assert_eq!(
            enforcement.schema_version,
            SANDBOX_EGRESS_ENFORCEMENT_SCHEMA_VERSION
        );
        assert_eq!(
            enforcement.mode,
            SandboxEgressEnforcementMode::SupervisorProxy
        );
        assert_eq!(
            enforcement.reload_policy,
            SandboxEgressReloadPolicy::RecreateRequired
        );
        assert_eq!(enforcement.policy().rules().len(), 1);
        assert_eq!(enforcement.policy().rules()[0].name, "stripe");
        assert_eq!(
            enforcement.policy().rules()[0].methods,
            vec!["POST".to_string()]
        );
        enforcement
            .validate()
            .expect("materialized egress enforcement contract should validate");
    }

    #[test]
    fn bundle_config_materializes_default_deny_supervisor_proxy_egress_contract_env() {
        let config =
            build_bundle_config("nimbus-db", &sample_spec(), &KrunBundleOptions::default())
                .expect("bundle config should build");

        let enforcement = egress_enforcement_from_config(&config);

        assert_eq!(
            enforcement.schema_version,
            SANDBOX_EGRESS_ENFORCEMENT_SCHEMA_VERSION
        );
        assert_eq!(
            enforcement.mode,
            SandboxEgressEnforcementMode::SupervisorProxy
        );
        assert_eq!(
            enforcement.reload_policy,
            SandboxEgressReloadPolicy::RecreateRequired
        );
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
        let spec =
            sample_spec().with_egress_policy(SandboxEgressPolicy::new([SandboxEgressRule::new(
                "wildcard",
                PublishedEndpointProtocol::Https,
                "*",
                443,
            )]));

        let error = build_bundle_config("nimbus-db", &spec, &KrunBundleOptions::default())
            .expect_err("invalid sandbox egress policy should fail bundle generation");

        assert!(
            error.to_string().contains("wildcards"),
            "bundle generation should expose the invalid egress policy error: {error}"
        );
    }

    #[test]
    fn bundle_config_sets_explicit_krun_capabilities() {
        let spec = sample_spec();
        let config = build_bundle_config("nimbus-db", &spec, &KrunBundleOptions::default())
            .expect("bundle config should build");
        let capabilities = &config["process"]["capabilities"];
        let expected = json!(["CAP_NET_ADMIN", "CAP_NET_BIND_SERVICE", "CAP_SYS_ADMIN"]);

        assert_eq!(capabilities["bounding"], expected);
        assert_eq!(capabilities["effective"], expected);
        assert_eq!(capabilities["permitted"], expected);
        assert_eq!(capabilities["inheritable"], json!([]));
        assert_eq!(capabilities["ambient"], json!([]));
    }

    #[test]
    fn bundle_config_sets_explicit_seccomp_allowlist() {
        let spec = sample_spec();
        let config = build_bundle_config("nimbus-db", &spec, &KrunBundleOptions::default())
            .expect("bundle config should build");
        let seccomp = &config["linux"]["seccomp"];

        assert_eq!(seccomp["defaultAction"], "SCMP_ACT_ERRNO");
        assert_eq!(seccomp["defaultErrnoRet"], 1);
        assert_eq!(
            seccomp["architectures"],
            json!([
                "SCMP_ARCH_X86_64",
                "SCMP_ARCH_X86",
                "SCMP_ARCH_X32",
                "SCMP_ARCH_AARCH64"
            ])
        );
        let names = seccomp["syscalls"][0]["names"]
            .as_array()
            .expect("seccomp allowlist should contain syscall names");
        for syscall in [
            "close_range",
            "execve",
            "ioctl",
            "fgetxattr",
            "mmap",
            "openat",
            "preadv",
            "read",
            "write",
        ] {
            assert!(
                names.iter().any(|name| name.as_str() == Some(syscall)),
                "expected krun seccomp allowlist to include {syscall}"
            );
        }
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
        let config = build_bundle_config("nimbus-db", &spec, &KrunBundleOptions::default())
            .expect("bundle config should build");

        assert_eq!(config["process"]["user"]["uid"], 0);
        assert_eq!(config["process"]["user"]["gid"], 0);
    }

    #[test]
    fn bundle_config_uses_root_when_no_passwd_available() {
        let temp_dir = TempDir::new().expect("temporary directory should be created");
        let rootfs = temp_dir.path().join("rootfs");
        fs::create_dir_all(rootfs.join("etc")).expect("rootfs etc directory should exist");

        let spec = sample_spec_with_rootfs(&rootfs);
        let config = build_bundle_config("nimbus-db", &spec, &KrunBundleOptions::default())
            .expect("bundle config should build");

        assert_eq!(config["process"]["user"]["uid"], 0);
        assert_eq!(config["process"]["user"]["gid"], 0);
    }

    #[test]
    fn bundle_config_appends_additional_bind_mounts() {
        let spec = sample_spec();
        let config = build_bundle_config(
            "nimbus-db",
            &spec,
            &KrunBundleOptions {
                additional_mounts: vec![KrunBundleMount {
                    destination: "/.nimbus".to_owned(),
                    source: Path::new("/usr/libexec/nimbus").into(),
                    options: vec!["rbind".to_owned(), "ro".to_owned()],
                }],
            },
        )
        .expect("bundle config should build");

        let mounts = config["mounts"]
            .as_array()
            .expect("mounts should be an array");
        let helper_mount = mounts
            .iter()
            .find(|mount| mount["destination"] == "/.nimbus")
            .expect("expected helper bind mount to be present");
        assert_eq!(helper_mount["source"], "/usr/libexec/nimbus");
        assert_eq!(helper_mount["type"], "bind");
    }

    #[test]
    fn bundle_config_sets_linux_memory_limit_from_generic_resources() {
        let spec = sample_spec().with_resource_limits(
            SandboxResourceLimits::default().with_memory_limit_bytes(256 * 1024 * 1024),
        );

        let config = build_bundle_config("nimbus-db", &spec, &KrunBundleOptions::default())
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

        let error = build_bundle_config("nimbus-db", &spec, &KrunBundleOptions::default())
            .expect_err("krun cpu count without memory should be rejected");

        assert!(
            error
                .to_string()
                .contains("cpu_count requires memory_limit_bytes"),
            "expected actionable validation error, got: {error}"
        );
    }

    #[test]
    fn bundle_config_rejects_zero_host_port() {
        let spec = sample_spec_with_rootfs(Path::new("/srv/rootfs"))
            .with_port_binding(SandboxPortBinding::tcp("invalid-host", 0, 8080));

        let error = build_bundle_config("nimbus-db", &spec, &KrunBundleOptions::default())
            .expect_err("zero host ports should be rejected");

        assert!(
            error
                .to_string()
                .contains("host_port must be greater than zero"),
            "expected actionable host port validation error, got: {error}"
        );
    }

    #[test]
    fn bundle_config_rejects_zero_guest_port() {
        let spec = sample_spec_with_rootfs(Path::new("/srv/rootfs"))
            .with_port_binding(SandboxPortBinding::tcp("invalid-guest", 18080, 0));

        let error = build_bundle_config("nimbus-db", &spec, &KrunBundleOptions::default())
            .expect_err("zero guest ports should be rejected");

        assert!(
            error
                .to_string()
                .contains("guest_port must be greater than zero"),
            "expected actionable guest port validation error, got: {error}"
        );
    }

    fn sample_spec() -> SandboxSpec {
        sample_spec_with_rootfs(Path::new("/srv/rootfs"))
    }

    fn sample_spec_with_rootfs(rootfs: &Path) -> SandboxSpec {
        SandboxSpec::new(
            TenantId::new("tenant").expect("tenant id should be valid"),
            SandboxOwnerSpec::service("db"),
            SandboxBackendKind::Krun,
            SandboxRootSpec::Rootfs(SandboxRootfsSpec::new(rootfs)),
            SandboxProcessSpec::new(["/usr/bin/postgres", "-D", "/var/lib/postgresql/data"])
                .with_env(["PATH=/usr/bin", "PGDATA=/var/lib/postgresql/data"]),
        )
        .with_port_bindings([
            SandboxPortBinding::new("postgres", PublishedEndpointProtocol::Tcp, 15432, 5432),
            SandboxPortBinding::new("health", PublishedEndpointProtocol::Http, 18080, 8080),
        ])
    }
}
