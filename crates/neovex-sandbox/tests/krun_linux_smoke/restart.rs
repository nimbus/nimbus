use super::support::*;
use super::*;

#[test]
#[ignore = "requires a Linux host with KVM, buildah, conmon, and a mounted rootfs"]
fn krun_backend_smoke_boots_http_service_and_survives_backend_restart() {
    let rootfs = env_path("NEOVEX_KRUN_SMOKE_ROOTFS");
    let host_port = env_u16("NEOVEX_KRUN_SMOKE_HOST_PORT").unwrap_or(18080);
    let guest_port = env_u16("NEOVEX_KRUN_SMOKE_GUEST_PORT").unwrap_or(8080);

    let base_dir = env_path("NEOVEX_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("bundles");
    let state_root = base_dir.join("state");

    let config = smoke_backend_config(bundle_root, state_root.clone());
    let backend = KrunSandboxBackend::new(config.clone());
    let guest_port_str = guest_port.to_string();
    let spec = SandboxSpec::new(
        sandbox_tenant(),
        "http-smoke",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new(rootfs),
        SandboxProcessSpec::new(["/bin/busybox", "httpd", "-f", "-p", &guest_port_str]),
    )
    .with_port_binding(http_binding(host_port, guest_port));

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
    assert_httpish_response(
        &http_response,
        "expected an HTTP response from the guest service",
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

/// M3 verification: prove an execute-mode sandbox with restart policy
/// `OnFailure { max_restarts: 1 }` is restarted after a failed first boot and
/// eventually reaches `Ready` on the second boot.
#[test]
#[ignore = "requires a Linux host with KVM, conmon, and a mounted rootfs"]
fn krun_backend_m3_restart_policy_restarts_failed_vm() {
    let rootfs = env_path("NEOVEX_KRUN_SMOKE_ROOTFS");
    let host_port: u16 = 18087;
    let guest_port: u16 = 8087;

    let base_dir = env_path("NEOVEX_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("m3-restart-bundles");
    let state_root = base_dir.join("m3-restart-state");
    let restart_marker_host = rootfs.join(".neovex-m3-restart-count-18087");
    let _ = std::fs::remove_file(&restart_marker_host);

    let backend = KrunSandboxBackend::new(smoke_backend_config(bundle_root, state_root.clone()));
    let restart_marker_guest = "/.neovex-m3-restart-count-18087";
    let restart_script = format!(
        "COUNT=0; \
         if [ -f {restart_marker_guest} ]; then COUNT=$(cat {restart_marker_guest}); fi; \
         COUNT=$((COUNT + 1)); \
         echo $COUNT > {restart_marker_guest}; \
         if [ \"$COUNT\" -eq 1 ]; then exit 42; fi; \
         exec /bin/busybox httpd -f -p {guest_port}"
    );
    let spec = SandboxSpec::new(
        sandbox_tenant(),
        "m3-restart-policy",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new(rootfs),
        SandboxProcessSpec::new(["/bin/busybox", "sh", "-c", &restart_script]),
    )
    .with_restart_policy(SandboxRestartPolicy::OnFailure { max_restarts: 1 })
    .with_port_binding(http_binding(host_port, guest_port));

    let handle = block_on(backend.start(spec)).expect("restart-policy sandbox should start");
    let cleanup_guard = CleanupGuard::new(backend.clone(), handle.id.clone());

    let ready_handle = wait_for_ready(&backend, &handle.id, Duration::from_secs(30));
    assert_eq!(ready_handle.status, SandboxStatus::Ready);
    assert_eq!(ready_handle.published_endpoints.len(), 1);
    assert_eq!(
        ready_handle.published_endpoints[0].address.port(),
        host_port
    );

    let http_response = wait_for_http_response(host_port, Duration::from_secs(15));
    assert_httpish_response(
        &http_response,
        "expected HTTP response after restart-policy recovery",
    );

    let restart_marker_text = std::fs::read_to_string(&restart_marker_host)
        .expect("restart marker should be written in the rootfs");
    assert_eq!(
        restart_marker_text.trim(),
        "2",
        "guest should have booted twice: initial failure, then restarted success"
    );

    let manifest_path = state_root
        .join("containers")
        .join(handle.id.as_str())
        .join("manifest.json");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&manifest_path).expect("manifest should be readable after restart"),
    )
    .expect("manifest should be valid JSON after restart");
    assert_eq!(manifest["restart_count"].as_u64(), Some(1));
    assert_eq!(manifest["last_exit_code"].as_i64(), Some(42));
    assert_eq!(manifest["status"].as_str(), Some("ready"));

    block_on(backend.stop(&handle.id)).expect("stop should succeed");
    cleanup_guard.disarm();
    let _ = std::fs::remove_file(&restart_marker_host);
}

/// M3 verification: prove restart-policy backoff delays repeated restart
/// attempts instead of relaunching immediately on every crash.
#[test]
#[ignore = "requires a Linux host with KVM, conmon, and a mounted rootfs"]
fn krun_backend_m3_restart_backoff_delays_repeated_restarts() {
    let rootfs = env_path("NEOVEX_KRUN_SMOKE_ROOTFS");
    let host_port: u16 = 18088;
    let guest_port: u16 = 8088;

    let base_dir = env_path("NEOVEX_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("m3-backoff-bundles");
    let state_root = base_dir.join("m3-backoff-state");
    let restart_marker_host = rootfs.join(".neovex-m3-restart-count-18088");
    let _ = std::fs::remove_file(&restart_marker_host);

    let backend = KrunSandboxBackend::new(smoke_backend_config(bundle_root, state_root.clone()));
    let restart_marker_guest = "/.neovex-m3-restart-count-18088";
    let restart_script = format!(
        "COUNT=0; \
         if [ -f {restart_marker_guest} ]; then COUNT=$(cat {restart_marker_guest}); fi; \
         COUNT=$((COUNT + 1)); \
         echo $COUNT > {restart_marker_guest}; \
         if [ \"$COUNT\" -lt 3 ]; then exit 42; fi; \
         exec /bin/busybox httpd -f -p {guest_port}"
    );
    let spec = SandboxSpec::new(
        sandbox_tenant(),
        "m3-restart-backoff",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new(rootfs),
        SandboxProcessSpec::new(["/bin/busybox", "sh", "-c", &restart_script]),
    )
    .with_restart_policy(SandboxRestartPolicy::OnFailure { max_restarts: 2 })
    .with_port_binding(http_binding(host_port, guest_port));

    let start_elapsed = Instant::now();
    let handle = block_on(backend.start(spec)).expect("restart-backoff sandbox should start");
    let cleanup_guard = CleanupGuard::new(backend.clone(), handle.id.clone());

    let ready_handle = wait_for_ready(&backend, &handle.id, Duration::from_secs(45));
    let total_elapsed = start_elapsed.elapsed();
    assert_eq!(ready_handle.status, SandboxStatus::Ready);
    assert_eq!(ready_handle.published_endpoints.len(), 1);
    assert_eq!(
        ready_handle.published_endpoints[0].address.port(),
        host_port
    );
    assert!(
        total_elapsed >= Duration::from_millis(2_500),
        "two crash restarts should incur visible backoff delay, observed {:?}",
        total_elapsed
    );

    let http_response = wait_for_http_response(host_port, Duration::from_secs(15));
    assert_httpish_response(
        &http_response,
        "expected HTTP response after restart-backoff recovery",
    );

    let restart_marker_text = std::fs::read_to_string(&restart_marker_host)
        .expect("restart marker should be written in the rootfs");
    assert_eq!(
        restart_marker_text.trim(),
        "3",
        "guest should have booted three times: two failures, then restarted success"
    );

    let manifest_path = state_root
        .join("containers")
        .join(handle.id.as_str())
        .join("manifest.json");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&manifest_path).expect("manifest should be readable after restart"),
    )
    .expect("manifest should be valid JSON after restart");
    assert_eq!(manifest["restart_count"].as_u64(), Some(2));
    assert_eq!(manifest["last_exit_code"].as_i64(), Some(42));
    assert_eq!(manifest["status"].as_str(), Some("ready"));

    block_on(backend.stop(&handle.id)).expect("stop should succeed");
    cleanup_guard.disarm();
    let _ = std::fs::remove_file(&restart_marker_host);
}
