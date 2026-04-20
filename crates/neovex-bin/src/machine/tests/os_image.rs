use super::*;

#[test]
fn machine_os_apply_updates_config_and_invalidates_materialized_artifacts() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    run_machine_command_for_test(
        MachineCommand {
            command: MachineSubcommand::Init(MachineInitCommand {
                cpus: DEFAULT_MACHINE_CPUS,
                memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
                disk_gib: DEFAULT_MACHINE_DISK_GIB,
                image: "docker://ghcr.io/agentstation/neovex-machine-os:v0.1.0".to_owned(),
                ssh_identity: None,
                ignition_file: None,
                efi_store: None,
                volumes: Vec::new(),
                now: false,
                name: None,
            }),
        },
        &layout,
    )
    .expect("machine init should succeed");

    let paths = layout.paths(DEFAULT_MACHINE_NAME);
    fs::write(&paths.materialized_image_path, b"old-image").expect("image path should write");
    fs::write(&paths.efi_variable_store_path, b"old-efi").expect("efi store should write");

    run_machine_command_for_test(
        MachineCommand {
            command: MachineSubcommand::Os(MachineOsCommand {
                command: MachineOsSubcommand::Apply(MachineOsApplyCommand {
                    image: format!(
                        "docker://{DEFAULT_NEOVEX_MACHINE_IMAGE_REPOSITORY}:{}",
                        current_machine_release_tag()
                    ),
                    restart: false,
                }),
            }),
        },
        &layout,
    )
    .expect("machine os apply should succeed");

    let config = read_json_file_if_exists::<MachineConfigRecord>(&paths.config_path)
        .expect("config should read")
        .expect("config should exist");
    let state = read_json_file_if_exists::<MachineStateRecord>(&paths.state_path)
        .expect("state should read")
        .expect("state should exist");

    assert_eq!(
        config.guest.image_source,
        MachineImageSource::OciReference {
            reference: format!(
                "docker://{DEFAULT_NEOVEX_MACHINE_IMAGE_REPOSITORY}:{}",
                current_machine_release_tag()
            ),
        }
    );
    assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
    assert_eq!(state.manager, MachineManagerState::Unconfigured);
    assert!(!paths.materialized_image_path.exists());
    assert!(!paths.efi_variable_store_path.exists());
}

#[test]
fn machine_os_upgrade_plan_uses_supported_stream_target() {
    let config = MachineConfigRecord {
        version: CURRENT_MACHINE_CONFIG_VERSION,
        name: DEFAULT_MACHINE_NAME.to_owned(),
        provider: MachineProvider::Krunkit,
        guest: MachineGuestConfig {
            image_source: MachineImageSource::OciReference {
                reference: supported_stream_current_image_for_upgrade_test(),
            },
            ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
            ssh_identity_path: None,
            ignition_file_path: None,
            efi_variable_store_path: None,
        },
        resources: MachineResources {
            cpus: DEFAULT_MACHINE_CPUS,
            memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
            disk_gib: DEFAULT_MACHINE_DISK_GIB,
        },
        volumes: Vec::new(),
        roots: MachineRootLayout::new(
            PathBuf::from("/tmp/config"),
            PathBuf::from("/tmp/state"),
            PathBuf::from("/tmp/runtime"),
        ),
    };

    let plan = plan_machine_os_upgrade(&config).expect("upgrade plan should resolve");

    assert_eq!(
        plan.current_image,
        supported_stream_current_image_for_upgrade_test()
    );
    assert_eq!(
        plan.current_version,
        if cfg!(target_os = "macos") {
            "sha256:abc123"
        } else {
            "v0.1.0"
        }
    );
    assert_eq!(plan.target_image, default_machine_image());
    assert_eq!(plan.target_version, expected_upgrade_target_version());
    assert!(plan.update_available);
}

