use super::*;

#[test]
fn machine_status_renders_uninitialized_view_when_machine_is_absent() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths(DEFAULT_MACHINE_NAME);

    let rendered = render_machine_status_view(
        MachineCommandResult::Uninitialized,
        &paths,
        None,
        None,
        MachineStatusOutputFormat::Yaml,
        false,
        false,
    )
    .expect("uninitialized machine view should render");

    assert!(rendered.contains("result: uninitialized"));
    assert!(rendered.contains("initialized: false"));
    assert!(rendered.contains("lifecycle: uninitialized"));
    assert!(rendered.contains("reachable: false"));
}

#[test]
fn machine_action_view_renders_concise_start_summary() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("team-a");

    let rendered = render_machine_action_view(MachineCommandResult::Started, &paths)
        .expect("machine action summary should render");

    assert_eq!(rendered, "Machine \"team-a\" started successfully\n");
}

#[test]
fn machine_action_view_rejects_status_results() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("team-a");

    let error = render_machine_action_view(MachineCommandResult::Status, &paths)
        .expect_err("status result should not render as an action summary");

    assert!(
        error
            .to_string()
            .contains("machine action renderer cannot summarize")
    );
}

#[test]
fn machine_os_apply_view_renders_action_summary_and_hint() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths(DEFAULT_MACHINE_NAME);

    let rendered = render_machine_os_apply_view(
        MachineOsCommandResult::Applied,
        &paths,
        &MachineOsApplyOutcome {
            previous_image: "docker://example.com/old@sha256:old".to_owned(),
            current_image: "docker://example.com/new@sha256:new".to_owned(),
            changed: true,
            restarted: false,
            lifecycle: MachineLifecycle::Stopped,
        },
        false,
    )
    .expect("machine os apply summary should render");

    assert!(rendered.contains("Machine \"default\" machine OS applied successfully"));
    assert!(rendered.contains("Image: docker://example.com/new@sha256:new"));
    assert!(rendered.contains("Previous image: docker://example.com/old@sha256:old"));
    assert!(rendered.contains("Hint: run `nimbus machine start` to boot the updated image"));
}

#[test]
fn machine_os_upgrade_view_renders_dry_run_action_summary_and_hint() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths(DEFAULT_MACHINE_NAME);

    let rendered = render_machine_os_upgrade_view(
        MachineOsCommandResult::UpgradeCheck,
        &paths,
        &MachineOsUpgradePlan {
            current_image: "docker://example.com/current@sha256:aaa".to_owned(),
            current_version: "sha256:aaa".to_owned(),
            target_image: "docker://example.com/target@sha256:bbb".to_owned(),
            target_version: "sha256:bbb".to_owned(),
            update_available: true,
        },
        true,
        false,
        false,
    )
    .expect("machine os upgrade summary should render");

    assert!(rendered.contains("Machine \"default\" machine OS update available"));
    assert!(rendered.contains("Current image: docker://example.com/current@sha256:aaa"));
    assert!(rendered.contains("Target image: docker://example.com/target@sha256:bbb"));
    assert!(
        rendered.contains("Hint: run `nimbus machine os upgrade` to apply the supported image")
    );
}

#[test]
fn machine_status_table_output_is_default_human_summary() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("team-a");
    let config = MachineConfigRecord {
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
            cpus: 4,
            memory_mib: 4096,
            disk_gib: 40,
        },
        volumes: Vec::new(),
        roots: layout,
    };
    let state = MachineStateRecord {
        version: CURRENT_MACHINE_STATE_VERSION,
        lifecycle: MachineLifecycle::Running,
        manager: MachineManagerState::Ready,
        runtime: None,
        last_error: None,
    };

    let rendered = render_machine_status_view(
        MachineCommandResult::Status,
        &paths,
        Some(&config),
        Some(&state),
        MachineStatusOutputFormat::Table,
        false,
        false,
    )
    .expect("table output should render");

    assert!(rendered.contains("NAME"));
    assert!(rendered.contains("LIFECYCLE"));
    assert!(rendered.contains("MEMORY(MiB)"));
    assert!(rendered.contains("team-a"));
    assert!(rendered.contains("running"));
    assert!(rendered.contains("krunkit"));
    assert!(rendered.contains("4096"));
    assert!(rendered.contains("reachable") || rendered.contains("unreachable"));
    assert!(!rendered.contains("guest:"));
}

