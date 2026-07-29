use super::*;
use crate::machine::manager::image::is_loopback_registry;
use sha2::{Digest, Sha256};

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn loopback_registry_detection_requires_exact_loopback_host() {
    for reference in [
        "docker://localhost:5000/nimbus/machine-os:test",
        "docker://LOCALHOST:5000/nimbus/machine-os:test",
        "docker://127.0.0.1:5000/nimbus/machine-os:test",
        "docker://127.42.0.9:5000/nimbus/machine-os:test",
        "docker://[::1]:5000/nimbus/machine-os:test",
    ] {
        assert!(
            is_loopback_registry(reference),
            "{reference} should use local plaintext registry transport"
        );
    }

    for reference in [
        "docker://localhost.evil.com/nimbus/machine-os:test",
        "docker://127.0.0.1.attacker.tld/nimbus/machine-os:test",
        "docker://[::1].attacker.tld/nimbus/machine-os:test",
        "docker://example.com/nimbus/machine-os:test",
    ] {
        assert!(
            !is_loopback_registry(reference),
            "{reference} must not use local plaintext registry transport"
        );
    }
}

#[test]
fn converge_machine_image_contract_rebuilds_boot_artifacts_when_recorded_image_drifted() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let mut config = sample_config(&image_path);
    config.guest.image_source = MachineImageSource::OciReference {
        reference: default_machine_image_for_provider(MachineProvider::Krunkit),
    };
    let paths = config.roots.paths("default");
    paths
        .ensure_directories()
        .expect("machine directories should exist");
    fs::write(&paths.materialized_image_path, b"old-image").expect("image should write");
    fs::write(&paths.efi_variable_store_path, b"old-efi").expect("efi store should write");

    let mut state = MachineStateRecord::initialized();
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            vmm: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path: paths.materialized_image_path.clone(),
        efi_variable_store_path: paths.efi_variable_store_path.clone(),
        machine_image_source: "docker://quay.io/podman/machine-os@sha256:old-digest".to_owned(),
        ssh_listener_id: fixture_machine_ssh_listener_id("launch-image"),
        forwarder_authority: test_forwarder_authority(&config),
        ssh_port: 20022,
        rest_uri: format!("unix://{}", paths.vmm_endpoint_path.display()),
        ready_vsock_port: READY_VSOCK_PORT,
    });

    converge_machine_image_contract(&paths, &mut config, &mut state)
        .expect("contract convergence should succeed");

    assert_eq!(
        config.guest.image_source,
        MachineImageSource::OciReference {
            reference: default_machine_image_for_provider(MachineProvider::Krunkit),
        }
    );
    assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
    assert_eq!(state.manager, MachineManagerState::Stale);
    assert!(
        state
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("boot artifacts were reset")
    );
    assert!(!paths.materialized_image_path.exists());
    assert!(!paths.efi_variable_store_path.exists());
}

#[test]
fn machine_image_rebuild_reason_requires_rebuild_when_boot_artifacts_lack_identity() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    paths
        .ensure_directories()
        .expect("machine directories should exist");
    fs::write(&paths.materialized_image_path, b"old-image").expect("image should write");

    let reason = machine_image_rebuild_reason(
        &paths,
        &MachineStateRecord::initialized(),
        "docker://quay.io/podman/machine-os@sha256:test",
    )
    .expect("boot artifacts without recorded identity should rebuild");

    assert!(reason.contains("without a recorded base-image identity"));
}

#[test]
fn launch_plan_requires_bootable_local_disk_image() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    let state = MachineStateRecord::initialized();
    let port_authority = test_port_authority(temp_dir.path());
    let plan = MachineLaunchPlan::build(&port_authority, &paths, &config, &state)
        .expect("launch plan should build");

    assert!(
        plan.vmm_command
            .args
            .iter()
            .any(|arg| arg.contains("virtio-blk,path="))
    );
    assert!(
        plan.vmm_command
            .args
            .iter()
            .any(|arg| arg.contains("virtio-net,type=unixgram"))
    );
    assert!(plan.vmm_command.args.iter().any(|arg| {
        arg == &format!(
            "virtio-vsock,port=1025,socketURL={},listen",
            paths.ready_socket_path.display()
        )
    }));
    assert!(plan.vmm_command.args.iter().any(|arg| {
        arg == &format!(
            "virtio-vsock,port=1024,socketURL={},listen",
            paths.ignition_socket_path.display()
        )
    }));
    assert!(
        !plan
            .gvproxy_command
            .args
            .iter()
            .any(|arg| arg == "-forward-sock")
    );
    assert_eq!(
        plan.ignition_file_path,
        Some(paths.generated_ignition_path.clone())
    );
}

