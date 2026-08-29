use super::support::*;
use super::*;

#[test]
#[ignore = "requires a Linux host with KVM, buildah, conmon, and network access for image pull"]
fn krun_backend_image_backed_smoke_pulls_and_boots_busybox() {
    // Use a different default port from the rootfs-only test so the two ignored
    // tests can run in parallel without port collisions. Callers can still
    // override via env vars, but the defaults are safe for `-- --ignored`.
    let host_port =
        env_u16("NIMBUS_KRUN_IMAGE_SMOKE_HOST_PORT").unwrap_or_else(|| smoke_host_port(18081));
    let guest_port = env_u16("NIMBUS_KRUN_IMAGE_SMOKE_GUEST_PORT").unwrap_or(8081);

    let base_dir = env_path("NIMBUS_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("image-bundles");
    let state_root = base_dir.join("image-state");

    let config = smoke_backend_config(bundle_root.clone(), state_root.clone());
    let backend = KrunSandboxBackend::new(config);
    let mut spec = image_spec("image-smoke", "docker://busybox:latest")
        .with_port_binding(http_binding(host_port, guest_port));
    spec.process = busybox_http_process(guest_port);

    let provisioned = provision_krun(&backend, &state_root, spec, true)
        .expect("image-backed krun provision phases should succeed");
    assert!(!provisioned.ingress.is_empty());
    let handle = provisioned.handle;
    let teardown = provisioned.teardown;
    let cleanup_guard = CleanupGuard::new(backend.clone(), teardown.clone());
    let ingress = provisioned.ingress;

    let ready_handle = wait_for_ready(&backend, &handle.id, Duration::from_secs(30));
    assert_eq!(
        ready_handle.status,
        SandboxStatus::Ready,
        "image-backed sandbox should reach ready"
    );
    assert_eq!(
        ready_handle.published_endpoints[0].address,
        std::net::SocketAddr::from(([127, 0, 0, 1], host_port)),
        "image-backed sandbox endpoint should stay on loopback"
    );

    let http_response = wait_for_http_response(host_port, Duration::from_secs(15));
    assert_httpish_response(
        &http_response,
        "expected HTTP response from image-backed sandbox",
    );
    assert_host_port_not_bound_to_non_loopback(host_port);

    let restarted_backend = KrunSandboxBackend::new(smoke_backend_config(bundle_root, state_root));
    let restarted_handle = wait_for_ready(&restarted_backend, &handle.id, Duration::from_secs(30));
    assert_eq!(restarted_handle.status, SandboxStatus::Ready);

    drop(ingress);
    retire_krun(&restarted_backend, &teardown).expect("exact teardown should succeed");
    let stopped_handle = block_on(restarted_backend.inspect(&handle.id))
        .expect("inspect after stop should succeed")
        .expect("stopped sandbox should still have a manifest");
    assert_eq!(stopped_handle.handle.status, SandboxStatus::Stopped);

    cleanup_guard.disarm();
}

/// M2 verification: prove direct-rootfs resource limits lower into both OCI
/// memory limits and crun's authenticated resource annotations on Linux.
#[test]
#[ignore = "requires a Linux host with KVM, conmon, and a mounted rootfs"]
fn krun_backend_m2_direct_rootfs_resource_limits_lowering() {
    let rootfs = env_path("NIMBUS_KRUN_SMOKE_ROOTFS");
    let host_port = smoke_host_port(18083);
    let guest_port: u16 = 8083;

    let base_dir = env_path("NIMBUS_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("m2-resources-rootfs-bundles");
    let state_root = base_dir.join("m2-resources-rootfs-state");

    let backend = KrunSandboxBackend::new(smoke_backend_config(
        bundle_root.clone(),
        state_root.clone(),
    ));
    let guest_port_str = guest_port.to_string();
    let mut spec = rootfs_spec("m2-rootfs-resources", rootfs.clone());
    let untrusted_vm_config = rootfs.join(".krun_vm.json");
    std::fs::write(&untrusted_vm_config, "{\"kernel_path\":\"/tenant-kernel\"}")
        .expect("the direct-rootfs smoke fixture should seed an untrusted VM sidecar");
    spec.process = SandboxProcessSpec::new(["/bin/busybox", "httpd", "-f", "-p", &guest_port_str]);
    let spec = spec
        .with_resource_limits(
            SandboxResourceLimits::default()
                .with_cpu_count(2)
                .with_memory_limit_bytes(256 * 1024 * 1024),
        )
        .with_port_binding(http_binding(host_port, guest_port));

    let provisioned = provision_krun(&backend, &state_root, spec, true)
        .expect("rootfs-backed resource-limits provision phases should succeed");
    assert!(!provisioned.ingress.is_empty());
    let handle = provisioned.handle;
    let teardown = provisioned.teardown;
    let cleanup_guard = CleanupGuard::new(backend.clone(), teardown.clone());
    let ingress = provisioned.ingress;

    let ready_handle = wait_for_ready(&backend, &handle.id, Duration::from_secs(30));
    assert_eq!(ready_handle.status, SandboxStatus::Ready);

    assert!(
        !untrusted_vm_config.exists(),
        "Nimbus must remove image-controlled krun VM configuration before launch"
    );

    let bundle_config_path = bundle_config_path(&bundle_root, &sandbox_tenant(), &handle.id);
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
    assert_eq!(bundle_config["annotations"]["krun.cpus"], "2");
    assert_eq!(bundle_config["annotations"]["krun.ram_mib"], "256");
    eprintln!(
        "direct-rootfs linux.resources.memory.limit: {:?}",
        bundle_config["linux"]["resources"]["memory"]["limit"]
    );

    let http_response = wait_for_http_response(host_port, Duration::from_secs(15));
    assert_httpish_response(
        &http_response,
        "expected HTTP response from direct-rootfs resource-limits sandbox",
    );

    drop(ingress);
    retire_krun(&backend, &teardown).expect("exact teardown should succeed");
    cleanup_guard.disarm();
}

/// M2 verification: prove image-backed resource limits lower into both OCI
/// memory limits and crun's authenticated resource annotations on Linux.
#[test]
#[ignore = "requires a Linux host with KVM, buildah, conmon, and network access"]
fn krun_backend_m2_image_backed_resource_limits_lowering() {
    let host_port = smoke_host_port(18084);
    let guest_port: u16 = 8084;

    let base_dir = env_path("NIMBUS_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("m2-resources-image-bundles");
    let state_root = base_dir.join("m2-resources-image-state");

    let config = smoke_backend_config(bundle_root.clone(), state_root.clone());
    let backend = KrunSandboxBackend::new(config);
    let mut spec = image_spec("m2-image-resources", "docker://busybox:latest")
        .with_resource_limits(
            SandboxResourceLimits::default()
                .with_cpu_count(2)
                .with_memory_limit_bytes(256 * 1024 * 1024),
        )
        .with_port_binding(http_binding(host_port, guest_port));
    spec.process = busybox_http_process(guest_port);

    let provisioned = provision_krun(&backend, &state_root, spec, true)
        .expect("image-backed resource-limits provision phases should succeed");
    assert!(!provisioned.ingress.is_empty());
    let handle = provisioned.handle;
    let teardown = provisioned.teardown;
    let cleanup_guard = CleanupGuard::new(backend.clone(), teardown.clone());
    let ingress = provisioned.ingress;

    let ready_handle = wait_for_ready(&backend, &handle.id, Duration::from_secs(30));
    assert_eq!(ready_handle.status, SandboxStatus::Ready);

    let bundle_config_path = bundle_config_path(&bundle_root, &sandbox_tenant(), &handle.id);
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
    assert_eq!(bundle_config["annotations"]["krun.cpus"], "2");
    assert_eq!(bundle_config["annotations"]["krun.ram_mib"], "256");
    eprintln!(
        "image-backed linux.resources.memory.limit: {:?}",
        bundle_config["linux"]["resources"]["memory"]["limit"]
    );

    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(manifest_path(&state_root, &sandbox_tenant(), &handle.id))
            .expect("the image-backed manifest should be readable"),
    )
    .expect("the image-backed manifest should be valid JSON");
    let materialized_rootfs = std::path::Path::new(
        manifest["spec"]["root"]["rootfs"]
            .as_str()
            .expect("the image-backed manifest should record its materialized rootfs"),
    );
    assert!(
        !materialized_rootfs.join(".krun_vm.json").exists(),
        "Nimbus must remove image-controlled krun VM configuration before launch"
    );

    let http_response = wait_for_http_response(host_port, Duration::from_secs(15));
    assert_httpish_response(
        &http_response,
        "expected HTTP response from image-backed resource-limits sandbox",
    );

    drop(ingress);
    retire_krun(&backend, &teardown).expect("exact teardown should succeed");
    cleanup_guard.disarm();
}
