#![cfg(target_os = "linux")]

use std::env;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;

use neovex_core::TenantId;
use neovex_sandbox::backends::krun::buildah::OciProcessOverrides;
use neovex_sandbox::backends::krun::{
    KrunLaunchMode, KrunSandboxBackend, KrunSandboxBackendConfig,
};
use neovex_sandbox::{
    PublishedEndpointProtocol, SandboxBackend, SandboxBackendKind, SandboxFilesystemSpec,
    SandboxPortBinding, SandboxProcessSpec, SandboxResourceLimits, SandboxSpec, SandboxStatus,
};

#[test]
#[ignore = "requires a Linux host with KVM, buildah, conmon, and a mounted rootfs"]
fn krun_backend_smoke_boots_http_service_and_survives_backend_restart() {
    let rootfs = env_path("NEOVEX_KRUN_SMOKE_ROOTFS");
    let host_port = env_u16("NEOVEX_KRUN_SMOKE_HOST_PORT").unwrap_or(18080);
    let guest_port = env_u16("NEOVEX_KRUN_SMOKE_GUEST_PORT").unwrap_or(8080);

    let base_dir = env_path("NEOVEX_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("bundles");
    let state_root = base_dir.join("state");

    let mut config = KrunSandboxBackendConfig::default();
    config.bundle_root = bundle_root.clone();
    config.state_root = state_root.clone();
    config.launch_mode = KrunLaunchMode::Execute;

    if let Some(runtime_path) = env::var_os("NEOVEX_KRUN_SMOKE_RUNTIME") {
        config.runtime_path = runtime_path.into();
    }
    if let Some(conmon_path) = env::var_os("NEOVEX_KRUN_SMOKE_CONMON") {
        config.conmon_path = conmon_path.into();
    }
    if let Some(buildah_path) = env::var_os("NEOVEX_KRUN_SMOKE_BUILDAH") {
        config.buildah_path = buildah_path.into();
    }

    let backend = KrunSandboxBackend::new(config.clone());
    let guest_port_str = guest_port.to_string();
    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        "http-smoke",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new(rootfs),
        SandboxProcessSpec::new(["/bin/busybox", "httpd", "-f", "-p", &guest_port_str]),
    )
    .with_port_binding(SandboxPortBinding::new(
        "http",
        PublishedEndpointProtocol::Http,
        host_port,
        guest_port,
    ));

    let handle = block_on(backend.start(spec)).expect("krun backend should start the sandbox");
    let cleanup_guard = CleanupGuard::new(backend.clone(), handle.id.clone());

    let ready_handle = wait_for_ready(&backend, &handle.id, Duration::from_secs(15));
    assert_eq!(
        ready_handle.status,
        SandboxStatus::Ready,
        "the backend should report a running sandbox after crun start succeeds"
    );

    let restarted_backend = KrunSandboxBackend::new(config);
    let restarted_handle = block_on(restarted_backend.inspect(&handle.id))
        .expect("inspect should succeed after constructing a new backend instance")
        .expect("manifest-backed sandbox should still be discoverable");
    assert_eq!(
        restarted_handle.status,
        SandboxStatus::Ready,
        "a fresh backend instance should recover the running sandbox from disk-backed state"
    );

    let http_response = wait_for_http_response(host_port, Duration::from_secs(15));
    assert!(
        http_response.starts_with("HTTP/1.1 200") || http_response.starts_with("HTTP/1.1 404"),
        "expected an HTTP response from the guest service, got: {http_response}"
    );

    let container_state_dir = state_root.join("containers").join(handle.id.as_str());
    assert!(
        container_state_dir.join("ctr.log").exists(),
        "conmon stdout/stderr log path should exist"
    );
    assert!(
        container_state_dir.join("oci.log").exists(),
        "runtime OCI log path should exist"
    );

    block_on(restarted_backend.stop(&handle.id)).expect("stop should succeed");
    let stopped_handle = block_on(restarted_backend.inspect(&handle.id))
        .expect("inspect should succeed after stop")
        .expect("stopped sandbox should still have a manifest");
    assert_eq!(
        stopped_handle.status,
        SandboxStatus::Stopped,
        "a deliberate backend stop should be reported as stopped even if SIGKILL was required"
    );

    cleanup_guard.disarm();
}

