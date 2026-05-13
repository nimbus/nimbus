use super::*;

#[test]
fn cli_defaults_to_embedded_sqlite() {
    let cli = parse_start(["nimbus", "start"]);
    let config = persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    )
    .expect("default sqlite config should build");
    assert_eq!(
        config,
        nimbus::ServicePersistenceConfig::embedded("./data", nimbus::EmbeddedProviderKind::Sqlite)
    );
}

#[test]
fn start_command_default_has_no_auto_tenant() {
    let command = StartCommand::default();
    assert!(
        command.auto_tenant.is_none(),
        "start should not auto-create a tenant by default"
    );
}

#[test]
fn cli_requires_explicit_start_subcommand_for_server_flags() {
    assert!(Cli::try_parse_from(["nimbus"]).is_err());
    assert!(Cli::try_parse_from(["nimbus", "--compose-file", "./compose.dev.yaml"]).is_err());
}

#[test]
fn retired_serve_namespace_is_not_supported() {
    let error = Cli::try_parse_from(["nimbus", "serve", "--help"])
        .expect_err("retired serve namespace should not parse");
    assert_eq!(error.kind(), ErrorKind::InvalidSubcommand);
}

#[test]
fn cli_supports_top_level_version_flag() {
    let error = Cli::try_parse_from(["nimbus", "--version"])
        .expect_err("top-level version flag should short-circuit with display output");
    assert_eq!(error.kind(), ErrorKind::DisplayVersion);
    assert_eq!(
        error.to_string(),
        format!("nimbus {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn cli_help_describes_codegen_machine_and_compose_surface() {
    let error = Cli::try_parse_from(["nimbus", "--help"]).expect_err("help should short-circuit");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let rendered = error.to_string();
    assert!(rendered.contains(
        "Convex-compatible reactive backend with local development and Compose-backed services"
    ));
    assert!(rendered.contains("Usage:"));
    assert!(rendered.contains("Available Commands:"));
    assert!(rendered.contains("Examples:"));
    assert!(rendered.contains("nimbus start"));
    assert!(rendered.contains("nimbus dev"));
    assert!(rendered.contains("nimbus codegen --app ./demos/convex/html"));
    assert!(rendered.contains("nimbus token rotate"));
    assert!(rendered.contains("nimbus machine start"));
    assert!(rendered.contains("nimbus compose up"));
    assert!(rendered.contains("start"));
    assert!(rendered.contains("dev"));
    assert!(rendered.contains("codegen"));
    assert!(rendered.contains("token"));
    assert!(rendered.contains("machine     Manage local developer machines"));
    assert!(rendered.contains("compose"));
}

#[test]
fn cli_parses_start_command_with_optional_compose_file() {
    let cli = parse_start(["nimbus", "start", "--compose-file", "./compose.dev.yaml"]);
    assert_eq!(cli.compose_file, vec![PathBuf::from("./compose.dev.yaml")]);
}

#[test]
fn cli_parses_start_command_with_multiple_compose_files_in_order() {
    let cli = parse_start([
        "nimbus",
        "start",
        "--compose-file",
        "./compose.yaml",
        "--compose-file",
        "./compose.dev.yaml",
    ]);
    assert_eq!(
        cli.compose_file,
        vec![
            PathBuf::from("./compose.yaml"),
            PathBuf::from("./compose.dev.yaml")
        ]
    );
}

#[test]
fn cli_parses_start_command_with_app_dir() {
    let cli = parse_start(["nimbus", "start", "--app-dir", "./demos/convex/html"]);
    assert_eq!(cli.app_dir, Some(PathBuf::from("./demos/convex/html")));
}

#[test]
fn cli_defaults_start_host_to_loopback_and_accepts_explicit_host() {
    let default_cli = parse_start(["nimbus", "start"]);
    assert_eq!(default_cli.host, "127.0.0.1");

    let explicit_cli = parse_start(["nimbus", "start", "--host", "0.0.0.0"]);
    assert_eq!(explicit_cli.host, "0.0.0.0");
}

#[test]
fn cli_parses_start_command_with_skip_codegen() {
    let cli = parse_start([
        "nimbus",
        "start",
        "--app-dir",
        "./demos/convex/html",
        "--skip-codegen",
    ]);
    assert_eq!(cli.app_dir, Some(PathBuf::from("./demos/convex/html")));
    assert!(cli.skip_codegen);
}

#[test]
fn start_startup_summary_mentions_url_app_codegen_and_deploy_api() {
    let command = StartCommand {
        port: 0,
        app_dir: Some(PathBuf::from("./app")),
        skip_codegen: true,
        compose_file: vec![PathBuf::from("./compose.yaml")],
        deploy_admin_token: Some("dev-token".to_string()),
        ..StartCommand::default()
    };

    let lines = super::boot::start_startup_summary_lines(
        &command,
        Some(&super::boot::ResolvedStartAppDir::Explicit(PathBuf::from(
            "./app",
        ))),
        Some(
            &crate::compose::discovery::ResolvedComposeSelection::explicit(PathBuf::from(
                "./compose.yaml",
            )),
        ),
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 3210)),
        true,
    );

    assert!(
        lines
            .iter()
            .any(|line| line == "Nimbus server listening at http://localhost:3210/")
    );
    assert!(lines.iter().any(|line| line == "app dir: ./app"));
    assert!(
        lines
            .iter()
            .any(|line| line == "codegen preflight: skipped by --skip-codegen")
    );
    assert!(
        lines
            .iter()
            .any(|line| line == "compose file: ./compose.yaml")
    );
    assert!(lines.iter().any(|line| line == "deploy admin API: enabled"));
}

