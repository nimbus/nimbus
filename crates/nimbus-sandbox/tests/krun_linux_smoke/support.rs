use super::*;
use std::collections::{BTreeSet, VecDeque};
use std::ffi::OsString;
use std::path::{Component, Path};

use ulid::Ulid;

pub(super) use super::provision_support::{ExactTeardownFixture, provision_krun, retire_krun};

pub(super) fn smoke_backend_config(
    bundle_root: PathBuf,
    state_root: PathBuf,
) -> KrunSandboxBackendConfig {
    let mut config = KrunSandboxBackendConfig::default();
    config.bundle_root = bundle_root;
    config.workload_state_root = state_root.clone();
    config.network_state_root = state_root;
    config.start_mode = KrunStartMode::Execute;

    if let Some(runtime_path) = env::var_os("NIMBUS_KRUN_SMOKE_RUNTIME") {
        config.runtime_path = runtime_path.into();
    }
    if let Some(conmon_path) = env::var_os("NIMBUS_KRUN_SMOKE_CONMON") {
        config.conmon_path = conmon_path.into();
    }
    if let Some(buildah_path) = env::var_os("NIMBUS_KRUN_SMOKE_BUILDAH") {
        config.buildah_path = buildah_path.into();
    }
    if let Some(helper_root) = env::var_os("NIMBUS_KRUN_GUEST_USER_HELPER_ROOT") {
        config.guest_user_helper_root = helper_root.into();
    }
    if let Some(supernet) = env::var_os("NIMBUS_KRUN_SMOKE_NODE_NETWORK_SUPERNET") {
        let supernet = supernet
            .into_string()
            .expect("NIMBUS_KRUN_SMOKE_NODE_NETWORK_SUPERNET must be valid UTF-8");
        assert!(
            !supernet.trim().is_empty(),
            "NIMBUS_KRUN_SMOKE_NODE_NETWORK_SUPERNET cannot be empty"
        );
        config.node_network_supernet = supernet;
    }
    config.netavark_path = env::var_os("NIMBUS_NETAVARK")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_existing_path("/usr/lib/podman/netavark", "netavark"));
    config.aardvark_dns_path = env::var_os("NIMBUS_AARDVARK_DNS")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_existing_path("/usr/lib/podman/aardvark-dns", "aardvark-dns"));

    require_smoke_executable("krun runtime", &config.runtime_path);
    require_smoke_executable("conmon", &config.conmon_path);
    require_smoke_executable("netavark", &config.netavark_path);
    require_smoke_executable("aardvark-dns", &config.aardvark_dns_path);

    config
}

fn require_smoke_executable(label: &str, configured: &std::path::Path) {
    let resolved = if configured.components().count() > 1 {
        configured.to_path_buf()
    } else {
        std::env::var_os("PATH")
            .and_then(|path| {
                std::env::split_paths(&path)
                    .map(|directory| directory.join(configured))
                    .find(|candidate| candidate.is_file())
            })
            .unwrap_or_else(|| configured.to_path_buf())
    };
    let metadata = std::fs::metadata(&resolved).unwrap_or_else(|error| {
        panic!(
            "Linux smoke {label} {} is not a readable executable file: {error}",
            resolved.display()
        )
    });
    assert!(
        metadata.is_file(),
        "Linux smoke {label} {} is not a regular file",
        resolved.display()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert!(
            metadata.permissions().mode() & 0o111 != 0,
            "Linux smoke {label} {} is not executable",
            resolved.display()
        );
    }
}

fn default_existing_path(preferred: &str, fallback: &str) -> PathBuf {
    let preferred = PathBuf::from(preferred);
    if preferred.exists() {
        preferred
    } else {
        PathBuf::from(fallback)
    }
}

#[test]
fn smoke_helper_uses_an_existing_preferred_tool_path() {
    let temp = tempfile::tempdir().expect("tool-path fixture should build");
    let preferred = temp.path().join("netavark");
    std::fs::write(&preferred, b"fixture").expect("tool-path fixture should write");
    let preferred_text = preferred.to_string_lossy();

    assert_eq!(
        default_existing_path(&preferred_text, "netavark"),
        preferred
    );
    assert_eq!(
        default_existing_path("/definitely/missing/nimbus-netavark", "netavark"),
        PathBuf::from("netavark")
    );
}

pub(super) fn sandbox_tenant() -> TenantId {
    TenantId::new("tenant").expect("tenant id should be valid")
}

