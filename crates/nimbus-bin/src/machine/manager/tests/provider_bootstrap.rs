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

    // vfkit is an applehv sibling of krunkit: it must report the identical
    // podman-aligned capability contract so the shared launch/bootstrap path
    // treats both managed applehv guests the same way.
    assert!(!MachineProvider::Vfkit.uses_provider_networking());
    assert!(MachineProvider::Vfkit.requires_exclusive_active());
    assert_eq!(
        MachineProvider::Vfkit.image_format(),
        MachineImageFormat::Raw
    );
    assert_eq!(
        MachineProvider::Vfkit.bootstrap_mode(),
        MachineBootstrapMode::Ignition
    );
    assert_eq!(MachineProvider::Vfkit.oci_artifact_disk_type(), "applehv");
    assert!(MachineProvider::Vfkit.uses_managed_applehv_guest());
    assert!(MachineProvider::Krunkit.uses_managed_applehv_guest());
    assert!(!MachineProvider::Wsl2.uses_managed_applehv_guest());

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
fn krunkit_backend_pairs_gvproxy_unixgram_listen_mode() {
    let backend = KrunkitVmmBackend;
    assert_eq!(backend.provider(), MachineProvider::Krunkit);
    // krunkit drives host networking through gvproxy.
    assert!(backend.requires_gvproxy());

    // The host side listens on a unixgram socket and the krunkit
    // `virtio-net,type=unixgram` device dials it. gvproxy's `-listen-vfkit`
    // mode speaks exactly that wire format, so the listen arguments must pair
    // the flag with a `unixgram://` URL pointing at the shared socket.
    let socket = PathBuf::from("/tmp/nimbus-machine/gvproxy.sock");
    assert_eq!(
        backend.gvproxy_listen_args(&socket),
        vec![
            "-listen-vfkit".to_owned(),
            format!("unixgram://{}", socket.display()),
        ]
    );
}

#[test]
fn vfkit_backend_pairs_gvproxy_unixgram_listen_mode() {
    let backend = VfkitVmmBackend;
    assert_eq!(backend.provider(), MachineProvider::Vfkit);
    // vfkit, like krunkit, drives host networking through gvproxy.
    assert!(backend.requires_gvproxy());

    // vfkit's `virtio-net,unixSocketPath=` device dials gvproxy's `-listen-vfkit`
    // unixgram listener. The host listen contract is identical to krunkit (krunkit
    // reuses vfkit's transport), so the listen arguments must pair the flag with a
    // `unixgram://` URL at the shared socket; only the on-VMM net-device grammar in
    // `build_launch_command` differs between the two backends.
    let socket = PathBuf::from("/tmp/nimbus-machine/gvproxy.sock");
    assert_eq!(
        backend.gvproxy_listen_args(&socket),
        vec![
            "-listen-vfkit".to_owned(),
            format!("unixgram://{}", socket.display()),
        ]
    );
}

#[test]
fn vfkit_backend_resolves_binary_from_env_override() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let vfkit_path = temp_dir.path().join("vfkit");
    // The env guard installs the vfkit stub and points NIMBUS_MACHINE_VFKIT at it,
    // so resolution must honor the per-VMM override ahead of the bundled/known
    // helper directories.
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());

    let resolved = VfkitVmmBackend
        .resolve_vmm_binary()
        .expect("vfkit binary should resolve via NIMBUS_MACHINE_VFKIT");

    assert_eq!(resolved, vfkit_path);
}

/// Build a VMM launch command for `backend` against the deterministic sample
/// config so the per-VMM device grammar can be asserted without booting a VM.
/// `build_launch_command` only formats paths, so no filesystem state is needed.
fn build_sample_launch_command(
    backend: &dyn MachineVmmBackend,
    image_path: &Path,
    paths: &MachinePaths,
    config: &MachineConfigRecord,
) -> MachineCommandLine {
    let efi_variable_store_path = paths.efi_variable_store_path.clone();
    let rest_uri = format!("unix://{}", paths.vmm_endpoint_path.display());
    let ctx = VmmLaunchContext {
        paths,
        config,
        image_path,
        efi_variable_store_path: &efi_variable_store_path,
        rest_uri: &rest_uri,
        bootstrap_mode: MachineBootstrapMode::Ignition,
        machine_config_bundle_dir: None,
    };
    backend
        .build_launch_command(Path::new("/opt/test/vmm"), &ctx)
        .expect("launch command should build")
}

