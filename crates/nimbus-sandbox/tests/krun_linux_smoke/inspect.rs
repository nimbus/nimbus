use super::support::*;
use super::*;

/// M3 verification: prove image USER is finally applied inside the guest via
/// the mounted nimbus guest helper, while the host-side VMM still runs as root.
#[test]
#[ignore = "requires a Linux host with KVM, conmon, a mounted BusyBox rootfs, and a built guest helper"]
fn krun_backend_m3_guest_user_switch_applies_image_user_inside_guest() {
    let host_port = smoke_host_port(18089);
    let guest_port: u16 = 8089;

    let base_dir = env_path("NIMBUS_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("m3-user-bundles");
    let state_root = base_dir.join("m3-user-state");

    let backend = KrunSandboxBackend::new(smoke_backend_config(
        bundle_root.clone(),
        state_root.clone(),
    ));
    let guest_script = format!(
        "echo NIMBUS_UID=$(/bin/busybox id -u) >&2; \
         echo NIMBUS_GID=$(/bin/busybox id -g) >&2; \
         exec /bin/busybox httpd -f -p {guest_port}"
    );
    let mut spec = built_busybox_image_spec(
        "m3-guest-user-switch",
        "nimbus-m3-user-fixture",
        "USER www-data",
    )
    .with_port_binding(http_binding(host_port, guest_port));
    spec.process = SandboxProcessSpec::new([
        "/bin/busybox".to_owned(),
        "sh".to_owned(),
        "-c".to_owned(),
        guest_script,
    ]);

    let provisioned = provision_krun(&backend, &state_root, spec, true)
        .expect("guest-user-switch image-backed provision phases should succeed");
    assert!(!provisioned.ingress.is_empty());
    let handle = provisioned.handle;
    let teardown = provisioned.teardown;
    let cleanup_guard = CleanupGuard::new(backend.clone(), teardown.clone());
    let ingress = provisioned.ingress;

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
        "expected HTTP response from guest-user-switch sandbox",
    );

    let tenant_id = sandbox_tenant();
    let ctr_log_path = container_state_dir(&state_root, &tenant_id, &handle.id).join("ctr.log");
    let ctr_log = std::fs::read_to_string(&ctr_log_path)
        .unwrap_or_else(|_| panic!("ctr.log should be readable at {}", ctr_log_path.display()));
    let uid_line = ctr_log
        .lines()
        .find(|line| line.contains("NIMBUS_UID="))
        .expect("ctr.log should contain NIMBUS_UID=");
    let gid_line = ctr_log
        .lines()
        .find(|line| line.contains("NIMBUS_GID="))
        .expect("ctr.log should contain NIMBUS_GID=");
    let uid_value = uid_line
        .split("NIMBUS_UID=")
        .nth(1)
        .expect("NIMBUS_UID= should have a value")
        .trim();
    let gid_value = gid_line
        .split("NIMBUS_GID=")
        .nth(1)
        .expect("NIMBUS_GID= should have a value")
        .trim();
    eprintln!("guest uid={uid_value}, gid={gid_value}");
    assert_eq!(uid_value, "33", "guest should run as www-data uid 33");
    assert_eq!(gid_value, "33", "guest should run as www-data gid 33");

    let bundle_config_path = bundle_config_path(&bundle_root, &tenant_id, &handle.id);
    let bundle_config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&bundle_config_path).unwrap_or_else(|_| {
            panic!(
                "bundle config should be readable at {}",
                bundle_config_path.display()
            )
        }))
        .expect("bundle config should be valid JSON");
    assert_eq!(
        bundle_config["process"]["args"][0].as_str(),
        Some("/.nimbus/nimbus-guest-user-switch")
    );

    drop(ingress);
    retire_krun(&backend, &teardown).expect("exact teardown should succeed");
    cleanup_guard.disarm();
}
