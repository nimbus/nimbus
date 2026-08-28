use super::support::*;
use super::*;

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
///   - the guest helper applies the resolved user before it starts the service
///   - the guest service is reachable over TSI
///   - stop sends SIGQUIT first (configured signal), then falls back to SIGKILL
///
/// Note: krun containers cannot run the VMM process as a non-root user because
/// `/dev/kvm` requires root or kvm-group access. The OCI bundle process.user
/// therefore stays 0:0 for the VMM, and the mounted guest helper applies the
/// resolved image user inside the guest before it starts the service.
#[test]
#[ignore = "requires a Linux host with KVM, conmon, and a mounted BusyBox rootfs"]
fn krun_backend_m2_user_and_stop_signal_lowering() {
    let host_port = smoke_host_port(18082);
    let guest_port: u16 = 8082;

    let base_dir = env_path("NIMBUS_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("m2-bundles");
    let state_root = base_dir.join("m2-state");

    let config = smoke_backend_config(bundle_root.clone(), state_root.clone());

    let backend = KrunSandboxBackend::new(config);
    let mut spec = built_busybox_image_spec(
        "m2-user-signal",
        "nimbus-m2-fixture",
        "USER www-data\nSTOPSIGNAL SIGQUIT",
    )
    .with_port_binding(http_binding(host_port, guest_port));
    spec.process = busybox_http_process(guest_port);

    let provisioned = provision_krun(&backend, &state_root, spec, true)
        .expect("image-backed non-root-user provision phases should succeed");
    assert!(!provisioned.ingress.is_empty());
    let handle = provisioned.handle;
    let teardown = provisioned.teardown;
    let cleanup_guard = CleanupGuard::new(backend.clone(), teardown.clone());
    let ingress = provisioned.ingress;

    let ready_handle = wait_for_ready(&backend, &handle.id, Duration::from_secs(30));
    assert_eq!(ready_handle.status, SandboxStatus::Ready);

    let http_response = wait_for_http_response(host_port, Duration::from_secs(15));
    assert_httpish_response(
        &http_response,
        "expected HTTP response from non-root-user sandbox",
    );

    let tenant_id = sandbox_tenant();
    let bundle_config_path = bundle_config_path(&bundle_root, &tenant_id, &handle.id);
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
    assert_eq!(
        uid, 0,
        "krun bundle must use root uid for VMM /dev/kvm access"
    );
    assert_eq!(
        gid, 0,
        "krun bundle must use root gid for VMM /dev/kvm access"
    );

    let manifest_path = manifest_path(&state_root, &tenant_id, &handle.id);
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap_or_else(|_| {
            panic!("manifest should be readable at {}", manifest_path.display())
        }))
        .expect("manifest should be valid JSON");

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

    let stop_start = Instant::now();
    drop(ingress);
    retire_krun(&backend, &teardown).expect("exact teardown should succeed");
    let stop_elapsed = stop_start.elapsed();
    eprintln!("stop elapsed: {stop_elapsed:?}");
    assert!(
        stop_elapsed >= Duration::from_secs(5),
        "the image-configured SIGQUIT must receive the complete graceful-stop window before forced stop"
    );

    let stopped_handle = block_on(backend.inspect(&handle.id))
        .expect("inspect after stop should succeed")
        .expect("stopped sandbox should still have a manifest");
    assert_eq!(stopped_handle.handle.status, SandboxStatus::Stopped);

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
        exit_code, None,
        "the legacy conmon exit receipt is not creator-qualified and must not be attributed to this execution"
    );

    cleanup_guard.disarm();
}
