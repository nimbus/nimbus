use super::*;

#[test]
fn machine_start_reports_oci_materialization_failure_for_unreachable_registry_image() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
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
                image: "docker://127.0.0.1:1/example/nimbus-machine-os:test".to_owned(),
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

    let error = run_machine_command_for_test(
        MachineCommand {
            command: MachineSubcommand::Start(MachineStartCommand::default()),
        },
        &layout,
    )
    .expect_err("machine start should surface OCI pull failure");

    let error_message = error.to_string();
    assert!(
        error_message.contains("failed to resolve machine guest OCI reference"),
        "expected OCI resolution error, got: {error_message}"
    );
}

#[test]
fn machine_start_auto_initializes_before_start_failure() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );

    let error = run_machine_command_for_test(
        MachineCommand {
            command: MachineSubcommand::Start(MachineStartCommand {
                cpus: Some(4),
                memory_mib: Some(4096),
                disk_gib: Some(40),
                image: Some("docker://127.0.0.1:1/example/nimbus-machine-os:test".to_owned()),
                ssh_identity: None,
                ignition_file: None,
                efi_store: None,
                volumes: vec![MachineVolume {
                    source: PathBuf::from("/Users"),
                    target: PathBuf::from("/Users"),
                }],
                quiet: false,
                no_info: false,
                name: None,
            }),
        },
        &layout,
    )
    .expect_err("machine start should surface OCI pull failure after auto-init");

    assert!(
        error
            .to_string()
            .contains("failed to resolve machine guest OCI reference")
    );

    let paths = layout.paths(DEFAULT_MACHINE_NAME);
    let config = read_json_file_if_exists::<MachineConfigRecord>(&paths.config_path)
        .expect("config should read")
        .expect("config should exist after auto-init");
    assert_eq!(config.resources.cpus, 4);
    assert_eq!(config.resources.memory_mib, 4096);
    assert_eq!(config.resources.disk_gib, 40);
    assert_eq!(
        config.guest.image_source,
        MachineImageSource::OciReference {
            reference: "docker://127.0.0.1:1/example/nimbus-machine-os:test".to_owned(),
        }
    );
    assert_eq!(
        config.volumes,
        vec![MachineVolume {
            source: PathBuf::from("/Users"),
            target: PathBuf::from("/Users"),
        }]
    );
}

#[test]
fn machine_start_auto_initializes_named_machine_before_start_failure() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );

    let error = run_machine_command_for_test(
        MachineCommand {
            command: MachineSubcommand::Start(MachineStartCommand {
                image: Some("docker://127.0.0.1:1/example/nimbus-machine-os:test".to_owned()),
                name: Some("team-a".to_owned()),
                ..MachineStartCommand::default()
            }),
        },
        &layout,
    )
    .expect_err("machine start should surface OCI pull failure after named auto-init");

    assert!(
        error
            .to_string()
            .contains("failed to resolve machine guest OCI reference")
    );

    assert!(layout.paths("team-a").config_path.is_file());
    assert!(layout.paths("team-a").state_path.is_file());
    assert!(!layout.paths(DEFAULT_MACHINE_NAME).config_path.exists());
}

#[test]
fn machine_init_now_attempts_start_after_initialization() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
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
                image: "docker://127.0.0.1:1/example/nimbus-machine-os:test".to_owned(),
                ssh_identity: None,
                ignition_file: None,
                efi_store: None,
                volumes: Vec::new(),
                now: true,
                name: None,
            }),
        },
        &layout,
    )
    .expect_err("machine init --now should attempt start");

    assert!(
        error
            .to_string()
            .contains("failed to resolve machine guest OCI reference")
    );

    let paths = layout.paths(DEFAULT_MACHINE_NAME);
    assert!(paths.config_path.is_file());
    assert!(paths.state_path.is_file());
}

#[test]
fn machine_start_rejects_create_if_missing_overrides_when_machine_exists() {
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
                image: default_machine_image().to_owned(),
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

    let error = run_machine_command_for_test(
        MachineCommand {
            command: MachineSubcommand::Start(MachineStartCommand {
                memory_mib: Some(4096),
                ..MachineStartCommand::default()
            }),
        },
        &layout,
    )
    .expect_err("machine start should reject create-only overrides on existing machines");

    assert!(
        error.to_string().contains(
            "use `nimbus machine set` to change CPU, memory, or disk for an existing machine"
        ),
        "unexpected error: {error}"
    );
    assert!(
        error.to_string().contains("Hint:"),
        "unexpected error: {error}"
    );
}
