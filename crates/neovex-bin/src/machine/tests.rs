use std::io::{Read, Write};
use std::os::unix::net::UnixListener as StdUnixListener;
use std::sync::{Mutex, OnceLock};

use super::*;
use crate::machine::manager::MachineHelperEnvGuard;
use clap::{Parser, error::ErrorKind};
use tempfile::TempDir;

#[derive(Debug, Parser)]
struct RootCli {
    #[command(subcommand)]
    command: Option<RootCommand>,
}

#[derive(Debug, Subcommand)]
enum RootCommand {
    Machine(MachineCommand),
}

fn expected_default_machine_image() -> String {
    if cfg!(target_os = "macos") {
        format!(
            "docker://{DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY}@{DEFAULT_PODMAN_MACHINE_IMAGE_DIGEST}"
        )
    } else {
        format!(
            "docker://{DEFAULT_NEOVEX_MACHINE_IMAGE_REPOSITORY}:{}",
            current_machine_release_tag()
        )
    }
}

fn machine_guest_binary_override_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_machine_guest_binary_override_env() -> std::sync::MutexGuard<'static, ()> {
    machine_guest_binary_override_env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct GuestBinaryOverrideEnvGuard {
    previous: Option<std::ffi::OsString>,
}

impl GuestBinaryOverrideEnvGuard {
    fn clear() -> Self {
        let previous = std::env::var_os("NEOVEX_MACHINE_GUEST_BINARY");
        unsafe { std::env::remove_var("NEOVEX_MACHINE_GUEST_BINARY") };
        Self { previous }
    }

    fn set(path: &Path) -> Self {
        let previous = std::env::var_os("NEOVEX_MACHINE_GUEST_BINARY");
        unsafe { std::env::set_var("NEOVEX_MACHINE_GUEST_BINARY", path) };
        Self { previous }
    }
}

impl Drop for GuestBinaryOverrideEnvGuard {
    fn drop(&mut self) {
        match self.previous.as_ref() {
            Some(value) => unsafe { std::env::set_var("NEOVEX_MACHINE_GUEST_BINARY", value) },
            None => unsafe { std::env::remove_var("NEOVEX_MACHINE_GUEST_BINARY") },
        }
    }
}

fn supported_stream_current_image_for_upgrade_test() -> String {
    if cfg!(target_os = "macos") {
        "docker://quay.io/podman/machine-os@sha256:abc123".to_owned()
    } else {
        "docker://ghcr.io/agentstation/neovex-machine-os:v0.1.0".to_owned()
    }
}

fn supported_stream_digest_image_for_upgrade_test() -> String {
    if cfg!(target_os = "macos") {
        format!("docker://{DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY}@sha256:abc123")
    } else {
        "docker://ghcr.io/agentstation/neovex-machine-os@sha256:abc123".to_owned()
    }
}

fn expected_upgrade_target_version() -> String {
    if cfg!(target_os = "macos") {
        DEFAULT_PODMAN_MACHINE_IMAGE_DIGEST.to_owned()
    } else {
        current_machine_release_tag()
    }
}