#[test]
fn start_startup_summary_reports_auto_discovered_override_companion() {
    let command = StartCommand::default();
    let selection = crate::compose::discovery::ResolvedComposeSelection {
        origin: crate::compose::discovery::ComposeSelectionOrigin::AutoDiscovered,
        project_root: PathBuf::from("/workspace"),
        files: vec![
            PathBuf::from("/workspace/compose.yaml"),
            PathBuf::from("/workspace/compose.override.yaml"),
        ],
        display_files: vec![
            PathBuf::from("/workspace/compose.yaml"),
            PathBuf::from("/workspace/compose.override.yaml"),
        ],
    };

    let lines = super::boot::start_startup_summary_lines(
        &command,
        None,
        Some(&selection),
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 3210)),
        false,
    );

    assert!(lines.iter().any(|line| {
        line == "compose file: auto-discovered /workspace/compose.yaml (+ compose.override.yaml)"
    }));
}

#[test]
fn start_startup_summary_reports_auto_detected_app_dir() {
    let command = StartCommand::default();
    let lines = super::boot::start_startup_summary_lines(
        &command,
        Some(&super::boot::ResolvedStartAppDir::AutoDetected(
            PathBuf::from("/workspace/functions"),
        )),
        None,
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 3210)),
        false,
    );

    assert!(
        lines
            .iter()
            .any(|line| line == "app dir: auto-detected /workspace/functions")
    );
}

#[test]
fn start_startup_summary_reports_compose_file_environment_selection() {
    let command = StartCommand::default();
    let selection = crate::compose::discovery::ResolvedComposeSelection {
        origin: crate::compose::discovery::ComposeSelectionOrigin::ExplicitEnvironment,
        project_root: PathBuf::from("/workspace"),
        files: vec![
            PathBuf::from("/workspace/compose.yaml"),
            PathBuf::from("/workspace/compose.dev.yaml"),
        ],
        display_files: vec![
            PathBuf::from("./compose.yaml"),
            PathBuf::from("./compose.dev.yaml"),
        ],
    };

    let lines = super::boot::start_startup_summary_lines(
        &command,
        None,
        Some(&selection),
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 3210)),
        false,
    );

    assert!(lines.iter().any(|line| {
        line == "compose file: COMPOSE_FILE=./compose.yaml (+ 1 extra Compose files)"
    }));
}

