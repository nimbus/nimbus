use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde_json::json;
use tempfile::{NamedTempFile, TempDir};

use super::inspect::parse_inspect_output;
use super::{
    BuildahCli, OciExposedPort, OciExposedPortProtocol, OciImageConfig, OciImageLaunchDefaults,
};
use crate::backends::oci::command::CommandSpec;
use crate::spec::SandboxImageProcessOverrides;

fn fake_buildah_cli(script_path: PathBuf) -> BuildahCli {
    BuildahCli::new("/bin/sh").with_launcher_args([script_path.to_string_lossy().into_owned()])
}

#[test]
fn wrap_unshare_prefixes_existing_command() {
    let buildah = BuildahCli::new("buildah");
    let wrapped = buildah.wrap_unshare(
        &CommandSpec::new("/usr/libexec/neovex/crun")
            .arg("state")
            .arg("sandbox-123"),
    );

    assert_eq!(wrapped.program, PathBuf::from("buildah"));
    assert_eq!(
        wrapped.args,
        vec![
            "unshare",
            "--",
            "/usr/libexec/neovex/crun",
            "state",
            "sandbox-123",
        ]
    );
}

#[test]
fn maybe_wrap_is_identity_when_unshare_is_disabled() {
    let buildah = BuildahCli::new("buildah");
    let command = CommandSpec::new("/usr/bin/crun")
        .arg("state")
        .arg("sandbox-123");

    assert_eq!(buildah.maybe_wrap(command.clone()), command);
}

#[test]
fn inspect_command_requests_container_json_without_template_mode() {
    let buildah = BuildahCli::new("/usr/bin/buildah");
    let command = buildah.inspect_command("postgres-working");

    assert_eq!(command.program, PathBuf::from("/usr/bin/buildah"));
    assert_eq!(
        command.args,
        vec!["inspect", "--type", "container", "postgres-working"]
    );
}

#[test]
fn build_command_matches_expected_shape() {
    let buildah = BuildahCli::new("/usr/bin/buildah");
    let command = buildah.build_command(
        "neovex-test",
        Path::new("/workspace/Dockerfile"),
        Path::new("/workspace"),
    );

    assert_eq!(command.program, PathBuf::from("/usr/bin/buildah"));
    assert_eq!(
        command.args,
        vec![
            "bud",
            "-t",
            "neovex-test",
            "-f",
            "/workspace/Dockerfile",
            "/workspace",
        ]
    );
}

#[test]
fn inspect_payload_merges_oci_and_docker_image_config_fields() {
    let config = parse_inspect_output(sample_inspect_json().as_bytes())
        .expect("sample inspect output should parse");

    assert_eq!(
        config,
        OciImageConfig {
            entrypoint: vec!["/usr/local/bin/docker-entrypoint.sh".to_owned()],
            cmd: vec!["postgres".to_owned()],
            env: vec![
                "PATH=/usr/local/bin:/usr/bin".to_owned(),
                "POSTGRES_DB=postgres".to_owned(),
            ],
            working_dir: Some("/var/lib/postgresql".to_owned()),
            user: Some("999:999".to_owned()),
            exposed_ports: vec!["5432/tcp".to_owned()],
            volumes: vec!["/var/lib/postgresql/data".to_owned()],
            stop_signal: Some("SIGINT".to_owned()),
            healthcheck: Some(super::ImageHealthcheck {
                test: vec!["CMD-SHELL".to_owned(), "pg_isready -U postgres".to_owned()],
                interval: Some(10_000_000_000),
                timeout: Some(5_000_000_000),
                start_period: Some(30_000_000_000),
                retries: Some(3),
            }),
            labels: std::iter::once(("com.example.role".to_owned(), "primary".to_owned(),))
                .collect(),
        }
    );
}

