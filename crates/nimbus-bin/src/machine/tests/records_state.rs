use super::*;

#[test]
fn hidden_machine_api_subcommand_falls_back_without_home() {
    let original_home = std::env::var_os("HOME");
    // SAFETY: this test runs in the serialized machine lane and restores HOME before returning.
    unsafe { std::env::remove_var("HOME") };

    let roots = resolve_roots_for_command(&MachineCommand {
        command: MachineSubcommand::Api(MachineApiCommand {
            socket_path: Some(PathBuf::from("/tmp/nimbus.sock")),
            socket_activation: false,
            control_data_dir: Some(PathBuf::from("/tmp/nimbus-control")),
        }),
    })
    .expect("hidden machine api should fall back without HOME");

    if let Some(home) = original_home {
        // SAFETY: see comment above; restore process-local HOME for later tests.
        unsafe { std::env::set_var("HOME", home) };
    }

    assert_eq!(
        roots.config_root,
        PathBuf::from("/var/lib/nimbus/machine/config")
    );
    assert_eq!(
        roots.state_root,
        PathBuf::from("/var/lib/nimbus/machine/state")
    );
    assert_eq!(
        roots.data_root,
        PathBuf::from("/var/lib/nimbus/machine/data")
    );
    assert_eq!(
        roots.cache_root,
        PathBuf::from("/var/lib/nimbus/machine/cache")
    );
}

#[test]
fn machine_paths_use_short_runtime_root_and_typed_socket_layout() {
    let layout = MachineRootLayout::new(
        PathBuf::from("/tmp/config-root"),
        PathBuf::from("/tmp/state-root"),
        PathBuf::from("/tmp/nimbus"),
    );
    let paths = layout.paths("default");

    assert_eq!(paths.runtime_dir, PathBuf::from("/tmp/nimbus"));
    assert_eq!(
        paths.materialized_image_path,
        PathBuf::from("/tmp/data/default/images/default.raw")
    );
    assert_eq!(paths.image_cache_dir, PathBuf::from("/tmp/cache/images"));
    assert_eq!(
        paths.guest_binary_cache_dir,
        PathBuf::from("/tmp/cache/guest-nimbus")
    );
    assert_eq!(
        paths.api_socket_path,
        PathBuf::from("/tmp/nimbus/default-api.sock")
    );
    assert_eq!(
        paths.krunkit_log_path,
        PathBuf::from("/tmp/nimbus/default-krunkit.log")
    );
    assert_eq!(
        layout.lock_path("default"),
        PathBuf::from("/tmp/state-root/default.lock")
    );
}

#[test]
fn machine_volume_requires_absolute_host_and_guest_paths() {
    let error = MachineVolume::parse("Users:/Users").expect_err("relative source should fail");
    assert!(error.contains("source path must be absolute"));

    let error = MachineVolume::parse("/Users:Users").expect_err("relative target should fail");
    assert!(error.contains("target path must be absolute"));
}

#[test]
fn machine_init_writes_config_and_status_files() {
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
                bootc_native: false,
                efi_store: None,
                volumes: vec![MachineVolume {
                    source: PathBuf::from("/Users"),
                    target: PathBuf::from("/Users"),
                }],
                now: false,
                name: None,
            }),
        },
        &layout,
    )
    .expect("machine init should succeed");

    let paths = layout.paths(DEFAULT_MACHINE_NAME);
    let config = read_json_file_if_exists::<MachineConfigRecord>(&paths.config_path)
        .expect("config should read")
        .expect("config should exist");
    let state = read_json_file_if_exists::<MachineStateRecord>(&paths.state_path)
        .expect("state should read")
        .expect("state should exist");

    assert_eq!(config.version, CURRENT_MACHINE_CONFIG_VERSION);
    assert_eq!(config.provider, MachineProvider::Krunkit);
    assert_eq!(config.resources.cpus, DEFAULT_MACHINE_CPUS);
    assert_eq!(
        config.guest.image_source,
        MachineImageSource::OciReference {
            reference: default_machine_image().to_owned(),
        }
    );
    assert_eq!(
        config.guest.provisioning,
        MachineGuestProvisioning::Ignition
    );
    assert_eq!(config.guest.ssh_user, DEFAULT_MACHINE_SSH_USER);
    assert_eq!(config.guest.ssh_identity_path, None);
    assert_eq!(config.guest.ignition_file_path, None);
    assert_eq!(config.guest.efi_variable_store_path, None);
    assert_eq!(config.roots.data_root, temp_dir.path().join("data"));
    assert_eq!(config.roots.cache_root, temp_dir.path().join("cache"));
    assert_eq!(state.version, CURRENT_MACHINE_STATE_VERSION);
    assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
    assert!(paths.data_dir.exists());
    assert!(paths.image_cache_dir.exists());
    assert!(paths.guest_binary_cache_dir.exists());
    assert!(paths.runtime_dir.exists());
}

