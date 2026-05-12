use super::support::*;
use super::*;

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
    let base_dir = env_path("NIMBUS_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("autoport-bundles");
    let state_root = base_dir.join("autoport-state");

    let mut config = smoke_backend_config(bundle_root, state_root);
    config.published_port_range = 15100..=15105;

    let buildah = buildah_program();
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
            "localhost/nimbus-autoport:latest",
        ],
        false,
    );
    run_host_command(&buildah, &["rm", "autoport-fixture"], true);

    let backend = KrunSandboxBackend::new(config);

    let make_spec = |name: &str| empty_image_spec(name);

    // --- Sandbox A ---
    let handle_a = block_on(
        backend.start_from_image(
            SandboxImageLaunchSpec::new(
                make_spec("autoport-a"),
                "localhost/nimbus-autoport:latest",
            )
            .with_process_overrides(busybox_http_overrides(8080)),
        ),
    )
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

    let http_a = wait_for_http_response(port_a, Duration::from_secs(15));
    assert_httpish_response(
        &http_a,
        &format!("sandbox A should respond via auto-assigned port {port_a}"),
    );
    eprintln!("sandbox A HTTP connectivity on port {port_a}: OK");

    // --- Sandbox B ---
    let handle_b = block_on(
        backend.start_from_image(
            SandboxImageLaunchSpec::new(
                make_spec("autoport-b"),
                "localhost/nimbus-autoport:latest",
            )
            .with_process_overrides(busybox_http_overrides(8080)),
        ),
    )
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
    assert_httpish_response(
        &http_b,
        &format!("sandbox B should respond via auto-assigned port {port_b}"),
    );
    eprintln!("sandbox B HTTP connectivity on port {port_b}: OK");

    // --- Stop A, verify port release ---
    block_on(backend.stop(&handle_a.id)).expect("stop A should succeed");
    cleanup_a.disarm();
    eprintln!("sandbox A stopped, port {port_a} should be released");

    // --- Sandbox C: should reuse A's released port ---
    let handle_c = block_on(
        backend.start_from_image(
            SandboxImageLaunchSpec::new(
                make_spec("autoport-c"),
                "localhost/nimbus-autoport:latest",
            )
            .with_process_overrides(busybox_http_overrides(8080)),
        ),
    )
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
    assert_httpish_response(
        &http_c,
        &format!("sandbox C should respond via reused port {port_c}"),
    );
    eprintln!("sandbox C HTTP connectivity on reused port {port_c}: OK");

    block_on(backend.stop(&handle_b.id)).expect("stop B should succeed");
    cleanup_b.disarm();
    block_on(backend.stop(&handle_c.id)).expect("stop C should succeed");
    cleanup_c.disarm();

    eprintln!("auto-port-assignment: all 3 sandboxes verified, port reuse confirmed");
}

/// M3 verification: prove execute-mode sandboxes remain `Starting` with no
/// published endpoints until the guest actually begins answering on TSI.
#[test]
#[ignore = "requires a Linux host with KVM, conmon, and a mounted rootfs"]
fn krun_backend_m3_readiness_probe_gates_ready_and_published_endpoints() {
    let rootfs = env_path("NIMBUS_KRUN_SMOKE_ROOTFS");
    let host_port: u16 = 18085;
    let guest_port: u16 = 8085;

    let base_dir = env_path("NIMBUS_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("m3-readiness-bundles");
    let state_root = base_dir.join("m3-readiness-state");

    let backend = KrunSandboxBackend::new(smoke_backend_config(bundle_root, state_root));
    let delayed_command = format!("sleep 2; exec /bin/busybox httpd -f -p {guest_port}");
    let spec = SandboxSpec::new(
        sandbox_tenant(),
        "m3-readiness-gate",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new(rootfs),
        SandboxProcessSpec::new(["/bin/busybox", "sh", "-c", &delayed_command]),
    )
    .with_port_binding(http_binding(host_port, guest_port));

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
        if let Some(current) = block_on(backend.inspect(&handle.id))
            .expect("inspect should succeed")
            .filter(|h| h.status == SandboxStatus::Starting)
        {
            observed_starting = true;
            assert!(
                current.published_endpoints.is_empty(),
                "published endpoints should remain hidden while the guest is still booting"
            );
            break;
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
    assert_httpish_response(
        &http_response,
        "expected HTTP response from readiness-gated sandbox",
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
    let rootfs = env_path("NIMBUS_KRUN_SMOKE_ROOTFS");
    let host_port: u16 = 18086;
    let guest_port: u16 = 8086;

    let base_dir = env_path("NIMBUS_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("m3-liveness-bundles");
    let state_root = base_dir.join("m3-liveness-state");

    let backend = KrunSandboxBackend::new(smoke_backend_config(bundle_root, state_root));
    let liveness_script = format!(
        "/bin/busybox httpd -f -p {guest_port} & \
         HTTPD_PID=$!; \
         sleep 2; \
         kill $HTTPD_PID; \
         sleep 3; \
         /bin/busybox httpd -f -p {guest_port}"
    );
    let spec = SandboxSpec::new(
        sandbox_tenant(),
        "m3-liveness-gate",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new(rootfs),
        SandboxProcessSpec::new(["/bin/busybox", "sh", "-c", &liveness_script]),
    )
    .with_port_binding(http_binding(host_port, guest_port));

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
    assert_httpish_response(
        &initial_http,
        "expected initial HTTP response before liveness regression",
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
    assert_httpish_response(
        &recovered_http,
        "expected HTTP response after liveness recovery",
    );

    block_on(backend.stop(&handle.id)).expect("stop should succeed");
    cleanup_guard.disarm();
}
