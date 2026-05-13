use super::*;

#[test]
fn parses_machine_init_defaults_to_version_pinned_release_image() {
    let cli = RootCli::parse_from(["nimbus", "machine", "init"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine init should parse");
    };

    match machine.command {
        MachineSubcommand::Init(init) => {
            assert_eq!(init.image, expected_default_machine_image());
            if cfg!(target_os = "macos") {
                assert!(init.volumes.is_empty());
                assert_eq!(
                    default_machine_volumes(),
                    vec![
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
                    ]
                );
            }
        }
        _ => panic!("expected init subcommand"),
    }
}

#[test]
fn parses_machine_init_with_resource_overrides() {
    let cli = RootCli::parse_from([
        "nimbus",
        "machine",
        "init",
        "--cpus",
        "4",
        "--memory",
        "4096",
        "--disk-size",
        "40",
        "--image",
        "docker://ghcr.io/nimbus/nimbus-machine-os:test",
        "--identity",
        "/tmp/nimbus-test-ed25519",
        "--ignition-path",
        "/tmp/nimbus-test.ign",
        "--firmware",
        "/tmp/nimbus-test.efi",
        "--volume",
        "/Users:/Users",
    ]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine subcommand should parse");
    };

    match machine.command {
        MachineSubcommand::Init(init) => {
            assert_eq!(init.cpus, 4);
            assert_eq!(init.memory_mib, 4096);
            assert_eq!(init.disk_gib, 40);
            assert_eq!(init.image, "docker://ghcr.io/nimbus/nimbus-machine-os:test");
            assert_eq!(
                init.ssh_identity,
                Some(PathBuf::from("/tmp/nimbus-test-ed25519"))
            );
            assert_eq!(
                init.ignition_file,
                Some(PathBuf::from("/tmp/nimbus-test.ign"))
            );
            assert_eq!(init.efi_store, Some(PathBuf::from("/tmp/nimbus-test.efi")));
            assert_eq!(
                init.volumes,
                vec![MachineVolume {
                    source: PathBuf::from("/Users"),
                    target: PathBuf::from("/Users"),
                }]
            );
        }
        _ => panic!("expected init subcommand"),
    }
}

#[test]
fn machine_init_accepts_short_flag_aliases() {
    let cli = RootCli::parse_from([
        "nimbus",
        "machine",
        "init",
        "-c",
        "4",
        "-m",
        "4096",
        "-d",
        "40",
        "-v",
        "/Users:/Users",
    ]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine subcommand should parse");
    };

    match machine.command {
        MachineSubcommand::Init(init) => {
            assert_eq!(init.cpus, 4);
            assert_eq!(init.memory_mib, 4096);
            assert_eq!(init.disk_gib, 40);
            assert_eq!(
                init.volumes,
                vec![MachineVolume {
                    source: PathBuf::from("/Users"),
                    target: PathBuf::from("/Users"),
                }]
            );
        }
        _ => panic!("expected init subcommand"),
    }
}

#[test]
fn machine_init_rejects_removed_flag_names() {
    for removed_flag in [
        "--ssh-identity",
        "--ignition-file",
        "--efi-store",
        "--memory-mib",
        "--disk-gib",
    ] {
        let error = RootCli::try_parse_from(["nimbus", "machine", "init", removed_flag, "value"])
            .expect_err("removed flag should be rejected");
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
        let rendered = error.to_string();
        assert!(rendered.contains(removed_flag));
        assert!(rendered.contains("unexpected argument"));
    }
}

#[test]
fn machine_init_parses_now_flag() {
    let cli = RootCli::parse_from(["nimbus", "machine", "init", "--now"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine init should parse");
    };

    match machine.command {
        MachineSubcommand::Init(init) => assert!(init.now),
        _ => panic!("expected init subcommand"),
    }
}

#[test]
fn machine_start_parses_create_if_missing_overrides() {
    let cli = RootCli::parse_from([
        "nimbus",
        "machine",
        "start",
        "-c",
        "4",
        "--memory",
        "4096",
        "--disk-size",
        "40",
        "--image",
        "docker://ghcr.io/nimbus/nimbus-machine-os:test",
        "--identity",
        "/tmp/nimbus-test-ed25519",
        "--ignition-path",
        "/tmp/nimbus-test.ign",
        "--firmware",
        "/tmp/nimbus-test.efi",
        "-v",
        "/Users:/Users",
    ]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine start should parse");
    };

    match machine.command {
        MachineSubcommand::Start(start) => {
            assert_eq!(start.cpus, Some(4));
            assert_eq!(start.memory_mib, Some(4096));
            assert_eq!(start.disk_gib, Some(40));
            assert_eq!(
                start.image,
                Some("docker://ghcr.io/nimbus/nimbus-machine-os:test".to_owned())
            );
            assert_eq!(
                start.ssh_identity,
                Some(PathBuf::from("/tmp/nimbus-test-ed25519"))
            );
            assert_eq!(
                start.ignition_file,
                Some(PathBuf::from("/tmp/nimbus-test.ign"))
            );
            assert_eq!(start.efi_store, Some(PathBuf::from("/tmp/nimbus-test.efi")));
            assert_eq!(
                start.volumes,
                vec![MachineVolume {
                    source: PathBuf::from("/Users"),
                    target: PathBuf::from("/Users"),
                }]
            );
            assert!(!start.quiet);
            assert!(!start.no_info);
        }
        _ => panic!("expected start subcommand"),
    }
}

#[test]
fn machine_start_accepts_quiet_and_no_info_flags() {
    let cli = RootCli::parse_from([
        "nimbus",
        "machine",
        "start",
        "--quiet",
        "--no-info",
        "team-a",
    ]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine start should parse");
    };

    match machine.command {
        MachineSubcommand::Start(start) => {
            assert!(start.quiet);
            assert!(start.no_info);
            assert_eq!(start.name.as_deref(), Some("team-a"));
        }
        _ => panic!("expected start subcommand"),
    }
}

#[test]
fn machine_lifecycle_subcommands_accept_optional_name_positionals() {
    let cli = RootCli::parse_from(["nimbus", "machine", "init", "team-a"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine init should parse");
    };
    match machine.command {
        MachineSubcommand::Init(init) => assert_eq!(init.name.as_deref(), Some("team-a")),
        _ => panic!("expected init subcommand"),
    }

    let cli = RootCli::parse_from(["nimbus", "machine", "start", "team-a"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine start should parse");
    };
    match machine.command {
        MachineSubcommand::Start(start) => assert_eq!(start.name.as_deref(), Some("team-a")),
        _ => panic!("expected start subcommand"),
    }

    let cli = RootCli::parse_from(["nimbus", "machine", "stop", "team-a"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine stop should parse");
    };
    match machine.command {
        MachineSubcommand::Stop(stop) => assert_eq!(stop.name.as_deref(), Some("team-a")),
        _ => panic!("expected stop subcommand"),
    }

    let cli = RootCli::parse_from(["nimbus", "machine", "status", "team-a"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine status should parse");
    };
    match machine.command {
        MachineSubcommand::Status(status) => {
            assert_eq!(status.name.as_deref(), Some("team-a"))
        }
        _ => panic!("expected status subcommand"),
    }

    let cli = RootCli::parse_from(["nimbus", "machine", "inspect", "team-a"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine inspect should parse");
    };
    match machine.command {
        MachineSubcommand::Inspect(inspect) => {
            assert_eq!(inspect.name.as_deref(), Some("team-a"))
        }
        _ => panic!("expected inspect subcommand"),
    }

    let cli = RootCli::parse_from(["nimbus", "machine", "set", "--cpus", "4", "team-a"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine set should parse");
    };
    match machine.command {
        MachineSubcommand::Set(set) => {
            assert_eq!(set.cpus, Some(4));
            assert_eq!(set.name.as_deref(), Some("team-a"));
        }
        _ => panic!("expected set subcommand"),
    }

    let cli = RootCli::parse_from(["nimbus", "machine", "rm", "team-a"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine rm should parse");
    };
    match machine.command {
        MachineSubcommand::Rm(remove) => assert_eq!(remove.name.as_deref(), Some("team-a")),
        _ => panic!("expected rm subcommand"),
    }
}

#[test]
fn machine_status_defaults_to_table_output_format() {
    let cli = RootCli::parse_from(["nimbus", "machine", "status"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine status should parse");
    };

    match machine.command {
        MachineSubcommand::Status(status) => {
            assert!(!status.quiet);
            assert!(!status.no_heading);
            assert_eq!(status.format, MachineStatusOutputFormat::Table);
            assert_eq!(status.name.as_deref(), None);
        }
        _ => panic!("expected status subcommand"),
    }
}

#[test]
fn machine_status_accepts_output_shaping_flags() {
    for (format_value, expected) in [
        ("json", MachineStatusOutputFormat::Json),
        ("yaml", MachineStatusOutputFormat::Yaml),
        ("table", MachineStatusOutputFormat::Table),
    ] {
        let cli = RootCli::parse_from(["nimbus", "machine", "status", "-f", format_value]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine status should parse");
        };

        match machine.command {
            MachineSubcommand::Status(status) => assert_eq!(status.format, expected),
            _ => panic!("expected status subcommand"),
        }
    }

    let cli = RootCli::parse_from(["nimbus", "machine", "status", "--noheading"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine status with noheading should parse");
    };
    match machine.command {
        MachineSubcommand::Status(status) => assert!(status.no_heading),
        _ => panic!("expected status subcommand"),
    }
}

#[test]
fn machine_status_accepts_quiet_mode() {
    let cli = RootCli::parse_from(["nimbus", "machine", "status", "--quiet", "team-a"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine status should parse");
    };

    match machine.command {
        MachineSubcommand::Status(status) => {
            assert!(status.quiet);
            assert_eq!(status.name.as_deref(), Some("team-a"));
        }
        _ => panic!("expected status subcommand"),
    }
}

#[test]
fn parses_machine_lifecycle_subcommands() {
    for command in ["start", "stop", "status", "list", "info", "inspect", "rm"] {
        let cli = RootCli::parse_from(["nimbus", "machine", command]);
        let Some(RootCommand::Machine(_)) = cli.command else {
            panic!("machine {command} should parse");
        };
    }
}

#[test]
fn machine_help_uses_user_facing_descriptions() {
    let error = RootCli::try_parse_from(["nimbus", "machine", "--help"])
        .expect_err("help should short-circuit");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let rendered = error.to_string();
    assert!(rendered.contains("Usage:"));
    assert!(rendered.contains("Available Commands:"));
    assert!(rendered.contains("Examples:"));
    assert!(rendered.contains("nimbus machine init --now"));
    assert!(rendered.contains("nimbus machine status -f json"));
    assert!(rendered.contains("Initialize a new machine"));
    assert!(rendered.contains("Start a machine, creating it if needed"));
    assert!(rendered.contains("Stop a running machine"));
    assert!(rendered.contains("Display machine status"));
    assert!(rendered.contains("List initialized machines"));
    assert!(rendered.contains("Display machine host info"));
    assert!(rendered.contains("Inspect a machine record"));
    assert!(rendered.contains("Update a stopped machine"));
    assert!(rendered.contains("Securely copy files between the host and a machine"));
    assert!(rendered.contains("Log in to a machine using SSH"));
    assert!(rendered.contains("Remove an existing machine"));
    assert!(rendered.contains("Manage machine OS images"));
    assert!(!rendered.contains("Validate persisted machine state"));
    assert!(!rendered.contains("runtime roots"));
}

#[test]
fn machine_os_help_uses_user_facing_descriptions() {
    let error = RootCli::try_parse_from(["nimbus", "machine", "os", "--help"])
        .expect_err("help should short-circuit");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let rendered = error.to_string();
    assert!(rendered.contains("Use a specific machine OS image on the next boot"));
    assert!(rendered.contains("Switch to the supported machine OS image for this nimbus release"));
    assert!(!rendered.contains("supported image that matches this nimbus host version"));
}

#[test]
fn machine_init_help_uses_user_facing_flag_descriptions() {
    let error = RootCli::try_parse_from(["nimbus", "machine", "init", "--help"])
        .expect_err("help should short-circuit");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let rendered = error.to_string();
    assert!(rendered.contains("--now"));
    assert!(rendered.contains("--cpus"));
    assert!(rendered.contains("-c"));
    assert!(rendered.contains("--memory"));
    assert!(rendered.contains("-m"));
    assert!(rendered.contains("--disk-size"));
    assert!(rendered.contains("-d"));
    assert!(rendered.contains("--identity"));
    assert!(rendered.contains("--ignition-path"));
    assert!(rendered.contains("--firmware"));
    assert!(rendered.contains("--volume"));
    assert!(rendered.contains("-v"));
    assert!(rendered.contains("Number of CPUs"));
    assert!(rendered.contains("Memory in MiB"));
    assert!(rendered.contains("Disk size in GiB"));
    assert!(rendered.contains("Machine OS image"));
    assert!(rendered.contains("Path to SSH identity for guest access"));
    assert!(rendered.contains("Path to Ignition config file"));
    assert!(rendered.contains("Path to EFI variable store"));
    assert!(rendered.contains("Host:guest volume mount"));
    assert!(!rendered.contains("to record in the machine config"));
    assert!(!rendered.contains("future virtiofs setup"));
    assert!(!rendered.contains("bootstrap vsock channel"));
    assert!(!rendered.contains("--ssh-identity"));
    assert!(!rendered.contains("--ignition-file"));
    assert!(!rendered.contains("--efi-store"));
    assert!(!rendered.contains("--memory-mib"));
    assert!(!rendered.contains("--disk-gib"));
}

#[test]
fn machine_start_help_describes_create_if_missing_overrides() {
    let error = RootCli::try_parse_from(["nimbus", "machine", "start", "--help"])
        .expect_err("help should short-circuit");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let rendered = error.to_string();
    assert!(rendered.contains("Start a machine, creating it if needed"));
    assert!(rendered.contains("Number of CPUs to use if start creates the machine"));
    assert!(rendered.contains("Machine OS image to use if start creates the machine"));
    assert!(
        rendered.contains("Path to SSH identity for guest access if start creates the machine")
    );
    assert!(rendered.contains("--memory"));
    assert!(rendered.contains("--disk-size"));
    assert!(rendered.contains("--identity"));
    assert!(rendered.contains("--ignition-path"));
    assert!(rendered.contains("--firmware"));
    assert!(rendered.contains("--volume"));
    assert!(rendered.contains("--quiet"));
    assert!(rendered.contains("-q"));
    assert!(rendered.contains("--no-info"));
    assert!(rendered.contains("Examples:"));
    assert!(rendered.contains("nimbus machine start"));
    assert!(rendered.contains("nimbus machine start --quiet team-a"));
}

#[test]
fn machine_status_help_describes_output_formats() {
    let error = RootCli::try_parse_from(["nimbus", "machine", "status", "--help"])
        .expect_err("help should short-circuit");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let rendered = error.to_string();
    assert!(rendered.contains("--format"));
    assert!(rendered.contains("-f"));
    assert!(rendered.contains("--quiet"));
    assert!(rendered.contains("-q"));
    assert!(rendered.contains("--noheading"));
    assert!(rendered.contains("-n"));
    assert!(rendered.contains("json"));
    assert!(rendered.contains("yaml"));
    assert!(rendered.contains("table"));
    assert!(rendered.contains("[default: table]"));
}

#[test]
fn machine_list_parses_alias_formats_and_quiet_mode() {
    let cli = RootCli::parse_from(["nimbus", "machine", "list"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine list should parse");
    };
    match machine.command {
        MachineSubcommand::List(list) => {
            assert_eq!(list.format(), MachineListOutputFormat::Table);
            assert!(list.format.is_none());
            assert!(!list.quiet);
            assert!(!list.no_heading);
        }
        _ => panic!("expected list subcommand"),
    }

    let cli = RootCli::parse_from(["nimbus", "machine", "ls", "-f", "json", "--quiet", "-n"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine ls should parse");
    };
    match machine.command {
        MachineSubcommand::List(list) => {
            assert_eq!(list.format(), MachineListOutputFormat::Json);
            assert_eq!(list.format, Some(MachineListOutputFormat::Json));
            assert!(list.quiet);
            assert!(list.no_heading);
        }
        _ => panic!("expected list subcommand"),
    }
}

#[test]
fn machine_list_help_describes_formats_and_quiet_mode() {
    let error = RootCli::try_parse_from(["nimbus", "machine", "list", "--help"])
        .expect_err("help should short-circuit");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let rendered = error.to_string();
    assert!(rendered.contains("List initialized machines"));
    assert!(rendered.contains("--format"));
    assert!(rendered.contains("-f"));
    assert!(rendered.contains("json"));
    assert!(rendered.contains("table"));
    assert!(rendered.contains("--quiet"));
    assert!(rendered.contains("-q"));
    assert!(rendered.contains("--noheading"));
    assert!(rendered.contains("-n"));
}

#[test]
fn machine_info_defaults_to_yaml_and_accepts_short_format_flag() {
    let cli = RootCli::parse_from(["nimbus", "machine", "info"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine info should parse");
    };
    match machine.command {
        MachineSubcommand::Info(info) => {
            assert_eq!(info.format, MachineInfoOutputFormat::Yaml);
        }
        _ => panic!("expected info subcommand"),
    }

    let cli = RootCli::parse_from(["nimbus", "machine", "info", "-f", "json"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine info with json format should parse");
    };
    match machine.command {
        MachineSubcommand::Info(info) => {
            assert_eq!(info.format, MachineInfoOutputFormat::Json);
        }
        _ => panic!("expected info subcommand"),
    }
}

#[test]
fn machine_info_help_describes_structured_formats() {
    let error = RootCli::try_parse_from(["nimbus", "machine", "info", "--help"])
        .expect_err("help should short-circuit");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let rendered = error.to_string();
    assert!(rendered.contains("Display machine host info"));
    assert!(rendered.contains("--format"));
    assert!(rendered.contains("-f"));
    assert!(rendered.contains("json"));
    assert!(rendered.contains("yaml"));
    assert!(rendered.contains("[default: yaml]"));
    assert!(rendered.contains("nimbus machine info"));
}

#[test]
fn machine_inspect_defaults_to_json_and_accepts_yaml() {
    let cli = RootCli::parse_from(["nimbus", "machine", "inspect"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine inspect should parse");
    };
    match machine.command {
        MachineSubcommand::Inspect(inspect) => {
            assert_eq!(inspect.format, MachineInspectOutputFormat::Json);
            assert_eq!(inspect.name.as_deref(), None);
        }
        _ => panic!("expected inspect subcommand"),
    }

    let cli = RootCli::parse_from(["nimbus", "machine", "inspect", "-f", "yaml", "team-a"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine inspect with yaml should parse");
    };
    match machine.command {
        MachineSubcommand::Inspect(inspect) => {
            assert_eq!(inspect.format, MachineInspectOutputFormat::Yaml);
            assert_eq!(inspect.name.as_deref(), Some("team-a"));
        }
        _ => panic!("expected inspect subcommand"),
    }
}

#[test]
fn machine_inspect_help_describes_output_formats() {
    let error = RootCli::try_parse_from(["nimbus", "machine", "inspect", "--help"])
        .expect_err("help should short-circuit");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let rendered = error.to_string();
    assert!(rendered.contains("Inspect a machine record"));
    assert!(rendered.contains("--format"));
    assert!(rendered.contains("-f"));
    assert!(rendered.contains("json"));
    assert!(rendered.contains("yaml"));
    assert!(rendered.contains("[default: json]"));
}

#[test]
fn machine_set_help_describes_resource_flags() {
    let error = RootCli::try_parse_from(["nimbus", "machine", "set", "--help"])
        .expect_err("help should short-circuit");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let rendered = error.to_string();
    assert!(rendered.contains("Update a stopped machine"));
    assert!(rendered.contains("--cpus"));
    assert!(rendered.contains("--memory"));
    assert!(rendered.contains("--disk-size"));
    assert!(rendered.contains("Number of CPUs"));
    assert!(rendered.contains("Memory in MiB"));
    assert!(rendered.contains("Disk size in GiB"));
}

#[test]
fn machine_leaf_help_uses_shared_template_and_examples() {
    let cases = [
        (
            vec!["nimbus", "machine", "init", "--help"],
            "nimbus machine init --now",
        ),
        (
            vec!["nimbus", "machine", "stop", "--help"],
            "nimbus machine stop",
        ),
        (
            vec!["nimbus", "machine", "status", "--help"],
            "nimbus machine status -f json",
        ),
        (
            vec!["nimbus", "machine", "list", "--help"],
            "nimbus machine list -f json",
        ),
        (
            vec!["nimbus", "machine", "inspect", "--help"],
            "nimbus machine inspect -f yaml team-a",
        ),
        (
            vec!["nimbus", "machine", "set", "--help"],
            "nimbus machine set --cpus 4 --memory 4096",
        ),
        (
            vec!["nimbus", "machine", "cp", "--help"],
            "nimbus machine cp ./local.txt default:/tmp/remote.txt",
        ),
        (
            vec!["nimbus", "machine", "ssh", "--help"],
            "nimbus machine ssh team-a uname -a",
        ),
        (
            vec!["nimbus", "machine", "rm", "--help"],
            "nimbus machine rm team-a",
        ),
        (
            vec!["nimbus", "machine", "os", "apply", "--help"],
            "nimbus machine os apply docker://quay.io/podman/machine-os@sha256:<digest>",
        ),
        (
            vec!["nimbus", "machine", "os", "upgrade", "--help"],
            "nimbus machine os upgrade --dry-run",
        ),
    ];

    for (argv, example_snippet) in cases {
        let error = RootCli::try_parse_from(argv).expect_err("help should short-circuit");
        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        let rendered = error.to_string();
        assert!(rendered.contains("Usage:"), "{rendered}");
        assert!(rendered.contains("Examples:"), "{rendered}");
        assert!(rendered.contains(example_snippet), "{rendered}");
    }
}

#[test]
fn parses_machine_os_subcommands() {
    let cli = RootCli::parse_from([
        "nimbus",
        "machine",
        "os",
        "apply",
        "ghcr.io/nimbus/nimbus-machine-os:v9.9.9",
        "--restart",
    ]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine os apply should parse");
    };
    match machine.command {
        MachineSubcommand::Os(os) => match os.command {
            MachineOsSubcommand::Apply(apply) => {
                assert_eq!(apply.image, "ghcr.io/nimbus/nimbus-machine-os:v9.9.9");
                assert!(apply.restart);
            }
            _ => panic!("expected machine os apply subcommand"),
        },
        _ => panic!("expected machine os subcommand"),
    }

    let cli = RootCli::parse_from([
        "nimbus",
        "machine",
        "os",
        "upgrade",
        "--dry-run",
        "--restart",
    ]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine os upgrade should parse");
    };
    match machine.command {
        MachineSubcommand::Os(os) => match os.command {
            MachineOsSubcommand::Upgrade(upgrade) => {
                assert!(upgrade.dry_run);
                assert!(upgrade.restart);
            }
            _ => panic!("expected machine os upgrade subcommand"),
        },
        _ => panic!("expected machine os subcommand"),
    }
}

#[test]
fn parses_hidden_machine_api_subcommand() {
    let cli = RootCli::parse_from([
        "nimbus",
        "machine",
        "api",
        "--socket-path",
        "/tmp/nimbus.sock",
        "--control-data-dir",
        "/tmp/nimbus-control",
    ]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine api should parse");
    };

    match machine.command {
        MachineSubcommand::Api(api) => {
            assert_eq!(api.socket_path, Some(PathBuf::from("/tmp/nimbus.sock")));
            assert_eq!(
                api.control_data_dir,
                Some(PathBuf::from("/tmp/nimbus-control"))
            );
            assert!(!api.socket_activation);
        }
        _ => panic!("expected api subcommand"),
    }
}