#[test]
fn launch_plan_bootc_machine_config_attaches_bundle_without_ignition() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
    let image_path = temp_dir.path().join("disk.raw");
    let ssh_identity_path = temp_dir.path().join("machine-key");
    let ssh_public_key_path = temp_dir.path().join("machine-key.pub");
    fs::write(&image_path, []).expect("image should write");
    fs::write(&ssh_identity_path, "fake key").expect("identity should write");
    fs::write(
        &ssh_public_key_path,
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestKey jack@example",
    )
    .expect("public key should write");

    let mut config = sample_config(&image_path);
    config.guest.provisioning = MachineGuestProvisioning::BootcMachineConfig;
    config.guest.ssh_user = DEFAULT_BOOTC_MACHINE_SSH_USER.to_owned();
    config.guest.ssh_identity_path = Some(ssh_identity_path);
    let paths = config.roots.paths("default");
    let state = MachineStateRecord::initialized();
    let port_authority = test_port_authority(temp_dir.path());

    let plan = MachineLaunchPlan::build(&port_authority, &paths, &config, &state)
        .expect("launch plan should build");

    assert_eq!(
        plan.machine_config_bundle_dir,
        Some(paths.guest_config_bundle_dir.clone())
    );
    assert_eq!(plan.ignition_file_path, None);
    assert!(paths.guest_config_bundle_dir.join("machine.json").is_file());
    assert!(plan.vmm_command.args.iter().any(|arg| {
        arg == &format!(
            "virtio-vsock,port=1025,socketURL={},listen",
            paths.ready_socket_path.display()
        )
    }));
    assert!(!plan.vmm_command.args.iter().any(|arg| {
        arg == &format!(
            "virtio-vsock,port=1024,socketURL={},listen",
            paths.ignition_socket_path.display()
        )
    }));
    assert!(plan.vmm_command.args.iter().any(|arg| {
        arg == &format!(
            "virtio-fs,sharedDir={},mountTag=nimbus-machine-config",
            paths.guest_config_bundle_dir.display()
        )
    }));
}

#[test]
fn launch_plan_keeps_gvproxy_network_only_and_builds_separate_machine_api_forward() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
    let image_path = temp_dir.path().join("disk.raw");
    let ssh_identity_path = temp_dir.path().join("machine-key");
    let ssh_public_key_path = temp_dir.path().join("machine-key.pub");
    fs::write(&image_path, []).expect("image should write");
    fs::write(&ssh_identity_path, "fake key").expect("identity should write");
    fs::write(
        &ssh_public_key_path,
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestKey jack@example",
    )
    .expect("public key should write");

    let mut config = sample_config(&image_path);
    config.guest.ssh_identity_path = Some(ssh_identity_path.clone());

    let paths = config.roots.paths("default");
    let state = MachineStateRecord::initialized();
    let port_authority = test_port_authority(temp_dir.path());
    let plan = MachineLaunchPlan::build(&port_authority, &paths, &config, &state)
        .expect("launch plan should build");

    assert!(!plan.gvproxy_command.args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "-forward-sock" | "-forward-dest" | "-forward-user" | "-forward-identity"
        )
    }));
    let forward = build_machine_api_forward_command(&paths, &config, 20022)
        .expect("machine API forward command should build");
    let forward_args = forward
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(forward_args.windows(2).any(|pair| pair[0] == "-L"
        && pair[1] == format!("{}:{GUEST_NIMBUS_SOCKET}", paths.api_socket_path.display())));
    assert!(
        forward_args
            .windows(2)
            .any(|pair| pair[0] == "-i" && pair[1] == ssh_identity_path.display().to_string())
    );
    assert!(forward_args.iter().any(|arg| arg == "root@127.0.0.1"));
    assert!(
        forward_args
            .windows(2)
            .any(|pair| { pair[0] == "-o" && pair[1] == "ExitOnForwardFailure=yes" })
    );
}

#[test]
fn build_virtio_vsock_listen_arg_matches_podman_listen_mode() {
    let socket_path = Path::new("/tmp/nimbus-test.sock");

    assert_eq!(
        build_virtio_vsock_listen_arg(1024, socket_path),
        "virtio-vsock,port=1024,socketURL=/tmp/nimbus-test.sock,listen"
    );
}

#[test]
fn registry_image_reference_materializes_raw_disk_from_oci_registry() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::test_sibling_roots(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("default");
    let raw_payload = b"raw-disk-oci-bytes".to_vec();
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&raw_payload)
        .expect("gzip payload should write");
    let gzip_payload = encoder.finish().expect("gzip payload should finish");
    let reference = serve_fake_oci_registry(gzip_payload);

    let materialized = resolve_bootable_image_path(
        &paths,
        &MachineImageSource::OciReference { reference },
        MachineProvider::Krunkit,
    )
    .expect("registry image should materialize");

    assert_eq!(materialized, paths.materialized_image_path);
    assert_eq!(
        fs::read(&paths.materialized_image_path).expect("materialized image should read"),
        raw_payload
    );
}