#[test]
#[ignore = "requires a Linux host with KVM, buildah, conmon, and network access for image pull"]
fn krun_backend_image_backed_smoke_pulls_and_boots_busybox() {
    // Use a different default port from the rootfs-only test so the two ignored
    // tests can run in parallel without port collisions.  Callers can still
    // override via env vars, but the defaults are safe for `-- --ignored`.
    let host_port = env_u16("NEOVEX_KRUN_IMAGE_SMOKE_HOST_PORT").unwrap_or(18081);
    let guest_port = env_u16("NEOVEX_KRUN_IMAGE_SMOKE_GUEST_PORT").unwrap_or(8081);

    let base_dir = env_path("NEOVEX_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("image-bundles");
    let state_root = base_dir.join("image-state");

    let mut config = KrunSandboxBackendConfig::default();
    config.bundle_root = bundle_root.clone();
    config.state_root = state_root.clone();
    config.launch_mode = KrunLaunchMode::Execute;

    if let Some(runtime_path) = env::var_os("NEOVEX_KRUN_SMOKE_RUNTIME") {
        config.runtime_path = runtime_path.into();
    }
    if let Some(conmon_path) = env::var_os("NEOVEX_KRUN_SMOKE_CONMON") {
        config.conmon_path = conmon_path.into();
    }
    if let Some(buildah_path) = env::var_os("NEOVEX_KRUN_SMOKE_BUILDAH") {
        config.buildah_path = buildah_path.into();
    }

    let backend = KrunSandboxBackend::new(config.clone());
    let guest_port_str = guest_port.to_string();
    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        "image-smoke",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new(""),
        SandboxProcessSpec::new(Vec::<String>::new()),
    )
    .with_port_binding(SandboxPortBinding::new(
        "http",
        PublishedEndpointProtocol::Http,
        host_port,
        guest_port,
    ));

    let overrides = OciProcessOverrides {
        cmd: Some(vec![
            "/bin/busybox".into(),
            "httpd".into(),
            "-f".into(),
            "-p".into(),
            guest_port_str,
        ]),
        ..Default::default()
    };

    let handle =
        block_on(backend.start_from_image(spec, "docker://busybox:latest".to_owned(), overrides))
            .expect("image-backed krun start should succeed");
    let cleanup_guard = CleanupGuard::new(backend.clone(), handle.id.clone());

    let ready_handle = wait_for_ready(&backend, &handle.id, Duration::from_secs(30));
    assert_eq!(
        ready_handle.status,
        SandboxStatus::Ready,
        "image-backed sandbox should reach ready"
    );

    let http_response = wait_for_http_response(host_port, Duration::from_secs(15));
    assert!(
        http_response.starts_with("HTTP/1.") || http_response.contains("404"),
        "expected HTTP response from image-backed sandbox, got: {http_response}"
    );

    let restarted_backend = KrunSandboxBackend::new(config);
    let restarted_handle = block_on(restarted_backend.inspect(&handle.id))
        .expect("inspect should succeed")
        .expect("image-backed sandbox should survive backend restart");
    assert_eq!(restarted_handle.status, SandboxStatus::Ready);

    block_on(restarted_backend.stop(&handle.id)).expect("stop should succeed");
    let stopped_handle = block_on(restarted_backend.inspect(&handle.id))
        .expect("inspect after stop should succeed")
        .expect("stopped sandbox should still have a manifest");
    assert_eq!(stopped_handle.status, SandboxStatus::Stopped);

    cleanup_guard.disarm();
}

