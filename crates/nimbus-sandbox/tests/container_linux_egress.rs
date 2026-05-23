#![cfg(target_os = "linux")]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use tempfile::TempDir;

use nimbus_core::TenantId;
use nimbus_sandbox::backends::container::{
    ContainerLaunchMode, ContainerSandboxBackend, ContainerSandboxBackendConfig,
};
use nimbus_sandbox::{
    SandboxBackend, SandboxBackendKind, SandboxFilesystemSpec, SandboxId, SandboxImageLaunchSpec,
    SandboxImageProcessOverrides, SandboxMountSpec, SandboxProcessSpec, SandboxSpec,
};

const RESULT_VOLUME: &str = "egress-proof";
const RESULT_PATH_IN_GUEST: &str = "/nimbus-egress/result";

#[test]
#[ignore = "requires Linux root with conmon, crun, buildah, netavark, aardvark-dns, and OCI image pull access"]
fn container_execute_mode_denies_direct_external_egress() {
    assert_root();

    let temp_dir = TempDir::new().expect("temporary workdir should be created");
    let workdir = env::var_os("NIMBUS_CONTAINER_EGRESS_WORKDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| temp_dir.path().to_path_buf());
    fs::create_dir_all(&workdir).expect("egress smoke workdir should exist");

    let backend = ContainerSandboxBackend::new(smoke_config(&workdir));
    let tenant_id = TenantId::new("egress-proof").expect("tenant id should parse");
    let spec = SandboxSpec::new(
        tenant_id.clone(),
        "deny-direct-egress",
        SandboxBackendKind::Container,
        SandboxFilesystemSpec::new(PathBuf::new()),
        SandboxProcessSpec::new(Vec::<String>::new()),
    )
    .with_mount(SandboxMountSpec::tenant_volume(
        RESULT_VOLUME,
        "/nimbus-egress",
    ));
    let image = env::var("NIMBUS_CONTAINER_EGRESS_IMAGE")
        .unwrap_or_else(|_| "docker.io/library/busybox:latest".to_owned());
    let target =
        env::var("NIMBUS_CONTAINER_EGRESS_TARGET").unwrap_or_else(|_| "http://1.1.1.1".to_owned());
    let command = format!(
        "if wget -T 4 -q -O /tmp/nimbus-egress-body {target:?}; then echo allowed > {RESULT_PATH_IN_GUEST}; else echo denied > {RESULT_PATH_IN_GUEST}; fi; sleep 30"
    );

    let handle = block_on(
        backend.start_from_image(
            SandboxImageLaunchSpec::new(spec, image).with_process_overrides(
                SandboxImageProcessOverrides::default()
                    .with_entrypoint(["/bin/sh", "-c"])
                    .with_cmd([command]),
            ),
        ),
    )
    .expect("container should start");
    let cleanup = CleanupGuard::new(backend.clone(), handle.id.clone(), tenant_id.clone());

    let result_path = tenant_volume_path(&workdir, &tenant_id, RESULT_VOLUME).join("result");
    let result = wait_for_result(&result_path, Duration::from_secs(20));

    block_on(backend.stop(&handle.id))
        .expect("container should stop and release network artifacts");
    block_on(backend.remove_tenant_artifacts(tenant_id))
        .expect("tenant artifacts should clean up after proof");
    cleanup.disarm();

    assert_eq!(
        result.trim(),
        "denied",
        "direct guest egress should be denied by the netavark internal bridge"
    );
}

fn smoke_config(workdir: &Path) -> ContainerSandboxBackendConfig {
    let mut config = ContainerSandboxBackendConfig::under_root(workdir);
    config.launch_mode = ContainerLaunchMode::Execute;
    config.conmon_path = default_existing_path("/usr/bin/conmon", "conmon");
    config.runtime_path = default_existing_path("/usr/bin/crun", "crun");
    config.buildah_path = default_existing_path("/usr/bin/buildah", "buildah");
    config.netavark_path = env::var_os("NIMBUS_NETAVARK")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_existing_path("/usr/lib/podman/netavark", "netavark"));
    config.aardvark_dns_path = env::var_os("NIMBUS_AARDVARK_DNS")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_existing_path("/usr/lib/podman/aardvark-dns", "aardvark-dns"));
    config.use_buildah_unshare = false;
    config.start_timeout = Duration::from_secs(30);
    config
}

fn default_existing_path(preferred: &str, fallback: &str) -> PathBuf {
    let preferred = PathBuf::from(preferred);
    if preferred.exists() {
        preferred
    } else {
        PathBuf::from(fallback)
    }
}

fn tenant_volume_path(workdir: &Path, tenant_id: &TenantId, volume_name: &str) -> PathBuf {
    workdir
        .join("state")
        .join("tenants")
        .join(tenant_id.as_str())
        .join("volumes")
        .join(volume_name)
}

fn wait_for_result(path: &Path, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(contents) = fs::read_to_string(path) {
            return contents;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!(
        "timed out waiting for direct-egress proof result at {}",
        path.display()
    );
}

fn assert_root() {
    // SAFETY: geteuid has no preconditions and does not dereference pointers.
    let effective_uid = unsafe { libc::geteuid() };
    assert_eq!(
        effective_uid, 0,
        "container egress smoke test must run as root so it can create network namespaces"
    );
}

struct CleanupGuard {
    backend: ContainerSandboxBackend,
    sandbox_id: SandboxId,
    tenant_id: Option<TenantId>,
}

impl CleanupGuard {
    fn new(backend: ContainerSandboxBackend, sandbox_id: SandboxId, tenant_id: TenantId) -> Self {
        Self {
            backend,
            sandbox_id,
            tenant_id: Some(tenant_id),
        }
    }

    fn disarm(mut self) {
        self.tenant_id = None;
    }
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        let Some(tenant_id) = self.tenant_id.take() else {
            return;
        };
        let _ = block_on(self.backend.stop(&self.sandbox_id));
        let _ = block_on(self.backend.remove_tenant_artifacts(tenant_id));
    }
}