#[test]
fn resolve_process_spec_uses_image_defaults() {
    let config = parse_inspect_output(sample_inspect_json().as_bytes())
        .expect("sample inspect output should parse");

    let process = config
        .resolve_process_spec(&SandboxImageProcessOverrides::default())
        .expect("image defaults should lower into a process spec");

    assert_eq!(
        process.args,
        vec![
            "/usr/local/bin/docker-entrypoint.sh".to_owned(),
            "postgres".to_owned(),
        ]
    );
    assert_eq!(
        process.env,
        vec![
            "PATH=/usr/local/bin:/usr/bin".to_owned(),
            "POSTGRES_DB=postgres".to_owned(),
        ]
    );
    assert_eq!(process.cwd, PathBuf::from("/var/lib/postgresql"));
    assert!(!process.terminal);
}

#[test]
fn resolve_process_spec_applies_overrides_with_env_precedence() {
    let config = parse_inspect_output(sample_inspect_json().as_bytes())
        .expect("sample inspect output should parse");

    let process = config
        .resolve_process_spec(&SandboxImageProcessOverrides {
            entrypoint: Some(vec!["/bin/sh".to_owned(), "-lc".to_owned()]),
            cmd: Some(vec!["exec custom-api".to_owned()]),
            env: vec!["POSTGRES_DB=app".to_owned(), "LOG_LEVEL=debug".to_owned()],
            cwd: Some(PathBuf::from("/workspace")),
            user: None,
            terminal: true,
        })
        .expect("overrides should lower into a process spec");

    assert_eq!(
        process.args,
        vec![
            "/bin/sh".to_owned(),
            "-lc".to_owned(),
            "exec custom-api".to_owned(),
        ]
    );
    assert_eq!(
        process.env,
        vec![
            "PATH=/usr/local/bin:/usr/bin".to_owned(),
            "POSTGRES_DB=app".to_owned(),
            "LOG_LEVEL=debug".to_owned(),
        ]
    );
    assert_eq!(process.cwd, PathBuf::from("/workspace"));
    assert!(process.terminal);
}

#[test]
fn resolve_process_spec_rejects_missing_launch_command() {
    let config = OciImageConfig {
        entrypoint: Vec::new(),
        cmd: Vec::new(),
        env: Vec::new(),
        working_dir: None,
        user: None,
        exposed_ports: Vec::new(),
        volumes: Vec::new(),
        stop_signal: None,
        healthcheck: None,
        labels: Default::default(),
    };

    let error = config
        .resolve_process_spec(&SandboxImageProcessOverrides::default())
        .expect_err("missing command should be rejected");
    assert!(
        error
            .to_string()
            .contains("did not provide a launch command"),
        "unexpected error: {error}"
    );
}

#[test]
fn exposed_port_bindings_parse_and_sort_image_ports() {
    let config = parse_inspect_output(sample_inspect_with_ports_json().as_bytes())
        .expect("sample inspect output should parse");

    let ports = config
        .exposed_port_bindings()
        .expect("exposed ports should parse");

    assert_eq!(
        ports,
        vec![
            OciExposedPort {
                port: 53,
                protocol: OciExposedPortProtocol::Udp,
                raw: "53/udp".to_owned(),
            },
            OciExposedPort {
                port: 5432,
                protocol: OciExposedPortProtocol::Tcp,
                raw: "5432/tcp".to_owned(),
            },
            OciExposedPort {
                port: 8080,
                protocol: OciExposedPortProtocol::Tcp,
                raw: "8080/tcp".to_owned(),
            },
        ]
    );
}

#[test]
fn exposed_port_bindings_reject_invalid_port_shape() {
    let config = OciImageConfig {
        entrypoint: vec!["/bin/service".to_owned()],
        cmd: Vec::new(),
        env: Vec::new(),
        working_dir: None,
        user: None,
        exposed_ports: vec!["not-a-port".to_owned()],
        volumes: Vec::new(),
        stop_signal: None,
        healthcheck: None,
        labels: Default::default(),
    };

    let error = config
        .exposed_port_bindings()
        .expect_err("invalid port shape should be rejected");
    assert!(
        error.to_string().contains("PORT/PROTOCOL"),
        "unexpected error: {error}"
    );
}