pub(super) fn http_binding(host_port: u16, guest_port: u16) -> SandboxPortBinding {
    SandboxPortBinding::new("http", EndpointProtocol::Http, host_port, guest_port)
}

pub(super) fn smoke_host_port(default: u16) -> u16 {
    let Some(offset) = env_u16("NIMBUS_KRUN_SMOKE_HOST_PORT_OFFSET") else {
        return default;
    };
    default.checked_add(offset).unwrap_or_else(|| {
        panic!("NIMBUS_KRUN_SMOKE_HOST_PORT_OFFSET={offset} overflows default host port {default}")
    })
}

pub(super) fn rootfs_spec(name: &str, rootfs: impl Into<PathBuf>) -> SandboxSpec {
    SandboxSpec::new(
        sandbox_tenant(),
        SandboxOwnerSpec::standalone_named(name),
        SandboxBackendKind::Krun,
        SandboxRootSpec::rootfs(rootfs),
        SandboxProcessSpec::new(Vec::<String>::new()),
    )
}

pub(super) fn image_spec(name: &str, image_reference: impl Into<String>) -> SandboxSpec {
    SandboxSpec::new(
        sandbox_tenant(),
        SandboxOwnerSpec::standalone_named(name),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image_reference(image_reference),
        SandboxProcessSpec::new(Vec::<String>::new()),
    )
}

