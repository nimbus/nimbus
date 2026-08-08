use super::support::*;
use super::*;

#[test]
#[ignore = "requires a Linux host with KVM, buildah, conmon, and network access for image pull"]
fn krun_backend_image_backed_smoke_pulls_and_boots_busybox() {
    // Use a different default port from the rootfs-only test so the two ignored
    // tests can run in parallel without port collisions. Callers can still
    // override via env vars, but the defaults are safe for `-- --ignored`.
    let host_port = env_u16("NIMBUS_KRUN_IMAGE_SMOKE_HOST_PORT").unwrap_or(18081);
    let guest_port = env_u16("NIMBUS_KRUN_IMAGE_SMOKE_GUEST_PORT").unwrap_or(8081);

    let base_dir = env_path("NIMBUS_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("image-bundles");
    let state_root = base_dir.join("image-state");

    let config = smoke_backend_config(bundle_root, state_root.clone());
    let backend = KrunSandboxBackend::new(config.clone());
    let mut spec = image_spec("image-smoke", "docker://busybox:latest")
        .with_port_binding(http_binding(host_port, guest_port));
    spec.process = busybox_http_process(guest_port);

    let provisioned = provision_krun(&backend, &state_root, spec, true)
        .expect("image-backed krun provision phases should succeed");
    assert!(!provisioned.ingress.is_empty());
    let handle = provisioned.handle;
    let _ingress = provisioned.ingress;
    let cleanup_guard = CleanupGuard::new(backend.clone(), handle.id.clone());

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

    let restarted_backend = KrunSandboxBackend::new(config);
    let restarted_handle = block_on(restarted_backend.inspect(&handle.id))
        .expect("inspect should succeed")
        .expect("image-backed sandbox should survive backend restart");
    assert_eq!(restarted_handle.handle.status, SandboxStatus::Ready);

    block_on(restarted_backend.stop(&handle.id)).expect("stop should succeed");
    let stopped_handle = block_on(restarted_backend.inspect(&handle.id))
        .expect("inspect after stop should succeed")
        .expect("stopped sandbox should still have a manifest");
    assert_eq!(stopped_handle.handle.status, SandboxStatus::Stopped);

    cleanup_guard.disarm();
}

/// M2 verification: prove direct-rootfs resource limits lower into both OCI
/// memory limits and the krun VM config sidecar on a real Linux host.
#[test]
#[ignore = "requires a Linux host with KVM, conmon, and a mounted rootfs"]
fn krun_backend_m2_direct_rootfs_resource_limits_lowering() {
    let rootfs = env_path("NIMBUS_KRUN_SMOKE_ROOTFS");
    let host_port: u16 = 18083;
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
    let _ingress = provisioned.ingress;
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
    assert_httpish_response(
        &http_response,
        "expected HTTP response from direct-rootfs resource-limits sandbox",
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
    let _ingress = provisioned.ingress;
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

    let buildah = buildah_program();
    let session_name = read_manifest_mount_session_name(&state_root, &handle.id);
    let vm_config_text = read_buildah_rootfs_file(&buildah, &session_name, ".krun_vm.json");
    let vm_config: serde_json::Value =
        serde_json::from_str(&vm_config_text).expect("vm config should be valid JSON");
    assert_eq!(vm_config["cpus"].as_u64(), Some(2));
    assert_eq!(vm_config["ram_mib"].as_u64(), Some(256));
    eprintln!("image-backed .krun_vm.json ({session_name}): {vm_config_text}");

    let http_response = wait_for_http_response(host_port, Duration::from_secs(15));
    assert_httpish_response(
        &http_response,
        "expected HTTP response from image-backed resource-limits sandbox",
    );

    block_on(backend.stop(&handle.id)).expect("stop should succeed");
    cleanup_guard.disarm();
}