#[test]
fn resolve_launch_defaults_collects_rootfs_process_ports_and_metadata() {
    let config = parse_inspect_output(sample_inspect_with_ports_json().as_bytes())
        .expect("sample inspect output should parse");

    let defaults = config
        .resolve_launch_defaults(
            "/srv/rootfs",
            &SandboxImageProcessOverrides {
                cmd: Some(vec!["serve".to_owned()]),
                env: vec!["SERVICE_MODE=foreground".to_owned()],
                ..SandboxImageProcessOverrides::default()
            },
        )
        .expect("launch defaults should resolve");

    assert_eq!(
        defaults,
        OciImageLaunchDefaults {
            filesystem: crate::spec::SandboxFilesystemSpec::new("/srv/rootfs"),
            process: crate::spec::SandboxProcessSpec::new(["/usr/local/bin/service", "serve",])
                .with_env(["SERVICE_MODE=foreground"])
                .with_cwd("/"),
            exposed_ports: vec![
                OciExposedPort {
                    port: 53,
                    protocol: OciExposedPortProtocol::Udp,
                    raw: "53/udp".to_owned(),
                },
                OciExposedPort {
                    port: 5432,
                    protocol: OciExposedPortProtocol::Tcp,
                    raw: "5432/tcp".to_owned(),
                },
                OciExposedPort {
                    port: 8080,
                    protocol: OciExposedPortProtocol::Tcp,
                    raw: "8080/tcp".to_owned(),
                },
            ],
            user: Some("1000:1000".to_owned()),
            stop_signal: Some("SIGTERM".to_owned()),
            healthcheck: Some(super::ImageHealthcheck {
                test: vec![
                    "CMD-SHELL".to_owned(),
                    "curl -f http://localhost/health".to_owned()
                ],
                interval: Some(15_000_000_000),
                timeout: Some(3_000_000_000),
                start_period: Some(20_000_000_000),
                retries: Some(5),
            }),
            labels: std::iter::once(("com.example.service".to_owned(), "edge".to_owned(),))
                .collect(),
        }
    );
}

#[test]
fn pull_mount_inspect_and_cleanup_execute_expected_commands() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (script_path, log_path) = write_fake_buildah_script(&temp_dir);
    let buildah = fake_buildah_cli(script_path).with_unshare(true);

    let pulled = buildah
        .pull("postgres-working", "postgres:16")
        .expect("pull should succeed");
    assert_eq!(pulled.session_name, "postgres-working");
    assert_eq!(pulled.image_reference, "postgres:16");

    let mount_path = buildah
        .mount_rootfs_session("postgres-working")
        .expect("mount should succeed");
    assert_eq!(mount_path, PathBuf::from("/tmp/fake-rootfs"));

    let inspected = buildah
        .inspect_rootfs_session("postgres-working")
        .expect("inspect should succeed");
    assert_eq!(inspected.cmd, vec!["postgres"]);

    buildah
        .cleanup_rootfs_session("postgres-working")
        .expect("cleanup should succeed");

    let log = fs::read_to_string(log_path).expect("fake buildah log should be readable");
    let lines: Vec<_> = log.lines().collect();
    assert_eq!(lines[0], "from --name postgres-working postgres:16");
    assert!(
        lines[1].starts_with("unshare -- "),
        "mount should run inside buildah unshare when enabled"
    );
    assert!(lines[1].ends_with(" mount postgres-working"));
    assert_eq!(lines[2], "inspect --type container postgres-working");
    assert!(
        lines[3].starts_with("unshare -- "),
        "umount should run inside buildah unshare when enabled"
    );
    assert!(lines[3].ends_with(" umount postgres-working"));
    assert!(
        lines[4].starts_with("unshare -- "),
        "rm should run inside buildah unshare when enabled"
    );
    assert!(lines[4].ends_with(" rm postgres-working"));
}