#[test]
fn krunkit_build_launch_command_uses_libkrun_device_grammar() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    let command = build_sample_launch_command(&KrunkitVmmBackend, &image_path, &paths, &config);

    // Shared base args: CPU/memory sizing, the restful control endpoint, and the
    // pidfile slot the readiness/stop lifecycle watches.
    assert!(
        command
            .args
            .windows(2)
            .any(|pair| pair[0] == "--cpus" && pair[1] == "2")
    );
    assert!(
        command
            .args
            .windows(2)
            .any(|pair| pair[0] == "--memory" && pair[1] == "2048")
    );
    assert!(
        command
            .args
            .windows(2)
            .any(|pair| pair[0] == "--pidfile"
                && pair[1] == paths.vmm_pid_path.display().to_string())
    );

    // krunkit exposes its own diagnostic --log-file; vfkit does not.
    assert!(
        command
            .args
            .windows(2)
            .any(|pair| pair[0] == "--log-file"
                && pair[1] == paths.vmm_log_path.display().to_string())
    );

    // krunkit's block device carries the raw format, and its net device is the
    // libkrun unixgram grammar with offloading + vfkitMagic.
    assert!(
        command
            .args
            .iter()
            .any(|arg| arg == &format!("virtio-blk,path={},format=raw", image_path.display()))
    );
    assert!(command.args.iter().any(|arg| arg
        == &format!(
            "virtio-net,type=unixgram,path={},mac={},offloading=on,vfkitMagic=on",
            paths.gvproxy_socket_path.display(),
            DEFAULT_MACHINE_MAC_ADDRESS
        )));

    // Shared applehv devices: machine-ready + Ignition vsock listeners.
    assert!(command.args.iter().any(
        |arg| arg == &build_virtio_vsock_listen_arg(READY_VSOCK_PORT, &paths.ready_socket_path)
    ));
    assert!(
        command
            .args
            .iter()
            .any(|arg| arg == &build_virtio_vsock_listen_arg(1024, &paths.ignition_socket_path))
    );
}

#[test]
fn vfkit_build_launch_command_uses_virtualization_framework_device_grammar() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    let command = build_sample_launch_command(&VfkitVmmBackend, &image_path, &paths, &config);

    // vfkit shares the base args (sizing, restful URI, pidfile) with krunkit.
    assert!(
        command
            .args
            .windows(2)
            .any(|pair| pair[0] == "--cpus" && pair[1] == "2")
    );
    assert!(
        command
            .args
            .windows(2)
            .any(|pair| pair[0] == "--restful-uri"
                && pair[1] == format!("unix://{}", paths.vmm_endpoint_path.display()))
    );

    // vfkit has no --log-file flag; the guest console still lands in the shared
    // serial log device below.
    assert!(!command.args.iter().any(|arg| arg == "--log-file"));

    // vfkit's block device omits `format=`, and its net device uses
    // `unixSocketPath=` rather than the libkrun `type=unixgram` grammar.
    assert!(
        command
            .args
            .iter()
            .any(|arg| arg == &format!("virtio-blk,path={}", image_path.display()))
    );
    assert!(!command.args.iter().any(|arg| arg.contains("format=raw")));
    assert!(command.args.iter().any(|arg| arg
        == &format!(
            "virtio-net,unixSocketPath={},mac={}",
            paths.gvproxy_socket_path.display(),
            DEFAULT_MACHINE_MAC_ADDRESS
        )));
    assert!(!command.args.iter().any(|arg| arg.contains("type=unixgram")));

    // Shared applehv devices are wired identically to krunkit: the serial console
    // log plus the machine-ready and Ignition vsock listeners.
    assert!(command.args.iter().any(|arg| arg
        == &format!(
            "virtio-serial,logFilePath={}",
            paths.machine_log_path.display()
        )));
    assert!(command.args.iter().any(
        |arg| arg == &build_virtio_vsock_listen_arg(READY_VSOCK_PORT, &paths.ready_socket_path)
    ));
    assert!(
        command
            .args
            .iter()
            .any(|arg| arg == &build_virtio_vsock_listen_arg(1024, &paths.ignition_socket_path))
    );
}

