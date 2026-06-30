#![cfg(target_os = "linux")]

//! KME2 runtime deny proof for the krun (libkrun/TSI microVM) backend.
//!
//! The krun execute path runs its VMM inside a deny-by-default network
//! namespace. Under libkrun TSI the guest's `connect()`/`sendto()` are host
//! sockets issued by the VMM process, so a netns around the VMM confines the
//! guest's egress to the namespace's only outbound path: a host-side egress PEP
//! bound on the bridge gateway. This test proves that a guest attempting direct
//! external egress with no reachable proxy gets a route failure.
//!
//! It is gated behind both `#[ignore]` and a `/dev/kvm` precondition. The krun
//! execute path is still fail-closed before launch planning (lifted by KME4),
//! so until KME4 lands this proof cannot boot a guest; it exists now as the
//! pinned runtime harness and runs once execute mode is enabled on real KVM
//! hardware as root.
//!
//! Teardown never relies on guest exit: a one-shot TSI guest that holds an
//! ESTABLISHED connection hangs VM teardown, so the harness kills the VMM and
//! releases the persistent netns under hard timeouts.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use tempfile::TempDir;

use nimbus_core::TenantId;
use nimbus_sandbox::backends::krun::{KrunSandboxBackend, KrunSandboxBackendConfig, KrunStartMode};
use nimbus_sandbox::{
    SandboxBackend, SandboxBackendKind, SandboxId, SandboxMountSpec, SandboxOwnerSpec,
    SandboxProcessSpec, SandboxRootSpec, SandboxSpec,
};

const RESULT_VOLUME: &str = "krun-egress-proof";
const RESULT_PATH_IN_GUEST: &str = "/nimbus-egress/result";
const GRACEFUL_STOP_TIMEOUT: Duration = Duration::from_secs(5);
const FORCE_CMD_TIMEOUT_SECS: u64 = 10;

#[test]
#[ignore = "requires a Linux root host with /dev/kvm, crun(krun), conmon, buildah, netavark, aardvark-dns, and OCI image pull; the runtime deny proof is gated behind the KME4 execute fail-close lift"]
fn krun_execute_mode_denies_direct_external_egress() {
    if !egress_proof_preconditions_met() {
        return;
    }

    let (temp_dir, workdir) = test_workdir("krun-direct-egress");

    let backend = KrunSandboxBackend::new(egress_backend_config(&workdir, 15300));
    let tenant_id = TenantId::new("krun-egress-proof").expect("tenant id should parse");
    let image = env::var("NIMBUS_KRUN_EGRESS_IMAGE")
        .unwrap_or_else(|_| "docker://busybox:latest".to_owned());
    let target =
        env::var("NIMBUS_KRUN_EGRESS_TARGET").unwrap_or_else(|_| "http://1.1.1.1".to_owned());
    let command = format!(
        "if (unset HTTP_PROXY http_proxy HTTPS_PROXY https_proxy ALL_PROXY all_proxy NO_PROXY no_proxy; wget -T 4 -q -O /tmp/nimbus-egress-body {target:?}); then echo allowed > {RESULT_PATH_IN_GUEST}; else echo denied > {RESULT_PATH_IN_GUEST}; fi; sleep 30"
    );
    let spec = SandboxSpec::new(
        tenant_id.clone(),
        SandboxOwnerSpec::standalone_named("krun-deny-direct-egress"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image_reference(image),
        SandboxProcessSpec::new(Vec::<String>::new())
            .with_entrypoint(["/bin/sh", "-c"])
            .with_command([command]),
    )
    .with_mount(SandboxMountSpec::tenant_volume(
        RESULT_VOLUME,
        "/nimbus-egress",
    ));

    let handle = block_on(backend.start(spec))
        .expect("krun guest should start once execute mode is enabled (KME4)");

    // The guard force-tears-down on scope exit AND on panic: it never blocks on
    // guest exit, killing the VMM and releasing the netns under hard timeouts.
    let _teardown = ForceTeardownGuard::new(
        backend.clone(),
        handle.id.clone(),
        sandbox_netns_path(&workdir, &tenant_id, &handle.id),
    );

    let result_path = tenant_volume_path(&workdir, &tenant_id, RESULT_VOLUME).join("result");
    let result = wait_for_result(
        &result_path,
        Duration::from_secs(25),
        "krun direct-egress proof result",
    );

    assert_eq!(
        result.trim(),
        "denied",
        "a krun guest with no reachable egress proxy must get a route failure inside the deny-by-default network namespace"
    );

    drop(temp_dir);
}

fn egress_proof_preconditions_met() -> bool {
    // SAFETY: geteuid has no preconditions and does not dereference pointers.
    let is_root = unsafe { libc::geteuid() } == 0;
    if !is_root {
        eprintln!(
            "skipping krun egress proof: must run as root to create persistent network namespaces"
        );
        return false;
    }
    if !Path::new("/dev/kvm").exists() {
        eprintln!("skipping krun egress proof: /dev/kvm is required to boot a libkrun microVM");
        return false;
    }
    true
}

fn egress_backend_config(workdir: &Path, first_port: u16) -> KrunSandboxBackendConfig {
    let mut config = KrunSandboxBackendConfig::under_root(workdir);
    config.start_mode = KrunStartMode::Execute;
    config.runtime_path = env::var_os("NIMBUS_KRUN_EGRESS_RUNTIME")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_existing_path("/usr/libexec/nimbus/crun", "crun"));
    config.conmon_path = env::var_os("NIMBUS_KRUN_EGRESS_CONMON")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_existing_path("/usr/bin/conmon", "conmon"));
    config.buildah_path = env::var_os("NIMBUS_KRUN_EGRESS_BUILDAH")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_existing_path("/usr/bin/buildah", "buildah"));
    config.netavark_path = env::var_os("NIMBUS_NETAVARK")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_existing_path("/usr/lib/podman/netavark", "netavark"));
    config.aardvark_dns_path = env::var_os("NIMBUS_AARDVARK_DNS")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_existing_path("/usr/lib/podman/aardvark-dns", "aardvark-dns"));
    config.use_buildah_unshare = false;
    config.published_port_range = first_port..=first_port + 20;
    config.start_timeout = Duration::from_secs(30);
    config
}