#[test]
fn prepare_image_launch_combines_buildah_materialization_and_launch_defaults() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (script_path, log_path) = write_fake_buildah_script(&temp_dir);
    let buildah = fake_buildah_cli(script_path).with_unshare(true);

    let prepared = buildah
        .prepare_image_launch(
            "postgres-working",
            "postgres:16",
            &SandboxImageProcessOverrides {
                env: vec!["PGPORT=5432".to_owned()],
                cwd: Some(PathBuf::from("/workspace")),
                ..SandboxImageProcessOverrides::default()
            },
        )
        .expect("prepared launch should succeed");

    assert_eq!(prepared.mount_session.session_name, "postgres-working");
    assert_eq!(prepared.mount_session.image_reference, "postgres:16");
    assert_eq!(
        prepared.launch_defaults.filesystem.rootfs,
        PathBuf::from("/tmp/fake-rootfs")
    );
    assert_eq!(
        prepared.launch_defaults.process.args,
        vec![
            "/usr/local/bin/docker-entrypoint.sh".to_owned(),
            "postgres".to_owned()
        ]
    );
    assert_eq!(
        prepared.launch_defaults.process.env,
        vec![
            "PATH=/usr/local/bin:/usr/bin".to_owned(),
            "POSTGRES_DB=postgres".to_owned(),
            "PGPORT=5432".to_owned(),
        ]
    );
    assert_eq!(
        prepared.launch_defaults.process.cwd,
        PathBuf::from("/workspace")
    );
    assert_eq!(
        prepared.launch_defaults.exposed_ports,
        vec![OciExposedPort {
            port: 5432,
            protocol: OciExposedPortProtocol::Tcp,
            raw: "5432/tcp".to_owned(),
        }]
    );

    let log = fs::read_to_string(log_path).expect("fake buildah log should be readable");
    let lines: Vec<_> = log.lines().collect();
    assert_eq!(lines[0], "from --name postgres-working postgres:16");
    assert!(lines[1].ends_with(" mount postgres-working"));
    assert_eq!(lines[2], "inspect --type container postgres-working");
}

#[test]
fn prepare_image_launch_prefers_process_user_override_over_image_user() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (script_path, _log_path) = write_fake_buildah_script(&temp_dir);
    let buildah = fake_buildah_cli(script_path).with_unshare(true);

    let prepared = buildah
        .prepare_image_launch(
            "postgres-working",
            "postgres:16",
            &SandboxImageProcessOverrides::default().with_user("123:456"),
        )
        .expect("prepared launch should succeed");

    assert_eq!(
        prepared.launch_defaults.user,
        Some("123:456".to_owned()),
        "explicit process user override should win over the image USER"
    );
}

#[test]
fn build_materializes_localhost_image_before_creating_working_container() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (script_path, log_path) = write_fake_buildah_script(&temp_dir);
    let buildah = fake_buildah_cli(script_path);

    let built = buildah
        .build(
            "neovex-api",
            "api-working",
            Path::new("/workspace/Dockerfile"),
            Path::new("/workspace"),
        )
        .expect("build should succeed");
    assert_eq!(built.image_reference, "localhost/neovex-api");

    let log = fs::read_to_string(log_path).expect("fake buildah log should be readable");
    let lines: Vec<_> = log.lines().collect();
    assert_eq!(
        lines[0],
        "bud -t neovex-api -f /workspace/Dockerfile /workspace"
    );
    assert_eq!(lines[1], "from --name api-working localhost/neovex-api");
}

#[test]
fn prepare_built_image_launch_uses_built_image_reference() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (script_path, log_path) = write_fake_buildah_script(&temp_dir);
    let buildah = fake_buildah_cli(script_path);

    let prepared = buildah
        .prepare_built_image_launch(
            "neovex-api",
            "api-working",
            Path::new("/workspace/Dockerfile"),
            Path::new("/workspace"),
            &SandboxImageProcessOverrides::default(),
        )
        .expect("prepared built launch should succeed");

    assert_eq!(
        prepared.mount_session.image_reference,
        "localhost/neovex-api"
    );
    assert_eq!(
        prepared.launch_defaults.filesystem.rootfs,
        PathBuf::from("/tmp/fake-rootfs")
    );

    let log = fs::read_to_string(log_path).expect("fake buildah log should be readable");
    let lines: Vec<_> = log.lines().collect();
    assert_eq!(
        lines,
        vec![
            "bud -t neovex-api -f /workspace/Dockerfile /workspace",
            "from --name api-working localhost/neovex-api",
            "mount api-working",
            "inspect --type container api-working",
        ]
    );
}