#[test]
fn applehv_backends_emit_one_virtiofs_device_per_user_volume() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    let mut config = sample_config(&image_path);
    // The macOS default host-share set: each entry must surface as its own
    // virtio-fs device in the launch command, not be collapsed, reordered into a
    // single mount, or dropped. Distinct sources + distinct targets so each
    // device string (and its digest-derived mount tag) is unique.
    config.volumes = vec![
        MachineVolume {
            source: PathBuf::from("/Users"),
            target: PathBuf::from("/Users"),
        },
        MachineVolume {
            source: PathBuf::from("/private"),
            target: PathBuf::from("/private"),
        },
        MachineVolume {
            source: PathBuf::from("/var/folders"),
            target: PathBuf::from("/var/folders"),
        },
    ];
    let paths = config.roots.paths("default");

    // Both applehv backends route user volumes through the shared
    // `push_shared_applehv_devices` path, so the host-dir mount emission must be
    // byte-identical across krunkit and vfkit — neither backend's block/net
    // grammar may perturb the virtio-fs devices.
    for backend in [
        &KrunkitVmmBackend as &dyn MachineVmmBackend,
        &VfkitVmmBackend as &dyn MachineVmmBackend,
    ] {
        let provider = backend.provider();
        let command = build_sample_launch_command(backend, &image_path, &paths, &config);

        for volume in &config.volumes {
            // The guest mounts each share by the digest-derived tag of its
            // target, so the expected device is computed from the production
            // formatter rather than a hardcoded digest.
            let expected = build_virtiofs_arg(&volume.source, &mount_tag(&volume.target));
            let occurrences = command.args.iter().filter(|arg| *arg == &expected).count();
            assert_eq!(
                occurrences,
                1,
                "{provider:?} backend should emit exactly one virtio-fs device for {} (expected `{expected}`)",
                volume.source.display(),
            );
        }

        // `build_sample_launch_command` passes no machine-config bundle, so every
        // virtio-fs device originates from a user volume: the count must equal the
        // number of volumes exactly, proving none are coalesced or duplicated.
        let virtiofs_device_count = command
            .args
            .iter()
            .filter(|arg| arg.starts_with("virtio-fs,sharedDir="))
            .count();
        assert_eq!(
            virtiofs_device_count,
            config.volumes.len(),
            "{provider:?} backend should emit one virtio-fs device per user volume",
        );

        // Each virtio-fs device must be introduced by its own `--device` flag, so
        // the volumes contribute one `--device`/`virtio-fs,…` pair apiece.
        let device_flagged_virtiofs = command
            .args
            .windows(2)
            .filter(|pair| pair[0] == "--device" && pair[1].starts_with("virtio-fs,sharedDir="))
            .count();
        assert_eq!(
            device_flagged_virtiofs,
            config.volumes.len(),
            "{provider:?} backend should precede every virtio-fs device with its own --device flag",
        );
    }
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
fn build_digest_reference_replaces_tag_and_existing_digest() {
    let child = build_digest_reference(
        "docker://ghcr.io/nimbus/machine-os:v0.1.30@sha256:1111111111111111111111111111111111111111111111111111111111111111",
        "sha256:2222222222222222222222222222222222222222222222222222222222222222",
    )
    .expect("child digest reference should build");

    assert_eq!(child.registry(), "ghcr.io");
    assert_eq!(child.repository(), "nimbus/machine-os");
    assert_eq!(child.tag(), None);
    assert_eq!(
        child.digest(),
        Some("sha256:2222222222222222222222222222222222222222222222222222222222222222")
    );
    assert_eq!(
        child.to_string(),
        "ghcr.io/nimbus/machine-os@sha256:2222222222222222222222222222222222222222222222222222222222222222"
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
fn bootc_machine_os_uses_baked_nimbus_binary_without_host_sync() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let mut config = sample_config(&image_path);
    config.guest.image_source = MachineImageSource::OciReference {
        reference: "docker://ghcr.io/nimbus/machine-os:v0.1.22".to_owned(),
    };
    config.guest.provisioning = MachineGuestProvisioning::BootcMachineConfig;
    config.guest.ssh_user = DEFAULT_BOOTC_MACHINE_SSH_USER.to_owned();

    assert!(requires_bootc_machine_config(&config));
    assert!(!requires_host_guest_nimbus_sync(&config));

    let error = validate_machine_bootstrap_contract(&config)
        .expect_err("bootc-native provisioning should still require machine SSH identity");
    assert!(error.to_string().contains("bootc-native"));
    assert!(error.to_string().contains("--identity"));
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
