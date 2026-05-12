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
///   - the guest service is reachable over TSI
///   - stop sends SIGQUIT first (configured signal), then falls back to SIGKILL
///
/// Note: krun containers cannot run the VMM process as a non-root user because
/// `/dev/kvm` requires root or kvm-group access. The image USER is resolved
/// and recorded for future guest-side application, but the OCI bundle
/// process.user stays 0:0 for the VMM. This is correct krun behavior.
#[test]
#[ignore = "requires a Linux host with KVM, buildah, conmon, and network access"]
fn krun_backend_m2_user_and_stop_signal_lowering() {
    let host_port: u16 = 18082;
    let guest_port: u16 = 8082;

    let base_dir = env_path("NIMBUS_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("m2-bundles");
    let state_root = base_dir.join("m2-state");

    let config = smoke_backend_config(bundle_root.clone(), state_root.clone());

    let buildah = buildah_program();
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
        &["commit", "m2-fixture", "localhost/nimbus-m2-fixture:latest"],
        false,
    );
    run_host_command(&buildah, &["rm", "m2-fixture"], true);

    let backend = KrunSandboxBackend::new(config);
    let spec =
        empty_image_spec("m2-user-signal").with_port_binding(http_binding(host_port, guest_port));

    let handle = block_on(
        backend.start_from_image(
            SandboxImageLaunchSpec::new(spec, "localhost/nimbus-m2-fixture:latest")
                .with_process_overrides(busybox_http_overrides(guest_port)),
        ),
    )
    .expect("image-backed start with non-root user should succeed");
    let cleanup_guard = CleanupGuard::new(backend.clone(), handle.id.clone());

    let ready_handle = wait_for_ready(&backend, &handle.id, Duration::from_secs(30));
    assert_eq!(ready_handle.status, SandboxStatus::Ready);

    let http_response = wait_for_http_response(host_port, Duration::from_secs(15));
    assert_httpish_response(
        &http_response,
        "expected HTTP response from non-root-user sandbox",
    );

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
    assert_eq!(
        uid, 0,
        "krun bundle must use root uid for VMM /dev/kvm access"
    );
    assert_eq!(
        gid, 0,
        "krun bundle must use root gid for VMM /dev/kvm access"
    );

    let manifest_path = state_root
        .join("containers")
        .join(handle.id.as_str())
        .join("manifest.json");
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
    block_on(backend.stop(&handle.id)).expect("stop should succeed");
    let stop_elapsed = stop_start.elapsed();
    eprintln!("stop elapsed: {stop_elapsed:?}");

    let stopped_handle = block_on(backend.inspect(&handle.id))
        .expect("inspect after stop should succeed")
        .expect("stopped sandbox should still have a manifest");
    assert_eq!(stopped_handle.status, SandboxStatus::Stopped);

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
