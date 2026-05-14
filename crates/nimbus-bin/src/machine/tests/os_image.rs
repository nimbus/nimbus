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
                image: "docker://quay.io/podman/machine-os@sha256:legacy".to_owned(),
                ssh_identity: None,
                ignition_file: None,
                bootc_native: false,
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
                    image: default_machine_image(),
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
            reference: default_machine_image(),
        }
    );
    assert_eq!(
        config.guest.provisioning,
        MachineGuestProvisioning::BootcMachineConfig
    );
    assert_eq!(config.guest.ssh_user, DEFAULT_BOOTC_MACHINE_SSH_USER);
    assert_eq!(config.guest.ignition_file_path, None);
    assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
    assert_eq!(state.manager, MachineManagerState::Unconfigured);
    assert!(!paths.materialized_image_path.exists());
    assert!(!paths.efi_variable_store_path.exists());
}

#[test]
fn default_nimbus_image_initializes_bootc_native_machine_config() {
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
                image: default_machine_image(),
                ssh_identity: None,
                ignition_file: None,
                bootc_native: false,
                efi_store: None,
                volumes: Vec::new(),
                now: false,
                name: None,
            }),
        },
        &layout,
    )
    .expect("default Nimbus image should initialize as bootc-native after promotion");

    let config = read_json_file_if_exists::<MachineConfigRecord>(
        &layout.paths(DEFAULT_MACHINE_NAME).config_path,
    )
    .expect("config should read")
    .expect("config should exist");
    assert_eq!(
        config.guest.provisioning,
        MachineGuestProvisioning::BootcMachineConfig
    );
    assert_eq!(config.guest.ssh_user, DEFAULT_BOOTC_MACHINE_SSH_USER);
    assert_eq!(config.guest.ignition_file_path, None);
}

#[test]
fn default_nimbus_image_rejects_legacy_ignition_override() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );

    let error = run_machine_command_for_test(
        MachineCommand {
            command: MachineSubcommand::Init(MachineInitCommand {
                cpus: DEFAULT_MACHINE_CPUS,
                memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
                disk_gib: DEFAULT_MACHINE_DISK_GIB,
                image: default_machine_image(),
                ssh_identity: None,
                ignition_file: Some(temp_dir.path().join("legacy.ign")),
                bootc_native: false,
                efi_store: None,
                volumes: Vec::new(),
                now: false,
                name: None,
            }),
        },
        &layout,
    )
    .expect_err("default bootc image should reject legacy Ignition");

    let rendered = error.to_string();
    assert!(rendered.contains("cannot also use an Ignition file"));
    assert!(!layout.paths(DEFAULT_MACHINE_NAME).config_path.exists());
}

#[test]
fn bootc_machine_os_apply_requires_guest_api_without_replacing_host_disk() {
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
                image: default_machine_image(),
                ssh_identity: None,
                ignition_file: None,
                bootc_native: false,
                efi_store: None,
                volumes: Vec::new(),
                now: false,
                name: None,
            }),
        },
        &layout,
    )
    .expect("default bootc machine should initialize");

    let paths = layout.paths(DEFAULT_MACHINE_NAME);
    fs::write(&paths.materialized_image_path, b"bootc-disk").expect("image path should write");
    fs::write(&paths.efi_variable_store_path, b"bootc-efi").expect("efi store should write");

    let error = run_machine_command_for_test(
        MachineCommand {
            command: MachineSubcommand::Os(MachineOsCommand {
                command: MachineOsSubcommand::Apply(MachineOsApplyCommand {
                    image: "docker://ghcr.io/nimbus/machine-os:v9.9.9".to_owned(),
                    restart: false,
                }),
            }),
        },
        &layout,
    )
    .expect_err("bootc apply should require a running guest API");

    let rendered = error.to_string();
    assert!(rendered.contains("bootc-native machine OS changes require the guest machine API"));
    assert!(rendered.contains("run `nimbus machine start` first"));
    assert_eq!(
        fs::read(&paths.materialized_image_path).expect("materialized image should remain"),
        b"bootc-disk"
    );
    assert_eq!(
        fs::read(&paths.efi_variable_store_path).expect("efi store should remain"),
        b"bootc-efi"
    );
}