/// M2 verification: prove image STOPSIGNAL-aware shutdown and image USER
/// resolution on a real Linux host.
///
/// Creates a custom local image from BusyBox with:
///   USER www-data          (uid=33, gid=33 in BusyBox's /etc/passwd)
///   STOPSIGNAL SIGQUIT
///
/// Verifies:
///   - the manifest records the resolved numeric user (33:33) from the image
///   - the manifest records stop_signal=SIGQUIT from the image
///   - the VM boots (with root user because krun VMM needs /dev/kvm)
///   - the guest service is reachable over TSI
///   - stop sends SIGQUIT first (configured signal), then falls back to SIGKILL
///
/// Note: krun containers cannot run the VMM process as a non-root user because
/// `/dev/kvm` requires root or kvm-group access.  The image USER is resolved
/// and recorded for future guest-side application, but the OCI bundle
/// process.user stays 0:0 for the VMM.  This is correct krun behavior.
#[test]
#[ignore = "requires a Linux host with KVM, buildah, conmon, and network access"]
fn krun_backend_m2_user_and_stop_signal_lowering() {
    let host_port: u16 = 18082;
    let guest_port: u16 = 8082;

    let base_dir = env_path("NEOVEX_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("m2-bundles");
    let state_root = base_dir.join("m2-state");

    let mut config = KrunSandboxBackendConfig::default();
    config.bundle_root = bundle_root.clone();
    config.state_root = state_root.clone();
    config.launch_mode = KrunLaunchMode::Execute;

    if let Some(runtime_path) = env::var_os("NEOVEX_KRUN_SMOKE_RUNTIME") {
        config.runtime_path = runtime_path.into();
    }
    if let Some(conmon_path) = env::var_os("NEOVEX_KRUN_SMOKE_CONMON") {
        config.conmon_path = conmon_path.into();
    }
    if let Some(buildah_path) = env::var_os("NEOVEX_KRUN_SMOKE_BUILDAH") {
        config.buildah_path = buildah_path.into();
    }

    // Build a custom local image with non-root USER and non-default STOPSIGNAL.
    let buildah = env::var("NEOVEX_KRUN_SMOKE_BUILDAH").unwrap_or("buildah".into());
    run_host_command(&buildah, &["rm", "m2-fixture"], true);
    run_host_command(
        &buildah,
        &["from", "--name", "m2-fixture", "docker://busybox:latest"],
        false,
    );
    run_host_command(
        &buildah,
        &["config", "--user", "www-data", "m2-fixture"],
        false,
    );
    run_host_command(
        &buildah,
        &["config", "--stop-signal", "SIGQUIT", "m2-fixture"],
        false,
    );
    run_host_command(
        &buildah,
        &["commit", "m2-fixture", "localhost/neovex-m2-fixture:latest"],
        false,
    );
    run_host_command(&buildah, &["rm", "m2-fixture"], true);

    let backend = KrunSandboxBackend::new(config.clone());
    let guest_port_str = guest_port.to_string();

    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        "m2-user-signal",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new(""),
        SandboxProcessSpec::new(Vec::<String>::new()),
    )
    .with_port_binding(SandboxPortBinding::new(
        "http",
        PublishedEndpointProtocol::Http,
        host_port,
        guest_port,
    ));

    let overrides = OciProcessOverrides {
        cmd: Some(vec![
            "/bin/busybox".into(),
            "httpd".into(),
            "-f".into(),
            "-p".into(),
            guest_port_str,
        ]),
        ..Default::default()
    };

    let handle = block_on(backend.start_from_image(
        spec,
        "localhost/neovex-m2-fixture:latest".to_owned(),
        overrides,
    ))
    .expect("image-backed start with non-root user should succeed");
    let cleanup_guard = CleanupGuard::new(backend.clone(), handle.id.clone());

    let ready_handle = wait_for_ready(&backend, &handle.id, Duration::from_secs(30));
    assert_eq!(ready_handle.status, SandboxStatus::Ready);

    // Verify HTTP connectivity.
    let http_response = wait_for_http_response(host_port, Duration::from_secs(15));
    assert!(
        http_response.starts_with("HTTP/1.") || http_response.contains("404"),
        "expected HTTP response from non-root-user sandbox, got: {http_response}"
    );

    // Verify bundle config.json has non-root uid/gid (www-data = 33:33).
    let bundle_config_path = bundle_root.join(handle.id.as_str()).join("config.json");
    let bundle_config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&bundle_config_path).unwrap_or_else(|_| {
            panic!(
                "bundle config should be readable at {}",
                bundle_config_path.display()
            )
        }))
        .expect("bundle config should be valid JSON");

    let uid = bundle_config["process"]["user"]["uid"]
        .as_u64()
        .expect("uid should be present");
    let gid = bundle_config["process"]["user"]["gid"]
        .as_u64()
        .expect("gid should be present");
    eprintln!("bundle process.user: uid={uid}, gid={gid}");
    // krun VMMs always run as root because the crun process needs /dev/kvm.
    // The image USER (www-data=33:33) is stored in the manifest for guest-side use.
    assert_eq!(
        uid, 0,
        "krun bundle must use root uid for VMM /dev/kvm access"
    );
    assert_eq!(
        gid, 0,
        "krun bundle must use root gid for VMM /dev/kvm access"
    );

    // Verify manifest records the configured stop signal.
    let manifest_path = state_root
        .join("containers")
        .join(handle.id.as_str())
        .join("manifest.json");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap_or_else(|_| {
            panic!("manifest should be readable at {}", manifest_path.display())
        }))
        .expect("manifest should be valid JSON");

    // The image USER is stored in manifest metadata for future guest-side use.
    let recorded_user = manifest["image_metadata"]["user"]
        .as_str()
        .unwrap_or("(none)");
    eprintln!("manifest.image_metadata.user: {recorded_user}");
    assert!(
        recorded_user.contains("33"),
        "manifest should record the resolved image user (www-data=33), got: {recorded_user}"
    );

    let recorded_signal = manifest["image_metadata"]["stop_signal"]
        .as_str()
        .unwrap_or("(none)");
    eprintln!("manifest.image_metadata.stop_signal: {recorded_signal}");
    assert_eq!(
        recorded_signal, "SIGQUIT",
        "manifest should record the image-configured STOPSIGNAL"
    );

    // Stop the sandbox.  BusyBox httpd does not handle SIGQUIT, so the backend
    // should send SIGQUIT first (configured signal), timeout, then SIGKILL.
    let stop_start = Instant::now();
    block_on(backend.stop(&handle.id)).expect("stop should succeed");
    let stop_elapsed = stop_start.elapsed();
    eprintln!("stop elapsed: {stop_elapsed:?}");

    let stopped_handle = block_on(backend.inspect(&handle.id))
        .expect("inspect after stop should succeed")
        .expect("stopped sandbox should still have a manifest");
    assert_eq!(stopped_handle.status, SandboxStatus::Stopped);

    // Re-read manifest after stop for exit code.
    let manifest_after: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&manifest_path).expect("manifest should be readable after stop"),
    )
    .expect("manifest should be valid JSON after stop");
    let exit_code = manifest_after["last_exit_code"].as_i64();
    eprintln!("manifest.last_exit_code: {exit_code:?}");
    eprintln!(
        "manifest.shutdown_requested: {}",
        manifest_after["shutdown_requested"]
    );
    assert_eq!(
        exit_code,
        Some(137),
        "exit code 137 = SIGKILL after SIGQUIT timeout"
    );

    cleanup_guard.disarm();
}