#[test]
fn registry_image_reference_reuses_materialized_disk_when_present() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
    let layout = MachineRootLayout::test_sibling_roots(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("default");
    fs::create_dir_all(&paths.image_cache_dir).expect("image cache dir should exist");
    fs::create_dir_all(
        paths
            .materialized_image_path
            .parent()
            .expect("materialized image parent should exist"),
    )
    .expect("materialized image parent should exist");
    fs::write(&paths.materialized_image_path, []).expect("materialized image should write");

    let config = MachineConfigRecord {
        version: CURRENT_MACHINE_CONFIG_VERSION,
        name: "default".to_owned(),
        provider: MachineProvider::Krunkit,
        guest: MachineGuestConfig {
            image_source: MachineImageSource::OciReference {
                reference: "docker://quay.io/podman/machine-os@sha256:legacy-cached".to_owned(),
            },
            provisioning: MachineGuestProvisioning::Ignition,
            ssh_user: "core".to_owned(),
            ssh_identity_path: None,
            ignition_file_path: None,
            efi_variable_store_path: None,
        },
        resources: MachineResources {
            cpus: 2,
            memory_mib: 2048,
            disk_gib: 20,
        },
        volumes: Vec::new(),
        roots: layout.clone(),
        network_authority: test_network_authority_record(temp_dir.path(), "default"),
    };

    let port_authority = test_port_authority(temp_dir.path());
    let plan = MachineLaunchPlan::build(
        &port_authority,
        &paths,
        &config,
        &MachineStateRecord::initialized(),
    )
    .expect("materialized disk should satisfy launch plan");

    assert_eq!(plan.runtime.image_path, paths.materialized_image_path);
    assert!(
        plan.vmm_command
            .args
            .iter()
            .any(|arg| arg.contains(&format!(
                "virtio-blk,path={}",
                paths.materialized_image_path.display()
            )))
    );
}

#[test]
fn http_image_source_materializes_raw_disk_into_reserved_path() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::test_sibling_roots(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("default");
    let payload = b"raw-disk-bytes".to_vec();
    let sha256 = sha256_hex(&payload);
    let url = serve_single_http_response(payload.clone(), None);

    let materialized = resolve_bootable_image_path(
        &paths,
        &MachineImageSource::HttpUrl {
            url: url.clone(),
            sha256,
        },
        MachineProvider::Krunkit,
    )
    .expect("http source should materialize");

    assert_eq!(materialized, paths.materialized_image_path);
    assert_eq!(
        fs::read(&paths.materialized_image_path).expect("materialized image should read"),
        payload
    );
}

#[test]
fn http_image_source_rejects_sha256_mismatch_before_persisting_disk() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::test_sibling_roots(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("default");
    let payload = b"raw-disk-bytes".to_vec();
    let url = serve_single_http_response(payload, None);

    let error = resolve_bootable_image_path(
        &paths,
        &MachineImageSource::HttpUrl {
            url,
            sha256: "0000000000000000000000000000000000000000000000000000000000000000".to_owned(),
        },
        MachineProvider::Krunkit,
    )
    .expect_err("sha256 mismatch should reject http image");

    assert!(error.to_string().contains("has sha256"));
    assert!(
        !paths.materialized_image_path.exists(),
        "mismatched HTTP image must not be persisted as a boot disk"
    );
}

#[test]
fn http_image_source_rejects_plaintext_non_loopback_before_download() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::test_sibling_roots(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("default");

    let error = resolve_bootable_image_path(
        &paths,
        &MachineImageSource::HttpUrl {
            url: "http://example.com/disk.raw".to_owned(),
            sha256: "0000000000000000000000000000000000000000000000000000000000000000".to_owned(),
        },
        MachineProvider::Krunkit,
    )
    .expect_err("plaintext remote http image should fail before download");

    assert!(
        error
            .to_string()
            .contains("plaintext HTTP machine image sources are only allowed for loopback hosts")
    );
    assert!(
        !paths.materialized_image_path.exists(),
        "rejected plaintext HTTP image must not be persisted as a boot disk"
    );
}

#[test]
fn cached_zstd_machine_image_materializes_into_reserved_path() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let source_path = temp_dir.path().join("disk.raw.zst");
    let output_path = temp_dir.path().join("disk.raw");
    let payload = b"raw-disk-zstd-bytes".to_vec();
    let compressed = zstd::stream::encode_all(std::io::Cursor::new(&payload), 1)
        .expect("zstd payload should encode");
    fs::write(&source_path, compressed).expect("compressed source should write");

    materialize_cached_disk(&source_path, &output_path, "test zstd image")
        .expect("zstd image should materialize");

    assert_eq!(
        fs::read(&output_path).expect("materialized image should read"),
        payload
    );
}

#[test]
fn http_gzip_image_source_materializes_decompressed_disk_into_reserved_path() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::test_sibling_roots(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("default");
    let payload = b"raw-disk-gzip-bytes".to_vec();
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&payload)
        .expect("gzip payload should write");
    let gzip_payload = encoder.finish().expect("gzip payload should finish");
    let sha256 = sha256_hex(&payload);
    let url = serve_single_http_response(gzip_payload, Some("/disk.raw.gz"));

    let materialized = resolve_bootable_image_path(
        &paths,
        &MachineImageSource::HttpUrl {
            url: url.clone(),
            sha256,
        },
        MachineProvider::Krunkit,
    )
    .expect("gzip http source should materialize");

    assert_eq!(materialized, paths.materialized_image_path);
    assert_eq!(
        fs::read(&paths.materialized_image_path).expect("materialized image should read"),
        payload
    );
}
