use std::collections::BTreeSet;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::backends::oci::egress::{egress_proxy_env_entries, scrub_reserved_egress_env};
use crate::backends::oci::hardening::{masked_paths_json, readonly_paths_json};
use crate::error::{Result, SandboxError};
use crate::spec::{SandboxPortBinding, SandboxProcessSpec, SandboxResourceLimits, SandboxSpec};

const DEFAULT_PATH_ENV: &str = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
const MIN_MEMORY_LIMIT_BYTES: u64 = 1024 * 1024;
/// Linux capability allowlist granted to the krun VMM (crun) process.
///
/// Deny-by-default and deliberately minimal: the VMM runs confined inside a
/// host-created, netavark-configured deny-by-default network namespace, so it
/// is granted only what a tap-less libkrun TSI microVM provably needs.
///
/// - `CAP_NET_BIND_SERVICE`: bind published service ports (including <1024).
/// - `CAP_SYS_ADMIN`: mount the guest rootfs and pseudo-filesystems inside the
///   VMM's private mount namespace.
///
/// `CAP_NET_ADMIN` is intentionally EXCLUDED. crun only *joins* an
/// already-configured netns (it never creates or configures interfaces,
/// routes, or nftables itself — `configure_network` does all of that on the
/// host before the VMM launches), and libkrun TSI gives the guest no in-netns
/// tap device for the VMM to manage. There is therefore no path in which a
/// tap-less TSI microVM needs CAP_NET_ADMIN. Granting it would instead hand the
/// confined VMM exactly the privilege to add a default route or flush the netns
/// deny chain that pins its egress to the host-side PEP — the bypass this audit
/// closes. `CAP_NET_RAW`/`CAP_NET_BROADCAST` are likewise excluded (raw sockets
/// bypass the netns+PEP seam); see the `bundle_config_excludes_*` invariants.
const KRUN_REQUIRED_CAPABILITIES: &[&str] = &["CAP_NET_BIND_SERVICE", "CAP_SYS_ADMIN"];
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
    /// Host-side egress PEP URL (`http://<bridge-gateway>:<port>`) for an
    /// execute-mode launch with an egress-proxy assignment. When present, the
    /// guest env is pointed at this PEP via the shared container-shape proxy
    /// env; when absent (plan-only / no assignment) no proxy env is injected.
    pub egress_proxy_url: Option<String>,
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
    network_namespace_path: Option<&Path>,
    options: &KrunBundleOptions,
) -> Result<()> {
    std::fs::create_dir_all(&layout.bundle_dir).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to create krun bundle directory {}: {error}",
            layout.bundle_dir.display()
        ),
    })?;

    let config = build_bundle_config(hostname, spec, network_namespace_path, options)?;
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
    network_namespace_path: Option<&Path>,
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
    // Fail closed at bundle build if the egress policy is malformed. The policy
    // itself is enforced at the host by the per-sandbox egress PEP (see
    // `EgressProxyRegistry`); the guest is never handed a cooperative copy of it.
    spec.egress
        .compile()
        .map_err(|message| SandboxError::InvalidSpec { message })?;
    let process_env = process_env(spec, options.egress_proxy_url.as_deref());

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
    // The krun VMM (crun) joins a host-created deny-by-default network namespace
    // when one is supplied. Under libkrun TSI the guest's connect()/sendto() are
    // host sockets issued by this VMM process, so confining the VMM to a netns
    // confines the guest's egress to the namespace's only outbound path: the
    // host-side egress PEP bound on the bridge gateway.
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
    linux.insert("seccomp".to_owned(), krun_seccomp_profile());
    // OCI default-spec mount-namespace hardening: mask the sensitive host-kernel
    // /proc and /sys surfaces and mark the /proc control surfaces read-only.
    // Shared with the container backend so neither can drift to a weaker posture.
    linux.insert("maskedPaths".to_owned(), masked_paths_json());
    linux.insert("readonlyPaths".to_owned(), readonly_paths_json());

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