fn write_fake_buildah_script(temp_dir: &TempDir) -> (PathBuf, PathBuf) {
    let script_path = temp_dir.path().join("fake-buildah");
    let log_path = temp_dir.path().join("buildah.log");
    let script = format!(
        r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "{log_path}"
cmd="${{1:-}}"
if [ -z "$cmd" ]; then
  echo "missing buildah subcommand" >&2
  exit 1
fi
shift

if [ "$cmd" = "unshare" ]; then
  if [ "${{1:-}}" != "--" ]; then
    echo "expected -- after buildah unshare" >&2
    exit 1
  fi
  shift
  wrapped_program="${{1:-}}"
  if [ -z "$wrapped_program" ]; then
    echo "missing wrapped program for buildah unshare" >&2
    exit 1
  fi
  shift
  if [ "${{1:-}}" = "$0" ]; then
    shift
  fi
  cmd="${{1:-}}"
  if [ -z "$cmd" ]; then
    printf 'missing subcommand for wrapped program %s\n' "$wrapped_program" >&2
    exit 1
  fi
  shift
fi

case "$cmd" in
  from|bud|umount|rm)
    exit 0
    ;;
  mount)
    printf '%s\n' "/tmp/fake-rootfs"
    exit 0
    ;;
  inspect)
    cat <<'JSON'
{inspect_json}
JSON
    exit 0
    ;;
  *)
    printf 'unexpected command: %s\n' "$cmd" >&2
    exit 1
    ;;
esac
"#,
        log_path = log_path.display(),
        inspect_json = sample_inspect_json()
    );

    let mut temp_script =
        NamedTempFile::new_in(temp_dir.path()).expect("temporary fake buildah file should exist");
    temp_script
        .write_all(script.as_bytes())
        .expect("fake buildah script should be written");
    temp_script
        .flush()
        .expect("fake buildah script should flush cleanly");
    let mut permissions = temp_script
        .as_file()
        .metadata()
        .expect("fake buildah temp script metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    temp_script
        .as_file()
        .set_permissions(permissions)
        .expect("fake buildah temp script should be executable");
    temp_script
        .as_file()
        .sync_all()
        .expect("fake buildah script should sync cleanly");
    temp_script
        .into_temp_path()
        .persist(&script_path)
        .expect("fake buildah script should persist cleanly");

    (script_path, log_path)
}

fn sample_inspect_json() -> String {
    json!([
        {
            "OCIv1": {
                "Config": {
                    "Entrypoint": ["/usr/local/bin/docker-entrypoint.sh"],
                    "Cmd": ["postgres"],
                    "Env": [
                        "PATH=/usr/local/bin:/usr/bin",
                        "POSTGRES_DB=postgres"
                    ],
                    "WorkingDir": "/var/lib/postgresql",
                    "User": "999:999",
                    "ExposedPorts": {
                        "5432/tcp": {}
                    },
                    "Volumes": {
                        "/var/lib/postgresql/data": {}
                    },
                    "StopSignal": "SIGINT",
                    "Labels": {
                        "com.example.role": "primary"
                    }
                }
            },
            "Docker": {
                "Config": {
                    "Healthcheck": {
                        "Test": ["CMD-SHELL", "pg_isready -U postgres"],
                        "Interval": 10000000000_u64,
                        "Timeout": 5000000000_u64,
                        "StartPeriod": 30000000000_u64,
                        "Retries": 3
                    }
                }
            }
        }
    ])
    .to_string()
}

fn sample_inspect_with_ports_json() -> String {
    json!([
        {
            "OCIv1": {
                "Config": {
                    "Entrypoint": ["/usr/local/bin/service"],
                    "User": "1000:1000",
                    "ExposedPorts": {
                        "8080/tcp": {},
                        "53/udp": {},
                        "5432/tcp": {}
                    },
                    "StopSignal": "SIGTERM",
                    "Labels": {
                        "com.example.service": "edge"
                    }
                }
            },
            "Docker": {
                "Config": {
                    "Healthcheck": {
                        "Test": ["CMD-SHELL", "curl -f http://localhost/health"],
                        "Interval": 15000000000_u64,
                        "Timeout": 3000000000_u64,
                        "StartPeriod": 20000000000_u64,
                        "Retries": 5
                    }
                }
            }
        }
    ])
    .to_string()
}
