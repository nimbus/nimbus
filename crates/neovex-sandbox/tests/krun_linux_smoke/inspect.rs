use super::support::*;
use super::*;

/// M3 verification: prove image USER is finally applied inside the guest via
/// the mounted neovex guest helper, while the host-side VMM still runs as root.
#[test]
#[ignore = "requires a Linux host with KVM, buildah, conmon, and a built guest helper"]
fn krun_backend_m3_guest_user_switch_applies_image_user_inside_guest() {
    let host_port: u16 = 18089;
    let guest_port: u16 = 8089;

    let base_dir = env_path("NEOVEX_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("m3-user-bundles");
    let state_root = base_dir.join("m3-user-state");

    let mut config = smoke_backend_config(bundle_root.clone(), state_root.clone());
    config.guest_user_helper_root = env_path("NEOVEX_KRUN_GUEST_USER_HELPER_ROOT");

    let buildah = buildah_program();
    run_host_command(&buildah, &["rm", "m3-user-fixture"], true);
    run_host_command(
        &buildah,
        &[
            "from",
            "--name",
            "m3-user-fixture",
            "docker://busybox:latest",
        ],
        false,
    );
    run_host_command(
        &buildah,
        &["config", "--user", "www-data", "m3-user-fixture"],
        false,
    );
    run_host_command(
        &buildah,
        &[
            "commit",
            "m3-user-fixture",
            "localhost/neovex-m3-user-fixture:latest",
        ],
        false,
    );
    run_host_command(&buildah, &["rm", "m3-user-fixture"], true);

    let backend = KrunSandboxBackend::new(config);
    let guest_script = format!(
        "echo NEOVEX_UID=$(id -u) >&2; \
         echo NEOVEX_GID=$(id -g) >&2; \
         exec /bin/busybox httpd -f -p {guest_port}"
    );
    let spec = empty_image_spec("m3-guest-user-switch")
        .with_port_binding(http_binding(host_port, guest_port));

    let overrides = SandboxImageProcessOverrides {
        cmd: Some(vec![
            "/bin/busybox".into(),
            "sh".into(),
            "-c".into(),
            guest_script,
        ]),
        ..Default::default()
    };

    let handle = block_on(
        backend.start_from_image(
            SandboxImageLaunchSpec::new(spec, "localhost/neovex-m3-user-fixture:latest")
                .with_process_overrides(overrides),
        ),
    )
    .expect("guest-user-switch image-backed start should succeed");
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
        "expected HTTP response from guest-user-switch sandbox",
    );

    let ctr_log_path = state_root
        .join("containers")
        .join(handle.id.as_str())
        .join("ctr.log");
    let ctr_log = std::fs::read_to_string(&ctr_log_path)
        .unwrap_or_else(|_| panic!("ctr.log should be readable at {}", ctr_log_path.display()));
    let uid_line = ctr_log
        .lines()
        .find(|line| line.contains("NEOVEX_UID="))
        .expect("ctr.log should contain NEOVEX_UID=");
    let gid_line = ctr_log
        .lines()
        .find(|line| line.contains("NEOVEX_GID="))
        .expect("ctr.log should contain NEOVEX_GID=");
    let uid_value = uid_line
        .split("NEOVEX_UID=")
        .nth(1)
        .expect("NEOVEX_UID= should have a value")
        .trim();
    let gid_value = gid_line
        .split("NEOVEX_GID=")
        .nth(1)
        .expect("NEOVEX_GID= should have a value")
        .trim();
    eprintln!("guest uid={uid_value}, gid={gid_value}");
    assert_eq!(uid_value, "33", "guest should run as www-data uid 33");
    assert_eq!(gid_value, "33", "guest should run as www-data gid 33");

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
        bundle_config["process"]["args"][0].as_str(),
        Some("/.neovex/neovex-guest-user-switch")
    );

    block_on(backend.stop(&handle.id)).expect("stop should succeed");
    cleanup_guard.disarm();
}