#[test]
fn parses_machine_init_defaults_to_version_pinned_release_image() {
    let cli = RootCli::parse_from(["neovex", "machine", "init"]);
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
        "neovex",
        "machine",
        "init",
        "--cpus",
        "4",
        "--memory",
        "4096",
        "--disk-size",
        "40",
        "--image",
        "docker://ghcr.io/agentstation/neovex-machine-os:test",
        "--identity",
        "/tmp/neovex-test-ed25519",
        "--ignition-path",
        "/tmp/neovex-test.ign",
        "--firmware",
        "/tmp/neovex-test.efi",
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
            assert_eq!(
                init.image,
                "docker://ghcr.io/agentstation/neovex-machine-os:test"
            );
            assert_eq!(
                init.ssh_identity,
                Some(PathBuf::from("/tmp/neovex-test-ed25519"))
            );
            assert_eq!(
                init.ignition_file,
                Some(PathBuf::from("/tmp/neovex-test.ign"))
            );
            assert_eq!(init.efi_store, Some(PathBuf::from("/tmp/neovex-test.efi")));
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
        "neovex",
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
fn machine_init_rejects_legacy_flag_names() {
    for legacy_flag in [
        "--ssh-identity",
        "--ignition-file",
        "--efi-store",
        "--memory-mib",
        "--disk-gib",
    ] {
        let error = RootCli::try_parse_from(["neovex", "machine", "init", legacy_flag, "value"])
            .expect_err("legacy flag should be rejected");
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
        let rendered = error.to_string();
        assert!(rendered.contains(legacy_flag));
        assert!(rendered.contains("unexpected argument"));
    }
}

#[test]
fn machine_init_parses_now_flag() {
    let cli = RootCli::parse_from(["neovex", "machine", "init", "--now"]);
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
        "neovex",
        "machine",
        "start",
        "-c",
        "4",
        "--memory",
        "4096",
        "--disk-size",
        "40",
        "--image",
        "docker://ghcr.io/agentstation/neovex-machine-os:test",
        "--identity",
        "/tmp/neovex-test-ed25519",
        "--ignition-path",
        "/tmp/neovex-test.ign",
        "--firmware",
        "/tmp/neovex-test.efi",
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
                Some("docker://ghcr.io/agentstation/neovex-machine-os:test".to_owned())
            );
            assert_eq!(
                start.ssh_identity,
                Some(PathBuf::from("/tmp/neovex-test-ed25519"))
            );
            assert_eq!(
                start.ignition_file,
                Some(PathBuf::from("/tmp/neovex-test.ign"))
            );
            assert_eq!(start.efi_store, Some(PathBuf::from("/tmp/neovex-test.efi")));
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
        "neovex",
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
    let cli = RootCli::parse_from(["neovex", "machine", "init", "team-a"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine init should parse");
    };
    match machine.command {
        MachineSubcommand::Init(init) => assert_eq!(init.name.as_deref(), Some("team-a")),
        _ => panic!("expected init subcommand"),
    }

    let cli = RootCli::parse_from(["neovex", "machine", "start", "team-a"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine start should parse");
    };
    match machine.command {
        MachineSubcommand::Start(start) => assert_eq!(start.name.as_deref(), Some("team-a")),
        _ => panic!("expected start subcommand"),
    }

    let cli = RootCli::parse_from(["neovex", "machine", "stop", "team-a"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine stop should parse");
    };
    match machine.command {
        MachineSubcommand::Stop(stop) => assert_eq!(stop.name.as_deref(), Some("team-a")),
        _ => panic!("expected stop subcommand"),
    }

    let cli = RootCli::parse_from(["neovex", "machine", "status", "team-a"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine status should parse");
    };
    match machine.command {
        MachineSubcommand::Status(status) => {
            assert_eq!(status.name.as_deref(), Some("team-a"))
        }
        _ => panic!("expected status subcommand"),
    }

    let cli = RootCli::parse_from(["neovex", "machine", "inspect", "team-a"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine inspect should parse");
    };
    match machine.command {
        MachineSubcommand::Inspect(inspect) => {
            assert_eq!(inspect.name.as_deref(), Some("team-a"))
        }
        _ => panic!("expected inspect subcommand"),
    }

    let cli = RootCli::parse_from(["neovex", "machine", "set", "--cpus", "4", "team-a"]);
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

    let cli = RootCli::parse_from(["neovex", "machine", "rm", "team-a"]);
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
    let cli = RootCli::parse_from(["neovex", "machine", "status"]);
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
        let cli = RootCli::parse_from(["neovex", "machine", "status", "-f", format_value]);
        let Some(RootCommand::Machine(machine)) = cli.command else {
            panic!("machine status should parse");
        };

        match machine.command {
            MachineSubcommand::Status(status) => assert_eq!(status.format, expected),
            _ => panic!("expected status subcommand"),
        }
    }

    let cli = RootCli::parse_from(["neovex", "machine", "status", "--noheading"]);
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
    let cli = RootCli::parse_from(["neovex", "machine", "status", "--quiet", "team-a"]);
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
        let cli = RootCli::parse_from(["neovex", "machine", command]);
        let Some(RootCommand::Machine(_)) = cli.command else {
            panic!("machine {command} should parse");
        };
    }
}

#[test]
fn machine_help_uses_user_facing_descriptions() {
    let error = RootCli::try_parse_from(["neovex", "machine", "--help"])
        .expect_err("help should short-circuit");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let rendered = error.to_string();
    assert!(rendered.contains("Usage:"));
    assert!(rendered.contains("Available Commands:"));
    assert!(rendered.contains("Examples:"));
    assert!(rendered.contains("neovex machine init --now"));
    assert!(rendered.contains("neovex machine status -f json"));
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
    let error = RootCli::try_parse_from(["neovex", "machine", "os", "--help"])
        .expect_err("help should short-circuit");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let rendered = error.to_string();
    assert!(rendered.contains("Use a specific machine OS image on the next boot"));
    assert!(rendered.contains("Switch to the supported machine OS image for this neovex release"));
    assert!(!rendered.contains("supported image that matches this neovex host version"));
}

#[test]
fn machine_init_help_uses_user_facing_flag_descriptions() {
    let error = RootCli::try_parse_from(["neovex", "machine", "init", "--help"])
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
    let error = RootCli::try_parse_from(["neovex", "machine", "start", "--help"])
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
    assert!(rendered.contains("neovex machine start"));
    assert!(rendered.contains("neovex machine start --quiet team-a"));
}

#[test]
fn machine_status_help_describes_output_formats() {
    let error = RootCli::try_parse_from(["neovex", "machine", "status", "--help"])
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
    let cli = RootCli::parse_from(["neovex", "machine", "list"]);
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

    let cli = RootCli::parse_from(["neovex", "machine", "ls", "-f", "json", "--quiet", "-n"]);
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
    let error = RootCli::try_parse_from(["neovex", "machine", "list", "--help"])
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
    let cli = RootCli::parse_from(["neovex", "machine", "info"]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine info should parse");
    };
    match machine.command {
        MachineSubcommand::Info(info) => {
            assert_eq!(info.format, MachineInfoOutputFormat::Yaml);
        }
        _ => panic!("expected info subcommand"),
    }

    let cli = RootCli::parse_from(["neovex", "machine", "info", "-f", "json"]);
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
    let error = RootCli::try_parse_from(["neovex", "machine", "info", "--help"])
        .expect_err("help should short-circuit");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let rendered = error.to_string();
    assert!(rendered.contains("Display machine host info"));
    assert!(rendered.contains("--format"));
    assert!(rendered.contains("-f"));
    assert!(rendered.contains("json"));
    assert!(rendered.contains("yaml"));
    assert!(rendered.contains("[default: yaml]"));
    assert!(rendered.contains("neovex machine info"));
}

#[test]
fn machine_inspect_defaults_to_json_and_accepts_yaml() {
    let cli = RootCli::parse_from(["neovex", "machine", "inspect"]);
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

    let cli = RootCli::parse_from(["neovex", "machine", "inspect", "-f", "yaml", "team-a"]);
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
    let error = RootCli::try_parse_from(["neovex", "machine", "inspect", "--help"])
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
fn machine_cp_parses_paths_and_quiet_mode() {
    let cli = RootCli::parse_from([
        "neovex",
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
    let error = RootCli::try_parse_from(["neovex", "machine", "cp", "--help"])
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
fn machine_set_help_describes_resource_flags() {
    let error = RootCli::try_parse_from(["neovex", "machine", "set", "--help"])
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
            vec!["neovex", "machine", "init", "--help"],
            "neovex machine init --now",
        ),
        (
            vec!["neovex", "machine", "stop", "--help"],
            "neovex machine stop",
        ),
        (
            vec!["neovex", "machine", "status", "--help"],
            "neovex machine status -f json",
        ),
        (
            vec!["neovex", "machine", "list", "--help"],
            "neovex machine list -f json",
        ),
        (
            vec!["neovex", "machine", "inspect", "--help"],
            "neovex machine inspect -f yaml team-a",
        ),
        (
            vec!["neovex", "machine", "set", "--help"],
            "neovex machine set --cpus 4 --memory 4096",
        ),
        (
            vec!["neovex", "machine", "cp", "--help"],
            "neovex machine cp ./local.txt default:/tmp/remote.txt",
        ),
        (
            vec!["neovex", "machine", "ssh", "--help"],
            "neovex machine ssh team-a uname -a",
        ),
        (
            vec!["neovex", "machine", "rm", "--help"],
            "neovex machine rm team-a",
        ),
        (
            vec!["neovex", "machine", "os", "apply", "--help"],
            "neovex machine os apply docker://quay.io/podman/machine-os@sha256:<digest>",
        ),
        (
            vec!["neovex", "machine", "os", "upgrade", "--help"],
            "neovex machine os upgrade --dry-run",
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
        "neovex",
        "machine",
        "os",
        "apply",
        "ghcr.io/agentstation/neovex-machine-os:v9.9.9",
        "--restart",
    ]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine os apply should parse");
    };
    match machine.command {
        MachineSubcommand::Os(os) => match os.command {
            MachineOsSubcommand::Apply(apply) => {
                assert_eq!(apply.image, "ghcr.io/agentstation/neovex-machine-os:v9.9.9");
                assert!(apply.restart);
            }
            _ => panic!("expected machine os apply subcommand"),
        },
        _ => panic!("expected machine os subcommand"),
    }

    let cli = RootCli::parse_from([
        "neovex",
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
fn parses_machine_ssh_with_guest_command() {
    let cli = RootCli::parse_from(["neovex", "machine", "ssh", "uname", "-a"]);
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
fn parses_hidden_machine_api_subcommand() {
    let cli = RootCli::parse_from([
        "neovex",
        "machine",
        "api",
        "--socket-path",
        "/tmp/neovex.sock",
        "--control-data-dir",
        "/tmp/neovex-control",
    ]);
    let Some(RootCommand::Machine(machine)) = cli.command else {
        panic!("machine api should parse");
    };

    match machine.command {
        MachineSubcommand::Api(api) => {
            assert_eq!(api.socket_path, Some(PathBuf::from("/tmp/neovex.sock")));
            assert_eq!(
                api.control_data_dir,
                Some(PathBuf::from("/tmp/neovex-control"))
            );
            assert!(!api.socket_activation);
        }
        _ => panic!("expected api subcommand"),
    }
}

#[test]
fn hidden_machine_api_subcommand_falls_back_without_home() {
    let original_home = std::env::var_os("HOME");
    // SAFETY: this test runs in the serialized machine lane and restores HOME before returning.
    unsafe { std::env::remove_var("HOME") };

    let roots = resolve_roots_for_command(&MachineCommand {
        command: MachineSubcommand::Api(MachineApiCommand {
            socket_path: Some(PathBuf::from("/tmp/neovex.sock")),
            socket_activation: false,
            control_data_dir: Some(PathBuf::from("/tmp/neovex-control")),
        }),
    })
    .expect("hidden machine api should fall back without HOME");

    if let Some(home) = original_home {
        // SAFETY: see comment above; restore process-local HOME for later tests.
        unsafe { std::env::set_var("HOME", home) };
    }

    assert_eq!(
        roots.config_root,
        PathBuf::from("/var/lib/neovex/machine/config")
    );
    assert_eq!(
        roots.state_root,
        PathBuf::from("/var/lib/neovex/machine/state")
    );
    assert_eq!(
        roots.data_root,
        PathBuf::from("/var/lib/neovex/machine/data")
    );
    assert_eq!(
        roots.cache_root,
        PathBuf::from("/var/lib/neovex/machine/cache")
    );
}

#[test]
fn machine_paths_use_short_runtime_root_and_typed_socket_layout() {
    let layout = MachineRootLayout::new(
        PathBuf::from("/tmp/config-root"),
        PathBuf::from("/tmp/state-root"),
        PathBuf::from("/tmp/neovex"),
    );
    let paths = layout.paths("default");

    assert_eq!(paths.runtime_dir, PathBuf::from("/tmp/neovex"));
    assert_eq!(
        paths.materialized_image_path,
        PathBuf::from("/tmp/data/default/images/default.raw")
    );
    assert_eq!(paths.image_cache_dir, PathBuf::from("/tmp/cache/images"));
    assert_eq!(
        paths.guest_binary_cache_dir,
        PathBuf::from("/tmp/cache/guest-neovex")
    );
    assert_eq!(
        paths.api_socket_path,
        PathBuf::from("/tmp/neovex/default-api.sock")
    );
    assert_eq!(
        paths.krunkit_log_path,
        PathBuf::from("/tmp/neovex/default-krunkit.log")
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
    assert!(rendered.contains("neovex machine stop team-a"));
}

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
    assert!(rendered.contains("Hint: run `neovex machine start` to boot the updated image"));
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
        rendered.contains("Hint: run `neovex machine os upgrade` to apply the supported image")
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
            guest_binary_cache_dir: PathBuf::from("/tmp/cache/guest-neovex"),
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
            guest_binary_cache_dir: PathBuf::from("/tmp/cache/guest-neovex"),
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

#[test]
fn machine_status_marks_missing_machine_api_socket_as_unreachable() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths(DEFAULT_MACHINE_NAME);

    let api = machine_api_status_view(&paths, None);

    assert_eq!(api.socket_path, paths.api_socket_path);
    assert_eq!(api.guest_socket_path, None);
    assert_eq!(api.transport, None);
    assert_eq!(api.forward_user, None);
    assert_eq!(api.identity_path, None);
    assert!(!api.exists);
    assert!(!api.reachable);
    assert!(api.capabilities.is_none());
    assert!(api.error.is_none());
}

#[test]
fn machine_status_renders_release_asset_guest_binary_contract() {
    let _env_lock = lock_machine_guest_binary_override_env();
    let _env_guard = GuestBinaryOverrideEnvGuard::clear();

    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths(DEFAULT_MACHINE_NAME);
    paths
        .ensure_directories()
        .expect("machine directories should exist");
    let config = MachineConfigRecord {
        version: CURRENT_MACHINE_CONFIG_VERSION,
        name: DEFAULT_MACHINE_NAME.to_owned(),
        provider: MachineProvider::Krunkit,
        guest: MachineGuestConfig {
            image_source: MachineImageSource::parse(&default_machine_image())
                .expect("default image should parse"),
            ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
            ssh_identity_path: Some(temp_dir.path().join("neovex-test-ed25519")),
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
    let desired = inspect_desired_guest_neovex_binary(&paths);
    fs::write(&desired.desired_path, b"release guest binary").expect("guest binary should write");

    let rendered = render_machine_status_view(
        MachineCommandResult::Status,
        &paths,
        Some(&config),
        Some(&MachineStateRecord::initialized()),
        MachineStatusOutputFormat::Yaml,
        false,
        false,
    )
    .expect("machine view should render");
    let desired = inspect_desired_guest_neovex_binary(&paths);

    if !cfg!(target_os = "macos") {
        assert!(rendered.contains("guest_binary_contract: null"));
        return;
    }

    assert!(rendered.contains("guest_binary_contract:"));
    assert!(rendered.contains("source: release-asset"));
    assert!(rendered.contains(&format!(
        "source_detail: GitHub release asset {}",
        current_machine_release_tag()
    )));
    assert!(rendered.contains(&format!(
        "desired_version: {}",
        current_machine_release_tag()
    )));
    assert!(rendered.contains(&format!("desired_path: {}", desired.desired_path.display())));
    assert!(rendered.contains("desired_exists: true"));
    assert!(rendered.contains(&format!(
            "desired_hash: {}",
            desired
                .desired_hash
                .as_deref()
                .expect("desired hash should exist for cached release asset")
        )));
}

#[test]
fn machine_status_renders_explicit_override_guest_binary_contract() {
    let _env_lock = lock_machine_guest_binary_override_env();

    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths(DEFAULT_MACHINE_NAME);
    paths
        .ensure_directories()
        .expect("machine directories should exist");
    let override_binary = temp_dir.path().join("override-neovex");
    fs::write(&override_binary, b"override guest binary").expect("override binary should write");
    let _env_guard = GuestBinaryOverrideEnvGuard::set(&override_binary);

    let config = MachineConfigRecord {
        version: CURRENT_MACHINE_CONFIG_VERSION,
        name: DEFAULT_MACHINE_NAME.to_owned(),
        provider: MachineProvider::Krunkit,
        guest: MachineGuestConfig {
            image_source: MachineImageSource::parse(&default_machine_image())
                .expect("default image should parse"),
            ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
            ssh_identity_path: Some(temp_dir.path().join("neovex-test-ed25519")),
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
        MachineStatusOutputFormat::Yaml,
        false,
        false,
    )
    .expect("machine view should render");
    let desired = inspect_desired_guest_neovex_binary(&paths);

    if !cfg!(target_os = "macos") {
        assert!(rendered.contains("guest_binary_contract: null"));
        return;
    }

    assert!(rendered.contains("guest_binary_contract:"));
    assert!(rendered.contains("source: explicit-override"));
    assert!(rendered.contains(&format!(
        "source_detail: $NEOVEX_MACHINE_GUEST_BINARY={}",
        override_binary.display()
    )));
    assert!(rendered.contains(&format!("desired_path: {}", override_binary.display())));
    assert!(rendered.contains("desired_exists: true"));
    assert!(rendered.contains(&format!(
            "desired_hash: {}",
            desired
                .desired_hash
                .as_deref()
                .expect("desired hash should exist for explicit override")
        )));
}

#[test]
fn machine_status_detects_reachable_machine_api_socket() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths(DEFAULT_MACHINE_NAME);

    std::fs::create_dir_all(
        paths
            .api_socket_path
            .parent()
            .expect("machine api socket should have a parent"),
    )
    .expect("socket parent should exist");
    let listener =
        StdUnixListener::bind(&paths.api_socket_path).expect("listener should bind cleanly");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("server should accept");
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = serde_json::json!({
            "status": "ok",
            "role": "guest-machine-api",
            "protocol_version": "v1alpha2",
            "listen_mode": "direct-socket",
            "control_data_dir": temp_dir.path().join("control").display().to_string(),
        })
        .to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("server should write response");

        let (mut stream, _) = listener
            .accept()
            .expect("server should accept capabilities");
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = serde_json::json!({
                "protocol_version": "v1alpha2",
                "service_execution_ready": false,
                "service_execution_mode": "standard_containers",
                "supported_service_backends": ["container"],
                "supported_operations": ["healthz", "capabilities"],
                "binary_statuses": [
                    {
                        "name": "buildah",
                        "present": true,
                        "resolved_path": "/usr/bin/buildah",
                        "required_for_operations": ["service-sandboxes.build-start"]
                    }
                ],
                "operation_statuses": [
                    {
                        "name": "service-sandboxes.build-start",
                        "available": false,
                        "blockers": ["guest machine API does not yet expose service lifecycle operations"]
                    }
                ],
                "service_execution_blockers": [
                    "guest machine API does not yet expose service lifecycle operations"
                ]
            })
            .to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("server should write capabilities response");
    });

    std::thread::sleep(std::time::Duration::from_millis(100));
    let api = machine_api_status_view(&paths, None);
    server
        .join()
        .expect("machine API server thread should join cleanly");

    assert_eq!(api.socket_path, paths.api_socket_path);
    assert_eq!(api.guest_socket_path, None);
    assert_eq!(api.transport, None);
    assert_eq!(api.forward_user, None);
    assert_eq!(api.identity_path, None);
    assert!(api.exists);
    assert!(api.reachable);
    assert_eq!(api.role.as_deref(), Some("guest-machine-api"));
    assert_eq!(api.protocol_version.as_deref(), Some("v1alpha2"));
    assert_eq!(api.listen_mode.as_deref(), Some("direct-socket"));
    assert_eq!(
        api.capabilities
            .as_ref()
            .map(|capabilities| capabilities.service_execution_mode),
        Some(protocol::MachineApiServiceExecutionMode::StandardContainers)
    );
    assert_eq!(
        api.capabilities
            .as_ref()
            .map(|capabilities| capabilities.supported_service_backends.clone()),
        Some(vec![neovex::SandboxBackendKind::Container])
    );
    assert!(api.error.is_none());
}

#[test]
fn machine_status_reports_forwarding_contract_when_machine_identity_exists() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths(DEFAULT_MACHINE_NAME);
    let config = MachineConfigRecord {
        version: CURRENT_MACHINE_CONFIG_VERSION,
        name: DEFAULT_MACHINE_NAME.to_owned(),
        provider: MachineProvider::Krunkit,
        guest: MachineGuestConfig {
            image_source: MachineImageSource::parse(&default_machine_image())
                .expect("default image should parse"),
            ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
            ssh_identity_path: Some(PathBuf::from("/tmp/neovex-test-ed25519")),
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

    let api = machine_api_status_view(&paths, Some(&config));

    assert_eq!(api.socket_path, paths.api_socket_path);
    assert_eq!(
        api.guest_socket_path,
        Some(PathBuf::from("/run/neovex/neovex.sock"))
    );
    assert_eq!(
        api.transport.as_deref(),
        Some("gvproxy-ssh-forwarded-unix-socket")
    );
    assert_eq!(api.forward_user.as_deref(), Some("root"));
    assert_eq!(
        api.identity_path,
        Some(PathBuf::from("/tmp/neovex-test-ed25519"))
    );
    assert!(!api.exists);
    assert!(!api.reachable);
    assert!(api.capabilities.is_none());
    assert!(api.error.is_none());
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
                image: "docker://127.0.0.1:1/example/neovex-machine-os:test".to_owned(),
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
                image: Some("docker://127.0.0.1:1/example/neovex-machine-os:test".to_owned()),
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
            reference: "docker://127.0.0.1:1/example/neovex-machine-os:test".to_owned(),
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
                image: Some("docker://127.0.0.1:1/example/neovex-machine-os:test".to_owned()),
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
                image: "docker://127.0.0.1:1/example/neovex-machine-os:test".to_owned(),
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
            "use `neovex machine set` to change CPU, memory, or disk for an existing machine"
        ),
        "unexpected error: {error}"
    );
    assert!(
        error.to_string().contains("Hint:"),
        "unexpected error: {error}"
    );
}

fn run_machine_command_for_test(
    command: MachineCommand,
    layout: &MachineRootLayout,
) -> Result<(), Error> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(run_machine_command_with_layout(command, layout))
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