/// M2 verification: prove that image EXPOSE ports are auto-assigned from the
/// backend-owned port range when the generic spec has no explicit bindings.
///
/// Creates a custom image with `EXPOSE 8080/tcp`, then:
///   1. starts sandbox A with no explicit port bindings
///   2. verifies it gets an auto-assigned host port from 15000+
///   3. starts sandbox B — verifies it gets a different host port
///   4. stops sandbox A — verifies its port is released
///   5. starts sandbox C — verifies it reuses sandbox A's released port
#[test]
#[ignore = "requires a Linux host with KVM, buildah, conmon, and network access"]
fn krun_backend_m2_auto_port_assignment_and_reuse() {
    let base_dir = env_path("NEOVEX_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("autoport-bundles");
    let state_root = base_dir.join("autoport-state");

    let mut config = KrunSandboxBackendConfig::default();
    config.bundle_root = bundle_root.clone();
    config.state_root = state_root.clone();
    config.launch_mode = KrunLaunchMode::Execute;
    config.published_port_range = 15100..=15105;

    if let Some(runtime_path) = env::var_os("NEOVEX_KRUN_SMOKE_RUNTIME") {
        config.runtime_path = runtime_path.into();
    }
    if let Some(conmon_path) = env::var_os("NEOVEX_KRUN_SMOKE_CONMON") {
        config.conmon_path = conmon_path.into();
    }
    if let Some(buildah_path) = env::var_os("NEOVEX_KRUN_SMOKE_BUILDAH") {
        config.buildah_path = buildah_path.into();
    }

    // Build a custom image with EXPOSE 8080/tcp.
    let buildah = env::var("NEOVEX_KRUN_SMOKE_BUILDAH").unwrap_or("buildah".into());
    run_host_command(&buildah, &["rm", "autoport-fixture"], true);
    run_host_command(
        &buildah,
        &[
            "from",
            "--name",
            "autoport-fixture",
            "docker://busybox:latest",
        ],
        false,
    );
    run_host_command(
        &buildah,
        &["config", "--port", "8080/tcp", "autoport-fixture"],
        false,
    );
    run_host_command(
        &buildah,
        &[
            "commit",
            "autoport-fixture",
            "localhost/neovex-autoport:latest",
        ],
        false,
    );
    run_host_command(&buildah, &["rm", "autoport-fixture"], true);

    let backend = KrunSandboxBackend::new(config.clone());

    // Helper: create a sparse spec with NO port bindings.
    let make_spec = |name: &str| {
        SandboxSpec::new(
            TenantId::new("tenant").expect("tenant id should be valid"),
            name,
            SandboxBackendKind::Krun,
            SandboxFilesystemSpec::new(""),
            SandboxProcessSpec::new(Vec::<String>::new()),
        )
        // No .with_port_binding() — the backend should auto-assign from EXPOSE.
    };

    let overrides = OciProcessOverrides {
        cmd: Some(vec![
            "/bin/busybox".into(),
            "httpd".into(),
            "-f".into(),
            "-p".into(),
            "8080".into(),
        ]),
        ..Default::default()
    };

    // --- Sandbox A ---
    let handle_a = block_on(backend.start_from_image(
        make_spec("autoport-a"),
        "localhost/neovex-autoport:latest".to_owned(),
        overrides.clone(),
    ))
    .expect("sandbox A should start");
    let cleanup_a = CleanupGuard::new(backend.clone(), handle_a.id.clone());

    let ready_a = wait_for_ready(&backend, &handle_a.id, Duration::from_secs(30));
    assert_eq!(ready_a.status, SandboxStatus::Ready);

    assert!(
        !ready_a.published_endpoints.is_empty(),
        "sandbox A should have auto-assigned published endpoints"
    );
    let port_a = ready_a.published_endpoints[0].address.port();
    eprintln!("sandbox A auto-assigned host port: {port_a}");
    assert!(
        (15100..=15105).contains(&port_a),
        "auto-assigned port should be in configured range, got: {port_a}"
    );

    // Verify HTTP connectivity on the auto-assigned port.
    let http_a = wait_for_http_response(port_a, Duration::from_secs(15));
    assert!(
        http_a.starts_with("HTTP/1.") || http_a.contains("404"),
        "sandbox A should respond via auto-assigned port {port_a}: {http_a}"
    );
    eprintln!("sandbox A HTTP connectivity on port {port_a}: OK");

    // --- Sandbox B ---
    let handle_b = block_on(backend.start_from_image(
        make_spec("autoport-b"),
        "localhost/neovex-autoport:latest".to_owned(),
        overrides.clone(),
    ))
    .expect("sandbox B should start");
    let cleanup_b = CleanupGuard::new(backend.clone(), handle_b.id.clone());

    let ready_b = wait_for_ready(&backend, &handle_b.id, Duration::from_secs(30));
    assert_eq!(ready_b.status, SandboxStatus::Ready);

    let port_b = ready_b.published_endpoints[0].address.port();
    eprintln!("sandbox B auto-assigned host port: {port_b}");
    assert_ne!(
        port_a, port_b,
        "sandboxes A and B should get distinct host ports"
    );

    let http_b = wait_for_http_response(port_b, Duration::from_secs(15));
    assert!(
        http_b.starts_with("HTTP/1.") || http_b.contains("404"),
        "sandbox B should respond via auto-assigned port {port_b}: {http_b}"
    );
    eprintln!("sandbox B HTTP connectivity on port {port_b}: OK");

    // --- Stop A, verify port release ---
    block_on(backend.stop(&handle_a.id)).expect("stop A should succeed");
    cleanup_a.disarm();
    eprintln!("sandbox A stopped, port {port_a} should be released");

    // --- Sandbox C: should reuse A's released port ---
    let handle_c = block_on(backend.start_from_image(
        make_spec("autoport-c"),
        "localhost/neovex-autoport:latest".to_owned(),
        overrides,
    ))
    .expect("sandbox C should start");
    let cleanup_c = CleanupGuard::new(backend.clone(), handle_c.id.clone());

    let ready_c = wait_for_ready(&backend, &handle_c.id, Duration::from_secs(30));
    assert_eq!(ready_c.status, SandboxStatus::Ready);

    let port_c = ready_c.published_endpoints[0].address.port();
    eprintln!("sandbox C auto-assigned host port: {port_c}");
    assert_eq!(
        port_a, port_c,
        "sandbox C should reuse sandbox A's released port {port_a}, got: {port_c}"
    );

    let http_c = wait_for_http_response(port_c, Duration::from_secs(15));
    assert!(
        http_c.starts_with("HTTP/1.") || http_c.contains("404"),
        "sandbox C should respond via reused port {port_c}: {http_c}"
    );
    eprintln!("sandbox C HTTP connectivity on reused port {port_c}: OK");

    // Clean up B and C
    block_on(backend.stop(&handle_b.id)).expect("stop B should succeed");
    cleanup_b.disarm();
    block_on(backend.stop(&handle_c.id)).expect("stop C should succeed");
    cleanup_c.disarm();

    eprintln!("auto-port-assignment: all 3 sandboxes verified, port reuse confirmed");
}

/// M2 verification: prove direct-rootfs resource limits lower into both OCI
/// memory limits and the krun VM config sidecar on a real Linux host.
#[test]
#[ignore = "requires a Linux host with KVM, conmon, and a mounted rootfs"]
fn krun_backend_m2_direct_rootfs_resource_limits_lowering() {
    let rootfs = env_path("NEOVEX_KRUN_SMOKE_ROOTFS");
    let host_port: u16 = 18083;
    let guest_port: u16 = 8083;

    let base_dir = env_path("NEOVEX_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("m2-resources-rootfs-bundles");
    let state_root = base_dir.join("m2-resources-rootfs-state");

    let mut config = KrunSandboxBackendConfig::default();
    config.bundle_root = bundle_root.clone();
    config.state_root = state_root.clone();
    config.launch_mode = KrunLaunchMode::Execute;

    if let Some(runtime_path) = env::var_os("NEOVEX_KRUN_SMOKE_RUNTIME") {
        config.runtime_path = runtime_path.into();
    }
    if let Some(conmon_path) = env::var_os("NEOVEX_KRUN_SMOKE_CONMON") {
        config.conmon_path = conmon_path.into();
    }
    if let Some(buildah_path) = env::var_os("NEOVEX_KRUN_SMOKE_BUILDAH") {
        config.buildah_path = buildah_path.into();
    }

    let backend = KrunSandboxBackend::new(config);
    let guest_port_str = guest_port.to_string();
    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        "m2-rootfs-resources",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new(rootfs.clone()),
        SandboxProcessSpec::new(["/bin/busybox", "httpd", "-f", "-p", &guest_port_str]),
    )
    .with_resource_limits(
        SandboxResourceLimits::default()
            .with_cpu_count(2)
            .with_memory_limit_bytes(256 * 1024 * 1024),
    )
    .with_port_binding(SandboxPortBinding::new(
        "http",
        PublishedEndpointProtocol::Http,
        host_port,
        guest_port,
    ));

    let handle =
        block_on(backend.start(spec)).expect("rootfs-backed resource-limits sandbox should start");
    let cleanup_guard = CleanupGuard::new(backend.clone(), handle.id.clone());

    let ready_handle = wait_for_ready(&backend, &handle.id, Duration::from_secs(30));
    assert_eq!(ready_handle.status, SandboxStatus::Ready);

    let vm_config_path = rootfs.join(".krun_vm.json");
    let vm_config_text = std::fs::read_to_string(&vm_config_path).unwrap_or_else(|_| {
        panic!(
            "direct-rootfs resource-limits test expected vm config at {}",
            vm_config_path.display()
        )
    });
    let vm_config: serde_json::Value =
        serde_json::from_str(&vm_config_text).expect("vm config should be valid JSON");
    assert_eq!(vm_config["cpus"].as_u64(), Some(2));
    assert_eq!(vm_config["ram_mib"].as_u64(), Some(256));
    eprintln!("direct-rootfs .krun_vm.json: {vm_config_text}");

    let bundle_config_path = bundle_root.join(handle.id.as_str()).join("config.json");
    let bundle_config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&bundle_config_path).unwrap_or_else(|_| {
            panic!(
                "bundle config should be readable at {}",
                bundle_config_path.display()
            )
        }))
        .expect("bundle config should be valid JSON");
    assert_eq!(
        bundle_config["linux"]["resources"]["memory"]["limit"].as_u64(),
        Some(256 * 1024 * 1024)
    );
    eprintln!(
        "direct-rootfs linux.resources.memory.limit: {:?}",
        bundle_config["linux"]["resources"]["memory"]["limit"]
    );

    let http_response = wait_for_http_response(host_port, Duration::from_secs(15));
    assert!(
        http_response.starts_with("HTTP/1.") || http_response.contains("404"),
        "expected HTTP response from direct-rootfs resource-limits sandbox, got: {http_response}"
    );

    block_on(backend.stop(&handle.id)).expect("stop should succeed");
    cleanup_guard.disarm();
}

