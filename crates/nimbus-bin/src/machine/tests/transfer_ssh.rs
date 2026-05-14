use super::*;

#[test]
fn parses_machine_ssh_with_guest_command() {
    let cli = RootCli::parse_from(["nimbus", "machine", "ssh", "uname", "-a"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine ssh should parse");
    };

    match machine.command {
        MachineSubcommand::Ssh(ssh) => {
            assert_eq!(ssh.args, vec!["uname", "-a"]);
        }
        _ => panic!("expected ssh subcommand"),
    }
}

#[test]
fn machine_ssh_prefers_existing_machine_name_before_guest_command() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("team-a");
    paths.ensure_directories().expect("paths should exist");
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

    let ssh = MachineSshCommand {
        args: vec!["team-a".to_owned(), "uname".to_owned(), "-a".to_owned()],
    };

    let (machine_name, args) =
        resolve_machine_ssh_target(&ssh, &layout).expect("ssh target should resolve");

    assert_eq!(machine_name, "team-a");
    assert_eq!(args, vec!["uname", "-a"]);
}

#[test]
fn machine_ssh_treats_unknown_first_arg_as_guest_command() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let ssh = MachineSshCommand {
        args: vec!["uname".to_owned(), "-a".to_owned()],
    };

    let (machine_name, args) =
        resolve_machine_ssh_target(&ssh, &layout).expect("ssh target should resolve");

    assert_eq!(machine_name, DEFAULT_MACHINE_NAME);
    assert_eq!(args, vec!["uname", "-a"]);
}

#[test]
fn machine_cp_parses_paths_and_quiet_mode() {
    let cli = RootCli::parse_from([
        "nimbus",
        "machine",
        "cp",
        "--quiet",
        "./local.txt",
        "default:/tmp/remote.txt",
    ]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine cp should parse");
    };
    match machine.command {
        MachineSubcommand::Cp(copy) => {
            assert!(copy.quiet);
            assert_eq!(copy.src_path, "./local.txt");
            assert_eq!(copy.dest_path, "default:/tmp/remote.txt");
        }
        _ => panic!("expected cp subcommand"),
    }
}

#[test]
fn machine_cp_help_describes_machine_prefixed_paths() {
    let error = RootCli::try_parse_from(["nimbus", "machine", "cp", "--help"])
        .expect_err("help should short-circuit");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let rendered = error.to_string();
    assert!(rendered.contains("Securely copy files between the host and a machine"));
    assert!(rendered.contains("SRC_PATH"));
    assert!(rendered.contains("DEST_PATH"));
    assert!(rendered.contains("--quiet"));
    assert!(rendered.contains("-q"));
}

#[test]
fn machine_cp_transfer_resolves_host_to_guest() {
    let transfer = resolve_machine_cp_transfer("./local.txt", "team-a:/tmp/remote.txt")
        .expect("host to guest transfer should parse");

    assert_eq!(transfer.machine_name, "team-a");
    assert_eq!(transfer.machine_path, "/tmp/remote.txt");
    assert_eq!(transfer.host_path, "./local.txt");
    assert!(!transfer.guest_is_src);
}

#[test]
fn machine_cp_transfer_resolves_guest_to_host() {
    let transfer = resolve_machine_cp_transfer("team-a:/tmp/remote.txt", "./local.txt")
        .expect("guest to host transfer should parse");

    assert_eq!(transfer.machine_name, "team-a");
    assert_eq!(transfer.machine_path, "/tmp/remote.txt");
    assert_eq!(transfer.host_path, "./local.txt");
    assert!(transfer.guest_is_src);
}

#[test]
fn machine_cp_transfer_rejects_invalid_endpoint_combinations() {
    let error = resolve_machine_cp_transfer("./left", "./right")
        .expect_err("host to host transfer should fail");
    assert!(
        error
            .to_string()
            .contains("a machine name must prefix either the source path or destination path")
    );

    let error = resolve_machine_cp_transfer("one:/tmp/a", "two:/tmp/b")
        .expect_err("machine to machine transfer should fail");
    assert!(
        error
            .to_string()
            .contains("copying between two machines is unsupported")
    );
}

#[test]
fn machine_cp_treats_windows_drive_paths_as_host_paths() {
    assert_eq!(
        parse_machine_cp_endpoint(r"C:\temp\artifact.txt")
            .expect("windows path should parse as host"),
        MachineCpEndpoint::Host(r"C:\temp\artifact.txt".to_owned())
    );
}