pub(super) fn built_busybox_image_spec(
    name: &str,
    image_name: &str,
    dockerfile_metadata: &str,
) -> SandboxSpec {
    let base_dir = env_path("NIMBUS_KRUN_SMOKE_WORKDIR");
    let build_contexts = base_dir.join("build-contexts");
    std::fs::create_dir_all(&build_contexts)
        .expect("the Linux smoke build-context root should be created");
    let run_id = Ulid::new().to_string().to_ascii_lowercase();
    let context_dir = build_contexts.join(format!("{name}-{run_id}"));
    std::fs::create_dir(&context_dir)
        .expect("the Linux smoke build context must be fresh and unique");

    let fixture_rootfs = env_path("NIMBUS_KRUN_SMOKE_ROOTFS");
    std::fs::copy(
        fixture_rootfs.join("bin/busybox"),
        context_dir.join("busybox"),
    )
    .expect("the Linux smoke build context should copy BusyBox");
    std::fs::copy(
        fixture_rootfs.join("etc/passwd"),
        context_dir.join("passwd"),
    )
    .expect("the Linux smoke build context should copy passwd");
    let runtime_libraries = context_dir.join("runtime-libraries");
    let mut runtime_copy_instructions = String::new();
    for library_root in ["lib", "lib64"] {
        let source = fixture_rootfs.join(library_root);
        match std::fs::symlink_metadata(&source) {
            Ok(_) => {
                copy_smoke_tree(
                    &fixture_rootfs,
                    &source,
                    &runtime_libraries.join(library_root),
                )
                .unwrap_or_else(|error| {
                    panic!("the Linux smoke runtime-library tree must be safe: {error}")
                });
                runtime_copy_instructions.push_str(&format!(
                    "COPY runtime-libraries/{library_root}/ /{library_root}/\n"
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!(
                "the Linux smoke runtime-library root {} must be inspectable: {error}",
                source.display()
            ),
        }
    }

    let dockerfile_path = context_dir.join("Dockerfile");
    std::fs::write(
        &dockerfile_path,
        format!(
            "FROM scratch\nCOPY busybox /bin/busybox\nCOPY passwd /etc/passwd\n{runtime_copy_instructions}{dockerfile_metadata}\n"
        ),
    )
    .expect("the Linux smoke Dockerfile should be written");

    SandboxSpec::new(
        sandbox_tenant(),
        SandboxOwnerSpec::standalone_named(name),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image_build(
            format!("{image_name}-{run_id}"),
            dockerfile_path,
            context_dir,
        ),
        SandboxProcessSpec::new(Vec::<String>::new()),
    )
}

fn copy_smoke_tree(rootfs: &Path, source: &Path, destination: &Path) -> Result<(), String> {
    let relative_source = source.strip_prefix(rootfs).map_err(|_| {
        format!(
            "source {} is outside fixture root {}",
            source.display(),
            rootfs.display()
        )
    })?;
    let canonical_root = std::fs::canonicalize(rootfs).map_err(|error| {
        format!(
            "failed to resolve fixture root {}: {error}",
            rootfs.display()
        )
    })?;
    let source = canonical_root.join(relative_source);
    let mut active_directories = BTreeSet::new();
    copy_smoke_tree_inner(
        &canonical_root,
        &source,
        destination,
        &mut active_directories,
    )
}

fn copy_smoke_tree_inner(
    rootfs: &Path,
    source: &Path,
    destination: &Path,
    active_directories: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let source = resolve_smoke_rootfs_path(rootfs, source)?;
    let metadata = std::fs::symlink_metadata(&source)
        .map_err(|error| format!("failed to inspect {}: {error}", source.display()))?;
    if !metadata.is_dir() {
        return Err(format!(
            "runtime-library root {} is not a directory",
            source.display()
        ));
    }
    if !active_directories.insert(source.clone()) {
        return Err(format!(
            "runtime-library directory link cycle reaches {}",
            source.display()
        ));
    }

    let result = (|| {
        std::fs::create_dir_all(destination).map_err(|error| {
            format!(
                "failed to create runtime-library directory {}: {error}",
                destination.display()
            )
        })?;
        let entries = std::fs::read_dir(&source).map_err(|error| {
            format!(
                "failed to read runtime-library directory {}: {error}",
                source.display()
            )
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                format!(
                    "failed to read an entry under runtime-library directory {}: {error}",
                    source.display()
                )
            })?;
            let source_entry = resolve_smoke_rootfs_path(rootfs, &entry.path())?;
            let destination_entry = destination.join(entry.file_name());
            let metadata = std::fs::symlink_metadata(&source_entry).map_err(|error| {
                format!(
                    "failed to inspect runtime-library entry {}: {error}",
                    source_entry.display()
                )
            })?;
            if metadata.is_dir() {
                copy_smoke_tree_inner(
                    rootfs,
                    &source_entry,
                    &destination_entry,
                    active_directories,
                )?;
            } else if metadata.is_file() {
                std::fs::copy(&source_entry, &destination_entry).map_err(|error| {
                    format!(
                        "failed to copy runtime-library entry {} to {}: {error}",
                        source_entry.display(),
                        destination_entry.display()
                    )
                })?;
            } else {
                return Err(format!(
                    "unsupported runtime-library entry {}",
                    source_entry.display()
                ));
            }
        }
        Ok(())
    })();
    active_directories.remove(&source);
    result
}

fn resolve_smoke_rootfs_path(rootfs: &Path, path: &Path) -> Result<PathBuf, String> {
    let relative = path.strip_prefix(rootfs).map_err(|_| {
        format!(
            "runtime-library path {} escapes fixture root {}",
            path.display(),
            rootfs.display()
        )
    })?;
    let mut pending = VecDeque::new();
    prepend_smoke_path_components(relative, &mut pending)?;
    let mut resolved = rootfs.to_path_buf();
    let mut followed_links = 0_u8;

    while let Some(component) = pending.pop_front() {
        if component == "." {
            continue;
        }
        if component == ".." {
            if resolved == rootfs {
                return Err(format!(
                    "runtime-library path {} escapes fixture root {}",
                    path.display(),
                    rootfs.display()
                ));
            }
            resolved.pop();
            continue;
        }

        let candidate = resolved.join(&component);
        let metadata = std::fs::symlink_metadata(&candidate)
            .map_err(|error| format!("failed to inspect {}: {error}", candidate.display()))?;
        if metadata.file_type().is_symlink() {
            followed_links = followed_links.saturating_add(1);
            if followed_links > 64 {
                return Err(format!(
                    "runtime-library symlink chain from {} exceeds 64 links",
                    path.display()
                ));
            }
            let target = std::fs::read_link(&candidate).map_err(|error| {
                format!("failed to read symlink {}: {error}", candidate.display())
            })?;
            if target.is_absolute() {
                resolved = rootfs.to_path_buf();
            }
            prepend_smoke_path_components(&target, &mut pending)?;
        } else {
            resolved = candidate;
        }
    }
    Ok(resolved)
}

fn prepend_smoke_path_components(
    path: &Path,
    pending: &mut VecDeque<OsString>,
) -> Result<(), String> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) => {
                return Err(format!(
                    "runtime-library path {} has an unsupported platform prefix",
                    path.display()
                ));
            }
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => components.push(OsString::from("..")),
            Component::Normal(component) => components.push(component.to_os_string()),
        }
    }
    for component in components.into_iter().rev() {
        pending.push_front(component);
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn smoke_tree_resolves_fixture_relative_and_absolute_symlinks_inside_root() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("smoke-tree fixture should build");
    let rootfs = temp.path().join("rootfs");
    let source = rootfs.join("lib");
    std::fs::create_dir_all(&source).expect("fixture library root should build");
    std::fs::write(source.join("libreal.so"), b"library").expect("fixture library should write");
    symlink("libreal.so", source.join("librelative.so"))
        .expect("relative fixture symlink should build");
    symlink("/lib/libreal.so", source.join("libabsolute.so"))
        .expect("absolute fixture symlink should build");
    let destination = temp.path().join("context/lib");

    copy_smoke_tree(&rootfs, &source, &destination)
        .expect("safe rootfs links should resolve inside the fixture");

    for name in ["libreal.so", "librelative.so", "libabsolute.so"] {
        assert_eq!(
            std::fs::read(destination.join(name)).expect("copied library should read"),
            b"library",
            "{name} must come from the fixture root"
        );
    }
}

