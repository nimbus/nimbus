use super::support::*;
use super::*;

/// M2 verification: prove that the ingress owner realizes provider-assigned
/// listeners without the krun backend inventing a published host port.
///
/// Creates a custom image with `EXPOSE 8080/tcp`, then:
///   1. provisions sandbox A with an explicit provider-assigned binding
///   2. verifies the ingress owner records a nonzero host port
///   3. provisions sandbox B and verifies its host port is distinct
///   4. stops sandbox A and proves a later sandbox can bind and serve
#[test]
#[ignore = "requires a Linux host with KVM, conmon, and a mounted BusyBox rootfs"]
fn krun_backend_m2_provider_assigned_ingress_binding_and_cleanup() {
    let base_dir = env_path("NIMBUS_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("autoport-bundles");
    let state_root = base_dir.join("autoport-state");

    let backend = KrunSandboxBackend::new(smoke_backend_config(bundle_root, state_root.clone()));

    let make_spec = |name: &str| {
        let mut spec = built_busybox_image_spec(name, &format!("nimbus-{name}"), "EXPOSE 8080/tcp")
            .with_port_binding(http_binding(0, 8080));
        spec.process = busybox_http_process(8080);
        spec
    };

    // --- Sandbox A ---
    let provisioned_a = provision_krun(&backend, &state_root, make_spec("autoport-a"), true)
        .expect("sandbox A should provision through every lifecycle phase");
    assert!(!provisioned_a.ingress.is_empty());
    let handle_a = provisioned_a.handle;
    let teardown_a = provisioned_a.teardown;
    let cleanup_a = CleanupGuard::new(backend.clone(), teardown_a.clone());
    let ingress_a = provisioned_a.ingress;

    let ready_a = wait_for_ready(&backend, &handle_a.id, Duration::from_secs(30));
    assert_eq!(ready_a.status, SandboxStatus::Ready);

    assert!(
        ready_a.published_endpoints.is_empty(),
        "the krun backend must not claim the ingress owner's assigned port"
    );
    let port_a = ingress_a.addresses()[0].port();
    assert_ne!(port_a, 0, "the ingress owner must record its assigned port");
    eprintln!("sandbox A ingress-owner assigned host port: {port_a}");

    let http_a = wait_for_http_response(port_a, Duration::from_secs(15));
    assert_httpish_response(
        &http_a,
        &format!("sandbox A should respond via auto-assigned port {port_a}"),
    );
    eprintln!("sandbox A HTTP connectivity on port {port_a}: OK");

    // --- Sandbox B ---
    let provisioned_b = provision_krun(&backend, &state_root, make_spec("autoport-b"), true)
        .expect("sandbox B should provision through every lifecycle phase");
    assert!(!provisioned_b.ingress.is_empty());
    let handle_b = provisioned_b.handle;
    let teardown_b = provisioned_b.teardown;
    let cleanup_b = CleanupGuard::new(backend.clone(), teardown_b.clone());
    let ingress_b = provisioned_b.ingress;

    let ready_b = wait_for_ready(&backend, &handle_b.id, Duration::from_secs(30));
    assert_eq!(ready_b.status, SandboxStatus::Ready);

    assert!(ready_b.published_endpoints.is_empty());
    let port_b = ingress_b.addresses()[0].port();
    eprintln!("sandbox B ingress-owner assigned host port: {port_b}");
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
    drop(ingress_a);
    retire_krun(&backend, &teardown_a).expect("exact teardown A should succeed");
    cleanup_a.disarm();
    eprintln!("sandbox A stopped and released its ingress listener on port {port_a}");

    // --- Sandbox C: a later provider-assigned listener still binds and serves. ---
    let provisioned_c = provision_krun(&backend, &state_root, make_spec("autoport-c"), true)
        .expect("sandbox C should provision through every lifecycle phase");
    assert!(!provisioned_c.ingress.is_empty());
    let handle_c = provisioned_c.handle;
    let teardown_c = provisioned_c.teardown;
    let cleanup_c = CleanupGuard::new(backend.clone(), teardown_c.clone());
    let ingress_c = provisioned_c.ingress;

    let ready_c = wait_for_ready(&backend, &handle_c.id, Duration::from_secs(30));
    assert_eq!(ready_c.status, SandboxStatus::Ready);

    assert!(ready_c.published_endpoints.is_empty());
    let port_c = ingress_c.addresses()[0].port();
    assert_ne!(port_c, 0, "the ingress owner must record its assigned port");
    eprintln!("sandbox C ingress-owner assigned host port: {port_c}");

    let http_c = wait_for_http_response(port_c, Duration::from_secs(15));
    assert_httpish_response(
        &http_c,
        &format!("sandbox C should respond via provider-assigned port {port_c}"),
    );
    eprintln!("sandbox C HTTP connectivity on provider-assigned port {port_c}: OK");

    drop(ingress_b);
    retire_krun(&backend, &teardown_b).expect("exact teardown B should succeed");
    cleanup_b.disarm();
    drop(ingress_c);
    retire_krun(&backend, &teardown_c).expect("exact teardown C should succeed");
    cleanup_c.disarm();

    eprintln!("provider-assigned ingress: all 3 sandboxes verified with exact cleanup");
}

/// M3 verification: prove execute-mode sandboxes remain `Starting` with no
/// published endpoints until the guest actually begins answering on TSI.
#[test]
#[ignore = "requires a Linux host with KVM, conmon, and a mounted rootfs"]
fn krun_backend_m3_readiness_probe_gates_ready_and_published_endpoints() {
    let rootfs = env_path("NIMBUS_KRUN_SMOKE_ROOTFS");
    let host_port = smoke_host_port(18085);
    let guest_port: u16 = 8085;

    let base_dir = env_path("NIMBUS_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("m3-readiness-bundles");
    let state_root = base_dir.join("m3-readiness-state");

    let backend = KrunSandboxBackend::new(smoke_backend_config(bundle_root, state_root.clone()));
    let delayed_command = format!("sleep 2; exec /bin/busybox httpd -f -p {guest_port}");
    let mut spec = rootfs_spec("m3-readiness-gate", rootfs);
    spec.process = SandboxProcessSpec::new([
        "/bin/busybox".to_owned(),
        "sh".to_owned(),
        "-c".to_owned(),
        delayed_command,
    ]);
    let spec = spec.with_port_binding(http_binding(host_port, guest_port));

    let provisioned = provision_krun(&backend, &state_root, spec, true)
        .expect("readiness-gated sandbox should provision through every lifecycle phase");
    assert!(!provisioned.ingress.is_empty());
    let handle = provisioned.handle;
    let teardown = provisioned.teardown;
    let cleanup_guard = CleanupGuard::new(backend.clone(), teardown.clone());
    let ingress = provisioned.ingress;

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
            .filter(|inspection| inspection.handle.status == SandboxStatus::Starting)
        {
            observed_starting = true;
            assert!(
                current.handle.published_endpoints.is_empty(),
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

    drop(ingress);
    retire_krun(&backend, &teardown).expect("exact teardown should succeed");
    cleanup_guard.disarm();
}

/// M3 verification: prove execute-mode sandboxes degrade to `NotReady` when a
/// previously-ready guest service stops answering, then recover to `Ready` when
/// the same guest starts answering again without a VM restart.
#[test]
#[ignore = "requires a Linux host with KVM, conmon, and a mounted rootfs"]
fn krun_backend_m3_liveness_probe_degrades_and_recovers_without_vm_restart() {
    let rootfs = env_path("NIMBUS_KRUN_SMOKE_ROOTFS");
    let host_port = smoke_host_port(18086);
    let guest_port: u16 = 8086;

    let base_dir = env_path("NIMBUS_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("m3-liveness-bundles");
    let state_root = base_dir.join("m3-liveness-state");

    let backend = KrunSandboxBackend::new(smoke_backend_config(bundle_root, state_root.clone()));
    let liveness_script = format!(
        "/bin/busybox httpd -f -p {guest_port} & \
         HTTPD_PID=$!; \
         echo nimbus-smoke-liveness:initial:$HTTPD_PID; \
         /bin/busybox sleep 30; \
         echo nimbus-smoke-liveness:stopping:$HTTPD_PID; \
         /bin/busybox kill \"$HTTPD_PID\"; \
         wait \"$HTTPD_PID\" 2>/dev/null || true; \
         echo nimbus-smoke-liveness:stopped; \
         /bin/busybox sleep 10; \
         echo nimbus-smoke-liveness:restarting; \
         exec /bin/busybox httpd -f -p {guest_port}"
    );
    let mut spec = rootfs_spec("m3-liveness-gate", rootfs);
    spec.process = SandboxProcessSpec::new([
        "/bin/busybox".to_owned(),
        "sh".to_owned(),
        "-c".to_owned(),
        liveness_script,
    ]);
    let spec = spec.with_port_binding(http_binding(host_port, guest_port));

    let provisioned = provision_krun(&backend, &state_root, spec, true)
        .expect("liveness-gated sandbox should provision through every lifecycle phase");
    assert!(!provisioned.ingress.is_empty());
    let handle = provisioned.handle;
    let teardown = provisioned.teardown;
    let cleanup_guard = CleanupGuard::new(backend.clone(), teardown.clone());
    let ingress = provisioned.ingress;

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
    let runtime_pidfile =
        container_state_dir(&state_root, &sandbox_tenant(), &handle.id).join("pidfile");
    let runtime_pid_before = std::fs::read_to_string(&runtime_pidfile)
        .expect("the live krun pidfile should be readable")
        .trim()
        .to_owned();

    // Provider inspection is read-only. The upper saga retains the earlier
    // Ready phase and maps either backend Starting or NotReady to unavailable.
    let unavailable_handle = wait_for_unavailable(&backend, &handle.id, Duration::from_secs(45));
    assert!(
        unavailable_handle.published_endpoints.is_empty(),
        "execute-mode sandboxes should withdraw published endpoints when liveness probes fail"
    );
    wait_for_http_unreachable(host_port, Duration::from_secs(5));

    let recovered_handle = wait_for_ready(&backend, &handle.id, Duration::from_secs(25));
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
    let runtime_pid_after = std::fs::read_to_string(&runtime_pidfile)
        .expect("the recovered krun pidfile should be readable")
        .trim()
        .to_owned();
    assert_eq!(
        runtime_pid_before, runtime_pid_after,
        "liveness recovery must not restart the VM runtime"
    );

    drop(ingress);
    retire_krun(&backend, &teardown).expect("exact teardown should succeed");
    cleanup_guard.disarm();
}