#[test]
fn machine_init_writes_named_machine_records() {
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
                bootc_native: false,
                efi_store: None,
                volumes: Vec::new(),
                now: false,
                name: Some("team-a".to_owned()),
            }),
        },
        &layout,
    )
    .expect("named machine init should succeed");

    let named_paths = layout.paths("team-a");
    let default_paths = layout.paths(DEFAULT_MACHINE_NAME);
    let config = read_json_file_if_exists::<MachineConfigRecord>(&named_paths.config_path)
        .expect("named config should read")
        .expect("named config should exist");

    assert_eq!(config.name, "team-a");
    assert!(named_paths.config_path.is_file());
    assert!(named_paths.state_path.is_file());
    assert!(!default_paths.config_path.exists());
}

#[test]
fn write_json_file_atomically_replaces_existing_state_record() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let path = temp_dir.path().join("status.json");
    let first = MachineStateRecord::initialized();
    let second = MachineStateRecord::rebuilt("rewritten for atomic replace test");

    write_json_file(&path, &first).expect("first state write should succeed");
    write_json_file(&path, &second).expect("second state write should succeed");

    let stored = read_json_file_if_exists::<MachineStateRecord>(&path)
        .expect("stored state should read")
        .expect("stored state should exist");

    assert_eq!(stored, second);
    assert_eq!(stored.version, CURRENT_MACHINE_STATE_VERSION);
    assert_eq!(stored.manager, MachineManagerState::Stale);
}

#[test]
fn machine_remove_releases_reserved_machine_port() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths(DEFAULT_MACHINE_NAME);
    run_machine_command_for_test(
        MachineCommand {
            command: MachineSubcommand::Init(MachineInitCommand {
                cpus: DEFAULT_MACHINE_CPUS,
                memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
                disk_gib: DEFAULT_MACHINE_DISK_GIB,
                image: default_machine_image().to_owned(),
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
    fs::write(
        layout.port_allocation_state_path(),
        serde_json::to_vec_pretty(&serde_json::json!({
            "machine_ports": {
                DEFAULT_MACHINE_NAME: 20022
            }
        }))
        .expect("port allocation state should serialize"),
    )
    .expect("port allocation state should write");

    run_machine_command_for_test(
        MachineCommand {
            command: MachineSubcommand::Rm(MachineRmCommand { name: None }),
        },
        &layout,
    )
    .expect("machine rm should succeed");

    let allocation_state = fs::read(layout.port_allocation_state_path())
        .expect("port allocation state should still read after release");
    let json: serde_json::Value =
        serde_json::from_slice(&allocation_state).expect("port allocation state should parse");
    assert_eq!(
        json["machine_ports"]
            .as_object()
            .expect("machine ports should be an object")
            .len(),
        0
    );
    assert!(!paths.config_dir.exists());
    assert!(!paths.state_dir.exists());
}

#[test]
fn machine_remove_only_deletes_requested_machine() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );

    for machine_name in [DEFAULT_MACHINE_NAME, "team-a"] {
        run_machine_command_for_test(
            MachineCommand {
                command: MachineSubcommand::Init(MachineInitCommand {
                    cpus: DEFAULT_MACHINE_CPUS,
                    memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
                    disk_gib: DEFAULT_MACHINE_DISK_GIB,
                    image: default_machine_image().to_owned(),
                    ssh_identity: None,
                    ignition_file: None,
                    bootc_native: false,
                    efi_store: None,
                    volumes: Vec::new(),
                    now: false,
                    name: Some(machine_name.to_owned()),
                }),
            },
            &layout,
        )
        .expect("machine init should succeed");
    }

    run_machine_command_for_test(
        MachineCommand {
            command: MachineSubcommand::Rm(MachineRmCommand {
                name: Some("team-a".to_owned()),
            }),
        },
        &layout,
    )
    .expect("named machine rm should succeed");

    assert!(layout.paths(DEFAULT_MACHINE_NAME).config_path.exists());
    assert!(layout.paths(DEFAULT_MACHINE_NAME).state_path.exists());
    assert!(!layout.paths("team-a").config_path.exists());
    assert!(!layout.paths("team-a").state_path.exists());
}

#[test]
fn machine_set_updates_stopped_machine_config() {
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
                bootc_native: false,
                efi_store: None,
                volumes: Vec::new(),
                now: false,
                name: Some("team-a".to_owned()),
            }),
        },
        &layout,
    )
    .expect("machine init should succeed");

    run_machine_command_for_test(
        MachineCommand {
            command: MachineSubcommand::Set(MachineSetCommand {
                cpus: Some(4),
                memory_mib: Some(4096),
                disk_gib: Some(40),
                name: Some("team-a".to_owned()),
            }),
        },
        &layout,
    )
    .expect("machine set should succeed");

    let config =
        read_json_file_if_exists::<MachineConfigRecord>(&layout.paths("team-a").config_path)
            .expect("config should read")
            .expect("config should exist");
    assert_eq!(config.resources.cpus, 4);
    assert_eq!(config.resources.memory_mib, 4096);
    assert_eq!(config.resources.disk_gib, 40);
}