#[test]
fn start_compose_selection_discovers_from_current_dir_not_app_dir() {
    let temp = tempfile::tempdir().expect("tempdir should build");
    let project_root = temp.path().join("workspace");
    let nested_cwd = project_root.join("apps").join("web");
    let app_dir = temp.path().join("separate-app");
    fs::create_dir_all(&nested_cwd).expect("nested cwd should build");
    fs::create_dir_all(app_dir.join("convex")).expect("app dir should build");
    let compose_path = project_root.join("compose.yaml");
    fs::write(
        &compose_path,
        "name: demo\nservices:\n  db:\n    image: busybox:latest\n",
    )
    .expect("compose fixture should write");
    let command = StartCommand {
        app_dir: Some(app_dir),
        compose_file: Vec::new(),
        ..StartCommand::default()
    };

    let selection = with_current_dir(&nested_cwd, || {
        super::boot::resolve_optional_compose_selection(&command)
    })
    .expect("compose selection should resolve")
    .expect("compose selection should be discovered");

    assert_eq!(
        fs::canonicalize(selection.primary_file()).unwrap(),
        fs::canonicalize(&compose_path).unwrap()
    );
    assert_eq!(
        fs::canonicalize(&selection.project_root).unwrap(),
        fs::canonicalize(&project_root).unwrap()
    );
}

#[test]
fn start_compose_selection_prefers_explicit_flag_over_auto_discovery() {
    let temp = tempfile::tempdir().expect("tempdir should build");
    let nested_cwd = temp.path().join("apps").join("web");
    fs::create_dir_all(&nested_cwd).expect("nested cwd should build");
    fs::write(
        temp.path().join("compose.yaml"),
        "name: auto\nservices:\n  db:\n    image: busybox:latest\n",
    )
    .expect("auto compose fixture should write");
    let explicit_path = nested_cwd.join("compose.custom.yaml");
    fs::write(
        &explicit_path,
        "name: explicit\nservices:\n  db:\n    image: redis:7\n",
    )
    .expect("explicit compose fixture should write");
    let command = StartCommand {
        compose_file: vec![PathBuf::from("./compose.custom.yaml")],
        ..StartCommand::default()
    };

    let selection = with_current_dir(&nested_cwd, || {
        super::boot::resolve_optional_compose_selection(&command)
    })
    .expect("compose selection should resolve")
    .expect("compose selection should exist");

    assert_eq!(
        fs::canonicalize(selection.primary_file()).unwrap(),
        fs::canonicalize(&explicit_path).unwrap()
    );
    assert_eq!(selection.files.len(), 1);
}

#[test]
fn cli_parses_codegen_command_with_default_app_dir() {
    let cli = parse_codegen(["nimbus", "codegen"]);
    assert_eq!(cli.app, PathBuf::from("."));
    assert!(!cli.debug_node_apis);
}

#[test]
fn cli_parses_codegen_command_with_explicit_app_dir() {
    let cli = parse_codegen([
        "nimbus",
        "codegen",
        "--app",
        "./demos/convex/html",
        "--debug-node-apis",
    ]);
    assert_eq!(cli.app, PathBuf::from("./demos/convex/html"));
    assert!(cli.debug_node_apis);
}

#[test]
fn cli_rejects_removed_convex_app_dir_flag() {
    let error = Cli::try_parse_from(["nimbus", "start", "--convex-app-dir", "./demo"])
        .expect_err("removed app-dir flag should be removed");
    assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    assert!(error.to_string().contains("--convex-app-dir"));
}

#[test]
fn start_help_shows_app_dir_flag_name() {
    let error =
        Cli::try_parse_from(["nimbus", "start", "--help"]).expect_err("help should short-circuit");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let rendered = error.to_string();
    assert!(rendered.contains("--host"));
    assert!(rendered.contains("--app-dir"));
    assert!(rendered.contains("--skip-codegen"));
    assert!(rendered.contains("nimbus start --app-dir ./demos/convex/html"));
    assert!(rendered.contains("nimbus start --app-dir ./demos/convex/html --skip-codegen"));
    assert!(rendered.contains("COMPOSE_FILE"));
    assert!(!rendered.contains("--convex-app-dir"));
}