#[test]
fn machine_status_table_output_can_omit_headings() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("team-a");
    let config = MachineConfigRecord {
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
            cpus: 4,
            memory_mib: 4096,
            disk_gib: 40,
        },
        volumes: Vec::new(),
        roots: layout,
    };
    let state = MachineStateRecord {
        version: CURRENT_MACHINE_STATE_VERSION,
        lifecycle: MachineLifecycle::Running,
        manager: MachineManagerState::Ready,
        runtime: None,
        last_error: None,
    };

    let rendered = render_machine_status_view(
        MachineCommandResult::Status,
        &paths,
        Some(&config),
        Some(&state),
        MachineStatusOutputFormat::Table,
        true,
        false,
    )
    .expect("table output without headings should render");

    assert!(!rendered.contains("NAME"));
    assert!(!rendered.contains("LIFECYCLE"));
    assert!(rendered.contains("team-a"));
    assert!(rendered.contains("running"));
}

#[test]
fn machine_status_json_output_serializes_full_status_view() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("team-a");
    let config = MachineConfigRecord {
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
        roots: layout,
    };

    let rendered = render_machine_status_view(
        MachineCommandResult::Status,
        &paths,
        Some(&config),
        Some(&MachineStateRecord::initialized()),
        MachineStatusOutputFormat::Json,
        false,
        false,
    )
    .expect("json output should render");
    let json: serde_json::Value =
        serde_json::from_str(&rendered).expect("status JSON should parse");

    assert_eq!(json["name"], "team-a");
    assert_eq!(json["result"], "status");
    assert_eq!(json["provider"], "krunkit");
    assert_eq!(json["resources"]["cpus"], DEFAULT_MACHINE_CPUS);
    assert_eq!(json["initialized"], true);
}

#[test]
fn machine_status_yaml_output_serializes_full_status_view() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("team-a");

    let rendered = render_machine_status_view(
        MachineCommandResult::Uninitialized,
        &paths,
        None,
        None,
        MachineStatusOutputFormat::Yaml,
        false,
        false,
    )
    .expect("yaml output should render");

    assert!(rendered.contains("result: uninitialized"));
    assert!(rendered.contains("name: team-a"));
    assert!(rendered.contains("initialized: false"));
}

#[test]
fn machine_list_table_output_is_human_summary() {
    let machines = vec![
        MachineListEntryView {
            name: "default".to_owned(),
            is_default: true,
            lifecycle: MachineLifecycle::Stopped,
            provider: MachineProvider::Krunkit,
            cpus: 2,
            memory_mib: 2048,
            disk_gib: 20,
        },
        MachineListEntryView {
            name: "team-a".to_owned(),
            is_default: false,
            lifecycle: MachineLifecycle::Running,
            provider: MachineProvider::Krunkit,
            cpus: 4,
            memory_mib: 4096,
            disk_gib: 40,
        },
    ];

    let rendered = render_machine_list_view(
        &machines,
        &MachineListCommand {
            format: None,
            quiet: false,
            no_heading: false,
        },
    )
    .expect("table output should render");

    assert!(rendered.contains("NAME"));
    assert!(rendered.contains("LIFECYCLE"));
    assert!(rendered.contains("PROVIDER"));
    assert!(rendered.contains("MEMORY(MiB)"));
    assert!(rendered.contains("default*"));
    assert!(rendered.contains("team-a"));
    assert!(rendered.contains("running"));
    assert!(!rendered.contains("\"name\""));
}

#[test]
fn machine_list_json_output_serializes_machine_summaries() {
    let machines = vec![MachineListEntryView {
        name: "team-a".to_owned(),
        is_default: false,
        lifecycle: MachineLifecycle::Running,
        provider: MachineProvider::Krunkit,
        cpus: 4,
        memory_mib: 4096,
        disk_gib: 40,
    }];

    let rendered = render_machine_list_view(
        &machines,
        &MachineListCommand {
            format: Some(MachineListOutputFormat::Json),
            quiet: false,
            no_heading: false,
        },
    )
    .expect("json output should render");
    let json: serde_json::Value =
        serde_json::from_str(&rendered).expect("machine list JSON should parse");

    assert_eq!(json[0]["name"], "team-a");
    assert_eq!(json[0]["default"], false);
    assert_eq!(json[0]["lifecycle"], "running");
    assert_eq!(json[0]["provider"], "krunkit");
    assert_eq!(json[0]["cpus"], 4);
}

#[test]
fn machine_list_table_output_can_omit_headings() {
    let machines = vec![MachineListEntryView {
        name: "team-a".to_owned(),
        is_default: false,
        lifecycle: MachineLifecycle::Running,
        provider: MachineProvider::Krunkit,
        cpus: 4,
        memory_mib: 4096,
        disk_gib: 40,
    }];

    let rendered = render_machine_list_view(
        &machines,
        &MachineListCommand {
            format: None,
            quiet: false,
            no_heading: true,
        },
    )
    .expect("table output without headings should render");

    assert!(!rendered.contains("NAME"));
    assert!(!rendered.contains("LIFECYCLE"));
    assert!(rendered.contains("team-a"));
    assert!(rendered.contains("running"));
}