#[test]
fn machine_set_requires_at_least_one_resource_flag() {
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

    let error = run_machine_command_for_test(
        MachineCommand {
            command: MachineSubcommand::Set(MachineSetCommand::default()),
        },
        &layout,
    )
    .expect_err("machine set without flags should fail");

    assert!(
        error
            .to_string()
            .contains("requires at least one of `--cpus`, `--memory`, or `--disk-size`")
    );
}

#[test]
fn machine_set_rejects_running_machine() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("team-a");
    paths
        .ensure_directories()
        .expect("machine directories should exist");
    write_json_file(
        &paths.config_path,
        &MachineConfigRecord {
            version: CURRENT_MACHINE_CONFIG_VERSION,
            name: "team-a".to_owned(),
            provider: MachineProvider::Krunkit,
            guest: MachineGuestConfig {
                image_source: MachineImageSource::parse(&default_machine_image())
                    .expect("default image should parse"),
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
            roots: layout.clone(),
        },
    )
    .expect("config should write");
    write_json_file(
        &paths.state_path,
        &MachineStateRecord {
            version: CURRENT_MACHINE_STATE_VERSION,
            lifecycle: MachineLifecycle::Running,
            manager: MachineManagerState::Ready,
            runtime: None,
            last_error: None,
        },
    )
    .expect("state should write");

    let error = run_machine_command_for_test(
        MachineCommand {
            command: MachineSubcommand::Set(MachineSetCommand {
                cpus: Some(4),
                memory_mib: None,
                disk_gib: None,
                name: Some("team-a".to_owned()),
            }),
        },
        &layout,
    )
    .expect_err("machine set should reject running machine");

    let rendered = error.to_string();
    assert!(rendered.contains("machine 'team-a'"));
    assert!(rendered.contains("must be stopped"));
    assert!(rendered.contains("Hint:"));
    assert!(rendered.contains("nimbus machine stop team-a"));
}

#[test]
fn load_machine_config_rejects_older_schema_versions_with_clear_error() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths(DEFAULT_MACHINE_NAME);
    fs::create_dir_all(&paths.config_dir).expect("config dir should exist");
    let older_config = serde_json::json!({
        "version": CURRENT_MACHINE_CONFIG_VERSION - 1,
        "name": DEFAULT_MACHINE_NAME,
        "provider": "krunkit",
        "guest": {
            "image_source": {
                "kind": "oci-reference",
                "reference": default_machine_image(),
            },
            "ssh_user": DEFAULT_MACHINE_SSH_USER,
            "ssh_identity_path": null,
            "ignition_file_path": null,
            "efi_variable_store_path": null,
        },
        "resources": {
            "cpus": DEFAULT_MACHINE_CPUS,
            "memory_mib": DEFAULT_MACHINE_MEMORY_MIB,
            "disk_gib": DEFAULT_MACHINE_DISK_GIB,
        },
        "volumes": [],
        "roots": {
            "config_root": layout.config_root,
            "state_root": layout.state_root,
            "data_root": layout.data_root,
            "cache_root": layout.cache_root,
            "runtime_root": layout.runtime_root,
        },
    });
    fs::write(
        &paths.config_path,
        serde_json::to_vec_pretty(&older_config).expect("older config should serialize"),
    )
    .expect("older config should write");

    let error = load_machine_config_if_exists(&paths.config_path)
        .expect_err("older config version should fail");

    assert!(
        error
            .to_string()
            .contains("uses unsupported schema version")
    );
}