fn process_env(spec: &SandboxSpec, egress_proxy_url: Option<&str>) -> Vec<String> {
    let mut env = if spec.process.env.is_empty() {
        vec![DEFAULT_PATH_ENV.to_owned()]
    } else {
        spec.process.env.clone()
    };
    // Scrub every reserved egress key so a tenant-supplied proxy override can
    // never survive into the guest, then point the guest at the host-side PEP.
    scrub_reserved_egress_env(&mut env);
    if let Some(egress_proxy_url) = egress_proxy_url {
        env.extend(egress_proxy_env_entries(egress_proxy_url));
    }
    env
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
    if resources.disk_limit_bytes.is_some() {
        return Err(SandboxError::InvalidSpec {
            message: "krun sandbox disk_limit_bytes is not enforceable: the writable surface is a host bind-mount and OCI linux.resources has no total-disk-capacity control".to_owned(),
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
        KRUN_REQUIRED_CAPABILITIES, KrunBundleLayout, KrunBundleMount, KrunBundleOptions,
        build_bundle_config, format_port_map, write_bundle_config,
    };
    use crate::backend::SandboxBackendKind;
    use crate::backends::oci::hardening::{DEFAULT_MASKED_PATHS, DEFAULT_READONLY_PATHS};
    use crate::endpoint::PublishedEndpointProtocol;
    use crate::spec::{
        SandboxMountSource, SandboxMountSpec, SandboxOwnerSpec, SandboxPortBinding,
        SandboxProcessSpec, SandboxResourceLimits, SandboxRootSpec, SandboxRootfsSpec, SandboxSpec,
    };
    use nimbus_egress::{
        EGRESS_ENFORCEMENT_ENV, EGRESS_LEGACY_POLICY_ENV, EGRESS_PROXY_URL_ENV,
        EGRESS_RESERVED_ENV_KEYS, EgressPolicy, EgressProtocol, EgressRule,
    };

    fn env_from_config(config: &serde_json::Value) -> Vec<&str> {
        config["process"]["env"]
            .as_array()
            .expect("env should be an array")
            .iter()
            .map(|value| value.as_str().expect("env entries should be strings"))
            .collect()
    }

    #[test]
    fn bundle_config_sets_krun_handler_and_port_map() {
        let spec = sample_spec();
        let config = build_bundle_config("nimbus-db", &spec, None, &KrunBundleOptions::default())
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
    fn bundle_config_includes_network_namespace_when_supplied() {
        let spec = sample_spec();
        let netns_path = Path::new("/run/nimbus/netns/krun-egress-01");
        let config = build_bundle_config(
            "nimbus-db",
            &spec,
            Some(netns_path),
            &KrunBundleOptions::default(),
        )
        .expect("bundle config should build");
        let namespaces = config["linux"]["namespaces"]
            .as_array()
            .expect("linux.namespaces should be an array");

        let network_namespace = namespaces
            .iter()
            .find(|namespace| namespace["type"] == "network")
            .expect(
                "krun bundles must carry the network namespace so the VMM joins the deny-by-default netns",
            );
        assert_eq!(
            network_namespace["path"],
            netns_path.to_string_lossy().as_ref(),
            "the network namespace entry must point at the host-created netns path"
        );
    }

    #[test]
    fn bundle_config_has_no_netns_entry_when_path_absent() {
        let spec = sample_spec();
        let config = build_bundle_config("nimbus-db", &spec, None, &KrunBundleOptions::default())
            .expect("bundle config should build");
        let namespaces = config["linux"]["namespaces"]
            .as_array()
            .expect("linux.namespaces should be an array");

        assert!(
            namespaces
                .iter()
                .all(|namespace| namespace["type"] != "network"),
            "krun bundles must omit the network namespace when no netns path is supplied"
        );
    }

    #[test]
    fn write_bundle_config_materializes_config_json() {
        let temp_dir = TempDir::new().expect("temporary directory should be created");
        let layout = KrunBundleLayout::new(temp_dir.path().join("bundle"));
        let spec = sample_spec();

        write_bundle_config(
            &layout,
            "nimbus-db",
            &spec,
            None,
            &KrunBundleOptions::default(),
        )
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
        let config = build_bundle_config("nimbus-db", &spec, None, &KrunBundleOptions::default())
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
        let config = build_bundle_config("nimbus-db", &spec, None, &KrunBundleOptions::default())
            .expect("bundle config should build");

        assert_eq!(
            config["process"]["noNewPrivileges"], true,
            "krun VMM process should not be able to gain new privileges through exec"
        );
    }

    #[test]
    fn bundle_config_injects_assigned_egress_proxy_env_and_scrubs_spoofed_values() {
        // A real allow policy plus tenant-spoofed proxy/enforcement env. The
        // host-enforced model: the guest is pointed at the assigned PEP and the
        // spoofed values are scrubbed; no guest-cooperative contract is handed in.
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
            "HTTP_PROXY=http://attacker.invalid:1".to_owned(),
            "https_proxy=http://attacker.invalid:2".to_owned(),
            format!("{EGRESS_ENFORCEMENT_ENV}={{\"schema_version\":0}}"),
            format!("{EGRESS_LEGACY_POLICY_ENV}={{\"allow\":[]}}"),
            format!("{EGRESS_PROXY_URL_ENV}=http://attacker.invalid:4"),
        ];

        let config = build_bundle_config(
            "nimbus-db",
            &spec,
            None,
            &KrunBundleOptions {
                egress_proxy_url: Some("http://10.89.0.1:15000".to_owned()),
                ..Default::default()
            },
        )
        .expect("bundle config should build");
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
            "tenant-provided proxy env must be scrubbed: {env:?}"
        );
        // The retired guest-cooperative supervisor-proxy contract is gone: the
        // krun bundle never emits an egress enforcement plan for the guest to
        // self-enforce. Egress is host-enforced by the PEP the proxy env names.
        let enforcement_prefix = format!("{EGRESS_ENFORCEMENT_ENV}=");
        assert!(
            env.iter()
                .all(|entry| !entry.starts_with(&enforcement_prefix)),
            "krun bundles must not carry the guest-cooperative egress enforcement contract: {env:?}"
        );
        let legacy_policy_prefix = format!("{EGRESS_LEGACY_POLICY_ENV}=");
        assert!(
            env.iter()
                .all(|entry| !entry.starts_with(&legacy_policy_prefix)),
            "krun bundles must scrub the spoofed legacy egress policy env: {env:?}"
        );
    }

    #[test]
    fn bundle_config_routes_default_deny_through_assigned_proxy_without_guest_contract() {
        // Even a default deny-all policy routes the guest through its assigned
        // PEP (the PEP enforces deny-all); no enforcement contract reaches the guest.
        let config = build_bundle_config(
            "nimbus-db",
            &sample_spec(),
            None,
            &KrunBundleOptions {
                egress_proxy_url: Some("http://10.89.0.1:15001".to_owned()),
                ..Default::default()
            },
        )
        .expect("bundle config should build");
        let env = env_from_config(&config);

        for expected in [
            "HTTP_PROXY=http://10.89.0.1:15001",
            "HTTPS_PROXY=http://10.89.0.1:15001",
            "NO_PROXY=",
        ] {
            assert!(
                env.contains(&expected),
                "default-deny bundle must still route through the assigned PEP: {env:?}"
            );
        }
        let enforcement_prefix = format!("{EGRESS_ENFORCEMENT_ENV}=");
        assert!(
            env.iter()
                .all(|entry| !entry.starts_with(&enforcement_prefix)),
            "default-deny krun bundle must not emit a guest-cooperative enforcement contract: {env:?}"
        );
    }

    #[test]
    fn bundle_config_without_egress_assignment_injects_no_proxy_env() {
        // Plan-only / no egress-proxy assignment: no proxy env at all is injected,
        // and no guest-cooperative enforcement contract is emitted either.
        let config = build_bundle_config(
            "nimbus-db",
            &sample_spec(),
            None,
            &KrunBundleOptions::default(),
        )
        .expect("bundle config should build");
        let env = env_from_config(&config);

        for reserved in EGRESS_RESERVED_ENV_KEYS {
            let prefix = format!("{reserved}=");
            assert!(
                env.iter().all(|entry| !entry.starts_with(&prefix)),
                "a krun bundle without an egress-proxy assignment must inject no egress env ({reserved}): {env:?}"
            );
        }
    }

    #[test]
    fn bundle_config_rejects_invalid_sandbox_egress_policy() {
        let spec = sample_spec().with_egress_policy(EgressPolicy::new([EgressRule::new(
            "wildcard",
            EgressProtocol::Https,
            "*",
            443,
        )]));

        let error = build_bundle_config("nimbus-db", &spec, None, &KrunBundleOptions::default())
            .expect_err("invalid sandbox egress policy should fail bundle generation");

        assert!(
            error.to_string().contains("wildcards"),
            "bundle generation should expose the invalid egress policy error: {error}"
        );
    }

    #[test]
    fn bundle_config_sets_explicit_krun_capabilities() {
        let spec = sample_spec();
        let config = build_bundle_config("nimbus-db", &spec, None, &KrunBundleOptions::default())
            .expect("bundle config should build");
        let capabilities = &config["process"]["capabilities"];
        let expected = json!(["CAP_NET_BIND_SERVICE", "CAP_SYS_ADMIN"]);

        assert_eq!(capabilities["bounding"], expected);
        assert_eq!(capabilities["effective"], expected);
        assert_eq!(capabilities["permitted"], expected);
        assert_eq!(capabilities["inheritable"], json!([]));
        assert_eq!(capabilities["ambient"], json!([]));
    }

    /// Always-on negative invariant: the krun VMM capability set must never carry
    /// `CAP_NET_RAW` (raw/ICMP/`AF_PACKET` sockets) nor `CAP_NET_BROADCAST`. A
    /// future cap-set edit that reintroduces either — reopening a raw-socket
    /// egress path that bypasses the deny-by-default netns + PEP — fails CI here.
    #[test]
    fn bundle_config_excludes_raw_and_broadcast_socket_capabilities() {
        const FORBIDDEN_CAPABILITIES: &[&str] = &["CAP_NET_RAW", "CAP_NET_BROADCAST"];

        for forbidden in FORBIDDEN_CAPABILITIES {
            assert!(
                !KRUN_REQUIRED_CAPABILITIES.contains(forbidden),
                "KRUN_REQUIRED_CAPABILITIES must never grant {forbidden}: raw/broadcast sockets bypass the netns+PEP egress seam"
            );
        }

        let config = build_bundle_config(
            "nimbus-db",
            &sample_spec(),
            None,
            &KrunBundleOptions::default(),
        )
        .expect("bundle config should build");
        let capabilities = &config["process"]["capabilities"];
        for set in [
            "bounding",
            "effective",
            "permitted",
            "inheritable",
            "ambient",
        ] {
            let granted = capabilities[set]
                .as_array()
                .unwrap_or_else(|| panic!("capability set {set} should be an array"));
            for forbidden in FORBIDDEN_CAPABILITIES {
                assert!(
                    granted.iter().all(|cap| cap.as_str() != Some(*forbidden)),
                    "krun bundle capability set {set} must never contain {forbidden}: {granted:?}"
                );
            }
        }
    }

    /// Always-on negative invariant: the krun VMM capability set must never carry
    /// `CAP_NET_ADMIN`. crun joins a host-configured deny-by-default netns and a
    /// tap-less libkrun TSI microVM exposes no in-netns interface for the VMM to
    /// manage, so CAP_NET_ADMIN is never needed — but it *would* let the confined
    /// VMM add a default route or flush the netns deny chain that pins its egress
    /// to the host PEP. A future cap-set edit that reintroduces it fails CI here.
    #[test]
    fn bundle_config_excludes_cap_net_admin() {
        assert!(
            !KRUN_REQUIRED_CAPABILITIES.contains(&"CAP_NET_ADMIN"),
            "KRUN_REQUIRED_CAPABILITIES must never grant CAP_NET_ADMIN: it permits route/nftables edits that bypass the netns+PEP egress seam"
        );

        let config = build_bundle_config(
            "nimbus-db",
            &sample_spec(),
            None,
            &KrunBundleOptions::default(),
        )
        .expect("bundle config should build");
        let capabilities = &config["process"]["capabilities"];
        for set in [
            "bounding",
            "effective",
            "permitted",
            "inheritable",
            "ambient",
        ] {
            let granted = capabilities[set]
                .as_array()
                .unwrap_or_else(|| panic!("capability set {set} should be an array"));
            assert!(
                granted
                    .iter()
                    .all(|cap| cap.as_str() != Some("CAP_NET_ADMIN")),
                "krun bundle capability set {set} must never contain CAP_NET_ADMIN: {granted:?}"
            );
        }
    }

    /// The krun bundle must carry the OCI default-spec mount-namespace hardening:
    /// the sensitive host-kernel `/proc`/`/sys` surfaces masked and the `/proc`
    /// control surfaces read-only.
    #[test]
    fn bundle_config_sets_oci_masked_and_readonly_paths() {
        let config = build_bundle_config(
            "nimbus-db",
            &sample_spec(),
            None,
            &KrunBundleOptions::default(),
        )
        .expect("bundle config should build");

        let masked = config["linux"]["maskedPaths"]
            .as_array()
            .expect("linux.maskedPaths must be present");
        for required in [
            "/proc/kcore",
            "/proc/keys",
            "/proc/timer_list",
            "/sys/firmware",
        ] {
            assert!(
                masked.iter().any(|path| path.as_str() == Some(required)),
                "krun bundle must mask the sensitive host-kernel surface {required}: {masked:?}"
            );
        }
        assert_eq!(
            masked.len(),
            DEFAULT_MASKED_PATHS.len(),
            "krun bundle must carry the full shared masked-path set"
        );

        let readonly = config["linux"]["readonlyPaths"]
            .as_array()
            .expect("linux.readonlyPaths must be present");
        for required in ["/proc/sys", "/proc/sysrq-trigger"] {
            assert!(
                readonly.iter().any(|path| path.as_str() == Some(required)),
                "krun bundle must mark the /proc control surface {required} read-only: {readonly:?}"
            );
        }
        assert_eq!(
            readonly.len(),
            DEFAULT_READONLY_PATHS.len(),
            "krun bundle must carry the full shared read-only-path set"
        );
    }

    /// AF_UNIX / mount-namespace invariant: a krun bundle must never bind-mount a
    /// host AF_UNIX socket into the guest. The base mount set is all pseudo-fs
    /// (no host path is shared in), and even with tenant-volume binds the only
    /// admitted sources are Nimbus-owned directories — never a `.sock`.
    #[test]
    fn bundle_config_exposes_no_host_socket_mount() {
        // Base bundle: every default mount is a pseudo-filesystem; none is a bind
        // of a host path, so no host socket can ride in on the defaults.
        let base = build_bundle_config(
            "nimbus-db",
            &sample_spec(),
            None,
            &KrunBundleOptions::default(),
        )
        .expect("bundle config should build");
        for mount in base["mounts"].as_array().expect("mounts should be present") {
            assert_ne!(
                mount["type"], "bind",
                "the krun default mount set must not bind any host path: {mount}"
            );
            assert_no_socket_source(mount);
        }

        // Even with an additional bind (the shape `krun_additional_mounts` emits
        // for a resolved tenant volume), the source is a Nimbus-owned directory
        // path mounted nosuid+nodev — never a host AF_UNIX socket.
        let with_volume = build_bundle_config(
            "nimbus-db",
            &sample_spec(),
            None,
            &KrunBundleOptions {
                additional_mounts: vec![KrunBundleMount {
                    destination: "/data".to_owned(),
                    source: Path::new("/var/lib/nimbus/state/tenants/tenant/volumes/cache").into(),
                    options: vec![
                        "rbind".to_owned(),
                        "rw".to_owned(),
                        "nosuid".to_owned(),
                        "nodev".to_owned(),
                    ],
                }],
                ..Default::default()
            },
        )
        .expect("bundle config should build");
        let mounts = with_volume["mounts"]
            .as_array()
            .expect("mounts should be present");
        let volume_mount = mounts
            .iter()
            .find(|mount| mount["destination"] == "/data")
            .expect("the tenant-volume bind mount should be present");
        let options: Vec<&str> = volume_mount["options"]
            .as_array()
            .expect("bind mount options should be present")
            .iter()
            .map(|option| option.as_str().expect("options should be strings"))
            .collect();
        assert!(
            options.contains(&"nosuid") && options.contains(&"nodev"),
            "tenant-volume binds must be nosuid+nodev: {options:?}"
        );
        for mount in mounts {
            assert_no_socket_source(mount);
        }
    }

    /// Construction-time AF_UNIX guard at the type level: a sandbox mount source
    /// can only ever be a Nimbus-owned tenant volume. There is no host-path (let
    /// alone host-socket) mount-source variant, so a tenant can never name a host
    /// AF_UNIX socket as a mount source. This match fails to compile if such a
    /// variant is ever added, forcing a re-audit before any host path can be
    /// shared into a guest.
    #[test]
    fn sandbox_mount_source_admits_only_tenant_volumes() {
        let source = SandboxMountSpec::tenant_volume("cache", "/data").source;
        match source {
            SandboxMountSource::TenantVolume { name } => assert_eq!(name, "cache"),
        }
    }

    fn assert_no_socket_source(mount: &serde_json::Value) {
        if let Some(source) = mount["source"].as_str() {
            assert!(
                !source.ends_with(".sock") && !source.contains(".sock/"),
                "no krun mount source may be a host AF_UNIX socket: {source}"
            );
        }
    }

    #[test]
    fn bundle_config_sets_explicit_seccomp_allowlist() {
        let spec = sample_spec();
        let config = build_bundle_config("nimbus-db", &spec, None, &KrunBundleOptions::default())
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
        let config = build_bundle_config("nimbus-db", &spec, None, &KrunBundleOptions::default())
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
        let config = build_bundle_config("nimbus-db", &spec, None, &KrunBundleOptions::default())
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
            None,
            &KrunBundleOptions {
                additional_mounts: vec![KrunBundleMount {
                    destination: "/.nimbus".to_owned(),
                    source: Path::new("/usr/libexec/nimbus").into(),
                    options: vec!["rbind".to_owned(), "ro".to_owned()],
                }],
                ..Default::default()
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

        let config = build_bundle_config("nimbus-db", &spec, None, &KrunBundleOptions::default())
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

        let error = build_bundle_config("nimbus-db", &spec, None, &KrunBundleOptions::default())
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

        let error = build_bundle_config("nimbus-db", &spec, None, &KrunBundleOptions::default())
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

        let error = build_bundle_config("nimbus-db", &spec, None, &KrunBundleOptions::default())
            .expect_err("zero guest ports should be rejected");

        assert!(
            error
                .to_string()
                .contains("guest_port must be greater than zero"),
            "expected actionable guest port validation error, got: {error}"
        );
    }

    #[test]
    fn bundle_config_rejects_unenforceable_disk_limit() {
        let spec = sample_spec()
            .with_resource_limits(SandboxResourceLimits::default().with_disk_limit_bytes(1024));

        let error = build_bundle_config("nimbus-db", &spec, None, &KrunBundleOptions::default())
            .expect_err("an unenforceable disk_limit_bytes must fail closed");

        assert!(
            error
                .to_string()
                .contains("disk_limit_bytes is not enforceable"),
            "expected actionable disk-limit validation error, got: {error}"
        );

        // An enforceable limit (memory) on the same spec must still build a bundle:
        // the guard is specific to the disk knob crun cannot honor, not a blanket
        // rejection of resource limits.
        let with_memory = sample_spec().with_resource_limits(
            SandboxResourceLimits::default().with_memory_limit_bytes(256 * 1024 * 1024),
        );
        build_bundle_config(
            "nimbus-db",
            &with_memory,
            None,
            &KrunBundleOptions::default(),
        )
        .expect("a spec without disk_limit_bytes should still render a bundle");
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