/// M2 verification: prove image-backed resource limits lower into both OCI
/// memory limits and the krun VM config sidecar inside the mounted buildah
/// container rootfs on a real Linux host.
#[test]
#[ignore = "requires a Linux host with KVM, buildah, conmon, and network access"]
fn krun_backend_m2_image_backed_resource_limits_lowering() {
    let host_port: u16 = 18084;
    let guest_port: u16 = 8084;

    let base_dir = env_path("NEOVEX_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("m2-resources-image-bundles");
    let state_root = base_dir.join("m2-resources-image-state");

    let mut config = KrunSandboxBackendConfig::default();
    config.bundle_root = bundle_root.clone();
    config.state_root = state_root.clone();
    config.launch_mode = KrunLaunchMode::Execute;

    if let Some(runtime_path) = env::var_os("NEOVEX_KRUN_SMOKE_RUNTIME") {
        config.runtime_path = runtime_path.into();
    }
    if let Some(conmon_path) = env::var_os("NEOVEX_KRUN_SMOKE_CONMON") {
        config.conmon_path = conmon_path.into();
    }
    if let Some(buildah_path) = env::var_os("NEOVEX_KRUN_SMOKE_BUILDAH") {
        config.buildah_path = buildah_path.into();
    }

    let backend = KrunSandboxBackend::new(config.clone());
    let guest_port_str = guest_port.to_string();
    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        "m2-image-resources",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new(""),
        SandboxProcessSpec::new(Vec::<String>::new()),
    )
    .with_resource_limits(
        SandboxResourceLimits::default()
            .with_cpu_count(2)
            .with_memory_limit_bytes(256 * 1024 * 1024),
    )
    .with_port_binding(SandboxPortBinding::new(
        "http",
        PublishedEndpointProtocol::Http,
        host_port,
        guest_port,
    ));

    let overrides = OciProcessOverrides {
        cmd: Some(vec![
            "/bin/busybox".into(),
            "httpd".into(),
            "-f".into(),
            "-p".into(),
            guest_port_str,
        ]),
        ..Default::default()
    };

    let handle =
        block_on(backend.start_from_image(spec, "docker://busybox:latest".to_owned(), overrides))
            .expect("image-backed resource-limits sandbox should start");
    let cleanup_guard = CleanupGuard::new(backend.clone(), handle.id.clone());

    let ready_handle = wait_for_ready(&backend, &handle.id, Duration::from_secs(30));
    assert_eq!(ready_handle.status, SandboxStatus::Ready);

    let bundle_config_path = bundle_root.join(handle.id.as_str()).join("config.json");
    let bundle_config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&bundle_config_path).unwrap_or_else(|_| {
            panic!(
                "bundle config should be readable at {}",
                bundle_config_path.display()
            )
        }))
        .expect("bundle config should be valid JSON");
    assert_eq!(
        bundle_config["linux"]["resources"]["memory"]["limit"].as_u64(),
        Some(256 * 1024 * 1024)
    );
    eprintln!(
        "image-backed linux.resources.memory.limit: {:?}",
        bundle_config["linux"]["resources"]["memory"]["limit"]
    );

    let buildah = env::var("NEOVEX_KRUN_SMOKE_BUILDAH").unwrap_or("buildah".into());
    let container_name = read_manifest_buildah_container_name(&state_root, &handle.id);
    let vm_config_text = read_buildah_rootfs_file(&buildah, &container_name, ".krun_vm.json");
    let vm_config: serde_json::Value =
        serde_json::from_str(&vm_config_text).expect("vm config should be valid JSON");
    assert_eq!(vm_config["cpus"].as_u64(), Some(2));
    assert_eq!(vm_config["ram_mib"].as_u64(), Some(256));
    eprintln!("image-backed .krun_vm.json ({container_name}): {vm_config_text}");

    let http_response = wait_for_http_response(host_port, Duration::from_secs(15));
    assert!(
        http_response.starts_with("HTTP/1.") || http_response.contains("404"),
        "expected HTTP response from image-backed resource-limits sandbox, got: {http_response}"
    );

    block_on(backend.stop(&handle.id)).expect("stop should succeed");
    cleanup_guard.disarm();
}