#[test]
#[cfg(unix)]
fn smoke_tree_rejects_a_relative_symlink_that_escapes_the_fixture_root() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("smoke-tree fixture should build");
    let rootfs = temp.path().join("rootfs");
    let source = rootfs.join("lib");
    std::fs::create_dir_all(&source).expect("fixture library root should build");
    std::fs::write(temp.path().join("host-secret"), b"do not copy")
        .expect("outside sentinel should write");
    symlink("../../host-secret", source.join("escape.so"))
        .expect("escaping fixture symlink should build");

    let error = copy_smoke_tree(&rootfs, &source, &temp.path().join("context/lib"))
        .expect_err("an escaping rootfs link must fail closed");
    assert!(
        error.contains("escapes fixture root"),
        "diagnostic must identify the rootfs escape: {error}"
    );
}

pub(super) fn busybox_http_process(guest_port: u16) -> SandboxProcessSpec {
    SandboxProcessSpec::new(Vec::<String>::new()).with_command([
        "/bin/busybox".into(),
        "httpd".into(),
        "-f".into(),
        "-p".into(),
        guest_port.to_string(),
    ])
}

pub(super) fn assert_httpish_response(response: &str, context: &str) {
    assert!(
        response.starts_with("HTTP/1.") || response.contains("404"),
        "{context}, got: {response}"
    );
}

pub(super) fn assert_host_port_not_bound_to_non_loopback(port: u16) {
    let Some(host_address) = non_loopback_host_address() else {
        eprintln!("skipping non-loopback bind probe because no host address was discovered");
        return;
    };
    let probe_address = std::net::SocketAddr::new(host_address, port);

    match TcpStream::connect_timeout(&probe_address, Duration::from_secs(2)) {
        Ok(_) => panic!(
            "krun host port {port} accepted a connection on non-loopback address {host_address}"
        ),
        Err(error) => eprintln!("non-loopback bind probe {probe_address}: {error}"),
    }
}

fn non_loopback_host_address() -> Option<std::net::IpAddr> {
    if let Some(configured_address) = env::var("NIMBUS_KRUN_SMOKE_NON_LOOPBACK_HOST")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        let parsed = configured_address.parse::<std::net::IpAddr>().unwrap_or_else(|error| {
            panic!(
                "failed to parse NIMBUS_KRUN_SMOKE_NON_LOOPBACK_HOST={configured_address:?}: {error}"
            )
        });
        assert!(
            !parsed.is_loopback() && !parsed.is_unspecified(),
            "NIMBUS_KRUN_SMOKE_NON_LOOPBACK_HOST must be a non-loopback host address"
        );
        return Some(parsed);
    }

    let output = std::process::Command::new("hostname")
        .arg("-I")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .filter_map(|candidate| candidate.parse::<std::net::IpAddr>().ok())
        .find(|address| !address.is_loopback() && !address.is_unspecified())
}

pub(super) fn bundle_config_path(
    bundle_root: &std::path::Path,
    tenant_id: &TenantId,
    sandbox_id: &nimbus_sandbox::SandboxId,
) -> PathBuf {
    bundle_root
        .join("tenants")
        .join(tenant_id.as_str())
        .join("sandboxes")
        .join(sandbox_id.as_str())
        .join("bundle")
        .join("config.json")
}