#[test]
fn machine_list_quiet_output_prints_names_only() {
    let machines = vec![
        MachineListEntryView {
            name: "default".to_owned(),
            is_default: true,
            lifecycle: MachineLifecycle::Stopped,
            provider: MachineProvider::Krunkit,
            cpus: 2,
            memory_mib: 2048,
            disk_gib: 20,
        },
        MachineListEntryView {
            name: "team-a".to_owned(),
            is_default: false,
            lifecycle: MachineLifecycle::Running,
            provider: MachineProvider::Krunkit,
            cpus: 4,
            memory_mib: 4096,
            disk_gib: 40,
        },
    ];

    let rendered = render_machine_list_view(
        &machines,
        &MachineListCommand {
            format: None,
            quiet: true,
            no_heading: false,
        },
    )
    .expect("quiet output should render");

    assert_eq!(rendered, "default\nteam-a\n");
}

#[test]
fn machine_list_explicit_format_wins_over_quiet() {
    let machines = vec![MachineListEntryView {
        name: "default".to_owned(),
        is_default: true,
        lifecycle: MachineLifecycle::Stopped,
        provider: MachineProvider::Krunkit,
        cpus: 2,
        memory_mib: 2048,
        disk_gib: 20,
    }];

    let rendered = render_machine_list_view(
        &machines,
        &MachineListCommand {
            format: Some(MachineListOutputFormat::Json),
            quiet: true,
            no_heading: false,
        },
    )
    .expect("explicit format should win over quiet");

    assert!(rendered.contains("\"name\": \"default\""));
    assert!(rendered.contains("\"default\": true"));
    assert!(rendered.contains("\"lifecycle\": \"stopped\""));
}

#[test]
fn machine_info_renders_yaml_by_default() {
    let view = MachineInfoView {
        version: "0.1.20".to_owned(),
        host: MachineHostInfoView {
            arch: "aarch64".to_owned(),
            os: "macos".to_owned(),
            current_release: "v0.1.20".to_owned(),
            default_machine_name: DEFAULT_MACHINE_NAME.to_owned(),
            machine_count: 1,
            running_machine_count: 1,
            image_cache_dir: PathBuf::from("/tmp/cache/images"),
            guest_binary_cache_dir: PathBuf::from("/tmp/cache/guest-nimbus"),
            roots: MachineRootsView {
                config_root: PathBuf::from("/tmp/config"),
                state_root: PathBuf::from("/tmp/state"),
                data_root: PathBuf::from("/tmp/data"),
                cache_root: PathBuf::from("/tmp/cache"),
                runtime_root: PathBuf::from("/tmp/runtime"),
            },
            default_machine: MachineInfoDefaultMachineView {
                initialized: true,
                lifecycle: MachineLifecycle::Running,
                manager: MachineManagerState::Ready,
                provider: Some(MachineProvider::Krunkit),
                api_reachable: true,
            },
        },
    };

    let rendered = render_machine_info_view(&view, MachineInfoOutputFormat::Yaml)
        .expect("machine info should render");
    assert!(rendered.contains("version: 0.1.20"));
    assert!(rendered.contains("current_release: v0.1.20"));
    assert!(rendered.contains("default_machine_name: default"));
    assert!(rendered.contains("api_reachable: true"));
}

#[test]
fn machine_info_renders_json_when_requested() {
    let view = MachineInfoView {
        version: "0.1.20".to_owned(),
        host: MachineHostInfoView {
            arch: "aarch64".to_owned(),
            os: "macos".to_owned(),
            current_release: "v0.1.20".to_owned(),
            default_machine_name: DEFAULT_MACHINE_NAME.to_owned(),
            machine_count: 0,
            running_machine_count: 0,
            image_cache_dir: PathBuf::from("/tmp/cache/images"),
            guest_binary_cache_dir: PathBuf::from("/tmp/cache/guest-nimbus"),
            roots: MachineRootsView {
                config_root: PathBuf::from("/tmp/config"),
                state_root: PathBuf::from("/tmp/state"),
                data_root: PathBuf::from("/tmp/data"),
                cache_root: PathBuf::from("/tmp/cache"),
                runtime_root: PathBuf::from("/tmp/runtime"),
            },
            default_machine: MachineInfoDefaultMachineView {
                initialized: false,
                lifecycle: MachineLifecycle::Uninitialized,
                manager: MachineManagerState::Unconfigured,
                provider: None,
                api_reachable: false,
            },
        },
    };

    let rendered = render_machine_info_view(&view, MachineInfoOutputFormat::Json)
        .expect("machine info should render");
    assert!(rendered.contains("\"version\": \"0.1.20\""));
    assert!(rendered.contains("\"current_release\": \"v0.1.20\""));
    assert!(rendered.contains("\"initialized\": false"));
}

