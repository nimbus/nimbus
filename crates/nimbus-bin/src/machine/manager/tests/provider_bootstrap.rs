use super::*;

#[test]
fn krunkit_provider_capabilities_match_podman_aligned_contract() {
    assert!(!MachineProvider::Krunkit.uses_provider_networking());
    assert!(MachineProvider::Krunkit.requires_exclusive_active());
    assert_eq!(
        MachineProvider::Krunkit.image_format(),
        MachineImageFormat::Raw
    );
    assert_eq!(
        MachineProvider::Krunkit.bootstrap_mode(),
        MachineBootstrapMode::Ignition
    );
    assert_eq!(MachineProvider::Krunkit.oci_artifact_disk_type(), "applehv");
    assert!(MachineProvider::Wsl2.uses_provider_networking());
    assert!(!MachineProvider::Wsl2.requires_exclusive_active());
    assert_eq!(
        MachineProvider::Wsl2.image_format(),
        MachineImageFormat::Tar
    );
    assert_eq!(
        MachineProvider::Wsl2.bootstrap_mode(),
        MachineBootstrapMode::ShellScript
    );
    assert_eq!(MachineProvider::Wsl2.oci_artifact_disk_type(), "wsl");
}

#[test]
fn machine_image_reference_repository_strips_tag_and_digest() {
    assert_eq!(
        machine_image_reference_repository("docker://quay.io/podman/machine-os:6.0"),
        "quay.io/podman/machine-os"
    );
    assert_eq!(
        machine_image_reference_repository("docker://quay.io/podman/machine-os@sha256:abc123"),
        "quay.io/podman/machine-os"
    );
}

#[test]
fn podman_machine_os_requires_host_guest_nimbus_sync() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let mut config = sample_config(&image_path);
    config.guest.image_source = MachineImageSource::OciReference {
        reference: "docker://quay.io/podman/machine-os:6.0".to_owned(),
    };

    assert_eq!(
        requires_host_guest_nimbus_sync(&config),
        cfg!(target_os = "macos")
    );
}

#[test]
fn podman_machine_os_bootstrap_contract_requires_ssh_identity() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let mut config = sample_config(&image_path);
    config.guest.image_source = MachineImageSource::OciReference {
        reference: "docker://quay.io/podman/machine-os:6.0".to_owned(),
    };

    if cfg!(target_os = "macos") {
        let error = validate_machine_bootstrap_contract(&config)
            .expect_err("podman machine-os should require ssh identity");
        assert!(error.to_string().contains("--identity"));
    } else {
        validate_machine_bootstrap_contract(&config)
            .expect("non-macOS hosts should not require macOS SSH bootstrapping");
    }
}

#[test]
fn ensure_machine_bootstrap_identity_generates_machine_owned_key_for_host_managed_contract() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let mut config = sample_config(&image_path);
    config.guest.image_source = MachineImageSource::OciReference {
        reference: format!("docker://{DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY}@sha256:abc123"),
    };

    let paths = config.roots.paths("default");
    paths.ensure_directories().expect("paths should initialize");
    write_json_file(&paths.config_path, &config).expect("config should write");

    ensure_machine_bootstrap_identity(&paths, &mut config)
        .expect("bootstrap identity generation should succeed");

    if cfg!(target_os = "macos") {
        let identity_path = config
            .guest
            .ssh_identity_path
            .clone()
            .expect("macOS host-managed contract should record an identity path");
        let public_key_path = PathBuf::from(format!("{}.pub", identity_path.display()));
        assert_eq!(identity_path, paths.data_dir.join("machine"));
        assert!(identity_path.is_file());
        assert!(public_key_path.is_file());

        let stored: MachineConfigRecord = serde_json::from_slice(
            &fs::read(&paths.config_path).expect("config should still read"),
        )
        .expect("stored config should deserialize");
        assert_eq!(stored.guest.ssh_identity_path, Some(identity_path));
    } else {
        assert_eq!(config.guest.ssh_identity_path, None);
    }
}

#[test]
fn resolve_guest_nimbus_binary_reuses_cached_release_asset() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    paths
        .ensure_directories()
        .expect("machine directories should exist");

    let archive_name = guest_nimbus_archive_name().expect("archive name should resolve");
    let cached_binary = paths.guest_binary_cache_dir.join(format!(
        "{}-{}-nimbus",
        current_machine_release_tag(),
        archive_name.trim_end_matches(".tar.gz")
    ));
    fs::write(&cached_binary, b"cached guest binary").expect("cached binary should write");

    assert_eq!(
        resolve_guest_nimbus_binary(&paths).expect("cached guest binary should resolve"),
        cached_binary
    );
}