#[test]
fn bootc_machine_os_upgrade_requires_guest_api_without_replacing_host_disk() {
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
                image: default_machine_image(),
                ssh_identity: None,
                ignition_file: None,
                bootc_native: false,
                efi_store: None,
                volumes: Vec::new(),
                now: false,
                name: None,
            }),
        },
        &layout,
    )
    .expect("default bootc machine should initialize");

    let paths = layout.paths(DEFAULT_MACHINE_NAME);
    fs::write(&paths.materialized_image_path, b"bootc-disk").expect("image path should write");
    fs::write(&paths.efi_variable_store_path, b"bootc-efi").expect("efi store should write");

    let error = run_machine_command_for_test(
        MachineCommand {
            command: MachineSubcommand::Os(MachineOsCommand {
                command: MachineOsSubcommand::Upgrade(MachineOsUpgradeCommand {
                    dry_run: false,
                    restart: false,
                }),
            }),
        },
        &layout,
    )
    .expect_err("bootc upgrade should require a running guest API");

    let rendered = error.to_string();
    assert!(rendered.contains("bootc-native machine OS changes require the guest machine API"));
    assert!(rendered.contains("run `nimbus machine start` first"));
    assert_eq!(
        fs::read(&paths.materialized_image_path).expect("materialized image should remain"),
        b"bootc-disk"
    );
    assert_eq!(
        fs::read(&paths.efi_variable_store_path).expect("efi store should remain"),
        b"bootc-efi"
    );
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
            provisioning: if cfg!(target_os = "macos") {
                MachineGuestProvisioning::BootcMachineConfig
            } else {
                MachineGuestProvisioning::Ignition
            },
            ssh_user: if cfg!(target_os = "macos") {
                DEFAULT_BOOTC_MACHINE_SSH_USER.to_owned()
            } else {
                DEFAULT_MACHINE_SSH_USER.to_owned()
            },
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
fn default_macos_stream_uses_nimbus_bootc_contract() {
    let config = MachineConfigRecord {
        version: CURRENT_MACHINE_CONFIG_VERSION,
        name: DEFAULT_MACHINE_NAME.to_owned(),
        provider: MachineProvider::Krunkit,
        guest: MachineGuestConfig {
            image_source: MachineImageSource::OciReference {
                reference: default_machine_image(),
            },
            provisioning: MachineGuestProvisioning::BootcMachineConfig,
            ssh_user: DEFAULT_BOOTC_MACHINE_SSH_USER.to_owned(),
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
    assert!(!uses_host_managed_machine_image_contract(&config));
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
            provisioning: MachineGuestProvisioning::Ignition,
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
    assert_eq!(
        uses_host_managed_machine_image_contract(&config),
        cfg!(target_os = "macos")
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
            provisioning: if cfg!(target_os = "macos") {
                MachineGuestProvisioning::BootcMachineConfig
            } else {
                MachineGuestProvisioning::Ignition
            },
            ssh_user: if cfg!(target_os = "macos") {
                DEFAULT_BOOTC_MACHINE_SSH_USER.to_owned()
            } else {
                DEFAULT_MACHINE_SSH_USER.to_owned()
            },
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
        MachineImageSource::parse("ghcr.io/nimbus/machine-os:test")
            .expect("bare registry ref should parse"),
        MachineImageSource::OciReference {
            reference: "docker://ghcr.io/nimbus/machine-os:test".to_owned(),
        }
    );
    assert_eq!(
        MachineImageSource::parse("https://example.com/nimbus-machine.raw.zst")
            .expect("url should parse"),
        MachineImageSource::HttpUrl {
            url: "https://example.com/nimbus-machine.raw.zst".to_owned(),
        }
    );
    assert_eq!(
        MachineImageSource::parse("/tmp/nimbus-machine.raw").expect("path should parse"),
        MachineImageSource::LocalDisk {
            path: PathBuf::from("/tmp/nimbus-machine.raw"),
        }
    );
}