fn test_workdir(name: &str) -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().expect("temporary workdir should be created");
    let workdir = env::var_os("NIMBUS_KRUN_EGRESS_WORKDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| temp_dir.path().to_path_buf())
        .join(name);
    fs::create_dir_all(&workdir).expect("egress proof workdir should exist");
    (temp_dir, workdir)
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

fn sandbox_netns_path(workdir: &Path, tenant_id: &TenantId, sandbox_id: &SandboxId) -> PathBuf {
    workdir
        .join("state")
        .join("tenants")
        .join(tenant_id.as_str())
        .join("networks")
        .join("netns")
        .join(sandbox_id.as_str())
}

fn wait_for_result(path: &Path, timeout: Duration, label: &str) -> String {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(contents) = fs::read_to_string(path) {
            return contents;
        }
        thread::sleep(Duration::from_millis(200));
    }
    panic!("timed out waiting for {label} at {}", path.display());
}

/// Force-teardown harness: stop gracefully on a worker thread but never block on
/// guest exit, then kill the VMM and release the persistent netns under hard
/// timeouts. Runs on both the happy path (scope exit) and the panic path.
struct ForceTeardownGuard {
    backend: KrunSandboxBackend,
    sandbox_id: SandboxId,
    netns_path: PathBuf,
}

impl ForceTeardownGuard {
    fn new(backend: KrunSandboxBackend, sandbox_id: SandboxId, netns_path: PathBuf) -> Self {
        Self {
            backend,
            sandbox_id,
            netns_path,
        }
    }
}

impl Drop for ForceTeardownGuard {
    fn drop(&mut self) {
        let stop_handle = {
            let backend = self.backend.clone();
            let sandbox_id = self.sandbox_id.clone();
            thread::spawn(move || {
                let _ = block_on(backend.stop(&sandbox_id));
            })
        };

        let deadline = Instant::now() + GRACEFUL_STOP_TIMEOUT;
        while Instant::now() < deadline && !stop_handle.is_finished() {
            thread::sleep(Duration::from_millis(100));
        }

        if !stop_handle.is_finished() {
            // A one-shot TSI guest holding an ESTABLISHED connection hangs VM
            // teardown; kill the VMM (conmon/crun/krun) hard so the persistent
            // netns can be released.
            run_with_timeout("pkill", &["-9", "-f", self.sandbox_id.as_str()]);
        }

        // Release the persistent netns bind-mount regardless of guest state.
        run_with_timeout("umount", &["-l", &self.netns_path.to_string_lossy()]);
        let _ = fs::remove_file(&self.netns_path);
        let _ = stop_handle.join();
    }
}

fn run_with_timeout(program: &str, args: &[&str]) {
    let _ = Command::new("timeout")
        .arg(FORCE_CMD_TIMEOUT_SECS.to_string())
        .arg(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}