pub(super) fn container_state_dir(
    state_root: &std::path::Path,
    tenant_id: &TenantId,
    sandbox_id: &nimbus_sandbox::SandboxId,
) -> PathBuf {
    state_root
        .join("tenants")
        .join(tenant_id.as_str())
        .join("sandboxes")
        .join(sandbox_id.as_str())
        .join("state")
        .join("containers")
        .join(sandbox_id.as_str())
}

pub(super) fn manifest_path(
    state_root: &std::path::Path,
    tenant_id: &TenantId,
    sandbox_id: &nimbus_sandbox::SandboxId,
) -> PathBuf {
    container_state_dir(state_root, tenant_id, sandbox_id).join("manifest.json")
}

pub(super) fn wait_for_ready(
    backend: &KrunSandboxBackend,
    id: &nimbus_sandbox::SandboxId,
    timeout: Duration,
) -> nimbus_sandbox::SandboxHandle {
    wait_for_status(backend, id, SandboxStatus::Ready, timeout)
}

pub(super) fn wait_for_status(
    backend: &KrunSandboxBackend,
    id: &nimbus_sandbox::SandboxId,
    expected: SandboxStatus,
    timeout: Duration,
) -> nimbus_sandbox::SandboxHandle {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(inspection) = block_on(backend.inspect(id))
            .expect("inspect should succeed")
            .filter(|inspection| inspection.handle.status == expected)
        {
            return inspection.handle;
        }
        thread::sleep(Duration::from_millis(250));
    }

    panic!("sandbox did not reach {expected:?} within {:?}", timeout);
}

pub(super) fn wait_for_unavailable(
    backend: &KrunSandboxBackend,
    id: &nimbus_sandbox::SandboxId,
    timeout: Duration,
) -> nimbus_sandbox::SandboxHandle {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(inspection) = block_on(backend.inspect(id)).expect("inspect should succeed")
            && matches!(
                inspection.handle.status,
                SandboxStatus::Starting | SandboxStatus::NotReady
            )
        {
            return inspection.handle;
        }
        thread::sleep(Duration::from_millis(250));
    }

    panic!("sandbox did not become unavailable within {timeout:?}");
}

pub(super) fn wait_for_http_response(port: u16, timeout: Duration) -> String {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
            Ok(mut stream) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .expect("read timeout should be settable");
                stream
                    .write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n")
                    .expect("HTTP probe should be writable");

                let mut response = vec![0u8; 4096];
                match stream.read(&mut response) {
                    Ok(n) if n > 0 => {
                        let text = String::from_utf8_lossy(&response[..n]).to_string();
                        return text;
                    }
                    Ok(_) => eprintln!("HTTP probe connected but got empty response"),
                    Err(error) => eprintln!("HTTP probe read error: {error}"),
                }
            }
            Err(error) => {
                eprintln!("HTTP probe connect error on port {port}: {error}");
            }
        }
        thread::sleep(Duration::from_millis(500));
    }

    panic!(
        "guest service did not answer HTTP on port {port} within {:?}",
        timeout
    );
}

pub(super) fn wait_for_http_unreachable(port: u16, timeout: Duration) {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match TcpStream::connect_timeout(&addr, Duration::from_secs(1)) {
            Ok(mut stream) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                if stream
                    .write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n")
                    .is_err()
                {
                    return;
                }

                let mut response = [0u8; 256];
                match stream.read(&mut response) {
                    Ok(0) => return,
                    Err(_) => return,
                    Ok(_) => {}
                }
            }
            Err(_) => return,
        }
        thread::sleep(Duration::from_millis(250));
    }

    panic!(
        "guest service on port {port} remained reachable for {:?}",
        timeout
    );
}

pub(super) fn env_path(key: &str) -> PathBuf {
    env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("expected environment variable {key} to be set"))
}

pub(super) fn env_u16(key: &str) -> Option<u16> {
    env::var(key).ok().map(|value| {
        value
            .parse::<u16>()
            .unwrap_or_else(|error| panic!("failed to parse {key}={value:?} as u16: {error}"))
    })
}

pub(super) struct CleanupGuard {
    backend: KrunSandboxBackend,
    teardown: Option<ExactTeardownFixture>,
}

impl CleanupGuard {
    pub(super) fn new(backend: KrunSandboxBackend, teardown: ExactTeardownFixture) -> Self {
        Self {
            backend,
            teardown: Some(teardown),
        }
    }

    pub(super) fn disarm(mut self) {
        self.teardown = None;
    }
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        if let Some(teardown) = self.teardown.take() {
            let _ = retire_krun(&self.backend, &teardown);
        }
    }
}