#[test]
fn machine_list_prioritizes_active_and_default_machines() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );

    for (name, cpus, memory_mib, disk_gib, lifecycle) in [
        ("team-b", 6, 6144, 60, MachineLifecycle::Failed),
        ("default", 2, 2048, 20, MachineLifecycle::Stopped),
        ("team-a", 4, 4096, 40, MachineLifecycle::Running),
    ] {
        let paths = layout.paths(name);
        paths.ensure_directories().expect("paths should exist");
        write_json_file(
            &paths.config_path,
            &MachineConfigRecord {
                version: CURRENT_MACHINE_CONFIG_VERSION,
                name: name.to_owned(),
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
                    cpus,
                    memory_mib,
                    disk_gib,
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
                lifecycle,
                manager: MachineManagerState::Unconfigured,
                runtime: None,
                last_error: None,
            },
        )
        .expect("state should write");
        if matches!(
            lifecycle,
            MachineLifecycle::Starting | MachineLifecycle::Running
        ) {
            let current_pid = std::process::id().to_string();
            fs::write(&paths.krunkit_pid_path, &current_pid).expect("krunkit pidfile should write");
            fs::write(&paths.gvproxy_pid_path, &current_pid).expect("gvproxy pidfile should write");
        }
    }

    let machines = build_machine_list_entries(&layout).expect("machine list should build");

    assert_eq!(
        machines
            .iter()
            .map(|machine| machine.name.as_str())
            .collect::<Vec<_>>(),
        vec!["team-a", "default", "team-b"]
    );
    assert!(!machines[0].is_default);
    assert_eq!(machines[0].lifecycle, MachineLifecycle::Running);
    assert!(machines[1].is_default);
    assert_eq!(machines[1].cpus, 2);
    assert_eq!(machines[2].disk_gib, 60);
}

#[test]
fn machine_status_quiet_output_prints_name_only() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("team-a");

    let rendered = render_machine_status_view(
        MachineCommandResult::Uninitialized,
        &paths,
        None,
        None,
        MachineStatusOutputFormat::Json,
        false,
        true,
    )
    .expect("quiet output should render");

    assert_eq!(rendered, "team-a\n");
}

#[test]
fn machine_inspect_json_output_serializes_full_config_and_state() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let config = MachineConfigRecord {
        version: CURRENT_MACHINE_CONFIG_VERSION,
        name: "team-a".to_owned(),
        provider: MachineProvider::Krunkit,
        guest: MachineGuestConfig {
            image_source: MachineImageSource::parse(&default_machine_image())
                .expect("default image should parse"),
            provisioning: MachineGuestProvisioning::Ignition,
            ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
            ssh_identity_path: Some(PathBuf::from("/tmp/team-a")),
            ignition_file_path: None,
            efi_variable_store_path: None,
        },
        resources: MachineResources {
            cpus: 4,
            memory_mib: 4096,
            disk_gib: 40,
        },
        volumes: vec![MachineVolume {
            source: PathBuf::from("/Users"),
            target: PathBuf::from("/Users"),
        }],
        roots: layout,
    };
    let state = MachineStateRecord {
        version: CURRENT_MACHINE_STATE_VERSION,
        lifecycle: MachineLifecycle::Stopped,
        manager: MachineManagerState::Unconfigured,
        runtime: None,
        last_error: Some("none".to_owned()),
    };

    let rendered = render_machine_inspect_view(&config, &state, MachineInspectOutputFormat::Json)
        .expect("inspect json should render");
    let json: serde_json::Value =
        serde_json::from_str(&rendered).expect("inspect JSON should parse");

    assert_eq!(json["config"]["name"], "team-a");
    assert_eq!(json["config"]["provider"], "krunkit");
    assert_eq!(json["config"]["resources"]["cpus"], 4);
    assert_eq!(json["state"]["lifecycle"], "stopped");
    assert_eq!(json["state"]["manager"], "unconfigured");
}

#[test]
fn machine_inspect_yaml_output_serializes_full_config_and_state() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let config = MachineConfigRecord {
        version: CURRENT_MACHINE_CONFIG_VERSION,
        name: "default".to_owned(),
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
        roots: layout,
    };

    let rendered = render_machine_inspect_view(
        &config,
        &MachineStateRecord::initialized(),
        MachineInspectOutputFormat::Yaml,
    )
    .expect("inspect yaml should render");

    assert!(rendered.contains("config:"));
    assert!(rendered.contains("state:"));
    assert!(rendered.contains("name: default"));
    assert!(rendered.contains("provider: krunkit"));
    assert!(rendered.contains("lifecycle: stopped"));
}