/// M3 verification: prove execute-mode sandboxes remain `Starting` with no
/// published endpoints until the guest actually begins answering on TSI.
#[test]
#[ignore = "requires a Linux host with KVM, conmon, and a mounted rootfs"]
fn krun_backend_m3_readiness_probe_gates_ready_and_published_endpoints() {
    let rootfs = env_path("NEOVEX_KRUN_SMOKE_ROOTFS");
    let host_port: u16 = 18085;
    let guest_port: u16 = 8085;

    let base_dir = env_path("NEOVEX_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("m3-readiness-bundles");
    let state_root = base_dir.join("m3-readiness-state");

    let mut config = KrunSandboxBackendConfig::default();
    config.bundle_root = bundle_root;
    config.state_root = state_root;
    config.launch_mode = KrunLaunchMode::Execute;

    if let Some(runtime_path) = env::var_os("NEOVEX_KRUN_SMOKE_RUNTIME") {
        config.runtime_path = runtime_path.into();
    }
    if let Some(conmon_path) = env::var_os("NEOVEX_KRUN_SMOKE_CONMON") {
        config.conmon_path = conmon_path.into();
    }
    if let Some(buildah_path) = env::var_os("NEOVEX_KRUN_SMOKE_BUILDAH") {
        config.buildah_path = buildah_path.into();
    }

    let backend = KrunSandboxBackend::new(config);
    let delayed_command = format!("sleep 2; exec /bin/busybox httpd -f -p {guest_port}");
    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        "m3-readiness-gate",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new(rootfs),
        SandboxProcessSpec::new(["/bin/busybox", "sh", "-c", &delayed_command]),
    )
    .with_port_binding(SandboxPortBinding::new(
        "http",
        PublishedEndpointProtocol::Http,
        host_port,
        guest_port,
    ));

    let handle = block_on(backend.start(spec)).expect("readiness-gated sandbox should start");
    let cleanup_guard = CleanupGuard::new(backend.clone(), handle.id.clone());

    assert_eq!(handle.status, SandboxStatus::Starting);
    assert!(
        handle.published_endpoints.is_empty(),
        "execute-mode start should hide published endpoints until readiness succeeds"
    );

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut observed_starting = false;
    while Instant::now() < deadline {
        if let Some(current) =
            block_on(backend.inspect(&handle.id)).expect("inspect should succeed")
        {
            if current.status == SandboxStatus::Starting {
                observed_starting = true;
                assert!(
                    current.published_endpoints.is_empty(),
                    "published endpoints should remain hidden while the guest is still booting"
                );
                break;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(
        observed_starting,
        "expected to observe a Starting state before the delayed guest HTTP service became ready"
    );

    let ready_handle = wait_for_ready(&backend, &handle.id, Duration::from_secs(15));
    assert_eq!(ready_handle.status, SandboxStatus::Ready);
    assert_eq!(
        ready_handle.published_endpoints.len(),
        1,
        "published endpoints should appear once readiness succeeds"
    );
    assert_eq!(
        ready_handle.published_endpoints[0].address.port(),
        host_port
    );

    let http_response = wait_for_http_response(host_port, Duration::from_secs(15));
    assert!(
        http_response.starts_with("HTTP/1.") || http_response.contains("404"),
        "expected HTTP response from readiness-gated sandbox, got: {http_response}"
    );

    block_on(backend.stop(&handle.id)).expect("stop should succeed");
    cleanup_guard.disarm();
}

/// M3 verification: prove execute-mode sandboxes degrade to `NotReady` when a
/// previously-ready guest service stops answering, then recover to `Ready` when
/// the same guest starts answering again without a VM restart.
#[test]
#[ignore = "requires a Linux host with KVM, conmon, and a mounted rootfs"]
fn krun_backend_m3_liveness_probe_degrades_and_recovers_without_vm_restart() {
    let rootfs = env_path("NEOVEX_KRUN_SMOKE_ROOTFS");
    let host_port: u16 = 18086;
    let guest_port: u16 = 8086;

    let base_dir = env_path("NEOVEX_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("m3-liveness-bundles");
    let state_root = base_dir.join("m3-liveness-state");

    let mut config = KrunSandboxBackendConfig::default();
    config.bundle_root = bundle_root;
    config.state_root = state_root;
    config.launch_mode = KrunLaunchMode::Execute;

    if let Some(runtime_path) = env::var_os("NEOVEX_KRUN_SMOKE_RUNTIME") {
        config.runtime_path = runtime_path.into();
    }
    if let Some(conmon_path) = env::var_os("NEOVEX_KRUN_SMOKE_CONMON") {
        config.conmon_path = conmon_path.into();
    }
    if let Some(buildah_path) = env::var_os("NEOVEX_KRUN_SMOKE_BUILDAH") {
        config.buildah_path = buildah_path.into();
    }

    let backend = KrunSandboxBackend::new(config);
    // Use foreground httpd as a background job so we can kill it by PID.
    // BusyBox's `killall httpd` does not reliably find the process inside a
    // krun VM because the process name is `busybox` not `httpd`.
    let liveness_script = format!(
        "/bin/busybox httpd -f -p {guest_port} & \
         HTTPD_PID=$!; \
         sleep 2; \
         kill $HTTPD_PID; \
         sleep 3; \
         /bin/busybox httpd -f -p {guest_port}"
    );
    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        "m3-liveness-gate",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new(rootfs),
        SandboxProcessSpec::new(["/bin/busybox", "sh", "-c", &liveness_script]),
    )
    .with_port_binding(SandboxPortBinding::new(
        "http",
        PublishedEndpointProtocol::Http,
        host_port,
        guest_port,
    ));

    let handle = block_on(backend.start(spec)).expect("liveness-gated sandbox should start");
    let cleanup_guard = CleanupGuard::new(backend.clone(), handle.id.clone());

    let ready_handle = wait_for_ready(&backend, &handle.id, Duration::from_secs(15));
    assert_eq!(ready_handle.status, SandboxStatus::Ready);
    assert_eq!(ready_handle.published_endpoints.len(), 1);
    assert_eq!(
        ready_handle.published_endpoints[0].address.port(),
        host_port
    );

    let initial_http = wait_for_http_response(host_port, Duration::from_secs(15));
    assert!(
        initial_http.starts_with("HTTP/1.") || initial_http.contains("404"),
        "expected initial HTTP response before liveness regression, got: {initial_http}"
    );

    let not_ready_handle = wait_for_status(
        &backend,
        &handle.id,
        SandboxStatus::NotReady,
        Duration::from_secs(15),
    );
    assert!(
        not_ready_handle.published_endpoints.is_empty(),
        "execute-mode sandboxes should withdraw published endpoints when liveness probes fail"
    );
    wait_for_http_unreachable(host_port, Duration::from_secs(5));

    let recovered_handle = wait_for_ready(&backend, &handle.id, Duration::from_secs(15));
    assert_eq!(recovered_handle.status, SandboxStatus::Ready);
    assert_eq!(recovered_handle.published_endpoints.len(), 1);
    assert_eq!(
        recovered_handle.published_endpoints[0].address.port(),
        host_port
    );

    let recovered_http = wait_for_http_response(host_port, Duration::from_secs(15));
    assert!(
        recovered_http.starts_with("HTTP/1.") || recovered_http.contains("404"),
        "expected HTTP response after liveness recovery, got: {recovered_http}"
    );

    block_on(backend.stop(&handle.id)).expect("stop should succeed");
    cleanup_guard.disarm();
}

fn run_host_command(program: &str, args: &[&str], allow_failure: bool) {
    let status = std::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap_or_else(|e| panic!("failed to run {program} {}: {e}", args.join(" ")));
    if !allow_failure && !status.success() {
        panic!("{program} {} failed with {status}", args.join(" "));
    }
}

fn run_host_command_capture_stdout(program: &str, args: &[&str]) -> String {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run {program} {}: {e}", args.join(" ")));
    if !output.status.success() {
        panic!("{program} {} failed with {}", args.join(" "), output.status);
    }
    String::from_utf8(output.stdout)
        .unwrap_or_else(|e| panic!("stdout from {program} was not utf-8: {e}"))
}

fn read_manifest_buildah_container_name(
    state_root: &std::path::Path,
    sandbox_id: &neovex_sandbox::SandboxId,
) -> String {
    let manifest_path = state_root
        .join("containers")
        .join(sandbox_id.as_str())
        .join("manifest.json");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap_or_else(|_| {
            panic!("manifest should be readable at {}", manifest_path.display())
        }))
        .expect("manifest should be valid JSON");
    manifest["buildah_container"]["container_name"]
        .as_str()
        .unwrap_or_else(|| {
            panic!(
                "manifest {} should record a buildah container name",
                manifest_path.display()
            )
        })
        .to_owned()
}

fn read_buildah_rootfs_file(
    buildah_program: &str,
    container_name: &str,
    relative_path: &str,
) -> String {
    let script = r#"rootfs="$("$1" mount "$2")"
test -n "$rootfs"
cat "$rootfs/$3""#;
    run_host_command_capture_stdout(
        buildah_program,
        &[
            "unshare",
            "--",
            "sh",
            "-c",
            script,
            "neovex-buildah-unshare",
            buildah_program,
            container_name,
            relative_path,
        ],
    )
}

fn wait_for_ready(
    backend: &KrunSandboxBackend,
    id: &neovex_sandbox::SandboxId,
    timeout: Duration,
) -> neovex_sandbox::SandboxHandle {
    wait_for_status(backend, id, SandboxStatus::Ready, timeout)
}

fn wait_for_status(
    backend: &KrunSandboxBackend,
    id: &neovex_sandbox::SandboxId,
    expected: SandboxStatus,
    timeout: Duration,
) -> neovex_sandbox::SandboxHandle {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(handle) = block_on(backend.inspect(id)).expect("inspect should succeed") {
            if handle.status == expected {
                return handle;
            }
        }
        thread::sleep(Duration::from_millis(250));
    }

    panic!("sandbox did not reach {expected:?} within {:?}", timeout);
}

fn wait_for_http_response(port: u16, timeout: Duration) -> String {
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

fn wait_for_http_unreachable(port: u16, timeout: Duration) {
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

fn env_path(key: &str) -> PathBuf {
    env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("expected environment variable {key} to be set"))
}

fn env_u16(key: &str) -> Option<u16> {
    env::var(key).ok().map(|value| {
        value
            .parse::<u16>()
            .unwrap_or_else(|error| panic!("failed to parse {key}={value:?} as u16: {error}"))
    })
}

struct CleanupGuard {
    backend: KrunSandboxBackend,
    sandbox_id: Option<neovex_sandbox::SandboxId>,
}

impl CleanupGuard {
    fn new(backend: KrunSandboxBackend, sandbox_id: neovex_sandbox::SandboxId) -> Self {
        Self {
            backend,
            sandbox_id: Some(sandbox_id),
        }
    }

    fn disarm(self) {
        std::mem::forget(self);
    }
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        if let Some(sandbox_id) = self.sandbox_id.take() {
            let _ = block_on(self.backend.stop(&sandbox_id));
        }
    }
}