#[test]
fn host_managed_macos_stream_uses_podman_repository_contract() {
    let config = MachineConfigRecord {
        version: CURRENT_MACHINE_CONFIG_VERSION,
        name: DEFAULT_MACHINE_NAME.to_owned(),
        provider: MachineProvider::Krunkit,
        guest: MachineGuestConfig {
            image_source: MachineImageSource::OciReference {
                reference: default_machine_image(),
            },
            ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
            ssh_identity_path: None,
            ignition_file_path: None,
            efi_variable_store_path: None,
        },
        resources: MachineResources {
            cpus: DEFAULT_MACHINE_CPUS,
            memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
            disk_gib: DEFAULT_MACHINE_DISK_GIB,
        },
        volumes: Vec::new(),
        roots: MachineRootLayout::new(
            PathBuf::from("/tmp/config"),
            PathBuf::from("/tmp/state"),
            PathBuf::from("/tmp/runtime"),
        ),
    };

    let desired = desired_machine_image_source(&config);

    assert_eq!(desired, config.guest.image_source);
    assert_eq!(
        uses_host_managed_machine_image_contract(&config),
        cfg!(target_os = "macos")
    );
}

#[test]
fn explicit_podman_override_does_not_get_rewritten_to_default_digest() {
    let config = MachineConfigRecord {
        version: CURRENT_MACHINE_CONFIG_VERSION,
        name: DEFAULT_MACHINE_NAME.to_owned(),
        provider: MachineProvider::Krunkit,
        guest: MachineGuestConfig {
            image_source: MachineImageSource::OciReference {
                reference: "docker://quay.io/podman/machine-os@sha256:customoverride".to_owned(),
            },
            ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
            ssh_identity_path: None,
            ignition_file_path: None,
            efi_variable_store_path: None,
        },
        resources: MachineResources {
            cpus: DEFAULT_MACHINE_CPUS,
            memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
            disk_gib: DEFAULT_MACHINE_DISK_GIB,
        },
        volumes: Vec::new(),
        roots: MachineRootLayout::new(
            PathBuf::from("/tmp/config"),
            PathBuf::from("/tmp/state"),
            PathBuf::from("/tmp/runtime"),
        ),
    };

    assert_eq!(
        desired_machine_image_source(&config),
        config.guest.image_source
    );
}

#[test]
fn machine_os_upgrade_handles_digest_pinned_supported_streams() {
    let config = MachineConfigRecord {
        version: CURRENT_MACHINE_CONFIG_VERSION,
        name: DEFAULT_MACHINE_NAME.to_owned(),
        provider: MachineProvider::Krunkit,
        guest: MachineGuestConfig {
            image_source: MachineImageSource::OciReference {
                reference: supported_stream_digest_image_for_upgrade_test(),
            },
            ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
            ssh_identity_path: None,
            ignition_file_path: None,
            efi_variable_store_path: None,
        },
        resources: MachineResources {
            cpus: DEFAULT_MACHINE_CPUS,
            memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
            disk_gib: DEFAULT_MACHINE_DISK_GIB,
        },
        volumes: Vec::new(),
        roots: MachineRootLayout::new(
            PathBuf::from("/tmp/config"),
            PathBuf::from("/tmp/state"),
            PathBuf::from("/tmp/runtime"),
        ),
    };

    if cfg!(target_os = "macos") {
        let plan = plan_machine_os_upgrade(&config).expect("macOS digest streams should resolve");
        assert_eq!(
            plan.current_image,
            supported_stream_digest_image_for_upgrade_test()
        );
        assert_eq!(plan.current_version, "sha256:abc123");
        assert_eq!(plan.target_image, default_machine_image());
        assert_eq!(plan.target_version, expected_upgrade_target_version());
        assert!(plan.update_available);
    } else {
        let error = plan_machine_os_upgrade(&config).expect_err("linux digest streams should fail");
        assert!(error.to_string().contains("digest-pinned"));
    }
}

#[test]
fn machine_image_source_parse_supports_published_local_and_url_sources() {
    assert_eq!(
        MachineImageSource::parse("ghcr.io/agentstation/neovex-machine-os:test")
            .expect("bare registry ref should parse"),
        MachineImageSource::OciReference {
            reference: "docker://ghcr.io/agentstation/neovex-machine-os:test".to_owned(),
        }
    );
    assert_eq!(
        MachineImageSource::parse("https://example.com/neovex-machine.raw.zst")
            .expect("url should parse"),
        MachineImageSource::HttpUrl {
            url: "https://example.com/neovex-machine.raw.zst".to_owned(),
        }
    );
    assert_eq!(
        MachineImageSource::parse("/tmp/neovex-machine.raw").expect("path should parse"),
        MachineImageSource::LocalDisk {
            path: PathBuf::from("/tmp/neovex-machine.raw"),
        }
    );
}