#[test]
fn load_machine_config_rejects_newer_schema_version_with_clear_error() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths(DEFAULT_MACHINE_NAME);
    fs::create_dir_all(&paths.config_dir).expect("config dir should exist");
    let newer_config = serde_json::json!({
        "version": CURRENT_MACHINE_CONFIG_VERSION + 1,
        "name": DEFAULT_MACHINE_NAME,
        "provider": "krunkit",
        "guest": {
            "image_source": {
                "kind": "oci-reference",
                "reference": default_machine_image(),
            },
            "ssh_user": DEFAULT_MACHINE_SSH_USER,
            "ssh_identity_path": null,
            "ignition_file_path": null,
            "efi_variable_store_path": null,
        },
        "resources": {
            "cpus": DEFAULT_MACHINE_CPUS,
            "memory_mib": DEFAULT_MACHINE_MEMORY_MIB,
            "disk_gib": DEFAULT_MACHINE_DISK_GIB,
        },
        "volumes": [],
        "roots": {
            "config_root": layout.config_root,
            "state_root": layout.state_root,
            "data_root": layout.data_root,
            "cache_root": layout.cache_root,
            "runtime_root": layout.runtime_root,
        },
    });
    fs::write(
        &paths.config_path,
        serde_json::to_vec_pretty(&newer_config).expect("newer config should serialize"),
    )
    .expect("newer config should write");

    let error = load_machine_config_if_exists(&paths.config_path)
        .expect_err("newer config version should fail");

    assert!(error.to_string().contains("uses newer schema version"));
    assert!(
        error
            .to_string()
            .contains(&(CURRENT_MACHINE_CONFIG_VERSION + 1).to_string())
    );
}

#[test]
fn load_machine_state_rebuilds_older_schema_versions_with_explicit_error() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths(DEFAULT_MACHINE_NAME);
    fs::create_dir_all(&paths.state_dir).expect("state dir should exist");
    let older_state = serde_json::json!({
        "version": CURRENT_MACHINE_STATE_VERSION - 1,
        "lifecycle": "running",
        "manager": "ready",
        "runtime": null,
        "last_error": null,
    });
    fs::write(
        &paths.state_path,
        serde_json::to_vec_pretty(&older_state).expect("older state should serialize"),
    )
    .expect("older state should write");

    let state = load_machine_state_if_exists(&paths.state_path)
        .expect("state load should succeed by rebuilding")
        .expect("rebuilt state should exist");
    let rewritten = read_json_file_if_exists::<MachineStateRecord>(&paths.state_path)
        .expect("rewritten state should read")
        .expect("rewritten state should exist");

    assert_eq!(state.version, CURRENT_MACHINE_STATE_VERSION);
    assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
    assert_eq!(state.manager, MachineManagerState::Stale);
    assert!(
        state
            .last_error
            .as_deref()
            .is_some_and(|message| message.contains("unsupported schema version"))
    );
    assert_eq!(rewritten.version, CURRENT_MACHINE_STATE_VERSION);
    assert_eq!(rewritten, state);
}

#[test]
fn load_machine_state_rebuilds_unreadable_record_with_explicit_error() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths(DEFAULT_MACHINE_NAME);
    fs::create_dir_all(&paths.state_dir).expect("state dir should exist");
    fs::write(&paths.state_path, b"{not-json").expect("corrupt state should write");

    let state = load_machine_state_if_exists(&paths.state_path)
        .expect("state load should succeed by rebuilding")
        .expect("rebuilt state should exist");
    let rewritten = read_json_file_if_exists::<MachineStateRecord>(&paths.state_path)
        .expect("rebuilt state should read")
        .expect("rebuilt state should exist");

    assert_eq!(state.version, CURRENT_MACHINE_STATE_VERSION);
    assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
    assert_eq!(state.manager, MachineManagerState::Stale);
    assert!(state.runtime.is_none());
    assert!(
        state
            .last_error
            .as_deref()
            .is_some_and(|message| message.contains("machine state"))
    );
    assert_eq!(rewritten, state);
}

#[test]
fn machine_remove_deletes_config_state_and_runtime_roots() {
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
    fs::create_dir_all(&paths.runtime_dir).expect("runtime dir should exist");
    fs::write(paths.krunkit_gvproxy_socket_path(), [])
        .expect("derived krunkit socket should write");
    run_machine_command_for_test(
        MachineCommand {
            command: MachineSubcommand::Rm(MachineRmCommand { name: None }),
        },
        &layout,
    )
    .expect("machine rm should succeed");

    assert!(!paths.config_dir.exists());
    assert!(!paths.state_dir.exists());
    assert!(!paths.data_dir.exists());
    assert!(!paths.runtime_dir.exists());
}
